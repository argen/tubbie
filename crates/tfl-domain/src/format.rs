//! Formatting helpers for TfL domain values.
//!
//! All functions are pure (no I/O, no clock access). The caller is responsible
//! for deriving a `Duration` from `time_to_station` before calling in.

use crate::types::{Arrival, Platform};
use chrono::Duration;

/// Format a `time_to_station` duration into the TfL dot-matrix display string.
///
/// Thresholds match real TfL departure boards:
/// - Negative or `< 30 s` → `"Due"`
/// - `30 s..< 90 s`       → `"1 min"`
/// - `≥ 90 s`             → `"N mins"` where N = half-up rounding to nearest minute
///
/// No upper cap — TfL rarely predicts >60 min but the function remains robust.
pub fn format_time_to_station(duration: Duration) -> String {
    let secs = duration.num_seconds();
    if secs < 30 {
        return "Due".to_string();
    }
    if secs < 90 {
        return "1 min".to_string();
    }
    // Half-up rounding: (secs + 30) / 60
    let mins = (secs + 30) / 60;
    format!("{mins} mins")
}

/// Group a flat list of `Arrival`s into `Platform`s.
///
/// The group key is the verbatim `platform_name` string from TfL.
/// Within each platform, arrivals are sorted by `time_to_station` ascending
/// (soonest first). The sort is stable so equal-time arrivals remain in
/// insertion order.
///
/// Platforms appear in the output in the order their first arrival was
/// encountered in the input. Empty input → empty output.
pub fn group_by_platform(arrivals: Vec<Arrival>) -> Vec<Platform> {
    let mut platform_order: Vec<String> = Vec::new();
    let mut platform_map: std::collections::HashMap<String, Vec<Arrival>> =
        std::collections::HashMap::new();

    for arrival in arrivals {
        let key = arrival.platform_name.clone();
        if !platform_map.contains_key(&key) {
            platform_order.push(key.clone());
        }
        platform_map.entry(key).or_default().push(arrival);
    }

    platform_order
        .into_iter()
        .map(|name| {
            // Invariant: every name in platform_order was inserted into platform_map
            // in the same loop, so remove() must succeed.
            let mut arrivals = platform_map
                .remove(&name)
                .expect("platform_map key guaranteed by construction — name came from platform_order in the same loop");
            arrivals.sort_by_key(|a| a.time_to_station);
            Platform { name, arrivals }
        })
        .collect()
}
