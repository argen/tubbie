//! `BoardService` — single-shot refresh and polling stream.
//!
//! ## Stream semantics
//!
//! `stream` uses `futures::stream::unfold` with `tokio::time::interval` set to
//! `MissedTickBehavior::Skip`. This means:
//! - If a refresh takes longer than the interval, the missed tick is discarded.
//!   The next tick fires only after the interval elapses from the *completion*
//!   of the previous refresh. This prevents unbounded backlog on slow networks.
//! - The stream yields a `Board` immediately (first tick at t=0) and then on
//!   each subsequent interval.
//!
//! ## Stale-data transitions
//!
//! `last_ok: Option<Board>` tracks the most recent successful board. On fetch
//! failure, the last-ok board is re-emitted with `stale_since` set to the clock
//! time at the failure. The `stale_since` field is only ever set forward
//! (monotonically increasing); a success resets `stale_since` to `None`.
//!
//! ## Cancellation safety
//!
//! `stream` is implemented as `unfold` + a single `await` point per step.
//! Dropping the returned `Stream` cancels the in-flight `refresh` future at the
//! next `await` suspension point. No `tokio::spawn` is used; no tasks outlive
//! the stream.

use std::time::Duration;

use futures::stream::{self, Stream};
use tokio::time::{interval, MissedTickBehavior};

use tfl_client::{clock::Clock, http::TflHttp, TflClient};
use tfl_domain::{Arrival, Board, LineStatus, Platform, Station};

use crate::config::BoardConfig;
use crate::error::BoardError;
use crate::filter::apply_filters;

/// The board service. Generic over any `TflHttp` transport and `Clock`.
///
/// Inject a `FixtureTflHttp` + `FakeClock` for offline tests;
/// inject `ReqwestTflHttp` + `SystemClock` for production use.
pub struct BoardService<H: TflHttp, C: Clock> {
    client: TflClient<H>,
    clock: C,
}

impl<H: TflHttp, C: Clock> BoardService<H, C> {
    /// Create a new `BoardService` wrapping the given client and clock.
    pub fn new(client: TflClient<H>, clock: C) -> Self {
        Self { client, clock }
    }
}

impl<H: TflHttp + 'static, C: Clock + 'static> BoardService<H, C> {
    /// Search for tube stations matching `query`.
    ///
    /// Delegates to `TflClient::search_stations`. Results are unfiltered —
    /// command-layer validation has already rejected malicious inputs.
    ///
    /// # Errors
    /// Returns `BoardError::Fetch` if the TfL client returns an error.
    pub async fn search_stations(&self, query: &str) -> Result<Vec<Station>, BoardError> {
        Ok(self.client.search_stations(query).await?)
    }

    /// Fetch the current status for a single TfL line.
    ///
    /// Delegates to `TflClient::get_line_status`.
    ///
    /// # Errors
    /// Returns `BoardError::Fetch` if the TfL client returns an error.
    pub async fn get_line_status(&self, line_id: &str) -> Result<LineStatus, BoardError> {
        Ok(self.client.get_line_status(line_id).await?)
    }

    /// Fetch and filter arrivals for one station, returning a `Board`.
    ///
    /// `generated_at` is set from the injected clock, never from `Utc::now()`.
    /// `stale_since` is always `None` on a successful refresh — the caller
    /// (or the stream loop) sets it when needed.
    ///
    /// # Errors
    /// Returns `BoardError::Fetch` if the TfL client returns an error.
    pub async fn refresh(&self, cfg: &BoardConfig) -> Result<Board, BoardError> {
        let raw_arrivals = self.client.get_arrivals(&cfg.station_id).await?;
        let filtered = apply_filters(raw_arrivals, cfg);
        let board = build_board(&cfg.station_id, filtered, self.clock.now(), None);
        Ok(board)
    }

