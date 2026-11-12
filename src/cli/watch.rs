use std::str::FromStr;
use std::time::Duration;

use crate::cli::flags::{RtspAuth, Transport, parse_rtsp_auth, transport_flag};
use crate::cli::resolve::{append_path, append_stream, apply_template};
use crate::config;
use crate::errors::QamError;
use crate::media::scene::SceneDetector;
use crate::media::session::{FrameFormat, MediaPayload, RtspSession};
use crate::media::video_decode::VideoDecoder;
use crate::rtsp_url::build_url;
use crate::timefmt::{go_quote, parse_go_duration, rfc3339_nano};

#[derive(Debug, Default)]
pub struct WatchArgs {
    pub camera: Option<String>,
    pub camera_pos: Option<String>,
    pub action: Option<String>,
    pub action_template: Option<String>,
    pub threshold: Option<f64>,
    pub cooldown: Option<String>,
    pub duration: Option<String>,
    pub json: bool,
    pub path: Option<String>,
    pub rtsp_auth: Option<String>,
    pub rtsp_transport: Option<String>,
    pub stream: Option<String>,
    pub config: Option<String>,
}

pub async fn run_watch(args: WatchArgs) -> Result<(), QamError> {
    let camera_name = args
        .camera
        .or(args.camera_pos)
        .unwrap_or_default()
        .trim()
        .to_string();

    if camera_name.is_empty() {
        return Err(QamError::Usage("--camera is required".into()));
    }

    let action = args.action.unwrap_or_default().trim().to_string();
    if action.is_empty() {
        return Err(QamError::Usage(
            "--action is required (e.g., \"say motion\" or \"touch /tmp/motion\")".into(),
        ));
    }

    let threshold = args.threshold.unwrap_or(0.2);
    if threshold <= 0.0 || threshold >= 1.0 {
        return Err(QamError::Usage(
            "--threshold must be between 0 and 1 (e.g., 0.2)".into(),
        ));
    }

    if let Some(ref a) = args.rtsp_auth
        && !a.is_empty()
    {
        let (_, ok) = parse_rtsp_auth(a);
        if !ok {
            return Err(QamError::Usage(
                "invalid --rtsp-auth (use auto|basic|digest)".into(),
            ));
        }
    }

    if let Some(ref t) = args.rtsp_transport
        && !t.is_empty()
    {
        let (_, ok) = transport_flag(t);
        if !ok {
            return Err(QamError::Usage(
                "invalid --rtsp-transport (use tcp|udp)".into(),
            ));
        }
    }

    let cfg = if let Some(p) = args.config.as_deref() {
        config::load(p)?
    } else if let Ok(p) = config::default_path() {
        config::load(p.to_str().unwrap_or_default()).unwrap_or_default()
    } else {
        config::Config::default()
    };

    let cam = cfg
        .find_camera(&camera_name)
        .ok_or_else(|| QamError::Usage(format!("camera {camera_name:?} not found")))?;

    let flag_stream = args.stream.as_deref().unwrap_or("").trim();
    let flag_path = args.path.as_deref().unwrap_or("").trim();

    if !flag_stream.is_empty() && !flag_path.is_empty() {
        return Err(QamError::Usage(
            "use --path for custom RTSP token URLs; omit --stream".into(),
        ));
    }

    let mut effective_cam = cam.clone();
    let final_path = if !flag_path.is_empty() {
        flag_path.to_string()
    } else if !effective_cam.path.is_empty() {
        effective_cam.path.clone()
    } else {
        String::new()
    };
    if !final_path.is_empty() {
        effective_cam.path = final_path.clone();
        effective_cam.stream = String::new();
    }

    let mut url = build_url(&effective_cam)?;
    if !final_path.is_empty() {
        url = append_path(&url, &final_path);
    } else if !flag_stream.is_empty() {
        url = append_stream(&url, flag_stream);
    } else if !effective_cam.stream.is_empty() {
        url = append_stream(&url, &effective_cam.stream);
    }

    let tmpl = args.action_template.unwrap_or_default();
    let mut initial_action = action.clone();
    if !tmpl.is_empty() {
        initial_action = apply_template(&tmpl, &effective_cam.name, 0.0, chrono::Local::now());
    }

    let cooldown = if let Some(ref c) = args.cooldown {
        let trimmed = c.trim();
        if trimmed.is_empty() {
            Duration::from_secs(5)
        } else {
            parse_go_duration(trimmed)
                .map_err(|_| QamError::Usage(format!("invalid duration {c:?}")))?
        }
    } else {
        Duration::from_secs(5)
    };

    let runtime = if let Some(ref d) = args.duration {
        let trimmed = d.trim();
        if trimmed.is_empty() || trimmed == "0" || trimmed == "0s" {
            Duration::ZERO
        } else {
            parse_go_duration(trimmed)
                .map_err(|_| QamError::Usage(format!("invalid duration {d:?}")))?
        }
    } else {
        Duration::ZERO
    };

    let deadline = if !runtime.is_zero() {
        Some(tokio::time::Instant::now() + runtime)
    } else {
        None
    };

    let transport = if let Some(ref t) = args.rtsp_transport {
        if t != "tcp" && !t.is_empty() {
            Transport::from_str(t)?
        } else if !cam.rtsp_transport.is_empty() {
            Transport::from_str(&cam.rtsp_transport)?
        } else {
            Transport::Tcp
        }
    } else if !cam.rtsp_transport.is_empty() {
        Transport::from_str(&cam.rtsp_transport)?
    } else {
        Transport::Tcp
    };

    let auth = if let Some(ref a) = args.rtsp_auth {
        if a != "auto" && !a.is_empty() {
            RtspAuth::from_str(a)?
        } else if !cam.rtsp_auth.is_empty() {
            RtspAuth::from_str(&cam.rtsp_auth)?
        } else {
            RtspAuth::Auto
        }
    } else if !cam.rtsp_auth.is_empty() {
        RtspAuth::from_str(&cam.rtsp_auth)?
    } else {
        RtspAuth::Auto
    };

    let mut session = RtspSession::connect(
        &url,
        transport,
        auth,
        Duration::from_secs(10),
        false,
        FrameFormat::SIMPLE,
    )
    .await?;

    let _video_stream_id = session
        .video_stream_id
        .ok_or_else(|| QamError::Media("no video stream found in RTSP presentation".into()))?;
    let video_codec = session
        .video_codec
        .ok_or_else(|| QamError::Media("unsupported video codec in RTSP presentation".into()))?;

    let mut decoder = VideoDecoder::new(video_codec)?;
    let mut scene_detector = SceneDetector::new();
    let mut last_trigger: Option<chrono::DateTime<chrono::Local>> = None;

    loop {
        if let Some(dl) = deadline
            && tokio::time::Instant::now() >= dl
        {
            break;
        }

        let frame_fut = session.next_frame();
        let frame_res = if let Some(dl) = deadline {
            match tokio::time::timeout_at(dl, frame_fut).await {
                Ok(res) => res,
                Err(_) => break, // Runtime expired
            }
        } else {
            frame_fut.await
        };

        let frame = match frame_res {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                let class = crate::errors::classify_message(&e.to_string());
                return Err(QamError::Media(format!("stream ended: {e} ({class})")));
            }
        };

        if let MediaPayload::Video { ref data, .. } = frame.payload {
            let decoded_y = match decoder.decode_y_plane(data) {
                Ok(Some(y)) => y,
                Ok(None) => continue,
                Err(_) => continue,
            };

            let score = scene_detector.feed(&decoded_y.y_plane, decoded_y.width, decoded_y.height);
            if score > threshold {
                let now = chrono::Local::now();
                let should_trigger = match last_trigger {
                    None => true,
                    Some(last) => {
                        let elapsed = now.signed_duration_since(last);
                        elapsed.to_std().map(|d| d >= cooldown).unwrap_or(true)
                    }
                };

                if should_trigger {
                    last_trigger = Some(now);
                    let time_str = rfc3339_nano(now);

                    if args.json {
                        println!(
                            r#"{{"event":"motion","camera":"{}","score":{:.3},"time":"{}"}}"#,
                            effective_cam.name, score, time_str
                        );
                    } else {
                        println!(
                            "event=motion camera={} score={:.3} action={} time={}",
                            effective_cam.name,
                            score,
                            go_quote(&initial_action),
                            time_str
                        );
                    }

                    let act_to_run = if !tmpl.is_empty() {
                        apply_template(&tmpl, &effective_cam.name, score, now)
                    } else {
                        action.clone()
                    };

                    run_action(&act_to_run, score, now, &effective_cam.name);
                }
            }
        }
    }

    let _ = session.teardown().await;
    Ok(())
}

fn run_action(action: &str, score: f64, t: chrono::DateTime<chrono::Local>, camera: &str) {
    let score_str = format!("{:.3}", score);
    let time_str = rfc3339_nano(t);

    let shell = if cfg!(windows) {
        "cmd"
    } else if std::path::Path::new("/bin/sh").exists() {
        "/bin/sh"
    } else {
        "sh"
    };

    let mut cmd = tokio::process::Command::new(shell);
    if cfg!(windows) {
        cmd.arg("/C").arg(action);
    } else {
        cmd.arg("-c").arg(action);
    }

    cmd.env("CAMSNAP_SCORE", &score_str)
        .env("CAMSNAP_TIME", &time_str)
        .env("CAMSNAP_CAMERA", camera)
        .env("QAM_SCORE", &score_str)
        .env("QAM_TIME", &time_str)
        .env("QAM_CAMERA", camera);

    if let Ok(mut child) = cmd.spawn() {
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
    }
}
