//! B.3 acceptance — streaming bridge across uniffi.
//!
//! These tests prove the `subscribe_board` + `BoardSubscription` shape:
//! a long-running task in Rust forwards `BoardService::stream` emissions
//! to Swift via an mpsc channel, and ScenePhase-driven pause/resume maps
//! cleanly onto `AppPhase::{Active,Background}`. The bridge contract is
//! the load-bearing question for B.3 — these tests are the kill-switch.
//!
//! Per `tubbie-ios/CLAUDE.md` test discipline (RED → GREEN → revert): the
//! tests here were written before the implementation; deleting any of the
//! four exported items (`subscribe_board`, `BoardSubscription::next_snapshot`,
//! `pause`, `resume`) MUST make the relevant test fail compile or assert.
//!
//! ## What is NOT tested here
//!
//! Stream timing semantics (interval emit, station-change refresh, last_ok
//! lifecycle) are upstream's responsibility — `tfl-board/tests/board_service_tests.rs`
//! covers them exhaustively against the same `BoardService::stream` we
//! consume here. Re-asserting them in tfl-ffi would couple the FFI tests
//! to internal stream behaviour without adding signal. We test only:
//!
//! 1. The bridge actually emits JSON at all.
//! 2. The pause/resume signal reaches the upstream stream and changes
//!    its emission cadence (this is the ONLY tfl-ffi-specific contract
//!    upstream can't test).
//! 3. Validation rejects bad inputs without spawning a task (cheap path).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tfl_ffi::{subscribe_board, BoardSubscription, FfiError};

fn fixtures_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("..").join("fixtures")
}

