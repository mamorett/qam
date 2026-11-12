use crate::errors::QamError;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RtspAuth {
    #[default]
    Auto,
    Basic,
    Digest,
}

impl RtspAuth {
    pub fn as_str(&self) -> &'static str {
        match self {
            RtspAuth::Auto => "auto",
            RtspAuth::Basic => "basic",
            RtspAuth::Digest => "digest",
        }
    }
}

impl FromStr for RtspAuth {
    type Err = QamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(RtspAuth::Auto),
            "basic" => Ok(RtspAuth::Basic),
            "digest" => Ok(RtspAuth::Digest),
            _ => Err(QamError::Usage(
                "invalid --rtsp-auth (use auto|basic|digest)".to_string(),
            )),
        }
    }
}

/// Matches Go's `parseRTSPAuth(mode string) (string, bool)` helper.
pub fn parse_rtsp_auth(mode: &str) -> (String, bool) {
    match mode.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => (String::new(), true),
        "basic" => ("basic".to_string(), true),
        "digest" => ("digest".to_string(), true),
        _ => (String::new(), false),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    #[default]
    Tcp,
    Udp,
}

impl Transport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Transport::Tcp => "tcp",
            Transport::Udp => "udp",
        }
    }
}

impl FromStr for Transport {
    type Err = QamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "tcp" => Ok(Transport::Tcp),
            "udp" => Ok(Transport::Udp),
            _ => Err(QamError::Usage(
                "invalid --rtsp-transport (use tcp|udp)".to_string(),
            )),
        }
    }
}

/// Matches Go's `transportFlag(v string) (string, bool)` helper.
pub fn transport_flag(v: &str) -> (String, bool) {
    if v.trim().is_empty() {
        return ("tcp".to_string(), true);
    }
    match v.trim().to_ascii_lowercase().as_str() {
        "tcp" => ("tcp".to_string(), true),
        "udp" => ("udp".to_string(), true),
        _ => (String::new(), false),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientMode {
    #[default]
    Standard,
    FirstKeyframe,
}

impl FromStr for ClientMode {
    type Err = QamError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "standard" | "ffmpeg" => Ok(ClientMode::Standard),
            "first-keyframe" | "firstkeyframe" | "gortsplib" => Ok(ClientMode::FirstKeyframe),
            other => Err(QamError::Usage(format!(
                "invalid --rtsp-client {other:?} (use standard|first-keyframe)"
            ))),
        }
    }
}

/// Normalizes CLI boolean arguments so `--flag=true`, `--flag=false`, `--flag=1`, `--flag=0`
/// are cleanly handled before or during CLI parsing.
pub fn normalize_bool_flag_args<I, T>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString>,
{
    let mut out = Vec::new();
    for arg in args {
        let s = arg.into().to_string_lossy().into_owned();
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rtsp_auth_go_cases() {
        let cases = [
            ("auto", true, ""),
            ("basic", true, "basic"),
            ("digest", true, "digest"),
            ("", true, ""),
            ("weird", false, ""),
        ];
        for (input, ok, expect) in cases {
            let (got, got_ok) = parse_rtsp_auth(input);
            assert_eq!(got_ok, ok, "parse_rtsp_auth({input:?}) ok");
            assert_eq!(got, expect, "parse_rtsp_auth({input:?}) val");
        }
    }

    #[test]
    fn test_transport_flag_go_cases() {
        let cases = [
            ("", true, "tcp"),
            ("tcp", true, "tcp"),
            ("udp", true, "udp"),
            ("something", false, ""),
        ];
        for (input, ok, expect) in cases {
            let (got, got_ok) = transport_flag(input);
            assert_eq!(got_ok, ok, "transport_flag({input:?}) ok");
            assert_eq!(got, expect, "transport_flag({input:?}) val");
        }
    }
}
