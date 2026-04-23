use crate::error::TflError;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Duration;
use url::Url;
use zeroize::Zeroize;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BASE_URL: &str = "https://api.tfl.gov.uk/";
const USER_AGENT: &str = "tubbie/0.1 (https://github.com/argen/tubbie)";
const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Maximum number of retry attempts (not counting the initial attempt).
const MAX_RETRIES: u32 = 2;
/// Initial backoff duration in milliseconds.
#[cfg(not(test))]
const BACKOFF_BASE_MS: u64 = 500;
/// Short backoff for unit/integration tests so we don't burn wall-clock time.
/// Used in combination with `#[tokio::test(start_paused = true)]`.
#[cfg(test)]
const BACKOFF_BASE_MS: u64 = 10;
/// Backoff multiplier per retry.
const BACKOFF_FACTOR: u64 = 2;
/// Maximum backoff duration.
#[cfg(not(test))]
const BACKOFF_MAX_MS: u64 = 2_000;
/// Short backoff cap for tests (mirrors BACKOFF_BASE_MS scaling).
#[cfg(test)]
const BACKOFF_MAX_MS: u64 = 40;
/// Cap on `Retry-After` header value in seconds; beyond this we give up immediately.
const RETRY_AFTER_CAP_SECS: u64 = 5;

// ---------------------------------------------------------------------------
// TflHttp trait
// ---------------------------------------------------------------------------

/// Transport-only trait for fetching data from TfL.
///
/// Implementations:
/// - `ReqwestTflHttp` — hits `api.tfl.gov.uk` (live).
/// - `FixtureTflHttp` — reads from `fixtures/{endpoint}/{id}.json` (CI-safe).
///
/// `endpoint` is a path segment like `"arrivals"`, `"line-status"`, or `"stop-points"`.
/// `id` is a resource identifier like `"940GZZLUBZP"` or `"tube"`.
pub trait TflHttp: Send + Sync {
    fn fetch(
        &self,
        endpoint: &str,
        id: &str,
    ) -> impl std::future::Future<Output = Result<Value, TflError>> + Send;
}

// ---------------------------------------------------------------------------
// ReqwestTflHttp
// ---------------------------------------------------------------------------

/// App key wrapper that zeroizes its memory on drop and never prints the key
/// via `Debug`.
struct AppKey(String);

impl Drop for AppKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for AppKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Live TfL HTTP client backed by `reqwest`.
///
/// ## Connection reuse
/// The internal `reqwest::Client` is constructed once and shared across all
/// `fetch` calls — no per-call DNS lookup or TLS handshake after warm-up.
/// `pool_max_idle_per_host` is set to 4.
///
/// ## API-key precedence
/// 1. Explicit key via [`ReqwestTflHttp::with_app_key`] — highest priority.
/// 2. `TFL_APP_KEY` environment variable (read once at construction by [`ReqwestTflHttp::new`]).
/// 3. Anonymous access (no `app_key` query param) when neither is set.
///
/// ## Security
/// The key is stored in an [`AppKey`] wrapper that:
/// - Zeroizes the key bytes on drop.
/// - Overrides `Debug` to print `<redacted>` rather than the key.
///
/// Error messages never include the URL query string (which would contain the
/// key); see [`TflError::transport_from`].
pub struct ReqwestTflHttp {
    client: reqwest::Client,
    base_url: Url,
    app_key: Option<AppKey>,
}

impl std::fmt::Debug for ReqwestTflHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReqwestTflHttp { .. }")
    }
}

