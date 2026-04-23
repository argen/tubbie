//! Behavioural tests for `format_time_to_station`.
//!
//! Each row: (seconds, expected_label).
//! Thresholds per plan + TfL board convention:
//!   < 30 s          → "Due"
//!   30 s .. < 90 s  → "1 min"
//!   ≥ 90 s          → "N mins" (N = round-half-up to nearest minute)
//!   negative        → "Due"   (already departed; do not panic)

use chrono::Duration;
use tfl_domain::format::format_time_to_station;

#[test]
fn format_cases() {
    let cases: &[(i64, &str)] = &[
        // Negative — already departed
        (-10, "Due"),
        (-1, "Due"),
        // Zero
        (0, "Due"),
        // Sub-30 seconds
        (1, "Due"),
        (29, "Due"),
        // Exactly 30 s — boundary into "1 min"
        (30, "1 min"),
        (89, "1 min"),
        // Exactly 90 s — boundary into "N mins"
        (90, "2 mins"),
        // Rounding: 91 s → 1.52 min → rounds to 2
        (91, "2 mins"),
        // 150 s = 2.5 min → rounds to 3 (half-up: (150+30)/60 = 3)
        (150, "3 mins"),
        // 217 s (from Belsize Park fixture) → (217+30)/60 = 4 → "4 mins"
        (217, "4 mins"),
        // 847 s → (847+30)/60 = 14 → "14 mins"
        (847, "14 mins"),
        // 3600 s = 60 min → still "N mins" (no hours)
        (3600, "60 mins"),
        // 3660 s = 61 min
        (3660, "61 mins"),
    ];

    for &(secs, expected) in cases {
        let d = Duration::seconds(secs);
        let got = format_time_to_station(d);
        assert_eq!(
            got, expected,
            "format_time_to_station({secs}s) expected {expected:?} but got {got:?}"
        );
    }
}
