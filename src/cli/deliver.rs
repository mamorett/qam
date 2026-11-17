use std::path::Path;

use crate::cli::style::Styler;
use crate::config::Config;
use crate::errors::QamError;
use crate::slack::{maybe_upload_to_slack, resolve_slack_channel, resolve_slack_token};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlackFlags {
    pub channel: Option<String>,
    pub token: Option<String>,
    pub message: Option<String>,
}

pub struct DeliveryRequest<'a> {
    pub out_path: &'a str,
    pub is_temp_file: bool,
    pub cfg: &'a Config,
    pub slack: SlackFlags,
}

#[derive(Debug, Clone, Default)]
pub struct DeliveryOutcome {
    pub slack_uploaded: bool,
}

/// Runs post-capture delivery (Slack) and removes temp files on success.
pub async fn deliver(req: DeliveryRequest<'_>) -> Result<DeliveryOutcome, QamError> {
    let env_token = std::env::var("SLACK_TOKEN").ok();
    let slack_token = resolve_slack_token(
        req.slack.token.as_deref().unwrap_or(""),
        req.cfg,
        env_token.as_deref(),
    );
    let slack_channel = resolve_slack_channel(req.slack.channel.as_deref().unwrap_or(""), req.cfg);

    let mut slack_uploaded = false;
    if req.slack.channel.is_some() || !slack_channel.is_empty() {
        if slack_channel.is_empty() {
            return Err(QamError::Slack(
                "no Slack channel specified: pass a channel (e.g. --slack-channel \"#cam\" or --slack-channel=#cam) or configure a default channel with 'qam slack set --channel <channel>'. Note: in shells like zsh/bash, an unquoted '#' starts a comment.".to_string(),
            ));
        }
        if req.out_path == "-" {
            return Err(QamError::Usage(
                "--out - cannot be used with Slack upload".into(),
            ));
        }

        let sty = Styler::with_color(Styler::is_color_enabled());
        maybe_upload_to_slack(
            Path::new(req.out_path),
            &slack_token,
            &slack_channel,
            req.slack.message.as_deref().unwrap_or(""),
            &sty,
        )
        .await?;

        slack_uploaded = true;

        if req.is_temp_file {
            let _ = std::fs::remove_file(req.out_path);
        }
    }

    Ok(DeliveryOutcome { slack_uploaded })
}
