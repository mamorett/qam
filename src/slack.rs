use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

use crate::cli::style::Styler;
use crate::config::Config;
use crate::errors::QamError;

pub const DEFAULT_SLACK_API_BASE: &str = "https://slack.com/api";

fn url_encode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[derive(Debug, Deserialize)]
pub struct UploadUrlResponse {
    pub ok: bool,
    #[serde(default)]
    pub upload_url: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UploadedFile {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct CompleteUploadResponse {
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub files: Vec<UploadedFile>,
}

#[derive(Debug, Deserialize)]
pub struct ChannelEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct ResponseMetadata {
    #[serde(default)]
    pub next_cursor: String,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsListResponse {
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub channels: Vec<ChannelEntry>,
    #[serde(default)]
    pub response_metadata: ResponseMetadata,
}

#[derive(Debug, Deserialize, Default)]
pub struct MemberProfile {
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct MemberEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub profile: MemberProfile,
}

#[derive(Debug, Deserialize)]
pub struct UsersListResponse {
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub members: Vec<MemberEntry>,
    #[serde(default)]
    pub response_metadata: ResponseMetadata,
}

#[derive(Debug, Deserialize)]
pub struct OpenChannel {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct ConversationsOpenResponse {
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub channel: Option<OpenChannel>,
}

#[derive(Clone, Debug)]
pub struct SlackUploader {
    pub token: String,
    pub api_base: String,
    client: reqwest::Client,
}

impl SlackUploader {
    pub fn new(token: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_default();

        Self {
            token: token.into(),
            api_base: DEFAULT_SLACK_API_BASE.to_string(),
            client,
        }
    }

    pub fn with_api_base(mut self, base: impl Into<String>) -> Self {
        self.api_base = base.into();
        self
    }

    fn api_url(&self, path: &str) -> String {
        let base = self.api_base.trim_end_matches('/');
        let path = path.trim_start_matches('/');
        format!("{base}/{path}")
    }

    fn auth_headers(&self) -> Result<HeaderMap, QamError> {
        let mut headers = HeaderMap::new();
        let val = format!("Bearer {}", self.token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&val)
                .map_err(|e| QamError::Slack(format!("invalid auth header: {e}")))?,
        );
        Ok(headers)
    }

    /// Resolves channel ID from channel name, hash channel `#name`, ID, `@user`, or username.
    pub async fn resolve_channel(&self, name_or_id: &str) -> Result<String, QamError> {
        let trimmed = name_or_id.trim();
        if let Some(user_name) = trimmed.strip_prefix('@') {
            let user_id = self.resolve_user_id(user_name).await?;
            return self.open_dm(&user_id).await;
        }

        let clean_id = trimmed.strip_prefix('#').unwrap_or(trimmed);

        if clean_id.len() > 1
            && clean_id
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
        {
            if clean_id.starts_with('U') {
                return self.open_dm(clean_id).await;
            }
            return Ok(clean_id.to_string());
        }

        if is_slack_id(clean_id) {
            if clean_id.starts_with('U') {
                return self.open_dm(clean_id).await;
            }
            return Ok(clean_id.to_string());
        }

        // Paginate conversations.list
        let mut cursor = String::new();
        loop {
            let mut url = format!(
                "{}?limit=200&exclude_archived=true&types=public_channel,private_channel",
                self.api_url("conversations.list")
            );
            if !cursor.is_empty() {
                url.push_str("&cursor=");
                url.push_str(&url_encode(&cursor));
            }

            let resp = self
                .client
                .get(&url)
                .headers(self.auth_headers()?)
                .send()
                .await
                .map_err(|e| QamError::Slack(format!("conversations.list network error: {e}")))?;

            let list: ConversationsListResponse = resp
                .json()
                .await
                .map_err(|e| QamError::Slack(format!("parsing conversations.list JSON: {e}")))?;

            if !list.ok {
                return Err(QamError::Slack(format!("slack API error: {}", list.error)));
            }

            for ch in list.channels {
                if ch.name == clean_id {
                    return Ok(ch.id);
                }
            }

            cursor = list.response_metadata.next_cursor;
            if cursor.is_empty() {
                break;
            }
        }

        // If not found in channels, try users
        if let Ok(user_id) = self.resolve_user_id(clean_id).await {
            return self.open_dm(&user_id).await;
        }

        Err(QamError::Slack(format!(
            "channel or user {name_or_id:?} not found (make sure the bot is a member)"
        )))
    }

    /// Resolves a user ID from username or display name.
    pub async fn resolve_user_id(&self, name: &str) -> Result<String, QamError> {
        let name = name.trim();
        if is_slack_id(name) && name.starts_with('U') {
            return Ok(name.to_string());
        }

        let mut cursor = String::new();
        loop {
            let mut url = format!("{}?limit=200", self.api_url("users.list"));
            if !cursor.is_empty() {
                url.push_str("&cursor=");
                url.push_str(&url_encode(&cursor));
            }

            let resp = self
                .client
                .get(&url)
                .headers(self.auth_headers()?)
                .send()
                .await
                .map_err(|e| QamError::Slack(format!("users.list network error: {e}")))?;

            let list: UsersListResponse = resp
                .json()
                .await
                .map_err(|e| QamError::Slack(format!("parsing users.list JSON: {e}")))?;

            if !list.ok {
                return Err(QamError::Slack(format!("slack API error: {}", list.error)));
            }

            for member in list.members {
                if member.name == name || member.profile.display_name == name {
                    return Ok(member.id);
                }
            }

            cursor = list.response_metadata.next_cursor;
            if cursor.is_empty() {
                break;
            }
        }

        Err(QamError::Slack(format!("user {name:?} not found")))
    }

    /// Opens a DM channel for the given user ID.
    pub async fn open_dm(&self, user_id: &str) -> Result<String, QamError> {
        #[derive(Serialize)]
        struct OpenReq<'a> {
            users: &'a str,
        }

        let url = self.api_url("conversations.open");
        let resp = self
            .client
            .post(&url)
            .headers(self.auth_headers()?)
            .json(&OpenReq { users: user_id })
            .send()
            .await
            .map_err(|e| QamError::Slack(format!("conversations.open network error: {e}")))?;

        let open_resp: ConversationsOpenResponse = resp
            .json()
            .await
            .map_err(|e| QamError::Slack(format!("parsing conversations.open JSON: {e}")))?;

        if !open_resp.ok {
            return Err(QamError::Slack(format!(
                "slack API error: {}",
                open_resp.error
            )));
        }

        let ch = open_resp
            .channel
            .ok_or_else(|| QamError::Slack("missing channel in conversations.open".to_string()))?;
        Ok(ch.id)
    }

    /// Performs the 3-step Slack file upload.
    pub async fn upload_file(
        &self,
        file_path: &Path,
        channel_id: &str,
        initial_comment: &str,
    ) -> Result<CompleteUploadResponse, QamError> {
        let meta = tokio::fs::metadata(file_path)
            .await
            .map_err(|e| QamError::Slack(format!("stat file: {e}")))?;
        let file_size = meta.len();
        let file_name = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();

        let mime_type = mime_for_path(file_path);

        // Step 1: getUploadURLExternal
        let upload_url_resp = self
            .get_upload_url(&file_name, file_size)
            .await
            .map_err(|e| QamError::Slack(format!("getting upload URL (step 1/3): {e}")))?;

        // Step 2: raw POST to upload_url
        let file_bytes = tokio::fs::read(file_path)
            .await
            .map_err(|e| QamError::Slack(format!("reading file: {e}")))?;

        self.upload_content(&upload_url_resp.upload_url, file_bytes, mime_type)
            .await
            .map_err(|e| QamError::Slack(format!("uploading file content (step 2/3): {e}")))?;

        // Step 3: completeUploadExternal
        let complete_resp = self
            .complete_upload(&upload_url_resp.file_id, channel_id, initial_comment)
            .await
            .map_err(|e| QamError::Slack(format!("completing upload (step 3/3): {e}")))?;

        Ok(complete_resp)
    }

    async fn get_upload_url(
        &self,
        file_name: &str,
        file_size: u64,
    ) -> Result<UploadUrlResponse, QamError> {
        let url = format!(
            "{}?filename={}&length={}",
            self.api_url("files.getUploadURLExternal"),
            url_encode(file_name),
            file_size
        );

        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers()?)
            .send()
            .await
            .map_err(|e| QamError::Slack(format!("network error: {e}")))?;

        let result: UploadUrlResponse = resp
            .json()
            .await
            .map_err(|e| QamError::Slack(format!("JSON error: {e}")))?;

        if !result.ok {
            return Err(QamError::Slack(format!(
                "slack API error: {}",
                result.error
            )));
        }

        Ok(result)
    }

    async fn upload_content(
        &self,
        upload_url: &str,
        body: Vec<u8>,
        mime_type: &'static str,
    ) -> Result<(), QamError> {
        let len = body.len() as u64;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from(len));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(mime_type));

