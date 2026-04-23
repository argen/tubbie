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

use serde_json::Value;
use tfl_board::{BoardConfig, BoardError, BoardService};
use tfl_client::{clock::Clock, http::TflHttp};
use tfl_domain::{Board, LineStatus, Station};

// ---------------------------------------------------------------------------
// ConfigStore trait
// ---------------------------------------------------------------------------

/// Persistence abstraction for app configuration.
///
/// The production implementation (`StorePluginConfigStore`) delegates to
/// `tauri-plugin-store`. Tests use `MemoryConfigStore` for deterministic,
/// headless operation without a Tauri runtime.
pub trait ConfigStore: Send + Sync + 'static {
    /// Read a JSON value by key. Returns `None` if the key is absent.
    fn get(&self, key: &str) -> Option<Value>;

    /// Write a JSON value for key.
    fn set(&self, key: &str, value: Value);

    /// Persist changes to disk. No-op for the in-memory implementation.
    fn save(&self) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// In-memory ConfigStore (used in tests)
// ---------------------------------------------------------------------------

/// A `ConfigStore` implementation backed by a `HashMap` in memory.
///
/// All writes are visible immediately; `save()` is a no-op. Suitable for
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
}

impl Default for MemoryConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigStore for MemoryConfigStore {
    fn get(&self, key: &str) -> Option<Value> {
        self.data
            .lock()
            .expect("MemoryConfigStore lock poisoned")
            .get(key)
            .cloned()
    }

    fn set(&self, key: &str, value: Value) {
        self.data
            .lock()
            .expect("MemoryConfigStore lock poisoned")
            .insert(key.to_string(), value);
    }

    fn save(&self) -> Result<(), String> {
        // In-memory — nothing to flush.
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
}

impl AppState {
    /// Convenience: load `BoardConfig` from the store, falling back to the
    /// documented default (Belsize Park, no filters, 20 s poll).
    pub fn load_board_config(&self) -> BoardConfig {
        self.config_store
            .get("board_config")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(default_board_config)
    }
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Default board config: Belsize Park, no filters, 20-second poll.
pub fn default_board_config() -> BoardConfig {
    BoardConfig::new("940GZZLUBZP")
}
