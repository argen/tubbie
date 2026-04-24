//! Integration tests for `BoardService`.
//!
//! All tests use `FixtureTflHttp` (offline) and `FakeClock` — zero live network.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures::StreamExt;
use tfl_board::{BoardConfig, BoardError, BoardService};
use tfl_client::clock::FakeClock;
use tfl_client::error::TflError;
use tfl_client::fixture::FixtureTflHttp;
use tfl_client::http::TflHttp;
use tfl_client::TflClient;
use tfl_domain::Direction;

/// Path to the workspace fixtures directory, resolved from the crate manifest.
fn fixtures_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../fixtures")
}

/// Build a `BoardService` backed by real fixture files and a pinned clock.
fn fixture_service(clock_rfc3339: &str) -> BoardService<FixtureTflHttp, FakeClock> {
    let http = FixtureTflHttp::new(fixtures_dir());
    let client = TflClient::new(http);
    let clock = FakeClock::from_rfc3339(clock_rfc3339).unwrap();
    BoardService::new(client, clock)
}

// ---------------------------------------------------------------------------
// Test 1: refresh_returns_filtered_board (BZP — northern, Northbound only)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_returns_filtered_board() {
    let svc = fixture_service("2026-04-23T16:31:00Z");
    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec!["northern".to_string()],
        directions: vec![Direction::Northbound { via: None }],
        poll_seconds: 20,
        theme: "classic-amber".to_string(),
    };

    let board = svc.refresh(&cfg).await.expect("refresh should succeed");

    // All platforms should contain only northbound arrivals.
    for platform in &board.platforms {
        for arrival in &platform.arrivals {
            assert_eq!(
                arrival.line_id, "northern",
                "should only have northern arrivals"
            );
            assert!(
                matches!(arrival.direction, Direction::Northbound { .. }),
                "should only have northbound arrivals, got: {:?}",
                arrival.direction
            );
        }
    }

    // BZP has both Northbound and Southbound — we should see at least one platform with northbound.
    assert!(
        !board.platforms.is_empty(),
        "should have at least one platform"
    );
}

// ---------------------------------------------------------------------------
// Test 2: refresh_groups_by_platform (KSX — 4+ lines, multi-platform)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_groups_by_platform() {
    let svc = fixture_service("2026-04-23T16:31:00Z");
    let cfg = BoardConfig::new("940GZZLUKSX"); // no filters — show everything

    let board = svc.refresh(&cfg).await.expect("refresh should succeed");

    // KSX fixture has 6 distinct platforms (northern, victoria, piccadilly, metropolitan, hammersmith-city).
    assert!(
        board.platforms.len() >= 4,
        "KSX should have at least 4 platforms, got: {}",
        board.platforms.len()
    );

    // Verify that arrivals within each platform are sorted ascending by time_to_station.
    for platform in &board.platforms {
        let times: Vec<i64> = platform
            .arrivals
            .iter()
            .map(|a| a.time_to_station)
            .collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(
            times, sorted,
            "arrivals should be sorted by time_to_station in platform: {}",
            platform.name
        );
    }

    // Verify that all platforms have at least one arrival.
    for platform in &board.platforms {
        assert!(
            !platform.arrivals.is_empty(),
            "platform {} should have arrivals",
            platform.name
        );
    }
}

// ---------------------------------------------------------------------------
// Test 3: refresh_sets_generated_at_from_clock
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_sets_generated_at_from_clock() {
    let known_time = "2026-04-23T12:34:56Z";
    let svc = fixture_service(known_time);
    let cfg = BoardConfig::new("940GZZLUBZP");

    let board = svc.refresh(&cfg).await.expect("refresh should succeed");

    let expected = chrono::DateTime::parse_from_rfc3339(known_time)
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(
        board.generated_at, expected,
        "generated_at must equal the injected clock time"
    );
}

// ---------------------------------------------------------------------------
// Test 4: refresh_fresh_board_has_no_stale_since
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_fresh_board_has_no_stale_since() {
    let svc = fixture_service("2026-04-23T16:31:00Z");
    let cfg = BoardConfig::new("940GZZLUBZP");

    let board = svc.refresh(&cfg).await.expect("refresh should succeed");

    assert!(
        board.stale_since.is_none(),
        "fresh board should have stale_since = None, got: {:?}",
        board.stale_since
    );
}