        let resp = self
            .client
            .post(upload_url)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| QamError::Slack(format!("network error: {e}")))?;

        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let body_text = resp.text().await.unwrap_or_default();
            Err(QamError::Slack(format!(
                "upload failed with status {}: {}",
                status.as_u16(),
                body_text
            )))
        }
    }

    async fn complete_upload(
        &self,
        file_id: &str,
        channel_id: &str,
        initial_comment: &str,
    ) -> Result<CompleteUploadResponse, QamError> {
        #[derive(Serialize)]
        struct FileEntry<'a> {
            id: &'a str,
        }

        #[derive(Serialize)]
        struct CompleteReq<'a> {
            files: Vec<FileEntry<'a>>,
            #[serde(skip_serializing_if = "str::is_empty")]
            channel_id: &'a str,
            #[serde(skip_serializing_if = "str::is_empty")]
            initial_comment: &'a str,
        }

        let req_body = CompleteReq {
            files: vec![FileEntry { id: file_id }],
            channel_id,
            initial_comment,
        };

        let url = self.api_url("files.completeUploadExternal");
        let resp = self
            .client
            .post(&url)
            .headers(self.auth_headers()?)
            .json(&req_body)
            .send()
            .await
            .map_err(|e| QamError::Slack(format!("network error: {e}")))?;

        let result: CompleteUploadResponse = resp
            .json()
            .await
            .map_err(|e| QamError::Slack(format!("JSON error: {e}")))?;

        if !result.ok {
            return Err(QamError::Slack(format!(
                "slack API error: {}",
                result.error
            )));
        }

        Ok(result)
    }
}

