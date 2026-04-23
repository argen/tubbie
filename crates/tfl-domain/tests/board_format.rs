//! Insta snapshot test for board formatting.
//!
//! Loads the Belsize Park arrivals fixture at compile time, deserializes into
//! `Vec<Arrival>`, groups by platform, formats each arrival's time using a
//! fixed `DateTime<Utc>` (the fixture's recorded_at timestamp), and snapshots
//! the resulting textual representation.
//!
//! This test is the canary for any formatting regression through M4/M6.

use chrono::{DateTime, Duration, Utc};
use tfl_domain::{
    format::{format_time_to_station, group_by_platform},
    types::Arrival,
};

/// Belsize Park arrivals fixture, embedded at compile time.
const BZP_FIXTURE: &str = include_str!("../../../fixtures/arrivals/940GZZLUBZP.json");

/// Recorded-at timestamp from the fixture meta file.
/// This makes `time_to_station` formatting deterministic regardless of when
/// the test runs.
const RECORDED_AT: &str = "2026-04-23T16:31:48Z";

fn parse_recorded_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(RECORDED_AT)
        .expect("RECORDED_AT must be a valid RFC3339 timestamp")
        .with_timezone(&Utc)
}

/// Format a single arrival into a one-line text string for snapshot comparison.
///
/// Format: `"  {time_label:>6}  {destination}"` matching a simplified board column.
fn format_arrival_line(arrival: &Arrival, now: DateTime<Utc>) -> String {
    // Compute how far in the future the train arrives.  We use expected_arrival
    // minus the fixture clock (not `time_to_station` directly) so the snapshot
    // reflects realistic TfL formatting at the moment the fixture was recorded.
    let seconds_until = (arrival.expected_arrival - now).num_seconds();
    let duration = Duration::seconds(seconds_until);
    let time_label = format_time_to_station(duration);
    format!("  {:>6}  {}", time_label, arrival.destination_name)
}

/// Render the board as a multi-line string: one section per platform.
fn render_board(arrivals: Vec<Arrival>, now: DateTime<Utc>) -> String {
    let platforms = group_by_platform(arrivals);
    let mut out = String::new();
    for platform in &platforms {
        out.push_str(&format!("[{}]\n", platform.name));
        for arrival in &platform.arrivals {
            out.push_str(&format_arrival_line(arrival, now));
            out.push('\n');
        }
    }
    out
}

#[test]
fn board_format_belsize_park_snapshot() {
    let now = parse_recorded_at();
    let arrivals: Vec<Arrival> =
        serde_json::from_str(BZP_FIXTURE).expect("BZP fixture must deserialize");
    let rendered = render_board(arrivals, now);
    insta::assert_snapshot!(rendered);
}