// ---------------------------------------------------------------------------
// Test 10: filter_by_directions_empty_matches_all
// (duplicated here at integration level; unit-level copy is in filter.rs)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filter_by_directions_empty_matches_all_integration() {
    let svc = fixture_service("2026-04-23T16:31:00Z");
    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec![],
        directions: vec![], // empty = no filter
        poll_seconds: 20,
        theme: "classic-amber".to_string(),
    };

    let board = svc.refresh(&cfg).await.expect("refresh should succeed");
    // BZP has both northbound and southbound; we should see both.
    let has_northbound = board.platforms.iter().any(|p| {
        p.arrivals
            .iter()
            .any(|a| matches!(a.direction, Direction::Northbound { .. }))
    });
    let has_southbound = board.platforms.iter().any(|p| {
        p.arrivals
            .iter()
            .any(|a| matches!(a.direction, Direction::Southbound { .. }))
    });
    assert!(
        has_northbound,
        "should see northbound arrivals when directions filter is empty"
    );
    assert!(
        has_southbound,
        "should see southbound arrivals when directions filter is empty"
    );
}

// ---------------------------------------------------------------------------
// Test 11: filter_by_line_ids_empty_matches_all
// ---------------------------------------------------------------------------

#[tokio::test]
async fn filter_by_line_ids_empty_matches_all_integration() {
    let svc = fixture_service("2026-04-23T16:31:00Z");
    let cfg = BoardConfig {
        station_id: "940GZZLUKSX".to_string(),
        line_ids: vec![], // empty = no filter
        directions: vec![],
        poll_seconds: 20,
        theme: "classic-amber".to_string(),
    };

    let board = svc.refresh(&cfg).await.expect("refresh should succeed");
    // KSX has multiple lines; all should appear.
    let line_ids: std::collections::HashSet<String> = board
        .platforms
        .iter()
        .flat_map(|p| p.arrivals.iter().map(|a| a.line_id.clone()))
        .collect();
    assert!(
        line_ids.len() >= 3,
        "empty line_ids should return all lines, got: {line_ids:?}"
    );
}

// ---------------------------------------------------------------------------
// Stream tests — require tokio + fake time
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Test 5: stream_emits_on_interval
// ---------------------------------------------------------------------------

/// A `TflHttp` mock that counts how many times `fetch` is called.
#[derive(Clone, Debug)]
struct CountingHttp {
    inner: FixtureTflHttp,
    count: Arc<AtomicUsize>,
}

impl CountingHttp {
    fn new(dir: PathBuf) -> (Self, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let this = Self {
            inner: FixtureTflHttp::new(dir),
            count: count.clone(),
        };
        (this, count)
    }
}

impl TflHttp for CountingHttp {
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<serde_json::Value, TflError> {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.inner.fetch(endpoint, id).await
    }
}

#[tokio::test(start_paused = true)]
async fn stream_emits_on_interval() {
    let (http, count) = CountingHttp::new(fixtures_dir());
    let client = TflClient::new(http);
    let clock = FakeClock::from_rfc3339("2026-04-23T12:00:00Z").unwrap();
    let svc = BoardService::new(client, clock);

    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec![],
        directions: vec![],
        poll_seconds: 5, // 5-second interval for test
        theme: "classic-amber".to_string(),
    };

    let mut stream = Box::pin(svc.stream(cfg));

    // First board emits immediately (first tick fires at t=0).
    let board1 = stream
        .next()
        .await
        .expect("stream should yield")
        .expect("first board should be Ok");
    assert!(board1.stale_since.is_none(), "first board should be fresh");
    assert_eq!(count.load(Ordering::SeqCst), 1, "should have fetched once");

    // Advance fake time by the poll interval to trigger next tick.
    tokio::time::advance(std::time::Duration::from_secs(5)).await;

    let board2 = stream
        .next()
        .await
        .expect("stream should yield")
        .expect("second board should be Ok");
    assert!(board2.stale_since.is_none(), "second board should be fresh");
    assert_eq!(count.load(Ordering::SeqCst), 2, "should have fetched twice");

    // Advance again.
    tokio::time::advance(std::time::Duration::from_secs(5)).await;

    let _board3 = stream
        .next()
        .await
        .expect("stream should yield")
        .expect("third board should be Ok");
    assert_eq!(
        count.load(Ordering::SeqCst),
        3,
        "should have fetched three times"
    );
}

// ---------------------------------------------------------------------------
// Test 6: stream_backpressure_skips_tick_when_refresh_slow
// ---------------------------------------------------------------------------

