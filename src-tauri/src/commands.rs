//! Tauri IPC command handlers.
//!
//! All commands return `Result<T, String>` — Tauri IPC requires serializable
//! errors. Errors are converted to human-readable strings at this boundary;
//! `TflError::Transport` is rendered via its `Display` impl (which is already
//! URL-redacted, never leaking `app_key`).
//!
//! ## Argument validation
//!
//! Every public command validates its arguments before touching any state.
//! Validation failures return `Err("validation: <field> …")` — never panic.
//!
//! Field rules:
//! - `station_id`: 1–32 chars, ASCII alphanumeric + `_` + `-`.
//! - `line_id`: 1–32 chars, ASCII lowercase alphanumeric + `-`.
//! - `query`: max 100 chars, no null bytes.
//! - `poll_seconds`: clamped to [10, 300] (not rejected, UI can display effective value).
//! - `app_key`: max 64 chars, no null bytes (when `Some`).
//! - `line_ids`: at most 32 entries.
//! - `directions`: at most 16 entries.
//!
//! ## Async safety
//!
//! All handlers are `async fn` with `#[tauri::command]`. Config persistence is
//! delegated to `ConfigStore::save_config` / `ConfigStore::save_app_key`, which
//! are atomic compound operations (set+save under a single lock). The production
//! impl wraps the blocking `Store::save()` in `tokio::task::spawn_blocking`.
//!
//! ## M6 TODO
//!
//! Wire `BoardService::stream` with `tauri::async_runtime::spawn` + a
//! cancellation token bound to `WindowEvent::Destroyed`. The stream should
//! emit `Board` snapshots as `app.emit("board-update", board)` events.

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tauri_plugin_updater::UpdaterExt;

use tfl_board::{BoardConfig, VALID_THEME_IDS};
use tfl_domain::{
    is_supported_line_id, Board, Favorite, LineRef, LineStatus, NearbyStation, Station,
};

use crate::state::{AppState, DisplayPrefs, UpdatePrefs};

// ---------------------------------------------------------------------------
// Validation functions (pub within crate for tests)
// ---------------------------------------------------------------------------

/// Validate a `station_id` argument.
/// Allowed: ASCII alphanumeric + `-` + `_`, 1–32 chars.
pub(crate) fn validate_station_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 32 {
        return Err(format!(
            "validation: station_id must be 1–32 characters, got {}",
            id.len()
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "validation: station_id contains disallowed characters: {id:?}"
        ));
    }
    Ok(())
}

/// Validate a `line_id` argument.
/// Allowed: ASCII lowercase alphanumeric + `-`, 1–32 chars.
pub(crate) fn validate_line_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 32 {
        return Err(format!(
            "validation: line_id must be 1–32 characters, got {}",
            id.len()
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!(
            "validation: line_id must be lowercase alphanumeric + '-': {id:?}"
        ));
    }
    Ok(())
}

/// Validate latitude / longitude / limit arguments for the
/// `find_nearest_stations` command. Rejects NaN, infinity, out-of-range
/// coordinates, and limits outside `[1, 20]` so a buggy or hostile
/// renderer cannot ask us to rank thousands of stations or dispatch
/// CoreLocation at coordinates that overflow our haversine.
pub(crate) fn validate_nearest_args(lat: f64, lon: f64, limit: u32) -> Result<(), String> {
    if !lat.is_finite() || !lon.is_finite() {
        return Err("validation: lat/lon must be finite".to_string());
    }
    if !(-90.0..=90.0).contains(&lat) {
        return Err(format!("validation: lat must be in [-90, 90], got {lat}"));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(format!("validation: lon must be in [-180, 180], got {lon}"));
    }
    if !(1..=20).contains(&limit) {
        return Err(format!("validation: limit must be in [1, 20], got {limit}"));
    }
    Ok(())
}

/// Validate a search `query` argument.
/// Max 100 chars, no null bytes.
pub(crate) fn validate_query(query: &str) -> Result<(), String> {
    if query.len() > 100 {
        return Err(format!(
            "validation: query must be ≤100 characters, got {}",
            query.len()
        ));
    }
    if query.contains('\0') {
        return Err("validation: query must not contain null bytes".to_string());
    }
    Ok(())
}

/// Clamp `poll_seconds` to the allowed range [10, 300].
///
/// The floor is 10 s rather than 5 s: TfL's arrivals data refreshes ~30 s,
/// so sub-10 s polling only adds load without returning fresher data.
pub(crate) fn clamp_poll_seconds(v: u32) -> u32 {
    v.clamp(10, 300)
}

/// Validate an optional `app_key`.
///
/// Accepts any printable non-null ASCII up to 64 chars — TfL does not publish
/// a strict key grammar, so we avoid over-restricting. When `Some`: max 64
/// chars, no null bytes.
pub(crate) fn validate_app_key(key: &Option<String>) -> Result<(), String> {
    if let Some(k) = key {
        if k.len() > 64 {
            return Err(format!(
                "validation: app_key must be ≤64 characters, got {}",
                k.len()
            ));
        }
        if k.contains('\0') {
            return Err("validation: app_key must not contain null bytes".to_string());
        }
    }
    Ok(())
}

/// Validate a `common_name` string for a favorite station.
///
/// 200-char cap: generous relative to the longest real TfL station name
/// (~52 chars — "London Heathrow Terminals 2 & 3 Underground Station"), but
/// tight enough to bound JSON storage and UI render allocation (LOW-2).
///
/// Character allowlist (P4.3): Unicode letters, digits, ASCII spaces,
/// and the punctuation set audited from every name in
/// `fixtures/stop-points/*.json` — `& ' ( ) - . /`. Anything else
/// (control characters, angle brackets, semicolons, emoji, zero-width
/// spaces, tabs, etc.) is rejected so favorites JSON cannot smuggle
/// arbitrary content into the disk store or back through the renderer.
/// The cap is enforced over byte length to bound disk usage; the
/// allowlist iterates `chars()` so multi-byte UTF-8 sequences pass
/// when they decode to a `char::is_alphabetic()` code point.
pub(crate) fn validate_common_name(name: &str) -> Result<(), String> {
    if name.len() > 200 {
        return Err(format!(
            "validation: common_name must be ≤200 characters, got {}",
            name.len()
        ));
    }
    if name.contains('\0') {
        return Err("validation: common_name must not contain null bytes".to_string());
    }
    if let Some(bad) = name.chars().find(|c| !is_allowed_station_name_char(*c)) {
        return Err(format!(
            "validation: common_name contains disallowed character {:?} (U+{:04X})",
            bad, bad as u32
        ));
    }
    Ok(())
}

/// Allowed-character predicate for `common_name`.
///
/// Accepts Unicode letters and digits, ASCII space, and the punctuation
/// set found in real TfL station names.
fn is_allowed_station_name_char(c: char) -> bool {
    c.is_alphabetic()
        || c.is_ascii_digit()
        || c == ' '
        || matches!(c, '&' | '\'' | '(' | ')' | '-' | '.' | '/')
}

/// Validate a `LineRef.name` string stored in a favorite.
///
/// Same 200-char cap and null-byte restriction as `validate_common_name`
/// — both fields live in the same favorites JSON and share the same risk
/// profile (LOW-2).
pub(crate) fn validate_line_name(name: &str) -> Result<(), String> {
    if name.len() > 200 {
        return Err(format!(
            "validation: LineRef.name must be ≤200 characters, got {}",
            name.len()
        ));
    }
    if name.contains('\0') {
        return Err("validation: LineRef.name must not contain null bytes".to_string());
    }
    Ok(())
}

