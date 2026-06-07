//! Tubbie — Tauri shell library entry point.
//!
//! Tauri v2 uses a library entry point so the binary just calls `run()`.
//! Mobile builds annotate the entry with `#[tauri::mobile_entry_point]`.
//!
//! ## Architecture
//!
//! - `AppState` is constructed here and `manage()`d into the Tauri builder.
//!   It holds the live `BoardService`, the `KeychainBackedConfigStore`, and an
//!   `AbortHandle` for the active stream task.
//! - All IPC commands live in `commands.rs`. They are thin wrappers that
//!   delegate to `tfl-board` / `tfl-client` and return `Result<T, String>`.
//! - `state.rs` defines `AppState` + the `ConfigStore` trait + `MemoryConfigStore`
//!   (for tests). `store_impl.rs` has the production `KeychainBackedConfigStore`.
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
#[cfg(target_os = "macos")]
pub mod location;
pub mod pool_key;
pub mod state;
pub mod store_impl;

use std::sync::{Arc, Mutex, RwLock as StdRwLock};
use std::time::Duration;

use futures::StreamExt;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, LogicalSize, Manager, PhysicalPosition, WindowEvent,
};
use tfl_board::{BoardConfig, BoardService, LifecyclePhase, TokioSleepTimer, WarmFallback};
use tfl_cache::TflClient;
use tfl_client::{clock::SystemClock, http::ReqwestTflHttp};
use tokio::sync::RwLock;
use tokio::task::AbortHandle;

#[cfg(target_os = "macos")]
use commands::request_current_location;
use commands::{
    add_favorite, apply_board_size, check_for_updates, find_nearest_stations,
    get_all_line_statuses, get_board, get_line_status, has_app_key, install_update, list_favorites,
    load_app_key, load_config, load_display_mode, load_display_prefs, load_update_prefs,
    remove_favorite, save_app_key, save_config, save_display_mode, save_display_prefs,
    save_update_prefs, search_stations, set_tray_disruption,
};
use state::{AnyBoardService, AppState};
#[cfg(target_os = "macos")]
use store_impl::KeychainBackedConfigStore;
use store_impl::{StorePluginConfigStore, StorePluginFavoritesStore};
// On non-macOS the startup API-key read calls the `ConfigStore` trait method
// `load_app_key` on a concrete `StorePluginConfigStore` (the macOS path uses
// the inherent keychain helper instead), so the trait must be in scope there
// for method resolution.
#[cfg(not(target_os = "macos"))]
use state::ConfigStore as _;

/// RAII handle for a Tauri event listener. Calling `unlisten` in `Drop`
/// guarantees cleanup even if the awaiting task is aborted mid-flight (window
/// destroyed, app teardown, etc.) — without this, an aborted
/// `wait_for_first_board_emit` would leak the listener and the closure's
/// captured `Arc<Mutex<…>>` for the rest of the `AppHandle`'s lifetime.
struct ListenerGuard {
    app: tauri::AppHandle,
    id: tauri::EventId,
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.app.unlisten(self.id);
    }
}

/// Block until either a `board://updated` event arrives on `app` or
/// `fallback` elapses, whichever happens first.
///
/// Used by the stop-points warm task to give the stream's first /Arrivals
/// fetch unobstructed access to the global `cooldown_until` gate. If the
/// warm fires first and 429s on /StopPoint/Mode/tube (TfL's most rate-
/// limited route), the stream's first fetch sleeps behind the cooldown
/// for nothing — the user is staring at the board, not the settings.
///
/// The fallback is a safety net: if the stream is permanently broken we
/// still want the warm to fire (best-effort), rather than leave the
/// settings cache cold forever.
///
/// Uses [`WarmFallback<TokioSleepTimer>`] — wall-clock deadline measured by
/// `tokio::time::sleep`. The iOS counterpart swaps in an active-only timer
/// via `ActiveTimeTimer` so the deadline does not count down while the app
/// is backgrounded (invariant 8).
async fn wait_for_first_board_emit(app: &tauri::AppHandle, fallback: Duration) {
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx_slot: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>> =
        Arc::new(Mutex::new(Some(tx)));
    let tx_slot_for_listener = Arc::clone(&tx_slot);
    // `listen_any` callback is `Fn`, not `FnMut`, so move the Sender into a
    // Mutex<Option<...>> and `take()` it on the first hit. Subsequent hits
    // see `None` and fall through.
    let listener_id = app.listen_any("board://updated", move |_event| {
        if let Ok(mut guard) = tx_slot_for_listener.lock() {
            if let Some(s) = guard.take() {
                let _ = s.send(());
            }
        }
    });
    let _guard = ListenerGuard {
        app: app.clone(),
        id: listener_id,
    };

    WarmFallback::new(TokioSleepTimer, fallback).wait(rx).await;
    // _guard drops here, calling app.unlisten(listener_id).
}

