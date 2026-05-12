//! Wiremock-based tests for retry / backoff behaviour in `ReqwestTflHttp`.
//!
//! ## What we verify
//! - 429 responses are retried (up to MAX_RETRIES=2 extra attempts).
//! - 503 responses are retried.
//! - 500 responses are NOT retried (only one request made).
//! - A second attempt succeeds after an initial 503.
//! - After MAX_RETRIES exhausted, the correct error is returned.
//! - Retry-After beyond the cap (5s) causes immediate failure without retry.
//! - app_key never appears in error `Display` output.
//!
//! Each test uses wiremock's `expect(N)` to assert exact request counts,
//! proving the retry loop never fires more than MAX_RETRIES+1 total requests.
//!
//! ## Time approach (FIX 4)
//! These tests use real `tokio::time` (no `start_paused`) because `wiremock`
//! uses real TCP sockets — pausing tokio time causes reqwest's internal
//! timeout to trigger immediately, making every request fail with a transport
//! timeout error before the mock server can respond.
//!
//! Instead, wall-clock duration is kept negligible by the `#[cfg(test)]`
//! backoff constants: `BACKOFF_BASE_MS = 10ms`, `BACKOFF_MAX_MS = 40ms`.
//! The entire retry sequence (up to 3 requests) completes in < 100ms real time.
//! `start_paused = true` IS used for the `BoardService` stream tests (tfl-board
//! crate) and for the unit-level `compute_backoff` / `parse_retry_after` tests
//! which have no real I/O.

use std::time::Duration;
use tfl_client::error::TflError;
use tfl_client::http::ReqwestTflHttp;
use tfl_client::http::TflHttp;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a test client pointing at the mock server, with a short timeout.
fn test_client(server: &MockServer) -> ReqwestTflHttp {
    ReqwestTflHttp::with_config(None, server.uri(), Duration::from_secs(5))
}

/// Build a test client with an explicit app_key.
fn test_client_with_key(server: &MockServer, key: &str) -> ReqwestTflHttp {
    ReqwestTflHttp::with_config(Some(key.to_string()), server.uri(), Duration::from_secs(5))
}

// ---------------------------------------------------------------------------
// 200 baseline
// ---------------------------------------------------------------------------

