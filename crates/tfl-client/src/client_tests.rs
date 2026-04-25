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
        // All three use the canonical `940GZZLU*` id prefix so search_stations'
        // dedupe filter lets them through; their common names put them into
        // tier 1 / tier 2 for relevance-order assertions.
        let results2 = synthetic
            .search_stations("bank")
            .await
            .expect("synthetic search should succeed");
        assert_eq!(results2.len(), 3, "all 3 contain 'bank'");
        // 940GZZLUBBB: "bank underground station" starts_with "bank" → tier 1
        // 940GZZLUAAA: "bank road underground" starts_with "bank" → tier 1
        // 940GZZLUCCC: "old bank junction" contains "bank" → tier 2
        // Within tier 1: alphabetical by common_name → A ("bank road…") then B.
        assert_eq!(results2[0].id, "940GZZLUAAA");
        assert_eq!(results2[1].id, "940GZZLUBBB");
        assert_eq!(results2[2].id, "940GZZLUCCC");
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
                { "id": "940GZZLUVIC", "commonName": "Victoria Underground Station", "modes": ["tube"], "lat": 51.5, "lon": -0.14 },
                { "id": "490BUSVIC",   "commonName": "Victoria Bus Stop",             "modes": ["bus"],  "lat": 51.5, "lon": -0.14 },
                { "id": "940GZZLUVCX", "commonName": "Victoria Coach Terminal",       "modes": ["tube", "bus"], "lat": 51.5, "lon": -0.14 }
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

        // Bus-only stop must be excluded (fails both the mode filter and the
        // canonical-id filter).
        let ids: Vec<&str> = results.iter().map(|s| s.id.as_str()).collect();
        assert!(
            !ids.contains(&"490BUSVIC"),
            "bus-only stop must be excluded; got: {ids:?}"
        );
        assert!(
            ids.contains(&"940GZZLUVIC"),
            "tube stop must be included; got: {ids:?}"
        );
        assert!(
            ids.contains(&"940GZZLUVCX"),
            "tube+bus stop must be included (has tube mode + canonical id); got: {ids:?}"
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
    // Station.lines projection from TfL `lineModeGroups`
    // -------------------------------------------------------------------------
    //
    // TfL's /StopPoint/Mode/{mode} response includes `lineModeGroups`:
    //   "lineModeGroups": [
    //     { "modeName": "tube", "lineIdentifier": ["bakerloo", "central"] },
    //     { "modeName": "bus",  "lineIdentifier": ["24", "29"] }
    //   ]
    // The `lines` field on Station is what the UI consumes. search_stations
    // must project the tube entry of `lineModeGroups` into Station.lines so
    // the Settings UI can show station-scoped line chips.

    fn write_stop_points_fixture(dir: &std::path::Path, body: serde_json::Value) {
        let ep_dir = dir.join("stop-points");
        std::fs::create_dir_all(&ep_dir).unwrap();
        std::fs::write(
            ep_dir.join("tube.json"),
            serde_json::to_string(&body).unwrap(),
        )
        .unwrap();
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
        let results = client
            .search_stations("oxford")
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        let oxc = &results[0];
        let line_ids: Vec<&str> = oxc.lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            line_ids,
            vec!["bakerloo", "central", "victoria"],
            "Station.lines should contain exactly the tube lineIdentifier entries"
        );

        // Names must be human-readable, not the raw id.
        let names: Vec<&str> = oxc.lines.iter().map(|l| l.name.as_str()).collect();
        assert!(
            names.contains(&"Bakerloo")
                && names.contains(&"Central")
                && names.contains(&"Victoria"),
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
                    "lat": 51.5,
                    "lon": -0.1,
                    "lineModeGroups": [
                        { "modeName": "tube", "lineIdentifier": ["northern"] },
                        { "modeName": "bus",  "lineIdentifier": ["24", "29"] }
                    ]
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("mixed")
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        let line_ids: Vec<&str> = results[0].lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            line_ids,
            vec!["northern"],
            "only tube lineModeGroups entries should populate Station.lines"
        );
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
                    "lat": 51.5,
                    "lon": -0.1,
                    "lineModeGroups": []
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("bare")
            .await
            .expect("search should succeed");

        assert_eq!(results.len(), 1);
        assert!(
            results[0].lines.is_empty(),
            "station without lineModeGroups must have empty Station.lines (fall back to global chip list)"
        );
    }

    #[tokio::test]
    async fn search_stations_accepts_trimmed_line_mode_groups_without_mode_name() {
        // Our bundled fixture drops the `modeName` field to save space (the
        // parent Station's `modes` already constrains us to tube stations).
        // An entry without modeName must still populate Station.lines.
        let dir = tempfile::tempdir().unwrap();
        write_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUTRM",
                    "commonName": "Trimmed Fixture Station",
                    "modes": ["tube"],
                    "lat": 51.5,
                    "lon": -0.1,
                    "lineModeGroups": [ { "lineIdentifier": ["jubilee", "metropolitan"] } ]
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("trimmed")
            .await
            .expect("search should succeed");

        let line_ids: Vec<&str> = results[0].lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(line_ids, vec!["jubilee", "metropolitan"]);
    }

    #[tokio::test]
    async fn search_stations_prefers_explicit_lines_field_over_line_mode_groups() {
        // Backward-compat: if a fixture (or future API) already supplies the
        // processed `lines` field, honour it verbatim and do NOT overwrite with
        // lineModeGroups. This keeps existing inline-JSON tests stable.
        let dir = tempfile::tempdir().unwrap();
        write_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUEXP",
                    "commonName": "Explicit Lines Underground",
                    "modes": ["tube"],
                    "lat": 51.5,
                    "lon": -0.1,
                    "lines": [ { "id": "jubilee", "name": "Jubilee" } ],
                    "lineModeGroups": [
                        { "modeName": "tube", "lineIdentifier": ["northern", "central"] }
                    ]
                }]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("explicit")
            .await
            .expect("search should succeed");

        let line_ids: Vec<&str> = results[0].lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            line_ids,
            vec!["jubilee"],
            "explicit `lines` field must take precedence over lineModeGroups"
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

    // -------------------------------------------------------------------------
    // stop-points cache (search_stations + warm_stop_points_cache)
    // -------------------------------------------------------------------------
    //
    // Any call that needs the full tube stop-points list should hit the
    // transport at most once per TTL window. Re-fetching 16 MB on every
    // keystroke made the typeahead feel broken; the cache makes the second
    // keystroke instant.

    use crate::http::TflHttp;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Wraps any `TflHttp` and counts calls per (endpoint, id) pair.
    struct CountingTflHttp<H: TflHttp> {
        inner: H,
        count: Arc<AtomicUsize>,
    }

    impl<H: TflHttp> CountingTflHttp<H> {
        fn new(inner: H) -> (Self, Arc<AtomicUsize>) {
            let count = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    inner,
                    count: count.clone(),
                },
                count,
            )
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

    #[tokio::test]
    async fn search_stations_uses_cached_stations_on_second_call() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        let first = client.search_stations("belsize").await.unwrap();
        let second = client.search_stations("king").await.unwrap();

        assert!(
            !first.is_empty() && !second.is_empty(),
            "both searches should return results"
        );
        // 1 stop-points fetch + 3 hub fetches (HUBBAN, HUBWHC, HUBTCR) on the
        // cold load; the second search hits the cache and adds 0.
        assert_eq!(
            count.load(Ordering::SeqCst),
            4,
            "two search_stations calls should share a single stop-points load (1 + 3 hub fetches)",
        );
    }

    #[tokio::test]
    async fn search_stations_refetches_after_cache_invalidated() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client.search_stations("belsize").await.unwrap();
        client.invalidate_stop_points_cache();
        client.search_stations("belsize").await.unwrap();

        // First load: 1 stop-points + 3 hub fetches = 4.
        // Invalidation clears only stop_points_cache; hub_lines_cache stays warm.
        // Refetch: 1 stop-points + 0 hub fetches = 1. Total: 5.
        assert_eq!(
            count.load(Ordering::SeqCst),
            5,
            "invalidating the cache forces a stop-points refetch; hub lines stay cached",
        );
    }

    #[tokio::test]
    async fn warm_stop_points_cache_populates_cache_for_zero_extra_fetches() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        let warmed = client.warm_stop_points_cache().await.unwrap();
        assert!(warmed > 100, "fixture should contain many tube stations");

        // Subsequent searches must not trigger any further fetches.
        let _ = client.search_stations("victoria").await.unwrap();
        let _ = client.search_stations("king").await.unwrap();

        // 1 stop-points fetch + 3 hub fetches on warm; both searches are cache hits.
        assert_eq!(
            count.load(Ordering::SeqCst),
            4,
            "warm + two searches should total one stop-points fetch plus three hub fetches",
        );
    }

    // -------------------------------------------------------------------------
    // Live-shape integration: searching the real fixtures/stop-points/tube.json
    // -------------------------------------------------------------------------
    //
    // This is the check PR #11 missed. Earlier tests asserted behaviour on
    // trimmed per-test fixtures; a user reported the live search still showed
    // no dropdown. We exercise the full stack (deserialization, cache, filter,
    // relevance ordering) against the same 1 682-station fixture the app ships.

    #[tokio::test]
    async fn search_stations_finds_victoria_in_full_fixture() {
        let client = real_client();

        let results = client
            .search_stations("victoria")
            .await
            .expect("victoria search should succeed");

        assert!(
            !results.is_empty(),
            "expected at least one match for 'victoria' in the shipped fixture"
        );

        // User-reported bug: searching "victoria" used to return 5+ rows
        // (hub, two platforms, two 4900* bus stops) all labelled "Victoria"
        // or "Victoria Station" with no way to tell which one is canonical.
        // The dropdown must now show exactly the 940G* parent — at most one
        // result per canonical station common-name.
        let mut seen_names = std::collections::BTreeSet::new();
        for r in &results {
            assert!(
                r.id.starts_with("940GZZLU"),
                "non-canonical id made it into results: {:?} ({:?})",
                r.id,
                r.common_name
            );
            assert!(
                seen_names.insert(r.common_name.to_lowercase()),
                "duplicate station name in results: {:?}",
                r.common_name
            );
        }

        let victoria_tube = results
            .iter()
            .find(|s| s.id == "940GZZLUVIC" && s.modes.iter().any(|m| m == "tube"))
            .expect("Victoria Underground Station (id 940GZZLUVIC) should be in results");

        // The settings UI keys its per-station line chips off Station.lines.
        // The fixture encodes Victoria with lineIdentifier [district, circle,
        // victoria] — all three must survive deserialization.
        let line_ids: Vec<&str> = victoria_tube.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            line_ids.contains(&"victoria"),
            "expected victoria line in Station.lines, got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"district") || line_ids.contains(&"circle"),
            "expected at least one of district/circle in Station.lines, got {line_ids:?}"
        );
    }

    #[tokio::test]
    async fn search_stations_victoria_second_call_hits_cache() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        let _ = client.search_stations("victoria").await.unwrap();
        let _ = client.search_stations("victoria").await.unwrap();
        let _ = client.search_stations("oxford").await.unwrap();

        // Cold load: 1 stop-points + 3 hub fetches = 4; both subsequent searches are cache hits.
        assert_eq!(
            count.load(Ordering::SeqCst),
            4,
            "three searches against the full fixture should share one stop-points load"
        );
    }

    #[tokio::test]
    async fn warm_stop_points_cache_is_idempotent() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client.warm_stop_points_cache().await.unwrap();
        client.warm_stop_points_cache().await.unwrap();
        client.warm_stop_points_cache().await.unwrap();

        // First warm: 1 stop-points + 3 hub fetches = 4. Subsequent warms are cache hits.
        assert_eq!(
            count.load(Ordering::SeqCst),
            4,
            "repeated warm calls within the TTL should not refetch",
        );
    }

    // -------------------------------------------------------------------------
    // Hub line merge — multi-mode stations get DLR / Elizabeth / Overground chips
    // -------------------------------------------------------------------------
    //
    // When a tube station carries a `hubNaptanCode`, `stop_points_cached` must
    // fetch the hub's child stop-points and merge their lines into the station's
    // `Station.lines`. This is the data the Settings chip UI renders for the
    // line-filter.

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
        // A station with hubNaptanCode plus a hub fixture that adds a DLR
        // child. After search_stations the Station.lines must contain the DLR
        // line from the hub alongside the tube lines from lineModeGroups.
        let dir = tempfile::tempdir().unwrap();
        write_hub_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUBNK",
                    "commonName": "Bank Underground Station",
                    "modes": ["tube"],
                    "lat": 51.51225,
                    "lon": -0.087792,
                    "hubNaptanCode": "HUBBAN",
                    "lineModeGroups": [{
                        "lineIdentifier": ["central", "waterloo-city", "northern"]
                    }]
                }]
            }),
        );
        write_hub_fixture(
            dir.path(),
            "HUBBAN",
            serde_json::json!({
                "id": "HUBBAN",
                "children": [
                    {
                        "id": "940GZZLUBNK",
                        "modes": ["tube"],
                        "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["central","waterloo-city","northern"]}]
                    },
                    {
                        "id": "940GZZDLBNK",
                        "modes": ["dlr"],
                        "lineModeGroups": [{"modeName": "dlr", "lineIdentifier": ["dlr"]}]
                    }
                ]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("bank")
            .await
            .expect("search should succeed");

        let bank = results
            .iter()
            .find(|s| s.id == "940GZZLUBNK")
            .expect("Bank must be in results");

        let line_ids: Vec<&str> = bank.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            line_ids.contains(&"central"),
            "tube lines must still be present, got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"dlr"),
            "DLR from hub child must be merged in, got {line_ids:?}"
        );
    }

    #[tokio::test]
    async fn hub_merge_deduplicates_lines_present_in_both_parent_and_hub() {
        // If the hub child repeats a line already in the tube parent's
        // lineModeGroups, the merged Station.lines must not contain duplicates.
        let dir = tempfile::tempdir().unwrap();
        write_hub_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUBNK",
                    "commonName": "Bank Underground Station",
                    "modes": ["tube"],
                    "lat": 51.51225,
                    "lon": -0.087792,
                    "hubNaptanCode": "HUBBAN",
                    "lineModeGroups": [{
                        "lineIdentifier": ["central", "northern"]
                    }]
                }]
            }),
        );
        write_hub_fixture(
            dir.path(),
            "HUBBAN",
            serde_json::json!({
                "id": "HUBBAN",
                "children": [
                    {
                        "id": "940GZZLUBNK",
                        "modes": ["tube"],
                        "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["central","northern"]}]
                    },
                    {
                        "id": "940GZZDLBNK",
                        "modes": ["dlr"],
                        // hub child also lists "central" — must not duplicate
                        "lineModeGroups": [{"modeName": "dlr", "lineIdentifier": ["dlr","central"]}]
                    }
                ]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("bank")
            .await
            .expect("search should succeed");

        let bank = results.iter().find(|s| s.id == "940GZZLUBNK").unwrap();
        let central_count = bank.lines.iter().filter(|l| l.id == "central").count();
        assert_eq!(
            central_count,
            1,
            "central must appear exactly once; got lines: {:?}",
            bank.lines.iter().map(|l| &l.id).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn hub_merge_missing_hub_fixture_falls_back_gracefully() {
        // A station with hubNaptanCode but no hub fixture must not error —
        // the tube-only lines from lineModeGroups still show up.
        let dir = tempfile::tempdir().unwrap();
        write_hub_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUBNK",
                    "commonName": "Bank Underground Station",
                    "modes": ["tube"],
                    "lat": 51.51225,
                    "lon": -0.087792,
                    "hubNaptanCode": "HUBBAN",
                    "lineModeGroups": [{
                        "lineIdentifier": ["central", "northern"]
                    }]
                }]
            }),
        );
        // intentionally omit the HUBBAN.json hub fixture

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("bank")
            .await
            .expect("missing hub fixture must not cause an error");

        let bank = results.iter().find(|s| s.id == "940GZZLUBNK").unwrap();
        let line_ids: Vec<&str> = bank.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            line_ids.contains(&"central"),
            "tube lines must still be present even when hub fixture is missing"
        );
    }

    #[tokio::test]
    async fn hub_merge_drops_unsupported_modes_from_hub_children() {
        // Bus and national-rail children in the hub must not contribute
        // their line ids — only tube/dlr/overground/elizabeth-line children.
        let dir = tempfile::tempdir().unwrap();
        write_hub_stop_points_fixture(
            dir.path(),
            serde_json::json!({
                "total": 1,
                "stopPoints": [{
                    "id": "940GZZLUVIC",
                    "commonName": "Victoria Underground Station",
                    "modes": ["tube"],
                    "lat": 51.495,
                    "lon": -0.144,
                    "hubNaptanCode": "HUBVIC",
                    "lineModeGroups": [{
                        "lineIdentifier": ["victoria", "district", "circle"]
                    }]
                }]
            }),
        );
        write_hub_fixture(
            dir.path(),
            "HUBVIC",
            serde_json::json!({
                "id": "HUBVIC",
                "children": [
                    {
                        "id": "940GZZLUVIC",
                        "modes": ["tube"],
                        "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["victoria","district","circle"]}]
                    },
                    {
                        "id": "490VIC",
                        "modes": ["bus"],
                        "lineModeGroups": [{"modeName": "bus", "lineIdentifier": ["52","C1"]}]
                    },
                    {
                        "id": "910GVIC",
                        "modes": ["national-rail"],
                        "lineModeGroups": [{"modeName": "national-rail", "lineIdentifier": ["gatwick-express","southern"]}]
                    }
                ]
            }),
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client
            .search_stations("victoria")
            .await
            .expect("search should succeed");

        let victoria = results.iter().find(|s| s.id == "940GZZLUVIC").unwrap();
        let line_ids: Vec<&str> = victoria.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            !line_ids.iter().any(|id| *id == "52" || *id == "C1"),
            "bus line ids must be excluded, got {line_ids:?}"
        );
        assert!(
            !line_ids
                .iter()
                .any(|id| *id == "gatwick-express" || *id == "southern"),
            "national-rail line ids must be excluded, got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"victoria"),
            "tube lines must remain, got {line_ids:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Real-fixture integration: Bank, Whitechapel, TCR multi-mode chips
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn search_bank_includes_dlr_chip() {
        let client = real_client();
        let results = client
            .search_stations("bank")
            .await
            .expect("search should succeed");

        let bank = results
            .iter()
            .find(|s| s.id == "940GZZLUBNK")
            .expect("Bank must appear in results");

        let line_ids: Vec<&str> = bank.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            line_ids.contains(&"dlr"),
            "Bank must include DLR chip after hub merge; got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"central"),
            "Bank must still include tube lines; got {line_ids:?}"
        );
    }

    #[tokio::test]
    async fn search_tottenham_court_road_includes_elizabeth_chip() {
        let client = real_client();
        let results = client
            .search_stations("tottenham")
            .await
            .expect("search should succeed");

        let tcr = results
            .iter()
            .find(|s| s.id == "940GZZLUTCR")
            .expect("TCR must appear in results");

        let line_ids: Vec<&str> = tcr.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            line_ids.contains(&"elizabeth-line"),
            "TCR must include Elizabeth chip after hub merge; got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"central"),
            "TCR must still include tube lines; got {line_ids:?}"
        );
    }

    #[tokio::test]
    async fn search_whitechapel_includes_elizabeth_and_mildmay_chips() {
        let client = real_client();
        let results = client
            .search_stations("whitechapel")
            .await
            .expect("search should succeed");

        let wpl = results
            .iter()
            .find(|s| s.id == "940GZZLUWPL")
            .expect("Whitechapel must appear in results");

        let line_ids: Vec<&str> = wpl.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            line_ids.contains(&"elizabeth-line"),
            "Whitechapel must include Elizabeth chip; got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"mildmay"),
            "Whitechapel must include Mildmay (Overground) chip; got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"hammersmith-city"),
            "Whitechapel must still include tube lines; got {line_ids:?}"
        );
    }

    #[tokio::test]
    async fn search_belsize_park_tube_only_unchanged() {
        // Tube-only station without a hub must be unaffected by the merge.
        let client = real_client();
        let results = client
            .search_stations("belsize")
            .await
            .expect("search should succeed");

        let bzp = results
            .iter()
            .find(|s| s.id == "940GZZLUBZP")
            .expect("Belsize Park must appear in results");

        assert!(
            bzp.hub_naptan_code.is_none(),
            "Belsize Park must have no hub code"
        );
        let line_ids: Vec<&str> = bzp.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            line_ids.iter().all(|id| *id == "northern"),
            "Belsize Park must have only the Northern line, got {line_ids:?}"
        );
    }
}
