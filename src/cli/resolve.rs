use std::time::Duration;

use crate::config::{Camera, Config};
use crate::errors::QamError;
use crate::rtsp_url::build_url;
use crate::timefmt::rfc3339_nano;

/// Replaces the last path segment of an RTSP URL with `stream`, matching Go `appendStream`.
pub fn append_stream(base_url: &str, stream: &str) -> String {
    if stream.is_empty() {
        return base_url.to_string();
    }
    let stream_part = if stream.starts_with('/') {
        stream.to_string()
    } else {
        format!("/{stream}")
    };
    if let Some(idx) = base_url.rfind('/') {
        format!("{}{}", &base_url[..idx], stream_part)
    } else {
        format!("{base_url}{stream_part}")
    }
}

/// Replaces the trailing path segment of an RTSP URL with `path`, matching Go `appendPath`.
pub fn append_path(base_url: &str, path: &str) -> String {
    if path.is_empty() {
        return base_url.to_string();
    }
    let path_part = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    if let Some(idx) = base_url.rfind('/') {
        format!("{}{}", &base_url[..idx], path_part)
    } else {
        format!("{base_url}{path_part}")
    }
}

/// Resolves the final RTSP target URL from camera and CLI flags.
pub fn resolve_rtsp_url(
    cam: &Camera,
    flag_stream: Option<&str>,
    flag_path: Option<&str>,
) -> Result<String, QamError> {
    let flag_stream = flag_stream.unwrap_or("").trim();
    let flag_path = flag_path.unwrap_or("").trim();

    if !flag_stream.is_empty() && !flag_path.is_empty() {
        return Err(QamError::Usage(
            "use --path for custom RTSP token URLs; omit --stream".to_string(),
        ));
    }

    let mut effective_cam = cam.clone();
    if !flag_path.is_empty() {
        effective_cam.path = flag_path.to_string();
        effective_cam.stream = String::new();
    } else if !effective_cam.path.is_empty() {
        effective_cam.stream = String::new();
    } else if !flag_stream.is_empty() {
        effective_cam.stream = flag_stream.to_string();
    }

    let base = build_url(&effective_cam)?;
    if !flag_path.is_empty() {
        Ok(append_path(&base, flag_path))
    } else if !flag_stream.is_empty() {
        Ok(append_stream(&base, flag_stream))
    } else if !effective_cam.stream.is_empty() {
        Ok(append_stream(&base, &effective_cam.stream))
    } else {
        Ok(base)
    }
}

/// Resolves the RTSP transport: Flag (if explicitly provided) > Camera > Config Defaults > default ("tcp").
pub fn resolve_transport(
    flag: Option<&str>,
    cam: &Camera,
    cfg: &Config,
) -> Result<String, QamError> {
    let raw = if let Some(f) = flag {
        let trimmed = f.trim();
        if !trimmed.is_empty() {
            trimmed
        } else if !cam.rtsp_transport.is_empty() {
            cam.rtsp_transport.as_str()
        } else if !cfg.defaults.rtsp_transport.is_empty() {
            cfg.defaults.rtsp_transport.as_str()
        } else {
            "tcp"
        }
    } else if !cam.rtsp_transport.is_empty() {
        cam.rtsp_transport.as_str()
    } else if !cfg.defaults.rtsp_transport.is_empty() {
        cfg.defaults.rtsp_transport.as_str()
    } else {
        "tcp"
    };

    match raw.to_ascii_lowercase().as_str() {
        "tcp" => Ok("tcp".to_string()),
        "udp" => Ok("udp".to_string()),
        _ => Err(QamError::Usage(
            "invalid --rtsp-transport (use tcp|udp)".to_string(),
        )),
    }
}

/// Resolves the RTSP auth scheme: Flag (if explicitly provided) > Camera > Config Defaults > default ("auto").
pub fn resolve_auth(flag: Option<&str>, cam: &Camera, cfg: &Config) -> Result<String, QamError> {
    let raw = if let Some(f) = flag {
        let trimmed = f.trim();
        if !trimmed.is_empty() {
            trimmed
        } else if !cam.rtsp_auth.is_empty() {
            cam.rtsp_auth.as_str()
        } else if !cfg.defaults.rtsp_auth.is_empty() {
            cfg.defaults.rtsp_auth.as_str()
        } else {
            "auto"
        }
    } else if !cam.rtsp_auth.is_empty() {
        cam.rtsp_auth.as_str()
    } else if !cfg.defaults.rtsp_auth.is_empty() {
        cfg.defaults.rtsp_auth.as_str()
    } else {
        "auto"
    };

    match raw.to_ascii_lowercase().as_str() {
        "auto" | "" => Ok("auto".to_string()),
        "basic" => Ok("basic".to_string()),
        "digest" => Ok("digest".to_string()),
        _ => Err(QamError::Usage(
            "invalid --rtsp-auth (use auto|basic|digest)".to_string(),
        )),
    }
}