impl ReqwestTflHttp {
    /// Construct a client, reading `TFL_APP_KEY` from the environment (if set).
    ///
    /// The key is read **once** here; subsequent changes to the env var have no
    /// effect on this client instance.
    pub fn new() -> Self {
        let app_key = std::env::var("TFL_APP_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .map(AppKey);

        Self::build(
            app_key,
            Url::parse(BASE_URL).expect("base URL is valid"),
            None,
        )
    }

    /// Construct a client with an explicit API key (takes precedence over env var).
    ///
    /// This is the hook for M5's `tauri-plugin-store` integration.
    pub fn with_app_key(key: String) -> Self {
        Self::build(
            Some(AppKey(key)),
            Url::parse(BASE_URL).expect("base URL is valid"),
            None,
        )
    }

    /// Override the base URL — useful for pointing at a wiremock server in tests.
    ///
    /// API key and timeout use defaults (`TFL_APP_KEY` env var; 10s).
    pub fn with_base_url(base_url: impl AsRef<str>) -> Self {
        let url = Url::parse(base_url.as_ref()).expect("base URL must be valid");
        let app_key = std::env::var("TFL_APP_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .map(AppKey);
        Self::build(app_key, url, None)
    }

    /// Full-control constructor for tests: explicit key + base URL + timeout.
    pub fn with_config(
        app_key: Option<String>,
        base_url: impl AsRef<str>,
        timeout: Duration,
    ) -> Self {
        let url = Url::parse(base_url.as_ref()).expect("base URL must be valid");
        Self::build(app_key.map(AppKey), url, Some(timeout))
    }

    // ------------------------------------------------------------------
    // Private builder
    // ------------------------------------------------------------------

    fn build(app_key: Option<AppKey>, base_url: Url, timeout: Option<Duration>) -> Self {
        let timeout = timeout.unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .pool_max_idle_per_host(4)
            .user_agent(USER_AGENT)
            .build()
            .expect("reqwest client config is valid");

        Self {
            client,
            base_url,
            app_key,
        }
    }

    // ------------------------------------------------------------------
    // URL construction
    // ------------------------------------------------------------------

    /// Build the full request URL for an (endpoint, id) pair.
    ///
    /// URL construction uses `url::Url` throughout — never `format!` with
    /// user-controlled path segments. This guarantees proper percent-encoding
    /// and prevents scheme injection.
    fn build_url(&self, endpoint: &str, id: &str) -> Result<Url, TflError> {
        // Map (endpoint, id) → path relative to base.
        let path = match endpoint {
            "arrivals" => format!("StopPoint/{}/Arrivals", percent_encode(id)),
            "line-status" => format!("Line/Mode/{}/Status", percent_encode(id)),
            "stop-points" => format!("StopPoint/Mode/{}", percent_encode(id)),
            other => format!("{}/{}", percent_encode(other), percent_encode(id)),
        };

        let mut url = self.base_url.join(&path).map_err(|e| TflError::Transport {
            kind: format!("URL construction error: {e}"),
            url_sanitized: self.base_url.to_string(),
        })?;

        // Append app_key as a query parameter if we have one.
        if let Some(ref key) = self.app_key {
            url.query_pairs_mut().append_pair("app_key", &key.0);
        }

        Ok(url)
    }

    // ------------------------------------------------------------------
    // Request with retry loop
    // ------------------------------------------------------------------

    /// Execute one HTTP GET with retry on 429 / 503 / connect-timeout.
    ///
    /// Retry policy:
    /// - Max `MAX_RETRIES` additional attempts after the initial one.
    /// - Exponential backoff: `BACKOFF_BASE_MS * BACKOFF_FACTOR^attempt`, capped at `BACKOFF_MAX_MS`.
    /// - 429: respect `Retry-After` header (capped at `RETRY_AFTER_CAP_SECS`).
    /// - 503: exponential backoff.
    /// - Connect-timeout: exponential backoff.
    /// - Other 4xx / 5xx: no retry.
    async fn fetch_with_retry(&self, endpoint: &str, id: &str) -> Result<Value, TflError> {
        let url = self.build_url(endpoint, id)?;

        let mut last_err: Option<TflError> = None;

        for attempt in 0..=MAX_RETRIES {
            match self.do_request(url.clone()).await {
                Ok(val) => return Ok(val),

                Err(RetryDecision::Retry { after, err }) => {
                    last_err = Some(err);
                    if attempt < MAX_RETRIES {
                        let backoff = compute_backoff(attempt, after);
                        tokio::time::sleep(backoff).await;
                    }
                }

                Err(RetryDecision::Fail(err)) => return Err(err),
            }
        }

        Err(last_err.expect("last_err is set whenever we enter Retry branch"))
    }

    /// Perform one HTTP GET and return either a parsed value or a retry decision.
    async fn do_request(&self, url: Url) -> Result<Value, RetryDecision> {
        let response = self.client.get(url.clone()).send().await.map_err(|e| {
            let tfl_err = TflError::transport_from(&e);
            // Retry on connect / timeout errors.
            if e.is_connect() || e.is_timeout() {
                RetryDecision::Retry {
                    after: None,
                    err: tfl_err,
                }
            } else {
                RetryDecision::Fail(tfl_err)
            }
        })?;

        let status = response.status();

        if status.is_success() {
            let value: Value = response
                .json()
                .await
                .map_err(|e| RetryDecision::Fail(TflError::transport_from(&e)))?;
            return Ok(value);
        }

        match status.as_u16() {
            404 => Err(RetryDecision::Fail(TflError::NotFound(format!(
                "TfL returned 404 for {endpoint}/{id}",
                endpoint = url.path(),
                id = ""
            )))),

            429 => {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| parse_retry_after(s, Utc::now()));

                // If Retry-After exceeds our cap, give up immediately.
                if let Some(dur) = retry_after {
                    if dur.as_secs() > RETRY_AFTER_CAP_SECS {
                        return Err(RetryDecision::Fail(TflError::RateLimited {
                            retry_after: Some(dur),
                        }));
                    }
                }

                Err(RetryDecision::Retry {
                    after: retry_after,
                    err: TflError::RateLimited { retry_after },
                })
            }

            503 => {
                let body_snippet = body_snippet_from_response(response).await;
                Err(RetryDecision::Retry {
                    after: None,
                    err: TflError::Http {
                        status: 503,
                        body_snippet,
                    },
                })
            }

            _ => {
                let body_snippet = body_snippet_from_response(response).await;
                Err(RetryDecision::Fail(TflError::Http {
                    status: status.as_u16(),
                    body_snippet,
                }))
            }
        }
    }
}

impl Default for ReqwestTflHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl TflHttp for ReqwestTflHttp {
    /// Fetch a TfL resource.
    ///
    /// Maps HTTP error statuses to typed `TflError` variants:
    /// - 404 → `TflError::NotFound`
    /// - 429 → `TflError::RateLimited` (reads `Retry-After` header if present; retried up to MAX_RETRIES times)
    /// - 503 → `TflError::Http { status: 503, .. }` (retried up to MAX_RETRIES times)
    /// - other 4xx / 5xx → `TflError::Http { status, .. }` (not retried)
    ///
    /// SECURITY: the URL (including any `app_key` query param) is never included
    /// in error messages. See [`TflError::transport_from`] and [`TflError::Transport`].
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<Value, TflError> {
        self.fetch_with_retry(endpoint, id).await
    }
}

// ---------------------------------------------------------------------------
// Internal retry decision type
// ---------------------------------------------------------------------------

/// Internal signal from a single HTTP attempt.
enum RetryDecision {
    /// Retry the request; `after` is a requested delay (from `Retry-After`).
    Retry {
        after: Option<Duration>,
        err: TflError,
    },
    /// Do not retry; return this error to the caller.
    Fail(TflError),
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the backoff duration for a given retry attempt number (0-indexed).
///
/// If the server sent a `Retry-After` value, honour it as-is (it has already
/// been pre-filtered to ≤ `RETRY_AFTER_CAP_SECS` at the call site — capping it
/// again at `BACKOFF_MAX_MS` would violate the server's directive).
///
/// Without a server value, use exponential backoff:
/// `BACKOFF_BASE_MS * BACKOFF_FACTOR^attempt`, capped at `BACKOFF_MAX_MS`.
fn compute_backoff(attempt: u32, server_after: Option<Duration>) -> Duration {
    if let Some(after) = server_after {
        // Pre-filtered to <= RETRY_AFTER_CAP_SECS at the call site; return as-is.
        return after;
    }
    let ms = (BACKOFF_BASE_MS * BACKOFF_FACTOR.pow(attempt)).min(BACKOFF_MAX_MS);
    Duration::from_millis(ms)
}

/// Parse the value of a `Retry-After` HTTP header into a `Duration`.
///
/// RFC 7231 §7.1.3 allows two forms:
/// 1. A non-negative integer representing seconds (e.g. `"5"`).
/// 2. An HTTP-date string (e.g. `"Wed, 24 Apr 2026 12:00:00 GMT"`).
///
/// For the integer form the value is returned directly.
/// For the HTTP-date form the duration is computed as `date - now`; if the
/// date is in the past the duration is clamped to zero (treat as retry
/// immediately). If neither form parses, `None` is returned.
///
/// The `now` parameter exists so callers can inject a fixed timestamp in tests.
pub fn parse_retry_after(header: &str, now: DateTime<Utc>) -> Option<Duration> {
    let header = header.trim();
    if header.is_empty() {
        return None;
    }

    // Try integer seconds first.
    if let Ok(secs) = header.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    // Try HTTP-date form via the `httpdate` crate.
    // `httpdate::parse_http_date` returns a `std::time::SystemTime`.
    if let Ok(system_time) = httpdate::parse_http_date(header) {
        // Convert to chrono::DateTime<Utc> for safe arithmetic.
        let target: DateTime<Utc> = system_time.into();
        let delta = target.signed_duration_since(now);
        if delta <= chrono::Duration::zero() {
            // Date is in the past — wait 0 seconds.
            return Some(Duration::ZERO);
        }
        // Convert positive chrono::Duration to std::time::Duration.
        return delta.to_std().ok().or(Some(Duration::ZERO));
    }

    None
}

/// Percent-encode a path segment (spaces → %20, slashes → %2F, etc.).
///
/// In practice TfL IDs are alphanumeric (e.g. `940GZZLUBZP`, `tube`) so this
/// is belt-and-braces, but it prevents scheme injection if a caller passes an
/// unexpected character.
fn percent_encode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                vec![c]
            } else {
                // Encode as %XX for each byte.
                let mut buf = [0u8; 4];
                let encoded: String = c
                    .encode_utf8(&mut buf)
                    .bytes()
                    .map(|b| format!("%{b:02X}"))
                    .collect::<Vec<_>>()
                    .join("")
                    .chars()
                    .collect();
                encoded.chars().collect()
            }
        })
        .collect()
}

