//! Pure filtering functions for arrivals.
//!
//! All functions are free functions — no IO, no async.
//!
//! ## Why `line_ids` is not filtered here
//!
//! The user-facing **line-id chip filter** is applied on the frontend
//! display layer (`web/src/lib/components/Board.svelte`'s
//! `displayPlatforms` derived), NOT in this crate. Filtering at the
//! display layer means a chip toggle in Settings updates the visible
//! board instantly — without waiting for the next periodic stream tick
//! (~30 s) for the backend to re-apply and re-emit. See CLAUDE.md
//! invariants #3 (filter changes don't refetch) and #22 (line-id
//! filter is display-layer only) for the architectural rationale.
//!
//! Two related filters that DO stay in the backend:
//!
//! - **`directions`** (here): a per-tick filter that drops arrivals
//!   whose compass direction the user has hidden. Direction toggles
//!   are infrequent compared to line toggles, and the cost of the
//!   tick-delay is minor; keeping this in Rust keeps the backend's
//!   `Board` payload close to what the user wants to see, reducing
//!   unnecessary chrome in tests and snapshots.
//! - **`drop_arrivals_for_lines_not_serving`** (in `service.rs`):
//!   a defensive integrity filter that drops arrivals whose `line_id`
//!   is not in the station's allowed-lines set (per
//!   `TflClient::allowed_line_ids_for`). This guards against TfL
//!   surfacing predictions for lines that don't physically serve the
//!   queried station — independent of user preference, so it stays
//!   server-side regardless of the chip filter location.

use crate::config::BoardConfig;
use tfl_domain::{Arrival, Direction};

/// Apply the direction filter from `cfg` to a list of arrivals.
///
/// `line_ids` is intentionally NOT applied here — see the module docs.
///
/// - If `cfg.directions` is empty, no direction filter is applied.
pub fn apply_filters(arrivals: Vec<Arrival>, cfg: &BoardConfig) -> Vec<Arrival> {
    arrivals
        .into_iter()
        .filter(|a| direction_matches(a.direction, &cfg.directions))
        .collect()
}

/// Returns `true` if `direction` matches any entry in `filter`.
/// Returns `true` unconditionally when `filter` is empty.
fn direction_matches(direction: Direction, filter: &[Direction]) -> bool {
    if filter.is_empty() {
        return true;
    }
    filter.contains(&direction)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_arrival(line_id: &str, direction: Direction) -> Arrival {
        Arrival {
            id: "test-id".to_string(),
            station_name: "Test Station".to_string(),
            platform_name: "Platform 1".to_string(),
            line_id: line_id.to_string(),
            line_name: line_id.to_string(),
            direction,
            northern_branch: None,
            destination_name: "Destination".to_string(),
            towards: "Destination".to_string(),
            current_location: "At station".to_string(),
            time_to_station: 60,
            expected_arrival: Utc::now(),
            naptan_id: "940GZZLUTEST".to_string(),
        }
    }

    #[test]
    fn filter_by_directions_empty_matches_all() {
        let arrivals = vec![
            make_arrival("northern", Direction::Northbound),
            make_arrival("northern", Direction::Southbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            line_ids: vec![],
            directions: vec![], // empty = no filter
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals, &cfg);
        assert_eq!(result.len(), 2, "empty directions should pass all arrivals");
    }

    /// `line_ids` is the user's chip-filter preference and is now applied
    /// at the frontend display layer (`Board.svelte`'s `displayPlatforms`),
    /// NOT in this function. Setting `cfg.line_ids` here MUST be a
    /// no-op — every arrival passes regardless of which lines the user
    /// has selected. Guards against accidentally re-introducing
    /// backend-side line filtering (which would re-introduce the
    /// 30-second tick delay between chip toggle and visible effect).
    #[test]
    fn apply_filters_does_not_filter_by_line_id() {
        let arrivals = vec![
            make_arrival("northern", Direction::Northbound),
            make_arrival("victoria", Direction::Northbound),
            make_arrival("piccadilly", Direction::Westbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            // The user has narrowed to "northern" only on the frontend.
            // The backend MUST still hand the full set through.
            line_ids: vec!["northern".to_string()],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals, &cfg);
        assert_eq!(
            result.len(),
            3,
            "line_ids is a frontend-only display mask; backend must pass all arrivals"
        );
        assert!(result.iter().any(|a| a.line_id == "victoria"));
        assert!(result.iter().any(|a| a.line_id == "piccadilly"));
    }

    #[test]
    fn filter_by_direction_northbound() {
        let arrivals = vec![
            make_arrival("northern", Direction::Northbound),
            make_arrival("northern", Direction::Southbound),
            make_arrival("northern", Direction::Northbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            line_ids: vec![],
            directions: vec![Direction::Northbound],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals, &cfg);
        // Both Northbound variants should match, even with different `via` values
        assert_eq!(result.len(), 2, "both Northbound variants should match");
        assert!(result.iter().all(|a| a.direction == Direction::Northbound));
    }

    /// `line_ids` is ignored; only `directions` survives at the
    /// backend. With both set, the result reflects only the direction
    /// filter — the line filter applies later, at the frontend.
    #[test]
    fn line_ids_ignored_directions_still_filter() {
        let arrivals = vec![
            make_arrival("northern", Direction::Northbound),
            make_arrival("northern", Direction::Southbound),
            make_arrival("victoria", Direction::Northbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            // line_ids set, but ignored.
            line_ids: vec!["northern".to_string()],
            // directions still applies.
            directions: vec![Direction::Northbound],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals, &cfg);
        assert_eq!(
            result.len(),
            2,
            "both Northbound arrivals (northern + victoria) survive; line_ids is no-op"
        );
        assert!(result.iter().all(|a| a.direction == Direction::Northbound));
    }
}
