// fixture-recorder: hits the live TfL API and writes fixtures + metadata to disk.
//
// Usage: cargo run -p fixture-recorder --release
// Or via just: just record-fixtures
//
// SECURITY: app_key is NEVER written to fixtures or metadata.
// The sanitize_url() function strips ?app_key=... from all recorded URLs.
// This is verified by a unit test below.

#![deny(unsafe_code)]

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TFL_BASE: &str = "https://api.tfl.gov.uk";

/// Sidecar metadata written alongside each fixture.
#[derive(Serialize)]
struct FixtureMeta {
    recorded_at: String,
    /// Path + query of the TfL URL, with `app_key` stripped.
    url: String,
    /// From TfL's `Vary`/`ETag` response headers, if available.
    tfl_api_version: Option<String>,
}

/// Strip `app_key` from a URL's query string.
///
/// The returned string is the path + sanitized query (no scheme/host).
/// If no `app_key` param is present the function returns the path + query unchanged.
///
/// # Examples
/// ```
/// # use fixture_recorder::sanitize_url;  // (won't work outside bin context)
/// // "https://api.tfl.gov.uk/StopPoint/940GZZLUBZP/Arrivals?app_key=DEADBEEF"
/// // → "/StopPoint/940GZZLUBZP/Arrivals"
/// ```
pub fn sanitize_url(url: &str) -> String {
    // Split on '?' to separate path from query string.
    let (path, query_opt) = match url.find('?') {
        Some(pos) => (&url[..pos], Some(&url[pos + 1..])),
        None => (url, None),
    };

    // Strip scheme+host if present to get just the path portion.
    let path_only = if let Some(stripped) = path.strip_prefix("https://") {
        stripped
            .find('/')
            .map(|i| &stripped[i..])
            .unwrap_or(stripped)
    } else if let Some(stripped) = path.strip_prefix("http://") {
        stripped
            .find('/')
            .map(|i| &stripped[i..])
            .unwrap_or(stripped)
    } else {
        path
    };

    let Some(query) = query_opt else {
        return path_only.to_string();
    };

    // Rebuild the query string without app_key.
    let cleaned: Vec<&str> = query
        .split('&')
        .filter(|param| {
            let key = param.split('=').next().unwrap_or("");
            key != "app_key"
        })
        .collect();

    if cleaned.is_empty() {
        path_only.to_string()
    } else {
        format!("{path_only}?{}", cleaned.join("&"))
    }
}

// ---------------------------------------------------------------------------
// Stop-points trim logic
// ---------------------------------------------------------------------------

/// Trim a TfL `/StopPoint/Mode/{mode}` response to a compact representation.
///
/// The raw TfL response is multi-MB per request, bloated with unused fields
/// (`additionalProperties`, `children`, `lineGroup`, etc.). This function
/// discards everything `search_stations` doesn't need and keeps only
/// the fields required for station search and line-service display:
///
/// Per stop point: `id`, `commonName`, `lat`, `lon`, `modes`,
/// `lineModeGroups[].lineIdentifier`, **and `hubNaptanCode` when present**.
/// `hubNaptanCode` is what `stop_points_cached` uses to fan out the
/// hub-merge fetch that brings DLR / Overground / Elizabeth siblings into
/// a tube hub's `Station.lines` (Bank → DLR, Whitechapel → Mildmay +
/// Elizabeth, etc.). Stripping it here previously meant the hub-merge
/// silently no-op'd for every freshly-recorded fixture and the multi-mode
/// chip tests went red for reasons unrelated to the code under test.
///
/// Stop points whose `id` starts with `"HUB"` are excluded (hub aggregates,
/// not real stations) and stops that don't serve any mode in
/// [`SURFACED_MODES`] are dropped. The paginated envelope (`$type`, `total`,
/// `stopPoints`) is preserved. Used uniformly across the per-mode
/// stop-points fetches (tube / overground / dlr / elizabeth-line).
///
/// # Implementation note
/// Always-on (not a flag): the stop-points fixture is the only one with this
/// bloat problem. Adding a conditional flag would complicate the recorder for
/// no practical benefit — every re-recording of stop-points should produce the
/// compact form.
pub fn trim_stop_points(raw: &Value) -> Value {
    let obj = match raw.as_object() {
        Some(o) => o,
        None => return raw.clone(),
    };

    let stop_points: Vec<Value> = obj
        .get("stopPoints")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(trim_stop_point).collect())
        .unwrap_or_default();

    serde_json::json!({
        "$type": obj.get("$type").cloned().unwrap_or(Value::Null),
        "total": obj.get("total").cloned().unwrap_or(Value::Null),
        "stopPoints": stop_points,
    })
}

