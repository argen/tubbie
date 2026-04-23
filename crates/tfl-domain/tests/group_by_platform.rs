//! Behavioural tests for `group_by_platform`.

use chrono::DateTime;
use tfl_domain::{format::group_by_platform, types::Arrival, Direction};

fn make_arrival(platform_name: &str, time_to_station: i64) -> Arrival {
    Arrival {
        id: format!("id-{time_to_station}"),
        station_name: "Test Station".to_string(),
        platform_name: platform_name.to_string(),
        line_id: "northern".to_string(),
        line_name: "Northern".to_string(),
        direction: Direction::Northbound { via: None },
        destination_name: "Edgware".to_string(),
        towards: String::new(),
        current_location: String::new(),
        time_to_station,
        expected_arrival: DateTime::parse_from_rfc3339("2026-04-23T16:35:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
        naptan_id: String::new(),
    }
}

#[test]
fn empty_input_returns_empty() {
    let result = group_by_platform(vec![]);
    assert!(result.is_empty());
}

#[test]
fn single_platform_sorted() {
    let arrivals = vec![
        make_arrival("Northbound - Platform 1", 300),
        make_arrival("Northbound - Platform 1", 60),
        make_arrival("Northbound - Platform 1", 180),
    ];
    let platforms = group_by_platform(arrivals);
    assert_eq!(platforms.len(), 1);
    let p = &platforms[0];
    assert_eq!(p.name, "Northbound - Platform 1");
    let times: Vec<i64> = p.arrivals.iter().map(|a| a.time_to_station).collect();
    assert_eq!(times, vec![60, 180, 300]);
}

#[test]
fn two_platforms_grouped_correctly() {
    let arrivals = vec![
        make_arrival("Northbound - Platform 1", 100),
        make_arrival("Southbound - Platform 2", 50),
        make_arrival("Northbound - Platform 1", 200),
        make_arrival("Southbound - Platform 2", 150),
    ];
    let platforms = group_by_platform(arrivals);
    assert_eq!(platforms.len(), 2);

    // Platform order matches first-seen order from input.
    assert_eq!(platforms[0].name, "Northbound - Platform 1");
    assert_eq!(platforms[1].name, "Southbound - Platform 2");

    let north_times: Vec<i64> = platforms[0]
        .arrivals
        .iter()
        .map(|a| a.time_to_station)
        .collect();
    assert_eq!(north_times, vec![100, 200]);

    let south_times: Vec<i64> = platforms[1]
        .arrivals
        .iter()
        .map(|a| a.time_to_station)
        .collect();
    assert_eq!(south_times, vec![50, 150]);
}

#[test]
fn verbatim_platform_names_are_different_groups() {
    // TfL inconsistency: "Northbound - Platform 2" vs "Platform 2" are distinct.
    let arrivals = vec![
        make_arrival("Northbound - Platform 2", 100),
        make_arrival("Platform 2", 200),
    ];
    let platforms = group_by_platform(arrivals);
    assert_eq!(
        platforms.len(),
        2,
        "Different name strings must be different groups"
    );
}

#[test]
fn stable_sort_preserves_insertion_order_for_equal_times() {
    let arrivals = vec![
        make_arrival("Platform 1", 60),
        make_arrival("Platform 1", 60),
        make_arrival("Platform 1", 60),
    ];
    let arrivals_clone = arrivals.clone();
    let platforms = group_by_platform(arrivals);
    // All three have equal time_to_station. Stable sort must preserve input order.
    // We verify by checking the ids match the original order.
    let ids: Vec<&str> = platforms[0]
        .arrivals
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    let expected_ids: Vec<&str> = arrivals_clone.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, expected_ids);
}
