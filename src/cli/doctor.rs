use std::time::Duration;

use crate::cli::resolve::resolve_rtsp_url;
use crate::cli::style::Styler;
use crate::config::{self, resolve_load_path};
use crate::errors::QamError;
use crate::media::probe::probe_rtsp;
use crate::timefmt::parse_go_duration;

#[derive(Debug, Default)]
pub struct DoctorArgs {
    pub timeout: Option<String>,
    pub probe: bool,
    pub rtsp_auth: Option<String>,
    pub rtsp_transport: Option<String>,
    pub config: Option<String>,
}

pub async fn run_doctor(args: DoctorArgs) -> Result<(), QamError> {
    let sty = Styler::new();

    println!(
        "{}",
        sty.ok(
            "✔ built-in media engine (H.264, H.265, MJPEG decode; MP4/WAV mux; no ffmpeg required)"
        )
    );

    let resolved_path = resolve_load_path(args.config.as_deref().unwrap_or(""))?;
    let cfg = config::load_from_path(&resolved_path)?;

    println!("Config file: {}", resolved_path.display());

    if cfg.cameras.is_empty() {
        println!(
            "{}",
            sty.warn("No cameras saved. Add one with camsnap add ...")
        );
        return Ok(());
    }

    let timeout_dur = if let Some(ref t) = args.timeout {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            Duration::from_secs(2)
        } else {
            parse_go_duration(trimmed).unwrap_or(Duration::from_secs(2))
        }
    } else {
        Duration::from_secs(2)
    };
    let timeout_dur = if timeout_dur.is_zero() {
        Duration::from_secs(2)
    } else {
        timeout_dur
    };

    let auth_str = args.rtsp_auth.as_deref().unwrap_or("auto");
    let transport_str = args.rtsp_transport.as_deref().unwrap_or("tcp");

    for cam in &cfg.cameras {
        let host = &cam.host;
        let port = if cam.port == 0 { 554 } else { cam.port };
        let addr = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };

        match tokio::time::timeout(timeout_dur, tokio::net::TcpStream::connect(&addr)).await {
            Err(_) => {
                println!(
                    "{} {} dial {} failed: dial tcp {}: i/o timeout",
                    sty.err("✖"),
                    cam.name,
                    addr,
                    addr
                );
                continue;
            }
            Ok(Err(e)) => {
                println!("{} {} dial {} failed: {}", sty.err("✖"), cam.name, addr, e);
                continue;
            }
            Ok(Ok(_stream)) => {}
        }

        let url = match resolve_rtsp_url(cam, None, None) {
            Ok(u) => u,
            Err(e) => {
                println!("{} {} RTSP URL invalid: {}", sty.err("✖"), cam.name, e);
                continue;
            }
        };

        if args.probe {
            let probe_timeout = timeout_dur + Duration::from_secs(2);
            if let Err(e) = probe_rtsp(&url, probe_timeout, auth_str, transport_str).await {
                println!("{} {} probe failed: {}", sty.err("✖"), cam.name, e);
                continue;
            }
        }

        println!("{} {} reachable at {}", sty.ok("✔"), cam.name, addr);
    }

    Ok(())
}
