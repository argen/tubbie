//! Live integration tests that pin the "missing Elizabeth / Overground
//! at hub stations" bug class against real TfL data.
//!
//! These tests are `#[ignore]`'d so `cargo test` doesn't run them by
//! default — they need network and burn anonymous TfL quota. To run
//! them locally: `cargo test -p tfl-ffi --test live_hub_completeness
//! -- --ignored`.
//!
//! ## Why this harness exists
//!
//! Two compounding upstream bugs caused Elizabeth-line predictions to
//! disappear from the iOS board at Liverpool Street and Tottenham
//! Court Road, even though `search_stations_live` and `get_arrivals`
//! returned them correctly at the seams:
//!
//! 1. **Hub-merge silent failure** — `TflClient::hub_lines_cached`
//!    silently swallows transient errors (`Err(_) => return vec![]`),
//!    leaving the queried station's `Station.lines` field with only
//!    the tube parent's lines. Once that's stamped into
//!    `stop_points_cache`, the defensive `drop_arrivals_for_lines_not_serving`
//!    filter rejects every Elizabeth / Overground prediction for the
//!    rest of the cache TTL (15 minutes). Empirically reproduces on
//!    ~20 % of cold launches against the anonymous TfL bucket.
//!
//! 2. **`infer_compass_from_towards` reads only `towards`** — TfL
//!    emits `towards: ""` (empty string) for Elizabeth-line and
//!    Overground predictions at Liverpool Street. The per-line
//!    compass mapping has nothing to match against, falls through to
//!    the raw `direction` field (`"outbound"` / `"inbound"` /
//!    empty), and the resulting `Direction::Outbound` /
//!    `Direction::Inbound` / `Direction::Unknown` is then dropped by
//!    `drop_off_axis_predictions` because the line's `CompassAxis`
//!    is pinned to `EastWest`. The fallback should consult
//!    `destination_name` when `towards` is empty.
//!
//! ## What the tests assert
//!
//! - Liverpool Street's allowed-line set after warm contains both
//!   `elizabeth` and `weaver`.
//! - Liverpool Street's filtered board (post `apply_filters` +
//!   defensive filter + off-axis filter + hub-merge) contains at
//!   least one `elizabeth` arrival and at least one `weaver` arrival.
//! - Same for Tottenham Court Road's `elizabeth`.
//!
//! ## How to interpret a failure
//!
//! - **`allowed_line_ids_for` missing `elizabeth`** → bug 1
//!   (hub-merge silent failure). Re-run a few times; if it's
//!   consistent, look at `hub_lines_cached`'s error swallowing.
//! - **`allowed_line_ids_for` has `elizabeth` but the board has
//!   none** → bug 2 (`infer_compass_from_towards` → off-axis
//!   filter chain). Look at `direction.rs` and
//!   `drop_off_axis_predictions`.
//! - **Both** → both bugs are still active.
//!
//! ## Hub-vectors.json — single source of truth
//!
//! The canonical `(station_id, expected_lines)` pairs are loaded from
//! `tests/fixtures/hub-vectors.json`.
//! The per-fn tests below use `scenario_from_json(N)` — an index into the
//! positive scenarios in that file. The upstream consistency test in
//! `tfl-cache/src/multi_mode_hub_completeness_tests.rs` guards that
//! `CANONICAL_MULTI_MODE_HUBS` and the JSON agree, so the iOS side is
//! automatically in sync.

use std::sync::Arc;

use tfl_board::{BoardConfig, BoardService};
use tfl_cache::TflClient;
use tfl_client::clock::SystemClock;
use tfl_client::http::ReqwestTflHttp;

// ---------------------------------------------------------------------------
// JSON loading — tests/fixtures/hub-vectors.json
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct HubVectorFile {
    scenarios: Vec<HubVectorScenario>,
}

#[derive(serde::Deserialize)]
struct HubVectorScenario {
    id: String,
    station_id: String,
    expected_lines: Vec<String>,
    negative: bool,
}

/// Load the positive scenarios from
/// `tests/fixtures/hub-vectors.json`. The
/// path is resolved from `CARGO_MANIFEST_DIR` (`crates/tfl-ffi/`) to
/// `../../tests/fixtures/hub-vectors.json`.
fn load_positive_scenarios() -> Vec<HubVectorScenario> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let json_path = manifest
        .join("..") // crates/
        .join("..") // repo root
        .join("tests")
        .join("fixtures")
        .join("hub-vectors.json");
    let json_path = json_path.canonicalize().unwrap_or_else(|e| {
        panic!(
            "hub-vectors.json not found at {}: {}. \
             Is the tubbie submodule pinned to a SHA that includes 3A?",
            json_path.display(),
            e
        )
    });
    let raw = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", json_path.display(), e));
    let file: HubVectorFile = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("failed to parse {}: {}", json_path.display(), e));
    file.scenarios.into_iter().filter(|s| !s.negative).collect()
}