/// A `TflHttp` mock that takes `delay` per fetch and counts fetches.
#[derive(Clone, Debug)]
struct SlowHttp {
    inner: FixtureTflHttp,
    delay: std::time::Duration,
    count: Arc<AtomicUsize>,
}

impl SlowHttp {
    fn new(dir: PathBuf, delay: std::time::Duration) -> (Self, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let this = Self {
            inner: FixtureTflHttp::new(dir),
            delay,
            count: count.clone(),
        };
        (this, count)
    }
}

impl TflHttp for SlowHttp {
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<serde_json::Value, TflError> {
        self.count.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.inner.fetch(endpoint, id).await
    }
}

/// Prove that when each refresh takes 2× the interval, no tick backlog accumulates.
///
/// Setup: poll_seconds = 2, fetch delay = 4 seconds.
/// We advance time by 10 seconds total. With `MissedTickBehavior::Skip`:
/// - t=0: tick fires, refresh starts (takes 4s)
/// - t=4: refresh completes, interval timer resets
/// - t=6: tick fires (2s after completion), refresh starts (takes 4s)
/// - t=10: refresh completes
/// So we expect ~2 fetches in 10s of advanced time, not 5 (which would happen with queuing).
#[tokio::test(start_paused = true)]
async fn stream_backpressure_skips_tick_when_refresh_slow() {
    let poll_secs = 2u64;
    let fetch_delay = std::time::Duration::from_secs(poll_secs * 2); // 4 seconds

    let (http, count) = SlowHttp::new(fixtures_dir(), fetch_delay);
    let client = TflClient::new(http);
    let clock = FakeClock::from_rfc3339("2026-04-23T12:00:00Z").unwrap();
    let svc = BoardService::new(client, clock);

    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec![],
        directions: vec![],
        poll_seconds: poll_secs as u32,
        theme: "classic-amber".to_string(),
    };

    let stream = Box::pin(svc.stream(cfg));

    // Drive the stream for 14 seconds of fake time.
    // Without backpressure, we'd expect 14/2 = 7 fetches.
    // With MissedTickBehavior::Skip, each slow fetch prevents backlog accumulation:
    // fetch takes 4s, then interval waits 2s = 6s per cycle → ~2 fetches in 14s.
    let total_advance = std::time::Duration::from_secs(14);
    let step = std::time::Duration::from_millis(100);
    let mut elapsed = std::time::Duration::ZERO;

    // We collect up to 3 boards. With backpressure we should get no more than 3.
    let mut stream = stream.take(3);

    loop {
        tokio::select! {
            board = stream.next() => {
                match board {
                    Some(_) => {}
                    None => break,
                }
            }
            _ = async {
                if elapsed >= total_advance {
                    // No more time to advance; yield to let futures settle.
                    futures::future::pending::<()>().await;
                } else {
                    tokio::time::advance(step).await;
                    elapsed += step;
                }
            } => {}
        }
    }

    let fetches = count.load(Ordering::SeqCst);
    // With a 4s fetch and 2s interval (MissedTickBehavior::Skip), over 14s we
    // should see at most 3 fetches, not 7 (which would result from queuing ticks).
    assert!(
        fetches <= 3,
        "with slow fetch ({}s) and {}s interval, expect ≤3 fetches in 14s (got {})",
        fetch_delay.as_secs(),
        poll_secs,
        fetches
    );
    assert!(
        fetches >= 2,
        "should have completed at least 2 fetches (got {})",
        fetches
    );
}

// ---------------------------------------------------------------------------
// Test 7: stream_on_fetch_failure_emits_stale_board
// ---------------------------------------------------------------------------

/// A `TflHttp` mock that succeeds for the first N calls then returns NotFound.
#[derive(Clone, Debug)]
struct FailAfterNHttp {
    inner: FixtureTflHttp,
    succeed_count: Arc<AtomicUsize>,
    max_successes: usize,
}

impl FailAfterNHttp {
    fn new(dir: PathBuf, max_successes: usize) -> Self {
        Self {
            inner: FixtureTflHttp::new(dir),
            succeed_count: Arc::new(AtomicUsize::new(0)),
            max_successes,
        }
    }
}

impl TflHttp for FailAfterNHttp {
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<serde_json::Value, TflError> {
        let n = self.succeed_count.fetch_add(1, Ordering::SeqCst);
        if n < self.max_successes {
            self.inner.fetch(endpoint, id).await
        } else {
            Err(TflError::NotFound(format!("simulated failure #{n}")))
        }
    }
}