/// Read up to 512 bytes of text from a response body for error context.
///
/// SECURITY: the URL is NOT included. This function reads body only.
async fn body_snippet_from_response(response: reqwest::Response) -> String {
    match response.text().await {
        Ok(body) => truncate_to_512(body),
        Err(_) => "(could not read body)".to_string(),
    }
}

/// Truncate a string to at most 512 characters (char boundary safe).
fn truncate_to_512(s: String) -> String {
    if s.len() <= 512 {
        s
    } else {
        let mut idx = 512;
        while !s.is_char_boundary(idx) {
            idx -= 1;
        }
        format!("{}…", &s[..idx])
    }
}

// ---------------------------------------------------------------------------
// URL builder helper (kept public for backwards-compatibility with existing tests)
// ---------------------------------------------------------------------------

/// Build the TfL API URL for a given endpoint and id.
///
/// Mapping:
/// - `arrivals`    + id → `/StopPoint/{id}/Arrivals`
/// - `line-status` + id → `/Line/Mode/{id}/Status`
/// - `stop-points` + id → `/StopPoint/Mode/{id}`
#[doc(hidden)]
pub fn build_url(base: &str, endpoint: &str, id: &str) -> String {
    match endpoint {
        "arrivals" => format!("{base}/StopPoint/{id}/Arrivals"),
        "line-status" => format!("{base}/Line/Mode/{id}/Status"),
        "stop-points" => format!("{base}/StopPoint/Mode/{id}"),
        other => format!("{base}/{other}/{id}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // URL builder (kept for backwards-compat)
    // ------------------------------------------------------------------

    #[test]
    fn build_url_arrivals() {
        let url = build_url("https://api.tfl.gov.uk", "arrivals", "940GZZLUBZP");
        assert_eq!(url, "https://api.tfl.gov.uk/StopPoint/940GZZLUBZP/Arrivals");
    }

    #[test]
    fn build_url_line_status() {
        let url = build_url("https://api.tfl.gov.uk", "line-status", "tube");
        assert_eq!(url, "https://api.tfl.gov.uk/Line/Mode/tube/Status");
    }

    #[test]
    fn build_url_stop_points() {
        let url = build_url("https://api.tfl.gov.uk", "stop-points", "tube");
        assert_eq!(url, "https://api.tfl.gov.uk/StopPoint/Mode/tube");
    }

    // ------------------------------------------------------------------
    // URL builder with app_key (internal build_url method)
    // ------------------------------------------------------------------

    #[test]
    fn internal_build_url_appends_app_key() {
        let client = ReqwestTflHttp::with_config(
            Some("MYKEY".to_string()),
            "https://api.tfl.gov.uk/",
            Duration::from_secs(5),
        );
        let url = client.build_url("arrivals", "TEST").unwrap();
        let query: Vec<(_, _)> = url.query_pairs().collect();
        assert!(
            query.iter().any(|(k, v)| k == "app_key" && v == "MYKEY"),
            "app_key should be in query: {url}"
        );
        // Ensure no user-controlled segment made it to scheme position.
        assert_eq!(url.scheme(), "https");
    }

    #[test]
    fn internal_build_url_no_key_when_anonymous() {
        let client =
            ReqwestTflHttp::with_config(None, "https://api.tfl.gov.uk/", Duration::from_secs(5));
        let url = client.build_url("arrivals", "TEST").unwrap();
        assert!(
            url.query().is_none() || !url.query().unwrap().contains("app_key"),
            "anonymous client must not append app_key: {url}"
        );
    }

    #[test]
    fn explicit_key_takes_precedence_over_env() {
        // We can't easily test env-var precedence without std::env manipulation
        // (which is process-global and test-unsafe). This test ensures that
        // with_app_key constructs a client that embeds the explicit key.
        let client = ReqwestTflHttp::with_app_key("EXPLICIT".to_string());
        let url = client
            .build_url("arrivals", "TEST")
            .expect("URL build should succeed");
        let has_key = url
            .query_pairs()
            .any(|(k, v)| k == "app_key" && v == "EXPLICIT");
        assert!(has_key, "explicit key should be present in URL: {url}");
    }

    // ------------------------------------------------------------------
    // Debug impl does not leak key
    // ------------------------------------------------------------------

    #[test]
    fn debug_impl_does_not_leak_app_key() {
        let client = ReqwestTflHttp::with_app_key("SUPERSECRET".to_string());
        let debug_str = format!("{client:?}");
        assert!(
            !debug_str.contains("SUPERSECRET"),
            "Debug must not leak key, got: {debug_str}"
        );
        assert_eq!(debug_str, "ReqwestTflHttp { .. }");
    }

    // ------------------------------------------------------------------
    // Truncation helper
    // ------------------------------------------------------------------

    #[test]
    fn truncate_to_512_short() {
        let s = "hello".to_string();
        assert_eq!(truncate_to_512(s), "hello");
    }

    #[test]
    fn truncate_to_512_long() {
        let s = "a".repeat(600);
        let t = truncate_to_512(s);
        assert!(t.len() <= 515, "truncated len: {}", t.len());
        assert!(t.ends_with('…'));
    }

    // ------------------------------------------------------------------
    // Compute backoff
    // ------------------------------------------------------------------

    #[test]
    fn compute_backoff_exponential_without_server() {
        // In test builds BACKOFF_BASE_MS=10, BACKOFF_MAX_MS=40.
        assert_eq!(compute_backoff(0, None), Duration::from_millis(10));
        assert_eq!(compute_backoff(1, None), Duration::from_millis(20));
        assert_eq!(compute_backoff(2, None), Duration::from_millis(40));
        // Capped at BACKOFF_MAX_MS
        assert_eq!(compute_backoff(10, None), Duration::from_millis(40));
    }

    #[test]
    fn compute_backoff_honours_server_after() {
        let after = Duration::from_millis(1200);
        assert_eq!(compute_backoff(0, Some(after)), Duration::from_millis(1200));
    }

    /// A server Retry-After of 3s must be respected as 3s — NOT capped at
    /// BACKOFF_MAX_MS (2s). The outer call site already filters values > 5s.
    #[test]
    fn compute_backoff_honours_server_retry_after_within_cap() {
        let after = Duration::from_secs(3);
        assert_eq!(
            compute_backoff(0, Some(after)),
            Duration::from_secs(3),
            "server Retry-After=3s must not be capped at BACKOFF_MAX_MS"
        );
    }

    // The old `compute_backoff_caps_server_after` test was asserting the wrong
    // behaviour: it expected the server's Retry-After to be capped at
    // BACKOFF_MAX_MS (2s), but a server value of 3s–5s (within RETRY_AFTER_CAP)
    // should be honoured verbatim. That test has been replaced by
    // `compute_backoff_honours_server_retry_after_within_cap` above.
    // Large values (>RETRY_AFTER_CAP_SECS) are rejected at the do_request level
    // before compute_backoff is ever called, so no capping is needed here.

    // ------------------------------------------------------------------
    // parse_retry_after
    // ------------------------------------------------------------------

    #[test]
    fn parse_retry_after_integer_5() {
        let now = chrono::Utc::now();
        assert_eq!(parse_retry_after("5", now), Some(Duration::from_secs(5)));
    }

    #[test]
    fn parse_retry_after_integer_120() {
        let now = chrono::Utc::now();
        assert_eq!(
            parse_retry_after("120", now),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn parse_retry_after_integer_0() {
        let now = chrono::Utc::now();
        assert_eq!(parse_retry_after("0", now), Some(Duration::from_secs(0)));
    }

    #[test]
    fn parse_retry_after_http_date_future() {
        // Pin "now" to a fixed point so the test is deterministic.
        // Target = now + 60 seconds.
        // April 24, 2026 is a Friday.
        let now: DateTime<Utc> = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = parse_retry_after("Fri, 24 Apr 2026 12:01:00 GMT", now);
        assert!(result.is_some(), "HTTP-date 60s in future should parse");
        let dur = result.unwrap();
        // Allow ±1s for any rounding in the conversion.
        assert!(
            dur.as_secs() >= 59 && dur.as_secs() <= 61,
            "expected ~60s, got {}s",
            dur.as_secs()
        );
    }

    #[test]
    fn parse_retry_after_http_date_past() {
        // Date is in the past → clamp to zero.
        // April 24, 2026 is a Friday.
        let now: DateTime<Utc> = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let result = parse_retry_after("Fri, 24 Apr 2026 11:59:00 GMT", now);
        assert_eq!(
            result,
            Some(Duration::ZERO),
            "past HTTP-date should yield 0s"
        );
    }

    #[test]
    fn parse_retry_after_not_a_number() {
        let now = chrono::Utc::now();
        assert_eq!(parse_retry_after("not-a-number", now), None);
    }

    #[test]
    fn parse_retry_after_empty() {
        let now = chrono::Utc::now();
        assert_eq!(parse_retry_after("", now), None);
    }

    #[test]
    fn parse_retry_after_whitespace_only() {
        let now = chrono::Utc::now();
        assert_eq!(parse_retry_after("  ", now), None);
    }

    // ------------------------------------------------------------------
    // Integration tests using wiremock — require tokio runtime
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_200_returns_parsed_json() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = serde_json::json!([{"id": "abc", "timeToStation": 60}]);
        Mock::given(method("GET"))
            .and(path("/StopPoint/TEST/Arrivals"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = ReqwestTflHttp::with_base_url(server.uri());
        let value = client
            .fetch("arrivals", "TEST")
            .await
            .expect("200 should succeed");
        assert_eq!(value, body);
    }

    #[tokio::test]
    async fn fetch_404_returns_not_found() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/StopPoint/MISSING/Arrivals"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let client = ReqwestTflHttp::with_base_url(server.uri());
        let err = client
            .fetch("arrivals", "MISSING")
            .await
            .expect_err("404 should be an error");
        assert!(
            matches!(err, TflError::NotFound(_)),
            "expected NotFound, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn fetch_500_returns_http_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/StopPoint/TEST/Arrivals"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
            .mount(&server)
            .await;

        let client = ReqwestTflHttp::with_base_url(server.uri());
        let err = client
            .fetch("arrivals", "TEST")
            .await
            .expect_err("500 should be an error");

        match err {
            TflError::Http { status, .. } => {
                assert_eq!(status, 500, "expected status 500");
            }
            other => panic!("expected Http, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn fetch_error_display_does_not_contain_url() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/StopPoint/SENSITIVE/Arrivals"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "60")
                    .set_body_string("too many requests"),
            )
            .mount(&server)
            .await;

        // A client with an explicit app_key — must never leak it on rate-limit.
        let client = ReqwestTflHttp::with_config(
            Some("DEADBEEF".to_string()),
            server.uri(),
            Duration::from_secs(5),
        );
        let err = client
            .fetch("arrivals", "SENSITIVE")
            .await
            .expect_err("should error");
        let display = err.to_string();
        // Retry-After is 60s which exceeds RETRY_AFTER_CAP_SECS (5s) → immediate fail.
        assert!(
            !display.contains("DEADBEEF"),
            "app_key must not appear in error display, got: {display}"
        );
        assert!(
            !display.contains("localhost"),
            "raw URL must not appear in error display, got: {display}"
        );
    }

    /// Verify that app_key never leaks through Transport errors.
    /// This test uses with_config to inject a key, then induces a transport
    /// error (DNS failure on a non-existent host). The stringified error must
    /// not contain the key.
    #[tokio::test]
    async fn transport_error_does_not_leak_app_key_in_display() {
        let client = ReqwestTflHttp::with_config(
            Some("DEADBEEF".to_string()),
            "http://this.host.does.not.exist.invalid/",
            Duration::from_millis(500),
        );
        let err = client
            .fetch("arrivals", "TEST")
            .await
            .expect_err("should fail to connect");
        let display = err.to_string();
        assert!(
            !display.contains("DEADBEEF"),
            "app_key must not appear in Transport error display, got: {display}"
        );
    }
}
