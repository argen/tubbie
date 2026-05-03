//! Tests for the canonical TfL severity-code → render-tier bucket mapping.
//!
//! Source of truth for the codes:
//! https://api.tfl.gov.uk/StatusSeverity (queried 2026-05-03; codes 0–20).
//!
//! This bucket mapping is the **single canonical source** consumed by every
//! UI surface (Svelte today, future SwiftUI). UI code MUST NOT redefine it.

use tfl_domain::types::{severity_bucket, SeverityBucket};

// ---------------------------------------------------------------------------
// Per-code bucket assignments — one assertion per documented severity code.
// Lower numeric severity = worse network impact (TfL convention).
// ---------------------------------------------------------------------------

#[test]
fn severity_0_special_service_buckets_to_other() {
    assert_eq!(severity_bucket(0), SeverityBucket::Other);
}

#[test]
fn severity_1_closed_buckets_to_closed() {
    assert_eq!(severity_bucket(1), SeverityBucket::Closed);
}

#[test]
fn severity_2_suspended_buckets_to_closed() {
    assert_eq!(severity_bucket(2), SeverityBucket::Closed);
}

#[test]
fn severity_3_part_suspended_buckets_to_part_closure() {
    assert_eq!(severity_bucket(3), SeverityBucket::PartClosure);
}

#[test]
fn severity_4_planned_closure_buckets_to_part_closure() {
    assert_eq!(severity_bucket(4), SeverityBucket::PartClosure);
}

#[test]
fn severity_5_part_closure_buckets_to_part_closure() {
    assert_eq!(severity_bucket(5), SeverityBucket::PartClosure);
}

#[test]
fn severity_6_severe_delays_buckets_to_severe_delays() {
    assert_eq!(severity_bucket(6), SeverityBucket::SevereDelays);
}

#[test]
fn severity_7_reduced_service_buckets_to_reduced_service() {
    assert_eq!(severity_bucket(7), SeverityBucket::ReducedService);
}

#[test]
fn severity_8_bus_service_buckets_to_reduced_service() {
    // TfL severity 8 = "Bus Service" (rail replaced by bus) — a reduced
    // service from the rail-rider's perspective.
    assert_eq!(severity_bucket(8), SeverityBucket::ReducedService);
}

#[test]
fn severity_9_minor_delays_buckets_to_minor_delays() {
    assert_eq!(severity_bucket(9), SeverityBucket::MinorDelays);
}

#[test]
fn severity_10_good_service_buckets_to_good_service() {
    assert_eq!(severity_bucket(10), SeverityBucket::GoodService);
}

#[test]
fn severity_11_part_closed_buckets_to_part_closure() {
    assert_eq!(severity_bucket(11), SeverityBucket::PartClosure);
}

#[test]
fn severity_12_exit_only_buckets_to_other() {
    assert_eq!(severity_bucket(12), SeverityBucket::Other);
}

#[test]
fn severity_13_no_step_free_buckets_to_other() {
    assert_eq!(severity_bucket(13), SeverityBucket::Other);
}

#[test]
fn severity_14_change_of_frequency_buckets_to_minor_delays() {
    assert_eq!(severity_bucket(14), SeverityBucket::MinorDelays);
}

#[test]
fn severity_15_diverted_buckets_to_reduced_service() {
    assert_eq!(severity_bucket(15), SeverityBucket::ReducedService);
}

#[test]
fn severity_16_not_running_buckets_to_closed() {
    assert_eq!(severity_bucket(16), SeverityBucket::Closed);
}

#[test]
fn severity_17_issues_reported_buckets_to_information() {
    assert_eq!(severity_bucket(17), SeverityBucket::Information);
}

#[test]
fn severity_18_no_issues_buckets_to_good_service() {
    assert_eq!(severity_bucket(18), SeverityBucket::GoodService);
}

#[test]
fn severity_19_information_buckets_to_information() {
    assert_eq!(severity_bucket(19), SeverityBucket::Information);
}

#[test]
fn severity_20_service_closed_buckets_to_closed() {
    assert_eq!(severity_bucket(20), SeverityBucket::Closed);
}

// ---------------------------------------------------------------------------
// Out-of-range codes default to Other so a future TfL extension never panics.
// ---------------------------------------------------------------------------

#[test]
fn negative_severity_defaults_to_other() {
    assert_eq!(severity_bucket(-1), SeverityBucket::Other);
}

#[test]
fn unknown_high_severity_defaults_to_other() {
    assert_eq!(severity_bucket(99), SeverityBucket::Other);
}

// ---------------------------------------------------------------------------
// sort_rank: lower = worse, GoodService sorts last.
// Drives the worst-first ordering on the Status tab — the contract is
// "Closed before SevereDelays before MinorDelays before GoodService".
// ---------------------------------------------------------------------------

#[test]
fn sort_rank_orders_worst_first_then_information_then_good() {
    let order = [
        SeverityBucket::Closed,
        SeverityBucket::PartClosure,
        SeverityBucket::SevereDelays,
        SeverityBucket::ReducedService,
        SeverityBucket::MinorDelays,
        SeverityBucket::Information,
        SeverityBucket::Other,
        SeverityBucket::GoodService,
    ];
    for window in order.windows(2) {
        assert!(
            window[0].sort_rank() < window[1].sort_rank(),
            "{:?} must rank worse than {:?} (lower number)",
            window[0],
            window[1],
        );
    }
}

#[test]
fn sort_rank_good_service_sorts_last() {
    // The footer-grouping logic on the Status tab depends on GoodService
    // having the largest rank; if a new bucket is added that ranks higher,
    // healthy lines will be hidden behind it.
    let max = SeverityBucket::GoodService.sort_rank();
    for b in [
        SeverityBucket::Closed,
        SeverityBucket::PartClosure,
        SeverityBucket::SevereDelays,
        SeverityBucket::ReducedService,
        SeverityBucket::MinorDelays,
        SeverityBucket::Information,
        SeverityBucket::Other,
    ] {
        assert!(b.sort_rank() < max, "{b:?} must sort before GoodService");
    }
}
