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

/// Classifies an error message into a canonical error category string, matching Go `ClassifyError`.
pub fn classify_message(s: &str) -> &'static str {
    let msg = s.to_ascii_lowercase();
    if msg.contains("401") || msg.contains("unauthorized") || msg.contains("auth") {
        "auth"
    } else if msg.contains("connection refused") {
        "network-refused"
    } else if msg.contains("timed out") || msg.contains("timeout") {
        "network-timeout"
    } else if msg.contains("not found") || msg.contains("404") {
        "not-found"
    } else {
        "unknown"
    }
}

/// Classifies a `QamError` using typed inspection first, then falling back to `classify_message`.
pub fn classify(err: &QamError) -> &'static str {
    match err {
        QamError::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::ConnectionRefused => "network-refused",
            std::io::ErrorKind::TimedOut => "network-timeout",
            std::io::ErrorKind::NotFound => "not-found",
            std::io::ErrorKind::PermissionDenied => "auth",
            _ => classify_message(&err.to_string()),
        },
        _ => classify_message(&err.to_string()),
    }
}

/// Helper to apply a timeout to an async operation, matching Go `WithTimeout` where 0 means no timeout.
pub async fn with_timeout<F, T>(
    duration: std::time::Duration,
    future: F,
) -> Result<T, tokio::time::error::Elapsed>
where
    F: std::future::Future<Output = T>,
{
    if duration.is_zero() {
        Ok(future.await)
    } else {
        tokio::time::timeout(duration, future).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_error_table() {
        let cases = [
            ("401 Unauthorized", "auth"),
            ("Server returned 401 unauthorized", "auth"),
            ("Connection refused", "network-refused"),
            ("timed out", "network-timeout"),
            ("not found", "not-found"),
            ("weird message", "unknown"),
        ];
        for (input, want) in cases {
            assert_eq!(classify_message(input), want, "classify_message({input:?})");
            let qam_err = QamError::Media(input.to_string());
            assert_eq!(classify(&qam_err), want, "classify({input:?})");
        }
    }

    #[test]
    fn test_classify_typed_io() {
        let refused = QamError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "os error 61",
        ));
        assert_eq!(classify(&refused), "network-refused");

        let timed_out = QamError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timed out waiting",
        ));
        assert_eq!(classify(&timed_out), "network-timeout");

        let not_found = QamError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert_eq!(classify(&not_found), "not-found");
    }

    #[tokio::test]
    async fn test_with_timeout_zero() {
        let res = with_timeout(std::time::Duration::ZERO, async { 42 }).await;
        assert_eq!(res, Ok(42));
    }
}
