//! Wire-level test: two `get_line_status` calls for different lines share one
//! HTTP request when the cache is hot.
//!
//! Uses wiremock so the mock server asserts the exact request count.

use std::time::Duration;

use tfl_cache::TflClient;
use tfl_client::http::ReqwestTflHttp;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn two_line_status_body() -> serde_json::Value {
    serde_json::json!([
        {
            "id": "northern",
            "name": "Northern",
            "lineStatuses": [
                {"statusSeverity": 10, "statusSeverityDescription": "Good Service"}
            ]
        },
        {
            "id": "victoria",
            "name": "Victoria",
            "lineStatuses": [
                {"statusSeverity": 10, "statusSeverityDescription": "Good Service"}
            ]
        }
    ])
}

/// Two `get_line_status` calls for different lines should share one wire
/// request when the cache is hot. The mock is configured with `.expect(1)`;
/// if each call hits the wire independently (pre-cache behaviour) the mock
/// fires twice and `server.verify()` fails.
#[tokio::test]
async fn get_line_status_serves_repeat_calls_from_cache() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/Line/Mode/tube/Status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_line_status_body()))
        .expect(1) // exactly ONE wire hit for both calls
        .mount(&server)
        .await;

    let http = ReqwestTflHttp::with_config(None, server.uri(), Duration::from_secs(5));
    let client = TflClient::new(http);

    // Two calls for different line_ids — both should be served from the same
    // cached response after the first fetch.
    client
        .get_line_status("northern")
        .await
        .expect("first call should succeed");
    client
        .get_line_status("victoria")
        .await
        .expect("second call should hit cache");

    // If the cache is missing, this assertion fires: the mock saw 2 requests,
    // but we declared expect(1).
    server.verify().await;
}
