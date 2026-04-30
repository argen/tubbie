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

use std::sync::{Arc, RwLock as StdRwLock};

use async_trait::async_trait;
use serde_json::Value;
use tfl_board::{BoardConfig, BoardError, BoardService, LifecyclePhase};
use tfl_client::{clock::Clock, http::TflHttp};
use tfl_domain::{Board, Favorite, LineStatus, Station};
use tokio::{
    sync::{watch, RwLock},
    task::AbortHandle,
};

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

    /// Load the persisted display mode (`"window"` or `"menubar"`).
    /// Returns `"window"` when nothing has been saved.
    async fn load_display_mode(&self) -> Result<String, String>;

    /// Atomically set and persist the display mode. The caller is
    /// responsible for validating the value before invoking this.
    async fn save_display_mode(&self, mode: &str) -> Result<(), String>;
}

/// Default display mode used when no value has been persisted.
/// Mirrors the user's request that Tubbie launch as a normal floating
/// window unless they have explicitly opted into the menubar UI.
pub const DEFAULT_DISPLAY_MODE: &str = "window";

// ---------------------------------------------------------------------------
// FavoritesStore trait
// ---------------------------------------------------------------------------

/// Persistence abstraction for the favorites list.
///
/// Stored under a separate `"favorites"` key (sibling of `"board_config"`).
/// Mutations bypass the `cfg_tx` watch channel — selecting a favorite goes
/// through the existing `save_config` path which triggers invariant #2.
///
/// Tests substitute `MemoryFavoritesStore` for headless operation.
#[async_trait]
pub trait FavoritesStore: Send + Sync + 'static {
    /// Load the favorites list. Returns an empty list if nothing has been saved.
    async fn load_favorites(&self) -> Result<Vec<Favorite>, String>;

    /// Persist the full favorites list (overwrites what was there).
    async fn save_favorites(&self, favorites: &[Favorite]) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// In-memory FavoritesStore (used in tests)
// ---------------------------------------------------------------------------

/// A `FavoritesStore` backed by a `Vec` in memory.
pub struct MemoryFavoritesStore {
    data: std::sync::Mutex<Vec<Favorite>>,
}

impl MemoryFavoritesStore {
    pub fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemoryFavoritesStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FavoritesStore for MemoryFavoritesStore {
    async fn load_favorites(&self) -> Result<Vec<Favorite>, String> {
        Ok(self
            .data
            .lock()
            .expect("MemoryFavoritesStore lock poisoned")
            .clone())
    }

    async fn save_favorites(&self, favorites: &[Favorite]) -> Result<(), String> {
        *self
            .data
            .lock()
            .expect("MemoryFavoritesStore lock poisoned") = favorites.to_vec();
        Ok(())
    }
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

    async fn load_display_mode(&self) -> Result<String, String> {
        let mode = self
            .get_raw("display_mode")
            .and_then(|v| serde_json::from_value::<String>(v).ok())
            .unwrap_or_else(|| DEFAULT_DISPLAY_MODE.to_string());
        Ok(mode)
    }

    async fn save_display_mode(&self, mode: &str) -> Result<(), String> {
        self.set_raw("display_mode", serde_json::json!(mode));
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
    async fn warm_stop_points_cache(&self) -> Result<usize, BoardError>;
    async fn refresh_stop_points_cache(&self) -> Result<usize, BoardError>;
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

    async fn warm_stop_points_cache(&self) -> Result<usize, BoardError> {
        BoardService::warm_stop_points_cache(self).await
    }

    async fn refresh_stop_points_cache(&self) -> Result<usize, BoardError> {
        BoardService::refresh_stop_points_cache(self).await
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

    /// Favorites persistence layer. Uses a separate store key `"favorites"`.
    /// Mutations bypass `cfg_tx` — only station selection goes through
    /// `save_config` (invariant #2).
    pub favorites_store: Arc<dyn FavoritesStore>,

    /// Handle to the running stream task.
    ///
    /// Holds `Some(handle)` when a stream task is active. Routine config
    /// changes no longer abort the stream — the task observes the new
    /// `BoardConfig` via `cfg_tx`/`cfg_rx` and applies it on the next tick.
    /// The handle is cleared and the task aborted on window close
    /// (`WindowEvent::Destroyed`) or by the panic-recovery watcher in
    /// `lib.rs::run` when the task ends unexpectedly.
    pub stream_abort: Arc<RwLock<Option<AbortHandle>>>,

    /// Sender end of the live `BoardConfig` watch channel. The stream task
    /// holds the corresponding `Receiver` and re-reads the config on every
    /// tick (and on `changed()`), so writing here applies the new config
    /// without a stream restart. `save_config_inner` calls `send` after the
    /// store write completes.
    pub cfg_tx: Arc<watch::Sender<BoardConfig>>,

    /// Live display mode (`"window"` or `"menubar"`).
    ///
    /// Seeded at startup from the persisted value and mutated in place by
    /// `save_display_mode_inner` so the runtime swap (tray on/off, dock
    /// icon on/off, window chrome) is observed by every consumer without
    /// a process restart. The sync `std::sync::RwLock` is used so the
    /// `WindowEvent::Focused(false)` click-away handler — which runs on
    /// the Tauri main thread in a sync context — can read the value
    /// without spawning onto the Tokio runtime. Holds are microseconds.
    pub display_mode: Arc<StdRwLock<String>>,

    /// Lifecycle phase signal. Desktop builds always stay `Active`
    /// (`LifecyclePhase::always_active()`). The iOS shell writes
    /// `Background` / `Active` from its mobile run-event handler. Placed
    /// here so both the initial stream spawn and the panic-recovery watcher
    /// can subscribe a fresh `phase_rx` from the same stable sender.
    pub lifecycle: Arc<LifecyclePhase>,
}

impl AppState {
    /// Abort the current stream task (if any).
    ///
    /// Used on window close (`WindowEvent::Destroyed`) so the background
    /// task does not outlive the Tauri window. Routine config changes do
    /// **not** call this — they `cfg_tx.send(..)` instead and the stream
    /// applies the change on its next tick.
    pub async fn abort_stream(&self) {
        if let Some(handle) = self.stream_abort.write().await.take() {
            handle.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Default board config: Belsize Park, no filters, 30-second poll.
pub fn default_board_config() -> BoardConfig {
    BoardConfig::new("940GZZLUBZP")
}
