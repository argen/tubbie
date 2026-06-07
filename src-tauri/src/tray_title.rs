//! Menubar tray-title projection (Phase 2 — menubar live-arrival).
//!
//! Pure projection from a [`Board`] to the short next-arrival string shown
//! beside the menu-bar tray icon. No I/O, no Cocoa, no clock. The Cocoa
//! `set_title` dispatch and the bucket-boundary throttle live in `lib.rs`
//! (`set_tray_title` + the stream loop).
//!
//! Kept here — in the Mac shell, not the shared core — on purpose: it is a
//! candidate resident of the deferred `tfl-presenter` crate (ADR #10),
//! promoted there only if iOS grows an equivalent projection and the two
//! start to drift. Until then a Mac-only `pub(crate)` fn adds zero public
//! surface to `tfl-domain` / `tfl-board`.

use chrono::Duration;
use tfl_domain::format::format_time_to_station;
use tfl_domain::types::Board;

/// The menu-bar title for a board: the soonest upcoming arrival among the
/// user's selected lines, formatted ETA-first in the platform dot-matrix style
/// (`"Due"` / `"1 min"` / `"N mins"`). Returns `None` when nothing qualifies —
/// the caller clears the title so the menu bar shows the bare icon.
///
/// `line_ids` is the user's chip selection (`BoardConfig.line_ids`). It is the
/// SAME display mask the board UI applies (`Board.svelte`'s `linesGrouped`):
/// empty = all lines; otherwise only arrivals whose `line_id` is in the set
/// (exact match, mirroring the frontend — invariant #22). This keeps the menu
/// bar consistent with what the open board shows: a line you have filtered out
/// of the board must not win the menu-bar title. Direction filtering needs no
/// handling here — the board the stream hands us is already direction-filtered
/// by `apply_filters`.
///
/// "Soonest" = smallest `time_to_station`. We deliberately reuse
/// [`format_time_to_station`] (the single canonical ETA formatter) rather than
/// inventing a menu-bar-specific format: the arrival-display rule is that every
/// surface shows "Due / 1 min / 2 mins", never a wall-clock time.
pub(crate) fn next_arrival_title(board: &Board, line_ids: &[String]) -> Option<String> {
    let filter_active = !line_ids.is_empty();
    let soonest = board
        .platforms
        .iter()
        .flat_map(|p| p.arrivals.iter())
        .filter(|a| !filter_active || line_ids.iter().any(|id| id == &a.line_id))
        .min_by_key(|a| a.time_to_station)?;
    Some(format_time_to_station(Duration::seconds(
        soonest.time_to_station,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an arrival wire-JSON object with the given `time_to_station` and
    /// line. Only the fields the projection touches are meaningful; the rest
    /// are the minimum the `Arrival` deserializer requires.
    fn arrival_json(i: usize, tts: i64) -> serde_json::Value {
        arrival_json_line(i, tts, "victoria")
    }

    fn arrival_json_line(i: usize, tts: i64, line_id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": format!("a{i}"),
            "stationName": "Test Station",
            "platformName": "Northbound - Platform 1",
            "lineId": line_id,
            "lineName": line_id,
            "timeToStation": tts,
            "expectedArrival": "2026-01-15T10:05:00Z",
        })
    }

    const ALL_LINES: &[String] = &[];

    fn board_json(platforms: serde_json::Value) -> Board {
        serde_json::from_value(serde_json::json!({
            "station_id": "940GZZLUVIC",
            "generated_at": "2026-01-15T10:00:00Z",
            "stale_since": null,
            "platforms": platforms,
        }))
        .expect("valid board json")
    }

    /// Single-platform board from a list of `time_to_station` values.
    fn board_with(tts: &[i64]) -> Board {
        let arrivals: Vec<serde_json::Value> = tts
            .iter()
            .enumerate()
            .map(|(i, t)| arrival_json(i, *t))
            .collect();
        board_json(serde_json::json!([{ "name": "Northbound", "arrivals": arrivals }]))
    }

    #[test]
    fn empty_board_has_no_title() {
        let board = board_json(serde_json::json!([]));
        assert!(next_arrival_title(&board, ALL_LINES).is_none());
    }

    #[test]
    fn platform_with_no_arrivals_has_no_title() {
        let board = board_json(serde_json::json!([{ "name": "Northbound", "arrivals": [] }]));
        assert!(next_arrival_title(&board, ALL_LINES).is_none());
    }

    #[test]
    fn picks_soonest_arrival_and_formats_it() {
        // 200s -> "3 mins", 40s -> "1 min"; soonest is 40s.
        let board = board_with(&[200, 40]);
        assert_eq!(
            next_arrival_title(&board, ALL_LINES).as_deref(),
            Some("1 min")
        );
    }

    #[test]
    fn imminent_arrival_is_due() {
        let board = board_with(&[10]);
        assert_eq!(
            next_arrival_title(&board, ALL_LINES).as_deref(),
            Some("Due")
        );
    }

    #[test]
    fn soonest_is_taken_across_platforms_not_just_the_first() {
        // First platform's soonest is 300s ("5 mins"); a later platform has a
        // 30s train ("1 min"). The projection must scan ALL platforms.
        let board = board_json(serde_json::json!([
            { "name": "Northbound", "arrivals": [arrival_json(0, 300)] },
            { "name": "Southbound", "arrivals": [arrival_json(1, 30)] },
        ]));
        assert_eq!(
            next_arrival_title(&board, ALL_LINES).as_deref(),
            Some("1 min")
        );
    }

    // -----------------------------------------------------------------------
    // Line-selection mask (consistency with the board's lineIds chip filter)
    // -----------------------------------------------------------------------

    /// A multi-line board: Bakerloo arrives sooner (30s) than Victoria (180s).
    fn mixed_line_board() -> Board {
        board_json(serde_json::json!([
            { "name": "Northbound", "arrivals": [
                arrival_json_line(0, 30, "bakerloo"),
                arrival_json_line(1, 180, "victoria"),
            ]},
        ]))
    }

    #[test]
    fn selection_tracks_soonest_within_the_selected_lines_only() {
        // User selected Victoria only. The Bakerloo train is sooner but
        // filtered OUT of their board, so it must NOT win the menu-bar title.
        let board = mixed_line_board();
        let sel = vec!["victoria".to_string()];
        // Victoria's soonest is 180s -> "3 mins" (NOT Bakerloo's "Due").
        assert_eq!(next_arrival_title(&board, &sel).as_deref(), Some("3 mins"));
    }

    #[test]
    fn empty_selection_means_all_lines() {
        // No chip filter: the soonest across all lines wins (Bakerloo, 30s).
        let board = mixed_line_board();
        assert_eq!(
            next_arrival_title(&board, ALL_LINES).as_deref(),
            Some("1 min")
        );
    }

    #[test]
    fn selection_with_no_matching_arrivals_clears_the_title() {
        // User selected a line not present in the board → nothing qualifies,
        // so the title clears rather than falling back to an unrelated line.
        let board = mixed_line_board();
        let sel = vec!["northern".to_string()];
        assert!(next_arrival_title(&board, &sel).is_none());
    }
}
