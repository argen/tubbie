//! Live integration tests that hit `api.tfl.gov.uk`.
//!
//! **These tests are not run in CI.** They require network access and are
//! gated by the `live` cargo feature:
//!
//! ```sh
//! just verify-live
//! # or
//! cargo test -p tfl-cache --features live
//! ```
//!
//! If `TFL_APP_KEY` is set in the environment, the client will use it
//! (recommended to avoid rate-limiting on repeated runs).
//!
//! ## Network availability
//! Each test begins by probing DNS for `api.tfl.gov.uk`. If the probe fails
//! (no network, DNS unavailable), the test prints a message to stderr and
//! passes silently — we never want live tests to make CI flaky.

#![cfg(feature = "live")]

use tfl_cache::TflClient;
use tfl_client::http::ReqwestTflHttp;

// ---------------------------------------------------------------------------
// Probe helper
// ---------------------------------------------------------------------------

/// Returns true if `api.tfl.gov.uk` is reachable (TCP connect to port 443).
///
/// Used to skip tests gracefully when there is no network.
async fn tfl_api_reachable() -> bool {
    reqwest::get("https://api.tfl.gov.uk/Line/tube/Route")
        .await
        .map(|r| r.status().is_success() || r.status().as_u16() == 429)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Fetch arrivals for Belsize Park (940GZZLUBZP).
///
/// Off-peak: the station may genuinely have 0 arrivals predicted; the test
/// passes in that case. The important thing is the call does not error.
#[tokio::test]
async fn live_get_arrivals_belsize_park() {
    if !tfl_api_reachable().await {
        eprintln!("SKIP: api.tfl.gov.uk unreachable (no network)");
        return;
    }

    let http = ReqwestTflHttp::new();
    let client = TflClient::new(http);

    let arrivals = client
        .get_arrivals("940GZZLUBZP")
        .await
        .expect("get_arrivals should not error for a valid stop");

    // Arrivals may be empty off-peak — just verify the call succeeded.
    eprintln!(
        "live_get_arrivals_belsize_park: {} arrival(s) returned",
        arrivals.len()
    );
}

/// Search for "King's Cross" and verify at least one result is returned.
#[tokio::test]
async fn live_search_stations_returns_non_empty_for_tube() {
    if !tfl_api_reachable().await {
        eprintln!("SKIP: api.tfl.gov.uk unreachable (no network)");
        return;
    }

    let http = ReqwestTflHttp::new();
    let client = TflClient::new(http);

    let stations = client
        .search_stations("King's Cross")
        .await
        .expect("search_stations should not error");

    assert!(
        !stations.is_empty(),
        "Expected at least one result for \"King's Cross\", got none"
    );

    eprintln!(
        "live_search_stations: {} station(s) for \"King's Cross\"",
        stations.len()
    );

    // Sanity-check the first result has a non-empty name and id.
    let first = &stations[0];
    assert!(!first.id.is_empty(), "station id should not be empty");
    assert!(
        !first.common_name.is_empty(),
        "station name should not be empty"
    );
}

/// Fetch line status for the Victoria line.
#[tokio::test]
async fn live_get_line_status_returns_a_status() {
    if !tfl_api_reachable().await {
        eprintln!("SKIP: api.tfl.gov.uk unreachable (no network)");
        return;
    }

    let http = ReqwestTflHttp::new();
    let client = TflClient::new(http);

    let status = client
        .get_line_status("victoria")
        .await
        .expect("get_line_status should not error for victoria");

    assert_eq!(status.line_id, "victoria");
    assert!(
        !status.status.is_empty(),
        "Expected at least one status entry for victoria"
    );

    eprintln!(
        "live_get_line_status(victoria): {:?}",
        status.status.first().map(|s| &s.description)
    );
}