/// Load one positive scenario by 0-based index. Panics on out-of-bounds.
fn scenario_from_json(index: usize) -> HubVectorScenario {
    let mut scenarios = load_positive_scenarios();
    assert!(
        index < scenarios.len(),
        "scenario index {index} out of bounds (only {} positive scenarios in hub-vectors.json)",
        scenarios.len()
    );
    scenarios.remove(index)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Subset of [`refresh_and_count`] that only warms + reads
/// `allowed_line_ids_for`. Used by the canonical-hub coverage tests
/// below where we want to pin the hub-merge contract without burning
/// extra arrivals fetches per hub. Skipping the board refresh keeps the
/// 8-station sweep inside the anonymous TfL bucket.
async fn allowed_after_warm(station_id: &str) -> std::collections::HashSet<String> {
    let http = ReqwestTflHttp::new();
    let client = Arc::new(TflClient::new(http));

    client
        .warm_stop_points_cache()
        .await
        .expect("warm should succeed against live TfL");

    client
        .allowed_line_ids_for(station_id)
        .await
        .expect("allowed_line_ids_for should succeed")
}

/// Pin the (station_id, expected_lines) contract from
/// `hub-vectors.json` against live TfL. Mirrors the upstream
/// Layer 2 test in `tfl-cache/tests/live_hub_completeness.rs` — the
/// duplication is intentional: this iOS-side copy runs against the
/// SUBSET-modes client (`TflClient::new` uses every mode in
/// `SUPPORTED_MODES`), and its failure mode is exactly what an iOS
/// user experiences after warm. If only the upstream test exists, an
/// iOS-only mode-list misconfiguration ships unprotected.
async fn assert_canonical_hub_serves(case: &str, station_id: &str, expected: &[String]) {
    let allowed = allowed_after_warm(station_id).await;
    let missing: Vec<&str> = expected
        .iter()
        .filter(|l| !allowed.contains(*l))
        .map(String::as_str)
        .collect();
    let mut got: Vec<&str> = allowed.iter().map(String::as_str).collect();
    got.sort();
    assert!(
        missing.is_empty(),
        "[{case}] live {station_id} must serve {expected:?}; \
         missing: {missing:?}; live allowed set: {got:?}. \
         If this is real (not flake), the user is currently seeing the \
         \"Elizabeth missing at TCR / DLR missing at Bank\" bug class \
         on TestFlight RIGHT NOW. Re-run; if persistent, check \
         hub_lines_cached for cache-poisoning paths.",
    );
}

/// Refresh once and return the resulting board's per-line arrival
/// counts. Goes through the same `BoardService::refresh` path as
/// `subscribe_board_live`'s first emit, so a failure here is exactly
/// what an iOS user experiences on launch.
async fn refresh_and_count(
    station_id: &str,
) -> (
    std::collections::BTreeMap<String, usize>,
    std::collections::HashSet<String>,
) {
    let http = ReqwestTflHttp::new();
    let client = Arc::new(TflClient::new(http));

    client
        .warm_stop_points_cache()
        .await
        .expect("warm should succeed against live TfL");

    let allowed = client
        .allowed_line_ids_for(station_id)
        .await
        .expect("allowed_line_ids_for should succeed");

    let service = BoardService::new(Arc::clone(&client), SystemClock);
    let cfg = BoardConfig::new(station_id);
    let board = service.refresh(&cfg).await.expect("refresh should succeed");

    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for p in &board.platforms {
        for a in &p.arrivals {
            *counts.entry(a.line_id.clone()).or_default() += 1;
        }
    }

    (counts, allowed)
}

// ---------------------------------------------------------------------------
// Detailed board-level tests (Liverpool Street + TCR)
// ---------------------------------------------------------------------------

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn liverpool_street_board_includes_elizabeth_and_weaver() {
    let (counts, allowed) = refresh_and_count("940GZZLULVT").await;

    // Allowed-line set carries the hub-merged lines. If `elizabeth` or
    // `weaver` is missing here, the bug is in `hub_lines_cached`'s
    // silent error swallow (bug 1 in this file's docstring).
    assert!(
        allowed.contains("elizabeth"),
        "Liverpool Street allowed_line_ids_for must include 'elizabeth' \
         after warm. Got: {:?}. Likely bug: hub_lines_cached swallowed a \
         transient TfL error during warm — see hub_lines_cached's \
         `Err(_) => return vec![]` branch.",
        allowed.iter().collect::<Vec<_>>()
    );
    assert!(
        allowed.contains("weaver"),
        "Liverpool Street allowed_line_ids_for must include 'weaver' \
         after warm. Got: {:?}.",
        allowed.iter().collect::<Vec<_>>()
    );

    // Even when `allowed` is correct, off-axis filtering can silently
    // drop every Elizabeth / Weaver prediction if their direction
    // resolves to Inbound/Outbound/Unknown. Empirically: TfL emits
    // `towards: ""` for these lines, so `infer_compass_from_towards`
    // produces nothing and the fall-through to raw `direction`
    // produces `Outbound`, which `drop_off_axis_predictions` then
    // kills (line axis pinned to `EastWest`). Fix: consult
    // `destination_name` as a fallback in
    // `infer_compass_from_towards`.
    let elizabeth_count = counts.get("elizabeth").copied().unwrap_or(0);
    let weaver_count = counts.get("weaver").copied().unwrap_or(0);
    assert!(
        elizabeth_count > 0,
        "Liverpool Street board must include at least one Elizabeth \
         arrival post-filtering. Got line counts: {counts:?}. \
         Likely bug: infer_compass_from_towards relies on the \
         `towards` field which TfL leaves empty for Elizabeth at \
         this station; predictions fall through to Direction::Outbound \
         and drop_off_axis_predictions removes them.",
    );
    assert!(
        weaver_count > 0,
        "Liverpool Street board must include at least one Weaver \
         arrival post-filtering. Got line counts: {counts:?}.",
    );
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn tottenham_court_road_board_includes_elizabeth() {
    let (counts, allowed) = refresh_and_count("940GZZLUTCR").await;

    assert!(
        allowed.contains("elizabeth"),
        "TCR allowed_line_ids_for must include 'elizabeth'. Got: {:?}.",
        allowed.iter().collect::<Vec<_>>()
    );
    let elizabeth_count = counts.get("elizabeth").copied().unwrap_or(0);
    assert!(
        elizabeth_count > 0,
        "TCR board must include at least one Elizabeth arrival \
         post-filtering. Got line counts: {counts:?}.",
    );
}

// ---------------------------------------------------------------------------
// Canonical multi-mode hub coverage (Layer 2 mirror, iOS-side).
// Vectors loaded from hub-vectors.json via scenario_from_json(N).
// ---------------------------------------------------------------------------
//
// One test per entry in hub-vectors.json positive scenarios. Each pins only
// the hub-merge contract (`allowed_line_ids_for` superset) — board-arrival
// counts depend on time of day and a quiet evening would flake them.
//
// Why duplicate the upstream `tfl-cache/tests/live_hub_completeness.rs`:
// the upstream test runs against the FULL-modes client. This file runs
// against whatever the iOS shell instantiates — today the same full set,
// but the public `TflClient::with_modes` knob exists for a reason and an
// accidental subset-modes regression on iOS would silently ship without
// this guard. Adding a hub: add to hub-vectors.json AND
// `CANONICAL_MULTI_MODE_HUBS`, then add one fn here calling
// `scenario_from_json(N)` at the new index.

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_tcr_hub_merge_serves_elizabeth() {
    let s = scenario_from_json(0);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_bank_hub_merge_serves_dlr() {
    let s = scenario_from_json(1);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_liverpool_street_hub_merge_serves_elizabeth_and_weaver() {
    let s = scenario_from_json(2);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_stratford_hub_merge_serves_dlr_elizabeth_mildmay() {
    let s = scenario_from_json(3);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_canary_wharf_hub_merge_serves_dlr_and_elizabeth() {
    let s = scenario_from_json(4);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_whitechapel_hub_merge_serves_elizabeth_mildmay_windrush() {
    let s = scenario_from_json(5);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_paddington_hub_merge_serves_elizabeth() {
    let s = scenario_from_json(6);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_farringdon_hub_merge_serves_elizabeth() {
    let s = scenario_from_json(7);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[ignore = "live network; opt-in with --ignored"]
#[tokio::test(flavor = "multi_thread")]
async fn canonical_bond_street_hub_merge_serves_elizabeth() {
    let s = scenario_from_json(8);
    assert_canonical_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}