/// Background pool-key cache refresh (Phase 1). Fetches the pool from the
/// network on the ambient Tauri runtime and writes the selected key to the
/// config store under `"pool_key_cache"` for the NEXT launch. Fully fail-open:
/// any network / parse / store error is logged and swallowed — never surfaced,
/// never blocking. Does NOT touch the running client (the Mac client bakes its
/// key in at construction and has no live-swap path; that is Phase 2).
async fn refresh_pool_key_cache(app: tauri::AppHandle) {
    let client = match reqwest::Client::builder()
        .timeout(pool_key::FETCH_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tubbie:pool-key] failed to build refresh client: {e}");
            return;
        }
    };

    // Fail-open: a None here keeps whatever is already cached.
    let Some(key) = pool_key::fetch_one_pool_key(&client, pool_key::POOL_KEYS_URL).await else {
        return;
    };

    match StorePluginConfigStore::open(&app) {
        Ok(store) => {
            if let Err(e) = store
                .raw_set_and_save("pool_key_cache", serde_json::json!(key))
                .await
            {
                eprintln!("[tubbie:pool-key] failed to cache refreshed key: {e}");
            }
        }
        Err(e) => eprintln!("[tubbie:pool-key] failed to open store for cache write: {e}"),
    }
}

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

/// Reset a window's macOS style mask back to borderless.
///
/// Counterpart to [`strip_native_chrome`]. Used when transitioning from
/// window mode → menubar mode at runtime: `set_decorations(false)` *should*
/// take care of this, but the prior `strip_native_chrome` call set an
/// explicit style mask via `setStyleMask:` and Cocoa retains it until we
/// override. Without this helper, the menubar popover can render with a
/// stale title-bar style and feel "off" until the next launch.
///
/// `NSWindowStyleMaskBorderless == 0` — equivalent to `setStyleMask: 0`
/// in objc.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn restore_borderless_chrome(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ptr) = window.ns_window() else {
        return;
    };
    if ptr.is_null() {
        return;
    }

    const BORDERLESS: usize = 0;

    unsafe {
        let ns_window = &*(ptr as *const AnyObject);
        let _: () = msg_send![ns_window, setStyleMask: BORDERLESS];
    }
}

/// Tray icon id reused across builds. Lets us look up / remove the tray
/// when the user toggles display mode at runtime.
const TRAY_ID: &str = "tubbie-tray";

/// Build the menu-bar tray icon (idempotent).
///
/// Returns `Ok(())` immediately if a tray with [`TRAY_ID`] already exists —
/// safe to call repeatedly when toggling into menubar mode multiple times.
fn build_tray(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    if app.tray_by_id(TRAY_ID).is_some() {
        return Ok(());
    }

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

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::include_image!("icons/tray-icon.png"))
        .icon_as_template(true)
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "settings" {
                // Open the in-frame Settings panel: show + focus the main
                // window, then emit `open-settings` so the renderer flips the
                // settingsOpen store and the overlay mounts over the board.
                // Settings is no longer a separate webview window.
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                    if let Err(e) = win.emit("open-settings", ()) {
                        eprintln!("[tubbie] failed to emit open-settings from tray: {e}");
                    }
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

    Ok(())
}

/// Swap the menu-bar tray icon between the normal dot-matrix glyph and the
/// monochrome "disrupted" variant (the menubar disruption indicator).
///
/// Both icons are `icon_as_template(true)` so macOS auto-tints them for
/// light/dark/notch; the disrupted variant is a dot-matrix exclamation — a
/// distinct *silhouette*, NOT a colored dot — so the template model is
/// preserved. (Icons are original art, generated by `scripts/gen-tray-icons.py`;
/// the TfL roundel is a trademark and is deliberately not used.)
///
/// **Dispatches to the macOS main thread** — `TrayIcon::set_icon` reaches
/// `NSStatusItem` (Cocoa, main-thread-only; invariants #8/#9). Fire-and-forget;
/// a no-op in window mode where `tray_by_id` is `None`. `set_icon` can reset
/// the template flag, so we re-assert it after every swap.
pub(crate) fn apply_tray_disruption(app: &tauri::AppHandle, disrupted: bool) {
    let app_clone = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        if let Some(tray) = app_clone.tray_by_id(TRAY_ID) {
            let icon = if disrupted {
                tauri::include_image!("icons/tray-icon-alert.png")
            } else {
                tauri::include_image!("icons/tray-icon.png")
            };
            let _ = tray.set_icon(Some(icon));
            let _ = tray.set_icon_as_template(true);
        }
    }) {
        eprintln!("[tubbie] failed to dispatch tray disruption icon to main thread: {e}");
    }
}

