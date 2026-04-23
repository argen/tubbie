#![deny(unsafe_code)]

//! `tfl-board` — `BoardService` with filtering, polling stream, and stale-data fallback.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use tfl_board::{BoardService, BoardConfig};
//! use tfl_client::{TflClient, fixture::FixtureTflHttp, clock::SystemClock};
//!
//! let client = TflClient::new(FixtureTflHttp::new("fixtures/"));
//! let service = BoardService::new(client, SystemClock);
//! let cfg = BoardConfig::new("940GZZLUBZP");
//! let board = service.refresh(&cfg).await?;
//! ```

pub mod config;
pub mod error;
pub mod filter;
pub mod service;

pub use config::{BoardConfig, VALID_THEME_IDS};
pub use error::BoardError;
pub use service::BoardService;
