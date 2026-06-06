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

/// The menu-bar title for a board: the soonest upcoming arrival across all
/// platforms, formatted ETA-first in the platform dot-matrix style
/// (`"Due"` / `"1 min"` / `"N mins"`). Returns `None` when the board has no
/// arrivals — the caller clears the title so the menu bar shows the bare icon.
///
/// "Soonest" = smallest `time_to_station`. We deliberately reuse
/// [`format_time_to_station`] (the single canonical ETA formatter) rather than
/// inventing a menu-bar-specific format: the arrival-display rule is that every
/// surface shows "Due / 1 min / 2 mins", never a wall-clock time.
pub(crate) fn next_arrival_title(board: &Board) -> Option<String> {
    let soonest = board
        .platforms
        .iter()
        .flat_map(|p| p.arrivals.iter())
        .min_by_key(|a| a.time_to_station)?;
    Some(format_time_to_station(Duration::seconds(soonest.time_to_station)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an arrival wire-JSON object with the given `time_to_station`.
    /// Only the fields the projection touches are meaningful; the rest are
    /// the minimum the `Arrival` deserializer requires.
    fn arrival_json(i: usize, tts: i64) -> serde_json::Value {
        serde_json::json!({
            "id": format!("a{i}"),
            "stationName": "Test Station",
            "platformName": "Northbound - Platform 1",
            "lineId": "victoria",
            "lineName": "Victoria",
            "timeToStation": tts,
            "expectedArrival": "2026-01-15T10:05:00Z",
        })
    }

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
        assert!(next_arrival_title(&board).is_none());
    }

    #[test]
    fn platform_with_no_arrivals_has_no_title() {
        let board = board_json(serde_json::json!([{ "name": "Northbound", "arrivals": [] }]));
        assert!(next_arrival_title(&board).is_none());
    }

    #[test]
    fn picks_soonest_arrival_and_formats_it() {
        // 200s -> "3 mins", 40s -> "1 min"; soonest is 40s.
        let board = board_with(&[200, 40]);
        assert_eq!(next_arrival_title(&board).as_deref(), Some("1 min"));
    }

    #[test]
    fn imminent_arrival_is_due() {
        let board = board_with(&[10]);
        assert_eq!(next_arrival_title(&board).as_deref(), Some("Due"));
    }

    #[test]
    fn soonest_is_taken_across_platforms_not_just_the_first() {
        // First platform's soonest is 300s ("5 mins"); a later platform has a
        // 30s train ("1 min"). The projection must scan ALL platforms.
        let board = board_json(serde_json::json!([
            { "name": "Northbound", "arrivals": [arrival_json(0, 300)] },
            { "name": "Southbound", "arrivals": [arrival_json(1, 30)] },
        ]));
        assert_eq!(next_arrival_title(&board).as_deref(), Some("1 min"));
    }
}
