use clap::{Arg, Command};
use clap_complete::Shell;

use crate::cli::flags::{bool_arg, trailing_ignored_arg};
use crate::cli::style::Styler;
use crate::errors::QamError;

pub fn colorize_long(sty: &Styler) -> String {
    let title = sty.accent("qam");
    let common = sty.accent("Common commands:");
    let snap = sty.ok("snap");
    let clip = sty.ok("clip");
    let audio = sty.ok("audio");
    let doctor = sty.err("doctor");

    format!(
        "{title} – capture frames/clips and motion from RTSP/ONVIF cameras.\n\n\
        {common}\n  \
        {snap}     grab a frame (positional camera name allowed)\n  \
        {clip}     short clip; drop audio with --no-audio\n  \
        {audio}    record audio clip (default 10s, copy codec, .wav)\n  \
        {doctor}   checks media engine, reachability, optional RTSP probe\n"
    )
}

pub fn example_text() -> &'static str {
    "  qam add --name kitchen --host 192.168.0.175 --user tapo --pass secret --rtsp-transport udp --stream stream2\n  \
       qam snap kitchen --out shot.jpg\n  \
       qam snap kitchen --slack-channel #security --slack-message \"Kitchen check!\"\n  \
       qam clip kitchen --dur 5s --no-audio --out clip.mp4\n  \
       qam clip kitchen --dur 10s --slack-channel @steve\n  \
       qam audio kitchen --seconds 10 --out audio.wav\n  \
       qam watch kitchen --threshold 0.2 --cooldown 5s --action 'qam snap kitchen --slack-channel #alerts'\n  \
       qam doctor --probe --rtsp-transport udp"
}

pub fn build_cli(color: bool) -> Command {
    let sty = Styler::with_color(color);
    let long_desc = colorize_long(&sty);

    Command::new("qam")
        .about("qam captures frames and clips from RTSP cameras (Tapo first, Ubiquiti next)")
        .long_about(long_desc)
        .after_help(format!("Examples:\n{}\n", example_text()))
        .disable_version_flag(true)
        .arg(
            Arg::new("version")
                .short('v')
                .long("version")
                .action(clap::ArgAction::SetTrue)
                .help("version for qam"),
        )
        .arg(
            Arg::new("config")
                .long("config")
                .global(true)
                .value_name("path")
                .help("Path to config file (default: $XDG_CONFIG_HOME/camsnap/config.yaml)"),
        )
        .subcommand_required(false)
        .arg_required_else_help(false)
        .subcommands([
            build_add_cmd(),
            build_list_cmd(),
            build_snap_cmd(),
            build_clip_cmd(),
            build_audio_cmd(),
            build_discover_cmd(),
            build_watch_cmd(),
            build_doctor_cmd(),
            build_slack_cmd(),
            build_version_cmd(),
            build_completion_cmd(),
        ])
}

fn build_add_cmd() -> Command {
    Command::new("add")
        .about("Add or update a camera")
        .arg(Arg::new("name").long("name").help("Camera name (unique)"))
        .arg(Arg::new("host").long("host").help("Camera host or IP"))
        .arg(
            Arg::new("port")
                .long("port")
                .default_value("554")
                .value_parser(clap::value_parser!(u16))
                .help("Camera port (default 554)"),
        )
        .arg(
            Arg::new("protocol")
                .long("protocol")
                .default_value("rtsp")
                .help("Protocol (rtsp or rtsps)"),
        )
        .arg(Arg::new("user").long("user").help("Camera username"))
        .arg(Arg::new("pass").long("pass").help("Camera password"))
        .arg(
            Arg::new("path")
                .long("path")
                .help("Explicit RTSP path (e.g., /Bfy... token from UniFi Protect)"),
        )
        .arg(
            Arg::new("rtsp-transport")
                .long("rtsp-transport")
                .help("Preferred RTSP transport for this camera (tcp|udp)"),
        )
        .arg(
            Arg::new("stream")
                .long("stream")
                .help("Default RTSP stream path (stream1 or stream2)"),
        )
        .arg(
            Arg::new("rtsp-client")
                .long("rtsp-client")
                .help("Default RTSP client (ffmpeg|gortsplib)"),
        )
        .arg(bool_arg("no-audio", "Default: drop audio for this camera"))
        .arg(
            Arg::new("audio-codec")
                .long("audio-codec")
                .help("Default audio codec when recording (e.g., aac)"),
        )
        .arg(trailing_ignored_arg())
}