/// Run the UI side-effects that distinguish window mode from menubar mode.
///
/// **MUST run on the macOS main thread.** Every operation here ultimately
/// reaches a Cocoa API: `NSApplication::setActivationPolicy`,
/// `NSStatusBar::removeStatusItem` (via `TrayIcon::Drop`), `NSWindow`
/// style mask + size + visibility. Calling any of these from a Tokio
/// worker trips Cocoa's `BSServiceMainRunLoopQueue` barrier and crashes
/// the process with `EXC_BREAKPOINT`. Use [`apply_display_mode_effects`]
/// instead from any non-main-thread caller — that wrapper hops to the
/// main thread via `run_on_main_thread`.
///
/// Pure side-effects: does not read or mutate `AppState`. The caller
/// persists the new mode and updates `state.display_mode`.
///
/// Errors are swallowed (best-effort) for window/policy calls, matching
/// the existing `let _ = win.set_*` pattern. A failed tray build does
/// surface as `Err` because a missing tray in menubar mode leaves the
/// user with no way to interact with the app.
///
/// Used at startup (after the persisted mode is loaded — `setup` runs
/// on the main thread) and from `save_display_mode` (which dispatches
/// to the main thread first) — same code path either way, so startup
/// and live-toggle cannot drift.
pub(crate) fn apply_display_mode_effects_sync(
    app: &tauri::AppHandle,
    target: &str,
) -> Result<(), String> {
    // 1. macOS activation policy. Accessory hides the dock icon (menubar
    //    mode); Regular shows it (window mode). Tauri exposes
    //    `set_activation_policy` at runtime — Apple's NSApplication
    //    supports all transitions between Regular/Accessory/Prohibited.
    #[cfg(target_os = "macos")]
    {
        let policy = if target == "menubar" {
            tauri::ActivationPolicy::Accessory
        } else {
            tauri::ActivationPolicy::Regular
        };
        let _ = app.set_activation_policy(policy);
    }

    // 2. Tray icon. Build on the way into menubar; remove on the way out.
    //    `build_tray` is idempotent (early-returns if the tray already
    //    exists), so reapplying menubar mode is safe.
    if target == "menubar" {
        build_tray(app).map_err(|e| format!("tray build failed: {e}"))?;
    } else {
        let _ = app.remove_tray_by_id(TRAY_ID);
    }

    // 3. Window chrome + geometry. The static window config is borderless
    //    + transparent + always-on-top + 380×560; window mode re-decorates
    //    and resizes to 980×720, then strips the native chrome so our LED
    //    title bar can take over. Going back to menubar reverses both.
    if let Some(win) = app.get_webview_window("main") {
        if target == "menubar" {
            let _ = win.set_decorations(false);
            let _ = win.set_always_on_top(true);
            let _ = win.set_min_size(Some(LogicalSize::new(320.0, 400.0)));
            let _ = win.set_size(LogicalSize::new(380.0, 560.0));
            let _ = win.hide();
            #[cfg(target_os = "macos")]
            restore_borderless_chrome(&win);
        } else {
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
}

/// Apply a logical-pixel size to the main window.
///
/// **MUST run on the macOS main thread** — `WebviewWindow::set_size` reaches
/// `NSWindow::setFrame:display:` (and friends) which Cocoa asserts must be
/// called on the main thread. Same constraint as the display-mode side-
/// effects (see [`apply_display_mode_effects_sync`]). Use
/// [`apply_board_size_effects`] from any non-main-thread caller.
///
/// In menubar mode the popover keeps its current top-left anchor and grows
/// downward; we make a best-effort to keep the bottom edge on-screen by
/// nudging the y position up if the new height would push it past the
/// monitor's work area. This avoids a popover that disappears off the bottom
/// of a small display when switching from a 1-line to a 3-line station.
pub(crate) fn apply_board_size_sync(
    app: &tauri::AppHandle,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    let _ = win.set_size(LogicalSize::new(width, height));

    // Only the menubar popover is anchored under the tray and so vulnerable
    // to bottom-edge clipping. Window mode is centered + draggable, so
    // resize is harmless.
    let in_menubar = app
        .try_state::<AppState>()
        .as_ref()
        .and_then(|s| s.display_mode.try_read().ok().map(|g| g.clone()))
        .map(|m| m == "menubar")
        .unwrap_or(false);
    if in_menubar {
        clamp_window_above_screen_bottom(&win);
    }
    Ok(())
}

/// Slide the window up if its new height pushes the bottom edge past the
/// current monitor's work area. Best-effort: any missing piece (no monitor,
/// no position) leaves the window where it was. Reads the *current* outer
/// position rather than recomputing under the tray icon — that data is only
/// available from a tray click event, and the popover anchor at click time
/// is already correct.
fn clamp_window_above_screen_bottom(window: &tauri::WebviewWindow) {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let mon_pos = monitor.position();
    let mon_size = monitor.size();
    let win_bottom = pos.y + size.height as i32;
    let mon_bottom = mon_pos.y + mon_size.height as i32;
    let margin = 8;
    if win_bottom + margin > mon_bottom {
        let new_y = (mon_bottom - size.height as i32 - margin).max(mon_pos.y);
        let _ = window.set_position(PhysicalPosition::new(pos.x, new_y));
    }
}

/// Apply a window resize from any thread. Hops to the macOS main thread
/// and waits on a oneshot for completion. Mirrors
/// [`apply_display_mode_effects`] — see invariant #8 in `CLAUDE.md`.
pub(crate) async fn apply_board_size_effects(
    app: &tauri::AppHandle,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let app_clone = app.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    app.run_on_main_thread(move || {
        let res = apply_board_size_sync(&app_clone, width, height);
        let _ = tx.send(res);
    })
    .map_err(|e| format!("dispatch to main thread failed: {e}"))?;
    rx.await
        .map_err(|e| format!("apply_board_size dropped before completion: {e}"))?
}

/// Apply the display-mode side-effects from any thread.
///
/// Hops to the macOS main thread via `run_on_main_thread` and waits on a
/// oneshot for completion, then returns the result. Required because
/// every Cocoa API touched by [`apply_display_mode_effects_sync`] is
/// main-thread-only — calling them from a Tokio worker (which is where
/// Tauri commands run) trips `BSServiceMainRunLoopQueue::assertBarrierOnQueue`
/// and crashes the process. We learned this the hard way; do not inline
/// the sync version into a command handler.
pub(crate) async fn apply_display_mode_effects(
    app: &tauri::AppHandle,
    target: &str,
) -> Result<(), String> {
    let app_clone = app.clone();
    let target_owned = target.to_string();
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    app.run_on_main_thread(move || {
        let res = apply_display_mode_effects_sync(&app_clone, &target_owned);
        let _ = tx.send(res);
    })
    .map_err(|e| format!("dispatch to main thread failed: {e}"))?;
    rx.await
        .map_err(|e| format!("apply_display_mode_effects dropped before completion: {e}"))?
}

/// Application entry point. Called from `main.rs` (and mobile entry point).
///
/// Registers all plugins, builds `AppState` with the live TfL client, and
/// registers all IPC command handlers.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        // Opener plugin: lets the renderer open external URLs (TfL Open Data,
        // GitHub releases, the TfL API portal) in the system browser. Plain
        // `<a target="_blank">` is a no-op in a WKWebView, so the footer /
        // About / API-key links were dead before this. Scoped to the exact
        // hosts in `capabilities/default.json` (`opener:allow-open-url`).
        .plugin(tauri_plugin_opener::init())
        // Updater plugin. PR-A wiring; `active: false` in `tauri.conf.json`
        // keeps it inert until PR-B lands the real pubkey + signing config.
        // Registered here so the test harness (`updater_plugin_registered.rs`)
        // can assert `updater_builder()` succeeds without needing to mirror
        // the registration in `mock_builder()`.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Read saved API key, if any. The client is constructed once at
            // startup. Changing the key requires a restart (Settings UI shows
            // "restart to apply" after save_app_key).
            let plugin_store =
                StorePluginConfigStore::open(app.handle()).expect("failed to open config store");

            // Resolve display mode early. We pass it to
            // `apply_display_mode_effects` below — the single seam that owns
            // activation policy, tray, and window chrome both at startup and
            // at runtime when the user toggles via Settings.
            let initial_display_mode: String = plugin_store
                .raw_get("display_mode")
                .and_then(|v| serde_json::from_value::<String>(v).ok())
                .unwrap_or_else(|| state::DEFAULT_DISPLAY_MODE.to_string());

            // Live display-mode lock. Seeded with the persisted value; the
            // `WindowEvent::Focused(false)` click-away handler reads it on
            // every focus loss, and `save_display_mode` writes to it after
            // the user toggles in Settings.
            let display_mode_lock: Arc<StdRwLock<String>> =
                Arc::new(StdRwLock::new(initial_display_mode.clone()));

            // Load the API key from the macOS Keychain (MEDIUM-1 fix).
            // Falls back to a legacy JSON value if one exists so users
            // upgrading from the old plaintext-JSON implementation are not
            // locked out. The KeychainBackedConfigStore::load_app_key will
            // migrate the legacy value on the next async call; here we do a
            // direct synchronous read for the startup HTTP client.
            //
            // On non-macOS platforms (Linux CI, future ports) the Keychain is
            // unavailable; the API key lives in the store-plugin JSON instead.
            #[cfg(target_os = "macos")]
            let saved_key: Option<String> =
                store_impl::keychain_load_with_legacy_fallback(&plugin_store).unwrap_or_else(|e| {
                    eprintln!("[tubbie] Failed to load API key from Keychain at startup: {e}");
                    None
                });
            #[cfg(not(target_os = "macos"))]
            let saved_key: Option<String> =
                tauri::async_runtime::block_on(plugin_store.load_app_key()).unwrap_or_else(|e| {
                    eprintln!("[tubbie] Failed to load API key from store at startup: {e}");
                    None
                });

            // Pool-key fallback (Phase 1). The personal key (Keychain) always
            // wins; the pool is a zero-config fallback so a fresh install has
            // rate-limit headroom without registering at the TfL portal.
            //
            // NON-BLOCKING — the board is never blocked on the key service
            // (a hard invariant). The client bakes its key in at construction
            // and has no live-swap path, so the pool key spans two launches:
            //   * here we read the LAST-CACHED pool key SYNCHRONOUSLY (a local
            //     store read, no network) and bake it into the one client below;
            //   * a background task spawned after setup refreshes the cache for
            //     the NEXT launch (see `refresh_pool_key_cache`).
            // A first-ever launch with an empty cache runs anonymous for that
            // session (anonymous still shows arrivals); it is keyed from the
            // next launch on. Fail-open throughout.
            // See invariants #5 (one Arc<TflClient>), #6 (sync before stream).
            let has_personal_key = saved_key.is_some();
            let saved_key = pool_key::select_startup_key(saved_key, || {
                pool_key::validated_cached_key(plugin_store.raw_get("pool_key_cache"))
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
            // On macOS, wrap the plugin store in a KeychainBackedConfigStore so
            // that subsequent save_app_key / load_app_key calls go through the
            // macOS Keychain rather than the plaintext JSON file (MEDIUM-1).
            // On other platforms (Linux CI), use the store-plugin store directly.
            #[cfg(target_os = "macos")]
            let config_store = Arc::new(KeychainBackedConfigStore::new(plugin_store))
                as Arc<dyn state::ConfigStore>;
            #[cfg(not(target_os = "macos"))]
            let config_store = Arc::new(plugin_store) as Arc<dyn state::ConfigStore>;

            // Favorites store: separate `"favorites"` key, same config.json file.
            // Opened lazily-idempotent by the plugin — re-opening the same
            // path just returns an existing Arc<Store> handle.
            let favorites_store = Arc::new(
                StorePluginFavoritesStore::open(app.handle())
                    .expect("failed to open favorites store"),
            ) as Arc<dyn state::FavoritesStore>;

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

            // Pool-key cache refresh — fire-and-forget on the ambient runtime,
            // AFTER the stream is already spawned so it never gates startup.
            // Only when there is no personal key (a user with their own key
            // never needs the pool). Writes the freshest key to the store for
            // the NEXT launch; does not touch the already-built client.
            if !has_personal_key {
                let refresh_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    refresh_pool_key_cache(refresh_handle).await;
                });
            }

            // Pre-warm the stop-points cache so the first settings search is
            // instant rather than paying ~1-2s for the 16 MB /StopPoint/Mode/tube
            // fetch. Fire-and-forget — failure here must never block startup.
            //
            // Wait until the stream task has produced its first board (or an
            // 8 s fallback elapses) before warming. The /StopPoint/Mode/tube
            // endpoint is TfL's most aggressively rate-limited route; if it
            // 429s and sets the shared `cooldown_until` gate before the
            // stream's first /Arrivals fetch can run, the user stares at
            // "Loading arrivals…" for the duration of the cooldown for no
            // reason — settings search isn't even open. Using the existing
            // `board://updated` event as the "first emit happened" signal
            // keeps the plumbing minimal; the 8 s fallback ensures a
            // permanently broken stream doesn't starve the warm forever.
            let bs = Arc::clone(&board_service);
            let warm_app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                wait_for_first_board_emit(&warm_app_handle, Duration::from_secs(8)).await;
                match bs.warm_stop_points_cache().await {
                    Ok(n) => eprintln!("[tubbie] stop-points cache warmed ({n} stations)"),
                    Err(e) => eprintln!("[tubbie] stop-points cache warm failed: {e}"),
                }
            });

            // Periodic stop-points refresh.
            //
            // The cache is stale-while-revalidate: `search_stations` and
            // the hub-merge lookups in `get_arrivals` always return
            // whatever's currently cached (fresh or stale) — they never
            // block on a TTL-driven refresh past the initial warm. This
            // task is what keeps "whatever's currently cached" actually
            // fresh: every ~14 minutes (just under the 15-min
            // STOP_POINTS_TTL) it forces a single-flighted refresh. If a
            // tick is missed (laptop sleep, transient TfL outage) the
            // user sees slightly older station metadata until the next
            // tick — acceptable because TfL station metadata is stable
            // for months. The first tick is delayed by the period so it
            // doesn't race with the initial warm above.
            let refresh_bs = Arc::clone(&board_service);
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval_at(
                    tokio::time::Instant::now() + Duration::from_secs(14 * 60),
                    Duration::from_secs(14 * 60),
                );
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    ticker.tick().await;
                    if let Err(e) = refresh_bs.refresh_stop_points_cache().await {
                        eprintln!("[tubbie] stop-points cache periodic refresh failed: {e}");
                    }
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
                favorites_store,
                stream_abort,
                cfg_tx,
                display_mode: Arc::clone(&display_mode_lock),
                lifecycle,
            });

            // Apply the persisted mode now. Setup runs on the macOS main
            // thread, so we can call the sync version directly. The async
            // wrapper used by `save_display_mode` would deadlock here —
            // `run_on_main_thread` posts a user event that the main thread
            // can only process *after* setup returns.
            apply_display_mode_effects_sync(&app.handle().clone(), &initial_display_mode).map_err(
                |e| {
                    eprintln!("[tubbie] failed to apply initial display mode: {e}");
                    Box::<dyn std::error::Error>::from(e)
                },
            )?;

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
                WindowEvent::Focused(false) => {
                    // Click-away hides the popover only in menubar mode.
                    // In windowed mode the user expects the window to stay
                    // visible when it loses focus.
                    //
                    // `try_read` rather than `read`: the live display-mode
                    // lock is held briefly during `apply_display_mode`
                    // transitions, and the focus event must not block the
                    // main event loop. A contended lock is treated as
                    // "don't hide" — safe because the swap is rare and the
                    // user has already lost focus, so re-clicking the tray
                    // is the worst-case UX.
                    let should_hide = window
                        .state::<AppState>()
                        .display_mode
                        .try_read()
                        .map(|guard| guard.as_str() == "menubar")
                        .unwrap_or(false);
                    if should_hide {
                        let _ = window.hide();
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            search_stations,
            find_nearest_stations,
            #[cfg(target_os = "macos")]
            request_current_location,
            get_board,
            save_config,
            load_config,
            save_app_key,
            load_app_key,
            has_app_key,
            get_line_status,
            get_all_line_statuses,
            save_display_mode,
            load_display_mode,
            save_display_prefs,
            load_display_prefs,
            apply_board_size,
            list_favorites,
            add_favorite,
            remove_favorite,
            check_for_updates,
            install_update,
            load_update_prefs,
            save_update_prefs,
            set_tray_disruption,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tubbie");
}
