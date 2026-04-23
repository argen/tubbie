use crate::error::TflError;
use serde_json::Value;

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
    /// M0 stub: constructs the URL and issues the request.
    /// Typed deserialization, error mapping, and retry land in M2/M3.
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<Value, TflError> {
        let url = build_url(&self.base_url, endpoint, id);
        let response = self.client.get(&url).send().await?;
        let value: Value = response.json().await?;
        Ok(value)
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
}