fn build_list_cmd() -> Command {
    Command::new("list")
        .visible_alias("ls")
        .about("List saved cameras")
        .arg(trailing_ignored_arg())
}

fn build_snap_cmd() -> Command {
    Command::new("snap")
        .about("Capture a single frame to a file")
        .arg(Arg::new("camera_pos").help("Camera name to use"))
        .arg(Arg::new("camera").long("camera").help("Camera name to use"))
        .arg(
            Arg::new("out")
                .long("out")
                .help("Output file (e.g., snap.jpg)"),
        )
        .arg(
            Arg::new("path")
                .long("path")
                .help("Custom RTSP path (overrides --stream), e.g., /Bfy... from UniFi Protect"),
        )
        .arg(
            Arg::new("rtsp-auth")
                .long("rtsp-auth")
                .default_value("auto")
                .help("RTSP auth mode: auto|basic|digest"),
        )
        .arg(
            Arg::new("rtsp-client")
                .long("rtsp-client")
                .default_value("ffmpeg")
                .help("RTSP client: ffmpeg|gortsplib"),
        )
        .arg(
            Arg::new("rtsp-transport")
                .long("rtsp-transport")
                .default_value("tcp")
                .help("RTSP transport: tcp|udp"),
        )
        .arg(
            Arg::new("slack-channel")
                .long("slack-channel")
                .help("Slack channel or user to upload to (e.g. #general, @alice)"),
        )
        .arg(
            Arg::new("slack-message")
                .long("slack-message")
                .help("Optional message to post with the file"),
        )
        .arg(
            Arg::new("slack-token")
                .long("slack-token")
                .help("Slack API token (overrides config and SLACK_TOKEN env var)"),
        )
        .arg(
            Arg::new("stream")
                .long("stream")
                .help("RTSP path segment (stream1 or stream2); ignored if --path is set"),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .default_value("30s")
                .help("Timeout for capture"),
        )
        .arg(trailing_ignored_arg())
}

fn build_clip_cmd() -> Command {
    Command::new("clip")
        .about("Record a short clip to a file")
        .arg(Arg::new("camera_pos").help("Camera name to use"))
        .arg(Arg::new("audio-codec").long("audio-codec").help(
            "Audio codec (default aac for video clip, copy for audio-only); ignored if --no-audio",
        ))
        .arg(bool_arg(
            "audio-only",
            "Record audio only (drop video track)",
        ))
        .arg(Arg::new("camera").long("camera").help("Camera name to use"))
        .arg(
            Arg::new("dur")
                .long("dur")
                .default_value("10s")
                .help("Clip duration (e.g., 10s)"),
        )
        .arg(
            Arg::new("seconds")
                .short('s')
                .long("seconds")
                .value_parser(clap::value_parser!(i64))
                .help("Clip duration in seconds (alternative to --dur)"),
        )
        .arg(bool_arg("no-audio", "Drop audio track"))
        .arg(
            Arg::new("out")
                .long("out")
                .help("Output file (e.g., clip.mp4)"),
        )
        .arg(bool_arg("overwrite", "Overwrite output file if it exists"))
        .arg(
            Arg::new("path")
                .long("path")
                .help("Custom RTSP path (overrides --stream), e.g., /Bfy... from UniFi Protect"),
        )
        .arg(
            Arg::new("rtsp-auth")
                .long("rtsp-auth")
                .default_value("auto")
                .help("RTSP auth mode: auto|basic|digest"),
        )
        .arg(
            Arg::new("rtsp-transport")
                .long("rtsp-transport")
                .default_value("tcp")
                .help("RTSP transport: tcp|udp"),
        )
        .arg(
            Arg::new("slack-channel")
                .long("slack-channel")
                .help("Slack channel or user to upload to (e.g. #general, @alice)"),
        )
        .arg(
            Arg::new("slack-message")
                .long("slack-message")
                .help("Optional message to post with the file"),
        )
        .arg(
            Arg::new("slack-token")
                .long("slack-token")
                .help("Slack API token (overrides config and SLACK_TOKEN env var)"),
        )
        .arg(
            Arg::new("stream")
                .long("stream")
                .help("RTSP path segment (stream1 or stream2); ignored if --path is set"),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .default_value("30s")
                .help("Timeout for capture"),
        )
        .arg(trailing_ignored_arg())
}