/// Modes we trim stop-point fixtures down to. Stops not serving any of
/// these are dropped on disk so the offline test fixtures stay focused
/// on what tubbie actually surfaces. Mirrors `tfl_client::SUPPORTED_MODES`.
const SURFACED_MODES: &[&str] = &["tube", "dlr", "overground", "elizabeth-line"];

/// Trim a single stop point. Returns `None` if the stop should be dropped
/// (e.g. `id` starts with `"HUB"`, or `modes` doesn't include any of
/// `SURFACED_MODES`).
fn trim_stop_point(sp: &Value) -> Option<Value> {
    let obj = sp.as_object()?;

    let id = obj.get("id")?.as_str()?;

    // Drop hub aggregates — they don't represent real stations.
    if id.starts_with("HUB") {
        return None;
    }

    // Drop stop points that don't serve any mode tubbie surfaces.
    let modes: Vec<Value> = obj
        .get("modes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let serves_surfaced_mode = modes
        .iter()
        .filter_map(|m| m.as_str())
        .any(|m| SURFACED_MODES.contains(&m));
    if !serves_surfaced_mode {
        return None;
    }

    // Keep only lineIdentifier from each lineModeGroup.
    let line_mode_groups: Vec<Value> = obj
        .get("lineModeGroups")
        .and_then(|v| v.as_array())
        .map(|groups| {
            groups
                .iter()
                .map(|g| {
                    serde_json::json!({
                        "lineIdentifier": g.get("lineIdentifier").cloned().unwrap_or(Value::Array(vec![]))
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // hubNaptanCode is preserved when present, dropped when absent —
    // matches `RawStation.hub_naptan_code: Option<String>` semantics so
    // the runtime cache sees `None` for non-hub stops, not an empty string.
    let mut output = serde_json::json!({
        "id": id,
        "commonName": obj.get("commonName").cloned().unwrap_or(Value::Null),
        "lat": obj.get("lat").cloned().unwrap_or(Value::Null),
        "lon": obj.get("lon").cloned().unwrap_or(Value::Null),
        "modes": modes,
        "lineModeGroups": line_mode_groups,
    });
    if let Some(hub) = obj
        .get("hubNaptanCode")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        output
            .as_object_mut()
            .expect("trim output is an object")
            .insert("hubNaptanCode".to_string(), Value::String(hub.to_string()));
    }
    Some(output)
}

// ---------------------------------------------------------------------------
// Atomic file write helpers
// ---------------------------------------------------------------------------

/// Write `contents` to `dest` atomically using a `.tmp` sibling + rename.
///
/// Writes to `{dest}.tmp`, then `std::fs::rename` to `dest`. Because both
/// paths are on the same filesystem (inside the fixtures directory), the
/// rename is atomic at the OS level — a Ctrl-C between write and rename
/// leaves only the `.tmp` file, never a truncated final file.
pub fn write_atomic(dest: &Path, contents: &[u8]) -> std::io::Result<()> {
    let tmp_path = tmp_path_for(dest);
    // Write to .tmp first.
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(contents)?;
        f.flush()?;
        // f is dropped (and thus closed) here before the rename.
    }
    // Atomic rename to final path.
    std::fs::rename(&tmp_path, dest)?;
    Ok(())
}

/// Returns the `.tmp` sibling path for `dest` (same dir, `.tmp` suffix appended).
fn tmp_path_for(dest: &Path) -> PathBuf {
    let mut tmp = dest.to_path_buf();
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    tmp.set_file_name(name);
    tmp
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

struct Endpoint {
    /// Slug used for directory + meta (e.g. "arrivals").
    slug: &'static str,
    /// Filename stem (e.g. "940GZZLUBZP" → "940GZZLUBZP.json").
    id: &'static str,
    /// TfL API URL path (no base, no app_key).
    path: &'static str,
}

const ENDPOINTS: &[Endpoint] = &[
    // Tube arrivals (existing fixtures used by board_service_tests).
    Endpoint {
        slug: "arrivals",
        id: "940GZZLUBZP",
        path: "/StopPoint/940GZZLUBZP/Arrivals",
    },
    Endpoint {
        slug: "arrivals",
        id: "940GZZLUKSX",
        path: "/StopPoint/940GZZLUKSX/Arrivals",
    },
    Endpoint {
        slug: "arrivals",
        id: "940GZZLUBNK",
        path: "/StopPoint/940GZZLUBNK/Arrivals",
    },
    Endpoint {
        slug: "arrivals",
        id: "940GZZLUOXC",
        path: "/StopPoint/940GZZLUOXC/Arrivals",
    },
    // Overground arrivals — Hackney Central is Mildmay-only, used as the
    // canonical Overground-only station test target.
    Endpoint {
        slug: "arrivals",
        id: "910GHACKNYC",
        path: "/StopPoint/910GHACKNYC/Arrivals",
    },
    // Highbury & Islington tube parent — exercises hub-merge with
    // Overground siblings (Mildmay + Windrush) at a mixed-mode station.
    Endpoint {
        slug: "arrivals",
        id: "940GZZLUHAI",
        path: "/StopPoint/940GZZLUHAI/Arrivals",
    },
    // Per-mode line-status. The client now fans out across all four modes
    // in parallel; each fixture must be present for the merged cache to
    // populate fully in offline tests.
    Endpoint {
        slug: "line-status",
        id: "tube",
        path: "/Line/Mode/tube/Status",
    },
    Endpoint {
        slug: "line-status",
        id: "overground",
        path: "/Line/Mode/overground/Status",
    },
    Endpoint {
        slug: "line-status",
        id: "dlr",
        path: "/Line/Mode/dlr/Status",
    },
    Endpoint {
        slug: "line-status",
        id: "elizabeth-line",
        path: "/Line/Mode/elizabeth-line/Status",
    },
    // Per-mode stop-points. Each is trimmed independently; the runtime
    // client merges them into a single `Vec<Station>` with line-list
    // union on collisions across modes.
    Endpoint {
        slug: "stop-points",
        id: "tube",
        path: "/StopPoint/Mode/tube",
    },
    Endpoint {
        slug: "stop-points",
        id: "overground",
        path: "/StopPoint/Mode/overground",
    },
    Endpoint {
        slug: "stop-points",
        id: "dlr",
        path: "/StopPoint/Mode/dlr",
    },
    Endpoint {
        slug: "stop-points",
        id: "elizabeth-line",
        path: "/StopPoint/Mode/elizabeth-line",
    },
    // Hub StopPoint detail JSON for the multi-mode tube hubs the
    // search-merge tests exercise. Bank and TCR retained their hub
    // ids; Whitechapel was renamed by TfL from HUBWHC to HUBZWL after
    // the November 2024 Overground rebrand. Highbury & Islington's
    // tube parent is also a hub and is recorded so that the cross-mode
    // merge path is covered for an Overground station, not just for
    // tube + DLR / tube + Elizabeth.
    Endpoint {
        slug: "stop-point",
        id: "HUBBAN",
        path: "/StopPoint/HUBBAN",
    },
    Endpoint {
        slug: "stop-point",
        id: "HUBTCR",
        path: "/StopPoint/HUBTCR",
    },
    Endpoint {
        slug: "stop-point",
        id: "HUBZWL",
        path: "/StopPoint/HUBZWL",
    },
    Endpoint {
        slug: "stop-point",
        id: "HUBHHY",
        path: "/StopPoint/HUBHHY",
    },
];

#[tokio::main]
async fn main() {
    // Determine the workspace root (two levels up from this crate).
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("could not find workspace root from CARGO_MANIFEST_DIR");
    let fixtures_root = workspace_root.join("fixtures");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("tubbie-fixture-recorder/0.1 (github.com/argen/tubbie)")
        .build()
        .expect("failed to build HTTP client");

    eprintln!("=== fixture-recorder starting ===");
    eprintln!("Writing to: {}", fixtures_root.display());
    eprintln!();

    let mut success = 0usize;
    let mut failure = 0usize;

    for ep in ENDPOINTS {
        eprintln!("Fetching {} / {} ...", ep.slug, ep.id);

        // Polite delay between requests to stay well under TfL's 50 rpm limit.
        if success + failure > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        let url = format!("{TFL_BASE}{}", ep.path);
        let recorded_at = Utc::now();

        let result = fetch_and_record(
            &client,
            &url,
            ep.slug,
            ep.id,
            &fixtures_root,
            recorded_at.to_rfc3339(),
        )
        .await;

        match result {
            Ok(byte_size) => {
                eprintln!("  OK  {}/{}.json ({byte_size} bytes)", ep.slug, ep.id);
                success += 1;
            }
            Err(e) => {
                eprintln!("  ERR {}/{}: {e}", ep.slug, ep.id);
                failure += 1;
            }
        }
    }

    eprintln!();
    eprintln!("=== done: {success} succeeded, {failure} failed ===");

    if failure > 0 {
        std::process::exit(1);
    }
}

async fn fetch_and_record(
    client: &reqwest::Client,
    url: &str,
    slug: &str,
    id: &str,
    fixtures_root: &Path,
    recorded_at: String,
) -> Result<usize, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = response.status();
    let tfl_api_version = response
        .headers()
        .get("x-api-version")
        .or_else(|| response.headers().get("etag"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read body: {e}"))?;

    // Parse to validate it's real JSON (not an error HTML page).
    let mut value: Value =
        serde_json::from_str(&body).map_err(|e| format!("response is not valid JSON: {e}"))?;

    // Sanity check: reject TfL error envelopes that slipped through HTTP 200.
    if let Some(obj) = value.as_object() {
        if obj.contains_key("message") && !obj.contains_key("stopPoints") {
            let msg = obj.get("message").and_then(|v| v.as_str()).unwrap_or("?");
            return Err(format!("TfL error envelope: {msg}"));
        }
    }

    // Detect and abort on empty arrivals arrays (unusual but handle gracefully).
    if let Some(arr) = value.as_array() {
        if arr.is_empty() && slug == "arrivals" {
            eprintln!(
                "  WARN: {slug}/{id} returned an empty array — committing empty fixture anyway"
            );
        }
    }

    // Trim stop-points fixture to remove unused fields.
    // The raw response is ~23 MB; after trim it's ~0.44 MB.
    // Always-on for stop-points: we never need the fat version in fixtures.
    if slug == "stop-points" {
        value = trim_stop_points(&value);
        eprintln!("  TRIM: stop-points trimmed to compact form");
    }

    // Write fixture JSON — atomically via .tmp + rename.
    let endpoint_dir = fixtures_root.join(slug);
    std::fs::create_dir_all(&endpoint_dir)
        .map_err(|e| format!("failed to create dir {}: {e}", endpoint_dir.display()))?;

    // Pretty-print for readability in diffs; use 2-space indent.
    let pretty =
        serde_json::to_string_pretty(&value).map_err(|e| format!("serialization failed: {e}"))?;
    let fixture_path = endpoint_dir.join(format!("{id}.json"));
    write_atomic(&fixture_path, pretty.as_bytes())
        .map_err(|e| format!("failed to write {}: {e}", fixture_path.display()))?;

    // Write sidecar metadata atomically. SECURITY: URL is sanitized — no app_key.
    let meta = FixtureMeta {
        recorded_at,
        url: sanitize_url(url),
        tfl_api_version,
    };
    let meta_json =
        serde_json::to_string_pretty(&meta).map_err(|e| format!("meta serialization: {e}"))?;
    let meta_path = endpoint_dir.join(format!("{id}.meta.json"));
    write_atomic(&meta_path, meta_json.as_bytes())
        .map_err(|e| format!("failed to write meta {}: {e}", meta_path.display()))?;

    Ok(pretty.len())
}

// ---------------------------------------------------------------------------
// Unit tests — no network required.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// SECURITY: app_key must be stripped from recorded URLs.
    /// This test verifies the sanitizer without any network call.
    #[test]
    fn sanitize_strips_app_key() {
        let url = "https://api.tfl.gov.uk/StopPoint/940GZZLUBZP/Arrivals?app_key=DEADBEEF";
        let sanitized = sanitize_url(url);
        assert_eq!(sanitized, "/StopPoint/940GZZLUBZP/Arrivals");
        assert!(
            !sanitized.contains("DEADBEEF"),
            "app_key must not appear in sanitized URL"
        );
        assert!(
            !sanitized.contains("app_key"),
            "app_key param name must not appear"
        );
    }

    #[test]
    fn sanitize_strips_app_key_with_other_params() {
        let url = "https://api.tfl.gov.uk/StopPoint/940GZZLUBZP/Arrivals?app_key=SECRET&foo=bar";
        let sanitized = sanitize_url(url);
        assert_eq!(sanitized, "/StopPoint/940GZZLUBZP/Arrivals?foo=bar");
        assert!(!sanitized.contains("SECRET"));
    }

    #[test]
    fn sanitize_preserves_other_params() {
        let url = "https://api.tfl.gov.uk/StopPoint/Mode/tube?foo=bar&baz=qux";
        let sanitized = sanitize_url(url);
        assert_eq!(sanitized, "/StopPoint/Mode/tube?foo=bar&baz=qux");
    }

    #[test]
    fn sanitize_no_query_string() {
        let url = "https://api.tfl.gov.uk/StopPoint/940GZZLUBZP/Arrivals";
        let sanitized = sanitize_url(url);
        assert_eq!(sanitized, "/StopPoint/940GZZLUBZP/Arrivals");
    }

    #[test]
    fn sanitize_app_key_only_param() {
        let url = "https://api.tfl.gov.uk/Line/Mode/tube/Status?app_key=MY_SECRET_KEY";
        let sanitized = sanitize_url(url);
        assert_eq!(sanitized, "/Line/Mode/tube/Status");
        assert!(!sanitized.contains("MY_SECRET_KEY"));
    }

    // ---------------------------------------------------------------------------
    // trim_stop_points tests
    // ---------------------------------------------------------------------------

    fn make_stop_point(
        id: &str,
        common_name: &str,
        lat: f64,
        lon: f64,
        modes: &[&str],
        line_identifiers: &[&str],
    ) -> Value {
        serde_json::json!({
            "$type": "Tfl.Api.Presentation.Entities.StopPoint, Tfl.Api.Presentation.Entities",
            "id": id,
            "naptanId": id,
            "commonName": common_name,
            "lat": lat,
            "lon": lon,
            "modes": modes,
            "lineModeGroups": [{
                "$type": "Tfl.Api.Presentation.Entities.LineModeGroup, Tfl.Api.Presentation.Entities",
                "modeName": "tube",
                "lineIdentifier": line_identifiers,
            }],
            "children": [{"id": "CHILD001"}],
            "childrenUrls": ["https://api.tfl.gov.uk/StopPoint/CHILD001"],
            "lineGroup": [{"naptanIdReference": "12345"}],
            "additionalProperties": [{"key": "Zone", "value": "2"}],
            "hubNaptanCode": "HUBXXX",
            "icsCode": "1000000",
            "smsCode": "12345",
            "stationNaptan": id,
            "placeType": "StopPoint",
            "status": true,
        })
    }

    fn make_raw_response(stop_points: Vec<Value>) -> Value {
        serde_json::json!({
            "$type": "Tfl.Api.Presentation.Entities.Paged`1[Tfl.Api.Presentation.Entities.StopPoint], Tfl.Api.Presentation.Entities",
            "page": 1,
            "pageSize": 1000,
            "total": stop_points.len(),
            "stopPoints": stop_points,
        })
    }

    /// (i) Bus-only stop point is dropped.
    #[test]
    fn trim_drops_non_surfaced_mode_stop() {
        // A stop that only has bus mode — should be filtered out.
        let bus_stop = serde_json::json!({
            "id": "490004733B",
            "commonName": "Some Bus Stop",
            "lat": 51.5,
            "lon": -0.1,
            "modes": ["bus"],
            "lineModeGroups": [],
        });
        let raw = make_raw_response(vec![bus_stop]);
        let trimmed = trim_stop_points(&raw);
        let stop_points = trimmed["stopPoints"].as_array().unwrap();
        assert!(stop_points.is_empty(), "bus stop should have been dropped");
    }

    /// Overground-only stop is preserved (regression guard: previously
    /// trim_stop_point only accepted "tube" mode and silently dropped
    /// every Overground / DLR / Elizabeth station, producing empty
    /// fixtures for those modes).
    #[test]
    fn trim_keeps_overground_only_stop() {
        let og_stop = serde_json::json!({
            "id": "910GHACKNYC",
            "commonName": "Hackney Central Rail Station",
            "lat": 51.547105,
            "lon": -0.056058,
            "modes": ["overground"],
            "lineModeGroups": [{
                "modeName": "overground",
                "lineIdentifier": ["mildmay"],
            }],
        });
        let raw = make_raw_response(vec![og_stop]);
        let trimmed = trim_stop_points(&raw);
        let stop_points = trimmed["stopPoints"].as_array().unwrap();
        assert_eq!(stop_points.len(), 1, "overground stop must be preserved");
        assert_eq!(stop_points[0]["id"], "910GHACKNYC");
    }

    /// DLR-only stop is preserved (latent bug guard: search_stations
    /// today silently excludes 940GZZDL* via the prefix filter, but
    /// the recorder must capture DLR fixtures correctly so the new
    /// search-filter rewrite has data to test against).
    #[test]
    fn trim_keeps_dlr_only_stop() {
        let dlr_stop = serde_json::json!({
            "id": "940GZZDLBNK",
            "commonName": "Bank DLR Station",
            "lat": 51.513,
            "lon": -0.089,
            "modes": ["dlr"],
            "lineModeGroups": [{
                "modeName": "dlr",
                "lineIdentifier": ["dlr"],
            }],
        });
        let raw = make_raw_response(vec![dlr_stop]);
        let trimmed = trim_stop_points(&raw);
        let stop_points = trimmed["stopPoints"].as_array().unwrap();
        assert_eq!(stop_points.len(), 1, "DLR stop must be preserved");
    }

    /// (ii) `children` and other bloat are stripped, but `hubNaptanCode`
    /// MUST be preserved — production hub-merge needs it to find sibling
    /// stop-points for DLR / Overground / Elizabeth at hub stations.
    #[test]
    fn trim_strips_children_but_preserves_hub_naptan_code() {
        let sp = make_stop_point(
            "940GZZLUBNK",
            "Bank Underground Station",
            51.5125,
            -0.0878,
            &["tube"],
            &["central", "northern", "waterloo-city"],
        );
        let raw = make_raw_response(vec![sp]);
        let trimmed = trim_stop_points(&raw);
        let stop = &trimmed["stopPoints"][0];
        assert!(
            stop.get("children").is_none(),
            "children should be stripped, got: {stop}"
        );
        assert!(
            stop.get("additionalProperties").is_none(),
            "additionalProperties should be stripped"
        );
        // make_stop_point inserts `hubNaptanCode: "HUBXXX"` for every fixture;
        // confirm the trimmer keeps it intact.
        assert_eq!(
            stop.get("hubNaptanCode").and_then(|v| v.as_str()),
            Some("HUBXXX"),
            "hubNaptanCode MUST be preserved through trim — it drives hub-merge"
        );
    }

    /// Regression guard: a stop without a hubNaptanCode (the common case —
    /// a non-hub station like Belsize Park) MUST NOT have an empty
    /// `hubNaptanCode: ""` in the output. The runtime deserializes that
    /// field as `Option<String>` and an empty string would coerce to
    /// `Some("")`, breaking the `is_some()`-driven hub-merge dispatch.
    #[test]
    fn trim_omits_hub_naptan_code_when_absent() {
        let sp = serde_json::json!({
            "id": "940GZZLUBZP",
            "commonName": "Belsize Park Underground Station",
            "lat": 51.5505,
            "lon": -0.1648,
            "modes": ["tube"],
            "lineModeGroups": [{
                "modeName": "tube",
                "lineIdentifier": ["northern"],
            }],
            // no hubNaptanCode field — Belsize Park is not a hub.
        });
        let raw = make_raw_response(vec![sp]);
        let trimmed = trim_stop_points(&raw);
        let stop = &trimmed["stopPoints"][0];
        assert!(
            stop.get("hubNaptanCode").is_none(),
            "hubNaptanCode must not be inserted for non-hub stops; got: {stop}"
        );
    }

    /// (iii) `lineModeGroups[].lineIdentifier` is preserved.
    #[test]
    fn trim_preserves_line_identifier() {
        let sp = make_stop_point(
            "940GZZLUKSX",
            "King's Cross St. Pancras Underground Station",
            51.5308,
            -0.1238,
            &["tube"],
            &[
                "circle",
                "hammersmith-city",
                "metropolitan",
                "northern",
                "piccadilly",
                "victoria",
            ],
        );
        let raw = make_raw_response(vec![sp]);
        let trimmed = trim_stop_points(&raw);
        let stop = &trimmed["stopPoints"][0];
        let line_ids: Vec<&str> = stop["lineModeGroups"][0]["lineIdentifier"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            line_ids.contains(&"northern"),
            "northern line should be preserved: {line_ids:?}"
        );
        assert!(
            line_ids.contains(&"victoria"),
            "victoria line should be preserved: {line_ids:?}"
        );
    }

    /// (iv) `commonName`, `lat`, `lon` are preserved.
    #[test]
    fn trim_preserves_common_name_lat_lon() {
        let sp = make_stop_point(
            "940GZZLUBZP",
            "Belsize Park Underground Station",
            51.550529,
            -0.164783,
            &["tube"],
            &["northern"],
        );
        let raw = make_raw_response(vec![sp]);
        let trimmed = trim_stop_points(&raw);
        let stop = &trimmed["stopPoints"][0];
        assert_eq!(stop["commonName"], "Belsize Park Underground Station");
        assert!((stop["lat"].as_f64().unwrap() - 51.550529).abs() < 1e-6);
        assert!((stop["lon"].as_f64().unwrap() - (-0.164783)).abs() < 1e-6);
    }

    /// HUB... entries are dropped.
    #[test]
    fn trim_drops_hub_entries() {
        let hub = serde_json::json!({
            "id": "HUBKGX",
            "commonName": "King's Cross Hub",
            "lat": 51.53,
            "lon": -0.12,
            "modes": ["tube", "national-rail"],
            "lineModeGroups": [],
        });
        let raw = make_raw_response(vec![hub]);
        let trimmed = trim_stop_points(&raw);
        let stop_points = trimmed["stopPoints"].as_array().unwrap();
        assert!(stop_points.is_empty(), "HUB entry should have been dropped");
    }

    /// Envelope fields ($type, total) are preserved.
    #[test]
    fn trim_preserves_envelope() {
        let raw = make_raw_response(vec![]);
        let trimmed = trim_stop_points(&raw);
        assert!(
            trimmed["$type"].as_str().is_some(),
            "$type should be preserved"
        );
        assert_eq!(trimmed["total"], 0);
    }

    // ---------------------------------------------------------------------------
    // write_atomic tests
    // ---------------------------------------------------------------------------

    /// Successful write: final file exists, .tmp file is cleaned up.
    #[test]
    fn write_atomic_success_cleans_up_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("foo.json");
        let tmp = tmp_path_for(&dest);

        // Plant a stale .tmp to confirm it gets overwritten + cleaned.
        fs::write(&tmp, b"stale content").unwrap();

        write_atomic(&dest, b"final content").unwrap();

        assert!(dest.exists(), "final file should exist");
        assert!(!tmp.exists(), ".tmp should be gone after successful write");
        assert_eq!(fs::read(&dest).unwrap(), b"final content");
    }

    /// Write to a read-only directory: no partial final-path file is created.
    #[test]
    fn write_atomic_failure_leaves_no_partial_final_file() {
        let dir = tempfile::tempdir().unwrap();
        // Create a read-only subdirectory.
        let ro_dir = dir.path().join("readonly");
        fs::create_dir(&ro_dir).unwrap();
        // Make it read-only so writes fail.
        let mut perms = fs::metadata(&ro_dir).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o555);
        }
        fs::set_permissions(&ro_dir, perms).unwrap();

        let dest = ro_dir.join("output.json");
        let result = write_atomic(&dest, b"content");
        // Should fail (can't create .tmp in read-only dir).
        assert!(result.is_err(), "write to read-only dir should fail");
        // Final file must NOT exist (no partial write).
        assert!(
            !dest.exists(),
            "final file must not exist after failed write"
        );
    }
}
