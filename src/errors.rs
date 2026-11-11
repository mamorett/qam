use thiserror::Error;

#[derive(Debug, Error)]
pub enum QamError {
    #[error("{0}")]
    Usage(String),

    #[error("{0}")]
    Config(String),

    #[error("{0}")]
    RtspUrl(String),

    #[error("{0}")]
    Media(String),

    #[error("{0}")]
    Discovery(String),

    #[error("{0}")]
    Slack(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Interrupted(i32),
}

impl QamError {
    pub fn exit_code(&self) -> u8 {
        match self {
            QamError::Interrupted(code) => *code as u8,
            _ => 1,
        }
    }
}
