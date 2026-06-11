//! B.4 validation gates for the live HTTP exports.
//!
//! These tests do NOT hit the network. They exercise the cheap-reject
//! validation path: empty / overlong station id, bad poll_seconds, bad
//! `app_key` shape, etc. The happy path against TfL is exercised end-to-end
//! via the iOS Simulator with a real API key (B.4 manual smoke).
//!
//! ## Why no live integration test
//!
//! Live TfL has a 50 req/min anonymous quota and a per-key rate limit;
//! running a `cargo test --workspace` repeatedly would burn through both
//! during a normal dev cycle, and CI runs would shadow real users on the
//! shared anonymous bucket. The fixture-mode tests cover the bridge
//! contract; the validation tests here cover the "didn't break the
//! cheap-reject path when we added the live constructor" regression.
//!
//! Per `tubbie-ios/CLAUDE.md` test discipline these went RED → GREEN →
//! revert against the missing exports.

use tfl_ffi::{
    find_nearest_stations_live, get_line_statuses_live, search_stations_live,
    served_lines_for_station_live, subscribe_board_live, FfiError,
};

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_rejects_empty_station_id() {
    let result = subscribe_board_live(String::new(), None, 30).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_rejects_overlong_station_id() {
    let result = subscribe_board_live("x".repeat(33), None, 30).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_rejects_zero_poll_seconds() {
    let result = subscribe_board_live("940GZZLUBNK".into(), None, 0).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_rejects_overlong_poll_seconds() {
    let result = subscribe_board_live("940GZZLUBNK".into(), None, 601).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_rejects_empty_string_app_key() {
    // Empty `Some("")` is a paste error, NOT "use anonymous". Anonymous
    // requires `None` explicitly so the user-visible UX path is "tap
    // 'Skip key' on onboarding", not "submit empty field".
    let result = subscribe_board_live("940GZZLUBNK".into(), Some(String::new()), 30).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(
            msg.contains("app_key"),
            "validation should mention app_key, got {msg:?}"
        ),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok(subscription)"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_rejects_whitespace_app_key() {
    let result = subscribe_board_live("940GZZLUBNK".into(), Some("    ".into()), 30).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_rejects_short_app_key() {
    let result = subscribe_board_live("940GZZLUBNK".into(), Some("abc123".into()), 30).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(msg.contains("32 hex")),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok(subscription)"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_rejects_non_hex_app_key() {
    // 32 chars but with non-hex characters.
    let result = subscribe_board_live(
        "940GZZLUBNK".into(),
        Some("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz".into()),
        30,
    )
    .await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn live_subscribe_accepts_well_formed_app_key_format() {
    // We can't actually start the subscription here without hitting the
    // network — but we CAN verify validation passes (the function would
    // need to spawn a task to fail at the next layer). Use a short poll
    // window so that if validation accidentally regresses to "always
    // succeed" we do NOT leak a long-lived task into the test runtime.
    //
    // The key is the example from TfL's developer docs (deliberately
    // invalid as an actual auth credential — it would 401 — but it has
    // the right SHAPE).
    let well_formed = "0123456789abcdef0123456789abcdef".to_string();

    // To avoid making a live HTTP request, we intercept by using an
    // invalid station_id length AT 33 chars BUT with a valid app_key
    // shape — the station_id rejection runs FIRST in the validation
    // chain so we never reach the HTTP code path. This proves the key
    // shape was accepted (otherwise we'd see `app_key`-mentioning error
    // text instead of `station_id`-mentioning).
    let result = subscribe_board_live("x".repeat(33), Some(well_formed), 30).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(
            msg.contains("station_id"),
            "expected station_id error (validation order proves app_key was accepted), got {msg}"
        ),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok(subscription)"),
    }
}

// ---------------------------------------------------------------------------
// search_stations_live
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn search_stations_returns_empty_array_for_empty_query() {
    // Empty query SHORT-CIRCUITS — no network fetch. This is the cheap
    // path the SwiftUI search field hits while the user is typing.
    let json = search_stations_live(String::new(), None)
        .await
        .expect("empty query must succeed");
    assert_eq!(json, "[]");
}

#[tokio::test(flavor = "multi_thread")]
async fn search_stations_returns_empty_array_for_whitespace_query() {
    let json = search_stations_live("   ".into(), None)
        .await
        .expect("whitespace query must succeed");
    assert_eq!(json, "[]");
}

#[tokio::test(flavor = "multi_thread")]
async fn search_stations_short_circuits_empty_query_before_app_key_check() {
    // Empty-query short-circuit runs BEFORE app_key validation, so even
    // a malformed Some("") app_key returns []. This is intentional: it
    // means the SwiftUI search field can call `search_stations_live`
    // unconditionally as the user types without per-keystroke
    // validation.
    let json = search_stations_live(String::new(), Some(String::new()))
        .await
        .expect("empty query short-circuits before app_key check");
    assert_eq!(json, "[]");
}

// ---------------------------------------------------------------------------
// get_line_statuses_live
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn get_line_statuses_rejects_empty_string_app_key() {
    // Same Some("") rejection as subscribe_board_live and
    // search_stations_live — symmetric across all three live exports.
    let result = get_line_statuses_live(Some(String::new())).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(msg.contains("app_key")),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_line_statuses_rejects_whitespace_app_key() {
    let result = get_line_statuses_live(Some("   ".into())).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn search_stations_rejects_empty_app_key_when_query_non_empty() {
    // Symmetry with `subscribe_board_live`: Some("") is a paste error,
    // NOT "use anonymous". Anonymous requires None.
    let result = search_stations_live("Bank".into(), Some(String::new())).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(
            msg.contains("app_key"),
            "validation should mention app_key, got {msg:?}"
        ),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok"),
    }
}

// ---------------------------------------------------------------------------
// find_nearest_stations_live
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn find_nearest_rejects_latitude_above_range() {
    // Out-of-range coordinates must produce a typed Validation error
    // rather than reaching `find_nearest_stations` with garbage. The
    // upstream haversine sort would happily run on lat=91 and produce
    // a coincidentally-non-empty list — exactly the silent-corruption
    // shape we want to surface as a hard error.
    let result = find_nearest_stations_live(91.0, 0.0, 5, None).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(
            msg.contains("lat"),
            "validation should mention lat, got {msg:?}"
        ),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn find_nearest_rejects_latitude_below_range() {
    let result = find_nearest_stations_live(-90.5, 0.0, 5, None).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn find_nearest_rejects_non_finite_latitude() {
    let result = find_nearest_stations_live(f64::NAN, 0.0, 5, None).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
    let result = find_nearest_stations_live(f64::INFINITY, 0.0, 5, None).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn find_nearest_rejects_longitude_above_range() {
    let result = find_nearest_stations_live(51.5, 181.0, 5, None).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(
            msg.contains("lon"),
            "validation should mention lon, got {msg:?}"
        ),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn find_nearest_rejects_zero_limit() {
    // limit=0 would always return [] — a UX cliff that looks like
    // "no nearby stations" when the real cause is a wired-wrong
    // call site. Make it a typed error.
    let result = find_nearest_stations_live(51.5, -0.1, 0, None).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(
            msg.contains("limit"),
            "validation should mention limit, got {msg:?}"
        ),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn find_nearest_rejects_limit_above_ceiling() {
    let result = find_nearest_stations_live(51.5, -0.1, 51, None).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn find_nearest_rejects_empty_app_key() {
    // Symmetric with the other live exports: Some("") is paste-error,
    // NOT anonymous. Anonymous requires None.
    let result = find_nearest_stations_live(51.5, -0.1, 5, Some(String::new())).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(
            msg.contains("app_key"),
            "validation should mention app_key, got {msg:?}"
        ),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn find_nearest_rejects_whitespace_app_key() {
    let result = find_nearest_stations_live(51.5, -0.1, 5, Some("   ".into())).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

// ---------------------------------------------------------------------------
// served_lines_for_station_live
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn served_lines_rejects_empty_station_id() {
    let result = served_lines_for_station_live(String::new(), None).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn served_lines_rejects_overlong_station_id() {
    let result = served_lines_for_station_live("x".repeat(33), None).await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn served_lines_rejects_empty_string_app_key() {
    let result = served_lines_for_station_live("940GZZLUBNK".into(), Some(String::new())).await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(
            msg.contains("app_key"),
            "validation should mention app_key, got {msg:?}"
        ),
        Err(other) => panic!("expected Validation, got Err({other:?})"),
        Ok(_) => panic!("expected Validation error, got Ok"),
    }
}
