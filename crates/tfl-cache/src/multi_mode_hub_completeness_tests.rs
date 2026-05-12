//! Multi-mode hub completeness — the permanent regression harness.
//!
//! Pins the `(station_id, expected_lines_superset)` contract for every
//! interchange in [`crate::cache::CANONICAL_MULTI_MODE_HUBS`]: Tottenham
//! Court Road, Bank, Liverpool Street, Stratford, Canary Wharf, Whitechapel,
//! Paddington, Farringdon. Each scenario builds hermetic synthetic fixtures
//! (per-mode stop-points feeds + the hub detail JSON), warms the cache, and
//! asserts `allowed_line_ids_for(station_id)` is a superset of the expected
//! lines.
//!
//! **Why this file exists.** Every few weeks since 2026-04-29 someone has
//! had to fix "Elizabeth missing at TCR" or "DLR missing at Bank" or a
//! variant. The fixes have been single-station patches in `cache.rs`,
//! `direction.rs`, the chip migration, the canonicalisation rules — each
//! one defensible in isolation but easy to undo because the contract has
//! lived implicitly in three or four places. This harness makes the
//! contract explicit. If anyone reverts a hub-merge fix, breaks a mode
//! filter, drops a `lineModeGroups` parse, or canonicalises a line id
//! incorrectly, the matching scenario goes RED at `cargo test --workspace`.
//!
//! **What this file does NOT cover.** Live TfL data drift (e.g. TfL
//! genuinely returning 404 for a hub for a few hours). That belongs in the
//! Layer 2 live test (`crates/tfl-cache/tests/live_hub_completeness.rs`)
//! which hits the real API and is wired into the iOS bump-core recipe.
//!
//! **Adding a new interchange.** Append one entry to
//! `CANONICAL_MULTI_MODE_HUBS` in `cache.rs`. The test scenario itself
//! does have to be added by hand here — synthetic fixture shapes vary
//! per-station — but the constant is the contract everyone references.

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::cache::{TflClient, CANONICAL_MULTI_MODE_HUBS};
    use tfl_client::fixture::FixtureTflHttp;

    // -------------------------------------------------------------------------
    // Fixture-writing helpers
    // -------------------------------------------------------------------------

    /// Write per-mode `stop-points/{mode}.json` fixtures plus
    /// `stop-point/{hub_id}.json` hub-detail fixtures into `dir`.
    ///
    /// Modes not listed in `stops_per_mode` get an empty `stopPoints` array
    /// rather than being absent, because the per-mode warm fan-out treats
    /// an absent fixture as `TflError::NotFound` — that triggers the same
    /// retry-and-skip path live TfL would, but produces eprintln noise that
    /// pollutes a clean `cargo test` run. Empty-array stubs keep things
    /// hermetic and quiet.
    fn write_multi_mode_fixtures(
        dir: &Path,
        stops_per_mode: &[(&str, serde_json::Value)],
        hubs: &[(&str, serde_json::Value)],
    ) {
        let sp_dir = dir.join("stop-points");
        std::fs::create_dir_all(&sp_dir).unwrap();

        // Empty-array stubs for any mode not explicitly populated.
        let provided: std::collections::HashSet<&str> =
            stops_per_mode.iter().map(|(m, _)| *m).collect();
        for mode in crate::cache::SUPPORTED_MODES {
            if !provided.contains(*mode) {
                let path = sp_dir.join(format!("{mode}.json"));
                std::fs::write(
                    &path,
                    serde_json::to_string(&serde_json::json!({
                        "total": 0,
                        "stopPoints": [],
                    }))
                    .unwrap(),
                )
                .unwrap();
            }
        }

        for (mode, body) in stops_per_mode {
            let path = sp_dir.join(format!("{mode}.json"));
            std::fs::write(&path, serde_json::to_string(body).unwrap()).unwrap();
        }

        let hub_dir = dir.join("stop-point");
        std::fs::create_dir_all(&hub_dir).unwrap();
        for (hub_id, body) in hubs {
            let path = hub_dir.join(format!("{hub_id}.json"));
            std::fs::write(&path, serde_json::to_string(body).unwrap()).unwrap();
        }
    }

    /// Synthesise a stop-point JSON entry. Mirrors the fields the
    /// production deserialiser reads from `Station::Deserialize`
    /// (`crates/tfl-domain/src/types.rs:198–272`). `lineModeGroups` is one
    /// group per `mode` carrying `lines` — the simplest shape that
    /// exercises the merge path.
    fn synth_station(
        id: &str,
        hub: &str,
        modes: &[&str],
        lines: &[&str],
        lat: f64,
        lon: f64,
    ) -> serde_json::Value {
        let mode_groups: Vec<_> = modes
            .iter()
            .map(|m| serde_json::json!({"modeName": m, "lineIdentifier": lines}))
            .collect();
        serde_json::json!({
            "id": id,
            "commonName": format!("{id} synthetic"),
            "modes": modes,
            "hubNaptanCode": hub,
            "lat": lat,
            "lon": lon,
            "lineModeGroups": mode_groups,
        })
    }

    /// Synthesise a hub-child JSON entry as it appears in
    /// `/StopPoint/{HUBxxx}.children[]`. The `modes` field gates the outer
    /// filter in `hub_lines_cached`; the inner `lineModeGroups[].modeName`
    /// gates the per-line projection.
    fn synth_hub_child(id: &str, modes: &[&str], lines: &[&str]) -> serde_json::Value {
        let mode_name = modes.first().copied().unwrap_or("");
        serde_json::json!({
            "id": id,
            "modes": modes,
            "lineModeGroups": [{"modeName": mode_name, "lineIdentifier": lines}],
        })
    }

    /// Wrap a `children` array into a hub StopPoint detail document.
    fn synth_hub_doc(id: &str, children: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({"id": id, "children": children})
    }

    /// Build a client over hermetic fixtures, warm, and assert that
    /// `station_id`'s allowed-line set is a superset of `expected_lines`.
    /// The error message dumps the actual set so a CI failure tells the
    /// reader exactly what's missing.
    async fn assert_hub_serves_all_expected_lines(
        case: &str,
        stops_per_mode: &[(&str, serde_json::Value)],
        hubs: &[(&str, serde_json::Value)],
        station_id: &str,
        expected_lines: &[&str],
    ) {
        let dir = tempfile::tempdir().unwrap();
        write_multi_mode_fixtures(dir.path(), stops_per_mode, hubs);

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        client
            .warm_stop_points_cache()
            .await
            .unwrap_or_else(|e| panic!("[{case}] warm should succeed against fixtures: {e}"));

        let allowed = client
            .allowed_line_ids_for(station_id)
            .await
            .unwrap_or_else(|e| panic!("[{case}] allowed_line_ids_for should succeed: {e}"));

        let mut missing: Vec<&str> = Vec::new();
        for line in expected_lines {
            if !allowed.contains(*line) {
                missing.push(line);
            }
        }
        assert!(
            missing.is_empty(),
            "[{case}] {station_id} must serve {expected_lines:?} after warm; \
             missing: {missing:?}; got allowed set: {actual:?}. \
             Likely cause: the hub-merge step in `stop_points_cached` lost \
             the {missing:?} line(s) — check `hub_lines_cached` and the \
             outer mode filter on hub children.",
            actual = {
                let mut v: Vec<&str> = allowed.iter().map(String::as_str).collect();
                v.sort();
                v
            },
        );
    }

    // -------------------------------------------------------------------------
    // Per-canonical-hub scenarios
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn tcr_includes_central_northern_elizabeth() {
        // TCR canonical id `940GZZLUTCR` only carries tube lines in its own
        // entry. Elizabeth comes via the sibling `910GTOTCTRD` under the
        // shared hub `HUBTCR` — exactly the path that has historically
        // failed silently when `hub_lines_cached` swallowed errors.
        assert_hub_serves_all_expected_lines(
            "TCR",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLUTCR", "HUBTCR", &["tube"],
                                &["central", "northern"], 51.5160, -0.1310,
                            ),
                        ],
                    }),
                ),
                (
                    "elizabeth-line",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GTOTCTRD", "HUBTCR", &["elizabeth-line"],
                                &["elizabeth"], 51.5160, -0.1310,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBTCR",
                synth_hub_doc(
                    "HUBTCR",
                    vec![
                        synth_hub_child("940GZZLUTCR", &["tube"], &["central", "northern"]),
                        synth_hub_child("910GTOTCTRD", &["elizabeth-line"], &["elizabeth"]),
                    ],
                ),
            )],
            "940GZZLUTCR",
            &["central", "northern", "elizabeth"],
        )
        .await;
    }

    #[tokio::test]
    async fn bank_includes_central_northern_waterloo_city_dlr() {
        // Bank's canonical search id `940GZZLUBNK` is tube-only; DLR is a
        // sibling stop-point `940GZZDLBNK` under `HUBBAN`. Same hub-merge
        // path as TCR but a different mode pair.
        assert_hub_serves_all_expected_lines(
            "Bank",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLUBNK", "HUBBAN", &["tube"],
                                &["central", "northern", "waterloo-city"], 51.51225, -0.087792,
                            ),
                        ],
                    }),
                ),
                (
                    "dlr",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZDLBNK", "HUBBAN", &["dlr"],
                                &["dlr"], 51.51225, -0.087792,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBBAN",
                synth_hub_doc(
                    "HUBBAN",
                    vec![
                        synth_hub_child(
                            "940GZZLUBNK",
                            &["tube"],
                            &["central", "northern", "waterloo-city"],
                        ),
                        synth_hub_child("940GZZDLBNK", &["dlr"], &["dlr"]),
                    ],
                ),
            )],
            "940GZZLUBNK",
            &["central", "northern", "waterloo-city", "dlr"],
        )
        .await;
    }

    #[tokio::test]
    async fn bond_street_includes_central_jubilee_elizabeth() {
        // Bond Street's canonical search id `940GZZLUBND` carries Central
        // + Jubilee in the tube feed; Elizabeth comes via sibling
        // `910GBONDST` under shared hub `HUBBDS`. Identical hub-merge
        // contract to TCR — surfaced as a TestFlight regression on
        // 2026-05-08 because the original canonical list missed it.
        assert_hub_serves_all_expected_lines(
            "Bond Street",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLUBND", "HUBBDS", &["tube"],
                                &["central", "jubilee"], 51.514304, -0.149723,
                            ),
                        ],
                    }),
                ),
                (
                    "elizabeth-line",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GBONDST", "HUBBDS", &["elizabeth-line"],
                                &["elizabeth"], 51.514304, -0.149723,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBBDS",
                synth_hub_doc(
                    "HUBBDS",
                    vec![
                        synth_hub_child("940GZZLUBND", &["tube"], &["central", "jubilee"]),
                        synth_hub_child("910GBONDST", &["elizabeth-line"], &["elizabeth"]),
                    ],
                ),
            )],
            "940GZZLUBND",
            &["central", "jubilee", "elizabeth"],
        )
        .await;
    }

    #[tokio::test]
    async fn liverpool_street_includes_tube_elizabeth_weaver() {
        // The most-multi-mode interchange we surface: tube (×4) + Elizabeth
        // + a named Overground line via the post-Nov-2024 rename. Pinning
        // both `elizabeth` and `weaver` here defends against (a) the
        // hub-merge silent-fail and (b) the legacy `london-overground`
        // umbrella id sneaking back in.
        assert_hub_serves_all_expected_lines(
            "Liverpool Street",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLULVT", "HUBLVT", &["tube"],
                                &["central", "circle", "hammersmith-city", "metropolitan"],
                                51.5178, -0.0823,
                            ),
                        ],
                    }),
                ),
                (
                    "elizabeth-line",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GLIVST", "HUBLVT", &["elizabeth-line"],
                                &["elizabeth"], 51.5178, -0.0823,
                            ),
                        ],
                    }),
                ),
                (
                    "overground",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GLIVSTLL", "HUBLVT", &["overground"],
                                &["weaver"], 51.5178, -0.0823,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBLVT",
                synth_hub_doc(
                    "HUBLVT",
                    vec![
                        synth_hub_child(
                            "940GZZLULVT",
                            &["tube"],
                            &["central", "circle", "hammersmith-city", "metropolitan"],
                        ),
                        synth_hub_child("910GLIVST", &["elizabeth-line"], &["elizabeth"]),
                        synth_hub_child("910GLIVSTLL", &["overground"], &["weaver"]),
                    ],
                ),
            )],
            "940GZZLULVT",
            &[
                "central",
                "circle",
                "hammersmith-city",
                "metropolitan",
                "elizabeth",
                "weaver",
            ],
        )
        .await;
    }

    #[tokio::test]
    async fn stratford_includes_central_jubilee_dlr_elizabeth_mildmay() {
        // Three non-tube modes (DLR, Elizabeth, Overground/Mildmay) all on
        // one hub. If any of the three fan-out merges drops, this test
        // names which.
        assert_hub_serves_all_expected_lines(
            "Stratford",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLUSTD", "HUBSTD", &["tube"],
                                &["central", "jubilee"], 51.5416, -0.0042,
                            ),
                        ],
                    }),
                ),
                (
                    "dlr",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZDLSTD", "HUBSTD", &["dlr"],
                                &["dlr"], 51.5416, -0.0042,
                            ),
                        ],
                    }),
                ),
                (
                    "elizabeth-line",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GSTFD", "HUBSTD", &["elizabeth-line"],
                                &["elizabeth"], 51.5416, -0.0042,
                            ),
                        ],
                    }),
                ),
                (
                    "overground",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GSTFDLL", "HUBSTD", &["overground"],
                                &["mildmay"], 51.5416, -0.0042,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBSTD",
                synth_hub_doc(
                    "HUBSTD",
                    vec![
                        synth_hub_child("940GZZLUSTD", &["tube"], &["central", "jubilee"]),
                        synth_hub_child("940GZZDLSTD", &["dlr"], &["dlr"]),
                        synth_hub_child("910GSTFD", &["elizabeth-line"], &["elizabeth"]),
                        synth_hub_child("910GSTFDLL", &["overground"], &["mildmay"]),
                    ],
                ),
            )],
            "940GZZLUSTD",
            &["central", "jubilee", "dlr", "elizabeth", "mildmay"],
        )
        .await;
    }

    #[tokio::test]
    async fn canary_wharf_includes_jubilee_dlr_elizabeth() {
        assert_hub_serves_all_expected_lines(
            "Canary Wharf",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLUCYF", "HUBCWX", &["tube"],
                                &["jubilee"], 51.5051, -0.0179,
                            ),
                        ],
                    }),
                ),
                (
                    "dlr",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZDLCAW", "HUBCWX", &["dlr"],
                                &["dlr"], 51.5051, -0.0179,
                            ),
                        ],
                    }),
                ),
                (
                    "elizabeth-line",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GCANWRFE", "HUBCWX", &["elizabeth-line"],
                                &["elizabeth"], 51.5051, -0.0179,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBCWX",
                synth_hub_doc(
                    "HUBCWX",
                    vec![
                        synth_hub_child("940GZZLUCYF", &["tube"], &["jubilee"]),
                        synth_hub_child("940GZZDLCAW", &["dlr"], &["dlr"]),
                        synth_hub_child("910GCANWRFE", &["elizabeth-line"], &["elizabeth"]),
                    ],
                ),
            )],
            "940GZZLUCYF",
            &["jubilee", "dlr", "elizabeth"],
        )
        .await;
    }

    #[tokio::test]
    async fn whitechapel_includes_district_h_c_elizabeth_mildmay_windrush() {
        // Two named Overground lines (Mildmay + Windrush) on the same hub.
        // Defends against any future lazy-collapse of the OG named lines
        // back to a single sibling entry that would lose one of them.
        assert_hub_serves_all_expected_lines(
            "Whitechapel",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLUWCL", "HUBWCL", &["tube"],
                                &["district", "hammersmith-city"], 51.5194, -0.0612,
                            ),
                        ],
                    }),
                ),
                (
                    "elizabeth-line",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GWHCHPL", "HUBWCL", &["elizabeth-line"],
                                &["elizabeth"], 51.5194, -0.0612,
                            ),
                        ],
                    }),
                ),
                (
                    "overground",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GWHCHPLOG", "HUBWCL", &["overground"],
                                &["mildmay", "windrush"], 51.5194, -0.0612,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBWCL",
                synth_hub_doc(
                    "HUBWCL",
                    vec![
                        synth_hub_child(
                            "940GZZLUWCL",
                            &["tube"],
                            &["district", "hammersmith-city"],
                        ),
                        synth_hub_child("910GWHCHPL", &["elizabeth-line"], &["elizabeth"]),
                        synth_hub_child("910GWHCHPLOG", &["overground"], &["mildmay", "windrush"]),
                    ],
                ),
            )],
            "940GZZLUWCL",
            &[
                "district",
                "hammersmith-city",
                "elizabeth",
                "mildmay",
                "windrush",
            ],
        )
        .await;
    }

    #[tokio::test]
    async fn paddington_includes_bakerloo_circle_district_h_c_elizabeth() {
        // Four tube lines on one parent + Elizabeth from the sibling.
        // Largest tube-line set in our canonical list — defends against
        // any future bug that slices the tube `lineModeGroups` array.
        assert_hub_serves_all_expected_lines(
            "Paddington",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLUPAC", "HUBPAD", &["tube"],
                                &["bakerloo", "circle", "district", "hammersmith-city"],
                                51.5154, -0.1755,
                            ),
                        ],
                    }),
                ),
                (
                    "elizabeth-line",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GPADTLL", "HUBPAD", &["elizabeth-line"],
                                &["elizabeth"], 51.5154, -0.1755,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBPAD",
                synth_hub_doc(
                    "HUBPAD",
                    vec![
                        synth_hub_child(
                            "940GZZLUPAC",
                            &["tube"],
                            &["bakerloo", "circle", "district", "hammersmith-city"],
                        ),
                        synth_hub_child("910GPADTLL", &["elizabeth-line"], &["elizabeth"]),
                    ],
                ),
            )],
            "940GZZLUPAC",
            &[
                "bakerloo",
                "circle",
                "district",
                "hammersmith-city",
                "elizabeth",
            ],
        )
        .await;
    }

    #[tokio::test]
    async fn farringdon_includes_circle_h_c_metropolitan_elizabeth() {
        assert_hub_serves_all_expected_lines(
            "Farringdon",
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZLUFRD", "HUBFFD", &["tube"],
                                &["circle", "hammersmith-city", "metropolitan"],
                                51.5203, -0.1053,
                            ),
                        ],
                    }),
                ),
                (
                    "elizabeth-line",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "910GFRNDXR", "HUBFFD", &["elizabeth-line"],
                                &["elizabeth"], 51.5203, -0.1053,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBFFD",
                synth_hub_doc(
                    "HUBFFD",
                    vec![
                        synth_hub_child(
                            "940GZZLUFRD",
                            &["tube"],
                            &["circle", "hammersmith-city", "metropolitan"],
                        ),
                        synth_hub_child("910GFRNDXR", &["elizabeth-line"], &["elizabeth"]),
                    ],
                ),
            )],
            "940GZZLUFRD",
            &["circle", "hammersmith-city", "metropolitan", "elizabeth"],
        )
        .await;
    }

    // -------------------------------------------------------------------------
    // Negative — non-hub stations must NOT inherit unrelated lines
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn belsize_park_does_not_inherit_unrelated_hub_lines() {
        // Belsize Park is a single-mode tube stop with no `hubNaptanCode`.
        // It must NOT pick up DLR / Elizabeth / Overground lines from any
        // other hub even when they're present in the warm. Defends against
        // a future bug that would over-merge by lat/lon proximity or some
        // other heuristic that ignores the hub partition.
        let dir = tempfile::tempdir().unwrap();
        write_multi_mode_fixtures(
            dir.path(),
            &[
                (
                    "tube",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            // No hubNaptanCode — Belsize Park stands alone.
                            serde_json::json!({
                                "id": "940GZZLUBZP",
                                "commonName": "Belsize Park Underground Station",
                                "modes": ["tube"],
                                "lat": 51.5505,
                                "lon": -0.1644,
                                "lineModeGroups": [
                                    {"modeName": "tube", "lineIdentifier": ["northern"]},
                                ],
                            }),
                        ],
                    }),
                ),
                (
                    "dlr",
                    serde_json::json!({
                        "total": 1,
                        "stopPoints": [
                            synth_station(
                                "940GZZDLBNK", "HUBBAN", &["dlr"],
                                &["dlr"], 51.51225, -0.087792,
                            ),
                        ],
                    }),
                ),
            ],
            &[(
                "HUBBAN",
                synth_hub_doc(
                    "HUBBAN",
                    vec![synth_hub_child("940GZZDLBNK", &["dlr"], &["dlr"])],
                ),
            )],
        );

        let client = TflClient::new(FixtureTflHttp::new(dir.path()));
        client.warm_stop_points_cache().await.unwrap();
        let allowed = client.allowed_line_ids_for("940GZZLUBZP").await.unwrap();

        let actual: Vec<&str> = {
            let mut v: Vec<&str> = allowed.iter().map(String::as_str).collect();
            v.sort();
            v
        };
        assert_eq!(
            actual,
            vec!["northern"],
            "Belsize Park has no hub partner — must not inherit lines from any \
             other hub. Got: {actual:?}",
        );
    }

    // -------------------------------------------------------------------------
    // Coverage of CANONICAL_MULTI_MODE_HUBS — every entry must have a test
    // -------------------------------------------------------------------------
    //
    // The const is the regression contract; if someone adds a new
    // interchange but forgets the matching test, that interchange ships
    // unprotected. This test fails if the count drifts from what we have
    // scenarios for. Updating this number is intentional: it forces a
    // human to acknowledge that they added (or removed) a test.

    #[test]
    fn canonical_multi_mode_hubs_count_matches_test_scenarios() {
        const SCENARIO_COUNT: usize = 9;
        assert_eq!(
            CANONICAL_MULTI_MODE_HUBS.len(),
            SCENARIO_COUNT,
            "If you added a hub to CANONICAL_MULTI_MODE_HUBS, also add a \
             matching #[tokio::test] in this file (and its mirror in \
             crates/tfl-cache/tests/live_hub_completeness.rs + \
             tubbie-ios/crates/tfl-ffi/tests/live_hub_completeness.rs), \
             then bump SCENARIO_COUNT here.",
        );
    }
}
