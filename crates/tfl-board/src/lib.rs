#![deny(unsafe_code)]

//! `tfl-board` — `BoardService` with filtering, polling stream, and stale-data fallback.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use tfl_board::{BoardService, BoardConfig};
//! use tfl_cache::TflClient;
//! use tfl_client::fixture::FixtureTflHttp;
//!
//! let client = TflClient::new(FixtureTflHttp::new("fixtures/"));
//! let service = BoardService::new(client, SystemClock);
//! let cfg = BoardConfig::new("940GZZLUBZP");
//! let board = service.refresh(&cfg).await?;
//! ```

pub mod config;
pub mod error;
pub mod filter;
pub mod lifecycle;
pub mod service;
pub mod warm_fallback;

pub use config::{BoardConfig, VALID_THEME_IDS};
pub use error::BoardError;
pub use lifecycle::{AppPhase, LifecyclePhase};
pub use service::BoardService;
pub use warm_fallback::{Timer, TokioSleepTimer, WarmFallback, WarmOutcome};
