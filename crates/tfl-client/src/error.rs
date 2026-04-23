use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

/// Errors that can occur when fetching from TfL.
#[derive(Debug, Error)]
pub enum TflError {
    /// The requested resource was not found (e.g. missing fixture, 404 from live API).
    #[error("not found: {0}")]
    NotFound(String),

    /// A transport-level error (network failure, TLS error, 5xx response).
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// Response body could not be parsed as JSON (no path context available).
    #[error("parse error: {0}")]
    Parse(#[from] serde_json::Error),

    /// Failed to parse a fixture file — includes the path for easy debugging.
    #[error("failed to parse fixture at {path}: {source}")]
    ParseAt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// I/O error (e.g. reading a fixture file from disk).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The server returned HTTP 429 Too Many Requests.
    #[error("rate limited by TfL API (retry after: {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },

    /// An unexpected HTTP error (non-2xx, non-404, non-429, non-5xx).
    ///
    /// `body_snippet` is truncated to ≤512 chars. The URL is NOT included to
    /// avoid leaking any app_key that may appear in request URLs.
    #[error("HTTP {status} from TfL API: {body_snippet}")]
    Http { status: u16, body_snippet: String },
}