fn build_audio_cmd() -> Command {
    Command::new("audio")
        .visible_alias("record-audio")
        .about("Record an audio-only clip to a file")
        .arg(Arg::new("camera_pos").help("Camera name to use"))
        .arg(
            Arg::new("audio-codec")
                .long("audio-codec")
                .default_value("copy")
                .help("Audio codec (default copy)"),
        )
        .arg(Arg::new("camera").long("camera").help("Camera name to use"))
        .arg(
            Arg::new("dur")
                .long("dur")
                .default_value("10s")
                .help("Recording duration (e.g., 10s)"),
        )
        .arg(
            Arg::new("out")
                .long("out")
                .help("Output file (e.g., output_audio.wav)"),
        )
        .arg(bool_arg("overwrite", "Overwrite output file if it exists"))
        .arg(
            Arg::new("path")
                .long("path")
                .help("Custom RTSP path (overrides --stream), e.g., /Bfy... from UniFi Protect"),
        )
        .arg(
            Arg::new("rtsp-auth")
                .long("rtsp-auth")
                .default_value("auto")
                .help("RTSP auth mode: auto|basic|digest"),
        )
        .arg(
            Arg::new("rtsp-transport")
                .long("rtsp-transport")
                .default_value("tcp")
                .help("RTSP transport: tcp|udp"),
        )
        .arg(
            Arg::new("seconds")
                .short('s')
                .long("seconds")
                .value_parser(clap::value_parser!(i64))
                .help("Recording duration in seconds (alternative to --dur)"),
        )
        .arg(
            Arg::new("slack-channel")
                .long("slack-channel")
                .help("Slack channel or user to upload to (e.g. #general, @alice)"),
        )
        .arg(
            Arg::new("slack-message")
                .long("slack-message")
                .help("Optional message to post with the file"),
        )
        .arg(
            Arg::new("slack-token")
                .long("slack-token")
                .help("Slack API token (overrides config and SLACK_TOKEN env var)"),
        )
        .arg(
            Arg::new("stream")
                .long("stream")
                .help("RTSP path segment (stream1 or stream2); ignored if --path is set"),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .default_value("30s")
                .help("Timeout for capture"),
        )
        .arg(trailing_ignored_arg())
}

fn build_discover_cmd() -> Command {
    Command::new("discover")
        .about("Discover cameras on the local network via ONVIF WS-Discovery")
        .arg(bool_arg(
            "info",
            "Attempt ONVIF GetDeviceInformation (may require credentials)",
        ))
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .default_value("3s")
                .help("Discovery timeout"),
        )
        .arg(trailing_ignored_arg())
}

