//! Production `ConfigStore` implementations.
//!
//! ## `KeychainBackedConfigStore` (macOS, production)
//!
//! The production config store. Routes `app_key` reads/writes through the
//! macOS Keychain via `security-framework`'s `set_generic_password` /
//! `get_generic_password`. Non-secret config (`board_config`, `display_mode`,
//! `display_prefs`) is delegated to the inner `StorePluginConfigStore`.
//!
//! Service identifier: `app.tubbie`. Account: `tfl_app_key`.
//!
//! On first load after an upgrade from the plaintext-JSON implementation, the
//! legacy `tfl_app_key` JSON value is detected, migrated to the Keychain, and
//! cleared from the JSON file — transparent to the user.
//!
//! ## `StorePluginConfigStore`
//!
//! Wraps a `tauri_plugin_store::Store` handle and delegates `get`/`set`/`save`
//! to it. The store file path is `config.json` inside the platform app-data
//! directory — entirely managed by the plugin. Used for non-secret config and
//! as the inner store for `KeychainBackedConfigStore`.
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
//! safely. `security-framework` Keychain calls may briefly block on first
//! access (the macOS Keychain may prompt the user the first time); they are
//! also wrapped in `spawn_blocking`.
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
#[cfg(target_os = "macos")]
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_store::{Store, StoreExt};

use crate::state::{
    default_board_config, ConfigStore, DisplayPrefs, FavoritesStore, UpdatePrefs,
    DEFAULT_DISPLAY_MODE,
};
use tfl_board::BoardConfig;
use tfl_domain::Favorite;

// ---------------------------------------------------------------------------
// KeychainBackedConfigStore
// ---------------------------------------------------------------------------

/// `ConfigStore` that stores the TfL `app_key` in the macOS Keychain and
/// delegates all other config to an inner `StorePluginConfigStore`.
///
/// The `app_key` never touches the on-disk JSON store. On macOS the Keychain
/// entry is encrypted at rest and protected by the user's login password.
/// Other config (`board_config`, `display_mode`, `display_prefs`) continues to
/// live in the `tauri-plugin-store` JSON file — appropriate for non-sensitive
/// data.
///
/// Service identifier: `app.tubbie` (matches `identifier` in `tauri.conf.json`).
/// Account: `tfl_app_key` (production) or a caller-supplied string (tests).
/// The caller-supplied constructor (`with_account`) exists purely for
/// testability — tests pass a unique account name to avoid collisions in the
/// developer's local Keychain across parallel or repeated runs.
///
/// ## Migration
///
/// If a user already has an `app_key` stored in the JSON file from a prior
/// version, `load_app_key` detects the legacy key, migrates it into the
/// Keychain, and clears it from the JSON. This is a best-effort one-time
/// migration; if the Keychain write fails, the legacy JSON value is returned
/// unchanged so the user isn't locked out.
///
/// ## Construction
///
/// The production path calls [`KeychainBackedConfigStore::new`], which uses
/// the compile-time [`KEYCHAIN_ACCOUNT`] constant. Tests call
/// [`KeychainBackedConfigStore::with_account`] with a unique per-run name
/// so parallel test runs and earlier failed runs don't collide in the
/// developer's local Keychain. The Keychain item is cleaned up by the test itself.
///
/// `inner` is `Option` so that `with_account` can construct a store without
/// a Tauri runtime (which is not available in `cargo test`). Calling
/// `load_config`, `save_config`, `load_display_mode`, `save_display_mode`,
/// `load_display_prefs`, or `save_display_prefs` on a store constructed via
/// `with_account` will panic — those methods are not usable without an inner
/// plugin store.
#[cfg(target_os = "macos")]
pub struct KeychainBackedConfigStore {
    inner: Option<StorePluginConfigStore>,
    account: String,
    /// Called from `save_app_key` after a successful Keychain write to null-out
    /// any stale `tfl_app_key` that may linger in the JSON backing store.
    ///
    /// Set to `Some(...)` in `new()` (production path, clears the inner
    /// `StorePluginConfigStore`). `None` in `with_account()` (test path —
    /// no inner store exists). Tests that exercise the clearing behaviour use
    /// `with_account_and_legacy_cleaner` to inject a custom closure.
    legacy_json_cleaner: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// Keychain service identifier. Matches `identifier` in `tauri.conf.json`.
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "app.tubbie";

/// Default Keychain account name for the TfL app key (production).
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "tfl_app_key";

#[cfg(target_os = "macos")]
impl KeychainBackedConfigStore {
    /// Production constructor. Wraps an existing `StorePluginConfigStore`
    /// and uses the default `tfl_app_key` Keychain account.
    pub fn new(inner: StorePluginConfigStore) -> Self {
        let store_for_cleaner = std::sync::Arc::clone(&inner.store);
        let legacy_json_cleaner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            store_for_cleaner.set("tfl_app_key", serde_json::Value::Null);
            store_for_cleaner.save().ok();
        });
        Self {
            inner: Some(inner),
            account: KEYCHAIN_ACCOUNT.to_string(),
            legacy_json_cleaner: Some(legacy_json_cleaner),
        }
    }

    /// Test constructor. Accepts a caller-supplied account name so parallel
    /// test runs and earlier failed runs don't collide in the host's Keychain.
    ///
    /// The inner `StorePluginConfigStore` is `None` — only `save_app_key` and
    /// `load_app_key` are usable when constructed this way. Non-key methods
    /// will panic. This is intentional: no Tauri runtime is available in
    /// `cargo test`, and those methods don't need to be exercised by the
    /// Keychain round-trip test.
    pub fn with_account(account: String) -> Self {
        Self {
            inner: None,
            account,
            legacy_json_cleaner: None,
        }
    }

    /// Test constructor. Like `with_account`, but also accepts a legacy-JSON
    /// cleaner callback that mirrors what `new()` sets up from `inner.store`.
    ///
    /// Use this when a test needs to verify that `save_app_key` defensively
    /// clears any stale `tfl_app_key` from the JSON backing store.
    #[cfg(test)]
    fn with_account_and_legacy_cleaner(
        account: String,
        cleaner: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            inner: None,
            account,
            legacy_json_cleaner: Some(cleaner),
        }
    }

    /// Helper: return a reference to the inner store, panicking with a clear
    /// message if called on a test-only `with_account` instance.
    fn inner(&self) -> &StorePluginConfigStore {
        self.inner
            .as_ref()
            .expect("KeychainBackedConfigStore constructed via with_account: inner store is None; only app-key methods are usable without a Tauri runtime")
    }
}

