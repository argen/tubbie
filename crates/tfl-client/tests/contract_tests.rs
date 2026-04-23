//! Contract tests over the recorded TfL fixtures.
//!
//! Each test deserializes a fixture into typed domain structs, asserting that
//! the JSON shape TfL documents is stable and our types can parse it.
//! A deserialization failure here means either:
//!   (a) TfL changed their API schema — refresh fixtures + update types, or
//!   (b) A domain type is wrong — fix the type.

use std::path::PathBuf;
use tfl_client::fixture::FixtureTflHttp;
use tfl_client::http::TflHttp;
use tfl_domain::types::{Arrival, Station, TflLine};

fn fixtures_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../fixtures")
}

fn make_client() -> FixtureTflHttp {
    FixtureTflHttp::new(fixtures_dir())
}

// ---------------------------------------------------------------------------
// Arrivals contract: deserialize into typed Vec<Arrival>
// ---------------------------------------------------------------------------

async fn assert_arrivals_typed(id: &str) {
    let client = make_client();
    let value = client
        .fetch("arrivals", id)
        .await
        .unwrap_or_else(|e| panic!("fetch failed for arrivals/{id}: {e}"));

    let json_str = serde_json::to_string(&value).expect("re-serialise fixture to string");
    let arrivals: Vec<Arrival> = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("arrivals/{id}: typed deserialization failed: {e}"));

    // An empty array is technically valid (quiet station / off-peak) but log a warning.
    if arrivals.is_empty() {
        eprintln!("WARN: arrivals/{id} fixture is an empty array");
        return;
    }

    // Sanity: every arrival must have a non-empty id and non-negative time_to_station
    // is not guaranteed (some platforms show trains that have just left), so we
    // only assert the id field as the minimum contract.
    for (i, arrival) in arrivals.iter().enumerate() {
        assert!(
            !arrival.id.is_empty(),
            "arrivals/{id}[{i}]: id must be non-empty"
        );
        assert!(
            !arrival.line_id.is_empty(),
            "arrivals/{id}[{i}]: lineId must be non-empty"
        );
        assert!(
            !arrival.platform_name.is_empty(),
            "arrivals/{id}[{i}]: platformName must be non-empty"
        );
    }
}

#[tokio::test]
async fn contract_arrivals_belsize_park() {
    assert_arrivals_typed("940GZZLUBZP").await;
}

#[tokio::test]
async fn contract_arrivals_kings_cross() {
    assert_arrivals_typed("940GZZLUKSX").await;
}

#[tokio::test]
async fn contract_arrivals_bank() {
    assert_arrivals_typed("940GZZLUBNK").await;
}

#[tokio::test]
async fn contract_arrivals_oxford_circus() {
    assert_arrivals_typed("940GZZLUOXC").await;
}

// ---------------------------------------------------------------------------
// Line-status contract: deserialize into typed Vec<TflLine>
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contract_line_status_tube() {
    let client = make_client();
    let value = client
        .fetch("line-status", "tube")
        .await
        .expect("line-status/tube fixture should exist");

    let json_str = serde_json::to_string(&value).expect("re-serialise");
    let lines: Vec<TflLine> = serde_json::from_str(&json_str)
        .expect("line-status/tube: typed TflLine deserialization failed");

    assert!(
        !lines.is_empty(),
        "line-status/tube: must contain at least one line"
    );

    for (i, line) in lines.iter().enumerate() {
        assert!(
            !line.id.is_empty(),
            "line-status/tube[{i}]: id must be non-empty"
        );
        assert!(
            !line.name.is_empty(),
            "line-status/tube[{i}]: name must be non-empty"
        );
        assert!(
            !line.line_statuses.is_empty(),
            "line-status/tube[{i}] ({:?}): lineStatuses must not be empty",
            line.name
        );
    }
}

// ---------------------------------------------------------------------------
// Stop-points contract: deserialize each stopPoint into typed Station
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contract_stop_points_tube() {
    let client = make_client();
    let value = client
        .fetch("stop-points", "tube")
        .await
        .expect("stop-points/tube fixture should exist");

    // TfL's /StopPoint/Mode/{mode} returns a paginated envelope:
    // { "$type": "...", "total": N, "stopPoints": [...] }
    let obj = value
        .as_object()
        .expect("stop-points/tube: expected top-level JSON object");
    let stop_points_value = obj
        .get("stopPoints")
        .expect("stop-points/tube: missing `stopPoints` key");

    let json_str = serde_json::to_string(stop_points_value).expect("re-serialise");
    let stations: Vec<Station> = serde_json::from_str(&json_str)
        .expect("stop-points/tube: typed Station deserialization failed");

    assert!(
        !stations.is_empty(),
        "stop-points/tube: stopPoints must not be empty"
    );

    for (i, station) in stations.iter().enumerate() {
        assert!(
            !station.id.is_empty(),
            "stop-points/tube[{i}]: id must be non-empty"
        );
        assert!(
            !station.common_name.is_empty(),
            "stop-points/tube[{i}]: commonName must be non-empty"
        );
    }
}
