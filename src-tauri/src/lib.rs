//! Tubbie — Tauri shell library entry point.
//!
//! Tauri v2 uses a library entry point so the binary just calls `run()`.
//! Mobile builds annotate the entry with `#[tauri::mobile_entry_point]`.
//!
//! ## Architecture
//!
//! - `AppState` is constructed here and `manage()`d into the Tauri builder.
//!   It holds the live `BoardService` and the `StorePluginConfigStore`.
//! - All IPC commands live in `commands.rs`. They are thin wrappers that
//!   delegate to `tfl-board` / `tfl-client` and return `Result<T, String>`.
//! - `state.rs` defines `AppState` + the `ConfigStore` trait + `MemoryConfigStore`
//!   (for tests). `store_impl.rs` has the production `StorePluginConfigStore`.
//!
//! ## Polling streams
//!
//! M6 TODO: wire `BoardService::stream` here using `tauri::async_runtime::spawn`
//! bound to the window's `on_window_event(WindowEvent::Destroyed, ...)` lifecycle,
//! so the task is cancelled when the window closes. The stream emits `Board`
//! snapshots via `app.emit("board-update", board)`. See M6 spec.

#![deny(unsafe_code)]

pub mod commands;
pub mod state;
pub mod store_impl;

use std::sync::Arc;

use tauri::Manager;
use tfl_board::BoardService;
use tfl_client::{clock::SystemClock, http::ReqwestTflHttp, TflClient};

use commands::{
    get_board, get_line_status, load_app_key, load_config, save_app_key, save_config,
    search_stations,
};
use state::{AnyBoardService, AppState};
use store_impl::StorePluginConfigStore;

/// Application entry point. Called from `main.rs` (and mobile entry point).
///
/// Registers all plugins, builds `AppState` with the live TfL client, and
/// registers all IPC command handlers.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            // Read saved API key, if any. The client is constructed once at
            // startup. Changing the key requires a restart (Settings UI shows
            // "restart to apply" after save_app_key).
            let store =
                StorePluginConfigStore::open(app.handle()).expect("failed to open config store");

            let saved_key: Option<String> = store.raw_get("tfl_app_key").and_then(|v| {
                if v.is_null() {
                    None
                } else {
                    serde_json::from_value(v).ok()
                }
            });

            let http = match saved_key {
                Some(key) => ReqwestTflHttp::with_app_key(key),
                None => ReqwestTflHttp::new(),
            };

            let client = TflClient::new(http);
            let board_service =
                Arc::new(BoardService::new(client, SystemClock)) as Arc<dyn AnyBoardService>;
            let config_store = Arc::new(store) as Arc<dyn state::ConfigStore>;

            app.manage(AppState {
                board_service,
                config_store,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            search_stations,
            get_board,
            save_config,
            load_config,
            save_app_key,
            load_app_key,
            get_line_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tubbie");
}
