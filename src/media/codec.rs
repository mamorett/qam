use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    H264,
    H265,
    Mjpeg,
}

impl fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VideoCodec::H264 => write!(f, "H.264"),
            VideoCodec::H265 => write!(f, "H.265"),
            VideoCodec::Mjpeg => write!(f, "MJPEG"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioCodec {
    Aac,
    Opus,
    Pcma,
    Pcmu,
    L16,
    G726,
}

impl fmt::Display for AudioCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioCodec::Aac => write!(f, "AAC"),
            AudioCodec::Opus => write!(f, "Opus"),
            AudioCodec::Pcma => write!(f, "PCMA"),
            AudioCodec::Pcmu => write!(f, "PCMU"),
            AudioCodec::L16 => write!(f, "L16"),
            AudioCodec::G726 => write!(f, "G.726"),
        }
    }
}

impl AudioCodec {
    pub fn is_g711(&self) -> bool {
        matches!(self, AudioCodec::Pcma | AudioCodec::Pcmu)
    }
}

/// Checks whether a container extension natively supports copying an audio codec.
pub fn container_supports_copy(ext: &str, codec: AudioCodec) -> bool {
    let ext_lower = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match ext_lower.as_str() {
        "mp4" | "m4a" => matches!(codec, AudioCodec::Aac | AudioCodec::Opus),
        "mov" => matches!(
            codec,
            AudioCodec::Aac
                | AudioCodec::Opus
                | AudioCodec::Pcma
                | AudioCodec::Pcmu
                | AudioCodec::L16
        ),
        "wav" => matches!(
            codec,
            AudioCodec::Pcma | AudioCodec::Pcmu | AudioCodec::L16 | AudioCodec::G726
        ),
        "aac" => matches!(codec, AudioCodec::Aac),
        _ => false,
    }
}
