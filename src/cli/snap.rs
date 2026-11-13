use clap::Args;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use crate::cli::flags::{RtspAuth, Transport, parse_bool_flag, parse_duration_flag};
use crate::cli::resolve::resolve_rtsp_url;
use crate::cli::style::Styler;
use crate::errors::QamError;
use crate::media::snapshot::{SnapshotOptions, capture_snapshot};
use crate::slack::{maybe_upload_to_slack, resolve_slack_channel, resolve_slack_token};

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct SnapArgs {
    /// Camera name to use
    #[arg(short = 'c', long = "camera")]
    pub camera: Option<String>,

    /// Output file (e.g., snap.jpg, or - for stdout)
    #[arg(short = 'o', long = "out")]
    pub out: Option<String>,

    /// Timeout for RTSP operations
    #[arg(long = "timeout", default_value = "30s")]
    pub timeout: String,

    /// RTSP auth mode: auto|basic|digest
    #[arg(long = "rtsp-auth", default_value = "auto")]
    pub rtsp_auth: String,

    /// RTSP transport: tcp|udp
    #[arg(long = "rtsp-transport", default_value = "tcp")]
    pub rtsp_transport: String,

    /// RTSP path segment (stream1 or stream2); ignored if --path is set
    #[arg(long = "stream")]
    pub stream: Option<String>,

    /// Custom RTSP path (overrides --stream), e.g., /Bfy... from UniFi Protect
    #[arg(long = "path")]
    pub path: Option<String>,

    /// Quality setting: 0=default, 1..=31 (1 is best, 31 is lowest)
    #[arg(long = "quality", default_value = "0")]
    pub quality: u8,

    /// Overwrite existing output file
    #[arg(long = "overwrite", default_value = "true")]
    pub overwrite: String,

    /// RTSP client: ffmpeg|gortsplib (accepted for compatibility, qam uses pure-Rust engine)
    #[arg(long = "rtsp-client")]
    pub rtsp_client: Option<String>,

    /// Slack channel to post to
    #[arg(long = "slack-channel", num_args = 0..=1, default_missing_value = "")]
    pub slack_channel: Option<String>,

    /// Slack API token (xoxb-...)
    #[arg(long = "slack-token")]
    pub slack_token: Option<String>,

    /// Optional message to accompany file
    #[arg(long = "slack-message")]
    pub slack_message: Option<String>,

    /// Positional camera name if --camera not set, plus remaining trailing args
    #[arg(trailing_var_arg = true)]
    pub remaining: Vec<String>,
}

pub async fn run_snap(
    args: SnapArgs,
    global_config: Option<&str>,
    json_output: bool,
) -> Result<(), QamError> {
    // 1. Resolve camera name
    let mut camera_name = args.camera.unwrap_or_default();
    if camera_name.is_empty() && !args.remaining.is_empty() {
        camera_name = args.remaining[0].clone();
    }
    if camera_name.is_empty() {
        return Err(QamError::Usage("--camera is required".into()));
    }

    // 2. Validate stream vs path
    let stream = args.stream.unwrap_or_default();
    let custom_path = args.path.unwrap_or_default();
    if !stream.is_empty() && !custom_path.is_empty() {
        return Err(QamError::Usage(
            "use --path for custom RTSP token URLs; omit --stream".into(),
        ));
    }

    // 3. Resolve output path
    let mut is_temp_file = false;
    let out_path = if let Some(ref o) = args.out {
        if o.is_empty() {
            let tmp = tempfile::Builder::new()
                .prefix("qam-")
                .suffix(".jpg")
                .tempfile()?;
            let p = tmp.path().to_string_lossy().to_string();
            tmp.close()?;
            is_temp_file = true;
            eprintln!("No --out provided, writing snapshot to {p}");
            p
        } else {
            o.clone()
        }
    } else {
        let tmp = tempfile::Builder::new()
            .prefix("qam-")
            .suffix(".jpg")
            .tempfile()?;
        let p = tmp.path().to_string_lossy().to_string();
        tmp.close()?;
        is_temp_file = true;
        eprintln!("No --out provided, writing snapshot to {p}");
        p
    };

    let overwrite = parse_bool_flag(&args.overwrite).unwrap_or(true);
    if out_path != "-" && !overwrite && Path::new(&out_path).exists() {
        return Err(QamError::Media(format!("output file exists: {out_path}")));
    }

    // 4. Load config and find camera
    let cfg = crate::config::load(global_config.unwrap_or(""))?;

    let cam = cfg
        .find_camera(&camera_name)
        .ok_or_else(|| QamError::Usage(format!("camera {camera_name:?} not found")))?;

    let url = resolve_rtsp_url(&cam, Some(&stream), Some(&custom_path))?;

    // 5. Flags precedence
    let transport = if args.rtsp_transport != "tcp" {
        Transport::from_str(&args.rtsp_transport)?
    } else if !cam.rtsp_transport.is_empty() {
        Transport::from_str(&cam.rtsp_transport)?
    } else if !cfg.defaults.rtsp_transport.is_empty() {
        Transport::from_str(&cfg.defaults.rtsp_transport)?
    } else {
        Transport::Tcp
    };

    let auth = if args.rtsp_auth != "auto" {
        RtspAuth::from_str(&args.rtsp_auth)?
    } else if !cam.rtsp_auth.is_empty() {
        RtspAuth::from_str(&cam.rtsp_auth)?
    } else if !cfg.defaults.rtsp_auth.is_empty() {
        RtspAuth::from_str(&cfg.defaults.rtsp_auth)?
    } else {
        RtspAuth::Auto
    };

    let timeout: Duration = parse_duration_flag(&args.timeout).unwrap_or(Duration::from_secs(30));

    // 6. Capture snapshot
    capture_snapshot(SnapshotOptions {
        url: &url,
        transport,
        auth,
        timeout,
        out_path: &out_path,
        overwrite,
        quality: args.quality,
    })
    .await?;

    // 7. Slack upload if configured
    let env_token = std::env::var("SLACK_TOKEN").ok();
    let slack_token = resolve_slack_token(
        args.slack_token.as_deref().unwrap_or(""),
        &cfg,
        env_token.as_deref(),
    );
    let slack_channel = resolve_slack_channel(args.slack_channel.as_deref().unwrap_or(""), &cfg);

    let mut uploaded = false;
    if args.slack_channel.is_some() || !slack_channel.is_empty() {
        if slack_channel.is_empty() {
            return Err(QamError::Slack(
                "no Slack channel specified: pass a channel (e.g. --slack-channel \"#cam\" or --slack-channel=#cam) or configure a default channel with 'qam slack set --channel <channel>'. Note: in shells like zsh/bash, an unquoted '#' starts a comment.".to_string(),
            ));
        }
        if out_path == "-" {
            return Err(QamError::Usage(
                "--out - cannot be used with Slack upload".into(),
            ));
        }

        let sty = Styler::with_color(Styler::is_color_enabled());
        maybe_upload_to_slack(
            Path::new(&out_path),
            &slack_token,
            &slack_channel,
            args.slack_message.as_deref().unwrap_or(""),
            &sty,
        )
        .await?;

        uploaded = true;

        if is_temp_file {
            let _ = std::fs::remove_file(&out_path);
        }
    }

    // 8. Output presentation
    if json_output {
        let json_val = serde_json::json!({
            "path": out_path,
            "slack_uploaded": uploaded,
        });
        println!("{json_val}");
    }

    Ok(())
}
