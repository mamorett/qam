use crate::config::Camera;
use crate::errors::QamError;

const DEFAULT_PORT: u16 = 554;
const DEFAULT_PATH: &str = "/stream1";

/// Percent-encodes username or password matching Go's `url.UserPassword` escaping.
///
/// Unreserved characters kept unencoded: `A-Z`, `a-z`, `0-9`, `-`, `.`, `_`, `~`, `$`, `&`, `+`, `,`, `;`, `=`.
/// Everything else (including `:`, `@`, `/`, `?`, space, `!`, `*`, `'`, `(`, `)`, `%`, `#`, `[`, `]`, and non-ASCII)
/// is percent-encoded with uppercase hex digits (`%XX`).
pub fn encode_user_password(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'.' | b'_' | b'~' | b'$' | b'&' | b'+' | b',' | b';' | b'='
            )
        {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{:02X}", b);
        }
    }
    out
}

/// Builds an RTSP URL for a camera, matching Go's `rtsp.BuildURL`.
pub fn build_url(cam: &Camera) -> Result<String, QamError> {
    if cam.host.trim().is_empty() {
        return Err(QamError::RtspUrl("host is required".to_string()));
    }
    let raw_host = cam.host.trim();

    // If host contains no ':', append port (default 554). If it contains ':', use verbatim.
    let host_port = if !raw_host.contains(':') {
        let port = if cam.port != 0 {
            cam.port
        } else {
            DEFAULT_PORT
        };
        format!("{raw_host}:{port}")
    } else {
        raw_host.to_string()
    };

    let user_info = if !cam.user.is_empty() {
        let enc_user = encode_user_password(&cam.user);
        if !cam.pass.is_empty() {
            let enc_pass = encode_user_password(&cam.pass);
            format!("{enc_user}:{enc_pass}@")
        } else {
            format!("{enc_user}@")
        }
    } else {
        String::new()
    };

    let proto = if cam.scheme.is_empty() {
        "rtsp".to_string()
    } else {
        cam.scheme.clone()
    };

    match proto.to_ascii_lowercase().as_str() {
        "rtsp" | "rtsps" => {}
        _ => {
            return Err(QamError::RtspUrl(format!(
                "unsupported protocol {:?}",
                proto
            )));
        }
    }

    let mut path = if cam.path.is_empty() {
        DEFAULT_PATH.to_string()
    } else {
        cam.path.clone()
    };
    if !path.starts_with('/') {
        path.insert(0, '/');
    }

    Ok(format!("{proto}://{user_info}{host_port}{path}"))
}

/// Redacts passwords in RTSP URLs by replacing the password component with `***`.
pub fn redact_url(url_str: &str) -> String {
    if let Ok(mut u) = url::Url::parse(url_str) {
        if u.password().is_some() {
            let _ = u.set_password(Some("***"));
        }
        return u.to_string();
    }

    // Fallback if URL parsing fails: search for :// ... : ... @
    if let Some(proto_end) = url_str.find("://") {
        let rest = &url_str[proto_end + 3..];
        if let Some(at_idx) = rest.find('@') {
            let userinfo = &rest[..at_idx];
            if let Some(colon_idx) = userinfo.find(':') {
                let user = &userinfo[..colon_idx];
                let after_at = &rest[at_idx..];
                return format!("{}://{}:***{}", &url_str[..proto_end], user, after_at);
            }
        }
    }
    url_str.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url() {
        let cam = Camera {
            name: "cam1".into(),
            host: "192.168.1.50".into(),
            port: 554,
            scheme: "rtsp".into(),
            user: "user".into(),
            pass: "pass".into(),
            ..Default::default()
        };
        let got = build_url(&cam).expect("build_url");
        assert_eq!(got, "rtsp://user:pass@192.168.1.50:554/stream1");
    }

    #[test]
    fn test_build_url_invalid_protocol() {
        let cam = Camera {
            name: "cam3".into(),
            host: "10.0.0.3".into(),
            scheme: "http".into(),
            ..Default::default()
        };
        assert!(build_url(&cam).is_err());
    }

    #[test]
    fn test_build_url_defaults() {
        let cam = Camera {
            name: "cam2".into(),
            host: "10.0.0.2".into(),
            ..Default::default()
        };
        let got = build_url(&cam).expect("build_url");
        assert_eq!(got, "rtsp://10.0.0.2:554/stream1");
    }

    #[test]
    fn test_build_url_with_path() {
        let cam = Camera {
            name: "protect".into(),
            host: "192.168.1.1".into(),
            port: 7447,
            scheme: "rtsp".into(),
            path: "/Bfy47SNWz9n2WRrw".into(),
            ..Default::default()
        };
        let got = build_url(&cam).expect("build_url");
        assert_eq!(got, "rtsp://192.168.1.1:7447/Bfy47SNWz9n2WRrw");
    }

    #[test]
    fn test_build_url_escaping() {
        let cam = Camera {
            name: "escape_test".into(),
            host: "10.0.0.1".into(),
            user: "user$name&1".into(),
            pass: "p@ss:w/rd? space!".into(),
            ..Default::default()
        };
        let got = build_url(&cam).expect("build_url");
        assert_eq!(
            got,
            "rtsp://user$name&1:p%40ss%3Aw%2Frd%3F%20space%21@10.0.0.1:554/stream1"
        );
    }

    #[test]
    fn test_redact_url() {
        assert_eq!(
            redact_url("rtsp://admin:secret123@192.168.1.10:554/stream1"),
            "rtsp://admin:***@192.168.1.10:554/stream1"
        );
        assert_eq!(
            redact_url("rtsp://admin@192.168.1.10:554/stream1"),
            "rtsp://admin@192.168.1.10:554/stream1"
        );
        assert_eq!(
            redact_url("rtsp://192.168.1.10:554/stream1"),
            "rtsp://192.168.1.10:554/stream1"
        );
        // Fallback for non-standard scheme or parse failure
        assert_eq!(
            redact_url("custom://usr:pw@host:123/p"),
            "custom://usr:***@host:123/p"
        );
    }
}
