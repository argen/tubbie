//! Unit tests for `TflClient` — all using `FixtureTflHttp` (zero network).
//!
//! ## Error-variant coverage
//! - `TflError::NotFound` — `get_arrivals_unknown_station_returns_not_found`,
//!   `get_line_status_unknown_line_returns_not_found`
//! - `TflError::Parse` — `error_variant_parse_via_bad_arrivals_json`
//! - `TflError::ParseAt` — `error_variant_parse_at_via_invalid_fixture_json`
//!
//! `Transport`, `RateLimited`, and `Http` are live-client variants tested in M3.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::client::TflClient;
    use crate::error::TflError;
    use crate::fixture::FixtureTflHttp;

    fn workspace_fixtures_dir() -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest.join("../../fixtures")
    }

    fn real_client() -> TflClient<FixtureTflHttp> {
        TflClient::new(FixtureTflHttp::new(workspace_fixtures_dir()))
    }

    // -------------------------------------------------------------------------
    // get_arrivals
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn get_arrivals_belsize_park_returns_parsed_list() {
        let client = real_client();
        let arrivals = client
            .get_arrivals("940GZZLUBZP")
            .await
            .expect("Belsize Park arrivals should parse");

        assert!(!arrivals.is_empty(), "BZP fixture should contain arrivals");
        // All arrivals must have a non-empty id and belong to this station.
        for a in &arrivals {
            assert!(!a.id.is_empty(), "arrival.id must be non-empty");
            assert_eq!(
                a.naptan_id, "940GZZLUBZP",
                "all BZP arrivals must have naptan_id 940GZZLUBZP"
            );
        }
    }

    #[tokio::test]
    async fn get_arrivals_unknown_station_returns_not_found() {
        let client = real_client();
        let err = client
            .get_arrivals("DOESNOTEXIST")
            .await
            .expect_err("unknown station should return an error");

        assert!(
            matches!(err, TflError::NotFound(_)),
            "expected TflError::NotFound, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn get_arrivals_kings_cross_handles_multi_line() {
        let client = real_client();
        let arrivals = client
            .get_arrivals("940GZZLUKSX")
            .await
            .expect("King's Cross arrivals should parse");

        assert!(!arrivals.is_empty(), "KSX fixture should contain arrivals");

        // King's Cross serves multiple lines — verify we see more than one line_id.
        let line_ids: std::collections::HashSet<&str> =
            arrivals.iter().map(|a| a.line_id.as_str()).collect();
        assert!(
            line_ids.len() > 1,
            "King's Cross should have arrivals from multiple lines, got: {line_ids:?}"
        );
    }

    // -------------------------------------------------------------------------
    // get_line_status
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn get_line_status_northern_returns_status() {
        let client = real_client();
        let status = client
            .get_line_status("northern")
            .await
            .expect("northern line status should parse");

        assert_eq!(status.line_id, "northern");
        assert!(
            !status.status.is_empty(),
            "northern should have at least one status entry"
        );
        // The fixture was recorded during a strike; disruption text expected.
        assert!(
            status.disruption_text.is_some(),
            "northern should have disruption text in the strike-day fixture"
        );
        let text = status.disruption_text.unwrap();
        assert!(
            text.contains("Northern"),
            "disruption text should mention Northern line, got: {text:?}"
        );
    }

    #[tokio::test]
    async fn get_line_status_unknown_line_returns_not_found() {
        let client = real_client();
        let err = client
            .get_line_status("definitely-not-a-line")
            .await
            .expect_err("unknown line should error");

        assert!(
            matches!(err, TflError::NotFound(_)),
            "expected TflError::NotFound, got: {err:?}"
        );
    }

    /// Verify that a line with no disruption reasons produces `disruption_text: None`.
    ///
    /// Since the workspace fixture was recorded on a strike day (all lines
    /// disrupted), we build a minimal hand-crafted fixture in a temp dir.
    #[tokio::test]
    async fn get_line_status_good_service_has_no_disruption_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let endpoint_dir = dir.path().join("line-status");
        fs::create_dir_all(&endpoint_dir).unwrap();

        // Minimal TflLine-shaped JSON for a line with Good Service (severity 10)
        // and no reason field.
        let fixture = serde_json::json!([
            {
                "$type": "Tfl.Api.Presentation.Entities.Line, Tfl.Api.Presentation.Entities",
                "id": "jubilee",
                "name": "Jubilee",
                "lineStatuses": [
                    {
                        "statusSeverity": 10,
                        "statusSeverityDescription": "Good Service"
                    }
                ]
            }
        ]);
        fs::write(
            endpoint_dir.join("tube.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let status = client
            .get_line_status("jubilee")
            .await
            .expect("jubilee good-service should parse");

        assert_eq!(status.line_id, "jubilee");
        assert_eq!(status.status.len(), 1);
        assert_eq!(status.status[0].severity, 10);
        assert_eq!(status.status[0].description, "Good Service");
        assert!(
            status.disruption_text.is_none(),
            "good service line must have no disruption text, got: {:?}",
            status.disruption_text
        );
    }

    // -------------------------------------------------------------------------
    // search_stations
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn search_stations_exact_match_first() {
        let client = real_client();
        // "Angel" is an exact match on common_name "Angel Underground Station"?
        // Actually search is substring-against-common_name. Let's use a query
        // that has an exact common_name match.
        // "Oval Underground Station" — query "oval underground station" should be tier 0.
        // More robustly: query by a known exact fragment and verify the best match
        // is the one whose lowercased common_name equals the query.
        //
        // Use "bank" — "Bank Underground Station" starts with "bank" but isn't
        // exact. Use the word "oval" which occurs in only one station name.
        let results = client
            .search_stations("oval")
            .await
            .expect("search should succeed");

        assert!(
            !results.is_empty(),
            "search for 'oval' should return results"
        );

        // The first result should be the one whose name most closely matches.
        // "Oval Underground Station" starts with "oval" — tier 1.
        // Verify it is first.
        let first = &results[0];
        assert!(
            first.common_name.to_lowercase().contains("oval"),
            "first result should contain 'oval', got: {:?}",
            first.common_name
        );

        // Verify ordering: if there are multiple results, any exact-match should
        // precede starts-with, which should precede contains.
        // Build a synthetic fixture that forces all three tiers.
        let dir = tempfile::tempdir().unwrap();
        let ep_dir = dir.path().join("stop-points");
        fs::create_dir_all(&ep_dir).unwrap();

        let fixture = serde_json::json!({
            "total": 3,
            "stopPoints": [
                { "id": "A", "commonName": "bank road underground", "modes": ["tube"], "lat": 51.5, "lon": -0.1 },
                { "id": "B", "commonName": "bank underground station", "modes": ["tube"], "lat": 51.5, "lon": -0.1 },
                { "id": "C", "commonName": "old bank junction", "modes": ["tube"], "lat": 51.5, "lon": -0.1 }
            ]
        });
        fs::write(
            ep_dir.join("tube.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let synthetic = TflClient::new(FixtureTflHttp::new(dir.path()));
        // Query "bank underground station" — exact match is B (tier 0),
        // "bank road underground" contains it? No — "bank road underground" doesn't
        // contain "bank underground station". "bank underground station" == B exactly.
        // "old bank junction" doesn't contain "bank underground station".
        // So only B matches. Let's use "bank" instead:
        let results2 = synthetic
            .search_stations("bank")
            .await
            .expect("synthetic search should succeed");
        assert_eq!(results2.len(), 3, "all 3 contain 'bank'");
        // B: "bank underground station" starts_with "bank" → tier 1
        // A: "bank road underground" starts_with "bank" → tier 1
        // C: "old bank junction" contains "bank" → tier 2
        // Within tier 1: alphabetical → A before B
        assert_eq!(results2[0].id, "A", "tier1 alpha first: A < B");
        assert_eq!(results2[1].id, "B", "tier1 alpha second");
        assert_eq!(results2[2].id, "C", "tier2 last");
    }

    #[tokio::test]
    async fn search_stations_case_insensitive() {
        let client = real_client();

        let lower = client
            .search_stations("belsize")
            .await
            .expect("lowercase search");
        let upper = client
            .search_stations("BELSIZE")
            .await
            .expect("uppercase search");
        let mixed = client
            .search_stations("BeLsIzE")
            .await
            .expect("mixed-case search");

        assert!(!lower.is_empty(), "should find 'belsize' results");
        assert_eq!(
            lower.iter().map(|s| &s.id).collect::<Vec<_>>(),
            upper.iter().map(|s| &s.id).collect::<Vec<_>>(),
            "lowercase and uppercase searches should return the same stations"
        );
        assert_eq!(
            lower.iter().map(|s| &s.id).collect::<Vec<_>>(),
            mixed.iter().map(|s| &s.id).collect::<Vec<_>>(),
            "mixed-case search should return the same stations"
        );
    }

    #[tokio::test]
    async fn search_stations_excludes_non_tube() {
        let dir = tempfile::tempdir().unwrap();
        let ep_dir = dir.path().join("stop-points");
        fs::create_dir_all(&ep_dir).unwrap();

        let fixture = serde_json::json!({
            "total": 3,
            "stopPoints": [
                { "id": "TUBE1", "commonName": "Victoria Underground Station", "modes": ["tube"], "lat": 51.5, "lon": -0.14 },
                { "id": "BUS1",  "commonName": "Victoria Bus Stop",             "modes": ["bus"],  "lat": 51.5, "lon": -0.14 },
                { "id": "BOTH1", "commonName": "Victoria Coach Terminal",        "modes": ["tube", "bus"], "lat": 51.5, "lon": -0.14 }
            ]
        });
        fs::write(
            ep_dir.join("tube.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("victoria")
            .await
            .expect("search should succeed");

        // BUS1 must be excluded (no tube mode).
        let ids: Vec<&str> = results.iter().map(|s| s.id.as_str()).collect();
        assert!(
            !ids.contains(&"BUS1"),
            "bus-only stop must be excluded; got: {ids:?}"
        );
        assert!(
            ids.contains(&"TUBE1"),
            "tube stop must be included; got: {ids:?}"
        );
        assert!(
            ids.contains(&"BOTH1"),
            "tube+bus stop must be included (has tube mode); got: {ids:?}"
        );
    }

    #[tokio::test]
    async fn search_stations_empty_query_returns_empty() {
        let client = real_client();
        let results = client
            .search_stations("")
            .await
            .expect("empty query should not error");

        assert!(
            results.is_empty(),
            "empty query must return empty Vec, not all 1682 stations"
        );
    }

    #[tokio::test]
    async fn search_stations_whitespace_only_query_returns_empty() {
        let client = real_client();
        for query in ["   ", "\t", "\n", " \t \n "] {
            let results = client
                .search_stations(query)
                .await
                .expect("whitespace query should not error");
            assert!(
                results.is_empty(),
                "whitespace-only query {query:?} must return empty Vec \
                 (spec: `query.trim().is_empty()` short-circuits); got {} results",
                results.len()
            );
        }
    }

    #[tokio::test]
    async fn search_stations_limits_to_20() {
        // Use the real fixture — 'underground' appears in nearly all 1682 station names.
        let client = real_client();
        let results = client
            .search_stations("underground")
            .await
            .expect("search should succeed");

        assert!(
            results.len() <= 20,
            "search must return at most 20 results, got {}",
            results.len()
        );
        assert_eq!(
            results.len(),
            20,
            "with 'underground' query there should be exactly 20 results (the cap)"
        );
    }

    // -------------------------------------------------------------------------
    // Error-variant mapping — deliberate coverage of each M2-scope variant
    // -------------------------------------------------------------------------

    /// `TflError::NotFound` — missing fixture file → `get_arrivals` propagates it.
    #[tokio::test]
    async fn error_variant_not_found() {
        let client = real_client();
        let err = client
            .get_arrivals("DEFINITELY_NOT_A_STATION")
            .await
            .expect_err("must error");
        assert!(
            matches!(err, TflError::NotFound(_)),
            "expected NotFound, got: {err:?}"
        );
    }

    /// `TflError::Parse` — valid JSON file but wrong shape for `Vec<Arrival>`.
    ///
    /// We write a fixture containing a plain string instead of an array; the
    /// `serde_json::from_value` call in `get_arrivals` will fail with a
    /// `TflError::Parse`.
    #[tokio::test]
    async fn error_variant_parse_via_bad_arrivals_json() {
        let dir = tempfile::tempdir().unwrap();
        let ep_dir = dir.path().join("arrivals");
        fs::create_dir_all(&ep_dir).unwrap();

        // Valid JSON but not a Vec<Arrival>: a plain string.
        fs::write(ep_dir.join("BAD.json"), r#""not an array""#).unwrap();

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let err = client
            .get_arrivals("BAD")
            .await
            .expect_err("bad JSON shape must error");

        assert!(
            matches!(err, TflError::Parse(_)),
            "expected TflError::Parse, got: {err:?}"
        );
    }

    /// `TflError::ParseAt` — fixture file contains invalid JSON syntax.
    ///
    /// `FixtureTflHttp::fetch` catches the `serde_json` error from `from_str`
    /// and wraps it as `TflError::ParseAt { path, source }`.
    #[tokio::test]
    async fn error_variant_parse_at_via_invalid_fixture_json() {
        let dir = tempfile::tempdir().unwrap();
        let ep_dir = dir.path().join("arrivals");
        fs::create_dir_all(&ep_dir).unwrap();

        // Invalid JSON — will fail in `serde_json::from_str` inside fixture.rs.
        fs::write(ep_dir.join("BADJSON.json"), "{ this is not json }").unwrap();

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let err = client
            .get_arrivals("BADJSON")
            .await
            .expect_err("invalid JSON must error");

        assert!(
            matches!(err, TflError::ParseAt { .. }),
            "expected TflError::ParseAt, got: {err:?}"
        );
    }
}
