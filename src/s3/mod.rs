pub mod sigv4;

use chrono::{DateTime, Utc};
use quick_xml::Reader;
use quick_xml::events::Event;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue};
use std::fmt;
use std::net::Ipv4Addr;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use url::Url;

use crate::cli::style::Styler;
use crate::config::Config;
use crate::errors::QamError;

pub const DEFAULT_S3_REGION: &str = "us-east-1";
pub const DEFAULT_KEY_TEMPLATE: &str = "{prefix}{camera}/{date}/{camera}-{timestamp}.{ext}";
pub const MAX_SINGLE_PUT_BYTES: u64 = 5 * 1024 * 1024 * 1024; // 5 GiB

/// CLI flags for S3-compatible object storage upload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct S3Flags {
    pub enabled: Option<bool>,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub prefix: Option<String>,
    pub endpoint: Option<String>,
    pub region: Option<String>,
}

impl S3Flags {
    /// Returns `true` if an S3 upload was explicitly requested on the command line,
    /// or returns a `QamError::Usage` error on conflicting or dangling flags.
    pub fn upload_requested(&self) -> Result<bool, QamError> {
        let bucket_set = self.bucket.as_deref().is_some_and(|s| !s.trim().is_empty());

        if self.enabled == Some(false) && bucket_set {
            return Err(QamError::Usage(
                "conflicting flags: --s3=false and --s3-bucket".to_string(),
            ));
        }

        let requested = self.enabled == Some(true) || bucket_set;
        if !requested {
            for (name, val) in [
                ("--s3-key", &self.key),
                ("--s3-prefix", &self.prefix),
                ("--s3-endpoint", &self.endpoint),
                ("--s3-region", &self.region),
            ] {
                if val.is_some() {
                    return Err(QamError::Usage(format!(
                        "{name} has no effect without --s3 (or --s3-bucket)"
                    )));
                }
            }
            if self.bucket.is_some() {
                return Err(QamError::Usage(
                    "--s3-bucket requires a bucket name".to_string(),
                ));
            }
        }

        Ok(requested)
    }
}

/// Addressing style for S3 requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Addressing {
    #[default]
    Auto,
    Path,
    Virtual,
}

impl Addressing {
    pub fn as_str(&self) -> &'static str {
        match self {
            Addressing::Auto => "auto",
            Addressing::Path => "path",
            Addressing::Virtual => "virtual",
        }
    }
}

impl FromStr for Addressing {
    type Err = QamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Addressing::Auto),
            "path" => Ok(Addressing::Path),
            "virtual" => Ok(Addressing::Virtual),
            other => Err(QamError::Config(format!(
                "invalid S3 addressing {other:?} (use auto|path|virtual)"
            ))),
        }
    }
}

/// Resolved S3 configuration ready for signing and uploading.
#[derive(Clone, PartialEq, Eq)]
pub struct S3Settings {
    pub endpoint: Url,
    pub is_custom_endpoint: bool,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub key_template: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub addressing: Addressing,
    pub public_url: Option<String>,
    pub storage_class: Option<String>,
}

impl fmt::Debug for S3Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Settings")
            .field("endpoint", &self.endpoint.as_str())
            .field("is_custom_endpoint", &self.is_custom_endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("key_template", &self.key_template)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"***")
            .field("session_token", &self.session_token.as_ref().map(|_| "***"))
            .field("addressing", &self.addressing)
            .field("public_url", &self.public_url)
            .field("storage_class", &self.storage_class)
            .finish()
    }
}

/// Validates an S3 endpoint URL string.
pub fn validate_endpoint(raw: &str) -> Result<Url, QamError> {
    let trimmed = raw.trim().trim_end_matches('/');
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(QamError::S3(
            "endpoint must start with http:// or https://".to_string(),
        ));
    }
    let url = Url::parse(trimmed)
        .map_err(|e| QamError::S3(format!("invalid S3 endpoint URL {trimmed:?}: {e}")))?;
    if url.host_str().is_none() {
        return Err(QamError::S3(format!(
            "S3 endpoint URL {trimmed:?} is missing a host"
        )));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(QamError::S3(
            "S3 endpoint URL must not contain a query string or fragment".to_string(),
        ));
    }
    Ok(url)
}

