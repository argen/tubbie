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
//!   2. `AppState::cfg_tx.send(new_cfg)` publishes it to the running stream
//!      task; the task picks up the change on its next tick (or earlier via
//!      `cfg_rx.changed()`) without restarting. No 16 MB stop-points
//!      re-warm, no fresh arrivals burst, caches stay populated.
//!   3. A panic-recovery watcher loop polls `AbortHandle::is_finished()`
//!      every 2 s and respawns the task only if it has died.
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
use tfl_board::{BoardConfig, BoardService, LifecyclePhase};
use tfl_client::{clock::SystemClock, http::ReqwestTflHttp, TflClient};
use tokio::sync::RwLock;
use tokio::task::AbortHandle;

use commands::{
    get_board, get_line_status, has_app_key, load_app_key, load_config, load_display_mode,
    save_app_key, save_config, save_display_mode, search_stations,
};
use state::{AnyBoardService, AppState};
use store_impl::StorePluginConfigStore;

/// Spawn a stream task that emits `board://updated` Tauri events.
///
/// The task observes config changes via `cfg_rx` (a `watch::Receiver`) so
/// non-`station_id` updates (theme, directions, line filter, poll_seconds)
/// apply on the next tick without restarting the task. The watcher loop in
/// `run()` retains `is_finished()` checks as a panic-recovery safety net only.
///
/// Reuses the shared `Arc<TflClient<ReqwestTflHttp>>` from `AppState` so the
/// stream and the on-demand command path share a single set of caches
/// (`stop_points_cache`, `hub_children_cache`, `line_status_cache`) and one
/// HTTP client (one connection pool, one 429 cooldown gate).
async fn spawn_stream_task(
    app_handle: tauri::AppHandle,
    client: Arc<TflClient<ReqwestTflHttp>>,
    cfg_rx: tokio::sync::watch::Receiver<BoardConfig>,
    phase_rx: tokio::sync::watch::Receiver<tfl_board::AppPhase>,
    stream_abort: Arc<RwLock<Option<AbortHandle>>>,
) {
    let app = app_handle.clone();

    // Build a BoardService over the shared client. Cheap — `BoardService`
    // is just `Arc<TflClient>` + `Clock`. Construction does no I/O; the
    // shared client's caches are warmed once at startup by AppState.
    let service = BoardService::new(Arc::clone(&client), SystemClock);
    let mut stream = Box::pin(service.stream(cfg_rx, phase_rx));

    // Clone the Arc so the spawned task can clear its own handle when it ends.
    let stream_abort_clone = Arc::clone(&stream_abort);

    // Use tokio::task::spawn so we get a JoinHandle with abort_handle().
    let join_handle = tokio::task::spawn(async move {
        // Track whether the previous tick errored so we don't spam the log
        // when TfL is rate-limiting and every poll fails.
        let mut prev_was_err = false;
        loop {
            match stream.next().await {
                Some(Ok(board)) => {
                    if prev_was_err {
                        eprintln!("[tubbie] stream tick recovered");
                        prev_was_err = false;
                    }
                    if let Err(e) = app.emit("board://updated", &board) {
                        eprintln!("[tubbie] Failed to emit board://updated: {e}");
                    }
                }
                Some(Err(e)) => {
                    // BoardService::stream is infinite — on fetch failure with
                    // no last-ok board it emits the error and keeps polling.
                    // Breaking here would kill the task and the watcher would
                    // respawn it 2 s later, hammering TfL straight through any
                    // 429 cooldown. Log once per streak and let poll_seconds
                    // throttle retries.
                    //
                    // Emit `board://error` so the renderer can surface
                    // *something* to the user — without this the frontend has
                    // no way to learn that polling is failing (the seed
                    // `getBoard` IPC could resolve OK while the stream is
                    // the source of breakage), and the user is left staring
                    // at "Loading arrivals…" forever. Only emitted on the
                    // streak transition so a multi-minute outage doesn't
                    // spam the event channel.
                    if !prev_was_err {
                        eprintln!("[tubbie] stream tick failed (no last-ok board): {e}");
                        prev_was_err = true;
                        let payload = serde_json::json!({ "message": e.to_string() });
                        if let Err(emit_err) = app.emit("board://error", &payload) {
                            eprintln!("[tubbie] Failed to emit board://error: {emit_err}");
                        }
                    }
                }
                None => {
                    // Defensive: BoardService::stream is infinite. If a future
                    // change ever lets it terminate, fall through so the
                    // watcher can respawn.
                    eprintln!("[tubbie] Board stream ended unexpectedly.");
                    break;
                }
            }
        }
        // Clear the abort handle so the watcher loop detects the task has
        // ended and schedules a restart — covers panics and any future code
        // path that lets the task die.
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

            // Clone before consuming in the AppState HTTP client so we can also
            // thread the key into spawn_stream_task (and the watcher restarts).
            let stream_app_key = saved_key.clone();

            let http = match saved_key {
                Some(key) => ReqwestTflHttp::with_app_key(key),
                None => ReqwestTflHttp::new(),
            };

            let client = Arc::new(TflClient::new(http));
            let board_service = Arc::new(BoardService::new(Arc::clone(&client), SystemClock))
                as Arc<dyn AnyBoardService>;
            let config_store = Arc::new(store) as Arc<dyn state::ConfigStore>;
            let stream_abort: Arc<RwLock<Option<AbortHandle>>> = Arc::new(RwLock::new(None));

            // Desktop always stays Active; iOS swaps this for a real signal.
            let lifecycle = Arc::new(LifecyclePhase::always_active());

            // Seed the watch channel from the persisted config. The channel
            // is the live config source for the stream — `save_config`
            // writes to it after persisting, and the stream observes changes
            // mid-flight without restarting the task.
            //
            // Load synchronously here so the first stream tick observes the
            // user's saved station, not the default. The store's load is a
            // single in-memory JSON read wrapped in async for trait
            // uniformity — `block_on` cannot deadlock and avoids the race
            // where the stream would otherwise fetch arrivals for the
            // default station before an async loader had a chance to run.
            let initial_cfg = tauri::async_runtime::block_on(config_store.load_config())
                .unwrap_or_else(|e| {
                    eprintln!("[tubbie] Failed to load initial config: {e}");
                    state::default_board_config()
                });
            let (cfg_tx, cfg_rx) = tokio::sync::watch::channel::<BoardConfig>(initial_cfg);
            let cfg_tx = Arc::new(cfg_tx);

            // Spawn the initial stream task. Reuses the shared client so the
            // stream task and the on-demand command path share one set of
            // caches (no 16 MB stop-points re-warm per save_config).
            let stream_client = Arc::clone(&client);
            let sa = Arc::clone(&stream_abort);
            let ah = app.handle().clone();
            let initial_cfg_rx = cfg_rx.clone();
            let initial_phase_rx = lifecycle.subscribe();
            tauri::async_runtime::spawn(async move {
                spawn_stream_task(ah, stream_client, initial_cfg_rx, initial_phase_rx, sa).await;
            });

            // The stream client now reuses the AppState client, so the saved
            // app_key already routed through `with_app_key` above is what the
            // stream sees. `stream_app_key` is no longer threaded into the
            // stream task — drop it.
            let _ = stream_app_key;

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

            // Watcher loop: panic-recovery only. Routine config changes flow
            // through `cfg_tx` and the running stream picks them up on its
            // next tick — no respawn. This loop restarts the task only when
            // it dies (panic, or `WindowEvent::Destroyed` on shutdown).
            let sa2 = Arc::clone(&stream_abort);
            let ah2 = app.handle().clone();
            let watcher_client = Arc::clone(&client);
            let watcher_cfg_rx = cfg_rx.clone();
            let watcher_lifecycle = Arc::clone(&lifecycle);
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
                        // Task panicked or was explicitly aborted (window
                        // close). Restart with the live cfg_rx and the
                        // shared client — caches stay warm across the
                        // respawn.
                        spawn_stream_task(
                            ah2.clone(),
                            Arc::clone(&watcher_client),
                            watcher_cfg_rx.clone(),
                            watcher_lifecycle.subscribe(),
                            Arc::clone(&sa2),
                        )
                        .await;
                    }
                }
            });

            app.manage(AppState {
                board_service,
                config_store,
                stream_abort,
                cfg_tx,
                display_mode: display_mode.clone(),
                lifecycle,
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