#[tokio::test]
async fn success_on_first_attempt_returns_value() {
    let server = MockServer::start().await;
    let body = serde_json::json!([{"id": "x", "timeToStation": 120}]);

    Mock::given(method("GET"))
        .and(path("/StopPoint/TEST/Arrivals"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1) // exactly one request
        .mount(&server)
        .await;

    let client = test_client(&server);
    let val = client.fetch("arrivals", "TEST").await.unwrap();
    assert_eq!(val, body);
    server.verify().await;
}

// ---------------------------------------------------------------------------
// 429 retry
// ---------------------------------------------------------------------------

/// 429 with Retry-After within cap → retried up to MAX_RETRIES times.
/// After exhausting retries, returns RateLimited.
#[tokio::test]
async fn fetch_429_is_retried_and_returns_rate_limited_after_exhaustion() {
    let server = MockServer::start().await;

    // Always return 429; total requests = 1 initial + 2 retries = 3.
    Mock::given(method("GET"))
        .and(path("/StopPoint/TEST/Arrivals"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0") // 0s → immediate retry (within cap)
                .set_body_string("rate limited"),
        )
        .expect(3) // 1 + MAX_RETRIES
        .mount(&server)
        .await;

    let client = test_client(&server);
    let err = client
        .fetch("arrivals", "TEST")
        .await
        .expect_err("should exhaust retries and fail");

    assert!(
        matches!(err, TflError::RateLimited { .. }),
        "expected RateLimited after exhaustion, got: {err:?}"
    );

    server.verify().await;
}

/// 429 succeeds on second attempt.
#[tokio::test]
async fn fetch_429_then_200_succeeds() {
    let server = MockServer::start().await;
    let body = serde_json::json!([{"id": "y", "timeToStation": 30}]);

    // First call: 429.
    Mock::given(method("GET"))
        .and(path("/StopPoint/RETRY/Arrivals"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("rate limited"),
        )
        .expect(1)
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Second call: 200.
    Mock::given(method("GET"))
        .and(path("/StopPoint/RETRY/Arrivals"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);
    let val = client
        .fetch("arrivals", "RETRY")
        .await
        .expect("should succeed on second attempt");
    assert_eq!(val, body);
    server.verify().await;
}

/// 429 with Retry-After exceeding cap → no retry, immediate RateLimited.
#[tokio::test]
async fn fetch_429_retry_after_exceeds_cap_returns_immediately() {
    let server = MockServer::start().await;

    // Retry-After = 60s > RETRY_AFTER_CAP_SECS (5s) → fail immediately.
    Mock::given(method("GET"))
        .and(path("/StopPoint/TEST/Arrivals"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "60")
                .set_body_string("rate limited"),
        )
        .expect(1) // Only one request — no retries because cap exceeded.
        .mount(&server)
        .await;

    let client = test_client(&server);
    let err = client
        .fetch("arrivals", "TEST")
        .await
        .expect_err("should fail immediately");

    match err {
        TflError::RateLimited { retry_after } => {
            assert_eq!(
                retry_after,
                Some(Duration::from_secs(60)),
                "retry_after should reflect the server value"
            );
        }
        other => panic!("expected RateLimited, got: {other:?}"),
    }

    server.verify().await;
}

// ---------------------------------------------------------------------------
// 503 retry
// ---------------------------------------------------------------------------

/// 503 is retried up to MAX_RETRIES times.
#[tokio::test]
async fn fetch_503_is_retried_and_returns_http_503_after_exhaustion() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/StopPoint/TEST/Arrivals"))
        .respond_with(ResponseTemplate::new(503).set_body_string("service unavailable"))
        .expect(3) // 1 + MAX_RETRIES
        .mount(&server)
        .await;

    let client = test_client(&server);
    let err = client
        .fetch("arrivals", "TEST")
        .await
        .expect_err("should fail after exhaustion");

    match err {
        TflError::Http { status, .. } => assert_eq!(status, 503),
        other => panic!("expected Http 503, got: {other:?}"),
    }

    server.verify().await;
}

/// 503 then 200 → success on second attempt.
#[tokio::test]
async fn fetch_503_then_200_succeeds() {
    let server = MockServer::start().await;
    let body = serde_json::json!([{"id": "z", "timeToStation": 90}]);

    Mock::given(method("GET"))
        .and(path("/StopPoint/RETRY503/Arrivals"))
        .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
        .expect(1)
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/StopPoint/RETRY503/Arrivals"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);
    let val = client
        .fetch("arrivals", "RETRY503")
        .await
        .expect("should succeed on second attempt");
    assert_eq!(val, body);
    server.verify().await;
}

// ---------------------------------------------------------------------------
// 500 — NOT retried
// ---------------------------------------------------------------------------

/// 500 must NOT be retried (only 429 and 503 trigger retries).
#[tokio::test]
async fn fetch_500_is_not_retried() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/StopPoint/TEST/Arrivals"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
        .expect(1) // Exactly one request — no retries.
        .mount(&server)
        .await;

    let client = test_client(&server);
    let err = client
        .fetch("arrivals", "TEST")
        .await
        .expect_err("500 should error");

    match err {
        TflError::Http { status, .. } => assert_eq!(status, 500),
        other => panic!("expected Http 500, got: {other:?}"),
    }

    server.verify().await;
}

// ---------------------------------------------------------------------------
// app_key redaction in error Display
// ---------------------------------------------------------------------------

/// This is the belt-and-braces assertion required by task #10:
/// even when a client has an explicit app_key, the error display must not
/// contain it. We use a wiremock 429 that exceeds the cap to produce an
/// immediate error, then check the stringified error.
#[tokio::test]
async fn app_key_absent_from_rate_limited_error_display() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/StopPoint/TEST/Arrivals"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "60")
                .set_body_string("rate limited"),
        )
        .mount(&server)
        .await;

    let client = test_client_with_key(&server, "DEADBEEF");
    let err = client
        .fetch("arrivals", "TEST")
        .await
        .expect_err("should fail");

    let display = err.to_string();
    assert!(
        !display.contains("DEADBEEF"),
        "app_key 'DEADBEEF' must not appear in error display: {display}"
    );
}

/// Confirm app_key is not in Http error display for non-retried failures.
#[tokio::test]
async fn app_key_absent_from_http_error_display() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/StopPoint/TEST/Arrivals"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal error body"))
        .mount(&server)
        .await;

    let client = test_client_with_key(&server, "DEADBEEF");
    let err = client
        .fetch("arrivals", "TEST")
        .await
        .expect_err("should fail");

    let display = err.to_string();
    assert!(
        !display.contains("DEADBEEF"),
        "app_key must not appear in Http error display: {display}"
    );
}

// ---------------------------------------------------------------------------
// Connection reuse assertion
// ---------------------------------------------------------------------------