/// Resolves timeout duration: Flag (if explicitly provided) > Config Defaults > default (10s or custom).
pub fn resolve_timeout(flag: Option<Duration>, cfg: &Config, fallback: Duration) -> Duration {
    if let Some(d) = flag
        && !d.is_zero()
    {
        return d;
    }
    if !cfg.defaults.timeout.is_zero() {
        cfg.defaults.timeout
    } else {
        fallback
    }
}

/// Parses motion scene score from log line, matching Go `parseSceneScore`.
/// Example: `[Parsed_metadata_1 ...] scene_score=0.123`
pub fn parse_scene_score(line: &str) -> Option<f64> {
    let key = "scene_score=";
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let num_end = rest
        .find(|c: char| c != '.' && c != '-' && !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let part = &rest[..num_end];
    part.parse::<f64>().ok()
}

/// Applies `{camera}`, `{score}`, and `{time}` template parameters, matching Go `applyTemplate`.
pub fn apply_template<Tz>(tmpl: &str, camera: &str, score: f64, t: chrono::DateTime<Tz>) -> String
where
    Tz: chrono::TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let score_str = format!("{:.3}", score);
    let time_str = rfc3339_nano(t);
    tmpl.replace("{camera}", camera)
        .replace("{score}", &score_str)
        .replace("{time}", &time_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_stream() {
        let base = "rtsp://user:pass@192.168.0.10:554/stream1";
        assert_eq!(
            append_stream(base, "stream2"),
            "rtsp://user:pass@192.168.0.10:554/stream2"
        );
        assert_eq!(
            append_stream(base, "/stream2"),
            "rtsp://user:pass@192.168.0.10:554/stream2"
        );
        assert_eq!(append_stream(base, ""), base);
    }

    #[test]
    fn test_append_path() {
        let base = "rtsp://192.168.1.1:7447/stream1";
        assert_eq!(append_path(base, "/Bfy47"), "rtsp://192.168.1.1:7447/Bfy47");
        assert_eq!(append_path(base, "Bfy47"), "rtsp://192.168.1.1:7447/Bfy47");
        assert_eq!(append_path(base, ""), base);
    }

    #[test]
    fn test_parse_scene_score() {
        let line = "[Parsed_metadata_1] scene_score=0.321 something";
        let score = parse_scene_score(line).expect("expected score");
        assert!((score - 0.321).abs() < 1e-6);

        assert!(parse_scene_score("no score here").is_none());
    }

    #[test]
    fn test_apply_template() {
        use chrono::TimeZone;
        let t = chrono::Utc
            .with_ymd_and_hms(2026, 11, 12, 12, 0, 0)
            .unwrap();
        let rendered = apply_template("echo {camera} {score} {time}", "front", 0.4567, t);
        assert_eq!(rendered, "echo front 0.457 2026-11-12T12:00:00Z");
    }

    #[test]
    fn test_precedence_resolution() {
        let mut cfg = Config::default();
        cfg.defaults.rtsp_transport = "udp".to_string();
        cfg.defaults.rtsp_auth = "digest".to_string();

        let mut cam = Camera {
            name: "test".into(),
            host: "10.0.0.1".into(),
            ..Default::default()
        };

        // Fallback to cfg defaults
        assert_eq!(resolve_transport(None, &cam, &cfg).unwrap(), "udp");
        assert_eq!(resolve_auth(None, &cam, &cfg).unwrap(), "digest");

        // Camera overrides cfg defaults
        cam.rtsp_transport = "tcp".to_string();
        cam.rtsp_auth = "basic".to_string();
        assert_eq!(resolve_transport(None, &cam, &cfg).unwrap(), "tcp");
        assert_eq!(resolve_auth(None, &cam, &cfg).unwrap(), "basic");

        // Flag overrides camera
        assert_eq!(resolve_transport(Some("udp"), &cam, &cfg).unwrap(), "udp");
        assert_eq!(resolve_auth(Some("digest"), &cam, &cfg).unwrap(), "digest");
    }
}
