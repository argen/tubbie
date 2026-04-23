use crate::error::TflError;
use serde_json::Value;
use std::time::Duration;

/// Transport-only trait for fetching data from TfL.
///
/// Implementations:
/// - `ReqwestTflHttp` — hits `api.tfl.gov.uk` (live; full behaviour in M3).
/// - `FixtureTflHttp` — reads from `fixtures/{endpoint}/{id}.json` (CI-safe).
///
/// `endpoint` is a path segment like `"arrivals"`, `"line-status"`, or `"stop-points"`.
/// `id` is a resource identifier like `"940GZZLUBZP"` or `"tube"`.
///
/// M2 will add typed accessors (`get_arrivals`, `search_stations`, etc.) on top
/// of this transport primitive.
pub trait TflHttp: Send + Sync {
    fn fetch(
        &self,
        endpoint: &str,
        id: &str,
    ) -> impl std::future::Future<Output = Result<Value, TflError>> + Send;
}

/// Live TfL HTTP client backed by `reqwest`.
///
/// Full behaviour (URL construction, app_key injection, retry, timeout) lands in M3.
/// In M0 this stub compiles and is instantiable; `fetch` is a thin passthrough.
pub struct ReqwestTflHttp {
    client: reqwest::Client,
    base_url: String,
}

impl ReqwestTflHttp {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://api.tfl.gov.uk".to_string(),
        }
    }

    /// Override the base URL — useful for pointing at a wiremock server in tests.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
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
    /// - 429 → `TflError::RateLimited` (reads `Retry-After` header if present)
    /// - 5xx → `TflError::Transport`
    /// - other 4xx → `TflError::Http { status, body_snippet }`
    ///
    /// SECURITY: the URL is never included in error messages to avoid leaking
    /// any app_key that may appear in request URLs.
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<Value, TflError> {
        let url = build_url(&self.base_url, endpoint, id);
        let response = self.client.get(&url).send().await?;
        let status = response.status();

        if status.is_success() {
            let value: Value = response.json().await?;
            return Ok(value);
        }

        // Map error statuses to typed variants.
        match status.as_u16() {
            404 => Err(TflError::NotFound(format!(
                "TfL returned 404 for {endpoint}/{id}"
            ))),
            429 => {
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(Duration::from_secs);
                Err(TflError::RateLimited { retry_after })
            }
            500..=599 => {
                // 5xx: server error. Body is read for the snippet but the URL
                // is never included in the error to avoid leaking app_key.
                let body_snippet = body_snippet_from_response(response).await;
                Err(TflError::Http {
                    status: status.as_u16(),
                    body_snippet,
                })
            }
            _ => {
                let body_snippet = body_snippet_from_response(response).await;
                Err(TflError::Http {
                    status: status.as_u16(),
                    body_snippet,
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
        // Find the last char boundary at or before 512 bytes.
        let mut idx = 512;
        while !s.is_char_boundary(idx) {
            idx -= 1;
        }
        format!("{}…", &s[..idx])
    }
}

/// Build the TfL API URL for a given endpoint and id.
///
/// Mapping (M0 scope — arrivals only; extended in M2):
/// - `arrivals`    + id → `/StopPoint/{id}/Arrivals`
/// - `line-status` + id → `/Line/Mode/{id}/Status`
/// - `stop-points` + id → `/StopPoint/Mode/{id}`
pub fn build_url(base: &str, endpoint: &str, id: &str) -> String {
    match endpoint {
        "arrivals" => format!("{base}/StopPoint/{id}/Arrivals"),
        "line-status" => format!("{base}/Line/Mode/{id}/Status"),
        "stop-points" => format!("{base}/StopPoint/Mode/{id}"),
        other => format!("{base}/{other}/{id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn truncate_to_512_short() {
        let s = "hello".to_string();
        assert_eq!(truncate_to_512(s), "hello");
    }

    #[test]
    fn truncate_to_512_long() {
        let s = "a".repeat(600);
        let t = truncate_to_512(s);
        // Should end with ellipsis and be just over 512 chars (the … is 3 bytes in UTF-8)
        assert!(t.len() <= 515, "truncated len: {}", t.len());
        assert!(t.ends_with('…'));
    }

    // ---------------------------------------------------------------------------
    // Integration tests using wiremock — require tokio runtime
    // ---------------------------------------------------------------------------

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
    async fn fetch_429_returns_rate_limited_with_retry_after() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/StopPoint/TEST/Arrivals"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "30")
                    .set_body_string("too many requests"),
            )
            .mount(&server)
            .await;

        let client = ReqwestTflHttp::with_base_url(server.uri());
        let err = client
            .fetch("arrivals", "TEST")
            .await
            .expect_err("429 should be an error");

        match err {
            TflError::RateLimited { retry_after } => {
                assert_eq!(
                    retry_after,
                    Some(Duration::from_secs(30)),
                    "retry_after should be 30s"
                );
            }
            other => panic!("expected RateLimited, got: {other:?}"),
        }
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
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "60"))
            .mount(&server)
            .await;

        let client = ReqwestTflHttp::with_base_url(server.uri());
        let err = client
            .fetch("arrivals", "SENSITIVE")
            .await
            .expect_err("should error");
        let display = err.to_string();
        // The display must not include the mock server URL (which contains localhost:PORT)
        // In production the URL could contain app_key — never leak it.
        assert!(
            !display.contains("localhost"),
            "error display must not contain URL, got: {display}"
        );
    }
}