    /// Produce an infinite stream of `Board` snapshots.
    ///
    /// - Emits a board immediately on subscription.
    /// - Then emits on each interval (default `cfg.poll_seconds`).
    /// - `MissedTickBehavior::Skip` — if a refresh outlasts one interval, the
    ///   missed tick is dropped. At most one refresh is in flight at a time.
    /// - On fetch failure, re-emits the last-known board with `stale_since` set.
    ///   On fetch failure with no last-ok, the error is emitted and the stream
    ///   KEEPS POLLING. The `poll_seconds` interval provides rate-limiting between
    ///   retries.
    /// - Dropping the stream cancels the in-flight refresh future. No tasks leak.
    pub fn stream(self, cfg: BoardConfig) -> impl Stream<Item = Result<Board, BoardError>> + Send {
        let poll_dur = Duration::from_secs(u64::from(cfg.poll_seconds).max(1));

        stream::unfold(
            // State: (service, config, interval, last_ok_board)
            (
                self,
                cfg,
                {
                    let mut ivl = interval(poll_dur);
                    ivl.set_missed_tick_behavior(MissedTickBehavior::Skip);
                    ivl
                },
                None::<Board>,
            ),
            |(svc, cfg, mut ivl, last_ok)| async move {
                // Wait for the next tick (first tick fires immediately).
                ivl.tick().await;

                match svc.refresh(&cfg).await {
                    Ok(mut board) => {
                        // Success: clear stale_since, record as last_ok.
                        board.stale_since = None;
                        let emit = board.clone();
                        Some((Ok(emit), (svc, cfg, ivl, Some(board))))
                    }
                    Err(e) => {
                        if let Some(mut stale) = last_ok {
                            // We have a previous good board — mark it stale and re-emit.
                            // Only set stale_since if not already set (first failure).
                            if stale.stale_since.is_none() {
                                stale.stale_since = Some(svc.clock.now());
                            }
                            let emit = stale.clone();
                            Some((Ok(emit), (svc, cfg, ivl, Some(stale))))
                        } else {
                            // No previous good board — emit the error but keep polling.
                            // The next tick will retry; poll_seconds rate-limits retries.
                            Some((Err(e), (svc, cfg, ivl, None)))
                        }
                    }
                }
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Group filtered arrivals by platform name and build a `Board`.
///
/// Platforms are sorted by name for deterministic output.
/// Arrivals within each platform are sorted by `time_to_station` ascending.
fn build_board(
    station_id: &str,
    arrivals: Vec<Arrival>,
    generated_at: chrono::DateTime<chrono::Utc>,
    stale_since: Option<chrono::DateTime<chrono::Utc>>,
) -> Board {
    // Group by platform_name.
    let mut platform_map: std::collections::BTreeMap<String, Vec<Arrival>> =
        std::collections::BTreeMap::new();
    for arrival in arrivals {
        platform_map
            .entry(arrival.platform_name.clone())
            .or_default()
            .push(arrival);
    }

    // Sort each platform's arrivals by time_to_station ascending.
    let platforms: Vec<Platform> = platform_map
        .into_iter()
        .map(|(name, mut arrivals)| {
            arrivals.sort_by_key(|a| a.time_to_station);
            Platform { name, arrivals }
        })
        .collect();

    Board {
        station_id: station_id.to_string(),
        platforms,
        generated_at,
        stale_since,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use futures::StreamExt;
    use serde_json::Value;
    use tfl_client::{clock::FakeClock, error::TflError, http::TflHttp, TflClient};

    // -----------------------------------------------------------------------
    // Minimal mock TflHttp that returns a pre-programmed sequence of results.
    //
    // `TflError` is not `Clone`, so we store a `bool` sequence (true = ok,
    // false = error) and reconstruct the values on each call.
    // -----------------------------------------------------------------------

    /// A mock `TflHttp` whose N-th call succeeds iff `successes[N % len]` is
    /// `true`. An empty JSON array is returned on success; a 500 Http error on
    /// failure. An `Arc<AtomicU32>` tracks the call count for assertions.
    struct SeqTflHttp {
        successes: Vec<bool>,
        call_count: Arc<AtomicU32>,
    }

    impl SeqTflHttp {
        fn new(successes: Vec<bool>) -> (Self, Arc<AtomicU32>) {
            let counter = Arc::new(AtomicU32::new(0));
            let mock = SeqTflHttp {
                successes,
                call_count: Arc::clone(&counter),
            };
            (mock, counter)
        }
    }

    impl TflHttp for SeqTflHttp {
        async fn fetch(&self, _endpoint: &str, _id: &str) -> Result<Value, TflError> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst) as usize;
            if self.successes[idx % self.successes.len()] {
                Ok(serde_json::json!([]))
            } else {
                Err(TflError::Http {
                    status: 500,
                    body_snippet: "server error".to_string(),
                })
            }
        }
    }

    fn make_cfg() -> BoardConfig {
        BoardConfig {
            station_id: "TEST001".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 1,
            theme: "classic-amber".to_string(),
        }
    }

    fn make_clock() -> FakeClock {
        FakeClock::from_rfc3339("2026-04-24T12:00:00Z").unwrap()
    }

    // -----------------------------------------------------------------------
    // Test: stream retries after initial fetch failure (the bug fix)
    // -----------------------------------------------------------------------

    /// After an initial fetch failure with no last_ok, the stream must NOT
    /// terminate. The next tick must retry and — if it succeeds — yield `Ok(Board)`.
    #[tokio::test(start_paused = true)]
    async fn stream_retries_after_initial_failure() {
        // Call 0: error; call 1: success.
        let (mock, _counter) = SeqTflHttp::new(vec![false, true]);
        let client = TflClient::new(mock);
        let svc = BoardService::new(client, make_clock());

        let mut stream = Box::pin(svc.stream(make_cfg()));

        // First item: should be an error (no last_ok yet).
        let first = stream.next().await.expect("stream must not terminate");
        assert!(
            first.is_err(),
            "first item must be Err when initial fetch fails, got: {first:?}"
        );

        // Advance past the poll interval so the next tick fires.
        tokio::time::advance(Duration::from_secs(2)).await;

        // Second item: fetch succeeded — must yield Ok(Board), NOT None.
        let second = stream
            .next()
            .await
            .expect("stream must continue after error");
        assert!(
            second.is_ok(),
            "second item must be Ok after recovery, got: {second:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Test: stream keeps emitting errors on repeated failures (no last_ok)
    // -----------------------------------------------------------------------

    /// When every fetch fails and there is no last_ok, the stream must keep
    /// emitting errors — it must never terminate on its own.
    #[tokio::test(start_paused = true)]
    async fn stream_keeps_emitting_errors_without_last_ok() {
        let (mock, _counter) = SeqTflHttp::new(vec![false]);
        let client = TflClient::new(mock);
        let svc = BoardService::new(client, make_clock());

        let mut stream = Box::pin(svc.stream(make_cfg()));

        // Collect three consecutive error items.
        for i in 0..3u32 {
            tokio::time::advance(Duration::from_secs(2)).await;
            let item = stream
                .next()
                .await
                .unwrap_or_else(|| panic!("stream terminated at item {i}, expected Err"));
            assert!(item.is_err(), "item {i} should be Err, got: {item:?}");
        }
    }

    // -----------------------------------------------------------------------
    // Test: stale-data semantics are unchanged when last_ok is Some
    // -----------------------------------------------------------------------

    /// After a successful fetch, a subsequent failure must re-emit the previous
    /// board with `stale_since` set — not an error.
    #[tokio::test(start_paused = true)]
    async fn stream_emits_stale_board_after_success_then_failure() {
        // Call 0: success; call 1: failure.
        let (mock, _counter) = SeqTflHttp::new(vec![true, false]);
        let client = TflClient::new(mock);
        let svc = BoardService::new(client, make_clock());

        let mut stream = Box::pin(svc.stream(make_cfg()));

        // First item: success.
        let first = stream.next().await.expect("stream must not terminate");
        let ok_board = first.expect("first item must be Ok");
        assert!(
            ok_board.stale_since.is_none(),
            "fresh board must not be stale"
        );

        // Advance so the next tick fires.
        tokio::time::advance(Duration::from_secs(2)).await;

        // Second item: fetch fails — must get the stale board, NOT an Err.
        let second = stream.next().await.expect("stream must not terminate");
        let stale_board = second.expect("second item must be Ok (stale fallback)");
        assert!(
            stale_board.stale_since.is_some(),
            "stale board must have stale_since set"
        );
        assert_eq!(
            stale_board.station_id, ok_board.station_id,
            "stale board must be the same station"
        );
    }
}
