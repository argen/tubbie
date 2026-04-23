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

struct Endpoint {
    /// Slug used for directory + meta (e.g. "arrivals").
    slug: &'static str,
    /// Filename stem (e.g. "940GZZLUBZP" → "940GZZLUBZP.json").
    id: &'static str,
    /// TfL API URL path (no base, no app_key).
    path: &'static str,
}

const ENDPOINTS: &[Endpoint] = &[
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
    Endpoint {
        slug: "line-status",
        id: "tube",
        path: "/Line/Mode/tube/Status",
    },
    Endpoint {
        slug: "stop-points",
        id: "tube",
        path: "/StopPoint/Mode/tube",
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
    let value: Value =
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

    // Write fixture JSON.
    let endpoint_dir = fixtures_root.join(slug);
    std::fs::create_dir_all(&endpoint_dir)
        .map_err(|e| format!("failed to create dir {}: {e}", endpoint_dir.display()))?;

    // Pretty-print for readability in diffs; use 2-space indent.
    let pretty =
        serde_json::to_string_pretty(&value).map_err(|e| format!("serialization failed: {e}"))?;
    let fixture_path = endpoint_dir.join(format!("{id}.json"));
    std::fs::write(&fixture_path, &pretty)
        .map_err(|e| format!("failed to write {}: {e}", fixture_path.display()))?;

    // Write sidecar metadata. SECURITY: URL is sanitized — no app_key.
    let meta = FixtureMeta {
        recorded_at,
        url: sanitize_url(url),
        tfl_api_version,
    };
    let meta_json =
        serde_json::to_string_pretty(&meta).map_err(|e| format!("meta serialization: {e}"))?;
    let meta_path = endpoint_dir.join(format!("{id}.meta.json"));
    std::fs::write(&meta_path, &meta_json)
        .map_err(|e| format!("failed to write meta {}: {e}", meta_path.display()))?;

    Ok(pretty.len())
}

// ---------------------------------------------------------------------------
// Unit tests — no network required.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
}