fn first_non_empty(candidates: &[Option<&str>]) -> Option<String> {
    for s in candidates.iter().flatten() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn env_first(env: &dyn Fn(&str) -> Option<String>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(val) = env(k) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Resolves S3 settings with precedence: flag > QAM_S3_* env > AWS_* env > config > default.
pub fn resolve_s3_settings(
    flags: &S3Flags,
    cfg: &Config,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<S3Settings, QamError> {
    let bucket = first_non_empty(&[
        flags.bucket.as_deref(),
        env_first(env, &["QAM_S3_BUCKET"]).as_deref(),
        Some(cfg.s3.bucket.as_str()),
    ])
    .ok_or_else(|| {
        QamError::S3(
            "no S3 bucket specified: pass --s3-bucket <name> or configure one with 'qam s3 set --bucket <name>' (or QAM_S3_BUCKET)".to_string(),
        )
    })?;

    let region = first_non_empty(&[
        flags.region.as_deref(),
        env_first(env, &["QAM_S3_REGION", "AWS_REGION", "AWS_DEFAULT_REGION"]).as_deref(),
        Some(cfg.s3.region.as_str()),
    ])
    .unwrap_or_else(|| DEFAULT_S3_REGION.to_string());

    let custom_endpoint_str = first_non_empty(&[
        flags.endpoint.as_deref(),
        env_first(
            env,
            &["QAM_S3_ENDPOINT", "AWS_ENDPOINT_URL_S3", "AWS_ENDPOINT_URL"],
        )
        .as_deref(),
        Some(cfg.s3.endpoint.as_str()),
    ]);

    let is_custom_endpoint = custom_endpoint_str.is_some();
    let endpoint_str =
        custom_endpoint_str.unwrap_or_else(|| format!("https://s3.{region}.amazonaws.com"));
    let endpoint = validate_endpoint(&endpoint_str)?;

    let prefix = first_non_empty(&[
        flags.prefix.as_deref(),
        env_first(env, &["QAM_S3_PREFIX"]).as_deref(),
        Some(cfg.s3.prefix.as_str()),
    ])
    .unwrap_or_default();

    let key_template = first_non_empty(&[flags.key.as_deref(), Some(cfg.s3.key_template.as_str())])
        .unwrap_or_else(|| DEFAULT_KEY_TEMPLATE.to_string());

    let access_key_id = first_non_empty(&[
        env_first(env, &["QAM_S3_ACCESS_KEY_ID", "AWS_ACCESS_KEY_ID"]).as_deref(),
        Some(cfg.s3.access_key_id.as_str()),
    ]);
    let secret_access_key = first_non_empty(&[
        env_first(env, &["QAM_S3_SECRET_ACCESS_KEY", "AWS_SECRET_ACCESS_KEY"]).as_deref(),
        Some(cfg.s3.secret_access_key.as_str()),
    ]);

    let (access_key_id, secret_access_key) = match (access_key_id, secret_access_key) {
        (Some(ak), Some(sk)) => (ak, sk),
        _ => {
            return Err(QamError::S3(
                "S3 auth: no credentials found (set QAM_S3_ACCESS_KEY_ID/QAM_S3_SECRET_ACCESS_KEY or AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY, or run 'qam s3 set --access-key-id ... --secret-access-key ...')".to_string(),
            ));
        }
    };

    let session_token = first_non_empty(&[
        env_first(env, &["QAM_S3_SESSION_TOKEN", "AWS_SESSION_TOKEN"]).as_deref(),
        Some(cfg.s3.session_token.as_str()),
    ]);

    let addressing_str = first_non_empty(&[
        env_first(env, &["QAM_S3_ADDRESSING"]).as_deref(),
        Some(cfg.s3.addressing.as_str()),
    ])
    .unwrap_or_else(|| "auto".to_string());
    let addressing = Addressing::from_str(&addressing_str)?;

    let base_path = endpoint.path().trim_end_matches('/');
    if addressing == Addressing::Virtual && !base_path.is_empty() {
        return Err(QamError::S3(
            "virtual-hosted addressing cannot be used with an endpoint URL that has a path"
                .to_string(),
        ));
    }

    let public_url = first_non_empty(&[
        env_first(env, &["QAM_S3_PUBLIC_URL"]).as_deref(),
        Some(cfg.s3.public_url.as_str()),
    ])
    .map(|u| u.trim_end_matches('/').to_string());

    let storage_class = first_non_empty(&[Some(cfg.s3.storage_class.as_str())]);

    Ok(S3Settings {
        endpoint,
        is_custom_endpoint,
        region,
        bucket,
        prefix,
        key_template,
        access_key_id,
        secret_access_key,
        session_token,
        addressing,
        public_url,
        storage_class,
    })
}

fn is_dns_compatible_bucket(bucket: &str) -> bool {
    let len = bucket.len();
    if !(3..=63).contains(&len) {
        return false;
    }
    if bucket.contains("..") || bucket.starts_with('-') || bucket.ends_with('-') {
        return false;
    }
    if bucket.parse::<Ipv4Addr>().is_ok() {
        return false;
    }
    bucket
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

impl S3Settings {
    /// Resolves `Addressing::Auto` into `Addressing::Path` or `Addressing::Virtual`.
    pub fn effective_addressing(&self) -> Addressing {
        match self.addressing {
            Addressing::Path => Addressing::Path,
            Addressing::Virtual => Addressing::Virtual,
            Addressing::Auto => {
                let https = self.endpoint.scheme() == "https";
                if !self.is_custom_endpoint
                    && is_dns_compatible_bucket(&self.bucket)
                    && (!https || !self.bucket.contains('.'))
                {
                    Addressing::Virtual
                } else {
                    Addressing::Path
                }
            }
        }
    }

    /// Builds the request `Url`, the canonical `Host` header value, and the canonical URI.
    pub fn object_url(&self, key: &str) -> Result<(Url, String, String), QamError> {
        let clean_key = key.trim_start_matches('/');
        let encoded_key = sigv4::uri_encode(clean_key, false);

        let scheme = self.endpoint.scheme();
        let host = self
            .endpoint
            .host_str()
            .ok_or_else(|| QamError::S3("endpoint URL missing host".to_string()))?;
        let port_suffix = match self.endpoint.port() {
            Some(p) => format!(":{p}"),
            None => String::new(),
        };

        let (raw_url, canonical_uri) = match self.effective_addressing() {
            Addressing::Path | Addressing::Auto => {
                let base_path = self.endpoint.path().trim_end_matches('/');
                let encoded_bucket = sigv4::uri_encode(&self.bucket, true);
                let uri = format!("{base_path}/{encoded_bucket}/{encoded_key}");
                let raw = format!("{scheme}://{host}{port_suffix}{uri}");
                (raw, uri)
            }
            Addressing::Virtual => {
                let uri = format!("/{encoded_key}");
                let raw = format!("{scheme}://{}.{host}{port_suffix}{uri}", self.bucket);
                (raw, uri)
            }
        };

        let url = Url::parse(&raw_url)
            .map_err(|e| QamError::S3(format!("building S3 object URL: {e}")))?;

        let url_host = url
            .host_str()
            .ok_or_else(|| QamError::S3("object URL missing host".to_string()))?;
        let host_header = match url.port() {
            Some(p) => format!("{url_host}:{p}"),
            None => url_host.to_string(),
        };

        Ok((url, host_header, canonical_uri))
    }
}

/// Context for rendering an S3 object key template.
pub struct KeyContext<'a> {
    pub camera: &'a str,
    pub prefix: &'a str,
    pub local_path: &'a Path,
    pub now: DateTime<Utc>,
}

/// Sanitizes a camera name for use in an object key: keeps `[A-Za-z0-9._-]`, replaces everything else with `_`.
pub fn sanitize_camera_name(camera: &str) -> String {
    camera
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Renders an object key template using UTC timestamps.
pub fn render_object_key(template: &str, ctx: &KeyContext<'_>) -> Result<String, QamError> {
    let tmpl = if template.trim().is_empty() {
        DEFAULT_KEY_TEMPLATE
    } else {
        template.trim()
    };

    let camera_sanitized = sanitize_camera_name(ctx.camera);
    let date = ctx.now.format("%Y-%m-%d").to_string();
    let time = ctx.now.format("%H%M%S").to_string();
    let timestamp = ctx.now.format("%Y%m%dT%H%M%SZ").to_string();
    let filename = ctx
        .local_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = ctx
        .local_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let rendered = tmpl
        .replace("{prefix}", ctx.prefix)
        .replace("{camera}", &camera_sanitized)
        .replace("{date}", &date)
        .replace("{time}", &time)
        .replace("{timestamp}", &timestamp)
        .replace("{filename}", filename)
        .replace("{ext}", &ext);

    let key = rendered.trim_start_matches('/').to_string();
    if key.is_empty() {
        return Err(QamError::S3("rendered S3 object key is empty".to_string()));
    }
    if key.len() > 1024 {
        return Err(QamError::S3(format!(
            "rendered S3 object key exceeds 1024 bytes ({} bytes)",
            key.len()
        )));
    }

    Ok(key)
}

/// Result of a successful S3 object upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3UploadResult {
    pub bucket: String,
    pub key: String,
    pub uri: String,
    pub url: Option<String>,
    pub etag: Option<String>,
}

/// S3-compatible object storage uploader using pure-Rust SigV4 over `reqwest`.
#[derive(Clone, Debug)]
pub struct S3Uploader {
    pub settings: S3Settings,
    client: reqwest::Client,
}

impl S3Uploader {
    pub fn new(settings: S3Settings) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();

        Self { settings, client }
    }

    pub async fn put_object(&self, path: &Path, key: &str) -> Result<S3UploadResult, QamError> {
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|e| QamError::S3(format!("stat file: {e}")))?;
        if meta.len() > MAX_SINGLE_PUT_BYTES {
            return Err(QamError::S3(format!(
                "file {} ({} bytes) exceeds 5 GiB single-PUT S3 limit",
                path.display(),
                meta.len()
            )));
        }

        let body = tokio::fs::read(path)
            .await
            .map_err(|e| QamError::S3(format!("reading file: {e}")))?;
        let payload_sha256 = sigv4::sha256_hex(&body);
        let mime_type = crate::mime::mime_for_path(path);

        let clean_key = key.trim_start_matches('/');
        let (url, host_header, canonical_uri) = self.settings.object_url(clean_key)?;

        let mut last_err = None;
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            let now = Utc::now();
            let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
            let mut sign_headers: Vec<(&str, &str)> = vec![
                ("content-type", mime_type),
                ("host", &host_header),
                ("x-amz-content-sha256", &payload_sha256),
                ("x-amz-date", &amz_date),
            ];
            if let Some(ref tok) = self.settings.session_token {
                sign_headers.push(("x-amz-security-token", tok));
            }
            if let Some(ref sc) = self.settings.storage_class {
                sign_headers.push(("x-amz-storage-class", sc));
            }

            let signed = sigv4::sign_request(
                "PUT",
                &canonical_uri,
                &[],
                &sign_headers,
                &payload_sha256,
                &self.settings.access_key_id,
                &self.settings.secret_access_key,
                &self.settings.region,
                now,
            );
            tracing::debug!("s3 canonical request:\n{}", signed.canonical_request);

            let mut req_headers = HeaderMap::new();
            req_headers.insert(CONTENT_LENGTH, HeaderValue::from(body.len() as u64));
            req_headers.insert(CONTENT_TYPE, HeaderValue::from_static(mime_type));
            insert_header(&mut req_headers, "x-amz-date", &signed.amz_date)?;
            insert_header(&mut req_headers, "x-amz-content-sha256", &payload_sha256)?;
            insert_header(
                &mut req_headers,
                AUTHORIZATION.as_str(),
                &signed.authorization_header,
            )?;
            if let Some(ref tok) = self.settings.session_token {
                insert_header(&mut req_headers, "x-amz-security-token", tok)?;
            }
            if let Some(ref sc) = self.settings.storage_class {
                insert_header(&mut req_headers, "x-amz-storage-class", sc)?;
            }

            match self
                .client
                .put(url.clone())
                .headers(req_headers)
                .body(body.clone())
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let etag = resp
                            .headers()
                            .get("etag")
                            .and_then(|v| v.to_str().ok())
                            .map(|s| s.to_string());
                        let uri = format!("s3://{}/{clean_key}", self.settings.bucket);
                        let public_url = self.settings.public_url.as_ref().map(|base| {
                            format!(
                                "{}/{}",
                                base.trim_end_matches('/'),
                                sigv4::uri_encode(clean_key, false)
                            )
                        });
                        return Ok(S3UploadResult {
                            bucket: self.settings.bucket.clone(),
                            key: clean_key.to_string(),
                            uri,
                            url: public_url,
                            etag,
                        });
                    }

                    let bucket_region = resp
                        .headers()
                        .get("x-amz-bucket-region")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());
                    let resp_body = resp.text().await.unwrap_or_default();
                    let err = format_s3_error(
                        "PUT",
                        &self.settings.bucket,
                        clean_key,
                        status.as_u16(),
                        &resp_body,
                        bucket_region.as_deref(),
                    );
                    if is_retryable_status(status.as_u16()) && attempt == 0 {
                        last_err = Some(err);
                        continue;
                    }
                    return Err(err);
                }
                Err(e) => {
                    let err = QamError::S3(format!("S3 PUT network error: {e}"));
                    if attempt == 0 {
                        last_err = Some(err);
                        continue;
                    }
                    return Err(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| QamError::S3("S3 PUT failed".to_string())))
    }

    pub async fn head_object(&self, key: &str) -> Result<(), QamError> {
        self.simple_object_request("HEAD", key).await
    }

    pub async fn delete_object(&self, key: &str) -> Result<(), QamError> {
        self.simple_object_request("DELETE", key).await
    }

    async fn simple_object_request(&self, method: &str, key: &str) -> Result<(), QamError> {
        let clean_key = key.trim_start_matches('/');
        let (url, host_header, canonical_uri) = self.settings.object_url(clean_key)?;
        let empty_sha256 = sigv4::sha256_hex(b"");

        let mut last_err = None;
        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            let now = Utc::now();
            let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
            let mut sign_headers: Vec<(&str, &str)> = vec![
                ("host", &host_header),
                ("x-amz-content-sha256", &empty_sha256),
                ("x-amz-date", &amz_date),
            ];
            if let Some(ref tok) = self.settings.session_token {
                sign_headers.push(("x-amz-security-token", tok));
            }

            let signed = sigv4::sign_request(
                method,
                &canonical_uri,
                &[],
                &sign_headers,
                &empty_sha256,
                &self.settings.access_key_id,
                &self.settings.secret_access_key,
                &self.settings.region,
                now,
            );
            tracing::debug!("s3 canonical request:\n{}", signed.canonical_request);

            let mut req_headers = HeaderMap::new();
            insert_header(&mut req_headers, "x-amz-date", &signed.amz_date)?;
            insert_header(&mut req_headers, "x-amz-content-sha256", &empty_sha256)?;
            insert_header(
                &mut req_headers,
                AUTHORIZATION.as_str(),
                &signed.authorization_header,
            )?;
            if let Some(ref tok) = self.settings.session_token {
                insert_header(&mut req_headers, "x-amz-security-token", tok)?;
            }

            let req_method = reqwest::Method::from_bytes(method.as_bytes())
                .map_err(|e| QamError::S3(format!("invalid HTTP method {method}: {e}")))?;

            match self
                .client
                .request(req_method, url.clone())
                .headers(req_headers)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(());
                    }
                    let bucket_region = resp
                        .headers()
                        .get("x-amz-bucket-region")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());
                    let resp_body = resp.text().await.unwrap_or_default();
                    let err = format_s3_error(
                        method,
                        &self.settings.bucket,
                        clean_key,
                        status.as_u16(),
                        &resp_body,
                        bucket_region.as_deref(),
                    );
                    if is_retryable_status(status.as_u16()) && attempt == 0 {
                        last_err = Some(err);
                        continue;
                    }
                    return Err(err);
                }
                Err(e) => {
                    let err = QamError::S3(format!("S3 {method} network error: {e}"));
                    if attempt == 0 {
                        last_err = Some(err);
                        continue;
                    }
                    return Err(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| QamError::S3(format!("S3 {method} failed"))))
    }
}