/// Validate a `BoardConfig`'s fields.
///
/// Checks `station_id`, each `line_id`, collection length caps, and rejects
/// configs with too many entries to bound downstream allocations.
pub(crate) fn validate_board_config(cfg: &BoardConfig) -> Result<(), String> {
    validate_station_id(&cfg.station_id)?;
    if cfg.line_ids.len() > 32 {
        return Err(format!(
            "validation: line_ids must have at most 32 entries, got {}",
            cfg.line_ids.len()
        ));
    }
    if cfg.directions.len() > 16 {
        return Err(format!(
            "validation: directions must have at most 16 entries, got {}",
            cfg.directions.len()
        ));
    }
    for line_id in &cfg.line_ids {
        validate_line_id(line_id)?;
    }
    // directions: enum values validated by serde deserialization
    // poll_seconds: clamped, not rejected
    // theme: must be one of the four known theme IDs
    if !VALID_THEME_IDS.contains(&cfg.theme.as_str()) {
        return Err(format!(
            "validation: theme must be one of {:?}, got {:?}",
            VALID_THEME_IDS, cfg.theme
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Inner functions (stateless logic, testable without tauri::State)
// ---------------------------------------------------------------------------

pub(crate) async fn search_stations_inner(
    query: &str,
    state: &AppState,
) -> Result<Vec<Station>, String> {
    validate_query(query)?;
    #[cfg(debug_assertions)]
    let started = std::time::Instant::now();
    let stations = crate::state::AnyBoardService::search_stations(&*state.board_service, query)
        .await
        .map_err(|e| e.to_string())?;
    #[cfg(debug_assertions)]
    {
        let names: Vec<&str> = stations
            .iter()
            .take(3)
            .map(|s| s.common_name.as_str())
            .collect();
        eprintln!(
            "[search_stations] q={:?} elapsed={}ms results={} first={:?}",
            query,
            started.elapsed().as_millis(),
            stations.len(),
            names,
        );
    }
    Ok(stations)
}

pub(crate) async fn find_nearest_stations_inner(
    lat: f64,
    lon: f64,
    limit: u32,
    state: &AppState,
) -> Result<Vec<NearbyStation>, String> {
    validate_nearest_args(lat, lon, limit)?;
    #[cfg(debug_assertions)]
    let started = std::time::Instant::now();
    let nearby = crate::state::AnyBoardService::find_nearest_stations(
        &*state.board_service,
        lat,
        lon,
        limit as usize,
    )
    .await
    .map_err(|e| e.to_string())?;
    #[cfg(debug_assertions)]
    {
        // Diagnostic line — never logs lat/lon, only the result count
        // and the closest station's name + distance so we can sanity-
        // check ranking from a dev console.
        let first = nearby
            .first()
            .map(|n| (n.station.common_name.as_str(), n.distance_m.round() as i64));
        eprintln!(
            "[find_nearest_stations] elapsed={}ms results={} first={:?}",
            started.elapsed().as_millis(),
            nearby.len(),
            first,
        );
    }
    Ok(nearby)
}

pub(crate) async fn get_board_inner(state: &AppState) -> Result<Board, String> {
    let cfg = state.config_store.load_config().await?;
    let board = crate::state::AnyBoardService::refresh(&*state.board_service, &cfg)
        .await
        .map_err(|e| e.to_string())?;
    Ok(board)
}

pub(crate) async fn save_config_inner(cfg: &BoardConfig, state: &AppState) -> Result<(), String> {
    validate_board_config(cfg)?;
    let cfg = BoardConfig {
        poll_seconds: clamp_poll_seconds(cfg.poll_seconds),
        ..cfg.clone()
    };
    state.config_store.save_config(&cfg).await?;
    // Publish the new config to the stream task via the watch channel. The
    // running stream picks up the change on its next tick (or immediately
    // via `cfg_rx.changed()`) without restarting — no 16 MB stop-points
    // re-warm, no fresh arrivals burst, and the shared client's caches
    // stay populated. `send` only fails if every receiver has dropped, in
    // which case the stream task is already dead and the watcher loop in
    // `lib.rs` will respawn it.
    let _ = state.cfg_tx.send(cfg);
    Ok(())
}

pub(crate) async fn load_config_inner(state: &AppState) -> Result<BoardConfig, String> {
    let mut cfg = state.config_store.load_config().await?;
    cfg.line_ids = migrate_legacy_line_ids(cfg.line_ids);
    Ok(cfg)
}

/// The six Overground line ids that replaced the legacy `"london-overground"`
/// id when TfL split the network in November 2024. Stable display order
/// (alphabetical, matching `web/src/lib/utils/format.ts::LINE_LABELS`).
pub(crate) const NAMED_OVERGROUND_LINES: &[&str] = &[
    "liberty",
    "lioness",
    "mildmay",
    "suffragette",
    "weaver",
    "windrush",
];

/// One-shot migration: rewrite a stored `BoardConfig.line_ids` so it stays
/// in sync with the canonical line forms used by station metadata,
/// arrivals (post `tfl_domain::canonicalize_line_id`), and the chip UI:
///
/// 1. `"london-overground"` (legacy) → six successor named OG ids
///    (`NAMED_OVERGROUND_LINES`). The live API stopped emitting predictions
///    under the legacy id when the named lines launched; without this
///    rewrite an upgrading user silently loses their Overground board.
///
/// 2. `"elizabeth-line"` (mode-form) → `"elizabeth"` (line-form).
///    Historical configs saved the chip as the mode-form because the
///    settings UI's `KNOWN_LINES` used to too. Arrival deserialization
///    canonicalises to `"elizabeth"`, so a stale `"elizabeth-line"` chip
///    silently masks every Elizabeth arrival.
///
/// Idempotent. Stable order: existing entries preserved in place; new
/// entries appended where the legacy id used to be; cross-list dedupe.
pub(crate) fn migrate_legacy_line_ids(line_ids: Vec<String>) -> Vec<String> {
    let needs_overground = line_ids.iter().any(|id| id == "london-overground");
    let needs_elizabeth = line_ids.iter().any(|id| id == "elizabeth-line");
    if !needs_overground && !needs_elizabeth {
        return line_ids;
    }

    let mut out = Vec::with_capacity(line_ids.len() + NAMED_OVERGROUND_LINES.len());
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut expanded = false;
    for id in line_ids {
        if id == "london-overground" {
            if !expanded {
                for &named in NAMED_OVERGROUND_LINES {
                    if seen.insert(named.to_string()) {
                        out.push(named.to_string());
                    }
                }
                expanded = true;
            }
            continue;
        }
        let canonical = if id == "elizabeth-line" {
            "elizabeth".to_string()
        } else {
            id
        };
        if seen.insert(canonical.clone()) {
            out.push(canonical);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Favorites inner functions
// ---------------------------------------------------------------------------

/// Run `migrate_legacy_line_ids` over each favorite's `lines` field so a
/// favorite saved before the November 2024 Overground rename still displays
/// the correct chips (invariant #14).
fn migrate_favorite_lines(favorites: Vec<Favorite>) -> Vec<Favorite> {
    favorites
        .into_iter()
        .map(|mut fav| {
            let line_ids: Vec<String> = fav.lines.iter().map(|l| l.id.clone()).collect();
            let migrated_ids = migrate_legacy_line_ids(line_ids);
            // Rebuild LineRef list: keep existing name where id is unchanged,
            // synthesise a name for newly-expanded ids.
            fav.lines = migrated_ids
                .into_iter()
                .map(|id| {
                    // Prefer the existing LineRef name if the id is unchanged.
                    if let Some(existing) = fav.lines.iter().find(|l| l.id == id) {
                        existing.clone()
                    } else {
                        LineRef {
                            name: tfl_domain::pretty_line_name(&id).to_string(),
                            id,
                        }
                    }
                })
                .collect();
            fav
        })
        .collect()
}

pub(crate) async fn load_favorites_inner(state: &AppState) -> Result<Vec<Favorite>, String> {
    let raw = state.favorites_store.load_favorites().await?;
    Ok(migrate_favorite_lines(raw))
}

pub(crate) async fn add_favorite_inner(
    station_id: String,
    common_name: String,
    lines: Vec<LineRef>,
    state: &AppState,
) -> Result<Vec<Favorite>, String> {
    // Validate station_id.
    validate_station_id(&station_id)?;
    // Validate common_name length (LOW-2: unbounded strings → JSON bloat).
    validate_common_name(&common_name)?;
    // Validate each line id and name.
    for line in &lines {
        validate_line_id(&line.id)?;
        validate_line_name(&line.name)?;
        if !is_supported_line_id(&line.id) {
            return Err(format!(
                "validation: line_id {:?} is not a supported TfL line",
                line.id
            ));
        }
    }

    let mut favorites = load_favorites_inner(state).await?;

    // Idempotent: skip if station_id already present.
    if favorites.iter().any(|f| f.station_id == station_id) {
        return Ok(favorites);
    }

    favorites.push(Favorite {
        station_id,
        common_name,
        lines,
    });
    state.favorites_store.save_favorites(&favorites).await?;
    Ok(favorites)
}

pub(crate) async fn remove_favorite_inner(
    station_id: String,
    state: &AppState,
) -> Result<Vec<Favorite>, String> {
    validate_station_id(&station_id)?;
    let mut favorites = load_favorites_inner(state).await?;
    favorites.retain(|f| f.station_id != station_id);
    state.favorites_store.save_favorites(&favorites).await?;
    Ok(favorites)
}

pub(crate) async fn save_app_key_inner(
    key: Option<String>,
    state: &AppState,
) -> Result<String, String> {
    validate_app_key(&key)?;
    state.config_store.save_app_key(key).await?;
    Ok("restart to apply".to_string())
}

pub(crate) async fn load_app_key_inner(state: &AppState) -> Result<Option<String>, String> {
    state.config_store.load_app_key().await
}

pub(crate) async fn has_app_key_inner(state: &AppState) -> Result<bool, String> {
    let key = state.config_store.load_app_key().await?;
    Ok(key.is_some())
}

/// Validate a board-window size in logical pixels.
///
/// Bounds are deliberately wider than the current preset table so the
/// renderer keeps full control over its tier choices, but tight enough that
/// a renderer-side bug (NaN, infinity, negative number) can't ask Cocoa for
/// a degenerate window. The lower bounds match the smallest reasonable
/// popover; the upper bounds cover a 3-line × 3-platform station on a 4K
/// display.
pub(crate) fn validate_board_size(width: f64, height: f64) -> Result<(f64, f64), String> {
    if !width.is_finite() || !height.is_finite() {
        return Err(format!(
            "validation: board size must be finite, got width={width}, height={height}"
        ));
    }
    const MIN_W: f64 = 320.0;
    const MAX_W: f64 = 1600.0;
    const MIN_H: f64 = 400.0;
    const MAX_H: f64 = 900.0;
    if !(MIN_W..=MAX_W).contains(&width) {
        return Err(format!(
            "validation: width must be in [{MIN_W}, {MAX_W}], got {width}"
        ));
    }
    if !(MIN_H..=MAX_H).contains(&height) {
        return Err(format!(
            "validation: height must be in [{MIN_H}, {MAX_H}], got {height}"
        ));
    }
    Ok((width, height))
}

/// Validate a `display_mode` argument. Only `"window"` and `"menubar"` are
/// accepted; the renderer must never persist an unrecognised value because
/// startup branches on this string and would silently fall back to default.
pub(crate) fn validate_display_mode(mode: &str) -> Result<(), String> {
    if mode == "window" || mode == "menubar" {
        Ok(())
    } else {
        Err(format!(
            "validation: display_mode must be \"window\" or \"menubar\", got {mode:?}"
        ))
    }
}

/// Persist a new display mode and update the live `AppState.display_mode`
/// lock so any reader (e.g. the `Focused(false)` click-away handler) sees
/// the new value immediately.
///
/// **Does not run UI side-effects.** The Tauri command wrapper is
/// responsible for calling `crate::apply_display_mode_effects` after this
/// returns, so the swap (tray, dock icon, window chrome) takes effect
/// without requiring a process restart. Splitting it this way keeps the
/// inner function unit-testable without a real Tauri `AppHandle`.
///
/// Returns the previous mode so the caller can decide whether the UI
/// effects need to run at all (no-op when unchanged).
pub(crate) async fn save_display_mode_inner(
    mode: &str,
    state: &AppState,
) -> Result<String, String> {
    validate_display_mode(mode)?;
    state.config_store.save_display_mode(mode).await?;
    let prev = {
        let mut cur = state
            .display_mode
            .write()
            .map_err(|e| format!("display_mode lock poisoned: {e}"))?;
        let prev = cur.clone();
        *cur = mode.to_string();
        prev
    };
    Ok(prev)
}

pub(crate) async fn load_display_mode_inner(state: &AppState) -> Result<String, String> {
    state.config_store.load_display_mode().await
}

/// Persist the desktop display preferences. Does **not** publish to
/// `cfg_tx` — `DisplayPrefs` is a frontend-only render flag (mirrors the
/// favorites precedent: invariant test
/// `add_favorite_does_not_publish_to_cfg_tx`). Backend filtering must
/// keep handing the full set through; the renderer collapses on display.
pub(crate) async fn save_display_prefs_inner(
    prefs: &DisplayPrefs,
    state: &AppState,
) -> Result<(), String> {
    state.config_store.save_display_prefs(prefs).await
}

pub(crate) async fn load_display_prefs_inner(state: &AppState) -> Result<DisplayPrefs, String> {
    state.config_store.load_display_prefs().await
}

pub(crate) async fn get_line_status_inner(
    line_id: &str,
    state: &AppState,
) -> Result<LineStatus, String> {
    validate_line_id(line_id)?;
    let status = crate::state::AnyBoardService::get_line_status(&*state.board_service, line_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(status)
}

pub(crate) async fn get_all_line_statuses_inner(
    state: &AppState,
) -> Result<Vec<LineStatus>, String> {
    crate::state::AnyBoardService::get_all_line_statuses(&*state.board_service)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tauri command handlers (thin wrappers over the inner functions)
// ---------------------------------------------------------------------------

/// Search for tube stations by name.
///
/// `query` is matched case-insensitively against station common names.
/// Returns a list of matching `Station` objects.
#[tauri::command]
pub async fn search_stations(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<Station>, String> {
    search_stations_inner(&query, &state).await
}

/// Find the closest stations to a `(lat, lon)` query point.
///
/// `limit` is clamped at the validation layer to `[1, 20]`. NaN and
/// infinity are rejected. Results are sorted ascending by haversine
/// distance and capped at the 25 km radius defined in
/// `crates/tfl-client/src/nearest.rs` — out-of-network coords (Paris,
/// Manchester) yield an empty vector.
#[tauri::command]
pub async fn find_nearest_stations(
    lat: f64,
    lon: f64,
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<NearbyStation>, String> {
    find_nearest_stations_inner(lat, lon, limit, &state).await
}

/// Request a single CoreLocation fix from the macOS bridge.
///
/// Single-flight: a double-tap on the crosshair button serialises into
/// two sequential requests. Single-shot: each request creates a fresh
/// `CLLocationManager` and tears it down on completion. 8 s timeout.
///
/// On non-macOS targets this command returns an error — the iOS shell
/// substitutes its own platform bridge by including a sibling
/// `location.rs` and registering the same command from its own
/// `lib.rs` invoke handler.
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn request_current_location(
    app: tauri::AppHandle,
) -> Result<crate::location::LocationFix, crate::location::LocationError> {
    crate::location::request_current_location(app).await
}

/// Fetch the arrivals board for the currently saved station config.
///
/// Uses the `BoardConfig` from the config store (or the default: Oxford Circus).
///
/// M6 TODO: wire a polling stream via `BoardService::stream` + Tauri event
/// emission so the frontend receives updates without polling this command.
#[tauri::command]
pub async fn get_board(state: State<'_, AppState>) -> Result<Board, String> {
    get_board_inner(&state).await
}

/// Persist a `BoardConfig` to the store.
///
/// Validates all fields and clamps `poll_seconds` to [10, 300] before saving.
#[tauri::command]
pub async fn save_config(cfg: BoardConfig, state: State<'_, AppState>) -> Result<(), String> {
    save_config_inner(&cfg, &state).await
}

/// Load the currently saved `BoardConfig`, or return the default.
///
/// Default: Oxford Circus (`940GZZLUOXC`), no line/direction filter, 30 s poll.
#[tauri::command]
pub async fn load_config(state: State<'_, AppState>) -> Result<BoardConfig, String> {
    load_config_inner(&state).await
}

/// Persist an optional TfL API key.
///
/// Pass `None` to clear the stored key. The client is constructed at startup
/// with the saved key; changes require a restart to take effect.
///
/// Returns `"restart to apply"` so the frontend can display a notice.
#[tauri::command]
pub async fn save_app_key(
    key: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    save_app_key_inner(key, &state).await
}

// SECURITY (MEDIUM-2): the TfL API key is a **Rust-only secret** — no renderer
// has a path to read its value.
//
// History: the key-reading UI once lived in a separate "settings" webview
// window, and `load_app_key` was gated to that window's label as a process /
// origin boundary. As of PR2 the Settings UI is an in-frame panel in the
// "main" window and the "settings" window is never created — so this gate is
// now an INERT BACKSTOP: it rejects every caller (no window is labelled
// "settings"). It is deliberately kept, not deleted: it is the last line of
// defence if someone ever reintroduces a renderer call path. The primary
// protection is now that the renderer has no `loadAppKey` wrapper at all
// (see web/src/lib/ipc/commands.ts) and never loads the value.
//
// Why enforce in the handler, not the capability JSON? Tauri v2 capability
// files control plugin permissions (`core:*`, `store:*`); custom
// `#[tauri::command]` handlers have no plugin-level ACL, so the check lives in
// the handler body via `WebviewWindow::label()`.
//
// `has_app_key` (boolean only) intentionally stays unrestricted so the renderer
// can show "configure your key" prompts without access to the value.

/// Returns `true` when the given window label is the (now-retired) settings
/// window. Since no window carries this label anymore, this returns `false` for
/// every real caller — see the SECURITY note above; the guard is a backstop.
///
/// `pub(crate)` so the unit test in `commands::tests` can assert the guard
/// directly without constructing a real `WebviewWindow`.
pub(crate) fn window_label_is_settings(label: &str) -> bool {
    label == "settings"
}

/// Load the stored TfL API key.
///
/// Returns `None` if no key has been saved.
///
/// # Security
///
/// Backstop guard: rejects any caller whose window label is not `"settings"`.
/// No window carries that label anymore (Settings is in-frame as of PR2), so in
/// practice this rejects **every** caller and the command is unreachable by
/// design. The renderer has no `loadAppKey` wrapper, so the key value never
/// crosses the IPC boundary into any webview. Kept as defence-in-depth against a
/// future reintroduction of a renderer call path.
#[tauri::command]
pub async fn load_app_key(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    if !window_label_is_settings(window.label()) {
        return Err(format!(
            "permission denied: load_app_key may only be called from the settings \
             window (caller: {:?})",
            window.label()
        ));
    }
    load_app_key_inner(&state).await
}

/// Returns `true` if a TfL API key has been stored, `false` otherwise.
///
/// Exposes only a boolean to the renderer — the actual key value never
/// leaves the Rust process. Use `load_app_key` only in privileged contexts.
#[tauri::command]
pub async fn has_app_key(state: State<'_, AppState>) -> Result<bool, String> {
    has_app_key_inner(&state).await
}

/// Set the menu-bar disruption indicator (Phase 3 menubar status). Swaps the
/// tray icon to the monochrome "disrupted" variant when `disrupted` is true.
/// Called from the frontend, which holds the live line statuses; the icon swap
/// itself hops to the macOS main thread inside `apply_tray_disruption`. No-op
/// in window mode (no tray). Fire-and-forget — errors are logged, not returned.
#[tauri::command]
pub fn set_tray_disruption(app: tauri::AppHandle, disrupted: bool) {
    crate::apply_tray_disruption(&app, disrupted);
}

/// Fetch the current status for a single TfL line.
///
/// `line_id` must be a valid lowercase line identifier (e.g. `"northern"`).
#[tauri::command]
pub async fn get_line_status(
    line_id: String,
    state: State<'_, AppState>,
) -> Result<LineStatus, String> {
    get_line_status_inner(&line_id, &state).await
}

/// Fetch the merged status for every TfL line across all surfaced modes
/// (tube, DLR, Overground, Elizabeth line), sorted worst-first by severity
/// then alphabetically by line id.
///
/// Shares the same 60 s line-status cache as `get_line_status` — no extra
/// TfL traffic when the per-line ticker has already warmed it.
#[tauri::command]
pub async fn get_all_line_statuses(state: State<'_, AppState>) -> Result<Vec<LineStatus>, String> {
    get_all_line_statuses_inner(&state).await
}

/// Return the public TfL pool keys for the TypeScript data path (`USE_TS_TFL`).
///
/// The webview can't read `POOL_KEYS_URL` itself — the endpoint sends no
/// `Access-Control-Allow-Origin`, so a cross-origin `fetch` is refused the body
/// — but the Rust shell's `reqwest` is immune to webview CORS. So the shell
/// proxies the (public, iOS-shared) keys to the renderer, which builds its
/// round-robin `KeyPool` and appends `app_key` to its direct TfL fetches.
/// Empty on any failure; the TS path then runs unauthenticated (fail-open).
/// Not a secret: unlike the personal `app_key`, these are published.
#[tauri::command]
pub async fn get_pool_keys() -> Vec<String> {
    crate::pool_key::fetch_all_pool_keys(crate::pool_key::POOL_KEYS_URL).await
}

/// Persist the display mode (`"window"` or `"menubar"`) and apply it
/// live — tray icon appears/disappears, dock icon toggles, window chrome
/// + size + always-on-top reconfigure, click-away behaviour tracks the
/// new mode — without a process restart.
///
/// Returns `"saved"` so the renderer can show a transient confirmation
/// chip; the actual mode change is reflected by the `displayMode` store
/// updating immediately.
#[tauri::command]
pub async fn save_display_mode(
    mode: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let prev = save_display_mode_inner(&mode, &state).await?;
    if prev != mode {
        // Hops to the macOS main thread internally — never call the
        // `_sync` variant from here. The Cocoa APIs reached by
        // `set_activation_policy` / tray remove / window resize all
        // assert main-thread-only and crash the process otherwise.
        crate::apply_display_mode_effects(&app, &mode).await?;
    }
    Ok("saved".to_string())
}

/// Load the persisted display mode. Defaults to `"window"`.
#[tauri::command]
pub async fn load_display_mode(state: State<'_, AppState>) -> Result<String, String> {
    load_display_mode_inner(&state).await
}

/// Persist the desktop display preferences (`group_destinations`, …).
///
/// Does **not** publish through `cfg_tx` — these are renderer-only flags;
/// the backend keeps shipping the full per-train set and the renderer
/// collapses on display. Mirrors the favorites precedent.
#[tauri::command]
pub async fn save_display_prefs(
    prefs: DisplayPrefs,
    state: State<'_, AppState>,
) -> Result<(), String> {
    save_display_prefs_inner(&prefs, &state).await
}

/// Load the persisted desktop display preferences. Returns the default
/// (`group_destinations: false`) when nothing has been saved.
#[tauri::command]
pub async fn load_display_prefs(state: State<'_, AppState>) -> Result<DisplayPrefs, String> {
    load_display_prefs_inner(&state).await
}

/// Return the current favorites list, applying legacy-id migration on load.
#[tauri::command]
pub async fn list_favorites(state: State<'_, AppState>) -> Result<Vec<Favorite>, String> {
    load_favorites_inner(&state).await
}

/// Add a station to favorites.
///
/// Idempotent: if `station_id` is already in the list this is a no-op.
/// Returns the updated list.
///
/// Does **not** publish to `cfg_tx` — the stream pipeline is unchanged.
/// Selecting a favorite goes through the existing `save_config` command.
#[tauri::command]
pub async fn add_favorite(
    station_id: String,
    common_name: String,
    lines: Vec<LineRef>,
    state: State<'_, AppState>,
) -> Result<Vec<Favorite>, String> {
    add_favorite_inner(station_id, common_name, lines, &state).await
}

/// Remove a station from favorites by `station_id`.
///
/// If the station is not in the list this is a no-op. Returns the updated list.
///
/// Does **not** publish to `cfg_tx`.
#[tauri::command]
pub async fn remove_favorite(
    station_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Favorite>, String> {
    remove_favorite_inner(station_id, &state).await
}

/// Resize the main window to fit the current board.
///
/// The renderer picks `(width, height)` from a small preset table tied to
/// the current display mode and the station's line / platform count, and
/// calls this command whenever the picked tier changes (it dedupes
/// renderer-side so the IPC isn't hit on every board tick).
///
/// Hops to the macOS main thread internally — `set_size` reaches Cocoa,
/// which asserts main-thread-only and crashes the process otherwise (see
/// invariant #8 in `CLAUDE.md`). Validation runs *before* dispatch so an
/// out-of-range request from a buggy renderer never reaches Cocoa.
#[tauri::command]
pub async fn apply_board_size(
    width: f64,
    height: f64,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let (w, h) = validate_board_size(width, height)?;
    crate::apply_board_size_effects(&app, w, h).await
}

// ---------------------------------------------------------------------------
// Updater commands (M8 PR-D)
// ---------------------------------------------------------------------------

/// IPC-boundary DTO for an available update. Mirrors the subset of
/// `tauri_plugin_updater::Update` fields the renderer actually displays.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateInfoDto {
    /// New version (e.g. "0.1.1").
    pub version: String,
    /// Currently-installed version, captured at check time so the renderer
    /// can show "0.1.0 -> 0.1.1" without a second IPC round-trip.
    pub current_version: String,
    /// Markdown release notes from the manifest. Empty string when absent.
    pub body: String,
}

/// Check the updater endpoint for a newer version. Returns:
///
/// - `Ok(None)` when no update is available or `plugins.updater.active`
///   is `false` (the plugin short-circuits in that case).
/// - `Ok(Some(_))` when a newer signed version is available.
/// - `Err(_)` for network or signature failures. The renderer distinguishes
///   the two via the error message — a `signature` substring routes to the
///   security-event copy in the Settings banner.
#[tauri::command]
pub async fn check_for_updates(app: tauri::AppHandle) -> Result<Option<UpdateInfoDto>, String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|e| format!("updater build: {e}"))?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(UpdateInfoDto {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            body: update.body.clone().unwrap_or_default(),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(format!("check_for_updates: {e}")),
    }
}

/// Download and install the latest signed update, then restart the app.
///
/// Re-checks the endpoint inside the command so the install operates on
/// whatever is currently signed-and-published — avoids state-management
/// of a held `Update` handle across IPC calls. Worst case (publisher
/// pulled the release between `check_for_updates` and `install_update`):
/// the command returns `Err` and the renderer surfaces the failure.
///
/// `download_and_install` downloads, signature-verifies and stages the
/// new `.app` bundle into `/Applications`. On macOS that's **all** it
/// does — the running process is not killed and not relaunched. The
/// v0.1.1 stuck-install bug was this command returning `Ok(())` with
/// the bundle already swapped on disk while the v0.1.0 process kept
/// running, so the renderer's `await installUpdate()` resolved into a
/// frozen `'installing'` UI. v0.1.2 fixes it by emitting
/// `updater://restart-imminent` (so the renderer can show a transient
/// `'restarting'` state) and then invoking `app.restart()`, which
/// `exec`s a fresh process from the new bundle.
///
/// The two no-op progress callbacks satisfy the API contract without
/// piping bytes across IPC for v0.1.x — release artifacts are ~10 MB
/// over modern broadband, well under the 2 s threshold where a
/// progress bar pays for itself (revisit in v0.2.0 if user feedback
/// warrants).
///
/// `app.restart()` returns `-> !`, so no code can follow it; any error
/// the renderer might observe comes from the pre-restart phase.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|e| format!("updater build: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("install_update check: {e}"))?
        .ok_or_else(|| "install_update: no update available".to_string())?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("install_update: {e}"))?;
    // Best-effort: fire the event before restart so the renderer can
    // paint the "Restarting Tubbie…" state even if the event delivery
    // races the process death. Ignore emit failures — restart still
    // happens, and the renderer's 5 s fallback timer will catch any
    // pathological case where neither the event nor the restart fires.
    let _ = app.emit("updater://restart-imminent", ());
    app.restart();
}

/// Load the persisted auto-update preferences. Returns the default
/// (`auto_check: true`) when nothing has been saved.
#[tauri::command]
pub async fn load_update_prefs(state: State<'_, AppState>) -> Result<UpdatePrefs, String> {
    state.config_store.load_update_prefs().await
}

/// Persist the auto-update preferences.
///
/// Does **not** publish through `cfg_tx` — these flags don't affect the
/// arrivals-board pipeline. Mirrors the `display_prefs` precedent.
#[tauri::command]
pub async fn save_update_prefs(
    prefs: UpdatePrefs,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.config_store.save_update_prefs(&prefs).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{MemoryConfigStore, MemoryFavoritesStore};
    use std::sync::Arc;
    use tfl_board::{BoardService, LifecyclePhase};
    use tfl_cache::TflClient;
    use tfl_client::{clock::FakeClock, fixture::FixtureTflHttp};
    use tokio::sync::{watch, RwLock};

    /// Path to the workspace fixtures directory (relative to this crate's manifest).
    fn fixture_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
    }

    /// Build an `AppState` backed by fixture files + in-memory config.
    ///
    /// **IMPORTANT — known limitation.** This helper drops the watch
    /// `Receiver`, so `state.cfg_tx.send(...)` inside `save_config_inner`
    /// returns `Err(NoReceivers)` — silently swallowed by `let _ =`. Tests
    /// that need to assert *anything* about the cfg_tx → stream pipeline
    /// MUST use [`fixture_state_with_stream`] instead. Using this helper
    /// for save-then-observe scenarios is what allowed the
    /// "save_config doesn't reach the stream" class of regressions to slip
    /// past CI.
    fn fixture_state() -> AppState {
        let clock = FakeClock::from_rfc3339("2025-01-15T10:00:00Z").unwrap();
        let http = FixtureTflHttp::new(fixture_dir());
        let client = Arc::new(TflClient::new(http));
        let board_service =
            Arc::new(BoardService::new(client, clock)) as Arc<dyn crate::state::AnyBoardService>;
        let config_store = Arc::new(MemoryConfigStore::new()) as Arc<dyn crate::state::ConfigStore>;
        let favorites_store =
            Arc::new(MemoryFavoritesStore::new()) as Arc<dyn crate::state::FavoritesStore>;
        let (cfg_tx, _cfg_rx) = watch::channel::<BoardConfig>(crate::state::default_board_config());
        AppState {
            board_service,
            config_store,
            favorites_store,
            stream_abort: Arc::new(RwLock::new(None)),
            cfg_tx: Arc::new(cfg_tx),
            display_mode: Arc::new(std::sync::RwLock::new("window".to_string())),
            lifecycle: Arc::new(LifecyclePhase::always_active()),
        }
    }

    /// Build an `AppState` plus a live stream `Receiver` that mirrors the
    /// production wiring: AppState holds `cfg_tx`, the stream reads from a
    /// matching `cfg_rx`, and both `BoardService` instances share one
    /// `TflClient` and one clock — exactly as `lib.rs::run` does.
    ///
    /// Returns `(state, stream, fixture_dir_clock)` so tests can:
    ///   1. Call `save_config_inner(&new_cfg, &state)` to drive the real
    ///      command path (validation + persist + cfg_tx.send).
    ///   2. Poll `stream.next().await` to observe what the stream task in
    ///      production would emit as a `board://updated` event.
    ///
    /// Use this helper for any test that asserts an effect of save_config
    /// on the stream. The dropped-receiver `fixture_state()` will silently
    /// pass even when the pipeline is broken.
    /// Boxed `Board` stream returned by [`fixture_state_with_stream`].
    type BoardStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<tfl_domain::Board, tfl_board::BoardError>> + Send>,
    >;

    fn fixture_state_with_stream(seed: BoardConfig) -> (AppState, BoardStream) {
        let clock = FakeClock::from_rfc3339("2026-01-15T10:00:00Z").unwrap();
        let http = FixtureTflHttp::new(fixture_dir());
        let client = Arc::new(TflClient::new(http));

        // Both BoardServices share the same Arc<TflClient> and clock,
        // mirroring the production wiring where `AppState.board_service`
        // and `spawn_stream_task` use the same `Arc::clone(&client)`.
        let board_service = Arc::new(BoardService::new(Arc::clone(&client), clock.clone()))
            as Arc<dyn crate::state::AnyBoardService>;
        let stream_svc = BoardService::new(Arc::clone(&client), clock);

        let config_store = Arc::new(MemoryConfigStore::new()) as Arc<dyn crate::state::ConfigStore>;
        let favorites_store =
            Arc::new(MemoryFavoritesStore::new()) as Arc<dyn crate::state::FavoritesStore>;
        let (cfg_tx, cfg_rx) = watch::channel::<BoardConfig>(seed);
        let cfg_tx = Arc::new(cfg_tx);

        let lifecycle = Arc::new(LifecyclePhase::always_active());
        let phase_rx = lifecycle.subscribe();
        let state = AppState {
            board_service,
            config_store,
            favorites_store,
            stream_abort: Arc::new(RwLock::new(None)),
            cfg_tx,
            display_mode: Arc::new(std::sync::RwLock::new("window".to_string())),
            lifecycle,
        };

        let stream: BoardStream = Box::pin(stream_svc.stream(cfg_rx, phase_rx));

        (state, stream)
    }

    // -----------------------------------------------------------------------
    // Validation: station_id
    // -----------------------------------------------------------------------

    #[test]
    fn station_id_too_long_is_rejected() {
        let long_id = "A".repeat(33);
        let result = validate_station_id(&long_id);
        assert!(
            result.is_err(),
            "should reject station_id longer than 32 chars"
        );
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn station_id_empty_is_rejected() {
        let result = validate_station_id("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn station_id_with_slash_is_rejected() {
        let result = validate_station_id("940GZZLU/BZP");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn station_id_with_path_traversal_is_rejected() {
        let result = validate_station_id("../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn station_id_with_null_byte_is_rejected() {
        let result = validate_station_id("940GZZLU\0BZP");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn station_id_valid_passes() {
        assert!(validate_station_id("940GZZLUBZP").is_ok());
        assert!(validate_station_id("STOP-1_A").is_ok());
    }

    // -----------------------------------------------------------------------
    // Validation: line_id
    // -----------------------------------------------------------------------

    #[test]
    fn line_id_uppercase_is_rejected() {
        let result = validate_line_id("Northern");
        assert!(result.is_err(), "uppercase line_id should be rejected");
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn line_id_empty_is_rejected() {
        assert!(validate_line_id("").is_err());
    }

    #[test]
    fn line_id_too_long_is_rejected() {
        let long_id = "a".repeat(33);
        assert!(validate_line_id(&long_id).is_err());
    }

    #[test]
    fn line_id_with_slash_is_rejected() {
        assert!(validate_line_id("nort/hern").is_err());
    }

    #[test]
    fn line_id_with_path_traversal_is_rejected() {
        assert!(validate_line_id("../secret").is_err());
    }

    #[test]
    fn line_id_valid_passes() {
        assert!(validate_line_id("northern").is_ok());
        assert!(validate_line_id("elizabeth-line").is_ok());
        assert!(validate_line_id("h2").is_ok());
    }

    // -----------------------------------------------------------------------
    // Validation: query
    // -----------------------------------------------------------------------

    #[test]
    fn query_too_long_is_rejected() {
        let long = "a".repeat(101);
        let result = validate_query(&long);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn query_with_null_byte_is_rejected() {
        let result = validate_query("Kings\0Cross");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn query_valid_passes() {
        assert!(validate_query("Belsize Park").is_ok());
        assert!(validate_query("").is_ok());
        assert!(validate_query(&"a".repeat(100)).is_ok());
    }

    // -----------------------------------------------------------------------
    // Validation: app_key
    // -----------------------------------------------------------------------

    #[test]
    fn app_key_too_long_is_rejected() {
        let long_key = Some("a".repeat(65));
        let result = validate_app_key(&long_key);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    #[test]
    fn app_key_with_null_byte_is_rejected() {
        let key = Some("deadbeef\0bad".to_string());
        assert!(validate_app_key(&key).is_err());
    }

    #[test]
    fn app_key_none_passes() {
        assert!(validate_app_key(&None).is_ok());
    }

    #[test]
    fn app_key_valid_passes() {
        assert!(validate_app_key(&Some("abc123".to_string())).is_ok());
        assert!(validate_app_key(&Some("a".repeat(64))).is_ok());
    }

    // -----------------------------------------------------------------------
    // Validation: poll_seconds clamping
    // -----------------------------------------------------------------------

    #[test]
    fn poll_seconds_below_min_is_clamped() {
        assert_eq!(clamp_poll_seconds(0), 10);
        assert_eq!(clamp_poll_seconds(9), 10);
    }

    #[test]
    fn poll_seconds_above_max_is_clamped() {
        assert_eq!(clamp_poll_seconds(301), 300);
        assert_eq!(clamp_poll_seconds(9999), 300);
    }

    #[test]
    fn poll_seconds_in_range_passes_through() {
        assert_eq!(clamp_poll_seconds(20), 20);
        assert_eq!(clamp_poll_seconds(10), 10);
        assert_eq!(clamp_poll_seconds(300), 300);
    }

    // -----------------------------------------------------------------------
    // Validation: board_config length caps (Fix 4)
    // -----------------------------------------------------------------------

    #[test]
    fn board_config_too_many_line_ids_is_rejected() {
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: (0..33).map(|i| format!("line{i}")).collect(),
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let err = validate_board_config(&cfg).expect_err("should reject >32 line_ids");
        assert!(err.contains("validation:"), "error: {err}");
        assert!(err.contains("line_ids"), "error: {err}");
    }

    #[test]
    fn board_config_exactly_32_line_ids_is_accepted() {
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec!["northern".to_string(); 32],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        assert!(
            validate_board_config(&cfg).is_ok(),
            "32 line_ids should pass"
        );
    }

    #[test]
    fn board_config_too_many_directions_is_rejected() {
        use tfl_domain::Direction;
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![Direction::Inbound; 17],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let err = validate_board_config(&cfg).expect_err("should reject >16 directions");
        assert!(err.contains("validation:"), "error: {err}");
        assert!(err.contains("directions"), "error: {err}");
    }

    #[test]
    fn board_config_exactly_16_directions_is_accepted() {
        use tfl_domain::Direction;
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![Direction::Inbound; 16],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        assert!(
            validate_board_config(&cfg).is_ok(),
            "16 directions should pass"
        );
    }

    // -----------------------------------------------------------------------
    // Legacy `london-overground` migration
    // -----------------------------------------------------------------------

    #[test]
    fn migrate_legacy_line_ids_rewrites_legacy_overground() {
        let input = vec![
            "northern".to_string(),
            "london-overground".to_string(),
            "victoria".to_string(),
        ];
        let out = migrate_legacy_line_ids(input);
        assert_eq!(
            out,
            vec![
                "northern",
                "liberty",
                "lioness",
                "mildmay",
                "suffragette",
                "weaver",
                "windrush",
                "victoria"
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
            "legacy id must expand in-place to the six named lines while preserving order"
        );
    }

    #[test]
    fn migrate_legacy_line_ids_is_idempotent() {
        let input = vec!["london-overground".to_string()];
        let once = migrate_legacy_line_ids(input);
        let twice = migrate_legacy_line_ids(once.clone());
        assert_eq!(once, twice, "second pass must be a no-op");
    }

    #[test]
    fn migrate_legacy_line_ids_dedupes_against_existing_named_ids() {
        // User had `mildmay` already and the legacy id; trailing windrush
        // is also explicit. Expansion expands at the legacy id's position
        // and skips ids already seen anywhere in the list.
        let input = vec![
            "mildmay".to_string(),
            "london-overground".to_string(),
            "windrush".to_string(),
        ];
        let out = migrate_legacy_line_ids(input);
        assert_eq!(
            out,
            vec![
                "mildmay",
                "liberty",
                "lioness",
                "suffragette",
                "weaver",
                "windrush"
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
            "expansion must skip ids the user already has (mildmay, windrush) \
             and preserve their original positions",
        );
    }

    #[test]
    fn migrate_legacy_line_ids_passes_through_modern_config() {
        let input = vec!["northern".to_string(), "mildmay".to_string()];
        let out = migrate_legacy_line_ids(input.clone());
        assert_eq!(
            out, input,
            "configs without the legacy id must be returned unchanged"
        );
    }

    #[test]
    fn migrate_legacy_line_ids_rewrites_elizabeth_line_to_elizabeth() {
        // Historical iOS / desktop builds saved the Elizabeth chip as
        // `"elizabeth-line"` (the mode form) because `KNOWN_LINES` used
        // that id. After `tfl_domain::canonicalize_line_id` runs at
        // arrival ingest, the wire-side id is `"elizabeth"` — so the
        // stale chip silently masks every Elizabeth arrival. The
        // migration rewrites in place and preserves position.
        let input = vec![
            "northern".to_string(),
            "elizabeth-line".to_string(),
            "victoria".to_string(),
        ];
        let out = migrate_legacy_line_ids(input);
        assert_eq!(
            out,
            vec![
                "northern".to_string(),
                "elizabeth".to_string(),
                "victoria".to_string(),
            ],
        );
    }

    #[test]
    fn migrate_legacy_line_ids_dedupes_elizabeth_against_existing_canonical_id() {
        // If the user has both forms saved (one from the chip click pre-
        // migration, one introduced post-canonicalisation), keep the
        // first occurrence and drop the duplicate.
        let input = vec![
            "elizabeth".to_string(),
            "northern".to_string(),
            "elizabeth-line".to_string(),
        ];
        let out = migrate_legacy_line_ids(input);
        assert_eq!(out, vec!["elizabeth".to_string(), "northern".to_string()],);
    }

    #[test]
    fn migrate_legacy_line_ids_handles_overground_and_elizabeth_together() {
        let input = vec![
            "elizabeth-line".to_string(),
            "london-overground".to_string(),
        ];
        let out = migrate_legacy_line_ids(input);
        assert_eq!(
            out,
            vec![
                "elizabeth".to_string(),
                "liberty".to_string(),
                "lioness".to_string(),
                "mildmay".to_string(),
                "suffragette".to_string(),
                "weaver".to_string(),
                "windrush".to_string(),
            ],
        );
    }

    #[tokio::test]
    async fn load_config_migrates_legacy_london_overground_id() {
        let state = fixture_state();
        // Simulate a pre-2024 stored config by saving via the lower-level
        // `set_raw` API on MemoryConfigStore — bypasses the modern
        // validate_board_config which would reject the legacy id today.
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec!["northern".to_string(), "london-overground".to_string()],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        state
            .config_store
            .save_config(&cfg)
            .await
            .expect("save legacy config");
        let loaded = load_config_inner(&state).await.expect("load");
        assert!(
            !loaded.line_ids.iter().any(|id| id == "london-overground"),
            "legacy id must be stripped on load; got {:?}",
            loaded.line_ids
        );
        for &named in NAMED_OVERGROUND_LINES {
            assert!(
                loaded.line_ids.iter().any(|id| id == named),
                "loaded config must include all six named Overground lines; missing {named}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Config round-trip
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_and_load_config_round_trips() {
        let state = fixture_state();
        let cfg = BoardConfig {
            station_id: "940GZZLUKSX".to_string(),
            line_ids: vec!["northern".to_string()],
            directions: vec![],
            poll_seconds: 30,
            theme: "classic-amber".to_string(),
        };
        save_config_inner(&cfg, &state)
            .await
            .expect("save should succeed");
        let loaded = load_config_inner(&state)
            .await
            .expect("load should succeed");
        assert_eq!(loaded.station_id, "940GZZLUKSX");
        assert_eq!(loaded.line_ids, vec!["northern"]);
        assert_eq!(loaded.poll_seconds, 30);
    }

    #[tokio::test]
    async fn load_config_returns_default_when_nothing_saved() {
        let state = fixture_state();
        let cfg = load_config_inner(&state)
            .await
            .expect("should return default");
        assert_eq!(cfg.station_id, "940GZZLUOXC");
        assert_eq!(cfg.poll_seconds, 30);
    }

    #[tokio::test]
    async fn save_config_clamps_poll_seconds() {
        let state = fixture_state();
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 9999,
            theme: "classic-amber".to_string(),
        };
        save_config_inner(&cfg, &state)
            .await
            .expect("save should succeed");
        let loaded = load_config_inner(&state).await.unwrap();
        assert_eq!(
            loaded.poll_seconds, 300,
            "poll_seconds should be clamped to 300"
        );
    }

    #[tokio::test]
    async fn save_config_rejects_invalid_station_id() {
        let state = fixture_state();
        let cfg = BoardConfig {
            station_id: "../etc/passwd".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let err = save_config_inner(&cfg, &state)
            .await
            .expect_err("should reject invalid station_id");
        assert!(err.contains("validation:"), "error: {err}");
    }

    #[tokio::test]
    async fn save_config_rejects_invalid_line_id() {
        let state = fixture_state();
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec!["Northern".to_string()], // uppercase — invalid
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let err = save_config_inner(&cfg, &state)
            .await
            .expect_err("should reject uppercase line_id");
        assert!(err.contains("validation:"), "error: {err}");
    }

    #[tokio::test]
    async fn save_config_rejects_too_many_line_ids() {
        let state = fixture_state();
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec!["northern".to_string(); 33],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let err = save_config_inner(&cfg, &state)
            .await
            .expect_err("should reject >32 line_ids");
        assert!(err.contains("validation:"), "error: {err}");
    }

    // -----------------------------------------------------------------------
    // App key round-trip
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_and_load_app_key_round_trips() {
        let state = fixture_state();
        let result = save_app_key_inner(Some("my-secret-key".to_string()), &state)
            .await
            .expect("should save");
        assert_eq!(result, "restart to apply");
        let key = load_app_key_inner(&state).await.expect("should load");
        assert_eq!(key, Some("my-secret-key".to_string()));
    }

    #[tokio::test]
    async fn clear_app_key_works() {
        let state = fixture_state();
        save_app_key_inner(Some("key".to_string()), &state)
            .await
            .unwrap();
        save_app_key_inner(None, &state).await.unwrap();
        let key = load_app_key_inner(&state).await.unwrap();
        assert_eq!(key, None);
    }

    #[tokio::test]
    async fn save_app_key_rejects_too_long_key() {
        let state = fixture_state();
        let err = save_app_key_inner(Some("a".repeat(65)), &state)
            .await
            .expect_err("should reject key > 64 chars");
        assert!(err.contains("validation:"), "error: {err}");
    }

    #[tokio::test]
    async fn save_app_key_rejects_null_byte() {
        let state = fixture_state();
        let err = save_app_key_inner(Some("dead\0beef".to_string()), &state)
            .await
            .expect_err("should reject null byte in key");
        assert!(err.contains("validation:"), "error: {err}");
    }

    // -----------------------------------------------------------------------
    // spawn_blocking proof: simulated blocking I/O
    // -----------------------------------------------------------------------

    /// Proves that wrapping a blocking `save` in `spawn_blocking` allows other
    /// async tasks on the same Tokio runtime to make progress concurrently.
    ///
    /// The test spawns a "slow save" task (200ms block) and a concurrent
    /// "heartbeat" task. With `spawn_blocking`, the heartbeat finishes while
    /// the save is still blocking its OS thread. Without it, the worker thread
    /// would stall and the heartbeat could not complete until the save returned.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_blocking_does_not_stall_worker_thread() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let heartbeat_done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&heartbeat_done);

        // Heartbeat: completes after ~10ms — well before the 200ms blocking save.
        let heartbeat = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            flag.store(true, Ordering::SeqCst);
        });

        // Slow save wrapped in spawn_blocking so it doesn't stall any worker.
        let slow_save = tokio::task::spawn_blocking(|| {
            std::thread::sleep(std::time::Duration::from_millis(200));
        });

        // Both must complete within a generous timeout.
        tokio::time::timeout(std::time::Duration::from_millis(500), async move {
            heartbeat.await.expect("heartbeat task panicked");
            slow_save.await.expect("slow_save task panicked");
        })
        .await
        .expect("timed out — spawn_blocking may be stalling the runtime");

        assert!(
            heartbeat_done.load(Ordering::SeqCst),
            "heartbeat should have completed while slow save was running"
        );
    }

    // -----------------------------------------------------------------------
    // get_board
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_board_returns_board_for_default_config() {
        let state = fixture_state();
        let board = get_board_inner(&state).await.expect("should return board");
        assert_eq!(board.station_id, "940GZZLUOXC");
        assert!(!board.platforms.is_empty(), "board should have platforms");
    }

    #[tokio::test]
    async fn get_board_uses_saved_config() {
        let state = fixture_state();
        let cfg = BoardConfig {
            station_id: "940GZZLUKSX".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        save_config_inner(&cfg, &state).await.unwrap();
        let board = get_board_inner(&state).await.unwrap();
        assert_eq!(board.station_id, "940GZZLUKSX");
    }

    /// End-to-end: saving a new station with a multi-line `line_ids`
    /// filter MUST land the station, but MUST NOT pre-filter arrivals
    /// — `line_ids` is a frontend-only display mask now (CLAUDE.md
    /// invariant #22). The backend's `Board` payload contains every
    /// arrival the station serves; the chip filter is applied at the
    /// `Board.svelte::displayPlatforms` derivation in the renderer.
    /// Guarding the contract here keeps a future refactor of
    /// `apply_filters` from accidentally re-introducing backend
    /// line filtering, which would re-introduce the
    /// 30-second tick delay between chip toggle and visible effect.
    #[tokio::test]
    async fn save_config_then_get_board_applies_station_but_does_not_filter_lines() {
        let state = fixture_state();

        // 1. Save a config pointing at King's Cross (multi-line station)
        //    with TWO line ids set — the user's chip-filter preference.
        let cfg = BoardConfig {
            station_id: "940GZZLUKSX".to_string(),
            line_ids: vec!["northern".to_string(), "victoria".to_string()],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        save_config_inner(&cfg, &state)
            .await
            .expect("save should succeed");

        // 2. Refresh the board. Station MUST be the saved one.
        let board = get_board_inner(&state)
            .await
            .expect("get_board should succeed");
        assert_eq!(
            board.station_id, "940GZZLUKSX",
            "station_id from saved config must drive refresh"
        );

        // 3. The backend MUST hand through arrivals from lines OTHER
        //    than `[northern, victoria]` — King's Cross's fixture
        //    contains Circle / Hammersmith & City / Metropolitan /
        //    Piccadilly arrivals that the frontend will mask out, but
        //    the backend's payload is the unfiltered set.
        let seen_lines: std::collections::HashSet<String> = board
            .platforms
            .iter()
            .flat_map(|p| p.arrivals.iter().map(|a| a.line_id.clone()))
            .collect();
        assert!(
            !seen_lines.is_empty(),
            "King's Cross fixture should produce arrivals"
        );
        let unfiltered_lines: Vec<&String> = seen_lines
            .iter()
            .filter(|id| id.as_str() != "northern" && id.as_str() != "victoria")
            .collect();
        assert!(
            !unfiltered_lines.is_empty(),
            "backend MUST pass non-allowed lines through (frontend masks them); \
             saw only: {seen_lines:?}"
        );
    }

    // -----------------------------------------------------------------------
    // End-to-end save_config → cfg_tx → stream pipeline
    //
    // These tests exercise the production wiring: AppState holds the watch
    // sender, the stream task (or test stand-in) holds a matching receiver,
    // and `save_config_inner` is the single seam that links them. They are
    // the only tests that catch regressions in:
    //
    //   - `save_config_inner` actually publishing to `cfg_tx`
    //   - the stream observing `cfg_rx.changed()` and refreshing
    //   - the immediate-refresh-on-station-change semantic
    //   - the back-and-forth (A → B → A) settling on the latest value
    //
    // Use `fixture_state_with_stream` here, NOT `fixture_state` — the latter
    // drops the receiver and silently masks any pipeline break.
    // -----------------------------------------------------------------------

    /// Saving a new `station_id` via `save_config_inner` MUST make the
    /// running stream re-emit a board for the new station — that is the
    /// whole point of the watch-channel refactor. A regression here looks
    /// to the user like "I changed station and the board never updated".
    #[tokio::test(start_paused = true)]
    async fn save_config_publishes_station_change_to_running_stream() {
        use futures::StreamExt;
        use std::time::Duration;

        let initial_cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 30,
            theme: "classic-amber".to_string(),
        };
        let (state, mut stream) = fixture_state_with_stream(initial_cfg.clone());
        // Persist the initial cfg so the store, watch channel, and stream
        // are all aligned before the user's "station change" save lands.
        save_config_inner(&initial_cfg, &state)
            .await
            .expect("baseline save_config must succeed");

        // 1. First emit: stream's initial tick fires immediately for BZP.
        let first = stream
            .next()
            .await
            .expect("stream must yield an initial board");
        let first_board = first.expect("first emit must be Ok");
        assert_eq!(first_board.station_id, "940GZZLUBZP");

        // 2. User picks King's Cross. Production calls `save_config` IPC →
        //    `save_config_inner`. We do the same here, no shortcut.
        let new_cfg = BoardConfig {
            station_id: "940GZZLUKSX".to_string(),
            ..initial_cfg
        };
        save_config_inner(&new_cfg, &state)
            .await
            .expect("station-change save_config must succeed");

        // 3. The stream must emit a board for the NEW station, and it must
        //    arrive in well under one poll interval — the immediate-refresh
        //    path is what makes a station change feel responsive.
        let before = tokio::time::Instant::now();
        let second = stream
            .next()
            .await
            .expect("stream must yield after station change");
        let elapsed = before.elapsed();
        let second_board = second.expect("second emit must be Ok");

        assert_eq!(
            second_board.station_id, "940GZZLUKSX",
            "save_config_inner must publish through cfg_tx so the stream \
             refreshes against the new station; got station_id {:?}",
            second_board.station_id
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "station change must trigger an immediate refresh; \
             took {elapsed:?} (poll_seconds=30, so a missed refresh shows ~30 s)"
        );
    }

    /// A→B→A with a real stream tick between each pick must produce three
    /// emits in order (A, B, A). Models the user who picks the wrong station,
    /// sees its board, then picks the right one. The intermediate B emit is
    /// what the user *sees* on screen between the two clicks; without it the
    /// UI feels frozen.
    ///
    /// (The pure-synchronous A→B→A case — both saves before the stream gets a
    /// chance to refresh — collapses correctly in the watch channel: the
    /// receiver only ever observes the latest value A, station_changed=false
    /// against the displayed A, and no new emit fires. That's the correct
    /// "no-op" behaviour and is covered by `save_config_filter_change_does_not_
    /// force_immediate_refresh`'s no-emit timeout assertion.)
    #[tokio::test(start_paused = true)]
    async fn save_config_a_then_b_then_a_emits_each_in_order() {
        use futures::StreamExt;
        use std::time::Duration;

        let cfg_a = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 30,
            theme: "classic-amber".to_string(),
        };
        let (state, mut stream) = fixture_state_with_stream(cfg_a.clone());
        save_config_inner(&cfg_a, &state).await.unwrap();

        // Emit 1: A.
        let e1 = stream.next().await.unwrap().unwrap();
        assert_eq!(e1.station_id, "940GZZLUBZP", "first emit is A");

        // Pick B and wait for its emit.
        let cfg_b = BoardConfig {
            station_id: "940GZZLUKSX".to_string(),
            ..cfg_a.clone()
        };
        save_config_inner(&cfg_b, &state).await.unwrap();
        let e2 = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("stream must emit after B save")
            .unwrap()
            .unwrap();
        assert_eq!(e2.station_id, "940GZZLUKSX", "second emit is B");

        // Now pick A again and assert we get A back.
        save_config_inner(&cfg_a, &state).await.unwrap();
        let e3 = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("stream must emit after returning to A")
            .unwrap()
            .unwrap();
        assert_eq!(
            e3.station_id, "940GZZLUBZP",
            "third emit must be A (the user came back); got {:?}",
            e3.station_id
        );
    }

    /// Filter changes (line_ids / directions / theme) must NOT cause a
    /// fresh refresh — the "cheap" cfg-changed semantic is the whole reason
    /// chip toggles don't burn through the rate limit. If a filter change
    /// triggers an extra fetch, the user's chip-burst-spam scenario regresses.
    ///
    /// We assert this indirectly by checking that the next emit only arrives
    /// AFTER advancing past the poll interval — not immediately on the cfg
    /// change.
    #[tokio::test(start_paused = true)]
    async fn save_config_filter_change_does_not_force_immediate_refresh() {
        use futures::StreamExt;
        use std::time::Duration;

        let initial_cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 60,
            theme: "classic-amber".to_string(),
        };
        let (state, mut stream) = fixture_state_with_stream(initial_cfg.clone());
        save_config_inner(&initial_cfg, &state).await.unwrap();

        // First emit (immediate).
        let _ = stream.next().await.unwrap().unwrap();

        // Toggle a directions filter — same station, just a different filter.
        let mut filter_change = initial_cfg.clone();
        filter_change.directions = vec![tfl_domain::Direction::Northbound];
        save_config_inner(&filter_change, &state).await.unwrap();

        // No time advance — the filter change alone must NOT produce a new
        // emit. (The stream's CfgChanged path with !station_changed `continue`s
        // and waits for the next interval tick.)
        let immediate = tokio::time::timeout(Duration::from_millis(50), stream.next()).await;
        assert!(
            immediate.is_err(),
            "filter change must NOT trigger an immediate emit \
             (cheap cfg-changed semantic)"
        );

        // Advance past one poll interval — now the periodic tick should fire.
        tokio::time::advance(Duration::from_secs(61)).await;
        let after_tick = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;
        assert!(
            matches!(after_tick, Ok(Some(Ok(_)))),
            "after one poll interval the stream must emit a fresh board; got {after_tick:?}"
        );
    }

    /// Regression guard against the test-harness bug itself: the helper
    /// `fixture_state_with_stream` must wire the receiver to the sender
    /// such that `save_config_inner` actually delivers to a live receiver.
    /// If this test fails, every other pipeline test in this module is
    /// running against a dead channel and silently passing.
    #[tokio::test]
    async fn fixture_state_with_stream_keeps_receiver_alive() {
        let cfg = crate::state::default_board_config();
        let (state, _stream) = fixture_state_with_stream(cfg.clone());

        // `cfg_tx.send` returns Err only when there are no receivers. The
        // helper must keep the receiver inside the Stream alive so this
        // succeeds; the prior `fixture_state()` would fail this assertion.
        let sent = state.cfg_tx.send(cfg);
        assert!(
            sent.is_ok(),
            "fixture_state_with_stream must hand cfg_tx a live receiver \
             so save_config_inner has somewhere to publish"
        );
    }

    // -----------------------------------------------------------------------
    // search_stations
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn search_stations_returns_results() {
        let state = fixture_state();
        let results = search_stations_inner("Belsize", &state)
            .await
            .expect("search should succeed");
        assert!(
            results.iter().any(|s| s.common_name.contains("Belsize")),
            "should find Belsize Park: {results:?}"
        );
    }

    #[tokio::test]
    async fn search_stations_rejects_null_byte() {
        let state = fixture_state();
        let err = search_stations_inner("Kings\0Cross", &state)
            .await
            .expect_err("null byte should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    #[tokio::test]
    async fn search_stations_rejects_overlong_query() {
        let state = fixture_state();
        let err = search_stations_inner(&"a".repeat(101), &state)
            .await
            .expect_err("overlong query should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    // -----------------------------------------------------------------------
    // get_line_status validation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_line_status_rejects_uppercase_line_id() {
        let state = fixture_state();
        let err = get_line_status_inner("Northern", &state)
            .await
            .expect_err("uppercase line_id should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    #[tokio::test]
    async fn get_line_status_rejects_empty_line_id() {
        let state = fixture_state();
        let err = get_line_status_inner("", &state)
            .await
            .expect_err("empty line_id should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    #[tokio::test]
    async fn get_line_status_rejects_path_traversal() {
        let state = fixture_state();
        let err = get_line_status_inner("../etc", &state)
            .await
            .expect_err("path traversal should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    // -----------------------------------------------------------------------
    // get_all_line_statuses
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_all_line_statuses_inner_returns_all_lines_worst_first() {
        // fixture_state() is backed by fixture files that include line-status
        // JSON for tube, DLR, elizabeth-line, and overground — the same files
        // exercised by the tfl-cache unit tests.
        let state = fixture_state();
        let statuses = get_all_line_statuses_inner(&state)
            .await
            .expect("get_all_line_statuses_inner should succeed with fixture data");

        // Must include entries from every mode (tube has 11 lines, DLR 1,
        // Elizabeth 1, Overground 6) — 19 total.  Assert non-empty and that
        // the vector spans more than one mode by checking for known ids.
        assert!(
            !statuses.is_empty(),
            "expected at least one LineStatus entry"
        );
        let ids: Vec<&str> = statuses.iter().map(|s| s.line_id.as_str()).collect();
        assert!(
            ids.contains(&"northern"),
            "tube 'northern' should appear; got: {ids:?}"
        );
        assert!(ids.contains(&"dlr"), "dlr should appear; got: {ids:?}");

        // Verify worst-first ordering: for each adjacent pair, the sort_rank
        // of the earlier entry must be <= that of the later entry (lower rank
        // = worse, so a[rank] <= b[rank] means a is at least as bad as b).
        let worst_rank = |s: &tfl_domain::LineStatus| {
            s.status
                .iter()
                .map(|e| e.bucket.sort_rank())
                .min()
                .unwrap_or(u8::MAX)
        };
        for pair in statuses.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            assert!(
                worst_rank(a) <= worst_rank(b),
                "ordering violated: {:?} (sort_rank {:?}) appears before {:?} (sort_rank {:?})",
                a.line_id,
                worst_rank(a),
                b.line_id,
                worst_rank(b),
            );
        }
    }

    // -----------------------------------------------------------------------
    // Display mode
    // -----------------------------------------------------------------------

    #[test]
    fn validate_display_mode_accepts_known_values() {
        assert!(validate_display_mode("window").is_ok());
        assert!(validate_display_mode("menubar").is_ok());
    }

    #[test]
    fn validate_display_mode_rejects_unknown() {
        assert!(validate_display_mode("").is_err());
        assert!(validate_display_mode("Window").is_err());
        assert!(validate_display_mode("popover").is_err());
    }

    #[tokio::test]
    async fn load_display_mode_defaults_to_window() {
        let state = fixture_state();
        let mode = load_display_mode_inner(&state).await.expect("should load");
        assert_eq!(mode, "window");
    }

    #[tokio::test]
    async fn save_and_load_display_mode_round_trips() {
        let state = fixture_state();
        let prev = save_display_mode_inner("menubar", &state)
            .await
            .expect("should save");
        assert_eq!(prev, "window", "should report previous mode for caller");
        let mode = load_display_mode_inner(&state).await.expect("should load");
        assert_eq!(mode, "menubar");
    }

    #[tokio::test]
    async fn save_display_mode_rejects_unknown_value() {
        let state = fixture_state();
        let err = save_display_mode_inner("invalid", &state)
            .await
            .expect_err("should reject unknown mode");
        assert!(err.contains("validation:"), "error: {err}");
    }

    /// `save_display_mode_inner` MUST mutate the live `AppState.display_mode`
    /// lock, not just persist to the store. The runtime focus-handler in
    /// `lib.rs` reads through this lock to decide click-away behaviour, so
    /// a stale value here is what regresses live-toggle into the previous
    /// "restart to apply" UX.
    #[tokio::test]
    async fn save_display_mode_updates_live_state_lock() {
        let state = fixture_state();
        // Sanity: starting in window mode.
        assert_eq!(
            state
                .display_mode
                .read()
                .expect("lock not poisoned")
                .as_str(),
            "window"
        );

        save_display_mode_inner("menubar", &state)
            .await
            .expect("save should succeed");

        assert_eq!(
            state
                .display_mode
                .read()
                .expect("lock not poisoned")
                .as_str(),
            "menubar",
            "live display_mode lock must reflect the saved mode"
        );
    }

    /// Validation runs before any mutation, so an invalid mode must NOT
    /// touch the live state lock or persist anything. Without this guard,
    /// a renderer-side type confusion could slip an unrecognised value past
    /// the IPC boundary and silently corrupt the runtime state.
    #[tokio::test]
    async fn save_display_mode_invalid_value_does_not_mutate_state() {
        let state = fixture_state();
        save_display_mode_inner("menubar", &state)
            .await
            .expect("baseline save should succeed");
        assert_eq!(
            state
                .display_mode
                .read()
                .expect("lock not poisoned")
                .as_str(),
            "menubar"
        );

        let _ = save_display_mode_inner("popover", &state)
            .await
            .expect_err("invalid mode should be rejected");

        assert_eq!(
            state
                .display_mode
                .read()
                .expect("lock not poisoned")
                .as_str(),
            "menubar",
            "rejected save must leave the live lock untouched"
        );
        let persisted = load_display_mode_inner(&state).await.unwrap();
        assert_eq!(
            persisted, "menubar",
            "rejected save must not have written to the store either"
        );
    }

    /// Calling `save_display_mode_inner` with the current mode must succeed
    /// and report the same value as the "previous" mode — this is how the
    /// Tauri command wrapper detects "no UI work needed" and skips the
    /// expensive tray/window swap. A regression here would either error
    /// out (bad UX on every Settings re-open) or trigger redundant UI
    /// thrash on every save.
    #[tokio::test]
    async fn save_display_mode_idempotent_when_mode_unchanged() {
        let state = fixture_state();
        save_display_mode_inner("menubar", &state).await.unwrap();

        let prev = save_display_mode_inner("menubar", &state)
            .await
            .expect("re-saving the same mode must succeed");
        assert_eq!(prev, "menubar", "previous == current when unchanged");

        assert_eq!(
            state
                .display_mode
                .read()
                .expect("lock not poisoned")
                .as_str(),
            "menubar",
            "idempotent save leaves the live lock at the same value"
        );
    }

    // -----------------------------------------------------------------------
    // Board size (renderer-driven window resize)
    // -----------------------------------------------------------------------

    /// Each tier from the renderer's preset table must pass validation —
    /// otherwise a perfectly normal "switch to a busy station" event would
    /// be rejected and the window would stay too small. Guards against
    /// over-tightening the bounds in [`validate_board_size`].
    #[test]
    fn validate_board_size_accepts_renderer_preset_table() {
        // (width, height) tuples must mirror the table in
        // Board.svelte::pickBoardSize. Both modes have a 4-tier ladder
        // (1 / 2 / 3 / 4+ lines).
        for (w, h) in [
            // menubar tiers
            (380.0, 520.0),
            (380.0, 620.0),
            (380.0, 720.0),
            (380.0, 800.0),
            // window tiers
            (700.0, 560.0),
            (980.0, 680.0),
            (1200.0, 760.0),
            (1200.0, 880.0),
        ] {
            validate_board_size(w, h)
                .unwrap_or_else(|e| panic!("preset {w}x{h} should validate: {e}"));
        }
    }

    /// Out-of-range values from a buggy renderer must be rejected before
    /// reaching the main-thread Cocoa dispatch — a degenerate `set_size`
    /// can leave the window unusable until the next launch.
    #[test]
    fn validate_board_size_rejects_out_of_range() {
        assert!(
            validate_board_size(100.0, 600.0).is_err(),
            "width too small"
        );
        assert!(
            validate_board_size(2000.0, 600.0).is_err(),
            "width too large"
        );
        assert!(
            validate_board_size(800.0, 100.0).is_err(),
            "height too small"
        );
        assert!(
            validate_board_size(800.0, 2000.0).is_err(),
            "height too large"
        );
    }

    /// NaN / infinity must never reach Cocoa — `NSWindow::setFrame:` traps
    /// on non-finite values and crashes the process. The frontend converts
    /// numbers via JSON which can express both, so we have to guard at the
    /// IPC boundary.
    #[test]
    fn validate_board_size_rejects_non_finite() {
        assert!(validate_board_size(f64::NAN, 600.0).is_err());
        assert!(validate_board_size(800.0, f64::NAN).is_err());
        assert!(validate_board_size(f64::INFINITY, 600.0).is_err());
        assert!(validate_board_size(800.0, f64::NEG_INFINITY).is_err());
    }

    /// Validation returns the validated (width, height) so the caller can
    /// pass it straight through to the main-thread dispatch — keeps the
    /// signature symmetric with `apply_board_size_effects`.
    #[test]
    fn validate_board_size_returns_inputs_on_success() {
        let (w, h) = validate_board_size(980.0, 720.0).unwrap();
        assert_eq!(w, 980.0);
        assert_eq!(h, 720.0);
    }

    // -----------------------------------------------------------------------
    // Favorites — add / list / remove
    // -----------------------------------------------------------------------

    fn make_fav(station_id: &str) -> (String, String, Vec<LineRef>) {
        (
            station_id.to_string(),
            format!("{station_id} Station"),
            vec![LineRef {
                id: "northern".to_string(),
                name: "Northern".to_string(),
            }],
        )
    }

    /// `add_favorite` followed by `load_favorites` must round-trip.
    /// Guards the new store key path end-to-end.
    #[tokio::test]
    async fn add_favorite_persists_and_list_returns_it() {
        let state = fixture_state();
        let (sid, name, lines) = make_fav("940GZZLUBZP");
        let result = add_favorite_inner(sid.clone(), name.clone(), lines.clone(), &state)
            .await
            .expect("add should succeed");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].station_id, sid);
        assert_eq!(result[0].common_name, name);

        let loaded = load_favorites_inner(&state)
            .await
            .expect("load should succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].station_id, sid);
    }

    /// Calling `add_favorite` twice with the same `station_id` must produce
    /// exactly one entry in the list. Guards double-click / double-mount.
    #[tokio::test]
    async fn add_favorite_is_idempotent_on_duplicate_station_id() {
        let state = fixture_state();
        let (sid, name, lines) = make_fav("940GZZLUBZP");
        add_favorite_inner(sid.clone(), name.clone(), lines.clone(), &state)
            .await
            .expect("first add should succeed");
        let result = add_favorite_inner(sid.clone(), name.clone(), lines.clone(), &state)
            .await
            .expect("second add should be a no-op");
        assert_eq!(result.len(), 1, "duplicate add must not grow the list");
    }

    /// `add_favorite` must reject an invalid `station_id` with a validation
    /// error — same gating as `save_config`. Guards path-traversal via IPC.
    #[tokio::test]
    async fn add_favorite_rejects_invalid_station_id() {
        let state = fixture_state();
        let err = add_favorite_inner(
            "../etc/passwd".to_string(),
            "Bad".to_string(),
            vec![],
            &state,
        )
        .await
        .expect_err("invalid station_id should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    /// `add_favorite` must reject a line id that is not in the supported
    /// whitelist — attacker-controlled JSON from the renderer can supply any
    /// string for `lines`.
    #[tokio::test]
    async fn add_favorite_rejects_unsupported_line_id() {
        let state = fixture_state();
        let lines = vec![LineRef {
            id: "gatwick-express".to_string(),
            name: "Gatwick Express".to_string(),
        }];
        let err = add_favorite_inner(
            "940GZZLUBZP".to_string(),
            "Belsize Park".to_string(),
            lines,
            &state,
        )
        .await
        .expect_err("unsupported line_id should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    /// `add_favorite` must reject a `common_name` longer than 200 characters
    /// (LOW-2: unbounded string → JSON bloat / UI render bug). The 200-char cap
    /// is generous — the longest legit TfL station name is ~52 chars.
    #[tokio::test]
    async fn add_favorite_rejects_overlong_common_name() {
        let state = fixture_state();
        let long_name = "A".repeat(201);
        let lines = vec![LineRef {
            id: "northern".to_string(),
            name: "Northern".to_string(),
        }];
        let err = add_favorite_inner("940GZZLUBZP".to_string(), long_name, lines, &state)
            .await
            .expect_err("201-char common_name should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    /// `add_favorite` must reject a `LineRef.name` longer than 200 characters.
    /// Same cap as `common_name` to bound stored JSON size.
    #[tokio::test]
    async fn add_favorite_rejects_overlong_line_name() {
        let state = fixture_state();
        let long_line_name = "B".repeat(201);
        let lines = vec![LineRef {
            id: "northern".to_string(),
            name: long_line_name,
        }];
        let err = add_favorite_inner(
            "940GZZLUBZP".to_string(),
            "Belsize Park".to_string(),
            lines,
            &state,
        )
        .await
        .expect_err("201-char LineRef.name should be rejected");
        assert!(err.contains("validation:"), "error: {err}");
    }

    #[test]
    fn common_name_with_null_byte_is_rejected() {
        let result = validate_common_name("Belsize\0Park");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    /// Allowlist: real TfL station names use letters (incl. accented),
    /// digits, spaces, `-`, `'`, `(`, `)`, `&`, `.`, `/`. Any other character
    /// (control chars, `<`, `>`, `;`, emoji, etc.) must be rejected so the
    /// favorites JSON cannot smuggle arbitrary content into the disk store
    /// or the renderer.
    #[test]
    fn common_name_rejects_disallowed_chars() {
        for bad in [
            "Belsize<script>",
            "Belsize;Park",
            "Belsize\tPark",
            "Belsize=Park",
            "Belsize[Park]",
            "Belsize\"Park\"",
            "Belsize\u{200B}Park",
        ] {
            let result = validate_common_name(bad);
            assert!(
                result.is_err(),
                "expected {bad:?} to be rejected by allowlist"
            );
            assert!(
                result.unwrap_err().contains("validation:"),
                "error must use the validation: prefix for {bad:?}"
            );
        }
    }

    /// The allowlist MUST accept every character class that appears in
    /// `fixtures/stop-points/*.json`. Audit of real fixture data found:
    /// `&`, `'`, `(`, `)`, `-`, `.`, `/`. Plus spaces, ASCII letters,
    /// digits, and Unicode letters (none in current fixtures, but the
    /// London naming standard does not preclude them). If this test ever
    /// fails, the allowlist has dropped a real station name on the floor.
    #[test]
    fn common_name_accepts_real_station_punctuation() {
        for ok in [
            "Belsize Park",
            "King's Cross St. Pancras",
            "Hammersmith (H&C Line)",
            "Edgware Road (Bakerloo)",
            "Heathrow Terminals 2 & 3",
            "Harrow-on-the-Hill",
            "Shepherd's Bush",
            "Totteridge & Whetstone",
            "Liberté",      // Unicode letter — defensive, future-proof.
            "A/B Junction", // forward slash appears in raw fixture names.
            "St. James's Park",
        ] {
            assert!(
                validate_common_name(ok).is_ok(),
                "real-shape name {ok:?} must pass"
            );
        }
    }

    #[test]
    fn line_name_with_null_byte_is_rejected() {
        let result = validate_line_name("North\0ern");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("validation:"));
    }

    /// `remove_favorite` reduces the list by one.
    #[tokio::test]
    async fn remove_favorite_removes_entry() {
        let state = fixture_state();
        let (sid, name, lines) = make_fav("940GZZLUBZP");
        add_favorite_inner(sid.clone(), name, lines, &state)
            .await
            .expect("add should succeed");
        let result = remove_favorite_inner(sid.clone(), &state)
            .await
            .expect("remove should succeed");
        assert!(
            result.is_empty(),
            "list must be empty after removing sole entry"
        );

        let loaded = load_favorites_inner(&state)
            .await
            .expect("load should succeed");
        assert!(loaded.is_empty(), "stored list must be empty too");
    }

    /// `remove_favorite` on a station not in the list is a no-op (not an error).
    #[tokio::test]
    async fn remove_favorite_is_noop_when_absent() {
        let state = fixture_state();
        let result = remove_favorite_inner("940GZZLUBZP".to_string(), &state)
            .await
            .expect("remove of absent station must succeed");
        assert!(result.is_empty());
    }

    /// `add_favorite` and `remove_favorite` MUST NOT publish to `cfg_tx`.
    /// Any emission here would regress the chip-toggle burst protection (no
    /// unwanted fetch per invariant #3). Uses `fixture_state_with_stream` +
    /// 50 ms timeout to prove no emit fires — the same pattern as
    /// `save_config_filter_change_does_not_force_immediate_refresh`.
    #[tokio::test(start_paused = true)]
    async fn add_favorite_does_not_publish_to_cfg_tx() {
        use futures::StreamExt;
        use std::time::Duration;

        let initial_cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 60,
            theme: "classic-amber".to_string(),
        };
        let (state, mut stream) = fixture_state_with_stream(initial_cfg.clone());
        // Let the stream emit its initial board so we start from a known state.
        let _ = stream.next().await.unwrap().unwrap();

        // add_favorite MUST NOT cause an immediate emit.
        let (sid, name, lines) = make_fav("940GZZLUKSX");
        add_favorite_inner(sid, name, lines, &state)
            .await
            .expect("add should succeed");

        // No time advance — if cfg_tx were published, the stream would emit.
        let immediate = tokio::time::timeout(Duration::from_millis(50), stream.next()).await;
        assert!(
            immediate.is_err(),
            "add_favorite must NOT publish to cfg_tx (no emit expected)"
        );
    }

    /// Same as `add_favorite_does_not_publish_to_cfg_tx` but for the remove path.
    #[tokio::test(start_paused = true)]
    async fn remove_favorite_does_not_publish_to_cfg_tx() {
        use futures::StreamExt;
        use std::time::Duration;

        let initial_cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 60,
            theme: "classic-amber".to_string(),
        };
        let (state, mut stream) = fixture_state_with_stream(initial_cfg.clone());
        // Consume initial emit.
        let _ = stream.next().await.unwrap().unwrap();

        // Pre-seed so remove has something to remove.
        let (sid, name, lines) = make_fav("940GZZLUKSX");
        add_favorite_inner(sid.clone(), name, lines, &state)
            .await
            .unwrap();

        remove_favorite_inner(sid, &state)
            .await
            .expect("remove should succeed");

        let immediate = tokio::time::timeout(Duration::from_millis(50), stream.next()).await;
        assert!(
            immediate.is_err(),
            "remove_favorite must NOT publish to cfg_tx (no emit expected)"
        );
    }

    /// Selecting a favorite via `save_config` MUST trigger an immediate
    /// stream refresh — same as invariant #2. This test exists to prove the
    /// "select favorite → board updates" path works end-to-end.
    #[tokio::test(start_paused = true)]
    async fn selecting_favorite_via_save_config_triggers_immediate_refresh() {
        use futures::StreamExt;
        use std::time::Duration;

        let initial_cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 60,
            theme: "classic-amber".to_string(),
        };
        let (state, mut stream) = fixture_state_with_stream(initial_cfg.clone());

        // First emit: BZP.
        let _ = stream.next().await.unwrap().unwrap();

        // Simulate "user clicks KSX in favorites" → frontend calls save_config.
        let new_cfg = BoardConfig {
            station_id: "940GZZLUKSX".to_string(),
            ..initial_cfg
        };
        save_config_inner(&new_cfg, &state)
            .await
            .expect("save_config must succeed");

        let before = tokio::time::Instant::now();
        let board = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("stream must emit after selecting favorite via save_config")
            .unwrap()
            .unwrap();
        let elapsed = before.elapsed();

        assert_eq!(
            board.station_id, "940GZZLUKSX",
            "selecting a favorite via save_config must update the board"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "selection must trigger an immediate refresh; took {elapsed:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Favorites — legacy migration
    // -----------------------------------------------------------------------

    /// `load_favorites_inner` must rewrite `"london-overground"` in each
    /// favorite's `lines` field to the six named successor ids (invariant #14).
    #[tokio::test]
    async fn migrate_favorites_legacy_overground_ids() {
        let state = fixture_state();

        // Save a favorite with the legacy overground id directly to bypass
        // `add_favorite_inner`'s whitelist (which already rejects the legacy id).
        let fav_with_legacy = vec![Favorite {
            station_id: "940GZZLUBZP".to_string(),
            common_name: "Belsize Park".to_string(),
            lines: vec![
                LineRef {
                    id: "northern".to_string(),
                    name: "Northern".to_string(),
                },
                LineRef {
                    id: "london-overground".to_string(),
                    name: "Overground".to_string(),
                },
            ],
        }];
        state
            .favorites_store
            .save_favorites(&fav_with_legacy)
            .await
            .expect("save legacy favorite");

        // load_favorites_inner must migrate on load.
        let loaded = load_favorites_inner(&state).await.expect("load");
        assert_eq!(loaded.len(), 1);

        let line_ids: Vec<&str> = loaded[0].lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            !line_ids.contains(&"london-overground"),
            "legacy id must be stripped; got {line_ids:?}"
        );
        for &named in NAMED_OVERGROUND_LINES {
            assert!(
                line_ids.contains(&named),
                "expanded named id {named} must appear; got {line_ids:?}"
            );
        }
    }

    /// Running `load_favorites_inner` on an already-migrated config is a no-op.
    #[tokio::test]
    async fn load_favorites_idempotent_on_already_migrated() {
        let state = fixture_state();

        // Persist a favorite whose lines already use named ids.
        let fav_modern = vec![Favorite {
            station_id: "940GZZLUBZP".to_string(),
            common_name: "Belsize Park".to_string(),
            lines: vec![
                LineRef {
                    id: "northern".to_string(),
                    name: "Northern".to_string(),
                },
                LineRef {
                    id: "mildmay".to_string(),
                    name: "Mildmay".to_string(),
                },
            ],
        }];
        state
            .favorites_store
            .save_favorites(&fav_modern)
            .await
            .expect("save");

        let first = load_favorites_inner(&state).await.expect("first load");
        let second = load_favorites_inner(&state).await.expect("second load");

        assert_eq!(
            first, second,
            "re-loading a migrated config must be a no-op"
        );
    }

    // -----------------------------------------------------------------------
    // Display prefs — desktop-only render flags (Phase 3 of arrival-feedback plan)
    // -----------------------------------------------------------------------

    /// Round-trip through the new `"display_prefs"` store key.
    #[tokio::test]
    async fn save_display_prefs_persists_and_get_returns_it() {
        let state = fixture_state();

        save_display_prefs_inner(
            &DisplayPrefs {
                group_destinations: true,
            },
            &state,
        )
        .await
        .expect("save should succeed");

        let loaded = load_display_prefs_inner(&state).await.expect("load");
        assert!(
            loaded.group_destinations,
            "saved prefs must round-trip through the store"
        );
    }

    /// Upgrade path: a build that didn't write the key returns the default
    /// (group_destinations=false). The user MUST NOT silently inherit a
    /// grouped board on first launch after upgrade.
    #[tokio::test]
    async fn load_display_prefs_returns_default_when_missing() {
        let state = fixture_state();

        let loaded = load_display_prefs_inner(&state).await.expect("load");
        assert_eq!(
            loaded,
            DisplayPrefs::default(),
            "missing display_prefs key must default to DisplayPrefs::default()"
        );
        assert!(
            !loaded.group_destinations,
            "default must be group_destinations=false (opt-in only)"
        );
    }

    /// `save_display_prefs` MUST NOT publish to `cfg_tx` — it is a renderer-
    /// only flag, not part of the cfg pipeline. Mirrors the favorites
    /// invariant (`add_favorite_does_not_publish_to_cfg_tx`).
    #[tokio::test(start_paused = true)]
    async fn save_display_prefs_does_not_publish_to_cfg_tx() {
        use futures::StreamExt;
        use std::time::Duration;

        let initial_cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 60,
            theme: "classic-amber".to_string(),
        };
        let (state, mut stream) = fixture_state_with_stream(initial_cfg);
        // Consume the initial board emit so the stream is in steady state.
        let _ = stream.next().await.unwrap().unwrap();

        save_display_prefs_inner(
            &DisplayPrefs {
                group_destinations: true,
            },
            &state,
        )
        .await
        .expect("save should succeed");

        let immediate = tokio::time::timeout(Duration::from_millis(50), stream.next()).await;
        assert!(
            immediate.is_err(),
            "save_display_prefs must NOT publish to cfg_tx (no emit expected)"
        );
    }

    // -----------------------------------------------------------------------
    // Capability snapshot tests (MEDIUM-2 / M7 TODO)
    //
    // In Tauri v2, custom `#[tauri::command]` handlers (like `load_app_key`,
    // `save_app_key`, `has_app_key`) are not controlled by the capabilities
    // JSON `permissions` field — those fields only govern built-in plugin
    // commands (`core:*`, `store:*`). The per-window enforcement for custom
    // commands is implemented **in the command handler itself** via a window
    // label check.
    //
    // These tests guard the two-layer security contract:
    //
    // Layer 1 — Capability file structure (these tests):
    //   - `capabilities/default.json`  covers `["main"]` only, not settings.
    //   - `capabilities/settings.json` covers `["settings"]` only, not main.
    //     It records in a human-readable `"description"` field which privileged
    //     commands are restricted to this window (documentation anchor).
    //
    // Layer 2 — Runtime enforcement (see `load_app_key_inner_rejects_non_settings_window`):
    //   - `load_app_key` rejects any caller whose window label ≠ "settings".
    //   - `has_app_key` is intentionally NOT restricted (boolean only, safe).
    //   - `save_app_key` is intentionally NOT restricted (write, no data leak).
    //
    // If someone widens `default.json` to also cover "settings", the first
    // test catches it. If `load_app_key` loses its window-label guard, the
    // runtime test catches it.
    // -----------------------------------------------------------------------

    /// Load a capability JSON file from the `src-tauri/capabilities/` directory.
    /// `CARGO_MANIFEST_DIR` is set by Cargo to the directory containing `Cargo.toml`,
    /// which is `src-tauri/`.
    fn load_capability(filename: &str) -> serde_json::Value {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR not set — must run via `cargo test`");
        let path = std::path::Path::new(&manifest_dir)
            .join("capabilities")
            .join(filename);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
    }

    fn windows_list(cap: &serde_json::Value) -> Vec<String> {
        cap["windows"]
            .as_array()
            .expect("capabilities JSON must have a 'windows' array")
            .iter()
            .map(|v| {
                v.as_str()
                    .expect("window entry must be a string")
                    .to_string()
            })
            .collect()
    }

    // Layer 1 — capability file structure tests.

    #[test]
    fn capability_default_covers_main_only() {
        let cap = load_capability("default.json");
        let wins = windows_list(&cap);
        assert!(
            wins.iter().any(|w| w == "main"),
            "default.json must apply to the 'main' window; got: {wins:?}"
        );
        assert!(
            !wins.iter().any(|w| w == "settings"),
            "SECURITY: default.json must NOT cover the 'settings' window. \
             Tauri plugin permissions granted to 'main' must not be silently \
             inherited by the settings window. Got: {wins:?}"
        );
    }

    #[test]
    fn capability_settings_file_exists_and_covers_settings_only() {
        let cap = load_capability("settings.json");
        let wins = windows_list(&cap);
        assert!(
            wins.iter().any(|w| w == "settings"),
            "settings.json must apply to the 'settings' window; got: {wins:?}"
        );
        assert!(
            !wins.iter().any(|w| w == "main"),
            "settings.json must NOT apply to 'main' window (would over-privilege main); \
             got: {wins:?}"
        );
    }

    #[test]
    fn capability_settings_describes_load_app_key_restriction() {
        // The settings.json `description` field is the human-readable anchor
        // documenting that `load_app_key` is restricted to this window.
        // This test ensures the documentation anchor isn't accidentally removed.
        let cap = load_capability("settings.json");
        let desc = cap["description"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        assert!(
            desc.contains("load_app_key"),
            "settings.json description must mention load_app_key so reviewers \
             know why this file exists. Got description: {:?}",
            cap["description"].as_str().unwrap_or("(missing)")
        );
    }

    // Layer 2 — runtime enforcement test.
    //
    // The `load_app_key` inner function is a pure async fn that takes &AppState.
    // The window-label guard lives in the `load_app_key` command wrapper, which
    // receives a `tauri::WebviewWindow` parameter. We can't construct a real
    // WebviewWindow in a unit test, so instead we assert the invariant at the
    // code-structure level: the command wrapper must call a function that checks
    // the label and returns Err for non-settings callers.
    //
    // The function `load_app_key_rejects_non_settings` is pub(crate) for testing.

    #[tokio::test]
    async fn load_app_key_inner_reachable_from_settings_window() {
        // `load_app_key_inner` itself should still work fine given valid state.
        // This confirms we haven't broken the happy path.
        let state = fixture_state();
        save_app_key_inner(Some("test-key".to_string()), &state)
            .await
            .expect("save should succeed");
        let result = load_app_key_inner(&state).await;
        assert!(
            result.is_ok(),
            "load_app_key_inner must succeed: {result:?}"
        );
        assert_eq!(result.unwrap(), Some("test-key".to_string()));
    }

    #[test]
    fn load_app_key_window_guard_rejects_non_settings() {
        // The public command `load_app_key` has a window guard that checks
        // the label before delegating to `load_app_key_inner`. We verify the
        // guard function directly here.
        assert!(
            window_label_is_settings("settings"),
            "settings window must be permitted"
        );
        assert!(
            !window_label_is_settings("main"),
            "main window must be rejected by the guard"
        );
        assert!(
            !window_label_is_settings(""),
            "empty label must be rejected"
        );
        assert!(
            !window_label_is_settings("SETTINGS"),
            "label check must be case-sensitive"
        );
    }

    // -----------------------------------------------------------------------
    // TODO(1B-followup): End-to-end webview IPC integration test for
    // `load_app_key`.
    //
    // CONSTRAINT: `tauri::WebviewWindow` does not implement
    // `CommandArg<'_, MockRuntime>` in Tauri 2.x. `generate_handler!` relies
    // on every parameter implementing `CommandArg` for the chosen runtime; the
    // real `WebviewWindow` works only with the full Wry/Webkit runtime, not
    // with MockRuntime. Consequently, registering `load_app_key` in a
    // `mock_builder().invoke_handler(...)` fails to compile with:
    //
    //   error[E0277]: the trait bound `tauri::WebviewWindow:
    //   CommandArg<'_, MockRuntime>` is not satisfied
    //
    // The intent of the end-to-end test was to assert via IPC dispatch that
    // `load_app_key` rejects from "main" and accepts from "settings" as
    // wired-up command — not just at the `window_label_is_settings` guard
    // level. Two alternatives when Tauri lifts this restriction:
    //
    //   (A) Switch to a custom `CommandArg` impl for test windows once Tauri
    //       exposes one, re-enable the test with `get_ipc_response`.
    //   (B) Use an integration test binary against a real Tauri process
    //       (e.g. via `tauri-driver` / WebDriver) that spawns the actual app.
    //
    // What the existing tests DO cover (maintained in place):
    //   • `window_label_is_settings` returns true for "settings", false for
    //     "main" / "" / "SETTINGS" — covers the guard logic directly.
    //   • `load_app_key_inner_reachable_from_settings_window` — confirms the
    //     inner fn succeeds with valid state (happy path).
    //   • `capability_default_covers_main_only` / `capability_settings_file_exists_and_covers_settings_only`
    //     — confirm the capability JSON files don't accidentally cross-pollinate.
    //   • `capability_settings_describes_load_app_key_restriction` — keeps the
    //     documentation anchor in `capabilities/settings.json`.
    //
    // Together these form a multi-layer defence that a future maintainer can
    // upgrade to a true IPC-dispatch test when the runtime constraint lifts.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Updater preferences round-trip (M8 PR-D)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn load_update_prefs_default_is_auto_check_true() {
        // Pin the opt-OUT default. A live-data app shipping with stale
        // binaries (because auto-check defaulted off) would leave users on
        // unfixed WKWebView CVEs. Flipping this default is a security
        // regression — must trip the test.
        let state = fixture_state();
        let loaded = state
            .config_store
            .load_update_prefs()
            .await
            .expect("load default prefs");
        assert!(
            loaded.auto_check,
            "auto_check must default to true; got {loaded:?}"
        );
    }

    #[tokio::test]
    async fn save_update_prefs_round_trips() {
        let state = fixture_state();
        let prefs = UpdatePrefs { auto_check: false };
        state
            .config_store
            .save_update_prefs(&prefs)
            .await
            .expect("save");
        let loaded = state.config_store.load_update_prefs().await.expect("load");
        assert_eq!(loaded, prefs);
    }

    #[tokio::test]
    async fn update_prefs_storage_does_not_affect_other_keys() {
        // Saving update_prefs MUST NOT clobber adjacent store keys
        // (board_config, display_mode, display_prefs). All four share the
        // same underlying tauri-plugin-store file; a stray serialization
        // mistake here could corrupt the saved station id and surface
        // as "my home station reset itself overnight".
        //
        // BoardConfig doesn't implement PartialEq (it lives in
        // `crates/tfl-board`, which is the iOS-pinned public contract —
        // adding PartialEq would force a tfl-* edit this PR is forbidden
        // from making). Inspect station_id + line_ids field-by-field
        // instead; those are the surfaces a save-key collision would
        // realistically corrupt.
        let state = fixture_state();
        let original_cfg = state
            .config_store
            .load_config()
            .await
            .expect("load original cfg");

        state
            .config_store
            .save_update_prefs(&UpdatePrefs { auto_check: false })
            .await
            .expect("save update prefs");

        let cfg_after = state
            .config_store
            .load_config()
            .await
            .expect("load cfg after");
        assert_eq!(cfg_after.station_id, original_cfg.station_id);
        assert_eq!(cfg_after.line_ids, original_cfg.line_ids);
        assert_eq!(cfg_after.directions, original_cfg.directions);
        assert_eq!(cfg_after.poll_seconds, original_cfg.poll_seconds);
        assert_eq!(cfg_after.theme, original_cfg.theme);
    }
}