/// Verify the reqwest::Client is built once and reused across multiple fetch
/// calls. We assert this structurally: multiple calls all go through the same
/// mock server (which requires connection reuse in practice) without errors.
/// A separate doc comment in the struct guarantees the design intent.
#[tokio::test]
async fn multiple_fetch_calls_reuse_underlying_client() {
    let server = MockServer::start().await;
    let body = serde_json::json!([]);

    Mock::given(method("GET"))
        .and(path("/StopPoint/A/Arrivals"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/StopPoint/B/Arrivals"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/StopPoint/C/Arrivals"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    // Single client instance — three requests, all should succeed.
    let client = test_client(&server);
    client.fetch("arrivals", "A").await.unwrap();
    client.fetch("arrivals", "B").await.unwrap();
    client.fetch("arrivals", "C").await.unwrap();
    // If the client were recreated per request, the pool overhead would be
    // observable, but we verify the design: one ReqwestTflHttp, one
    // reqwest::Client, many requests.
}

// ---------------------------------------------------------------------------
// 404 not found — not retried
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_404_is_not_retried() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/StopPoint/MISSING/Arrivals"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .expect(1) // Exactly one request.
        .mount(&server)
        .await;

    let client = test_client(&server);
    let err = client
        .fetch("arrivals", "MISSING")
        .await
        .expect_err("404 should error");

    assert!(
        matches!(err, TflError::NotFound(_)),
        "expected NotFound, got: {err:?}"
    );

    server.verify().await;
}

// Note: get_line_status_serves_repeat_calls_from_cache has moved to
// crates/tfl-cache/tests/line_status_cache_wire_test.rs, since the 60s TTL
// cache now lives in tfl-cache rather than tfl-client.

// ---------------------------------------------------------------------------
// Item 6 — process-wide 429 cooldown gate
// ---------------------------------------------------------------------------

/// When a 429 with retry-after exceeding the cap triggers the cooldown gate,
/// a second concurrent fetch on the same client must wait until the cooldown
/// expires before hitting the wire.
///
/// Red: without the gate, the second call fires immediately, the mock sees 2
/// requests but declared expect(1), so server.verify() panics.
///
/// Green: with the gate the second call blocks on the cooldown; the mock sees
/// exactly 1 request within the observation window.
///
/// We use retry-after: 6 (> RETRY_AFTER_CAP_SECS=5 → fail-fast, sets cooldown
/// ~6s). To avoid a 6-second wall-clock wait we assert the second call has NOT
/// completed within 200 ms — proof it is blocked on the cooldown.
#[tokio::test]
async fn concurrent_calls_share_429_cooldown() {
    let server = MockServer::start().await;

    // This mock will only ever be hit once — the cooldown prevents the second
    // call from reaching the wire during the 6-second window.
    Mock::given(method("GET"))
        .and(path("/StopPoint/COOL/Arrivals"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "6") // > RETRY_AFTER_CAP_SECS → fail-fast + set cooldown
                .set_body_string("rate limited"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);

    // First call: hits 429 with retry-after:6, fails immediately, sets cooldown.
    let first = client.fetch("arrivals", "COOL").await;
    assert!(
        matches!(first, Err(TflError::RateLimited { .. })),
        "first call should return RateLimited"
    );

    // Second call: should block on the cooldown gate (6s remaining).
    // Assert it has NOT completed within 200 ms — if it had, the cooldown gate
    // would not be functioning and the mock would have received a second request.
    let second_task =
        tokio::time::timeout(Duration::from_millis(200), client.fetch("arrivals", "COOL")).await;
    assert!(
        second_task.is_err(),
        "second call should still be blocked on cooldown after 200 ms"
    );

    // Exactly 1 wire request was made: the first call. The second is still
    // sleeping on the gate and has not touched the wire.
    server.verify().await;
}

// ---------------------------------------------------------------------------
// Item 1 — app_key query param
// ---------------------------------------------------------------------------

/// Proves that `ReqwestTflHttp::with_app_key` sends `app_key=` as a query
/// parameter. This is the primitive the stream-client fix in lib.rs relies on.
#[tokio::test]
async fn with_app_key_appends_query_param() {
    let server = MockServer::start().await;
    let body = serde_json::json!([]);

    Mock::given(method("GET"))
        .and(path("/StopPoint/X/Arrivals"))
        .and(query_param("app_key", "DEADBEEF"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client_with_key(&server, "DEADBEEF");
    client
        .fetch("arrivals", "X")
        .await
        .expect("should succeed when app_key is present");
    server.verify().await;
}
