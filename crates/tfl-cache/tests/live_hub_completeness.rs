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
//! Append to `tfl_cache::CANONICAL_MULTI_MODE_HUBS`. The
//! generated test name pattern in this file iterates the const, so a
//! new entry produces a new `#[tokio::test]` automatically — but the
//! per-test `#[tokio::test]` attribute can't be applied dynamically, so
//! one fn per hub is hand-written below. Keep them in sync with the
//! const.

#![cfg(feature = "live")]

use tfl_cache::{TflClient, CANONICAL_MULTI_MODE_HUBS};
use tfl_client::http::ReqwestTflHttp;

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
async fn assert_live_hub_serves(case: &str, station_id: &str, expected: &[&str]) {
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

    let mut missing: Vec<&str> = Vec::new();
    for line in expected {
        if !allowed.contains(*line) {
            missing.push(line);
        }
    }
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

// One #[tokio::test] per canonical hub, so a CI failure names the
// offender. The const comment in `cache.rs` documents what each id
// corresponds to.

#[tokio::test(flavor = "multi_thread")]
async fn live_tcr_serves_central_northern_elizabeth() {
    assert_live_hub_serves(
        "TCR",
        CANONICAL_MULTI_MODE_HUBS[0].0,
        CANONICAL_MULTI_MODE_HUBS[0].1,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_bank_serves_tube_and_dlr() {
    assert_live_hub_serves(
        "Bank",
        CANONICAL_MULTI_MODE_HUBS[1].0,
        CANONICAL_MULTI_MODE_HUBS[1].1,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_liverpool_street_serves_tube_elizabeth_weaver() {
    assert_live_hub_serves(
        "Liverpool Street",
        CANONICAL_MULTI_MODE_HUBS[2].0,
        CANONICAL_MULTI_MODE_HUBS[2].1,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_stratford_serves_tube_dlr_elizabeth_mildmay() {
    assert_live_hub_serves(
        "Stratford",
        CANONICAL_MULTI_MODE_HUBS[3].0,
        CANONICAL_MULTI_MODE_HUBS[3].1,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_canary_wharf_serves_jubilee_dlr_elizabeth() {
    assert_live_hub_serves(
        "Canary Wharf",
        CANONICAL_MULTI_MODE_HUBS[4].0,
        CANONICAL_MULTI_MODE_HUBS[4].1,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_whitechapel_serves_tube_elizabeth_mildmay_windrush() {
    assert_live_hub_serves(
        "Whitechapel",
        CANONICAL_MULTI_MODE_HUBS[5].0,
        CANONICAL_MULTI_MODE_HUBS[5].1,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_paddington_serves_tube_and_elizabeth() {
    assert_live_hub_serves(
        "Paddington",
        CANONICAL_MULTI_MODE_HUBS[6].0,
        CANONICAL_MULTI_MODE_HUBS[6].1,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_farringdon_serves_tube_and_elizabeth() {
    assert_live_hub_serves(
        "Farringdon",
        CANONICAL_MULTI_MODE_HUBS[7].0,
        CANONICAL_MULTI_MODE_HUBS[7].1,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn live_bond_street_serves_central_jubilee_elizabeth() {
    assert_live_hub_serves(
        "Bond Street",
        CANONICAL_MULTI_MODE_HUBS[8].0,
        CANONICAL_MULTI_MODE_HUBS[8].1,
    )
    .await;
}
