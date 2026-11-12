use std::str::FromStr;
use std::time::Duration;

use crate::cli::flags::{RtspAuth, Transport, parse_rtsp_auth, transport_flag};
use crate::media::session::{FrameFormat, RtspSession};

/// Probes an RTSP URL up to 3 times to verify that the stream can be connected,
/// setup, played, and that at least one complete media frame is received.
pub async fn probe_rtsp(
    url: &str,
    attempt_timeout: Duration,
    auth_str: &str,
    transport_str: &str,
) -> Result<(), String> {
    let (_, auth_ok) = parse_rtsp_auth(auth_str);
    if !auth_ok {
        return Err("invalid --rtsp-auth (use auto|basic|digest)".to_string());
    }
    let (_, xport_ok) = transport_flag(transport_str);
    if !xport_ok {
        return Err("invalid --rtsp-transport (use tcp|udp)".to_string());
    }

    let transport = Transport::from_str(transport_str).unwrap_or(Transport::Tcp);
    let auth = RtspAuth::from_str(auth_str).unwrap_or(RtspAuth::Auto);

    let mut last_err = String::new();

    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let connect_fut = RtspSession::connect(
            url,
            transport,
            auth,
            attempt_timeout,
            false,
            FrameFormat::SIMPLE,
        );

        let mut session = match tokio::time::timeout(attempt_timeout, connect_fut).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                last_err = e.to_string();
                continue;
            }
            Err(_) => {
                last_err = "timed out connecting to RTSP stream".to_string();
                continue;
            }
        };

        // Try to receive >= 1 complete frame
        let frame_fut = session.next_frame();
        match tokio::time::timeout(attempt_timeout, frame_fut).await {
            Ok(Ok(Some(_frame))) => {
                let _ = session.teardown().await;
                return Ok(());
            }
            Ok(Ok(None)) => {
                last_err = "stream ended without frames".to_string();
                let _ = session.teardown().await;
                continue;
            }
            Ok(Err(e)) => {
                last_err = e.to_string();
                let _ = session.teardown().await;
                continue;
            }
            Err(_) => {
                last_err = "timed out waiting for frame".to_string();
                let _ = session.teardown().await;
                continue;
            }
        }
    }

    let class = crate::errors::classify_message(&last_err);
    Err(format!("{last_err} ({class})"))
}