pub fn is_slack_id(s: &str) -> bool {
    if s.len() < 9 || s.len() > 11 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

pub fn mime_for_path(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "aac" => "audio/aac",
        _ => "application/octet-stream",
    }
}

pub fn resolve_slack_token(flag_token: &str, cfg: &Config, env_token: Option<&str>) -> String {
    let t = flag_token.trim();
    if !t.is_empty() {
        return t.to_string();
    }
    if let Some(env) = env_token {
        let env_trimmed = env.trim();
        if !env_trimmed.is_empty() {
            return env_trimmed.to_string();
        }
    }
    cfg.slack.token.clone()
}

pub fn resolve_slack_channel(flag_channel: &str, cfg: &Config) -> String {
    let c = flag_channel.trim();
    if !c.is_empty() {
        return c.to_string();
    }
    cfg.slack.default_channel.clone()
}

/// Helper called after capture to conditionally upload to Slack.
pub async fn maybe_upload_to_slack(
    file_path: &Path,
    token: &str,
    channel_arg: &str,
    comment: &str,
    sty: &Styler,
) -> Result<(), QamError> {
    if channel_arg.is_empty() {
        return Ok(());
    }
    if token.is_empty() {
        return Err(QamError::Slack(
            "slack-channel specified but no token found (use --slack-token or SLACK_TOKEN environment variable)".to_string(),
        ));
    }

    let blue_arrow = sty.accent("→");
    let green_check = sty.ok("✔");

    println!(
        "{} uploading {} to Slack ({})…",
        blue_arrow,
        file_path.display(),
        channel_arg
    );

    let uploader = SlackUploader::new(token);
    let channel_id = uploader
        .resolve_channel(channel_arg)
        .await
        .map_err(|e| QamError::Slack(format!("resolving slack channel: {e}")))?;

    let resp = uploader
        .upload_file(file_path, &channel_id, comment)
        .await
        .map_err(|e| QamError::Slack(format!("uploading to slack: {e}")))?;

    if !resp.files.is_empty() {
        println!(
            "{} Slack upload complete! File ID: {}",
            green_check, resp.files[0].id
        );
    } else {
        println!("{} Slack upload complete!", green_check);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_slack_id() {
        assert!(is_slack_id("C01234567"));
        assert!(is_slack_id("U01234567"));
        assert!(is_slack_id("D01234567"));
        assert!(is_slack_id("G01234567"));
        assert!(is_slack_id("W01234567"));
        assert!(!is_slack_id("c01234567")); // lower case
        assert!(!is_slack_id("short"));
        assert!(!is_slack_id("too_long_id_here"));
        assert!(!is_slack_id("invalid!@#"));
    }

    #[test]
    fn test_resolve_slack_token() {
        let mut cfg = Config::default();
        cfg.slack.token = "config-token".into();

        // 1. Flag precedence
        assert_eq!(
            resolve_slack_token("flag-token", &cfg, Some("env-token")),
            "flag-token"
        );

        // 2. Env var precedence
        assert_eq!(
            resolve_slack_token("", &cfg, Some("env-token")),
            "env-token"
        );

        // 3. Config fallback
        assert_eq!(resolve_slack_token("", &cfg, None), "config-token");
    }

    #[test]
    fn test_resolve_slack_channel() {
        let mut cfg = Config::default();
        cfg.slack.default_channel = "config-channel".into();

        // 1. Flag precedence
        assert_eq!(resolve_slack_channel("flag-channel", &cfg), "flag-channel");

        // 2. Config fallback
        assert_eq!(resolve_slack_channel("", &cfg), "config-channel");
    }
}