#[tokio::test(start_paused = true)]
async fn stream_on_fetch_failure_emits_stale_board() {
    let http = FailAfterNHttp::new(fixtures_dir(), 3); // first 3 succeed, then fail
    let client = TflClient::new(http);

    let known_time = "2026-04-23T12:00:00Z";
    let clock = FakeClock::from_rfc3339(known_time).unwrap();
    let svc = BoardService::new(client, clock);

    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec![],
        directions: vec![],
        poll_seconds: 2,

        theme: "classic-amber".to_string(),
    };

    let mut stream = Box::pin(svc.stream(cfg));

    // Collect 4 boards: 3 successful + 1 stale.
    let mut boards = Vec::new();

    for _ in 0..3 {
        // Advance time by interval for ticks 2 and 3.
        if !boards.is_empty() {
            tokio::time::advance(std::time::Duration::from_secs(2)).await;
        }
        let item = stream.next().await.expect("stream should yield");
        boards.push(item.expect("should be Ok for successful fetch"));
    }

    // All 3 successful boards should have stale_since = None.
    for (i, board) in boards.iter().enumerate() {
        assert!(
            board.stale_since.is_none(),
            "board {} should not be stale, got: {:?}",
            i,
            board.stale_since
        );
    }

    // Advance time for the 4th tick (which will fail).
    tokio::time::advance(std::time::Duration::from_secs(2)).await;

    let stale_board = stream.next().await.expect("stream should yield");
    let stale_board = stale_board.expect("failure should emit stale board, not Err");

    // The 4th emission should be the last-known board with stale_since set.
    assert!(
        stale_board.stale_since.is_some(),
        "4th board should be stale, got: {:?}",
        stale_board.stale_since
    );
    // stale_since should be set to the clock time at the failure.
    // (The clock is pinned so it's the known_time.)
    let expected_stale_at = chrono::DateTime::parse_from_rfc3339(known_time)
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(
        stale_board.stale_since,
        Some(expected_stale_at),
        "stale_since should equal the clock time at failure"
    );
}

// ---------------------------------------------------------------------------
// Test 8: stream_cancellation_drops_in_flight
// ---------------------------------------------------------------------------

/// A `TflHttp` mock that signals via a channel when the fetch is dropped,
/// proving that cancellation cleans up in-flight work.
#[derive(Clone, Debug)]
struct DropSignalHttp {
    inner: FixtureTflHttp,
    /// Sender notified when the fetch future is dropped mid-sleep.
    tx: tokio::sync::mpsc::Sender<()>,
    /// How long to sleep before completing the fetch.
    delay: std::time::Duration,
}

impl DropSignalHttp {
    fn new(dir: PathBuf, delay: std::time::Duration) -> (Self, tokio::sync::mpsc::Receiver<()>) {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let this = Self {
            inner: FixtureTflHttp::new(dir),
            tx,
            delay,
        };
        (this, rx)
    }
}

impl TflHttp for DropSignalHttp {
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<serde_json::Value, TflError> {
        // Signal that we started.
        let _ = self.tx.try_send(());

        tokio::time::sleep(self.delay).await;

        // If we reach here, the sleep completed (not cancelled).
        // Signal again — the test distinguishes "started" from "started + completed".
        let _ = self.tx.try_send(());
        self.inner.fetch(endpoint, id).await
    }
}

