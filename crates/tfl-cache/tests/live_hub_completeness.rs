//! Layer 2 of the multi-mode hub regression harness — hits real TfL.
//!
//! Mirrors `multi_mode_hub_completeness_tests.rs` (the fixture-based
//! Layer 1) against `api.tfl.gov.uk`. The fixture layer pins the contract
//! shape; this live layer pins that the *real* world also satisfies the
//! contract today. Together they catch:
//!
//! - Layer 1 RED → fixture-shaped regression in `cache.rs` / hub-merge.
//! - Layer 2 RED → live TfL drift (a hub temporarily 404'ing, a feed
//!   dropping a line) — the case Layer 1 cannot see.
//!
//! ## How to run
//!
//! ```sh
//! cargo test -p tfl-cache --features live --test live_hub_completeness
//! ```
//!
//! With `TFL_APP_KEY` set, the anonymous-bucket 50 req/min ceiling
//! disappears and these tests run reliably back-to-back.
//!
//! ## CI gating
//!
//! Wired into `tubbie-ios/Justfile`'s `bump-core` recipe with the
//! `live=1` parameter — bumps that touch `tfl-cache` / `tfl-client` /
//! `tfl-domain` / `tfl-board` MUST run this. See `tubbie-ios/CLAUDE.md`.
//!
//! ## Adding a new interchange
//!
//! Add a new positive scenario to `tests/fixtures/hub-vectors.json` (the
//! single source of truth) AND to `CANONICAL_MULTI_MODE_HUBS` in `cache.rs`,
//! then add a matching hand-written `#[tokio::test]` fn below loading
//! `scenario_from_json(N)` at the new index. The consistency test in
//! `multi_mode_hub_completeness_tests.rs` will catch any ordering drift.

#![cfg(feature = "live")]

use tfl_cache::TflClient;
use tfl_client::http::ReqwestTflHttp;

// ---------------------------------------------------------------------------
// JSON loading — hub-vectors.json is the single source of truth
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

/// Load the positive scenarios from `tests/fixtures/hub-vectors.json` at the
/// workspace root. Panics if the file is missing or malformed — a missing
/// fixture is a bug, not a skip.
fn load_positive_scenarios() -> Vec<HubVectorScenario> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let json_path = manifest
        .join("..") // crates/
        .join("..") // workspace root
        .join("tests")
        .join("fixtures")
        .join("hub-vectors.json");
    let json_path = json_path.canonicalize().unwrap_or_else(|e| {
        panic!(
            "hub-vectors.json not found at {}: {}",
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

/// Returns true if `api.tfl.gov.uk` is reachable. Mirrors the helper in
/// `live_tfl.rs` so this file is self-contained — copy is intentional;
/// reaching across `tests/` files would require pulling each into a
/// shared module file, which is more friction than two duplicated lines.
async fn tfl_api_reachable() -> bool {
    reqwest::get("https://api.tfl.gov.uk/Line/tube/Route")
        .await
        .map(|r| r.status().is_success() || r.status().as_u16() == 429)
        .unwrap_or(false)
}

/// Build a fresh client, run a real warm, and assert that
/// `station_id`'s allowed-line set is a superset of `expected`. Skips
/// silently if the network is unreachable (matches the live_tfl.rs
/// convention; CI without internet must not turn this red).
async fn assert_live_hub_serves(case: &str, station_id: &str, expected: &[String]) {
    if !tfl_api_reachable().await {
        eprintln!("SKIP [{case}]: api.tfl.gov.uk unreachable");
        return;
    }

    let http = ReqwestTflHttp::new();
    let client = TflClient::new(http);

    client
        .warm_stop_points_cache()
        .await
        .unwrap_or_else(|e| panic!("[{case}] live warm should succeed: {e}"));

    let allowed = client
        .allowed_line_ids_for(station_id)
        .await
        .unwrap_or_else(|e| panic!("[{case}] allowed_line_ids_for should succeed: {e}"));

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
         If this is real (not flake), the user is currently seeing \
         the bug class \"Elizabeth missing at TCR / DLR missing at \
         Bank\" on TestFlight RIGHT NOW. Re-run; if persistent, check \
         hub_lines_cached for cache-poisoning paths and the per-mode \
         warm retry path for sustained transient failures.",
    );
}

// ---------------------------------------------------------------------------
// One #[tokio::test] per canonical hub — scenario loaded from hub-vectors.json
// at the matching index. CI failure names the offender.
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn live_tcr_serves_central_northern_elizabeth() {
    let s = scenario_from_json(0);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_bank_serves_tube_and_dlr() {
    let s = scenario_from_json(1);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_liverpool_street_serves_tube_elizabeth_weaver() {
    let s = scenario_from_json(2);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_stratford_serves_tube_dlr_elizabeth_mildmay() {
    let s = scenario_from_json(3);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_canary_wharf_serves_jubilee_dlr_elizabeth() {
    let s = scenario_from_json(4);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_whitechapel_serves_tube_elizabeth_mildmay_windrush() {
    let s = scenario_from_json(5);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_paddington_serves_tube_and_elizabeth() {
    let s = scenario_from_json(6);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_farringdon_serves_tube_and_elizabeth() {
    let s = scenario_from_json(7);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_bond_street_serves_central_jubilee_elizabeth() {
    let s = scenario_from_json(8);
    assert_live_hub_serves(&s.id, &s.station_id, &s.expected_lines).await;
}
