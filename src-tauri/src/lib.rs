//! Tubbie — Tauri shell library entry point.
//!
//! Tauri v2 uses a library entry point so the binary just calls `run()`.
//! Mobile builds annotate the entry with `#[tauri::mobile_entry_point]`.
//!
//! ## Architecture
//!
//! - `AppState` is constructed here and `manage()`d into the Tauri builder.
//!   It holds the live `BoardService`, the `StorePluginConfigStore`, and an
//!   `AbortHandle` for the active stream task.
//! - All IPC commands live in `commands.rs`. They are thin wrappers that
//!   delegate to `tfl-board` / `tfl-client` and return `Result<T, String>`.
//! - `state.rs` defines `AppState` + the `ConfigStore` trait + `MemoryConfigStore`
//!   (for tests). `store_impl.rs` has the production `StorePluginConfigStore`.
//!
//! ## Polling stream wiring (M6)
//!
//! On app startup, after loading the config, we spawn a Tokio task that runs
//! `BoardService::stream(cfg)` and emits each `Board` as a `board://updated`
//! Tauri event via `app.emit("board://updated", board)`.
//!
//! The task's `AbortHandle` is stored in `AppState::stream_abort`.
//!
//! When `save_config` is called:
//!   1. The new config is persisted to the store.
//!   2. `AppState::abort_stream()` cancels the running task (clears handle).
//!   3. A watcher loop (running in its own task) detects the `None` abort handle
//!      and spawns a fresh task with the latest config from the store.
//!
//! On `WindowEvent::Destroyed`, the abort handle is cancelled, cleanly
//! stopping the stream task before the Tauri runtime exits.

#![deny(unsafe_code)]

pub mod commands;
pub mod state;
pub mod store_impl;

use std::sync::Arc;

use futures::StreamExt;
use tauri::{Emitter, Manager};
use tfl_board::{BoardConfig, BoardService};
use tfl_client::{clock::SystemClock, http::ReqwestTflHttp, TflClient};
use tokio::sync::RwLock;
use tokio::task::AbortHandle;

use commands::{
    get_board, get_line_status, has_app_key, load_app_key, load_config, save_app_key, save_config,
    search_stations,
};
use state::{AnyBoardService, AppState};
use store_impl::StorePluginConfigStore;

/// Spawn a stream task for the latest config, storing its `AbortHandle` in
/// `stream_abort`.
///
/// Called at startup and whenever the abort handle is cleared (config change).
async fn spawn_stream_task(
    app_handle: tauri::AppHandle,
    config_store: Arc<dyn state::ConfigStore>,
    stream_abort: Arc<RwLock<Option<AbortHandle>>>,
) {
    // Load the latest config from the store every time we (re-)start.
    let cfg: BoardConfig = match config_store.load_config().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tubbie] Failed to load config for stream task: {e}");
            return;
        }
    };

    let app = app_handle.clone();

    // Build a fresh concrete BoardService for this stream instance.
    // We cannot call .stream() through the AnyBoardService trait object because
    // stream() consumes self and returns an impl Stream — impossible to make
    // object-safe. The service is cheap to construct (no state, just an HTTP
    // client wrapper).
    let http = ReqwestTflHttp::new();
    let client = TflClient::new(http);
    let service = BoardService::new(client, SystemClock);
    let mut stream = Box::pin(service.stream(cfg));

    // Clone the Arc so the spawned task can clear its own handle when it ends.
    let stream_abort_clone = Arc::clone(&stream_abort);

    // Use tokio::task::spawn so we get a JoinHandle with abort_handle().
    let join_handle = tokio::task::spawn(async move {
        loop {
            match stream.next().await {
                Some(Ok(board)) => {
                    if let Err(e) = app.emit("board://updated", &board) {
                        eprintln!("[tubbie] Failed to emit board://updated: {e}");
                    }
                }
                Some(Err(e)) => {
                    // BoardService::stream handles stale-data fallback internally.
                    // Err only occurs when there is no last-ok board. The stream
                    // terminates after emitting this error item.
                    eprintln!("[tubbie] Stream fatal error (no last-ok board): {e}");
                    break;
                }
                None => {
                    // Stream exhausted after a fatal error.
                    eprintln!("[tubbie] Board stream ended.");
                    break;
                }
            }
        }
        // Clear the abort handle so the watcher loop detects the task has
        // ended and schedules a restart — covers natural termination, panics,
        // and any future code path that lets the task die.
        eprintln!("[tubbie] stream task ended; scheduling restart");
        *stream_abort_clone.write().await = None;
    });

    let abort = join_handle.abort_handle();
    *stream_abort.write().await = Some(abort);
}

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
            let stream_abort: Arc<RwLock<Option<AbortHandle>>> = Arc::new(RwLock::new(None));

            // Spawn the initial stream task.
            let cs = Arc::clone(&config_store);
            let sa = Arc::clone(&stream_abort);
            let ah = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                spawn_stream_task(ah, cs, sa).await;
            });

            // Pre-warm the stop-points cache so the first settings search is
            // instant rather than paying ~1-2s for the 16 MB /StopPoint/Mode/tube
            // fetch. Fire-and-forget — failure here must never block startup.
            let bs = Arc::clone(&board_service);
            tauri::async_runtime::spawn(async move {
                match bs.warm_stop_points_cache().await {
                    Ok(n) => eprintln!("[tubbie] stop-points cache warmed ({n} stations)"),
                    Err(e) => eprintln!("[tubbie] stop-points cache warm failed: {e}"),
                }
            });

            // Watcher loop: restarts the stream when the abort handle is
            // cleared (e.g. after save_config cancels the previous task).
            let cs2 = Arc::clone(&config_store);
            let sa2 = Arc::clone(&stream_abort);
            let ah2 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    // Detect both: handle is None (explicitly cleared), or handle
                    // is still Some but the task has already finished (belt-and-
                    // braces against any code path that lets the task die without
                    // clearing its own handle).
                    let needs_restart = {
                        let mut guard = sa2.write().await;
                        match guard.as_ref() {
                            None => true,
                            Some(h) if h.is_finished() => {
                                // Task finished but handle was not cleared — take
                                // it now so spawn_stream_task sees a clean slate.
                                guard.take();
                                true
                            }
                            _ => false,
                        }
                    };
                    if needs_restart {
                        // Task was aborted or ended (config change or fatal stream
                        // error). Restart with the current store config.
                        spawn_stream_task(ah2.clone(), Arc::clone(&cs2), Arc::clone(&sa2)).await;
                    }
                }
            });

            app.manage(AppState {
                board_service,
                config_store,
                stream_abort,
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Cancel the stream task when the window closes so the
                // background task does not outlive the Tauri window.
                let sa = Arc::clone(&window.state::<AppState>().stream_abort);
                tauri::async_runtime::spawn(async move {
                    if let Some(handle) = sa.write().await.take() {
                        handle.abort();
                    }
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            search_stations,
            get_board,
            save_config,
            load_config,
            save_app_key,
            load_app_key,
            has_app_key,
            get_line_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tubbie");
}
