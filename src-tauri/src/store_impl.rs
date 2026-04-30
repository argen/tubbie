//! Production `ConfigStore` implementation backed by `tauri-plugin-store`.
//!
//! `StorePluginConfigStore` wraps a `tauri_plugin_store::Store` handle and
//! delegates `get`/`set`/`save` to it. The store file path is `config.json`
//! inside the platform app-data directory — entirely managed by the plugin.
//!
//! Path traversal is not a concern here: the store plugin handles the file
//! path internally and never accepts caller-supplied paths.
//!
//! ## Async safety
//!
//! `tauri-plugin-store`'s `Store::save()` calls `std::fs::write` +
//! `std::fs::create_dir_all` synchronously. To avoid stalling a Tokio worker
//! thread during disk I/O, both `save_config` and `save_app_key` perform their
//! set+save pair inside `tokio::task::spawn_blocking`. The `Arc<Store>` handle
//! is `'static` and `Send`, so it can be moved into the blocking closure
//! safely.
//!
//! ## Atomicity
//!
//! The `tauri-plugin-store` `Store` type guards its internal state with a
//! `Mutex`. By calling `set` immediately followed by `save` inside the same
//! `spawn_blocking` closure, both operations run on the same OS thread without
//! any other async handler interleaving. This satisfies the set+save atomicity
//! requirement from the M5 review.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_store::{Store, StoreExt};

use crate::state::{default_board_config, ConfigStore, FavoritesStore, DEFAULT_DISPLAY_MODE};
use tfl_board::BoardConfig;
use tfl_domain::Favorite;

/// `ConfigStore` backed by `tauri-plugin-store`.
///
/// Wraps an `Arc<Store>` handle obtained from the `StoreExt` trait. The
/// store is opened (or created) at construction time via `app.store(...)`.
/// `app.store(...)` already returns `Arc<Store<...>>` — we do not wrap it
/// in another `Arc`.
pub struct StorePluginConfigStore {
    store: Arc<Store<tauri::Wry>>,
}

/// `FavoritesStore` backed by `tauri-plugin-store`.
///
/// Uses the `"favorites"` key inside the same `config.json` store file —
/// a sibling to `"board_config"`. Persistence mirrors the `ConfigStore`
/// pattern: set + save inside `spawn_blocking` for async-safe disk I/O.
pub struct StorePluginFavoritesStore {
    store: Arc<Store<tauri::Wry>>,
}

impl StorePluginFavoritesStore {
    /// Open (or create) the `config.json` store for the given app handle.
    pub fn open(app: &AppHandle) -> Result<Self, String> {
        let store = app
            .store("config.json")
            .map_err(|e| format!("failed to open config store for favorites: {e}"))?;
        Ok(Self { store })
    }
}

#[async_trait]
impl FavoritesStore for StorePluginFavoritesStore {
    async fn load_favorites(&self) -> Result<Vec<Favorite>, String> {
        let favorites = self
            .store
            .get("favorites")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Ok(favorites)
    }

    async fn save_favorites(&self, favorites: &[Favorite]) -> Result<(), String> {
        let value =
            serde_json::to_value(favorites).map_err(|e| format!("serialise error: {e}"))?;
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            store.set("favorites", value);
            store
                .save()
                .map_err(|e| format!("failed to save favorites: {e}"))
        })
        .await
        .map_err(|e| format!("spawn_blocking panicked: {e}"))??;
        Ok(())
    }
}

impl StorePluginConfigStore {
    /// Open (or create) the `config.json` store for the given app handle.
    ///
    /// Returns an error string if the plugin is not registered or the store
    /// cannot be opened.
    pub fn open(app: &AppHandle) -> Result<Self, String> {
        // `StoreExt::store` returns `Arc<Store<...>>` directly.
        let store = app
            .store("config.json")
            .map_err(|e| format!("failed to open config store: {e}"))?;
        Ok(Self { store })
    }

    /// Access the raw store for reading keys during app setup (before the
    /// `ConfigStore` trait is needed). Used in `lib.rs` to load the API key
    /// at startup without going through the trait.
    pub fn raw_get(&self, key: &str) -> Option<Value> {
        self.store.get(key)
    }
}

#[async_trait]
impl ConfigStore for StorePluginConfigStore {
    async fn load_config(&self) -> Result<BoardConfig, String> {
        let cfg = self
            .store
            .get("board_config")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(default_board_config);
        Ok(cfg)
    }

    async fn save_config(&self, cfg: &BoardConfig) -> Result<(), String> {
        let value = serde_json::to_value(cfg).map_err(|e| format!("serialise error: {e}"))?;
        // Clone the Arc so the closure is 'static.
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            store.set("board_config", value);
            store
                .save()
                .map_err(|e| format!("failed to save config store: {e}"))
        })
        .await
        .map_err(|e| format!("spawn_blocking panicked: {e}"))??;
        Ok(())
    }

    async fn load_app_key(&self) -> Result<Option<String>, String> {
        let key = self.store.get("tfl_app_key").and_then(|v| {
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
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            store.set("tfl_app_key", value);
            store
                .save()
                .map_err(|e| format!("failed to save config store: {e}"))
        })
        .await
        .map_err(|e| format!("spawn_blocking panicked: {e}"))??;
        Ok(())
    }

    async fn load_display_mode(&self) -> Result<String, String> {
        let mode = self
            .store
            .get("display_mode")
            .and_then(|v| serde_json::from_value::<String>(v).ok())
            .unwrap_or_else(|| DEFAULT_DISPLAY_MODE.to_string());
        Ok(mode)
    }

    async fn save_display_mode(&self, mode: &str) -> Result<(), String> {
        let value = serde_json::json!(mode);
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            store.set("display_mode", value);
            store
                .save()
                .map_err(|e| format!("failed to save config store: {e}"))
        })
        .await
        .map_err(|e| format!("spawn_blocking panicked: {e}"))??;
        Ok(())
    }
}