fn build_watch_cmd() -> Command {
    Command::new("watch")
        .about("Run motion detection and execute an action")
        .arg(Arg::new("camera_pos").help("Camera name to monitor"))
        .arg(
            Arg::new("action")
                .long("action")
                .help("Command to execute when motion detected"),
        )
        .arg(Arg::new("action-template").long("action-template").help(
            "Optional template to build action command (placeholders: {camera},{score},{time})",
        ))
        .arg(
            Arg::new("camera")
                .long("camera")
                .help("Camera name to monitor"),
        )
        .arg(
            Arg::new("cooldown")
                .long("cooldown")
                .default_value("5s")
                .help("Cooldown between triggering actions"),
        )
        .arg(
            Arg::new("duration")
                .long("duration")
                .default_value("0s")
                .help("Optional max runtime (0 = until interrupted)"),
        )
        .arg(bool_arg("json", "Log motion events as JSON lines"))
        .arg(
            Arg::new("path")
                .long("path")
                .help("Custom RTSP path (overrides --stream), e.g., /Bfy... from UniFi Protect"),
        )
        .arg(
            Arg::new("rtsp-auth")
                .long("rtsp-auth")
                .default_value("auto")
                .help("RTSP auth mode: auto|basic|digest"),
        )
        .arg(
            Arg::new("rtsp-transport")
                .long("rtsp-transport")
                .default_value("tcp")
                .help("RTSP transport: tcp|udp"),
        )
        .arg(
            Arg::new("stream")
                .long("stream")
                .help("RTSP path segment (stream1 or stream2); ignored if --path is set"),
        )
        .arg(
            Arg::new("threshold")
                .long("threshold")
                .default_value("0.2")
                .help("Scene change threshold (0-1, higher = less sensitive)"),
        )
        .arg(trailing_ignored_arg())
}

fn build_doctor_cmd() -> Command {
    Command::new("doctor")
        .about("Run basic checks (media engine, config present, camera ports reachable)")
        .arg(bool_arg("probe", "Probe each RTSP URL briefly"))
        .arg(
            Arg::new("rtsp-auth")
                .long("rtsp-auth")
                .default_value("auto")
                .help("RTSP auth mode: auto|basic|digest"),
        )
        .arg(
            Arg::new("rtsp-transport")
                .long("rtsp-transport")
                .default_value("tcp")
                .help("RTSP transport: tcp|udp (probe)"),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .default_value("2s")
                .help("Dial timeout per camera"),
        )
        .arg(trailing_ignored_arg())
}

fn build_slack_cmd() -> Command {
    Command::new("slack")
        .about("Manage Slack configuration")
        .subcommands([
            Command::new("set")
                .about("Store Slack credentials in config")
                .arg(
                    Arg::new("channel")
                        .long("channel")
                        .help("Default Slack channel or user"),
                )
                .arg(Arg::new("token").long("token").help("Slack API token"))
                .arg(trailing_ignored_arg()),
            Command::new("test")
                .about("Test Slack upload with a file")
                .arg(Arg::new("file").required(true).help("File to upload"))
                .arg(
                    Arg::new("channel")
                        .long("channel")
                        .help("Slack channel or user (overrides default)"),
                )
                .arg(Arg::new("message").long("message").help("Message to post")),
        ])
}

fn build_version_cmd() -> Command {
    Command::new("version")
        .about("Show qam version")
        .arg(trailing_ignored_arg())
}

fn build_completion_cmd() -> Command {
    Command::new("completion")
        .about("Generate the autocompletion script for qam for the specified shell")
        .subcommands([
            Command::new("bash").about("Generate the autocompletion script for bash"),
            Command::new("fish").about("Generate the autocompletion script for fish"),
            Command::new("powershell").about("Generate the autocompletion script for powershell"),
            Command::new("zsh").about("Generate the autocompletion script for zsh"),
        ])
}

