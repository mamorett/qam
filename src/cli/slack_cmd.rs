use std::path::Path;

use crate::config;
use crate::errors::QamError;
use crate::slack::{SlackUploader, resolve_slack_channel, resolve_slack_token};

pub fn execute_set(token: &str, channel: &str, config_path: Option<&str>) -> Result<(), QamError> {
    let path = config::resolve_load_path(config_path.unwrap_or(""))?;
    let mut cfg = config::load(config_path.unwrap_or(""))?;

    let t = token.trim();
    if !t.is_empty() {
        cfg.slack.token = t.to_string();
    }
    let c = channel.trim();
    if !c.is_empty() {
        cfg.slack.default_channel = c.to_string();
    }

    config::save_to_path(&path, &cfg)?;
    println!("Slack configuration updated in {}", path.display());
    Ok(())
}

pub async fn execute_test(
    file_path: &Path,
    channel_flag: &str,
    message_flag: &str,
    config_path: Option<&str>,
) -> Result<(), QamError> {
    let cfg = config::load(config_path.unwrap_or(""))?;

    let env_token = std::env::var("SLACK_TOKEN").ok();
    let token = resolve_slack_token("", &cfg, env_token.as_deref());

    let channel = if !channel_flag.trim().is_empty() {
        channel_flag.trim().to_string()
    } else {
        resolve_slack_channel("", &cfg)
    };

    if token.is_empty() {
        return Err(QamError::Slack(
            "no Slack token found (use SLACK_TOKEN env var or 'camsnap slack set --token')"
                .to_string(),
        ));
    }
    if channel.is_empty() {
        return Err(QamError::Slack(
            "no Slack channel specified (use --channel or 'camsnap slack set --channel')"
                .to_string(),
        ));
    }

    let message = if message_flag.trim().is_empty() {
        "Test upload from camsnap"
    } else {
        message_flag.trim()
    };

    let uploader = SlackUploader::new(&token);
    let ch_id = uploader
        .resolve_channel(&channel)
        .await
        .map_err(|e| QamError::Slack(format!("resolving channel: {e}")))?;

    println!(
        "Uploading {} to {} ({})…",
        file_path.display(),
        channel,
        ch_id
    );

    let resp = uploader.upload_file(file_path, &ch_id, message).await?;

    if !resp.files.is_empty() {
        println!("Success! File ID: {}", resp.files[0].id);
    } else {
        println!("Success!");
    }

    Ok(())
}