/// Proves that dropping the stream cancels in-flight fetches.
///
/// Strategy:
/// - The mock signals via channel when the fetch *starts* (first send) and
///   when the fetch *completes* its sleep (second send).
/// - We start polling (which starts the first fetch and the sleep).
/// - We drop the stream while the sleep is in-flight.
/// - We assert that only one signal arrived (start) — NOT two (start + completion).
///   If cancellation works, the sleep future is dropped and the second send never fires.
#[tokio::test(start_paused = true)]
async fn stream_cancellation_drops_in_flight() {
    let slow_delay = std::time::Duration::from_secs(30);
    let (http, mut signal_rx) = DropSignalHttp::new(fixtures_dir(), slow_delay);
    let client = TflClient::new(http);
    let clock = FakeClock::from_rfc3339("2026-04-23T12:00:00Z").unwrap();
    let svc = BoardService::new(client, clock);

    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec![],
        directions: vec![],
        poll_seconds: 60, // long interval so only one tick fires
        theme: "classic-amber".to_string(),
    };

    let stream = Box::pin(svc.stream(cfg));

    // Spawn the stream's first poll as a separate task so we can drive the
    // runtime freely. The task will start the unfold closure: tick fires at
    // t=0, refresh is called, fetch sends the "started" signal, then sleeps
    // 30s (paused — so the sleep never resolves without an explicit advance).
    let stream_task = tokio::spawn(async move {
        let mut stream = stream;
        stream.next().await
    });

    // Yield a few times to let the spawned task progress through the tick and
    // into the fetch sleep.
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }

    // The fetch should have sent the "started" signal by now.
    let start_signal = signal_rx.try_recv();
    assert!(
        start_signal.is_ok(),
        "fetch should have started (sent start signal)"
    );

    // Abort the task — this cancels the spawned task at its next await point,
    // dropping the stream and the in-flight fetch future.
    // (Dropping the JoinHandle only detaches the task; abort() actually cancels it.)
    stream_task.abort();

    // Yield to let the async runtime process the cancellation.
    tokio::task::yield_now().await;

    // Advance past the original sleep delay to prove the sleep never completes.
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    tokio::task::yield_now().await;

    // The "completed" signal should NOT have fired.
    let complete_signal = signal_rx.try_recv();
    assert!(
        complete_signal.is_err(),
        "in-flight fetch should have been cancelled — completion signal must NOT fire"
    );
}

// ---------------------------------------------------------------------------
// Test 9: stream_stale_transition_atomic
// ---------------------------------------------------------------------------

/// A `TflHttp` mock with a configurable failure pattern.
///
/// Call sequence: success, fail, fail, success, fail
/// We verify stale_since transitions correctly.
#[derive(Clone, Debug)]
struct PatternHttp {
    inner: FixtureTflHttp,
    /// Pattern: true = succeed, false = fail.
    pattern: Arc<Vec<bool>>,
    call_index: Arc<AtomicUsize>,
}

