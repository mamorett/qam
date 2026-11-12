use clap::Args;

use crate::cli::clip::{ClipArgs, run_clip};
use crate::errors::QamError;

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct AudioArgs {
    /// Camera name to use
    #[arg(short = 'c', long = "camera")]
    pub camera: Option<String>,

    /// Output file (e.g., output_audio.wav)
    #[arg(short = 'o', long = "out")]
    pub out: Option<String>,

    /// Recording duration (e.g., 10s)
    #[arg(long = "dur", default_value = "10s")]
    pub dur: String,

    /// Recording duration in seconds (alternative to --dur)
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

    /// Audio codec (default copy)
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

pub async fn run_audio(
    args: AudioArgs,
    global_config: Option<&str>,
    dur_explicit: bool,
    seconds_explicit: bool,
) -> Result<(), QamError> {
    let clip_args = ClipArgs {
        camera: args.camera,
        out: args.out,
        dur: args.dur,
        seconds: args.seconds,
        timeout: args.timeout,
        rtsp_auth: args.rtsp_auth,
        rtsp_transport: args.rtsp_transport,
        stream: args.stream,
        path: args.path,
        no_audio: false,
        audio_only: true,
        audio_codec: args.audio_codec,
        overwrite: args.overwrite,
        slack_channel: args.slack_channel,
        slack_token: args.slack_token,
        slack_message: args.slack_message,
        remaining: args.remaining,
    };

    run_clip(clip_args, global_config, dur_explicit, seconds_explicit).await
}