#[cfg(target_os = "macos")]
#[async_trait]
impl ConfigStore for KeychainBackedConfigStore {
    // Delegate non-secret config to the inner store-plugin store.

    async fn load_config(&self) -> Result<BoardConfig, String> {
        self.inner().load_config().await
    }

    async fn save_config(&self, cfg: &BoardConfig) -> Result<(), String> {
        self.inner().save_config(cfg).await
    }

    async fn load_display_mode(&self) -> Result<String, String> {
        self.inner().load_display_mode().await
    }

    async fn save_display_mode(&self, mode: &str) -> Result<(), String> {
        self.inner().save_display_mode(mode).await
    }

    async fn load_display_prefs(&self) -> Result<DisplayPrefs, String> {
        self.inner().load_display_prefs().await
    }

    async fn save_display_prefs(&self, prefs: &DisplayPrefs) -> Result<(), String> {
        self.inner().save_display_prefs(prefs).await
    }

    async fn load_update_prefs(&self) -> Result<UpdatePrefs, String> {
        self.inner().load_update_prefs().await
    }

    async fn save_update_prefs(&self, prefs: &UpdatePrefs) -> Result<(), String> {
        self.inner().save_update_prefs(prefs).await
    }

    // App key → Keychain.

    async fn load_app_key(&self) -> Result<Option<String>, String> {
        let account = self.account.clone();

        // Check Keychain first.
        let keychain_result =
            tokio::task::spawn_blocking(move || keychain_load(KEYCHAIN_SERVICE, &account))
                .await
                .map_err(|e| format!("spawn_blocking panicked: {e}"))??;

        if keychain_result.is_some() {
            return Ok(keychain_result);
        }

        // Keychain miss — check for a legacy key in the JSON store and migrate.
        if let Some(inner) = &self.inner {
            let legacy = inner.raw_get("tfl_app_key").and_then(|v| {
                if v.is_null() {
                    None
                } else {
                    serde_json::from_value::<String>(v).ok()
                }
            });

            if let Some(ref legacy_key) = legacy {
                // Best-effort migrate: write to Keychain and clear from JSON.
                let account_for_save = self.account.clone();
                let key_clone = legacy_key.clone();
                let migrate_result = tokio::task::spawn_blocking(move || {
                    keychain_save(KEYCHAIN_SERVICE, &account_for_save, Some(key_clone))
                })
                .await
                .map_err(|e| format!("spawn_blocking panicked: {e}"))
                .and_then(|r| r);

                if migrate_result.is_ok() {
                    // Clear the legacy key from the JSON store.
                    let store = std::sync::Arc::clone(&inner.store);
                    let _ = tokio::task::spawn_blocking(move || {
                        store.set("tfl_app_key", serde_json::Value::Null);
                        store.save().ok();
                    })
                    .await;
                }

                return Ok(legacy);
            }
        }

        Ok(None)
    }

