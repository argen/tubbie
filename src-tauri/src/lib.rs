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
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, LogicalSize, Manager, PhysicalPosition, WindowEvent,
};
use tfl_board::{BoardConfig, BoardService};
use tfl_client::{clock::SystemClock, http::ReqwestTflHttp, TflClient};
use tokio::sync::RwLock;
use tokio::task::AbortHandle;

use commands::{
    get_board, get_line_status, has_app_key, load_app_key, load_config, load_display_mode,
    save_app_key, save_config, save_display_mode, search_stations,
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
    // The stop-points cache is per-client and the AppState client's cache
    // (warmed at startup) is unreachable from here. Warm this stream's
    // own cache so the first `get_arrivals` call can resolve the station's
    // `hubNaptanCode` and fan out to DLR / Overground / Elizabeth siblings
    // at multi-mode hubs (Bank, TCR, Whitechapel, Stratford…).
    if let Err(e) = client.warm_stop_points_cache().await {
        eprintln!("[tubbie] stream task: failed to warm stop-points cache: {e}");
    }
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

/// Position the popover window anchored below the given tray icon rectangle.
///
/// `tray_rect.position` is the tray icon's top-left in physical screen pixels.
/// We centre the window horizontally under the icon, put its top just below the
/// icon (= menu bar), then clamp to the current monitor's work area so the
/// popover never clips off the right edge.
fn position_popover_under_tray(window: &tauri::WebviewWindow, tray_rect: tauri::Rect) {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let scale = monitor.scale_factor();
    let mon_pos = monitor.position();
    let mon_size = monitor.size();

    let win_size_physical = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(380, 560));

    // `Rect.position` / `.size` are dpi enums; normalise them to physical pixels
    // so our arithmetic stays in one coordinate space regardless of which
    // backend delivered the tray event.
    let tray_pos = tray_rect.position.to_physical::<f64>(scale);
    let tray_size = tray_rect.size.to_physical::<f64>(scale);

    let tray_cx = tray_pos.x + tray_size.width / 2.0;
    let tray_bottom = tray_pos.y + tray_size.height;

    let mut x = tray_cx - (win_size_physical.width as f64) / 2.0;
    let y = tray_bottom + (4.0 * scale);

    // Clamp horizontally to monitor bounds (with a small margin).
    let margin = 4.0 * scale;
    let min_x = mon_pos.x as f64 + margin;
    let max_x = mon_pos.x as f64 + mon_size.width as f64 - win_size_physical.width as f64 - margin;
    if x < min_x {
        x = min_x;
    }
    if x > max_x {
        x = max_x;
    }

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Show the popover, position it under the tray icon, and focus it.
fn show_popover(window: &tauri::WebviewWindow, tray_rect: tauri::Rect) {
    position_popover_under_tray(window, tray_rect);
    let _ = window.show();
    let _ = window.set_focus();
}

/// Strip the native macOS title bar chrome from a decorated window so that
/// our HTML title bar (see `Board.svelte` `.board__titlebar`) can take over.
///
/// The window stays "decorated" from Tauri/Cocoa's perspective — that's what
/// makes a transparent window reliably appear in window mode under the
/// `Regular` activation policy — but we render it without a visible title
/// bar background, title text, or native traffic-light buttons.
///
/// Equivalent to running this in objc:
/// ```objc
/// window.titlebarAppearsTransparent = YES;
/// window.titleVisibility = NSWindowTitleHidden;
/// window.styleMask |= NSWindowStyleMaskFullSizeContentView;
/// [[window standardWindowButton:NSWindowCloseButton] setHidden:YES];
/// [[window standardWindowButton:NSWindowMiniaturizeButton] setHidden:YES];
/// [[window standardWindowButton:NSWindowZoomButton] setHidden:YES];
/// ```
///
/// `unsafe_code` is gated to this single function — there is no Rust-safe
/// API to reach NSWindow's chrome controls.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn strip_native_chrome(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};

    let Ok(ptr) = window.ns_window() else {
        return;
    };
    if ptr.is_null() {
        return;
    }

    // NSWindowStyleMask bits (Cocoa uses NSUInteger = usize on 64-bit).
    const TITLED: usize = 1 << 0;
    const CLOSABLE: usize = 1 << 1;
    const MINIATURIZABLE: usize = 1 << 2;
    const RESIZABLE: usize = 1 << 3;
    const FULL_SIZE_CONTENT_VIEW: usize = 1 << 15;
    // NSWindowTitleVisibility::NSWindowTitleHidden == 1 (NSInteger).
    const TITLE_HIDDEN: isize = 1;
    // NSWindowButton enum: close=0, miniaturize=1, zoom=2 (NSUInteger).
    const BUTTON_KINDS: [usize; 3] = [0, 1, 2];

    unsafe {
        let ns_window = &*(ptr as *const AnyObject);

        let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: Bool::YES];
        let _: () = msg_send![ns_window, setTitleVisibility: TITLE_HIDDEN];

        // Build the style mask explicitly rather than OR-ing into the
        // existing mask — `set_decorations(true)` may not have settled yet
        // when this runs, so reading styleMask could return a partial bag
        // (e.g. just the FullSizeContentView bit) and we'd silently drop
        // Titled, leaving the window in a state where macOS won't render
        // it as a normal floating window.
        let new_mask = TITLED | CLOSABLE | MINIATURIZABLE | RESIZABLE | FULL_SIZE_CONTENT_VIEW;
        let _: () = msg_send![ns_window, setStyleMask: new_mask];

        for kind in BUTTON_KINDS {
            let btn: *mut AnyObject = msg_send![ns_window, standardWindowButton: kind];
            if !btn.is_null() {
                let _: () = msg_send![btn, setHidden: Bool::YES];
            }
        }
    }
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

            // Resolve display mode early — every conditional below (activation
            // policy, window chrome, tray, blur-to-hide) branches on it.
            let display_mode: String = store
                .raw_get("display_mode")
                .and_then(|v| serde_json::from_value::<String>(v).ok())
                .unwrap_or_else(|| state::DEFAULT_DISPLAY_MODE.to_string());

            // macOS: only the menubar mode hides the dock icon. In window
            // mode we want a normal Regular activation policy so the user
            // sees a dock icon and can ⌘-Tab to it.
            #[cfg(target_os = "macos")]
            {
                if display_mode == "menubar" {
                    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }

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
                display_mode: display_mode.clone(),
            });

            if display_mode == "menubar" {
                // --- Menubar tray icon + popover behaviour -----------------
                //
                // Left click on the tray icon toggles the popover window,
                // placing it anchored under the icon. Right click opens a
                // native menu (Settings / About / Quit). Losing focus hides
                // the popover — see the `on_window_event` handler below.

                let settings_item = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
                let about_item = PredefinedMenuItem::about(
                    app,
                    Some("About Tubbie"),
                    Some(tauri::menu::AboutMetadata {
                        name: Some("Tubbie".into()),
                        copyright: Some("© 2026 Bruno Belcastro".into()),
                        ..Default::default()
                    }),
                )?;
                let quit_item = PredefinedMenuItem::quit(app, Some("Quit Tubbie"))?;
                let tray_menu = MenuBuilder::new(app)
                    .item(&settings_item)
                    .separator()
                    .item(&about_item)
                    .separator()
                    .item(&quit_item)
                    .build()?;

                let _tray = TrayIconBuilder::with_id("tubbie-tray")
                    .icon(tauri::include_image!("icons/tray-icon.png"))
                    .icon_as_template(true)
                    .menu(&tray_menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| {
                        if event.id().as_ref() == "settings" {
                            if let Some(win) = app.get_webview_window("main") {
                                let _ = win.show();
                                let _ = win.set_focus();
                                let _ = app.emit("tray://open-settings", ());
                            }
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            rect,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(win) = app.get_webview_window("main") {
                                if win.is_visible().unwrap_or(false) {
                                    let _ = win.hide();
                                } else {
                                    show_popover(&win, rect);
                                }
                            }
                        }
                    })
                    .build(app)?;
            } else {
                // --- Floating window mode -----------------------------------
                //
                // The static window config is tuned for the menubar popover
                // (small, borderless, transparent, hidden on launch). For
                // window mode we re-decorate the window so macOS reliably
                // brings it onscreen, then strip the native chrome (title
                // bar background, title text, traffic-light buttons) so our
                // own LED-themed title bar in Board.svelte takes over.
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_decorations(true);
                    let _ = win.set_always_on_top(false);
                    let _ = win.set_min_size(Some(LogicalSize::new(600.0, 400.0)));
                    let _ = win.set_size(LogicalSize::new(980.0, 720.0));
                    let _ = win.center();
                    let _ = win.show();
                    let _ = win.set_focus();
                    #[cfg(target_os = "macos")]
                    strip_native_chrome(&win);
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::Destroyed => {
                    // Cancel the stream task when the window closes so the
                    // background task does not outlive the Tauri window.
                    let sa = Arc::clone(&window.state::<AppState>().stream_abort);
                    tauri::async_runtime::spawn(async move {
                        if let Some(handle) = sa.write().await.take() {
                            handle.abort();
                        }
                    });
                }
                WindowEvent::Focused(false)
                    if window.state::<AppState>().display_mode == "menubar" =>
                {
                    // Click-away hides the popover only in menubar mode.
                    // In windowed mode the user expects the window to stay
                    // visible when it loses focus.
                    let _ = window.hide();
                }
                _ => {}
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
            save_display_mode,
            load_display_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tubbie");
}