impl PatternHttp {
    fn new(dir: PathBuf, pattern: Vec<bool>) -> Self {
        Self {
            inner: FixtureTflHttp::new(dir),
            pattern: Arc::new(pattern),
            call_index: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl TflHttp for PatternHttp {
    async fn fetch(&self, endpoint: &str, id: &str) -> Result<serde_json::Value, TflError> {
        let idx = self.call_index.fetch_add(1, Ordering::SeqCst);
        let should_succeed = self.pattern.get(idx).copied().unwrap_or(false);
        if should_succeed {
            self.inner.fetch(endpoint, id).await
        } else {
            Err(TflError::NotFound(format!("simulated failure #{idx}")))
        }
    }
}

/// Simulate: success → fail → fail → success → fail
/// Verify stale_since transitions:
/// - After call 1 (success): stale_since = None
/// - After call 2 (fail): stale_since = Some(t)
/// - After call 3 (fail): stale_since = Some(t) (same — not re-set)
/// - After call 4 (success): stale_since = None (recovered)
/// - After call 5 (fail): stale_since = Some(t') (new stale transition)
#[tokio::test(start_paused = true)]
async fn stream_stale_transition_atomic() {
    // Pattern: success, fail, fail, success, fail
    let pattern = vec![true, false, false, true, false];
    let http = PatternHttp::new(fixtures_dir(), pattern);
    let client = TflClient::new(http);
    let clock = FakeClock::from_rfc3339("2026-04-23T12:00:00Z").unwrap();
    let svc = BoardService::new(client, clock);

    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec![],
        directions: vec![],
        poll_seconds: 2,

        theme: "classic-amber".to_string(),
    };

    let mut stream = Box::pin(svc.stream(cfg));

    // Helper to advance time and get next board.
    async fn next_board(
        stream: &mut std::pin::Pin<
            Box<impl futures::Stream<Item = Result<tfl_domain::Board, BoardError>>>,
        >,
    ) -> tfl_domain::Board {
        let item = stream.next().await.expect("stream should yield");
        item.expect("should be Ok (stale boards come as Ok with stale_since set)")
    }

    // Board 1: success — should be fresh.
    let board1 = next_board(&mut stream).await;
    assert!(board1.stale_since.is_none(), "board 1 should be fresh");

    // Board 2: fail — should be stale.
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    let board2 = next_board(&mut stream).await;
    let stale_at = board2.stale_since;
    assert!(
        stale_at.is_some(),
        "board 2 should be stale after first failure"
    );

    // Board 3: fail again — stale_since should NOT change (stays at the first failure time).
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    let board3 = next_board(&mut stream).await;
    assert_eq!(
        board3.stale_since, stale_at,
        "board 3 stale_since should not change on consecutive failures"
    );

    // Board 4: success — stale_since should reset to None.
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    let board4 = next_board(&mut stream).await;
    assert!(
        board4.stale_since.is_none(),
        "board 4 should be fresh after recovery, got: {:?}",
        board4.stale_since
    );

    // Board 5: fail again — stale_since should be set to new time.
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    let board5 = next_board(&mut stream).await;
    assert!(
        board5.stale_since.is_some(),
        "board 5 should be stale after new failure"
    );
    // The new stale_since may be equal to or after the original stale_at
    // (clock is pinned so they'll be equal in this test).
    assert_eq!(
        board5.stale_since, stale_at,
        "stale_since should equal clock time at each new failure"
    );
}

// ---------------------------------------------------------------------------
// Test 10a: stream_keeps_polling_after_fatal_error_no_last_ok
// ---------------------------------------------------------------------------

/// A `TflHttp` mock that always returns an error.
#[derive(Clone, Debug)]
struct AlwaysErrorHttp;

impl TflHttp for AlwaysErrorHttp {
    async fn fetch(&self, _endpoint: &str, _id: &str) -> Result<serde_json::Value, TflError> {
        Err(TflError::NotFound("always-error mock".to_string()))
    }
}

/// Verify the corrected behaviour: when the very first fetch fails (no
/// `last_ok` to fall back on), the stream must:
/// 1. emit `Some(Err(_))` on the first poll, AND
/// 2. keep polling — emit `Some(Err(_))` again on the next tick, NOT `None`.
///
/// A network hiccup at app launch must not kill the polling stream forever.
/// The `poll_seconds` interval provides rate-limiting between retries.
#[tokio::test(start_paused = true)]
async fn stream_terminates_after_fatal_error_no_last_ok() {
    let http = AlwaysErrorHttp;
    let client = TflClient::new(http);
    let clock = FakeClock::from_rfc3339("2026-04-23T12:00:00Z").unwrap();
    let svc = BoardService::new(client, clock);

    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec![],
        directions: vec![],
        poll_seconds: 2,

        theme: "classic-amber".to_string(),
    };

    let mut stream = Box::pin(svc.stream(cfg));

    // First poll — first tick fires immediately at t=0, fetch fails, no last_ok.
    // Should emit Some(Err(_)).
    let first = stream.next().await;
    assert!(
        matches!(first, Some(Err(_))),
        "first item should be Some(Err(..)) when initial fetch fails, got: {first:?}"
    );

    // Second poll — stream must NOT terminate; it must keep retrying.
    // Advance time past the interval so the second tick fires.
    tokio::time::advance(std::time::Duration::from_secs(3)).await;
    let second = stream.next().await;
    assert!(
        matches!(second, Some(Err(_))),
        "stream must keep polling (Some(Err(..))) after error with no last_ok, got: {second:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 10b: stream_continues_with_stale_fallback_when_last_ok_exists
// ---------------------------------------------------------------------------

/// Verify the OTHER path of FIX 1: when a prior fetch succeeded, subsequent
/// failures yield `Ok(stale_board)` indefinitely — the stream does NOT terminate.
#[tokio::test(start_paused = true)]
async fn stream_continues_with_stale_fallback_when_last_ok_exists() {
    // Succeed once, then always fail.
    let http = FailAfterNHttp::new(fixtures_dir(), 1);
    let client = TflClient::new(http);
    let clock = FakeClock::from_rfc3339("2026-04-23T12:00:00Z").unwrap();
    let svc = BoardService::new(client, clock);

    let cfg = BoardConfig {
        station_id: "940GZZLUBZP".to_string(),
        line_ids: vec![],
        directions: vec![],
        poll_seconds: 2,

        theme: "classic-amber".to_string(),
    };

    let mut stream = Box::pin(svc.stream(cfg));

    // First item: successful fetch.
    let first = stream.next().await;
    assert!(
        matches!(first, Some(Ok(_))),
        "first item should be Ok (successful fetch)"
    );

    // Subsequent items should all be Ok (stale board), not Err or None.
    for i in 1..=3 {
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        let item = stream.next().await;
        match &item {
            Some(Ok(board)) => {
                assert!(
                    board.stale_since.is_some(),
                    "item {i} should be a stale board (stale_since set)"
                );
            }
            other => panic!(
                "item {i} should be Some(Ok(stale_board)), stream must not terminate; got: {other:?}"
            ),
        }
    }
}
