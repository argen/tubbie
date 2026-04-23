//! Production `ConfigStore` implementation backed by `tauri-plugin-store`.
//!
//! `StorePluginConfigStore` wraps a `tauri_plugin_store::Store` handle and
//! delegates `get`/`set`/`save` to it. The store file path is `config.json`
//! inside the platform app-data directory — entirely managed by the plugin.
//!
//! Path traversal is not a concern here: the store plugin handles the file
//! path internally and never accepts caller-supplied paths.

use std::sync::Arc;

use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_store::{Store, StoreExt};

use crate::state::ConfigStore;

/// `ConfigStore` backed by `tauri-plugin-store`.
///
/// Wraps an `Arc<Store>` handle obtained from the `StoreExt` trait. The
/// store is opened (or created) at construction time via `app.store(...)`.
/// `app.store(...)` already returns `Arc<Store<...>>` — we do not wrap it
/// in another `Arc`.
pub struct StorePluginConfigStore {
    store: Arc<Store<tauri::Wry>>,
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

impl ConfigStore for StorePluginConfigStore {
    fn get(&self, key: &str) -> Option<Value> {
        self.store.get(key)
    }

    fn set(&self, key: &str, value: Value) {
        self.store.set(key, value);
    }

    fn save(&self) -> Result<(), String> {
        self.store
            .save()
            .map_err(|e| format!("failed to save config store: {e}"))
    }
}
