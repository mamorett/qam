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

/// Parses a boolean flag value, supporting "true", "false", "1", "0", "t", "f", "yes", "no".
pub fn parse_bool_flag(val: &str) -> Result<bool, String> {
    match val.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "t" | "yes" | "y" => Ok(true),
        "false" | "0" | "f" | "no" | "n" => Ok(false),
        other => Err(format!("invalid boolean value {other:?}")),
    }
}

/// Creates a clap boolean flag that supports `--flag`, `--flag=true`, `--flag=false`, `--flag=1`, `--flag=0`.
pub fn bool_arg(name: &'static str, help: &'static str) -> clap::Arg {
    clap::Arg::new(name)
        .long(name)
        .help(help)
        .action(clap::ArgAction::Set)
        .num_args(0..=1)
        .default_missing_value("true")
        .require_equals(true)
        .value_parser(parse_bool_flag)
}

/// Creates a trailing ignored positional arg for subcommands that silently ignore extra positionals.
pub fn trailing_ignored_arg() -> clap::Arg {
    clap::Arg::new("trailing_ignored")
        .num_args(0..)
        .trailing_var_arg(true)
        .hide(true)
}

/// Parses a duration string in Go syntax (e.g., 5s, 10m, 500ms).
pub fn parse_duration_flag(val: &str) -> Result<std::time::Duration, String> {
    crate::timefmt::parse_go_duration(val).map_err(|_| format!("invalid duration {val:?}"))
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

    #[test]
    fn test_parse_bool_flag() {
        assert_eq!(parse_bool_flag("true"), Ok(true));
        assert_eq!(parse_bool_flag("1"), Ok(true));
        assert_eq!(parse_bool_flag("t"), Ok(true));
        assert_eq!(parse_bool_flag("false"), Ok(false));
        assert_eq!(parse_bool_flag("0"), Ok(false));
        assert_eq!(parse_bool_flag("f"), Ok(false));
        assert!(parse_bool_flag("invalid").is_err());
    }

    #[test]
    fn test_parse_duration_flag() {
        assert_eq!(
            parse_duration_flag("10s"),
            Ok(std::time::Duration::from_secs(10))
        );
        assert_eq!(
            parse_duration_flag("500ms"),
            Ok(std::time::Duration::from_millis(500))
        );
        assert!(parse_duration_flag("invalid").is_err());
    }
}