fn insert_header(headers: &mut HeaderMap, name: &str, value: &str) -> Result<(), QamError> {
    let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| QamError::S3(format!("invalid header name {name}: {e}")))?;
    let header_val = HeaderValue::from_str(value)
        .map_err(|e| QamError::S3(format!("invalid header value for {name}: {e}")))?;
    headers.insert(header_name, header_val);
    Ok(())
}

fn is_retryable_status(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}

#[derive(Default)]
struct S3XmlError {
    code: String,
    message: String,
}

fn parse_s3_xml_error(body: &str) -> Option<S3XmlError> {
    if !body.trim_start().starts_with('<') {
        return None;
    }
    let mut reader = Reader::from_str(body);
    let mut current_tag = String::new();
    let mut out = S3XmlError::default();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                current_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
            }
            Ok(Event::End(_)) => {
                current_tag.clear();
            }
            Ok(Event::Text(e)) => {
                if let Ok(text) = e.unescape() {
                    match current_tag.as_str() {
                        "Code" => out.code.push_str(&text),
                        "Message" => out.message.push_str(&text),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    if out.code.is_empty() && out.message.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn format_s3_error(
    method: &str,
    bucket: &str,
    key: &str,
    status: u16,
    body: &str,
    bucket_region: Option<&str>,
) -> QamError {
    let parsed = parse_s3_xml_error(body);
    let code = parsed.as_ref().map(|p| p.code.as_str()).unwrap_or("");
    let message = parsed.as_ref().map(|p| p.message.as_str()).unwrap_or("");

    let mut msg = format!("S3 {method} s3://{bucket}/{key} failed: HTTP {status}");
    if !code.is_empty() {
        msg.push(' ');
        msg.push_str(code);
    }
    if !message.is_empty() {
        msg.push_str(": ");
        msg.push_str(message);
    }

    let hint = match (code, status) {
        ("SignatureDoesNotMatch" | "InvalidAccessKeyId", _) | (_, 403) => Some(
            "auth failed: check access key/secret, region, and addressing (try `addressing: path`)"
                .to_string(),
        ),
        ("RequestTimeTooSkewed", _) => Some("system clock is off; sync time (NTP)".to_string()),
        ("NoSuchBucket", _) | (_, 404) => Some("bucket not found".to_string()),
        ("AuthorizationHeaderMalformed" | "PermanentRedirect", _) | (_, 301 | 307) => {
            let mut h = "wrong region or endpoint".to_string();
            if let Some(r) = bucket_region {
                h.push_str(&format!(" (bucket region: {r})"));
            }
            Some(h)
        }
        _ => None,
    };

    if let Some(h) = hint {
        msg.push_str("\nHints: ");
        msg.push_str(&h);
    }

    QamError::S3(msg)
}

/// Uploads `file_path` to S3 and prints progress matching `maybe_upload_to_slack`.
pub async fn maybe_upload_to_s3(
    file_path: &Path,
    settings: &S3Settings,
    key: &str,
    sty: &Styler,
) -> Result<S3UploadResult, QamError> {
    let blue_arrow = sty.accent("→");
    let green_check = sty.ok("✔");

    let clean_key = key.trim_start_matches('/');
    println!(
        "{} uploading {} to s3://{}/{}…",
        blue_arrow,
        file_path.display(),
        settings.bucket,
        clean_key
    );

    let uploader = S3Uploader::new(settings.clone());
    let res = uploader.put_object(file_path, clean_key).await?;

    if let Some(ref url) = res.url {
        println!("{} S3 upload complete! {} ({})", green_check, res.uri, url);
    } else {
        println!("{} S3 upload complete! {}", green_check, res.uri);
    }

    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn fake_env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn test_s3_flags_upload_requested() {
        let empty = S3Flags::default();
        assert!(!empty.upload_requested().unwrap());

        let just_s3 = S3Flags {
            enabled: Some(true),
            ..Default::default()
        };
        assert!(just_s3.upload_requested().unwrap());

        let just_bucket = S3Flags {
            bucket: Some("b".into()),
            ..Default::default()
        };
        assert!(just_bucket.upload_requested().unwrap());

        let disabled = S3Flags {
            enabled: Some(false),
            ..Default::default()
        };
        assert!(!disabled.upload_requested().unwrap());

        let conflict = S3Flags {
            enabled: Some(false),
            bucket: Some("b".into()),
            ..Default::default()
        };
        assert!(conflict.upload_requested().is_err());

        for bad in [
            S3Flags {
                key: Some("k".into()),
                ..Default::default()
            },
            S3Flags {
                prefix: Some("p/".into()),
                ..Default::default()
            },
            S3Flags {
                endpoint: Some("http://127.0.0.1:9000".into()),
                ..Default::default()
            },
            S3Flags {
                region: Some("auto".into()),
                ..Default::default()
            },
            S3Flags {
                bucket: Some("".into()),
                ..Default::default()
            },
        ] {
            assert!(
                bad.upload_requested().is_err(),
                "expected error for {bad:?}"
            );
        }
    }

    #[test]
    fn test_resolve_s3_settings_precedence_and_redaction() {
        let mut cfg = Config::default();
        cfg.s3.bucket = "cfg-bucket".into();
        cfg.s3.region = "cfg-region".into();
        cfg.s3.endpoint = "https://cfg.example.com".into();
        cfg.s3.access_key_id = "CFG_AK".into();
        cfg.s3.secret_access_key = "CFG_SUPER_SECRET".into();
        cfg.s3.session_token = "CFG_TOK".into();

        // AWS_* overridden by QAM_S3_*, overridden by flags
        let env = fake_env(&[
            ("AWS_REGION", "aws-region"),
            ("QAM_S3_REGION", "qam-region"),
            ("AWS_ENDPOINT_URL", "https://aws-ep.example.com"),
            ("AWS_ENDPOINT_URL_S3", "https://aws-s3-ep.example.com"),
            ("QAM_S3_ENDPOINT", "https://qam-ep.example.com"),
            ("AWS_ACCESS_KEY_ID", "AWS_AK"),
            ("QAM_S3_ACCESS_KEY_ID", "QAM_AK"),
            ("AWS_SECRET_ACCESS_KEY", "AWS_SUPER_SECRET"),
            ("QAM_S3_SECRET_ACCESS_KEY", ""), // whitespace/empty skipped!
        ]);

        let flags = S3Flags {
            enabled: Some(true),
            bucket: Some("flag-bucket".into()),
            region: Some("flag-region".into()),
            ..Default::default()
        };

        let s = resolve_s3_settings(&flags, &cfg, &env).unwrap();
        assert_eq!(s.bucket, "flag-bucket");
        assert_eq!(s.region, "flag-region");
        assert_eq!(s.endpoint.as_str(), "https://qam-ep.example.com/");
        assert_eq!(s.access_key_id, "QAM_AK");
        // Empty QAM_S3_SECRET_ACCESS_KEY skipped -> falls back to AWS_SECRET_ACCESS_KEY
        assert_eq!(s.secret_access_key, "AWS_SUPER_SECRET");
        assert_eq!(s.session_token.as_deref(), Some("CFG_TOK"));

        let dbg = format!("{s:?}");
        assert!(!dbg.contains("AWS_SUPER_SECRET"));
        assert!(!dbg.contains("CFG_TOK"));
        assert!(dbg.contains("***"));
    }

    #[test]
    fn test_addressing_and_host_header() {
        let mut base = S3Settings {
            endpoint: Url::parse("https://s3.us-east-1.amazonaws.com").unwrap(),
            is_custom_endpoint: false,
            region: "us-east-1".into(),
            bucket: "my-bucket".into(),
            prefix: String::new(),
            key_template: DEFAULT_KEY_TEMPLATE.into(),
            access_key_id: "AK".into(),
            secret_access_key: "SK".into(),
            session_token: None,
            addressing: Addressing::Auto,
            public_url: None,
            storage_class: None,
        };

        assert_eq!(base.effective_addressing(), Addressing::Virtual);
        let (url, host, uri) = base.object_url("cam/shot.jpg").unwrap();
        assert_eq!(host, "my-bucket.s3.us-east-1.amazonaws.com");
        assert_eq!(uri, "/cam/shot.jpg");
        assert_eq!(url.path(), uri);

        // Dotted bucket over https -> Path
        base.bucket = "my.bucket".into();
        assert_eq!(base.effective_addressing(), Addressing::Path);

        // Custom endpoint -> Path, non-default port preserved in Host
        base.bucket = "my-bucket".into();
        base.is_custom_endpoint = true;
        base.endpoint = Url::parse("http://127.0.0.1:9000").unwrap();
        assert_eq!(base.effective_addressing(), Addressing::Path);
        let (url, host, uri) = base.object_url("a b/shot.jpg").unwrap();
        assert_eq!(host, "127.0.0.1:9000");
        assert_eq!(uri, "/my-bucket/a%20b/shot.jpg");
        assert_eq!(url.path(), uri);

        // Default port 443 omitted in Host
        base.endpoint = Url::parse("https://s3.example.com:443").unwrap();
        let (_, host, _) = base.object_url("shot.jpg").unwrap();
        assert_eq!(host, "s3.example.com");
    }

    #[test]
    fn test_virtual_addressing_with_endpoint_path_rejected() {
        let mut cfg = Config::default();
        cfg.s3.bucket = "b".into();
        cfg.s3.endpoint = "https://gw.example.com/s3".into();
        cfg.s3.addressing = "virtual".into();
        cfg.s3.access_key_id = "AK".into();
        cfg.s3.secret_access_key = "SK".into();
        let err = resolve_s3_settings(&S3Flags::default(), &cfg, &|_| None).unwrap_err();
        assert!(err.to_string().contains("virtual-hosted"));
    }

    #[test]
    fn test_render_object_key() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 7, 57, 46).unwrap();
        let p = Path::new("/tmp/shot.JPG");

        let default_key = render_object_key(
            DEFAULT_KEY_TEMPLATE,
            &KeyContext {
                camera: "Front Door",
                prefix: "qam/",
                local_path: p,
                now,
            },
        )
        .unwrap();
        assert_eq!(
            default_key,
            "qam/Front_Door/2026-11-17/Front_Door-20261117T075746Z.jpg"
        );

        let literal = render_object_key(
            "/custom/literal.jpg",
            &KeyContext {
                camera: "cam",
                prefix: "ignored/",
                local_path: p,
                now,
            },
        )
        .unwrap();
        assert_eq!(literal, "custom/literal.jpg");

        assert!(
            render_object_key(
                "/",
                &KeyContext {
                    camera: "cam",
                    prefix: "",
                    local_path: p,
                    now,
                }
            )
            .is_err()
        );
    }
}
