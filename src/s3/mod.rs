pub mod sigv4;

use chrono::{DateTime, Utc};
use std::fmt;
use std::net::Ipv4Addr;
use std::path::Path;
use std::str::FromStr;
use url::Url;

use crate::config::Config;
use crate::errors::QamError;

pub const DEFAULT_S3_REGION: &str = "us-east-1";
pub const DEFAULT_KEY_TEMPLATE: &str = "{prefix}{camera}/{date}/{camera}-{timestamp}.{ext}";

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
