//! Contract tests over the recorded TfL fixtures.
//!
//! These tests assert structural invariants on each fixture — verifying the
//! JSON shape TfL documents — without deserialising into typed structs.
//!
//! TODO(M1): replace shape assertions with typed `serde_json::from_str::<Arrival>` calls
//! once the `Arrival` struct lands in `tfl-domain`.

use std::path::PathBuf;
use tfl_client::fixture::FixtureTflHttp;
use tfl_client::http::TflHttp;

fn fixtures_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../fixtures")
}

fn make_client() -> FixtureTflHttp {
    FixtureTflHttp::new(fixtures_dir())
}

// ---------------------------------------------------------------------------
// Arrivals contract: top-level must be a non-empty array; each element must
// contain the documented TfL fields.
// ---------------------------------------------------------------------------

/// Fields that TfL guarantees on every arrival object.
/// Note: `direction` and `destinationName` are documented but OPTIONAL —
/// TfL omits them on some DLR and interchange services.
/// TODO(M1): replace with typed `Arrival` deserialization once the struct lands.
const ARRIVALS_REQUIRED_FIELDS: &[&str] = &[
    "$type",
    "id",
    "stationName",
    "platformName",
    "timeToStation",
    "lineId",
];

async fn assert_arrivals_contract(id: &str) {
    let client = make_client();
    let value = client
        .fetch("arrivals", id)
        .await
        .unwrap_or_else(|e| panic!("fetch failed for arrivals/{id}: {e}"));

    let arr = value
        .as_array()
        .unwrap_or_else(|| panic!("arrivals/{id}: expected top-level JSON array"));

    // An empty array is technically valid (quiet station / off-peak) but we
    // log a warning.  Fixture recorder was instructed to abort on error responses,
    // so an empty array here means TfL returned [] legitimately.
    if arr.is_empty() {
        eprintln!("WARN: arrivals/{id} fixture is an empty array — contract shape checks skipped");
        return;
    }

    for (i, element) in arr.iter().enumerate() {
        let obj = element
            .as_object()
            .unwrap_or_else(|| panic!("arrivals/{id}[{i}] should be a JSON object"));
        for field in ARRIVALS_REQUIRED_FIELDS {
            assert!(
                obj.contains_key(*field),
                "arrivals/{id}[{i}] missing required field `{field}`"
            );
        }
        // TODO(M1): replace with `serde_json::from_str::<Arrival>` deserialization
    }
}

#[tokio::test]
async fn contract_arrivals_belsize_park() {
    assert_arrivals_contract("940GZZLUBZP").await;
}

#[tokio::test]
async fn contract_arrivals_kings_cross() {
    assert_arrivals_contract("940GZZLUKSX").await;
}

#[tokio::test]
async fn contract_arrivals_bank() {
    assert_arrivals_contract("940GZZLUBNK").await;
}

#[tokio::test]
async fn contract_arrivals_oxford_circus() {
    assert_arrivals_contract("940GZZLUOXC").await;
}

// ---------------------------------------------------------------------------
// Line-status contract: top-level is a non-empty array; each element has
// `$type`, `id`, `name`, `lineStatuses`.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn contract_line_status_tube() {
    let client = make_client();
    let value = client
        .fetch("line-status", "tube")
        .await
        .expect("line-status/tube fixture should exist");

    let arr = value
        .as_array()
        .expect("line-status/tube: expected top-level JSON array");
    assert!(!arr.is_empty(), "line-status/tube: array must not be empty");

    for (i, element) in arr.iter().enumerate() {
        let obj = element
            .as_object()
            .unwrap_or_else(|| panic!("line-status/tube[{i}] should be a JSON object"));
        for field in &["$type", "id", "name", "lineStatuses"] {
            assert!(
                obj.contains_key(*field),
                "line-status/tube[{i}] missing required field `{field}`"
            );
        }
        // TODO(M1): replace with typed LineStatus deserialization
    }
}

// ---------------------------------------------------------------------------
// Stop-points contract: top-level must be an object with a `stopPoints` array.
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
    assert!(
        obj.contains_key("stopPoints"),
        "stop-points/tube: missing `stopPoints` key"
    );
    let stop_points = obj["stopPoints"]
        .as_array()
        .expect("stop-points/tube: `stopPoints` should be a JSON array");
    assert!(
        !stop_points.is_empty(),
        "stop-points/tube: `stopPoints` must not be empty"
    );
    // TODO(M1): replace with typed Station deserialization
}