pub async fn run_cli<I, T>(args: I) -> Result<(), QamError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let args_vec: Vec<std::ffi::OsString> = args.into_iter().map(|a| a.into()).collect();
    let is_color = Styler::is_color_enabled();

    let mut cli = build_cli(is_color);

    let matches = match cli.try_get_matches_from_mut(&args_vec) {
        Ok(m) => m,
        Err(err) => {
            if err.kind() == clap::error::ErrorKind::DisplayHelp
                || err.kind() == clap::error::ErrorKind::DisplayVersion
            {
                print!("{err}");
                return Ok(());
            }
            return Err(QamError::Usage(err.to_string().trim().to_string()));
        }
    };

    let config_path = matches.get_one::<String>("config").map(|s| s.as_str());

    if matches.get_flag("version") {
        println!("{}", crate::version::VERSION);
        return Ok(());
    }

    match matches.subcommand() {
        Some(("add", sub)) => {
            let name = sub.get_one::<String>("name").cloned().unwrap_or_default();
            let host = sub.get_one::<String>("host").cloned().unwrap_or_default();
            let port = *sub.get_one::<u16>("port").unwrap_or(&554);
            let protocol = sub
                .get_one::<String>("protocol")
                .cloned()
                .unwrap_or_default();
            let user = sub.get_one::<String>("user").cloned().unwrap_or_default();
            let pass = sub.get_one::<String>("pass").cloned().unwrap_or_default();
            let path = sub.get_one::<String>("path").cloned().unwrap_or_default();
            let rtsp_transport = sub
                .get_one::<String>("rtsp-transport")
                .cloned()
                .unwrap_or_default();
            let stream = sub.get_one::<String>("stream").cloned().unwrap_or_default();
            let rtsp_client = sub
                .get_one::<String>("rtsp-client")
                .cloned()
                .unwrap_or_default();
            let no_audio = *sub.get_one::<bool>("no-audio").unwrap_or(&false);
            let audio_codec = sub
                .get_one::<String>("audio-codec")
                .cloned()
                .unwrap_or_default();

            crate::cli::add::execute(
                name,
                host,
                port,
                protocol,
                user,
                pass,
                path,
                rtsp_transport,
                stream,
                rtsp_client,
                no_audio,
                audio_codec,
                config_path,
            )?;
        }
        Some(("list", _sub)) => {
            crate::cli::list::execute(config_path, !is_color)?;
        }
        Some(("version", _sub)) => {
            crate::cli::version::execute();
        }
        Some(("completion", sub)) => match sub.subcommand() {
            Some(("bash", _)) => crate::cli::completion::execute(Shell::Bash, &mut cli),
            Some(("fish", _)) => crate::cli::completion::execute(Shell::Fish, &mut cli),
            Some(("powershell", _)) => crate::cli::completion::execute(Shell::PowerShell, &mut cli),
            Some(("zsh", _)) => crate::cli::completion::execute(Shell::Zsh, &mut cli),
            _ => {
                return Err(QamError::Usage("shell required for completion".into()));
            }
        },
        Some(("slack", sub)) => match sub.subcommand() {
            Some(("set", set_sub)) => {
                let token = set_sub
                    .get_one::<String>("token")
                    .cloned()
                    .unwrap_or_default();
                let channel = set_sub
                    .get_one::<String>("channel")
                    .cloned()
                    .unwrap_or_default();
                crate::cli::slack_cmd::execute_set(&token, &channel, config_path)?;
            }
            Some(("test", test_sub)) => {
                let file = test_sub
                    .get_one::<String>("file")
                    .cloned()
                    .unwrap_or_default();
                let channel = test_sub
                    .get_one::<String>("channel")
                    .cloned()
                    .unwrap_or_default();
                let message = test_sub
                    .get_one::<String>("message")
                    .cloned()
                    .unwrap_or_default();
                crate::cli::slack_cmd::execute_test(
                    std::path::Path::new(&file),
                    &channel,
                    &message,
                    config_path,
                )
                .await?;
            }
            _ => {
                return Err(QamError::Usage("subcommand required for slack".into()));
            }
        },
        Some(("snap", _))
        | Some(("clip", _))
        | Some(("audio", _))
        | Some(("discover", _))
        | Some(("watch", _))
        | Some(("doctor", _)) => {
            return Err(QamError::Usage("not implemented".into()));
        }
        _ => {
            // No subcommand invoked: print help
            let _ = cli.print_help();
            println!();
        }
    }

    Ok(())
}
