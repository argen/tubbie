//! Unit tests for `TflClient` — all using `FixtureTflHttp` (zero network).
//!
//! ## Error-variant coverage
//! - `TflError::NotFound` — `get_arrivals_unknown_station_returns_not_found`,
//!   `get_line_status_unknown_line_returns_not_found`
//! - `TflError::Parse` — `error_variant_parse_via_bad_arrivals_json`
//! - `TflError::ParseAt` — `error_variant_parse_at_via_invalid_fixture_json`
//!
//! `Transport`, `RateLimited`, and `Http` are live-client variants tested in the
//! `http_retry.rs` integration test in `tfl-client`.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::cache::TflClient;
    use tfl_client::error::TflError;
    use tfl_client::fixture::FixtureTflHttp;
    use tfl_client::http::TflHttp;
    use tfl_domain::types::Station;

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
    async fn get_line_status_overground_returns_status() {
        let client = real_client();
        let status = client
            .get_line_status("mildmay")
            .await
            .expect("mildmay line status should parse from line-status/overground fixture");

        assert_eq!(status.line_id, "mildmay");
        assert!(
            !status.status.is_empty(),
            "mildmay should have at least one status entry from the recorded fixture"
        );
    }

    #[tokio::test]
    async fn get_line_status_dlr_returns_status() {
        let client = real_client();
        let status = client
            .get_line_status("dlr")
            .await
            .expect("dlr line status should parse from line-status/dlr fixture");

        assert_eq!(status.line_id, "dlr");
    }

    #[tokio::test]
    async fn get_line_status_elizabeth_returns_status() {
        let client = real_client();
        let status = client
            .get_line_status("elizabeth")
            .await
            .expect("elizabeth line status should parse from line-status/elizabeth-line fixture");

        assert_eq!(status.line_id, "elizabeth");
    }

    #[tokio::test]
    async fn client_with_subset_modes_only_fetches_those_modes() {
        let full = real_client();
        full.get_line_status("mildmay")
            .await
            .expect("default client should resolve overground line via overground fixture");

        let tube_only = TflClient::with_modes(
            FixtureTflHttp::new(workspace_fixtures_dir()),
            &["tube"],
        );
        let err = tube_only
            .get_line_status("mildmay")
            .await
            .expect_err("subset client must NOT find overground line");
        assert!(
            matches!(err, TflError::NotFound(_)),
            "expected NotFound, got: {err:?}"
        );

        tube_only
            .get_line_status("northern")
            .await
            .expect("subset client should still resolve tube lines");
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

    #[tokio::test]
    async fn get_line_status_good_service_has_no_disruption_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let endpoint_dir = dir.path().join("line-status");
        fs::create_dir_all(&endpoint_dir).unwrap();

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
    // get_all_line_statuses
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn get_all_line_statuses_returns_lines_from_every_supported_mode() {
        let client = real_client();
        let statuses = client
            .get_all_line_statuses()
            .await
            .expect("workspace fixture must yield a populated line list");

        let ids: std::collections::HashSet<&str> =
            statuses.iter().map(|s| s.line_id.as_str()).collect();

        assert!(ids.contains("northern"), "expected tube line 'northern', got: {ids:?}");
        assert!(ids.contains("dlr"), "expected DLR line 'dlr', got: {ids:?}");
        assert!(ids.contains("elizabeth"), "expected Elizabeth line 'elizabeth', got: {ids:?}");
        assert!(ids.contains("mildmay"), "expected Overground line 'mildmay', got: {ids:?}");
    }

    #[tokio::test]
    async fn get_all_line_statuses_sorts_worst_first_then_alphabetical() {
        use tfl_domain::types::SeverityBucket;

        let dir = tempfile::tempdir().expect("tempdir");
        let endpoint_dir = dir.path().join("line-status");
        fs::create_dir_all(&endpoint_dir).unwrap();
        let fixture = serde_json::json!([
            { "id": "echo-line", "name": "Echo", "lineStatuses": [
                { "statusSeverity": 10, "statusSeverityDescription": "Good Service" }
            ]},
            { "id": "delta-line", "name": "Delta", "lineStatuses": [
                { "statusSeverity": 6, "statusSeverityDescription": "Severe Delays" }
            ]},
            { "id": "charlie-line", "name": "Charlie", "lineStatuses": [
                { "statusSeverity": 9, "statusSeverityDescription": "Minor Delays" }
            ]},
            { "id": "bravo-line", "name": "Bravo", "lineStatuses": [
                { "statusSeverity": 6, "statusSeverityDescription": "Severe Delays" }
            ]},
            { "id": "alpha-line", "name": "Alpha", "lineStatuses": [
                { "statusSeverity": 1, "statusSeverityDescription": "Closed" }
            ]},
        ]);
        fs::write(
            endpoint_dir.join("tube.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let client = TflClient::with_modes(FixtureTflHttp::new(dir.path()), &["tube"]);
        let statuses = client
            .get_all_line_statuses()
            .await
            .expect("get_all_line_statuses should succeed with tube fixture");

        let order: Vec<&str> = statuses.iter().map(|s| s.line_id.as_str()).collect();
        assert_eq!(
            order,
            vec!["alpha-line", "bravo-line", "delta-line", "charlie-line", "echo-line"],
            "expected worst-first then alphabetical by line_id"
        );

        for s in &statuses {
            for entry in &s.status {
                assert_ne!(
                    entry.bucket,
                    SeverityBucket::Other,
                    "fixture-known severities must NOT bucket as Other; line={}",
                    s.line_id,
                );
            }
        }
    }

    #[tokio::test]
    async fn get_all_line_statuses_warm_cache_does_not_refetch() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        struct CountingTflHttp {
            inner: FixtureTflHttp,
            line_status_calls: Arc<AtomicUsize>,
        }
        impl TflHttp for CountingTflHttp {
            fn fetch(
                &self,
                endpoint: &str,
                id: &str,
            ) -> impl std::future::Future<Output = Result<serde_json::Value, TflError>> + Send
            {
                if endpoint == "line-status" {
                    self.line_status_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                let inner = self.inner.clone();
                let endpoint = endpoint.to_string();
                let id = id.to_string();
                async move { inner.fetch(&endpoint, &id).await }
            }
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let http = CountingTflHttp {
            inner: FixtureTflHttp::new(workspace_fixtures_dir()),
            line_status_calls: counter.clone(),
        };
        let client = TflClient::with_modes(http, &["tube"]);

        client.get_all_line_statuses().await.expect("first call should succeed");
        let after_cold = counter.load(Ordering::SeqCst);
        assert_eq!(after_cold, 1, "cold-cache call should fetch tube exactly once");

        client.get_all_line_statuses().await.expect("warm-cache call should succeed");
        let after_warm = counter.load(Ordering::SeqCst);
        assert_eq!(
            after_warm, 1,
            "warm-cache call MUST NOT refetch; expected 1 call total, got {after_warm}",
        );
    }

    #[tokio::test]
    async fn get_all_line_statuses_populates_validity_periods_when_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let endpoint_dir = dir.path().join("line-status");
        fs::create_dir_all(&endpoint_dir).unwrap();
        let fixture = serde_json::json!([
            {
                "id": "liberty",
                "name": "Liberty",
                "lineStatuses": [
                    {
                        "statusSeverity": 4,
                        "statusSeverityDescription": "Planned Closure",
                        "reason": "Liberty: planned closure for engineering work.",
                        "validityPeriods": [
                            {
                                "fromDate": "2026-05-04T22:00:00Z",
                                "toDate": "2026-05-05T04:30:00Z",
                                "isNow": true
                            }
                        ]
                    }
                ]
            }
        ]);
        fs::write(
            endpoint_dir.join("overground.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let client = TflClient::with_modes(FixtureTflHttp::new(dir.path()), &["overground"]);
        let statuses = client.get_all_line_statuses().await.expect("should succeed");

        let liberty = statuses
            .iter()
            .find(|s| s.line_id == "liberty")
            .expect("liberty should be present");

        assert_eq!(liberty.validity_periods.len(), 1);
        let vp = &liberty.validity_periods[0];
        assert!(vp.is_now, "is_now must round-trip from `isNow`");
        assert_eq!(
            vp.from.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-05-04T22:00:00Z",
        );
        assert_eq!(
            vp.to.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-05-05T04:30:00Z",
        );
    }

    #[tokio::test]
    async fn get_all_line_statuses_empty_validity_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let endpoint_dir = dir.path().join("line-status");
        fs::create_dir_all(&endpoint_dir).unwrap();
        let fixture = serde_json::json!([
            {
                "id": "jubilee",
                "name": "Jubilee",
                "lineStatuses": [
                    { "statusSeverity": 10, "statusSeverityDescription": "Good Service" }
                ]
            }
        ]);
        fs::write(
            endpoint_dir.join("tube.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let client = TflClient::with_modes(FixtureTflHttp::new(dir.path()), &["tube"]);
        let statuses = client.get_all_line_statuses().await.expect("should succeed");
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].validity_periods.is_empty());
    }

    #[tokio::test]
    async fn get_all_line_statuses_all_modes_failed_returns_err() {
        let dir = tempfile::tempdir().expect("tempdir");
        let client = TflClient::with_modes(FixtureTflHttp::new(dir.path()), &["tube"]);
        let err = client
            .get_all_line_statuses()
            .await
            .expect_err("all-modes-failed must propagate as error");
        assert!(
            matches!(err, TflError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );
    }

    // -------------------------------------------------------------------------
    // search_stations
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn search_stations_exact_match_first() {
        let client = real_client();
        let results = client
            .search_stations("oval")
            .await
            .expect("search should succeed");

        assert!(!results.is_empty(), "search for 'oval' should return results");
        let first = &results[0];
        assert!(
            first.common_name.to_lowercase().contains("oval"),
            "first result should contain 'oval', got: {:?}",
            first.common_name
        );

        let dir = tempfile::tempdir().unwrap();
        let ep_dir = dir.path().join("stop-points");
        fs::create_dir_all(&ep_dir).unwrap();

        let fixture = serde_json::json!({
            "total": 3,
            "stopPoints": [
                { "id": "940GZZLUAAA", "commonName": "bank road underground", "modes": ["tube"], "lat": 51.5, "lon": -0.1 },
                { "id": "940GZZLUBBB", "commonName": "bank underground station", "modes": ["tube"], "lat": 51.5, "lon": -0.1 },
                { "id": "940GZZLUCCC", "commonName": "old bank junction", "modes": ["tube"], "lat": 51.5, "lon": -0.1 }
            ]
        });
        fs::write(
            ep_dir.join("tube.json"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let synthetic = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results2 = synthetic.search_stations("bank").await.expect("synthetic search should succeed");
        assert_eq!(results2.len(), 3, "all 3 contain 'bank'");
        assert_eq!(results2[0].id, "940GZZLUAAA");
        assert_eq!(results2[1].id, "940GZZLUBBB");
        assert_eq!(results2[2].id, "940GZZLUCCC");
    }

    #[tokio::test]
    async fn search_stations_case_insensitive() {
        let client = real_client();

        let lower = client.search_stations("belsize").await.expect("lowercase search");
        let upper = client.search_stations("BELSIZE").await.expect("uppercase search");
        let mixed = client.search_stations("BeLsIzE").await.expect("mixed-case search");

        assert!(!lower.is_empty(), "should find 'belsize' results");
        assert_eq!(
            lower.iter().map(|s| &s.id).collect::<Vec<_>>(),
            upper.iter().map(|s| &s.id).collect::<Vec<_>>(),
        );
        assert_eq!(
            lower.iter().map(|s| &s.id).collect::<Vec<_>>(),
            mixed.iter().map(|s| &s.id).collect::<Vec<_>>(),
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
                { "id": "940GZZLUVIC", "commonName": "Victoria Underground Station", "modes": ["tube"], "lat": 51.5, "lon": -0.14 },
                { "id": "490BUSVIC",   "commonName": "Victoria Bus Stop",             "modes": ["bus"],  "lat": 51.5, "lon": -0.14 },
                { "id": "940GZZLUVCX", "commonName": "Victoria Coach Terminal",       "modes": ["tube", "bus"], "lat": 51.5, "lon": -0.14 }
            ]
        });
        fs::write(ep_dir.join("tube.json"), serde_json::to_string(&fixture).unwrap()).unwrap();

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("victoria").await.expect("search should succeed");

        let ids: Vec<&str> = results.iter().map(|s| s.id.as_str()).collect();
        assert!(!ids.contains(&"490BUSVIC"), "bus-only stop must be excluded; got: {ids:?}");
        assert!(ids.contains(&"940GZZLUVIC"), "tube stop must be included; got: {ids:?}");
        assert!(ids.contains(&"940GZZLUVCX"), "tube+bus stop must be included; got: {ids:?}");
    }

    #[tokio::test]
    async fn search_stations_empty_query_returns_empty() {
        let client = real_client();
        let results = client.search_stations("").await.expect("empty query should not error");
        assert!(results.is_empty(), "empty query must return empty Vec");
    }

    #[tokio::test]
    async fn search_stations_whitespace_only_query_returns_empty() {
        let client = real_client();
        for query in ["   ", "\t", "\n", " \t \n "] {
            let results = client.search_stations(query).await.expect("whitespace query should not error");
            assert!(results.is_empty(), "whitespace-only query {query:?} must return empty Vec");
        }
    }

    #[tokio::test]
    async fn search_stations_limits_to_20() {
        let client = real_client();
        let results = client.search_stations("underground").await.expect("search should succeed");
        assert!(results.len() <= 20, "search must return at most 20 results, got {}", results.len());
        assert_eq!(results.len(), 20, "with 'underground' query there should be exactly 20 results");
    }

    // -------------------------------------------------------------------------
    // Station.lines projection from TfL `lineModeGroups`
    // -------------------------------------------------------------------------

    fn write_stop_points_fixture(dir: &std::path::Path, body: serde_json::Value) {
        let ep_dir = dir.join("stop-points");
        std::fs::create_dir_all(&ep_dir).unwrap();
        std::fs::write(ep_dir.join("tube.json"), serde_json::to_string(&body).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn search_stations_populates_lines_from_line_mode_groups() {
        let dir = tempfile::tempdir().unwrap();
        write_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUOXC",
                    "commonName": "Oxford Circus Underground Station",
                    "modes": ["tube"],
                    "lat": 51.515,
                    "lon": -0.1418,
                    "lineModeGroups": [
                        { "modeName": "tube", "lineIdentifier": ["bakerloo", "central", "victoria"] }
                    ]
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("oxford").await.expect("search should succeed");

        assert_eq!(results.len(), 1);
        let oxc = &results[0];
        let line_ids: Vec<&str> = oxc.lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(line_ids, vec!["bakerloo", "central", "victoria"]);

        let names: Vec<&str> = oxc.lines.iter().map(|l| l.name.as_str()).collect();
        assert!(
            names.contains(&"Bakerloo") && names.contains(&"Central") && names.contains(&"Victoria"),
            "line names should be pretty-printed, got {names:?}"
        );
    }

    #[tokio::test]
    async fn search_stations_ignores_non_tube_line_mode_groups() {
        let dir = tempfile::tempdir().unwrap();
        write_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUMXD",
                    "commonName": "Mixed Bus And Tube",
                    "modes": ["tube", "bus"],
                    "lat": 51.5, "lon": -0.1,
                    "lineModeGroups": [
                        { "modeName": "tube", "lineIdentifier": ["northern"] },
                        { "modeName": "bus",  "lineIdentifier": ["24", "29"] }
                    ]
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("mixed").await.expect("search should succeed");

        assert_eq!(results.len(), 1);
        let line_ids: Vec<&str> = results[0].lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(line_ids, vec!["northern"]);
    }

    #[tokio::test]
    async fn search_stations_empty_lines_when_no_line_mode_groups() {
        let dir = tempfile::tempdir().unwrap();
        write_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUBRE",
                    "commonName": "Bare Station Underground Station",
                    "modes": ["tube"],
                    "lat": 51.5, "lon": -0.1,
                    "lineModeGroups": []
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("bare").await.expect("search should succeed");

        assert_eq!(results.len(), 1);
        assert!(results[0].lines.is_empty());
    }

    #[tokio::test]
    async fn search_stations_accepts_trimmed_line_mode_groups_without_mode_name() {
        let dir = tempfile::tempdir().unwrap();
        write_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUTRM",
                    "commonName": "Trimmed Fixture Station",
                    "modes": ["tube"],
                    "lat": 51.5, "lon": -0.1,
                    "lineModeGroups": [ { "lineIdentifier": ["jubilee", "metropolitan"] } ]
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("trimmed").await.expect("search should succeed");

        let line_ids: Vec<&str> = results[0].lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(line_ids, vec!["jubilee", "metropolitan"]);
    }

    #[tokio::test]
    async fn search_stations_prefers_explicit_lines_field_over_line_mode_groups() {
        let dir = tempfile::tempdir().unwrap();
        write_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUEXP",
                    "commonName": "Explicit Lines Underground",
                    "modes": ["tube"],
                    "lat": 51.5, "lon": -0.1,
                    "lines": [ { "id": "jubilee", "name": "Jubilee" } ],
                    "lineModeGroups": [
                        { "modeName": "tube", "lineIdentifier": ["northern", "central"] }
                    ]
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("explicit").await.expect("search should succeed");

        let line_ids: Vec<&str> = results[0].lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(line_ids, vec!["jubilee"]);
    }

    // -------------------------------------------------------------------------
    // Error-variant mapping
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn error_variant_not_found() {
        let client = real_client();
        let err = client.get_arrivals("DEFINITELY_NOT_A_STATION").await.expect_err("must error");
        assert!(matches!(err, TflError::NotFound(_)), "expected NotFound, got: {err:?}");
    }

    #[tokio::test]
    async fn error_variant_parse_via_bad_arrivals_json() {
        let dir = tempfile::tempdir().unwrap();
        let ep_dir = dir.path().join("arrivals");
        fs::create_dir_all(&ep_dir).unwrap();
        fs::write(ep_dir.join("BAD.json"), r#""not an array""#).unwrap();

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let err = client.get_arrivals("BAD").await.expect_err("bad JSON shape must error");

        assert!(matches!(err, TflError::Parse(_)), "expected TflError::Parse, got: {err:?}");
    }

    #[tokio::test]
    async fn error_variant_parse_at_via_invalid_fixture_json() {
        let dir = tempfile::tempdir().unwrap();
        let ep_dir = dir.path().join("arrivals");
        fs::create_dir_all(&ep_dir).unwrap();
        fs::write(ep_dir.join("BADJSON.json"), "{ this is not json }").unwrap();

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let err = client.get_arrivals("BADJSON").await.expect_err("invalid JSON must error");

        assert!(matches!(err, TflError::ParseAt { .. }), "expected TflError::ParseAt, got: {err:?}");
    }

    // -------------------------------------------------------------------------
    // stop-points cache
    // -------------------------------------------------------------------------

    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const SUPPORTED_MODES_COUNT: usize = crate::cache::SUPPORTED_MODES.len();

    struct CountingTflHttp<H: TflHttp> {
        inner: H,
        count: Arc<AtomicUsize>,
    }

    impl<H: TflHttp> CountingTflHttp<H> {
        fn new(inner: H) -> (Self, Arc<AtomicUsize>) {
            let count = Arc::new(AtomicUsize::new(0));
            (Self { inner, count: count.clone() }, count)
        }
    }

    impl<H: TflHttp> TflHttp for CountingTflHttp<H> {
        fn fetch(
            &self,
            endpoint: &str,
            id: &str,
        ) -> impl std::future::Future<Output = Result<Value, TflError>> + Send {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.inner.fetch(endpoint, id)
        }
    }

    type RecordedCalls = Arc<std::sync::Mutex<Vec<(String, String)>>>;

    struct RecordingTflHttp<H: TflHttp> {
        inner: H,
        calls: RecordedCalls,
    }

    impl<H: TflHttp> RecordingTflHttp<H> {
        fn new(inner: H) -> (Self, RecordedCalls) {
            let calls: RecordedCalls = Arc::new(std::sync::Mutex::new(Vec::new()));
            (Self { inner, calls: calls.clone() }, calls)
        }
    }

    impl<H: TflHttp> TflHttp for RecordingTflHttp<H> {
        fn fetch(
            &self,
            endpoint: &str,
            id: &str,
        ) -> impl std::future::Future<Output = Result<Value, TflError>> + Send {
            self.calls.lock().unwrap().push((endpoint.to_string(), id.to_string()));
            self.inner.fetch(endpoint, id)
        }
    }

    #[tokio::test]
    async fn warm_stop_points_dedupes_hub_fetches_before_fan_out() {
        let (http, calls) = RecordingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client.warm_stop_points_cache().await.expect("warm should succeed");

        let recorded = calls.lock().unwrap().clone();
        let hub_calls: Vec<&str> = recorded
            .iter()
            .filter(|(ep, _)| ep == "stop-point")
            .map(|(_, id)| id.as_str())
            .collect();
        let mut unique = hub_calls.clone();
        unique.sort();
        unique.dedup();

        assert_eq!(
            hub_calls.len(),
            unique.len(),
            "hub fan-out must be deduped: {} total stop-point fetches but only {} unique hub ids — {} duplicates",
            hub_calls.len(), unique.len(), hub_calls.len() - unique.len(),
        );
    }

    #[tokio::test]
    async fn search_stations_uses_cached_stations_on_second_call() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        let first = client.search_stations("belsize").await.unwrap();
        let after_first = count.load(Ordering::SeqCst);
        let second = client.search_stations("king").await.unwrap();
        let after_second = count.load(Ordering::SeqCst);

        assert!(!first.is_empty() && !second.is_empty());
        assert_eq!(
            after_first, after_second,
            "second search_stations call must not trigger any additional fetches",
        );
    }

    #[tokio::test]
    async fn search_stations_refetches_after_cache_invalidated() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client.search_stations("belsize").await.unwrap();
        let after_warm = count.load(Ordering::SeqCst);

        client.invalidate_stop_points_cache();
        client.search_stations("belsize").await.unwrap();
        let after_refetch = count.load(Ordering::SeqCst);

        let delta = after_refetch - after_warm;
        assert!(
            (1..=SUPPORTED_MODES_COUNT).contains(&delta),
            "refetch after invalidation should re-fetch stop-points (1..={SUPPORTED_MODES_COUNT}); observed {delta} new fetches",
        );
    }

    #[tokio::test]
    async fn warm_stop_points_cache_populates_cache_for_zero_extra_fetches() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        let warmed = client.warm_stop_points_cache().await.unwrap();
        assert!(warmed > 100, "fixture should contain many tube stations");
        let after_warm = count.load(Ordering::SeqCst);

        let _ = client.search_stations("victoria").await.unwrap();
        let _ = client.search_stations("king").await.unwrap();
        let after_searches = count.load(Ordering::SeqCst);

        assert_eq!(after_warm, after_searches, "two searches after warm must hit the cache");
    }

    #[tokio::test]
    async fn search_stations_finds_victoria_in_full_fixture() {
        let client = real_client();
        let results = client.search_stations("victoria").await.expect("victoria search should succeed");

        assert!(!results.is_empty());

        for r in &results {
            let id = r.id.as_str();
            assert!(
                id.starts_with("940GZZLU") || id.starts_with("940GZZDL") || id.starts_with("910G"),
                "non-canonical id made it into results: {id:?} ({:?})",
                r.common_name
            );
        }

        let victoria_tube = results
            .iter()
            .find(|s| s.id == "940GZZLUVIC" && s.modes.iter().any(|m| m == "tube"))
            .expect("Victoria Underground Station should be in results");

        let line_ids: Vec<&str> = victoria_tube.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(line_ids.contains(&"victoria"), "expected victoria line, got {line_ids:?}");
        assert!(
            line_ids.contains(&"district") || line_ids.contains(&"circle"),
            "expected at least one of district/circle, got {line_ids:?}"
        );
    }

    #[tokio::test]
    async fn search_stations_victoria_second_call_hits_cache() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        let _ = client.search_stations("victoria").await.unwrap();
        let after_first = count.load(Ordering::SeqCst);
        let _ = client.search_stations("victoria").await.unwrap();
        let _ = client.search_stations("oxford").await.unwrap();
        let after_three = count.load(Ordering::SeqCst);

        assert_eq!(after_first, after_three, "two further searches must add zero fetches");
    }

    #[tokio::test]
    async fn warm_stop_points_cache_is_idempotent() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client.warm_stop_points_cache().await.unwrap();
        let after_first = count.load(Ordering::SeqCst);
        client.warm_stop_points_cache().await.unwrap();
        client.warm_stop_points_cache().await.unwrap();
        let after_three = count.load(Ordering::SeqCst);

        assert_eq!(after_first, after_three, "repeated warm calls within TTL must add zero fetches");
    }

    // -------------------------------------------------------------------------
    // Hub line merge
    // -------------------------------------------------------------------------

    fn write_hub_stop_points_fixture(dir: &std::path::Path, body: serde_json::Value) {
        write_stop_points_fixture(dir, body);
    }

    fn write_hub_fixture(dir: &std::path::Path, hub_id: &str, body: serde_json::Value) {
        let hub_dir = dir.join("stop-point");
        std::fs::create_dir_all(&hub_dir).unwrap();
        std::fs::write(
            hub_dir.join(format!("{hub_id}.json")),
            serde_json::to_string(&body).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn hub_lines_merged_into_station_lines_synthetic() {
        let dir = tempfile::tempdir().unwrap();
        write_hub_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUBNK",
                    "commonName": "Bank Underground Station",
                    "modes": ["tube"],
                    "lat": 51.51225, "lon": -0.087792,
                    "hubNaptanCode": "HUBBAN",
                    "lineModeGroups": [{"lineIdentifier": ["central", "waterloo-city", "northern"]}]
                }]
            }),
        );
        write_hub_fixture(
            dir.path(),
            "HUBBAN",
            serde_json::json!({
                "id": "HUBBAN",
                "children": [
                    {"id": "940GZZLUBNK", "modes": ["tube"],
                     "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["central","waterloo-city","northern"]}]},
                    {"id": "940GZZDLBNK", "modes": ["dlr"],
                     "lineModeGroups": [{"modeName": "dlr", "lineIdentifier": ["dlr"]}]}
                ]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("bank").await.expect("search should succeed");

        let bank = results.iter().find(|s| s.id == "940GZZLUBNK").expect("Bank must be in results");
        let line_ids: Vec<&str> = bank.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(line_ids.contains(&"central"), "tube lines must still be present, got {line_ids:?}");
        assert!(line_ids.contains(&"dlr"), "DLR from hub child must be merged in, got {line_ids:?}");
    }

    #[tokio::test]
    async fn hub_merge_deduplicates_lines_present_in_both_parent_and_hub() {
        let dir = tempfile::tempdir().unwrap();
        write_hub_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUBNK",
                    "commonName": "Bank Underground Station",
                    "modes": ["tube"],
                    "lat": 51.51225, "lon": -0.087792,
                    "hubNaptanCode": "HUBBAN",
                    "lineModeGroups": [{"lineIdentifier": ["central", "northern"]}]
                }]
            }),
        );
        write_hub_fixture(
            dir.path(),
            "HUBBAN",
            serde_json::json!({
                "id": "HUBBAN",
                "children": [
                    {"id": "940GZZLUBNK", "modes": ["tube"],
                     "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["central","northern"]}]},
                    {"id": "940GZZDLBNK", "modes": ["dlr"],
                     "lineModeGroups": [{"modeName": "dlr", "lineIdentifier": ["dlr","central"]}]}
                ]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("bank").await.expect("search should succeed");

        let bank = results.iter().find(|s| s.id == "940GZZLUBNK").unwrap();
        let central_count = bank.lines.iter().filter(|l| l.id == "central").count();
        assert_eq!(central_count, 1, "central must appear exactly once");
    }

    #[tokio::test]
    async fn hub_merge_missing_hub_fixture_falls_back_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        write_hub_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUBNK",
                    "commonName": "Bank Underground Station",
                    "modes": ["tube"],
                    "lat": 51.51225, "lon": -0.087792,
                    "hubNaptanCode": "HUBBAN",
                    "lineModeGroups": [{"lineIdentifier": ["central", "northern"]}]
                }]
            }),
        );
        // intentionally omit the HUBBAN.json hub fixture

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("bank").await.expect("missing hub fixture must not cause error");

        let bank = results.iter().find(|s| s.id == "940GZZLUBNK").unwrap();
        let line_ids: Vec<&str> = bank.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(line_ids.contains(&"central"), "tube lines must still be present");
    }

    #[tokio::test]
    async fn hub_merge_drops_unsupported_modes_from_hub_children() {
        let dir = tempfile::tempdir().unwrap();
        write_hub_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUVIC",
                    "commonName": "Victoria Underground Station",
                    "modes": ["tube"],
                    "lat": 51.495, "lon": -0.144,
                    "hubNaptanCode": "HUBVIC",
                    "lineModeGroups": [{"lineIdentifier": ["victoria", "district", "circle"]}]
                }]
            }),
        );
        write_hub_fixture(
            dir.path(),
            "HUBVIC",
            serde_json::json!({
                "id": "HUBVIC",
                "children": [
                    {"id": "940GZZLUVIC", "modes": ["tube"],
                     "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["victoria","district","circle"]}]},
                    {"id": "490VIC", "modes": ["bus"],
                     "lineModeGroups": [{"modeName": "bus", "lineIdentifier": ["52","C1"]}]},
                    {"id": "910GVIC", "modes": ["national-rail"],
                     "lineModeGroups": [{"modeName": "national-rail", "lineIdentifier": ["gatwick-express","southern"]}]}
                ]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("victoria").await.expect("search should succeed");

        let victoria = results.iter().find(|s| s.id == "940GZZLUVIC").unwrap();
        let line_ids: Vec<&str> = victoria.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(!line_ids.iter().any(|id| *id == "52" || *id == "C1"), "bus lines must be excluded");
        assert!(!line_ids.iter().any(|id| *id == "gatwick-express" || *id == "southern"), "national-rail lines must be excluded");
        assert!(line_ids.contains(&"victoria"), "tube lines must remain");
    }

    // -------------------------------------------------------------------------
    // Multi-mode stop-points cache + search filter
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn stop_points_cache_includes_overground_dlr_elizabeth_stations() {
        let client = real_client();
        client.warm_stop_points_cache().await.expect("warm should succeed");

        let allowed_og = client.allowed_line_ids_for("910GHACKNYC").await.unwrap();
        assert!(
            allowed_og.contains("mildmay"),
            "Hackney Central should resolve to its Mildmay line; got {allowed_og:?}"
        );

        let allowed_dlr = client.allowed_line_ids_for("940GZZDLBEC").await.unwrap();
        assert!(
            allowed_dlr.contains("dlr"),
            "Beckton DLR should resolve to its dlr line; got {allowed_dlr:?}"
        );
    }

    #[tokio::test]
    async fn search_stations_returns_overground_only_station() {
        let client = real_client();
        let results = client.search_stations("hackney central").await.expect("hackney search should succeed");

        let hackney = results.iter().find(|s| s.id == "910GHACKNYC").unwrap_or_else(|| {
            panic!("Hackney Central must appear in results; got {:?}", results.iter().map(|s| &s.id).collect::<Vec<_>>())
        });
        let line_ids: Vec<&str> = hackney.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(line_ids.contains(&"mildmay"), "Hackney Central must carry mildmay; got {line_ids:?}");
    }

    #[tokio::test]
    async fn search_stations_includes_dlr_only_station() {
        let client = real_client();
        let results = client.search_stations("beckton").await.expect("beckton search should succeed");

        assert!(
            results.iter().any(|s| s.id == "940GZZDLBEC"),
            "Beckton DLR must appear in results; got {:?}",
            results.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn search_stations_excludes_national_rail_only_910g_stations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sp_dir = dir.path().join("stop-points");
        fs::create_dir_all(&sp_dir).unwrap();
        let line_dir = dir.path().join("line-status");
        fs::create_dir_all(&line_dir).unwrap();

        let tube = serde_json::json!({
            "stopPoints": [{
                "id": "940GZZLUTST",
                "commonName": "Test Tube Station",
                "lat": 51.5, "lon": -0.1,
                "modes": ["tube"],
                "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["northern"]}],
            }]
        });
        let og = serde_json::json!({
            "stopPoints": [{
                "id": "910GGATWICKEXP",
                "commonName": "Gatwick Express NR-Only Test",
                "lat": 51.5, "lon": -0.1,
                "modes": ["national-rail"],
                "lineModeGroups": [{"modeName": "national-rail", "lineIdentifier": ["gatwick-express"]}],
            }]
        });
        fs::write(sp_dir.join("tube.json"), serde_json::to_string(&tube).unwrap()).unwrap();
        fs::write(sp_dir.join("overground.json"), serde_json::to_string(&og).unwrap()).unwrap();
        fs::write(sp_dir.join("dlr.json"), r#"{"stopPoints":[]}"#).unwrap();
        fs::write(sp_dir.join("elizabeth-line.json"), r#"{"stopPoints":[]}"#).unwrap();
        for mode in ["tube", "overground", "dlr", "elizabeth-line"] {
            fs::write(line_dir.join(format!("{mode}.json")), "[]").unwrap();
        }

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("gatwick").await.unwrap();
        assert!(
            !results.iter().any(|s| s.id == "910GGATWICKEXP"),
            "NR-only 910G entry must be dropped; got {:?}",
            results.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn search_stations_excludes_platform_children_and_hubs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sp_dir = dir.path().join("stop-points");
        fs::create_dir_all(&sp_dir).unwrap();
        let line_dir = dir.path().join("line-status");
        fs::create_dir_all(&line_dir).unwrap();

        let tube = serde_json::json!({
            "stopPoints": [
                {"id": "940GZZLUTST", "commonName": "Test Station", "lat": 51.5, "lon": -0.1,
                 "modes": ["tube"], "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["northern"]}]},
                {"id": "9400ZZLUTST1", "commonName": "Test Station Platform 1", "lat": 51.5, "lon": -0.1,
                 "modes": ["tube"], "lineModeGroups": []},
                {"id": "4900ZZLUTST2", "commonName": "Test Station Bus Stop", "lat": 51.5, "lon": -0.1,
                 "modes": ["tube"], "lineModeGroups": []},
            ]
        });
        let og = serde_json::json!({
            "stopPoints": [
                {"id": "2100TSTPLT0", "commonName": "Test OG Platform Station", "lat": 51.5, "lon": -0.1,
                 "modes": ["overground"], "lineModeGroups": []},
                {"id": "4900TSTOGENT", "commonName": "Test OG Entrance", "lat": 51.5, "lon": -0.1,
                 "modes": ["overground"], "lineModeGroups": []},
            ]
        });
        fs::write(sp_dir.join("tube.json"), serde_json::to_string(&tube).unwrap()).unwrap();
        fs::write(sp_dir.join("overground.json"), serde_json::to_string(&og).unwrap()).unwrap();
        fs::write(sp_dir.join("dlr.json"), r#"{"stopPoints":[]}"#).unwrap();
        fs::write(sp_dir.join("elizabeth-line.json"), r#"{"stopPoints":[]}"#).unwrap();
        for mode in ["tube", "overground", "dlr", "elizabeth-line"] {
            fs::write(line_dir.join(format!("{mode}.json")), "[]").unwrap();
        }

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("test").await.unwrap();
        let ids: Vec<&str> = results.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["940GZZLUTST"], "only canonical station should remain");
    }

    #[tokio::test]
    async fn stop_points_cache_dedupes_station_id_across_modes_and_unions_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sp_dir = dir.path().join("stop-points");
        fs::create_dir_all(&sp_dir).unwrap();
        let line_dir = dir.path().join("line-status");
        fs::create_dir_all(&line_dir).unwrap();

        let tube = serde_json::json!({
            "stopPoints": [{
                "id": "940GZZLUSTR",
                "commonName": "Stratford Underground Station",
                "lat": 51.541, "lon": -0.003,
                "modes": ["tube"],
                "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["central", "jubilee"]}],
            }]
        });
        let og = serde_json::json!({
            "stopPoints": [{
                "id": "940GZZLUSTR",
                "commonName": "Stratford Underground Station",
                "lat": 51.541, "lon": -0.003,
                "modes": ["overground"],
                "lineModeGroups": [{"modeName": "overground", "lineIdentifier": ["mildmay"]}],
            }]
        });
        fs::write(sp_dir.join("tube.json"), serde_json::to_string(&tube).unwrap()).unwrap();
        fs::write(sp_dir.join("overground.json"), serde_json::to_string(&og).unwrap()).unwrap();
        fs::write(sp_dir.join("dlr.json"), r#"{"stopPoints":[]}"#).unwrap();
        fs::write(sp_dir.join("elizabeth-line.json"), r#"{"stopPoints":[]}"#).unwrap();
        for mode in ["tube", "overground", "dlr", "elizabeth-line"] {
            fs::write(line_dir.join(format!("{mode}.json")), "[]").unwrap();
        }

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("stratford").await.unwrap();
        let matches: Vec<&Station> = results.iter().filter(|s| s.id == "940GZZLUSTR").collect();
        assert_eq!(matches.len(), 1, "expected exactly one Stratford row");
        let lines: std::collections::BTreeSet<&str> =
            matches[0].lines.iter().map(|l| l.id.as_str()).collect();
        let expected: std::collections::BTreeSet<&str> =
            ["central", "jubilee", "mildmay"].into_iter().collect();
        assert_eq!(lines, expected, "merged station must carry the union of tube + overground line ids");
    }

    #[tokio::test]
    async fn search_dedupes_multi_mode_interchange_to_one_row() {
        let client = real_client();

        let bank_results = client.search_stations("bank").await.unwrap();
        let bank_rows: Vec<&Station> = bank_results
            .iter()
            .filter(|s| s.hub_naptan_code.as_deref() == Some("HUBBAN"))
            .collect();
        assert_eq!(bank_rows.len(), 1, "expected one Bank row; got {} ({:?})", bank_rows.len(),
            bank_rows.iter().map(|s| (&s.id, &s.common_name)).collect::<Vec<_>>());
        assert!(bank_rows[0].id.starts_with("940GZZLU"), "tube canonical should win");

        let farr_results = client.search_stations("farringdon").await.unwrap();
        let farr_rows: Vec<&Station> = farr_results
            .iter()
            .filter(|s| s.hub_naptan_code.as_deref() == Some("HUBZFD"))
            .collect();
        assert_eq!(farr_rows.len(), 1, "expected one Farringdon row");
        assert!(farr_rows[0].id.starts_with("940GZZLU"), "tube canonical should win");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn warm_retries_per_mode_on_transient_failure() {
        struct FlakyTubeHttp<H: TflHttp> {
            inner: H,
            tube_calls: Arc<AtomicUsize>,
        }
        impl<H: TflHttp> TflHttp for FlakyTubeHttp<H> {
            fn fetch(
                &self,
                endpoint: &str,
                id: &str,
            ) -> impl std::future::Future<Output = Result<Value, TflError>> + Send {
                let is_first_tube_warm = endpoint == "stop-points"
                    && id == "tube"
                    && self.tube_calls.fetch_add(1, Ordering::SeqCst) == 0;
                let inner_fut = self.inner.fetch(endpoint, id);
                async move {
                    if is_first_tube_warm {
                        Err(TflError::RateLimited { retry_after: None })
                    } else {
                        inner_fut.await
                    }
                }
            }
        }

        let tube_calls = Arc::new(AtomicUsize::new(0));
        let http = FlakyTubeHttp {
            inner: FixtureTflHttp::new(workspace_fixtures_dir()),
            tube_calls: tube_calls.clone(),
        };
        let client = TflClient::new(http);

        client.warm_stop_points_cache().await.expect("warm should ultimately succeed via per-mode retry");

        assert!(
            tube_calls.load(Ordering::SeqCst) >= 2,
            "tube fetch must have retried at least once; got {} calls",
            tube_calls.load(Ordering::SeqCst),
        );

        let bzp = client.allowed_line_ids_for("940GZZLUBZP").await.unwrap();
        assert!(bzp.contains("northern"), "Belsize Park must be in the cache after retry; got {bzp:?}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn warm_does_not_retry_on_terminal_errors() {
        struct AlwaysNotFoundDlrHttp<H: TflHttp> {
            inner: H,
            dlr_calls: Arc<AtomicUsize>,
        }
        impl<H: TflHttp> TflHttp for AlwaysNotFoundDlrHttp<H> {
            fn fetch(
                &self,
                endpoint: &str,
                id: &str,
            ) -> impl std::future::Future<Output = Result<Value, TflError>> + Send {
                let count_dlr = endpoint == "stop-points" && id == "dlr";
                if count_dlr {
                    self.dlr_calls.fetch_add(1, Ordering::SeqCst);
                }
                let inner_fut = self.inner.fetch(endpoint, id);
                async move {
                    if count_dlr {
                        Err(TflError::NotFound("simulated".to_string()))
                    } else {
                        inner_fut.await
                    }
                }
            }
        }

        let dlr_calls = Arc::new(AtomicUsize::new(0));
        let http = AlwaysNotFoundDlrHttp {
            inner: FixtureTflHttp::new(workspace_fixtures_dir()),
            dlr_calls: dlr_calls.clone(),
        };
        let client = TflClient::new(http);

        client.warm_stop_points_cache().await.expect("warm completes when at least one mode succeeds");

        assert_eq!(dlr_calls.load(Ordering::SeqCst), 1, "NotFound is terminal — DLR must be tried exactly once");
    }

    #[tokio::test]
    async fn search_stations_does_not_refetch_when_cache_is_stale_but_present() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client.warm_stop_points_cache().await.expect("warm should succeed");
        let after_warm = count.load(Ordering::SeqCst);

        client.__test_force_stale_stop_points_cache().expect("test helper must work");

        let results = client.search_stations("belsize").await.unwrap();
        let after_search = count.load(Ordering::SeqCst);

        assert!(!results.is_empty(), "stale cache must still serve results");
        assert_eq!(
            after_warm, after_search,
            "search against stale-but-present cache must not refetch",
        );
    }

    #[tokio::test]
    async fn refresh_stop_points_cache_forces_refetch_even_when_fresh() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client.warm_stop_points_cache().await.expect("warm should succeed");
        let after_warm = count.load(Ordering::SeqCst);

        let n = client.refresh_stop_points_cache().await.expect("refresh should succeed");
        let after_refresh = count.load(Ordering::SeqCst);

        assert!(n > 100, "expect populated station count");
        assert!(after_refresh > after_warm, "refresh_stop_points_cache must trigger network calls even on a fresh cache");
    }

    #[tokio::test]
    async fn allowed_line_ids_for_serves_stale_cache_past_ttl() {
        let client = real_client();
        client.warm_stop_points_cache().await.expect("warm should succeed");

        let allowed_fresh = client.allowed_line_ids_for("940GZZLUBNK").await.unwrap();
        assert!(allowed_fresh.contains("dlr"), "fresh cache: Bank tube parent should know about DLR; got {allowed_fresh:?}");

        client.__test_force_stale_stop_points_cache().expect("test helper must work");

        let allowed_stale = client.allowed_line_ids_for("940GZZLUBNK").await.unwrap();
        assert!(
            allowed_stale.contains("dlr"),
            "stale cache: Bank tube parent must STILL know about DLR; got {allowed_stale:?}"
        );
        assert_eq!(allowed_stale, allowed_fresh, "stale-but-cached lookup must return identical data to fresh");
    }

    #[tokio::test]
    async fn search_keeps_non_hub_stations_individually() {
        let client = real_client();
        let results = client.search_stations("hackney central").await.unwrap();
        assert!(
            results.iter().any(|s| s.id == "910GHACKNYC"),
            "Hackney Central (no hub) should not be deduped away",
        );
    }

    #[tokio::test]
    async fn search_bank_includes_dlr_chip() {
        let client = real_client();
        let results = client.search_stations("bank").await.expect("search should succeed");

        let bank = results.iter().find(|s| s.id == "940GZZLUBNK").expect("Bank must appear in results");
        let line_ids: Vec<&str> = bank.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(line_ids.contains(&"dlr"), "Bank must include DLR chip after hub merge; got {line_ids:?}");
        assert!(line_ids.contains(&"central"), "Bank must still include tube lines; got {line_ids:?}");
    }

    #[tokio::test]
    async fn search_tottenham_court_road_includes_elizabeth_chip() {
        let client = real_client();
        let results = client.search_stations("tottenham").await.expect("search should succeed");

        let tcr = results.iter().find(|s| s.id == "940GZZLUTCR").expect("TCR must appear in results");
        let line_ids: Vec<&str> = tcr.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(line_ids.contains(&"elizabeth"), "TCR must include Elizabeth chip; got {line_ids:?}");
        assert!(line_ids.contains(&"central"), "TCR must still include tube lines; got {line_ids:?}");
    }

    #[tokio::test]
    async fn search_whitechapel_includes_elizabeth_and_windrush_chips() {
        let client = real_client();
        let results = client.search_stations("whitechapel").await.expect("search should succeed");

        let wpl = results.iter().find(|s| s.id == "940GZZLUWPL").expect("Whitechapel must appear in results");
        let line_ids: Vec<&str> = wpl.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(line_ids.contains(&"elizabeth"), "Whitechapel must include Elizabeth chip; got {line_ids:?}");
        assert!(line_ids.contains(&"windrush"), "Whitechapel must include Windrush chip; got {line_ids:?}");
        assert!(line_ids.contains(&"hammersmith-city"), "Whitechapel must still include tube lines; got {line_ids:?}");
    }

    #[tokio::test]
    async fn search_belsize_park_tube_only_unchanged() {
        let client = real_client();
        let results = client.search_stations("belsize").await.expect("search should succeed");

        let bzp = results.iter().find(|s| s.id == "940GZZLUBZP").expect("Belsize Park must appear in results");
        assert!(bzp.hub_naptan_code.is_none(), "Belsize Park must have no hub code");
        let line_ids: Vec<&str> = bzp.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(line_ids.iter().all(|id| *id == "northern"), "Belsize Park must have only the Northern line");
    }

    // -------------------------------------------------------------------------
    // New: single-flight test with fake TflHttp
    // (This is the deliverable proof the cache is independently testable)
    // -------------------------------------------------------------------------

    /// **Single-flighted refresh**: concurrent callers during a cold-cache
    /// warm MUST serialise — only the first caller does the network fetch;
    /// subsequent callers await the lock, re-check the now-warm cache, and
    /// return immediately without issuing additional fetches.
    ///
    /// Uses a `SlowFakeTflHttp` that counts `stop-points` calls and adds a
    /// small artificial delay so the concurrent callers genuinely race.
    /// After `n` concurrent `warm_stop_points_cache` calls, the total
    /// `stop-points` fetch count must equal `SUPPORTED_MODES.len()` (one
    /// per mode, not `n * SUPPORTED_MODES.len()`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn single_flight_concurrent_warm_issues_one_fetch_per_mode() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        // A fake TflHttp that counts stop-points fetches and simulates
        // network latency so concurrent callers genuinely race.
        struct SlowFakeTflHttp {
            inner: FixtureTflHttp,
            stop_points_calls: Arc<AtomicUsize>,
        }

        impl TflHttp for SlowFakeTflHttp {
            fn fetch(
                &self,
                endpoint: &str,
                id: &str,
            ) -> impl std::future::Future<Output = Result<serde_json::Value, TflError>> + Send {
                if endpoint == "stop-points" {
                    self.stop_points_calls.fetch_add(1, Ordering::SeqCst);
                }
                let inner = self.inner.clone();
                let endpoint = endpoint.to_string();
                let id = id.to_string();
                async move {
                    // Simulate network latency — enough for concurrent callers to pile up.
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    inner.fetch(&endpoint, &id).await
                }
            }
        }

        let stop_points_calls = Arc::new(AtomicUsize::new(0));
        let http = SlowFakeTflHttp {
            inner: FixtureTflHttp::new(workspace_fixtures_dir()),
            stop_points_calls: stop_points_calls.clone(),
        };

        // Wrap in Arc so we can share across tasks.
        let client = Arc::new(TflClient::new(http));

        // Launch N concurrent warm calls. Without single-flight, all N
        // would each fan out SUPPORTED_MODES_COUNT stop-points fetches.
        const N: usize = 5;
        let handles: Vec<_> = (0..N)
            .map(|_| {
                let c = client.clone();
                tokio::spawn(async move { c.warm_stop_points_cache().await })
            })
            .collect();

        for h in handles {
            h.await.expect("task must not panic").expect("warm must succeed");
        }

        let total_calls = stop_points_calls.load(Ordering::SeqCst);
        // Single-flight: exactly SUPPORTED_MODES_COUNT fetches (one per mode).
        // Without single-flight: N * SUPPORTED_MODES_COUNT = 20 fetches.
        assert_eq!(
            total_calls,
            SUPPORTED_MODES_COUNT,
            "single-flight must serialise concurrent warms to exactly one fetch per mode \
             ({SUPPORTED_MODES_COUNT}); got {total_calls} — without single-flight each of the \
             {N} concurrent callers would each fan out {SUPPORTED_MODES_COUNT} mode fetches"
        );
    }
}
