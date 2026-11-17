use std::path::Path;

use crate::cli::style::Styler;
use crate::config::Config;
use crate::errors::QamError;
use crate::slack::{SlackUploader, resolve_slack_channel, resolve_slack_token};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlackFlags {
    pub enabled: Option<bool>,
    pub channel: Option<String>,
    pub token: Option<String>,
    pub message: Option<String>,
}

impl SlackFlags {
    /// Returns `true` if a Slack upload was explicitly requested on the command line,
    /// or returns a `QamError::Usage` error on conflicting or dangling flags.
    pub fn upload_requested(&self) -> Result<bool, QamError> {
        if self.enabled == Some(false) && self.channel.is_some() {
            return Err(QamError::Usage(
                "conflicting flags: --slack=false and --slack-channel".to_string(),
            ));
        }

        let requested = self.enabled == Some(true) || self.channel.is_some();
        if !requested {
            if self.message.is_some() {
                return Err(QamError::Usage(
                    "--slack-message has no effect without --slack (or --slack-channel)"
                        .to_string(),
                ));
            }
            if self.token.is_some() {
                return Err(QamError::Usage(
                    "--slack-token has no effect without --slack (or --slack-channel)".to_string(),
                ));
            }
        }

        Ok(requested)
    }
}

pub struct DeliveryRequest<'a> {
    pub out_path: &'a str,
    pub is_temp_file: bool,
    pub cfg: &'a Config,
    pub slack: SlackFlags,
    pub slack_api_base: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct DeliveryOutcome {
    pub slack_uploaded: bool,
}

/// Runs post-capture delivery (Slack) and removes temp files on success.
pub async fn deliver(req: DeliveryRequest<'_>) -> Result<DeliveryOutcome, QamError> {
    let slack_requested = req.slack.upload_requested()?;
    let mut slack_uploaded = false;

    if slack_requested {
        if req.out_path == "-" {
            return Err(QamError::Usage(
                "--out - cannot be used with Slack upload".into(),
            ));
        }

        let env_token = std::env::var("SLACK_TOKEN").ok();
        let slack_token = resolve_slack_token(
            req.slack.token.as_deref().unwrap_or(""),
            req.cfg,
            env_token.as_deref(),
        );
        let slack_channel =
            resolve_slack_channel(req.slack.channel.as_deref().unwrap_or(""), req.cfg);

        if slack_channel.is_empty() {
            return Err(QamError::Slack(
                "no Slack channel specified: pass --slack-channel \"#cam\" or configure a default channel with 'qam slack set --channel <channel>' (for --slack). Note: in shells like zsh/bash, an unquoted '#' starts a comment.".to_string(),
            ));
        }
        if slack_token.is_empty() {
            return Err(QamError::Slack(
                "slack upload requested but no token found (use --slack-token or SLACK_TOKEN environment variable)".to_string(),
            ));
        }

        let sty = Styler::with_color(Styler::is_color_enabled());
        let blue_arrow = sty.accent("→");
        let green_check = sty.ok("✔");
        let file_path = Path::new(req.out_path);

        println!(
            "{} uploading {} to Slack ({})…",
            blue_arrow,
            file_path.display(),
            slack_channel
        );

        let mut uploader = SlackUploader::new(&slack_token);
        if let Some(base) = req.slack_api_base {
            uploader = uploader.with_api_base(base);
        }
        let channel_id = uploader
            .resolve_channel(&slack_channel)
            .await
            .map_err(|e| QamError::Slack(format!("resolving slack channel: {e}")))?;

        let resp = uploader
            .upload_file(
                file_path,
                &channel_id,
                req.slack.message.as_deref().unwrap_or(""),
            )
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

        slack_uploaded = true;

        if req.is_temp_file {
            let _ = std::fs::remove_file(req.out_path);
        }
    }

    Ok(DeliveryOutcome { slack_uploaded })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slack_flags_upload_requested() {
        let cases = [
            (SlackFlags::default(), false),
            (
                SlackFlags {
                    enabled: Some(true),
                    ..Default::default()
                },
                true,
            ),
            (
                SlackFlags {
                    enabled: Some(false),
                    ..Default::default()
                },
                false,
            ),
            (
                SlackFlags {
                    channel: Some(String::new()),
                    ..Default::default()
                },
                true,
            ),
            (
                SlackFlags {
                    channel: Some("#alerts".into()),
                    ..Default::default()
                },
                true,
            ),
        ];

        for (flags, want) in cases {
            assert_eq!(flags.upload_requested().unwrap(), want, "{flags:?}");
        }

        for bad in [
            SlackFlags {
                enabled: Some(false),
                channel: Some("#x".into()),
                ..Default::default()
            },
            SlackFlags {
                message: Some("m".into()),
                ..Default::default()
            },
            SlackFlags {
                token: Some("t".into()),
                ..Default::default()
            },
        ] {
            assert!(
                bad.upload_requested().is_err(),
                "expected error for {bad:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_deliver_slack_is_strictly_opt_in() {
        let mock_server = wiremock::MockServer::start().await;
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("shot.jpg");
        std::fs::write(&file_path, b"jpg").unwrap();

        let mut cfg = Config::default();
        cfg.slack.token = "xoxb-test".into();
        cfg.slack.default_channel = "C111".into();

        // No --slack flag -> zero requests, file kept
        let out = deliver(DeliveryRequest {
            out_path: file_path.to_str().unwrap(),
            is_temp_file: true,
            cfg: &cfg,
            slack: SlackFlags::default(),
            slack_api_base: Some(&mock_server.uri()),
        })
        .await
        .unwrap();

        assert!(!out.slack_uploaded);
        assert!(file_path.exists());
        assert!(mock_server.received_requests().await.unwrap().is_empty());
    }
}
