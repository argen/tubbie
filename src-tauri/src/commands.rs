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
//! - `poll_seconds`: clamped to [5, 300] (not rejected, UI can display effective value).
//! - `app_key`: max 64 chars, no null bytes (when `Some`).
//!
//! ## Async safety
//!
//! All handlers are `async fn` with `#[tauri::command]`. No background tasks
//! are spawned here — polling streams are wired in M6 via event emission.
//! Dropping the Tauri window cancels any in-flight command via the executor.
//!
//! ## M6 TODO
//!
//! Wire `BoardService::stream` with `tauri::async_runtime::spawn` + a
//! cancellation token bound to `WindowEvent::Destroyed`. The stream should
//! emit `Board` snapshots as `app.emit("board-update", board)` events.

use serde_json::{json, Value};
use tauri::State;

use tfl_board::BoardConfig;
use tfl_domain::{Board, LineStatus, Station};

use crate::state::AppState;

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

/// Clamp `poll_seconds` to the allowed range [5, 300].
pub(crate) fn clamp_poll_seconds(v: u32) -> u32 {
    v.clamp(5, 300)
}

/// Validate an optional `app_key`.
/// When `Some`: max 64 chars, no null bytes.
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

/// Validate a `BoardConfig`'s fields.
pub(crate) fn validate_board_config(cfg: &BoardConfig) -> Result<(), String> {
    validate_station_id(&cfg.station_id)?;
    for line_id in &cfg.line_ids {
        validate_line_id(line_id)?;
    }
    // directions: enum values validated by serde deserialization
    // poll_seconds: clamped, not rejected
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
    let stations = crate::state::AnyBoardService::search_stations(&*state.board_service, query)
        .await
        .map_err(|e| e.to_string())?;
    Ok(stations)
}

pub(crate) async fn get_board_inner(state: &AppState) -> Result<Board, String> {
    let cfg = state.load_board_config();
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
    let value = serde_json::to_value(&cfg).map_err(|e| format!("serialise error: {e}"))?;
    state.config_store.set("board_config", value);
    state.config_store.save()?;
    Ok(())
}

pub(crate) async fn load_config_inner(state: &AppState) -> Result<BoardConfig, String> {
    Ok(state.load_board_config())
}

pub(crate) async fn save_app_key_inner(
    key: Option<String>,
    state: &AppState,
) -> Result<String, String> {
    validate_app_key(&key)?;
    let value = match &key {
        Some(k) => json!(k),
        None => Value::Null,
    };
    state.config_store.set("tfl_app_key", value);
    state.config_store.save()?;
    Ok("restart to apply".to_string())
}

pub(crate) async fn load_app_key_inner(state: &AppState) -> Result<Option<String>, String> {
    let key = state.config_store.get("tfl_app_key").and_then(|v| {
        if v.is_null() {
            None
        } else {
            serde_json::from_value::<String>(v).ok()
        }
    });
    Ok(key)
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

/// Fetch the arrivals board for the currently saved station config.
///
/// Uses the `BoardConfig` from the config store (or the default: Belsize Park).
///
/// M6 TODO: wire a polling stream via `BoardService::stream` + Tauri event
/// emission so the frontend receives updates without polling this command.
#[tauri::command]
pub async fn get_board(state: State<'_, AppState>) -> Result<Board, String> {
    get_board_inner(&state).await
}

/// Persist a `BoardConfig` to the store.
///
/// Validates all fields and clamps `poll_seconds` to [5, 300] before saving.
#[tauri::command]
pub async fn save_config(cfg: BoardConfig, state: State<'_, AppState>) -> Result<(), String> {
    save_config_inner(&cfg, &state).await
}

/// Load the currently saved `BoardConfig`, or return the default.
///
/// Default: Belsize Park (`940GZZLUBZP`), no line/direction filter, 20 s poll.
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

// SECURITY: only the Settings UI should call this command. The main board
// view must never invoke `load_app_key` — exposing the key to the board
// page would make it accessible to any renderer-side script.
/// Load the stored TfL API key.
///
/// Returns `None` if no key has been saved.
#[tauri::command]
pub async fn load_app_key(state: State<'_, AppState>) -> Result<Option<String>, String> {
    load_app_key_inner(&state).await
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::MemoryConfigStore;
    use std::sync::Arc;
    use tfl_board::BoardService;
    use tfl_client::{clock::FakeClock, fixture::FixtureTflHttp, TflClient};

    /// Path to the workspace fixtures directory (relative to this crate's manifest).
    fn fixture_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
    }

    /// Build an `AppState` backed by fixture files + in-memory config.
    fn fixture_state() -> AppState {
        let clock = FakeClock::from_rfc3339("2025-01-15T10:00:00Z").unwrap();
        let http = FixtureTflHttp::new(fixture_dir());
        let client = TflClient::new(http);
        let board_service =
            Arc::new(BoardService::new(client, clock)) as Arc<dyn crate::state::AnyBoardService>;
        let config_store = Arc::new(MemoryConfigStore::new()) as Arc<dyn crate::state::ConfigStore>;
        AppState {
            board_service,
            config_store,
        }
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
        assert_eq!(clamp_poll_seconds(0), 5);
        assert_eq!(clamp_poll_seconds(4), 5);
    }

    #[test]
    fn poll_seconds_above_max_is_clamped() {
        assert_eq!(clamp_poll_seconds(301), 300);
        assert_eq!(clamp_poll_seconds(9999), 300);
    }

    #[test]
    fn poll_seconds_in_range_passes_through() {
        assert_eq!(clamp_poll_seconds(20), 20);
        assert_eq!(clamp_poll_seconds(5), 5);
        assert_eq!(clamp_poll_seconds(300), 300);
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
        assert_eq!(cfg.station_id, "940GZZLUBZP");
        assert_eq!(cfg.poll_seconds, 20);
    }

    #[tokio::test]
    async fn save_config_clamps_poll_seconds() {
        let state = fixture_state();
        let cfg = BoardConfig {
            station_id: "940GZZLUBZP".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 9999,
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
        };
        let err = save_config_inner(&cfg, &state)
            .await
            .expect_err("should reject uppercase line_id");
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
    // get_board
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_board_returns_board_for_default_config() {
        let state = fixture_state();
        let board = get_board_inner(&state).await.expect("should return board");
        assert_eq!(board.station_id, "940GZZLUBZP");
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
        };
        save_config_inner(&cfg, &state).await.unwrap();
        let board = get_board_inner(&state).await.unwrap();
        assert_eq!(board.station_id, "940GZZLUKSX");
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
}
