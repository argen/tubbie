//! Integration tests for `affected_segments` — driven through the REAL
//! `tfl_line_to_line_status` mapping via the public `get_line_status` path
//! (`FixtureTflHttp`, zero network). These do NOT re-implement the dedup
//! logic: reverting `build_affected_segments` in `cache.rs` turns them RED.

use std::fs;

use tfl_cache::TflClient;
use tfl_client::fixture::FixtureTflHttp;

/// Write a single-mode (tube) `line-status` fixture and return a client scoped
/// to the tube mode only, so no other per-mode fixtures are required. The
/// `TempDir` is returned so the caller keeps it alive for the test's duration.
fn tube_client(fixture: serde_json::Value) -> (tempfile::TempDir, TflClient<FixtureTflHttp>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let endpoint_dir = dir.path().join("line-status");
    fs::create_dir_all(&endpoint_dir).unwrap();
    fs::write(
        endpoint_dir.join("tube.json"),
        serde_json::to_string(&fixture).unwrap(),
    )
    .unwrap();
    let client = TflClient::with_modes(FixtureTflHttp::new(dir.path()), &["tube"]);
    (dir, client)
}

/// Two `affectedRoutes` forming a reverse-duplicate pair (A→B and B→A) must
/// collapse to ONE segment, keeping the first-seen orientation. Asserts on the
/// domain `LineStatus` produced by the real conversion.
#[tokio::test]
async fn affected_segments_dedupes_reverse_pair() {
    let (_dir, client) = tube_client(serde_json::json!([
        { "id": "metropolitan", "name": "Metropolitan", "lineStatuses": [
            {
                "statusSeverity": 6,
                "statusSeverityDescription": "Severe Delays",
                "reason": "Met Line: severe delays",
                "disruption": {
                    "description": "Severe delays on the Metropolitan line.",
                    "affectedRoutes": [
                        { "name": "Watford - Aldgate", "originationName": "Harrow-on-the-Hill", "destinationName": "Watford" },
                        { "name": "Watford - Harrow", "originationName": "Watford", "destinationName": "Harrow-on-the-Hill" }
                    ]
                }
            }
        ]}
    ]));

    let status = client
        .get_line_status("metropolitan")
        .await
        .expect("metropolitan line status should parse");

    let segments = &status.status[0].affected_segments;
    assert_eq!(
        segments.len(),
        1,
        "reverse-duplicate pair must collapse to one segment, got: {segments:?}"
    );
    assert_eq!(segments[0].from, "Harrow-on-the-Hill");
    assert_eq!(segments[0].to, "Watford");
}

/// A Good-Service line (no `disruption`) must yield an empty `affected_segments`.
#[tokio::test]
async fn affected_segments_empty_for_good_service() {
    let (_dir, client) = tube_client(serde_json::json!([
        { "id": "northern", "name": "Northern", "lineStatuses": [
            { "statusSeverity": 10, "statusSeverityDescription": "Good Service" }
        ]}
    ]));

    let status = client
        .get_line_status("northern")
        .await
        .expect("northern good-service should parse");

    assert!(
        status.status[0].affected_segments.is_empty(),
        "good service must have no affected segments"
    );
}

/// Routes missing an origination or destination name must be skipped
/// (fail-open guard) — only the fully-named pair survives.
#[tokio::test]
async fn affected_segments_skips_routes_with_empty_names() {
    let (_dir, client) = tube_client(serde_json::json!([
        { "id": "jubilee", "name": "Jubilee", "lineStatuses": [
            {
                "statusSeverity": 9,
                "statusSeverityDescription": "Minor Delays",
                "disruption": {
                    "description": "Minor delays.",
                    "affectedRoutes": [
                        { "name": "no names", "originationName": "", "destinationName": "" },
                        { "name": "only from", "originationName": "Stratford", "destinationName": "" },
                        { "name": "valid pair", "originationName": "Wembley Park", "destinationName": "Stanmore" }
                    ]
                }
            }
        ]}
    ]));

    let status = client
        .get_line_status("jubilee")
        .await
        .expect("jubilee line status should parse");

    let segments = &status.status[0].affected_segments;
    assert_eq!(segments.len(), 1, "only the fully-named route survives");
    assert_eq!(segments[0].from, "Wembley Park");
    assert_eq!(segments[0].to, "Stanmore");
}
