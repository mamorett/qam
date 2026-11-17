use chrono::Utc;
use std::path::Path;
use std::str::FromStr;

use crate::config;
use crate::errors::QamError;
use crate::s3::{
    Addressing, KeyContext, S3Flags, S3Uploader, render_object_key, resolve_s3_settings,
    validate_endpoint,
};

#[derive(Debug, Default)]
pub struct S3SetArgs {
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub key_template: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
    pub addressing: Option<String>,
    pub public_url: Option<String>,
    pub storage_class: Option<String>,
}

pub fn execute_set(args: S3SetArgs, config_path: Option<&str>) -> Result<(), QamError> {
    let path = config::resolve_load_path(config_path.unwrap_or(""))?;
    let mut cfg = config::load(config_path.unwrap_or(""))?;

    if let Some(ep) = args.endpoint {
        let trimmed = ep.trim();
        if !trimmed.is_empty() {
            validate_endpoint(trimmed)?;
            cfg.s3.endpoint = trimmed.trim_end_matches('/').to_string();
        }
    }
    if let Some(r) = args.region {
        let trimmed = r.trim();
        if !trimmed.is_empty() {
            cfg.s3.region = trimmed.to_string();
        }
    }
    if let Some(b) = args.bucket {
        let trimmed = b.trim();
        if !trimmed.is_empty() {
            cfg.s3.bucket = trimmed.to_string();
        }
    }
    if let Some(p) = args.prefix {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            cfg.s3.prefix = trimmed.to_string();
        }
    }
    if let Some(kt) = args.key_template {
        let trimmed = kt.trim();
        if !trimmed.is_empty() {
            cfg.s3.key_template = trimmed.to_string();
        }
    }
    if let Some(ak) = args.access_key_id {
        let trimmed = ak.trim();
        if !trimmed.is_empty() {
            cfg.s3.access_key_id = trimmed.to_string();
        }
    }
    if let Some(sk) = args.secret_access_key {
        let trimmed = sk.trim();
        if !trimmed.is_empty() {
            cfg.s3.secret_access_key = trimmed.to_string();
        }
    }
    if let Some(st) = args.session_token {
        let trimmed = st.trim();
        if !trimmed.is_empty() {
            cfg.s3.session_token = trimmed.to_string();
        }
    }
    if let Some(addr) = args.addressing {
        let trimmed = addr.trim();
        if !trimmed.is_empty() {
            let parsed = Addressing::from_str(trimmed)?;
            cfg.s3.addressing = parsed.as_str().to_string();
        }
    }
    if let Some(pub_url) = args.public_url {
        let trimmed = pub_url.trim();
        if !trimmed.is_empty() {
            cfg.s3.public_url = trimmed.trim_end_matches('/').to_string();
        }
    }
    if let Some(sc) = args.storage_class {
        let trimmed = sc.trim();
        if !trimmed.is_empty() {
            cfg.s3.storage_class = trimmed.to_string();
        }
    }

    config::save_to_path(&path, &cfg)?;
    println!("S3 configuration updated in {}", path.display());
    println!("Uploads happen only when you pass --s3 (or --s3-bucket) to snap/clip/audio.");
    Ok(())
}

pub async fn execute_test(
    file_path: &Path,
    bucket: Option<String>,
    key: Option<String>,
    endpoint: Option<String>,
    region: Option<String>,
    config_path: Option<&str>,
) -> Result<(), QamError> {
    let cfg = config::load(config_path.unwrap_or(""))?;
    let flags = S3Flags {
        enabled: Some(true),
        bucket,
        key,
        prefix: None,
        endpoint,
        region,
    };

    let settings = resolve_s3_settings(&flags, &cfg, &|k| std::env::var(k).ok())?;
    let rendered_key = render_object_key(
        &settings.key_template,
        &KeyContext {
            camera: "test",
            prefix: &settings.prefix,
            local_path: file_path,
            now: Utc::now(),
        },
    )?;

    println!(
        "Uploading {} to s3://{}/{}…",
        file_path.display(),
        settings.bucket,
        rendered_key
    );

    let uploader = S3Uploader::new(settings);
    let res = uploader.put_object(file_path, &rendered_key).await?;
    uploader.head_object(&rendered_key).await?;

    if let Some(ref url) = res.url {
        println!("Success! {} ({})", res.uri, url);
    } else {
        println!("Success! {}", res.uri);
    }

    Ok(())
}
