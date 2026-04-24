//! Application state injected into Tauri commands via `tauri::State`.
//!
//! `AppState` is the single shared-state type managed by Tauri. It holds:
//! - `board_service`: the `BoardService` wrapping the live TfL client.
//! - `config_store`: a `ConfigStore` trait object for config persistence.
//!
//! Tests substitute `MemoryConfigStore` for deterministic, headless operation.
//! `AppState` is type-erased via `Arc<dyn Any + Send + Sync>` so we can store
//! both `BoardService<ReqwestTflHttp, SystemClock>` (production) and
//! `BoardService<FixtureTflHttp, FakeClock>` (tests) behind the same field.
//!
//! The `board_service` field uses a trait object (`BoxedBoardService`) that
//! exposes only the methods commands need, avoiding generic leakage into Tauri
//! state management.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tfl_board::{BoardConfig, BoardError, BoardService};
use tfl_client::{clock::Clock, http::TflHttp};
use tfl_domain::{Board, LineStatus, Station};
use tokio::{sync::RwLock, task::AbortHandle};

// ---------------------------------------------------------------------------
// ConfigStore trait
// ---------------------------------------------------------------------------

/// Persistence abstraction for app configuration.
///
/// Each method is a compound atomic operation: `save_config` and `save_app_key`
/// both perform the in-memory `set` and the durable `save` under a single lock
/// acquisition, preventing concurrent handlers from observing partial state.
///
/// The production implementation (`StorePluginConfigStore`) wraps the blocking
/// `Store::save()` in `tokio::task::spawn_blocking` so the Tokio worker thread
/// is never stalled by disk I/O.
///
/// Tests use `MemoryConfigStore` for deterministic, headless operation without
/// a Tauri runtime.
#[async_trait]
pub trait ConfigStore: Send + Sync + 'static {
    /// Load the board configuration. Returns the documented default
    /// (Belsize Park, no filters, 20 s poll) if no config has been saved.
    async fn load_config(&self) -> Result<BoardConfig, String>;

    /// Atomically set and persist the board configuration.
    async fn save_config(&self, cfg: &BoardConfig) -> Result<(), String>;

    /// Load the stored TfL API key. Returns `None` if no key has been saved.
    async fn load_app_key(&self) -> Result<Option<String>, String>;

    /// Atomically set and persist the TfL API key (pass `None` to clear).
    async fn save_app_key(&self, key: Option<String>) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// In-memory ConfigStore (used in tests)
// ---------------------------------------------------------------------------

/// A `ConfigStore` implementation backed by a `HashMap` in memory.
///
/// All writes are visible immediately; persistence is a no-op. Suitable for
/// unit-testing command handlers without a Tauri runtime or filesystem.
pub struct MemoryConfigStore {
    data: std::sync::Mutex<std::collections::HashMap<String, Value>>,
}

impl MemoryConfigStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn get_raw(&self, key: &str) -> Option<Value> {
        self.data
            .lock()
            .expect("MemoryConfigStore lock poisoned")
            .get(key)
            .cloned()
    }

    fn set_raw(&self, key: &str, value: Value) {
        self.data
            .lock()
            .expect("MemoryConfigStore lock poisoned")
            .insert(key.to_string(), value);
    }
}

impl Default for MemoryConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConfigStore for MemoryConfigStore {
    async fn load_config(&self) -> Result<BoardConfig, String> {
        let cfg = self
            .get_raw("board_config")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(default_board_config);
        Ok(cfg)
    }

    async fn save_config(&self, cfg: &BoardConfig) -> Result<(), String> {
        let value = serde_json::to_value(cfg).map_err(|e| format!("serialise error: {e}"))?;
        // In-memory: set+save are atomic under the Mutex (no actual I/O).
        self.set_raw("board_config", value);
        Ok(())
    }

    async fn load_app_key(&self) -> Result<Option<String>, String> {
        let key = self.get_raw("tfl_app_key").and_then(|v| {
            if v.is_null() {
                None
            } else {
                serde_json::from_value::<String>(v).ok()
            }
        });
        Ok(key)
    }

    async fn save_app_key(&self, key: Option<String>) -> Result<(), String> {
        let value = match &key {
            Some(k) => serde_json::json!(k),
            None => Value::Null,
        };
        self.set_raw("tfl_app_key", value);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// BoardService trait object (erases H + C generics for state storage)
// ---------------------------------------------------------------------------

/// Object-safe wrapper around the methods commands need from `BoardService`.
///
/// Erasing `H: TflHttp + C: Clock` generics here means `AppState` is a plain
/// struct (not generic), which is what Tauri's `manage()` API requires.
#[async_trait::async_trait]
pub trait AnyBoardService: Send + Sync + 'static {
    async fn search_stations(&self, query: &str) -> Result<Vec<Station>, BoardError>;
    async fn get_line_status(&self, line_id: &str) -> Result<LineStatus, BoardError>;
    async fn refresh(&self, cfg: &BoardConfig) -> Result<Board, BoardError>;
}

#[async_trait::async_trait]
impl<H: TflHttp + 'static, C: Clock + 'static> AnyBoardService for BoardService<H, C> {
    async fn search_stations(&self, query: &str) -> Result<Vec<Station>, BoardError> {
        BoardService::search_stations(self, query).await
    }

    async fn get_line_status(&self, line_id: &str) -> Result<LineStatus, BoardError> {
        BoardService::get_line_status(self, line_id).await
    }

    async fn refresh(&self, cfg: &BoardConfig) -> Result<Board, BoardError> {
        BoardService::refresh(self, cfg).await
    }
}

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

/// Shared application state. Managed by Tauri and accessible via
/// `tauri::State<'_, AppState>` in every `#[tauri::command]`.
pub struct AppState {
    /// The board service, type-erased so both production and test
    /// implementations can be stored without making `AppState` generic.
    pub board_service: Arc<dyn AnyBoardService>,

    /// Config persistence layer. Swapped for `MemoryConfigStore` in tests.
    pub config_store: Arc<dyn ConfigStore>,

    /// Handle to the running stream task.
    ///
    /// Holds `Some(handle)` when a stream task is active. Set to `None` and
    /// the task aborted when `save_config` is called (stream restarts with
    /// new config from `lib.rs`). Also aborted on window close.
    pub stream_abort: Arc<RwLock<Option<AbortHandle>>>,
}

impl AppState {
    /// Abort the current stream task (if any).
    ///
    /// Clearing the abort handle signals the watcher loop in `lib.rs` to
    /// restart the stream with the latest config from the store.
    /// This is async because `RwLock::write` is async.
    pub async fn abort_stream(&self) {
        if let Some(handle) = self.stream_abort.write().await.take() {
            handle.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Default board config: Belsize Park, no filters, 20-second poll.
pub fn default_board_config() -> BoardConfig {
    BoardConfig::new("940GZZLUBZP")
}