fn fixture_recorded_at(station_id: &str) -> String {
    let meta_path = fixtures_dir()
        .join("arrivals")
        .join(format!("{station_id}.meta.json"));
    let raw = std::fs::read_to_string(&meta_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", meta_path.display()));
    let v: serde_json::Value = serde_json::from_str(&raw).expect("meta is JSON");
    v["recorded_at"]
        .as_str()
        .expect("meta.recorded_at is a string")
        .to_string()
}

async fn fresh_subscription(poll_seconds: u32) -> Arc<BoardSubscription> {
    subscribe_board(
        "940GZZLUBNK".into(),
        fixtures_dir().to_string_lossy().into_owned(),
        fixture_recorded_at("940GZZLUBNK"),
        poll_seconds,
    )
    .await
    .expect("subscribe_board should succeed against the bundled fixture")
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribe_board_emits_an_initial_snapshot() {
    let sub = fresh_subscription(5).await;

    let snap = tokio::time::timeout(Duration::from_secs(2), sub.next_snapshot())
        .await
        .expect("first snapshot should arrive within 2 s of subscribe")
        .expect("first snapshot should not be a Refresh error");
    let json = snap.expect("subscription should not have shut down before first emit");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("snapshot is JSON");
    assert_eq!(parsed["station_id"], "940GZZLUBNK");
    assert!(parsed["platforms"].is_array());
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_stops_emissions_resume_unblocks_them() {
    // Use a 1-second poll so the next tick fires fast under the real-clock
    // test context. Smaller would just stress the test harness without
    // adding signal — the ONE thing this test proves is that pause()
    // visibly suppresses an emission a resumed stream would have produced.
    //
    // ## Why real-clock and not `tokio::time::pause()`
    //
    // `BoardService::stream` uses `tokio::time::interval`. Under
    // `start_paused = true` the interval ticks would only fire on
    // `tokio::time::advance(...)`, which we control here, but the stream
    // also does `cfg_rx.changed().await` and `phase_rx.changed().await`
    // in its select arm — neither of those is timer-driven. Replacing the
    // real-clock waits with paused-time advances would make the test
    // *less* representative of the production path, not more, because the
    // production path runs a real tokio runtime under uniffi.
    let sub = fresh_subscription(1).await;

    // Drain the initial-emit-on-subscribe so subsequent waits measure
    // post-pause behaviour, not the cold-start emit.
    let _initial = tokio::time::timeout(Duration::from_secs(2), sub.next_snapshot())
        .await
        .expect("initial emit should arrive")
        .expect("initial emit should not error");

    sub.pause();

    // 4 s @ 1 s poll = at least three ticks that would have fired without
    // pause. Bridge must NOT yield in that window. The 4 s ceiling is
    // chosen so loaded CI runs (a one-off ~2.5 s spin-up) still see
    // multiple suppressed ticks rather than racing the assertion.
    let result = tokio::time::timeout(Duration::from_secs(4), sub.next_snapshot()).await;
    assert!(
        result.is_err(),
        "pause() must suppress emissions; got an emit: {result:?}"
    );

    sub.resume();

    // Background → Active triggers an immediate refresh upstream
    // (invariant 8). Bound generously for CI noise: 3 s is the upper
    // limit before we treat resume as broken.
    let snap = tokio::time::timeout(Duration::from_secs(3), sub.next_snapshot())
        .await
        .expect("resume() must produce a fresh emit within 3 s")
        .expect("resume emit must not be an error");
    assert!(snap.is_some(), "resume emit must not be subscription-end");
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_and_resume_are_idempotent() {
    let sub = fresh_subscription(2).await;

    // Drain the cold-start emit.
    let _ = tokio::time::timeout(Duration::from_secs(2), sub.next_snapshot())
        .await
        .expect("initial emit should arrive");

    // No-op repeats must not panic, double-spawn, or change the channel
    // state. The receiving side simply doesn't see anything because the
    // upstream stream uses `send_if_modified`.
    sub.pause();
    sub.pause();
    sub.resume();
    sub.resume();

    // Post-condition (review feedback): a no-panic assertion would be
    // useless. Instead prove the lifecycle still works after the redundant
    // toggles by asserting the next emit lands. Background → Active forces
    // an immediate refresh upstream, so this should be prompt.
    let snap = tokio::time::timeout(Duration::from_secs(3), sub.next_snapshot())
        .await
        .expect("after redundant pause/resume toggles, next emit must arrive within 3 s")
        .expect("emit must not be a typed error");
    assert!(
        snap.is_some(),
        "redundant toggles must not have shut the subscription down"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_makes_next_snapshot_return_subscription_end() {
    let sub = fresh_subscription(60).await;

    // Drain whatever the cold-start emit produced so the post-shutdown
    // wait measures the shutdown signal, not a queued emission.
    let _ = tokio::time::timeout(Duration::from_secs(2), sub.next_snapshot()).await;

    sub.shutdown().await;

    let snap = tokio::time::timeout(Duration::from_secs(2), sub.next_snapshot())
        .await
        .expect("post-shutdown next_snapshot must return promptly")
        .expect("post-shutdown must not be a typed error");
    assert!(
        snap.is_none(),
        "post-shutdown snapshot must be None (subscription end), got {snap:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribe_rejects_empty_station_id() {
    let result = subscribe_board(
        String::new(),
        fixtures_dir().to_string_lossy().into_owned(),
        fixture_recorded_at("940GZZLUBNK"),
        30,
    )
    .await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribe_rejects_zero_poll_seconds() {
    let result = subscribe_board(
        "940GZZLUBNK".into(),
        fixtures_dir().to_string_lossy().into_owned(),
        fixture_recorded_at("940GZZLUBNK"),
        0,
    )
    .await;
    match result {
        Err(FfiError::Validation(msg)) => assert!(msg.contains("poll_seconds")),
        Err(other) => panic!("expected Validation, got {other:?}"),
        Ok(_) => panic!("expected Validation error, got Ok(subscription)"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribe_rejects_overlong_poll_seconds() {
    let result = subscribe_board(
        "940GZZLUBNK".into(),
        fixtures_dir().to_string_lossy().into_owned(),
        fixture_recorded_at("940GZZLUBNK"),
        601,
    )
    .await;
    assert!(matches!(result, Err(FfiError::Validation(_))));
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribe_rejects_missing_fixtures_dir() {
    let result = subscribe_board(
        "940GZZLUBNK".into(),
        "/this/path/does/not/exist".into(),
        fixture_recorded_at("940GZZLUBNK"),
        30,
    )
    .await;
    assert!(matches!(result, Err(FfiError::Io(_))));
}
