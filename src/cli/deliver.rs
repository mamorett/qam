use chrono::Utc;
use std::path::Path;

use crate::cli::style::Styler;
use crate::config::Config;
use crate::errors::QamError;
use crate::s3::{
    KeyContext, S3Flags, S3UploadResult, maybe_upload_to_s3, render_object_key, resolve_s3_settings,
};
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
    pub camera: &'a str,
    pub cfg: &'a Config,
    pub slack: SlackFlags,
    pub s3: S3Flags,
    pub slack_api_base: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct DeliveryOutcome {
    pub slack_uploaded: bool,
    pub s3: Option<S3UploadResult>,
}

/// Runs post-capture delivery (S3 first, then Slack) and removes temp files only if all requested uploads succeed.
pub async fn deliver(req: DeliveryRequest<'_>) -> Result<DeliveryOutcome, QamError> {
    let s3_requested = req.s3.upload_requested()?;
    let slack_requested = req.slack.upload_requested()?;

    if !s3_requested && !slack_requested {
        return Ok(DeliveryOutcome::default());
    }

    if req.out_path == "-" {
        if s3_requested {
            return Err(QamError::Usage(
                "--out - cannot be used with S3 upload".into(),
            ));
        }
        return Err(QamError::Usage(
            "--out - cannot be used with Slack upload".into(),
        ));
    }

    let sty = Styler::with_color(Styler::is_color_enabled());
    let file_path = Path::new(req.out_path);
    let mut errors: Vec<QamError> = Vec::new();
    let mut s3_result: Option<S3UploadResult> = None;
    let mut slack_uploaded = false;

    if s3_requested {
        match run_s3_upload(file_path, req.camera, &req.s3, req.cfg, &sty).await {
            Ok(res) => s3_result = Some(res),
            Err(e) => errors.push(e),
        }
    }

    if slack_requested {
        match run_slack_upload(file_path, &req.slack, req.cfg, req.slack_api_base, &sty).await {
            Ok(()) => slack_uploaded = true,
            Err(e) => errors.push(e),
        }
    }

    if req.is_temp_file {
        if errors.is_empty() {
            let _ = std::fs::remove_file(req.out_path);
        } else {
            eprintln!("kept local file at {}", req.out_path);
        }
    }

    if !errors.is_empty() {
        let mut iter = errors.into_iter();
        let first = iter.next().unwrap();
        if let Some(second) = iter.next() {
            let combined = format!("{first}; {second}");
            return Err(match first {
                QamError::S3(_) => QamError::S3(combined),
                QamError::Slack(_) => QamError::Slack(combined),
                QamError::Usage(_) => QamError::Usage(combined),
                other => QamError::S3(format!("{other}; {second}")),
            });
        }
        return Err(first);
    }

    Ok(DeliveryOutcome {
        slack_uploaded,
        s3: s3_result,
    })
}

async fn run_s3_upload(
    file_path: &Path,
    camera: &str,
    s3_flags: &S3Flags,
    cfg: &Config,
    sty: &Styler,
) -> Result<S3UploadResult, QamError> {
    let settings = resolve_s3_settings(s3_flags, cfg, &|k| std::env::var(k).ok())?;
    let key = render_object_key(
        &settings.key_template,
        &KeyContext {
            camera,
            prefix: &settings.prefix,
            local_path: file_path,
            now: Utc::now(),
        },
    )?;
    maybe_upload_to_s3(file_path, &settings, &key, sty).await
}

async fn run_slack_upload(
    file_path: &Path,
    slack: &SlackFlags,
    cfg: &Config,
    api_base: Option<&str>,
    sty: &Styler,
) -> Result<(), QamError> {
    let env_token = std::env::var("SLACK_TOKEN").ok();
    let slack_token = resolve_slack_token(
        slack.token.as_deref().unwrap_or(""),
        cfg,
        env_token.as_deref(),
    );
    let slack_channel = resolve_slack_channel(slack.channel.as_deref().unwrap_or(""), cfg);

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

    let blue_arrow = sty.accent("→");
    let green_check = sty.ok("✔");

    println!(
        "{} uploading {} to Slack ({})…",
        blue_arrow,
        file_path.display(),
        slack_channel
    );

    let mut uploader = SlackUploader::new(&slack_token);
    if let Some(base) = api_base {
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
            slack.message.as_deref().unwrap_or(""),
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

    Ok(())
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
            camera: "kitchen",
            cfg: &cfg,
            slack: SlackFlags::default(),
            s3: S3Flags::default(),
            slack_api_base: Some(&mock_server.uri()),
        })
        .await
        .unwrap();

        assert!(!out.slack_uploaded);
        assert!(out.s3.is_none());
        assert!(file_path.exists());
        assert!(mock_server.received_requests().await.unwrap().is_empty());
    }
}
