//! Pure filtering functions for arrivals.
//!
//! All functions are free functions — no IO, no async.

use crate::config::BoardConfig;
use tfl_domain::{Arrival, Direction};

/// Apply line and direction filters from `cfg` to a list of arrivals.
///
/// - If `cfg.line_ids` is empty, no line filter is applied.
/// - If `cfg.directions` is empty, no direction filter is applied.
/// - Both filters are AND-ed together.
pub fn apply_filters(arrivals: Vec<Arrival>, cfg: &BoardConfig) -> Vec<Arrival> {
    arrivals
        .into_iter()
        .filter(|a| line_matches(&a.line_id, &cfg.line_ids))
        .filter(|a| direction_matches(a.direction, &cfg.directions))
        .collect()
}

/// Returns `true` if `line_id` matches the filter list (case-insensitive).
/// Returns `true` unconditionally when `filter` is empty.
fn line_matches(line_id: &str, filter: &[String]) -> bool {
    if filter.is_empty() {
        return true;
    }
    let id_lower = line_id.to_ascii_lowercase();
    filter.iter().any(|f| f.to_ascii_lowercase() == id_lower)
}

/// Returns `true` if `direction` matches any entry in `filter`.
/// Returns `true` unconditionally when `filter` is empty.
fn direction_matches(direction: Direction, filter: &[Direction]) -> bool {
    if filter.is_empty() {
        return true;
    }
    filter.iter().any(|f| *f == direction)
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
    fn filter_by_line_ids_empty_matches_all() {
        let arrivals = vec![
            make_arrival("northern", Direction::Northbound),
            make_arrival("victoria", Direction::Northbound),
            make_arrival("piccadilly", Direction::Westbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            line_ids: vec![], // empty = no filter
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals.clone(), &cfg);
        assert_eq!(result.len(), 3, "empty line_ids should pass all arrivals");
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

    #[test]
    fn filter_single_line_id() {
        let arrivals = vec![
            make_arrival("northern", Direction::Northbound),
            make_arrival("victoria", Direction::Northbound),
            make_arrival("northern", Direction::Southbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            line_ids: vec!["northern".to_string()],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals, &cfg);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|a| a.line_id == "northern"));
    }

    #[test]
    fn filter_line_id_case_insensitive() {
        let arrivals = vec![
            make_arrival("northern", Direction::Northbound),
            make_arrival("victoria", Direction::Northbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            line_ids: vec!["Northern".to_string()],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals, &cfg);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line_id, "northern");
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
        assert!(result
            .iter()
            .all(|a| a.direction == Direction::Northbound));
    }

    #[test]
    fn filter_combined_line_and_direction() {
        let arrivals = vec![
            make_arrival("northern", Direction::Northbound),
            make_arrival("northern", Direction::Southbound),
            make_arrival("victoria", Direction::Northbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            line_ids: vec!["northern".to_string()],
            directions: vec![Direction::Northbound],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals, &cfg);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line_id, "northern");
        assert!(result[0].direction == Direction::Northbound);
    }

    #[test]
    fn filter_no_matches_returns_empty() {
        let arrivals = vec![
            make_arrival("central", Direction::Eastbound),
            make_arrival("central", Direction::Westbound),
        ];
        let cfg = BoardConfig {
            station_id: "TEST".to_string(),
            line_ids: vec!["northern".to_string()],
            directions: vec![],
            poll_seconds: 20,
            theme: "classic-amber".to_string(),
        };
        let result = apply_filters(arrivals, &cfg);
        assert!(result.is_empty());
    }
}
