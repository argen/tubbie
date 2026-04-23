use thiserror::Error;

/// Errors that can occur when fetching from TfL.
#[derive(Debug, Error)]
pub enum TflError {
    /// The requested resource was not found (e.g. missing fixture, 404 from live API).
    #[error("not found: {0}")]
    NotFound(String),

    /// A transport-level error (network failure, TLS error, etc.).
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// Response body could not be parsed as JSON.
    #[error("parse error: {0}")]
    Parse(#[from] serde_json::Error),

    /// I/O error (e.g. reading a fixture file from disk).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
