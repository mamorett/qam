use clap::Args;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use crate::cli::flags::{RtspAuth, Transport, parse_bool_flag, parse_duration_flag};
use crate::cli::resolve::{append_path, append_stream};
use crate::cli::style::Styler;
use crate::config::{Config, default_path, load};
use crate::errors::QamError;
use crate::media::recorder::{ClipRecordOptions, record_media};
use crate::rtsp_url::build_url;
use crate::slack::{maybe_upload_to_slack, resolve_slack_channel, resolve_slack_token};

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ClipArgs {
    /// Camera name to use
    #[arg(short = 'c', long = "camera")]
    pub camera: Option<String>,

    /// Output file (e.g., clip.mp4)
    #[arg(short = 'o', long = "out")]
    pub out: Option<String>,

    /// Clip duration (e.g., 10s)
    #[arg(long = "dur", default_value = "10s")]
    pub dur: String,

    /// Clip duration in seconds (alternative to --dur)
    #[arg(short = 's', long = "seconds")]
    pub seconds: Option<i64>,

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

    /// Drop audio track
    #[arg(long = "no-audio")]
    pub no_audio: bool,

    /// Record audio only (drop video track)
    #[arg(long = "audio-only")]
    pub audio_only: bool,

    /// Audio codec (default aac for video clip, copy for audio-only); ignored if --no-audio
    #[arg(long = "audio-codec")]
    pub audio_codec: Option<String>,

    /// Overwrite existing output file
    #[arg(long = "overwrite")]
    pub overwrite: Option<String>,

    /// Slack channel to post to
    #[arg(long = "slack-channel")]
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

pub async fn run_clip(
    args: ClipArgs,
    global_config: Option<&str>,
    dur_explicit: bool,
    seconds_explicit: bool,
) -> Result<(), QamError> {
    // 1. Resolve camera name
    let mut camera_name = args.camera.unwrap_or_default();
    if camera_name.is_empty() && !args.remaining.is_empty() {
        camera_name = args.remaining[0].clone();
    }
    if camera_name.is_empty() {
        return Err(QamError::Usage("--camera is required".into()));
    }

    // 2. Duration validation
    if dur_explicit && seconds_explicit {
        return Err(QamError::Usage(
            "cannot specify both --dur and --seconds".into(),
        ));
    }
    let duration = if seconds_explicit {
        let sec = args.seconds.unwrap_or(0);
        if sec <= 0 {
            return Err(QamError::Usage("--seconds must be > 0".into()));
        }
        Duration::from_secs(sec as u64)
    } else {
        let d = parse_duration_flag(&args.dur).unwrap_or(Duration::from_secs(10));
        if d.is_zero() {
            return Err(QamError::Usage("--dur must be > 0".into()));
        }
        d
    };

    // 3. Audio validations
    if args.audio_only && args.no_audio {
        return Err(QamError::Usage(
            "cannot combine --audio-only and --no-audio".into(),
        ));
    }

    let stream = args.stream.unwrap_or_default();
    let mut custom_path = args.path.unwrap_or_default();
    if !stream.is_empty() && !custom_path.is_empty() {
        return Err(QamError::Usage(
            "use --path for custom RTSP token URLs; omit --stream".into(),
        ));
    }

    // 4. Overwrite resolution
    // For standard video clip, default overwrite to true unless audio-only or explicitly provided
    let overwrite = match args.overwrite {
        Some(ref s) => parse_bool_flag(s).unwrap_or(!args.audio_only),
        None => !args.audio_only,
    };

    // 5. Output path resolution
    let mut is_temp_file = false;
    let out_path = if let Some(ref o) = args.out {
        if o.is_empty() {
            let ext = if args.audio_only { "wav" } else { "mp4" };
            let label = if args.audio_only { "audio" } else { "clip" };
            let tmp = tempfile::Builder::new()
                .prefix("qam-")
                .suffix(&format!(".{ext}"))
                .tempfile()?;
            let p = tmp.path().to_string_lossy().to_string();
            tmp.close()?;
            is_temp_file = true;
            eprintln!("No --out provided, writing {label} to {p}");
            p
        } else {
            o.clone()
        }
    } else {
        let ext = if args.audio_only { "wav" } else { "mp4" };
        let label = if args.audio_only { "audio" } else { "clip" };
        let tmp = tempfile::Builder::new()
            .prefix("qam-")
            .suffix(&format!(".{ext}"))
            .tempfile()?;
        let p = tmp.path().to_string_lossy().to_string();
        tmp.close()?;
        is_temp_file = true;
        eprintln!("No --out provided, writing {label} to {p}");
        p
    };

    if out_path == "-" {
        let label = if args.audio_only { "audio" } else { "clip" };
        return Err(QamError::Usage(format!(
            "stdout output not supported for {label}"
        )));
    }

    if !overwrite && Path::new(&out_path).exists() {
        return Err(QamError::Media(format!(
            "output file {out_path:?} already exists (use --overwrite to replace)"
        )));
    }

    // 6. Load config
    let cfg = if let Some(p) = global_config {
        load(p).unwrap_or_default()
    } else if let Ok(p) = default_path() {
        load(p.to_str().unwrap_or_default()).unwrap_or_default()
    } else {
        Config::default()
    };

    let mut cam = cfg
        .find_camera(&camera_name)
        .ok_or_else(|| QamError::Usage(format!("camera {camera_name:?} not found")))?;

    if custom_path.is_empty() && !cam.path.is_empty() {
        custom_path = cam.path.clone();
    }
    if !custom_path.is_empty() {
        cam.path = custom_path.clone();
        cam.stream = String::new();
    }

    let mut url = build_url(&cam)?;
    if !custom_path.is_empty() {
        url = append_path(&url, &custom_path);
    } else {
        url = append_stream(&url, &stream);
    }

    // 7. Flags precedence
    let transport = if args.rtsp_transport != "tcp" {
        Transport::from_str(&args.rtsp_transport)?
    } else if !cam.rtsp_transport.is_empty() {
        Transport::from_str(&cam.rtsp_transport)?
    } else {
        Transport::Tcp
    };

    let auth = if args.rtsp_auth != "auto" {
        RtspAuth::from_str(&args.rtsp_auth)?
    } else if !cam.rtsp_auth.is_empty() {
        RtspAuth::from_str(&cam.rtsp_auth)?
    } else {
        RtspAuth::Auto
    };

    let timeout: Duration = parse_duration_flag(&args.timeout).unwrap_or(Duration::from_secs(30));

    let no_audio = if args.audio_only {
        false
    } else if args.no_audio {
        true
    } else {
        cam.no_audio
    };

    // 8. Record
    record_media(ClipRecordOptions {
        url: &url,
        transport,
        auth,
        timeout,
        duration,
        out_path: &out_path,
        overwrite,
        no_audio,
        audio_only: args.audio_only,
    })
    .await?;

    // 9. Slack upload if configured
    let env_token = std::env::var("SLACK_TOKEN").ok();
    let slack_token = resolve_slack_token(
        args.slack_token.as_deref().unwrap_or(""),
        &cfg,
        env_token.as_deref(),
    );
    let slack_channel = resolve_slack_channel(args.slack_channel.as_deref().unwrap_or(""), &cfg);

    if !slack_channel.is_empty() {
        let sty = Styler::with_color(Styler::is_color_enabled());
        maybe_upload_to_slack(
            Path::new(&out_path),
            &slack_token,
            &slack_channel,
            args.slack_message.as_deref().unwrap_or(""),
            &sty,
        )
        .await?;

        if is_temp_file {
            let _ = std::fs::remove_file(&out_path);
        }
    }

    Ok(())
}
