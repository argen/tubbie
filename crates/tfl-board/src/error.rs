use tfl_client::error::TflError;
use thiserror::Error;

/// Errors emitted by `BoardService`.
///
/// Stale-data transitions (TfL fetch failures after a successful fetch) are
/// NOT errors — they are represented as `Board::stale_since: Some(...)`.
/// Only programmer-level failures or catastrophic conditions surface here.
#[derive(Debug, Error)]
pub enum BoardError {
    /// The underlying TfL client returned an error on the very first fetch
    /// (before any successful board was produced). Subsequent failures after
    /// the first success become stale-board state, not errors.
    #[error("TfL fetch failed: {0}")]
    Fetch(#[from] TflError),

    /// A required configuration value was invalid (e.g. empty station_id).
    #[error("invalid board configuration: {0}")]
    InvalidConfig(String),
}