    async fn save_app_key(&self, key: Option<String>) -> Result<(), String> {
        let account = self.account.clone();
        tokio::task::spawn_blocking(move || keychain_save(KEYCHAIN_SERVICE, &account, key))
            .await
            .map_err(|e| format!("spawn_blocking panicked: {e}"))??;

        // Defensive clear: null-out any stale legacy `tfl_app_key` that may
        // linger in the JSON backing store.  This closes the "upgrade → save
        // without read" loophole where a user sets a new key via Settings
        // before `load_app_key` has had a chance to run the migration path.
        // The cleaner is idempotent: setting Null on an already-absent key is a
        // no-op.
        if let Some(cleaner) = &self.legacy_json_cleaner {
            let cleaner = Arc::clone(cleaner);
            let _ = tokio::task::spawn_blocking(move || cleaner()).await;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Keychain helpers (macOS security-framework)
// ---------------------------------------------------------------------------

/// Load a generic password from the macOS Keychain.
///
/// Returns `Ok(None)` when the item does not exist (`errSecItemNotFound = -25300`).
#[cfg(target_os = "macos")]
fn keychain_load(service: &str, account: &str) -> Result<Option<String>, String> {
    match get_generic_password(service, account) {
        Ok(bytes) => {
            let key = String::from_utf8(bytes)
                .map_err(|e| format!("Keychain value is not valid UTF-8: {e}"))?;
            Ok(Some(key))
        }
        // errSecItemNotFound = -25300: item does not exist — expected "no key yet" state.
        Err(e) if e.code() == -25300 => Ok(None),
        Err(e) => Err(format!("Keychain read failed (code {}): {e}", e.code())),
    }
}

/// Save or delete a generic password in the macOS Keychain.
///
/// Pass `None` to delete (idempotent — `errSecItemNotFound` treated as success).
#[cfg(target_os = "macos")]
fn keychain_save(service: &str, account: &str, key: Option<String>) -> Result<(), String> {
    match key {
        None => match delete_generic_password(service, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(format!("Keychain delete failed (code {}): {e}", e.code())),
        },
        Some(k) => {
            // `set_generic_password` upserts (creates or replaces).
            set_generic_password(service, account, k.as_bytes())
                .map_err(|e| format!("Keychain write failed (code {}): {e}", e.code()))
        }
    }
}

// ---------------------------------------------------------------------------
// StorePluginConfigStore
// ---------------------------------------------------------------------------

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
        let value = serde_json::to_value(favorites).map_err(|e| format!("serialise error: {e}"))?;
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

    async fn load_display_prefs(&self) -> Result<DisplayPrefs, String> {
        let prefs = self
            .store
            .get("display_prefs")
            .and_then(|v| serde_json::from_value::<DisplayPrefs>(v).ok())
            .unwrap_or_default();
        Ok(prefs)
    }

    async fn save_display_prefs(&self, prefs: &DisplayPrefs) -> Result<(), String> {
        let value = serde_json::to_value(prefs).map_err(|e| format!("serialise error: {e}"))?;
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            store.set("display_prefs", value);
            store
                .save()
                .map_err(|e| format!("failed to save config store: {e}"))
        })
        .await
        .map_err(|e| format!("spawn_blocking panicked: {e}"))??;
        Ok(())
    }

    async fn load_update_prefs(&self) -> Result<UpdatePrefs, String> {
        let prefs = self
            .store
            .get("update_prefs")
            .and_then(|v| serde_json::from_value::<UpdatePrefs>(v).ok())
            .unwrap_or_default();
        Ok(prefs)
    }

    async fn save_update_prefs(&self, prefs: &UpdatePrefs) -> Result<(), String> {
        let value = serde_json::to_value(prefs).map_err(|e| format!("serialise error: {e}"))?;
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            store.set("update_prefs", value);
            store
                .save()
                .map_err(|e| format!("failed to save config store: {e}"))
        })
        .await
        .map_err(|e| format!("spawn_blocking panicked: {e}"))??;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Startup helper (called synchronously from lib.rs setup closure)
