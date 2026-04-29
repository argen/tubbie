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

    /// Multi-mode coverage: `get_line_status` MUST find Overground line ids.
    /// Guards `client.rs::fetch_line_status_all_modes` and the per-mode fan-out
    /// — without it, the live ticker shows "no status" for the six 2024-named
    /// Overground lines because only `line-status/tube` was being fetched.
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

    /// `TflClient::with_modes` MUST gate the per-mode fetch fan-out so a
    /// memory-constrained consumer (the iOS shell) can opt out of modes it
    /// doesn't surface. A mode not in the list MUST NOT be fetched, even
    /// if a fixture for it is sitting on disk.
    #[tokio::test]
    async fn client_with_subset_modes_only_fetches_those_modes() {
        // Default client (all 4 modes) — every line resolves.
        let full = real_client();
        full.get_line_status("mildmay")
            .await
            .expect("default client should resolve overground line via overground fixture");

        // Subset client (tube only) — overground line MUST 404 because the
        // overground line-status fixture is not consulted.
        let tube_only = crate::client::TflClient::with_modes(
            crate::fixture::FixtureTflHttp::new(workspace_fixtures_dir()),
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

        // Northern (tube) MUST still resolve on the subset client.
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

    /// Number of stop-points endpoints fanned out per warm cycle —
    /// one per surfaced mode (`SUPPORTED_MODES.len()`). Used by the
    /// delta-based cache assertions below to bound the expected
    /// post-invalidation refetch count without hardcoding a specific
    /// hub-fixture population.
    const SUPPORTED_MODES_COUNT: usize = crate::client::SUPPORTED_MODES.len();

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

    /// Shared call log for `RecordingTflHttp` — every observed
    /// `(endpoint, id)` pair, in fetch order.
    type RecordedCalls = Arc<std::sync::Mutex<Vec<(String, String)>>>;

    /// Records every `(endpoint, id)` call so a test can count specific
    /// fetches (e.g. how many `stop-point/HUB*` calls fire during warm).
    struct RecordingTflHttp<H: TflHttp> {
        inner: H,
        calls: RecordedCalls,
    }

    impl<H: TflHttp> RecordingTflHttp<H> {
        fn new(inner: H) -> (Self, RecordedCalls) {
            let calls: RecordedCalls = Arc::new(std::sync::Mutex::new(Vec::new()));
            (
                Self {
                    inner,
                    calls: calls.clone(),
                },
                calls,
            )
        }
    }

    impl<H: TflHttp> TflHttp for RecordingTflHttp<H> {
        fn fetch(
            &self,
            endpoint: &str,
            id: &str,
        ) -> impl std::future::Future<Output = Result<Value, TflError>> + Send {
            self.calls
                .lock()
                .unwrap()
                .push((endpoint.to_string(), id.to_string()));
            self.inner.fetch(endpoint, id)
        }
    }

    /// Regression guard: hub fetches MUST be deduped by hub_id before the
    /// parallel fan-out in `stop_points_cached`. Without dedupe, the cold
    /// warm fires one fetch per station-with-hub_naptan_code (757 total
    /// against the production fixtures), but only ~90 unique hubs exist —
    /// a single hub like HUBKGX is referenced by 23 stations across the
    /// tube + DLR + Elizabeth feeds. The 23 racers all see an empty
    /// `hub_lines_cache` and each fires its own HTTP request before any
    /// can populate it. Production-side this saturates the connection
    /// pool and risks tripping TfL's 429 cooldown gate; offline-side it
    /// just floods the FixtureTflHttp with redundant disk reads.
    #[tokio::test]
    async fn warm_stop_points_dedupes_hub_fetches_before_fan_out() {
        let (http, calls) = RecordingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client
            .warm_stop_points_cache()
            .await
            .expect("warm should succeed");

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
            hub_calls.len(),
            unique.len(),
            hub_calls.len() - unique.len(),
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

        assert!(
            !first.is_empty() && !second.is_empty(),
            "both searches should return results"
        );
        // The exact cold-load count varies with the fixture's hub population
        // (every station with a `hubNaptanCode` triggers one hub-detail fetch
        // for the lines-merge step). What matters here is the delta: the
        // second search MUST add zero fetches because the stop-points cache
        // and the hub-lines cache both hit on the second call.
        assert_eq!(
            after_first, after_second,
            "second search_stations call must not trigger any additional fetches \
             (cold load: {after_first} fetches; second call should be a pure cache hit)",
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

        // After invalidation, stop_points_cache is cold but hub_lines_cache is
        // still warm. The next search must refetch stop-points (one per mode
        // in `SUPPORTED_MODES`) but NO hub-detail fetches — the hub-lines
        // are read from the per-process cache that survives the invalidation.
        let delta = after_refetch - after_warm;
        assert!(
            (1..=SUPPORTED_MODES_COUNT).contains(&delta),
            "refetch after invalidation should re-fetch stop-points (1..={SUPPORTED_MODES_COUNT}) \
             with the hub-lines cache still warm; observed {delta} new fetches",
        );
    }

    #[tokio::test]
    async fn warm_stop_points_cache_populates_cache_for_zero_extra_fetches() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        let warmed = client.warm_stop_points_cache().await.unwrap();
        assert!(warmed > 100, "fixture should contain many tube stations");
        let after_warm = count.load(Ordering::SeqCst);

        // Subsequent searches must not trigger any further fetches.
        let _ = client.search_stations("victoria").await.unwrap();
        let _ = client.search_stations("king").await.unwrap();
        let after_searches = count.load(Ordering::SeqCst);

        assert_eq!(
            after_warm, after_searches,
            "two searches after warm must hit the cache and add zero fetches \
             (warm: {after_warm} fetches; after searches: {after_searches})",
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
        // Every result MUST be a canonical group id from the prefix whitelist
        // (940GZZLU = tube, 940GZZDL = DLR, 910G = Overground/Elizabeth).
        // Platform-level (`9400*`/`4900*`/`2100*`) and hub aggregator (`HUB*`)
        // ids must NEVER leak through. Common-name uniqueness is no longer
        // asserted because legitimately-distinct stations now appear in the
        // multi-mode result set (e.g. "Victoria Underground" and "Royal
        // Victoria DLR Station" are separate canonical stops with different
        // parent ids — the user expects to see both).
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
        let after_first = count.load(Ordering::SeqCst);
        let _ = client.search_stations("victoria").await.unwrap();
        let _ = client.search_stations("oxford").await.unwrap();
        let after_three = count.load(Ordering::SeqCst);

        assert_eq!(
            after_first, after_three,
            "two further searches must add zero fetches (cold load: {after_first}; \
             after three calls: {after_three})",
        );
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

        assert_eq!(
            after_first, after_three,
            "repeated warm calls within the TTL must add zero fetches",
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

    // -------------------------------------------------------------------------
    // Multi-mode stop-points cache + search filter (Phase 2)
    // -------------------------------------------------------------------------

    /// Cold-warm must populate the cache with stations from every surfaced
    /// mode, not just tube. Without this fan-out, Overground-only stations
    /// (Hackney Central) and DLR-only stations (Beckton) are absent from
    /// the cache, search returns empty, and `allowed_line_ids_for` returns
    /// `{}` for them — silently disabling the defensive filter.
    #[tokio::test]
    async fn stop_points_cache_includes_overground_dlr_elizabeth_stations() {
        let client = real_client();
        client
            .warm_stop_points_cache()
            .await
            .expect("warm should succeed");

        let allowed_og = client.allowed_line_ids_for("910GHACKNYC").await.unwrap();
        assert!(
            allowed_og.contains("mildmay"),
            "Hackney Central should resolve to its Mildmay line via the cached \
             overground stop-points; got {allowed_og:?}"
        );

        let allowed_dlr = client.allowed_line_ids_for("940GZZDLBEC").await.unwrap();
        assert!(
            allowed_dlr.contains("dlr"),
            "Beckton DLR should resolve to its dlr line; got {allowed_dlr:?}"
        );
    }

    /// `search_stations` must return Overground-only stations now that the
    /// per-mode fan-out merges overground stop-points into the cache and
    /// the search filter accepts `910G*` ids.
    #[tokio::test]
    async fn search_stations_returns_overground_only_station() {
        let client = real_client();
        let results = client
            .search_stations("hackney central")
            .await
            .expect("hackney search should succeed");

        let hackney = results
            .iter()
            .find(|s| s.id == "910GHACKNYC")
            .unwrap_or_else(|| {
                panic!(
                    "Hackney Central must appear in results; got {:?}",
                    results.iter().map(|s| &s.id).collect::<Vec<_>>()
                )
            });
        let line_ids: Vec<&str> = hackney.lines.iter().map(|l| l.id.as_str()).collect();
        assert!(
            line_ids.contains(&"mildmay"),
            "Hackney Central must carry mildmay; got {line_ids:?}"
        );
    }

    /// Closes the latent DLR-only-station bug: the previous `940GZZLU` prefix
    /// filter silently excluded `940GZZDL*` stations from search.
    #[tokio::test]
    async fn search_stations_includes_dlr_only_station() {
        let client = real_client();
        let results = client
            .search_stations("beckton")
            .await
            .expect("beckton search should succeed");

        assert!(
            results.iter().any(|s| s.id == "940GZZDLBEC"),
            "Beckton DLR must appear in results; got {:?}",
            results.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }

    /// 910G ids overlap NaPTAN-wise with National Rail (Gatwick Express,
    /// Thameslink, Southern, etc.). The filter must admit only stops
    /// whose `modes` include `overground` or `elizabeth-line`. Synthetic
    /// fixture per-test so we have a deterministic NR-only entry.
    #[tokio::test]
    async fn search_stations_excludes_national_rail_only_910g_stations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sp_dir = dir.path().join("stop-points");
        fs::create_dir_all(&sp_dir).unwrap();
        let line_dir = dir.path().join("line-status");
        fs::create_dir_all(&line_dir).unwrap();

        // Tube fixture has one entry to satisfy cache warm; the test target
        // is an NR-only 910G entry in the overground fixture.
        let tube = serde_json::json!({
            "stopPoints": [
                {
                    "id": "940GZZLUTST",
                    "commonName": "Test Tube Station",
                    "lat": 51.5,
                    "lon": -0.1,
                    "modes": ["tube"],
                    "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["northern"]}],
                }
            ]
        });
        let og = serde_json::json!({
            "stopPoints": [
                {
                    "id": "910GGATWICKEXP",
                    "commonName": "Gatwick Express NR-Only Test",
                    "lat": 51.5,
                    "lon": -0.1,
                    "modes": ["national-rail"],
                    "lineModeGroups": [{"modeName": "national-rail", "lineIdentifier": ["gatwick-express"]}],
                }
            ]
        });
        fs::write(
            sp_dir.join("tube.json"),
            serde_json::to_string(&tube).unwrap(),
        )
        .unwrap();
        fs::write(
            sp_dir.join("overground.json"),
            serde_json::to_string(&og).unwrap(),
        )
        .unwrap();
        // Empty placeholders so the fan-out doesn't all-fail.
        fs::write(sp_dir.join("dlr.json"), r#"{"stopPoints":[]}"#).unwrap();
        fs::write(sp_dir.join("elizabeth-line.json"), r#"{"stopPoints":[]}"#).unwrap();
        for mode in ["tube", "overground", "dlr", "elizabeth-line"] {
            fs::write(line_dir.join(format!("{mode}.json")), "[]").unwrap();
        }

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("gatwick").await.unwrap();
        assert!(
            !results.iter().any(|s| s.id == "910GGATWICKEXP"),
            "NR-only 910G entry must be dropped by the search filter; got {:?}",
            results.iter().map(|s| &s.id).collect::<Vec<_>>()
        );
    }

    /// Hub aggregators (`HUB*`) and platform-level children (`9400*`,
    /// `4900*`, `2100*`) must remain excluded from search results
    /// regardless of which per-mode fixture they appear in.
    #[tokio::test]
    async fn search_stations_excludes_platform_children_and_hubs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sp_dir = dir.path().join("stop-points");
        fs::create_dir_all(&sp_dir).unwrap();
        let line_dir = dir.path().join("line-status");
        fs::create_dir_all(&line_dir).unwrap();

        let tube = serde_json::json!({
            "stopPoints": [
                // Canonical: must appear.
                {
                    "id": "940GZZLUTST",
                    "commonName": "Test Station",
                    "lat": 51.5, "lon": -0.1,
                    "modes": ["tube"],
                    "lineModeGroups": [{"modeName": "tube", "lineIdentifier": ["northern"]}],
                },
                // Platform children: must be dropped.
                {
                    "id": "9400ZZLUTST1",
                    "commonName": "Test Station Platform 1",
                    "lat": 51.5, "lon": -0.1,
                    "modes": ["tube"],
                    "lineModeGroups": [],
                },
                {
                    "id": "4900ZZLUTST2",
                    "commonName": "Test Station Bus Stop",
                    "lat": 51.5, "lon": -0.1,
                    "modes": ["tube"],
                    "lineModeGroups": [],
                },
            ]
        });
        let og = serde_json::json!({
            "stopPoints": [
                // Overground platform-level (NaptanRailEntrance): must be dropped.
                {
                    "id": "2100TSTPLT0",
                    "commonName": "Test OG Platform Station",
                    "lat": 51.5, "lon": -0.1,
                    "modes": ["overground"],
                    "lineModeGroups": [],
                },
                {
                    "id": "4900TSTOGENT",
                    "commonName": "Test OG Entrance",
                    "lat": 51.5, "lon": -0.1,
                    "modes": ["overground"],
                    "lineModeGroups": [],
                },
            ]
        });
        fs::write(
            sp_dir.join("tube.json"),
            serde_json::to_string(&tube).unwrap(),
        )
        .unwrap();
        fs::write(
            sp_dir.join("overground.json"),
            serde_json::to_string(&og).unwrap(),
        )
        .unwrap();
        fs::write(sp_dir.join("dlr.json"), r#"{"stopPoints":[]}"#).unwrap();
        fs::write(sp_dir.join("elizabeth-line.json"), r#"{"stopPoints":[]}"#).unwrap();
        for mode in ["tube", "overground", "dlr", "elizabeth-line"] {
            fs::write(line_dir.join(format!("{mode}.json")), "[]").unwrap();
        }

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("test").await.unwrap();
        let ids: Vec<&str> = results.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["940GZZLUTST"],
            "only the canonical 940GZZLU station should remain after filtering"
        );
    }

    /// When a station id appears in multiple per-mode feeds (TfL hub stations
    /// like Stratford show up in tube, DLR, overground, and elizabeth-line
    /// `/StopPoint/Mode/{mode}` responses with the same id), the merged cache
    /// must keep one entry whose `lines` list is the union — never two rows
    /// with the same id.
    #[tokio::test]
    async fn stop_points_cache_dedupes_station_id_across_modes_and_unions_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sp_dir = dir.path().join("stop-points");
        fs::create_dir_all(&sp_dir).unwrap();
        let line_dir = dir.path().join("line-status");
        fs::create_dir_all(&line_dir).unwrap();

        // Same canonical id appearing in two feeds with different modes/lines.
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
        fs::write(
            sp_dir.join("tube.json"),
            serde_json::to_string(&tube).unwrap(),
        )
        .unwrap();
        fs::write(
            sp_dir.join("overground.json"),
            serde_json::to_string(&og).unwrap(),
        )
        .unwrap();
        fs::write(sp_dir.join("dlr.json"), r#"{"stopPoints":[]}"#).unwrap();
        fs::write(sp_dir.join("elizabeth-line.json"), r#"{"stopPoints":[]}"#).unwrap();
        for mode in ["tube", "overground", "dlr", "elizabeth-line"] {
            fs::write(line_dir.join(format!("{mode}.json")), "[]").unwrap();
        }

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        let results = client.search_stations("stratford").await.unwrap();
        let matches: Vec<&Station> = results.iter().filter(|s| s.id == "940GZZLUSTR").collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one Stratford row after cross-mode dedupe"
        );
        let lines: std::collections::BTreeSet<&str> =
            matches[0].lines.iter().map(|l| l.id.as_str()).collect();
        let expected: std::collections::BTreeSet<&str> =
            ["central", "jubilee", "mildmay"].into_iter().collect();
        assert_eq!(
            lines, expected,
            "merged station must carry the union of tube + overground line ids"
        );
    }

    /// At multi-mode interchanges (Bank, Farringdon, …) the per-mode
    /// stop-points feeds each contribute their own canonical entry that
    /// shares a `hubNaptanCode` with its sibling (`940GZZLUBNK` /
    /// `940GZZDLBNK` both → `HUBBAN`). After the hub-merge step in
    /// `stop_points_cached` both entries also carry the same union of
    /// lines. The search dropdown MUST show ONE row per physical
    /// station, picking the tube canonical (`940GZZLU` prefix) when it
    /// exists, then DLR (`940GZZDL`), then Overground/Elizabeth (`910G`).
    #[tokio::test]
    async fn search_dedupes_multi_mode_interchange_to_one_row() {
        let client = real_client();

        let bank_results = client.search_stations("bank").await.unwrap();
        let bank_rows: Vec<&Station> = bank_results
            .iter()
            .filter(|s| s.hub_naptan_code.as_deref() == Some("HUBBAN"))
            .collect();
        assert_eq!(
            bank_rows.len(),
            1,
            "expected one Bank row; got {} ({:?})",
            bank_rows.len(),
            bank_rows
                .iter()
                .map(|s| (&s.id, &s.common_name))
                .collect::<Vec<_>>(),
        );
        assert!(
            bank_rows[0].id.starts_with("940GZZLU"),
            "tube canonical should win over DLR canonical at the same hub; got {:?}",
            bank_rows[0].id,
        );

        let farr_results = client.search_stations("farringdon").await.unwrap();
        let farr_rows: Vec<&Station> = farr_results
            .iter()
            .filter(|s| s.hub_naptan_code.as_deref() == Some("HUBZFD"))
            .collect();
        assert_eq!(
            farr_rows.len(),
            1,
            "expected one Farringdon row; got {} ({:?})",
            farr_rows.len(),
            farr_rows
                .iter()
                .map(|s| (&s.id, &s.common_name))
                .collect::<Vec<_>>(),
        );
        assert!(
            farr_rows[0].id.starts_with("940GZZLU"),
            "tube canonical should win over Elizabeth canonical at the same hub; got {:?}",
            farr_rows[0].id,
        );
    }

    /// Stale-while-revalidate: once the cache has data, `search_stations`
    /// MUST never block on a refresh — even if the entry is past the
    /// `STOP_POINTS_TTL`. The periodic background task in `lib.rs::run`
    /// owns refresh; user-facing calls just read whatever's cached.
    #[tokio::test]
    async fn search_stations_does_not_refetch_when_cache_is_stale_but_present() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        // Initial warm primes the cache.
        client
            .warm_stop_points_cache()
            .await
            .expect("warm should succeed");
        let after_warm = count.load(Ordering::SeqCst);

        // Force the cache into the stale window without invalidating it.
        client
            .__test_force_stale_stop_points_cache()
            .expect("test helper must be able to force-stale the cache");

        // A search MUST return immediately with the stale data and add
        // ZERO new fetches. Pre-SWR, this would block on a full
        // single-flighted refresh.
        let results = client.search_stations("belsize").await.unwrap();
        let after_search = count.load(Ordering::SeqCst);

        assert!(!results.is_empty(), "stale cache must still serve results");
        assert_eq!(
            after_warm, after_search,
            "search against stale-but-present cache must not refetch \
             (after warm: {after_warm}; after stale-search: {after_search})",
        );
    }

    /// `refresh_stop_points_cache` MUST force a fan-out + hub-merge even
    /// when the cache is fresh — it's the periodic background task's
    /// hook for keeping the cache up to date independently of user-
    /// facing calls.
    #[tokio::test]
    async fn refresh_stop_points_cache_forces_refetch_even_when_fresh() {
        let (http, count) = CountingTflHttp::new(FixtureTflHttp::new(workspace_fixtures_dir()));
        let client = TflClient::new(http);

        client
            .warm_stop_points_cache()
            .await
            .expect("warm should succeed");
        let after_warm = count.load(Ordering::SeqCst);

        // Cache is fresh, but `refresh_stop_points_cache` MUST refetch anyway.
        let n = client
            .refresh_stop_points_cache()
            .await
            .expect("refresh should succeed");
        let after_refresh = count.load(Ordering::SeqCst);

        assert!(n > 100, "expect populated station count");
        assert!(
            after_refresh > after_warm,
            "refresh_stop_points_cache must trigger network calls even on a fresh cache \
             (after warm: {after_warm}; after refresh: {after_refresh})",
        );
    }

    /// **Invariant: hub lookup survives the stop-points TTL.** When the
    /// 15-min TTL expires, the cached station list goes "stale" but
    /// remains a perfectly usable source of `hub_naptan_code` and
    /// `lines` data — TfL station metadata changes infrequently and the
    /// next caller of `stop_points_cached` will refresh on its own
    /// schedule. Without this guarantee, the first stream tick after
    /// expiry loses the hub-merge fan-out for arrivals (Bank, Euston,
    /// Whitechapel siblings never fetched) and the user's chip filter
    /// silently sees zero matching arrivals at hub stations.
    ///
    /// Guards `read_cache_any` vs the older `read_fresh_cache` for
    /// `resolve_arrival_ids` and `allowed_line_ids_for`.
    #[tokio::test]
    async fn allowed_line_ids_for_serves_stale_cache_past_ttl() {
        let client = real_client();
        client
            .warm_stop_points_cache()
            .await
            .expect("warm should succeed");

        // Bank tube parent has hub_naptan_code = HUBBAN; after warm +
        // hub-merge its `lines` field includes the DLR sibling's lines.
        let allowed_fresh = client.allowed_line_ids_for("940GZZLUBNK").await.unwrap();
        assert!(
            allowed_fresh.contains("dlr"),
            "fresh cache: Bank tube parent should know about DLR via hub-merge; got {allowed_fresh:?}"
        );

        // Force the cache into the "stale" state without invalidating it.
        // We do this by overwriting `fetched_at` to a time well in the past;
        // exposed only via the test-helper since production code relies on
        // the wall-clock TTL.
        client
            .__test_force_stale_stop_points_cache()
            .expect("test helper must be able to force-stale the cache");

        // Now `read_fresh_cache` returns None, but `read_cache_any` keeps
        // returning the prior data. allowed_line_ids_for MUST survive.
        let allowed_stale = client.allowed_line_ids_for("940GZZLUBNK").await.unwrap();
        assert!(
            allowed_stale.contains("dlr"),
            "stale cache: Bank tube parent must STILL know about DLR via hub-merge; \
             without `read_cache_any` this set is empty and the defensive filter \
             drops every legitimate DLR arrival until the next refresh. got {allowed_stale:?}"
        );
        assert_eq!(
            allowed_stale, allowed_fresh,
            "stale-but-cached lookup must return identical data to fresh"
        );
    }

    /// Stations without a `hub_naptan_code` (Hampstead Heath, Belsize Park,
    /// most single-mode stops) MUST pass through the dedupe unchanged.
    #[tokio::test]
    async fn search_keeps_non_hub_stations_individually() {
        let client = real_client();
        let results = client.search_stations("hackney central").await.unwrap();
        assert!(
            results.iter().any(|s| s.id == "910GHACKNYC"),
            "Hackney Central (no hub) should not be deduped away; got {:?}",
            results.iter().map(|s| &s.id).collect::<Vec<_>>(),
        );
    }

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
        // TfL's `lineModeGroups` uses the bare id `"elizabeth"` for the
        // Elizabeth line; `"elizabeth-line"` is the historical mode-name
        // alias and is normalised by `pretty_line_name` / `format.ts`
        // to the same display label and CSS variable.
        assert!(
            line_ids.contains(&"elizabeth"),
            "TCR must include Elizabeth chip after hub merge; got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"central"),
            "TCR must still include tube lines; got {line_ids:?}"
        );
    }

    /// Whitechapel sits at the intersection of the District + Hammersmith &
    /// City tube, the Elizabeth line, and the Windrush Overground line
    /// (formerly East London Line). It does NOT serve Mildmay (which is
    /// the North London Line). This test guards the cross-mode hub merge
    /// for an Overground line that was renamed in November 2024.
    #[tokio::test]
    async fn search_whitechapel_includes_elizabeth_and_windrush_chips() {
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
            line_ids.contains(&"elizabeth"),
            "Whitechapel must include Elizabeth chip; got {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"windrush"),
            "Whitechapel must include Windrush (Overground / East London) chip; got {line_ids:?}"
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