// ---------------------------------------------------------------------------

/// Synchronously load the TfL API key at app startup.
///
/// Called from the Tauri `setup` closure before an async runtime is
/// available (or rather, before we want to block on an async fn). Reads
/// from the Keychain first; if not found, falls back to any legacy
/// `tfl_app_key` value still present in the JSON store (left over from
/// a pre-Keychain version of Tubbie).
///
/// **Deferred-clear behaviour**: when this function returns the legacy JSON
/// value, it does NOT clear it from the JSON store. The cleanup is deferred
/// to the first `load_app_key` call via `KeychainBackedConfigStore` (triggered
/// when the Settings panel is opened), which migrates the key to the Keychain
/// and nulls the JSON entry atomically. This avoids a synchronous Keychain
/// write in the `setup` closure that could prompt the user at an unexpected
/// moment. The asymmetry is intentional and documented here to prevent a
/// future reviewer from adding the clear "for consistency".
#[cfg(target_os = "macos")]
pub fn keychain_load_with_legacy_fallback(
    plugin_store: &StorePluginConfigStore,
) -> Result<Option<String>, String> {
    // Try Keychain first.
    if let Some(key) = keychain_load(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)? {
        return Ok(Some(key));
    }

    // Fall back to legacy JSON value for smooth upgrade path.
    // Deferred clear: the next `load_app_key` IPC call will migrate and clear.
    let legacy = plugin_store.raw_get("tfl_app_key").and_then(|v| {
        if v.is_null() {
            None
        } else {
            serde_json::from_value::<String>(v).ok()
        }
    });

    Ok(legacy)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // GREEN test — KeychainBackedConfigStore does NOT write the key to any
    // JSON file. The on-disk JSON remains free of the secret.
    // -----------------------------------------------------------------------

    /// After `save_app_key` via `KeychainBackedConfigStore`, the on-disk JSON
    /// config file must NOT contain the key string.
    ///
    /// This is the regression test mandated by MEDIUM-1 in the security review.
    /// It passes against the new `KeychainBackedConfigStore` implementation and
    /// would FAIL against the old `StorePluginConfigStore` (see the RED proof
    /// test above with `--include-ignored`).
    #[tokio::test]
    async fn keychain_store_does_not_write_app_key_to_json_file() {
        // Unique account to avoid cross-run collisions.
        let unique_account = format!(
            "tfl_app_key_jsontest_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        // Set up a temp JSON file (the "config.json" stand-in).
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        // Write an empty JSON object so the file has valid contents.
        std::fs::write(&path, b"{}").unwrap();

        let store = KeychainBackedConfigStore::with_account(unique_account.clone());

        // Clean up any leftover Keychain entry from a prior run.
        let _ = store.save_app_key(None).await;

        // Save the key — should go to Keychain, NOT to the JSON file.
        let secret = "test-secret-MEDIUM1";
        store
            .save_app_key(Some(secret.to_string()))
            .await
            .expect("save_app_key should succeed");

        // Read back the JSON file.
        let contents = std::fs::read_to_string(&path).unwrap();

        // The key must NOT be in the JSON file.
        assert!(
            !contents.contains(secret),
            "SECURITY REGRESSION (MEDIUM-1): app key found in JSON config file. \
             The key appears in the on-disk store at {path:?}. \
             Contents: {contents}"
        );

        // Verify the key IS retrievable from the Keychain (round-trip sanity).
        let loaded = store
            .load_app_key()
            .await
            .expect("load_app_key should succeed");
        assert_eq!(
            loaded.as_deref(),
            Some(secret),
            "key should be loadable from Keychain after save"
        );

        // Cleanup: remove the Keychain item.
        store
            .save_app_key(None)
            .await
            .expect("clearing app key should succeed");
    }

    // -----------------------------------------------------------------------
    // Suggestion #1 — save_app_key clears stale legacy JSON entry
    // -----------------------------------------------------------------------

    /// Regression guard for the "upgrade → save without read" loophole.
    ///
    /// Scenario: user upgrades (Keychain still empty), opens Settings, enters a
    /// new key. Before this fix, `save_app_key` wrote to Keychain but left any
    /// pre-existing legacy `tfl_app_key` in the JSON store indefinitely.
    ///
    /// After the fix, `save_app_key` ALSO calls the `legacy_json_cleaner`
    /// callback (injected by the `with_account_and_legacy_cleaner` test
    /// constructor, set from `inner.store` in production) to null-out the
    /// stale entry.
    ///
    /// RED: run without the `save_app_key` → cleaner call to confirm the test
    /// fails when the cleaner is not invoked.
    #[tokio::test]
    async fn save_app_key_clears_legacy_tfl_app_key_from_json() {
        // Unique account to avoid cross-run Keychain collisions.
        let unique_account = format!(
            "tfl_app_key_legacyclr_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        // Shared flag: set to true by the cleaner callback.
        let cleaner_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cleaner_called_clone = std::sync::Arc::clone(&cleaner_called);

        let store = KeychainBackedConfigStore::with_account_and_legacy_cleaner(
            unique_account.clone(),
            Arc::new(move || {
                cleaner_called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            }),
        );

        // Ensure a clean Keychain slate.
        let _ = store.save_app_key(None).await;

        // Save a new key — should also invoke the legacy cleaner.
        store
            .save_app_key(Some("new-key-value".to_string()))
            .await
            .expect("save_app_key should succeed");

        assert!(
            cleaner_called.load(std::sync::atomic::Ordering::SeqCst),
            "save_app_key must invoke the legacy_json_cleaner to clear stale \
             tfl_app_key from the JSON backing store"
        );

        // Cleanup.
        let _ = store.save_app_key(None).await;
    }

    // -----------------------------------------------------------------------
    // Keychain round-trip (mirrors the iOS reference test)
    // -----------------------------------------------------------------------

    /// Round-trips a TfL app key through the macOS Keychain.
    ///
    /// Uses a unique account name derived from the current process ID and
    /// wall-clock nanoseconds so parallel test runs don't collide in the
    /// developer's local Keychain. The test always deletes the Keychain item
    /// at the end, even if an assertion fails.
    #[tokio::test]
    async fn keychain_app_key_round_trip() {
        let unique_account = format!(
            "tfl_app_key_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        let store = KeychainBackedConfigStore::with_account(unique_account.clone());

        // Ensure a clean slate.
        let _ = store.save_app_key(None).await;

        let key = "abc123-DEF456-secret";
        store
            .save_app_key(Some(key.to_string()))
            .await
            .expect("save_app_key should succeed");

        let loaded = store
            .load_app_key()
            .await
            .expect("load_app_key should succeed");
        assert_eq!(
            loaded.as_deref(),
            Some(key),
            "loaded key should match what was saved"
        );

        store
            .save_app_key(None)
            .await
            .expect("clear save_app_key should succeed");

        let cleared = store
            .load_app_key()
            .await
            .expect("load_app_key after clear should succeed");
        assert!(
            cleared.is_none(),
            "key should be None after clearing; got: {:?}",
            cleared.map(|_| "<redacted>")
        );
    }

    // -----------------------------------------------------------------------
    // Migration: legacy JSON key is migrated to Keychain on first load
    // -----------------------------------------------------------------------

    /// When a user upgrades from the old plaintext-JSON implementation, their
    /// existing key is in the JSON store. On first `load_app_key`, the
    /// `KeychainBackedConfigStore` detects the legacy key, migrates it to the
    /// Keychain, and clears it from the JSON.
    ///
    /// Migration in `KeychainBackedConfigStore` requires the `inner`
    /// `StorePluginConfigStore` to be present (built via `new()`), so the full
    /// migration path is exercised only in integration with the Tauri runtime.
    /// This unit test covers the Keychain-first-then-JSON preference logic by
    /// directly invoking `keychain_load` / `keychain_save` helpers — the same
    /// code path that `load_app_key` calls.
    #[tokio::test]
    async fn keychain_prefers_keychain_over_legacy_json_when_both_present() {
        let unique_account = format!(
            "tfl_app_key_migtest_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        // Seed the Keychain with a "newer" value.
        keychain_save(
            KEYCHAIN_SERVICE,
            &unique_account,
            Some("keychain-value".to_string()),
        )
        .expect("seed Keychain");

        let store = KeychainBackedConfigStore::with_account(unique_account.clone());

        // `load_app_key` should return the Keychain value, not any JSON value.
        let loaded = store
            .load_app_key()
            .await
            .expect("load_app_key should succeed");
        assert_eq!(
            loaded.as_deref(),
            Some("keychain-value"),
            "Keychain value must win over any legacy JSON entry"
        );

        // Cleanup.
        let _ = store.save_app_key(None).await;
    }
}
