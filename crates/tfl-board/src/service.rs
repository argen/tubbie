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

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, Stream};
use tokio::time::{interval, MissedTickBehavior};

use tfl_client::{clock::Clock, http::TflHttp, TflClient};
use tfl_domain::{Arrival, Board, Direction, LineStatus, Platform, Station};

use crate::config::BoardConfig;
use crate::error::BoardError;
use crate::filter::apply_filters;

/// The board service. Generic over any `TflHttp` transport and `Clock`.
///
/// Inject a `FixtureTflHttp` + `FakeClock` for offline tests;
/// inject `ReqwestTflHttp` + `SystemClock` for production use.
///
/// The TfL client is held as an `Arc` so a single client (with its caches —
/// `stop_points_cache`, `hub_children_cache`, `line_status_cache`) can be
/// shared between the on-demand command path (`AppState::board_service`)
/// and the polling stream task. Sharing the client is what lets
/// `save_config` mutate stream behaviour without a 16 MB stop-points
/// re-warm on every chip click.
pub struct BoardService<H: TflHttp, C: Clock> {
    client: Arc<TflClient<H>>,
    clock: C,
}

impl<H: TflHttp, C: Clock> BoardService<H, C> {
    /// Create a new `BoardService` wrapping the given client and clock.
    pub fn new(client: Arc<TflClient<H>>, clock: C) -> Self {
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

    /// Pre-fetch the tube stop-points list so the first settings-search is
    /// instant. Fire-and-forget from app startup; safe to call repeatedly.
    pub async fn warm_stop_points_cache(&self) -> Result<usize, BoardError> {
        Ok(self.client.warm_stop_points_cache().await?)
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

    /// Produce an infinite stream of `Board` snapshots driven by a live
    /// config channel.
    ///
    /// The stream observes `cfg_rx` and applies non-`station_id` config
    /// changes (theme, line filter, directions, `poll_seconds`) on the next
    /// tick *without* respawning. This is what lets the Settings UI
    /// chip-toggle 12 lines in 1 s without triggering 12 stop-points
    /// re-warms or 12 fresh arrivals fetches.
    ///
    /// Per-tick semantics:
    /// - Wait for the next interval tick **or** a `cfg_rx.changed()`
    ///   notification (`tokio::select!`).
    /// - If `poll_seconds` changed, rebuild the `Interval` so the next tick
    ///   honours the new period.
    /// - If `station_id` changed, drop `last_ok` so the previous station's
    ///   data isn't re-emitted under the new station_id.
    /// - On a tick: refresh + emit (`MissedTickBehavior::Skip` keeps at most
    ///   one refresh in flight). On failure with a `last_ok`, re-emit it
    ///   marked stale; on failure with no `last_ok`, emit the error and
    ///   keep polling.
    /// - On a `CfgChanged` wake-up, do **not** issue a fresh fetch — just
    ///   continue. The next tick will refresh against the new config. This
    ///   "cheap" semantic avoids latency-shifting work onto every save and
    ///   keeps the UI fully responsive on rapid chip toggles. Filter
    ///   changes appear within at most `poll_seconds` of the toggle.
    /// - Dropping the stream cancels any in-flight refresh future at its
    ///   next `await`. No tasks leak. The stream ends only when every
    ///   `cfg_tx` (the watch sender) is dropped.
    pub fn stream(
        self,
        cfg_rx: tokio::sync::watch::Receiver<BoardConfig>,
    ) -> impl Stream<Item = Result<Board, BoardError>> + Send {
        let initial_cfg = cfg_rx.borrow().clone();
        let initial_dur = Duration::from_secs(u64::from(initial_cfg.poll_seconds).max(1));
        let mut ivl = interval(initial_dur);
        ivl.set_missed_tick_behavior(MissedTickBehavior::Skip);

        stream::unfold(
            // State: (service, current cfg, interval, last_ok_board, cfg_rx)
            (self, initial_cfg, ivl, None::<Board>, cfg_rx),
            |(svc, mut cur_cfg, mut ivl, mut last_ok, mut cfg_rx)| async move {
                // Drive forward until we have something to emit. CfgChanged
                // wake-ups apply config-driven side effects (interval rebuild,
                // last_ok drop on station_id change) but do **not** issue a
                // fetch — the next interval tick refreshes against the new
                // cfg. This is the "cheap" option from the design: it
                // amortises a click-burst into at most one refresh per
                // poll_seconds, instead of one per save_config.
                loop {
                    let outcome = tokio::select! {
                        _ = ivl.tick() => TickOutcome::Tick,
                        changed = cfg_rx.changed() => match changed {
                            Ok(()) => TickOutcome::CfgChanged,
                            // All senders dropped — terminate the stream.
                            Err(_) => return None,
                        },
                    };

                    // `borrow()` always returns the freshest value, even if
                    // multiple `send`s have stacked since the wake-up.
                    let new_cfg: BoardConfig = cfg_rx.borrow().clone();

                    if new_cfg.poll_seconds != cur_cfg.poll_seconds {
                        let new_dur = Duration::from_secs(u64::from(new_cfg.poll_seconds).max(1));
                        ivl = interval(new_dur);
                        ivl.set_missed_tick_behavior(MissedTickBehavior::Skip);
                    }
                    let station_changed = new_cfg.station_id != cur_cfg.station_id;
                    if station_changed {
                        // Drop stale data for the previous station; the next
                        // refresh produces a fresh board for the new station.
                        last_ok = None;
                    }
                    cur_cfg = new_cfg;

                    if matches!(outcome, TickOutcome::CfgChanged) && !station_changed {
                        // Cheap semantic for theme / directions / lines /
                        // poll_seconds: apply config side effects and wait
                        // for the next tick. The user already sees the old
                        // station's data and a filter change can wait a
                        // poll cycle.
                        continue;
                    }

                    if matches!(outcome, TickOutcome::CfgChanged) && station_changed {
                        // Station change is a deliberate user action that
                        // demands immediate feedback. Refresh now and reset
                        // the interval so the next periodic tick fires one
                        // poll_seconds from this forced refresh, not from
                        // the previously-scheduled time.
                        ivl.reset();
                    }

                    // TickOutcome::Tick — refresh and emit.
                    match svc.refresh(&cur_cfg).await {
                        Ok(mut board) => {
                            board.stale_since = None;
                            let emit = board.clone();
                            return Some((Ok(emit), (svc, cur_cfg, ivl, Some(board), cfg_rx)));
                        }
                        Err(e) => {
                            if let Some(mut stale) = last_ok {
                                if stale.stale_since.is_none() {
                                    stale.stale_since = Some(svc.clock.now());
                                }
                                let emit = stale.clone();
                                return Some((Ok(emit), (svc, cur_cfg, ivl, Some(stale), cfg_rx)));
                            }
                            return Some((Err(e), (svc, cur_cfg, ivl, None, cfg_rx)));
                        }
                    }
                }
            },
        )
    }
}

/// Why the unfold step woke up — used to drive the per-tick branch.
#[derive(Copy, Clone, Debug)]
enum TickOutcome {
    /// The interval timer fired.
    Tick,
    /// `cfg_rx.changed()` resolved — a new `BoardConfig` is in the channel.
    CfgChanged,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Group filtered arrivals by compass direction and build a `Board`.
///
/// At multi-line stations (e.g. Tottenham Court Road serves Central, Northern
/// and Elizabeth), the TfL `platform_name` string carries a per-line platform
/// suffix like `"Westbound - Platform 3"` vs `"Westbound - Platform 5"`. If we
/// grouped on that raw string we'd render two "Westbound" columns for the same
/// compass direction. Instead, each column represents one `Direction` and
/// merges arrivals from every line serving that direction, interleaved by
/// `time_to_station`.
///
/// Directions appear in a fixed reading order; any direction with zero
/// arrivals after filtering is omitted. Arrivals within a column are sorted
/// ascending by `time_to_station`.
fn build_board(
    station_id: &str,
    arrivals: Vec<Arrival>,
    generated_at: chrono::DateTime<chrono::Utc>,
    stale_since: Option<chrono::DateTime<chrono::Utc>>,
) -> Board {
    const DISPLAY_ORDER: [Direction; 7] = [
        Direction::Northbound,
        Direction::Southbound,
        Direction::Eastbound,
        Direction::Westbound,
        Direction::Inbound,
        Direction::Outbound,
        Direction::Unknown,
    ];

    let mut by_direction: std::collections::HashMap<Direction, Vec<Arrival>> =
        std::collections::HashMap::new();
    for arrival in arrivals {
        by_direction
            .entry(arrival.direction)
            .or_default()
            .push(arrival);
    }

    let platforms: Vec<Platform> = DISPLAY_ORDER
        .into_iter()
        .filter_map(|dir| {
            by_direction.remove(&dir).map(|mut arrivals| {
                arrivals.sort_by_key(|a| a.time_to_station);
                Platform {
                    name: direction_label(dir).to_string(),
                    arrivals,
                }
            })
        })
        .collect();

    Board {
        station_id: station_id.to_string(),
        platforms,
        generated_at,
        stale_since,
    }
}

/// Human-readable column label for a `Direction`.
fn direction_label(d: Direction) -> &'static str {
    match d {
        Direction::Northbound => "Northbound",
        Direction::Southbound => "Southbound",
        Direction::Eastbound => "Eastbound",
        Direction::Westbound => "Westbound",
        Direction::Inbound => "Inbound",
        Direction::Outbound => "Outbound",
        Direction::Unknown => "Other",
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
    use tfl_domain::Direction;
    use tokio::sync::watch;

    // -----------------------------------------------------------------------
    // Minimal mock TflHttp that returns a pre-programmed sequence of results.
    //
    // `TflError` is not `Clone`, so we store a `bool` sequence (true = ok,
    // false = error) and reconstruct the values on each call.
    // -----------------------------------------------------------------------

    /// A mock `TflHttp` whose N-th call succeeds iff `successes[N % len]` is
    /// `true`. An empty JSON array is returned on success; a 500 Http error on
    /// failure. An `Arc<AtomicU32>` tracks the call count for assertions, and
    /// `id_log` records every `id` argument so tests can assert which station
    /// the stream actually fetched after a config change.
    struct SeqTflHttp {
        successes: Vec<bool>,
        call_count: Arc<AtomicU32>,
        id_log: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl SeqTflHttp {
        fn new(successes: Vec<bool>) -> (Self, Arc<AtomicU32>) {
            let counter = Arc::new(AtomicU32::new(0));
            let id_log = Arc::new(std::sync::Mutex::new(Vec::new()));
            let mock = SeqTflHttp {
                successes,
                call_count: Arc::clone(&counter),
                id_log,
            };
            (mock, counter)
        }

        /// Variant that exposes the `id_log` Arc so tests can assert the
        /// sequence of station_ids fetched.
        fn new_with_id_log(
            successes: Vec<bool>,
        ) -> (Self, Arc<AtomicU32>, Arc<std::sync::Mutex<Vec<String>>>) {
            let counter = Arc::new(AtomicU32::new(0));
            let id_log = Arc::new(std::sync::Mutex::new(Vec::new()));
            let mock = SeqTflHttp {
                successes,
                call_count: Arc::clone(&counter),
                id_log: Arc::clone(&id_log),
            };
            (mock, counter, id_log)
        }
    }

    impl TflHttp for SeqTflHttp {
        async fn fetch(&self, _endpoint: &str, id: &str) -> Result<Value, TflError> {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst) as usize;
            if let Ok(mut log) = self.id_log.lock() {
                log.push(id.to_string());
            }
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

    /// Build a watch channel seeded with `cfg`. Returns the sender (kept by
    /// the caller so it can publish updates) and the receiver (passed into
    /// `BoardService::stream`).
    fn cfg_channel(cfg: BoardConfig) -> (watch::Sender<BoardConfig>, watch::Receiver<BoardConfig>) {
        watch::channel(cfg)
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
        let client = Arc::new(TflClient::new(mock));
        let svc = BoardService::new(client, make_clock());

        let (_tx, rx) = cfg_channel(make_cfg());
        let mut stream = Box::pin(svc.stream(rx));

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
        let client = Arc::new(TflClient::new(mock));
        let svc = BoardService::new(client, make_clock());

        let (_tx, rx) = cfg_channel(make_cfg());
        let mut stream = Box::pin(svc.stream(rx));

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
        let client = Arc::new(TflClient::new(mock));
        let svc = BoardService::new(client, make_clock());

        let (_tx, rx) = cfg_channel(make_cfg());
        let mut stream = Box::pin(svc.stream(rx));

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

    // -----------------------------------------------------------------------
    // Test (item 4): config-watch — directions change applies without restart
    // -----------------------------------------------------------------------

    /// Sending a new `BoardConfig` with different `directions` through the
    /// watch channel must apply on the next tick **without** the stream task
    /// being recreated. We assert this via the mock's call counter: starting
    /// the stream and driving two ticks with one config change in the middle
    /// must produce exactly two `fetch` calls — not three (which would
    /// indicate a respawn rebuilt its caches and re-fetched immediately).
    #[tokio::test(start_paused = true)]
    async fn stream_picks_up_directions_change_without_restart() {
        // All ticks succeed — we only care about call shape.
        let (mock, counter) = SeqTflHttp::new(vec![true, true, true, true]);
        let client = Arc::new(TflClient::new(mock));
        let svc = BoardService::new(client, make_clock());

        let initial_cfg = BoardConfig {
            station_id: "TEST001".to_string(),
            line_ids: vec![],
            directions: vec![Direction::Northbound],
            poll_seconds: 5,
            theme: "classic-amber".to_string(),
        };
        let (tx, rx) = cfg_channel(initial_cfg.clone());
        let mut stream = Box::pin(svc.stream(rx));

        // First tick fires immediately.
        let first = stream.next().await.expect("first item");
        assert!(first.is_ok());
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "first tick must have produced exactly one fetch"
        );

        // Publish a new config with a DIFFERENT directions filter — this is
        // the "user toggled a chip" case. No abort, no restart. The stream
        // task is still the same one we started with.
        let mut updated = initial_cfg.clone();
        updated.directions = vec![Direction::Southbound];
        tx.send(updated).expect("send must succeed");

        // Advance past the poll interval (5 s, with `start_paused`) so the
        // next tick fires. The CfgChanged wake-up itself must not have
        // produced a fetch (cheap semantic).
        tokio::time::advance(Duration::from_secs(6)).await;

        let second = stream.next().await.expect("second item");
        assert!(second.is_ok());

        // Critical assertion: only the two ticks fetched. If the stream had
        // been respawned this would be 3+ (warm + tick + tick).
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "exactly 2 fetches expected; a respawn would push this above 2"
        );
    }

    // -----------------------------------------------------------------------
    // Test (item 4): config-watch — poll_seconds rebuilds the interval
    // -----------------------------------------------------------------------

    /// Changing `poll_seconds` mid-stream must rebuild the `Interval` so the
    /// next tick fires at the new period. Start at 60 s, drive one tick,
    /// publish a 10 s config, advance only 10 s, and assert the next tick
    /// fires (i.e. the original 60 s interval was discarded).
    #[tokio::test(start_paused = true)]
    async fn stream_rebuilds_interval_when_poll_seconds_changes() {
        let (mock, counter) = SeqTflHttp::new(vec![true, true, true]);
        let client = Arc::new(TflClient::new(mock));
        let svc = BoardService::new(client, make_clock());

        let initial_cfg = BoardConfig {
            station_id: "TEST001".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 60,
            theme: "classic-amber".to_string(),
        };
        let (tx, rx) = cfg_channel(initial_cfg.clone());
        let mut stream = Box::pin(svc.stream(rx));

        // First tick (immediate).
        let first = stream.next().await.expect("first item");
        assert!(first.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Drop poll_seconds to 10 s. The CfgChanged wake-up rebuilds the
        // interval; the stream is now waiting for a 10 s tick, not 60 s.
        let mut faster = initial_cfg.clone();
        faster.poll_seconds = 10;
        tx.send(faster).expect("send must succeed");

        // 10 s elapses. With the rebuilt interval the next tick fires; with
        // the original 60 s interval it would not.
        tokio::time::advance(Duration::from_secs(11)).await;

        let second = stream.next().await.expect(
            "second tick must fire ~10 s after the poll_seconds change, not wait the original 60 s",
        );
        assert!(second.is_ok());
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "tick should have fired at the new 10 s cadence"
        );
    }

    // -----------------------------------------------------------------------
    // Test (item 4): config-watch — station_id change drops last_ok
    // -----------------------------------------------------------------------

    /// When `station_id` changes, the previous station's `last_ok` board
    /// must be dropped — we don't want to re-emit OXC's arrivals under
    /// "BZP" when the next refresh fails. Drive one successful refresh,
    /// switch station_id, then fail the next refresh; assert the emitted
    /// item is an `Err` (no stale-fallback to the old station's board).
    #[tokio::test(start_paused = true)]
    async fn stream_drops_last_ok_when_station_id_changes() {
        // Call 0: success (for station A); call 1: failure (for station B).
        let (mock, _counter, id_log) = SeqTflHttp::new_with_id_log(vec![true, false]);
        let client = Arc::new(TflClient::new(mock));
        let svc = BoardService::new(client, make_clock());

        let cfg_a = BoardConfig {
            station_id: "STATION_A".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 5,
            theme: "classic-amber".to_string(),
        };
        let (tx, rx) = cfg_channel(cfg_a.clone());
        let mut stream = Box::pin(svc.stream(rx));

        // First tick: success — populates last_ok for STATION_A.
        let first = stream.next().await.expect("first item");
        let board_a = first.expect("first must be Ok");
        assert_eq!(board_a.station_id, "STATION_A");

        // Switch to STATION_B and let the next tick run (which is rigged
        // to fail). With last_ok dropped, the failure must surface as Err
        // — *not* a stale STATION_A board.
        let mut cfg_b = cfg_a.clone();
        cfg_b.station_id = "STATION_B".to_string();
        tx.send(cfg_b).expect("send must succeed");

        tokio::time::advance(Duration::from_secs(6)).await;
        let second = stream.next().await.expect("second item");
        assert!(
            second.is_err(),
            "expected Err after station_id change + fetch failure (last_ok dropped); \
             got: {second:?}"
        );

        // Sanity: the second fetch was for the new station, proving the cfg
        // change actually flowed through to refresh().
        let log = id_log.lock().expect("id_log lock");
        assert_eq!(log.len(), 2, "exactly 2 fetches");
        assert_eq!(log[0], "STATION_A");
        assert_eq!(log[1], "STATION_B");
    }

    // -----------------------------------------------------------------------
    // Test (bugfix): station_id change forces an immediate refresh
    // -----------------------------------------------------------------------

    /// When the user picks a new station, the next emitted board must be for
    /// the new station regardless of where in the poll interval we were.
    /// Sub-`poll_seconds` waits otherwise leave the user staring at the old
    /// station's stale data for up to `poll_seconds` after a deliberate
    /// action.
    ///
    /// With `poll_seconds: 30` and only 100 ms of advance after the cfg
    /// publish, the periodic tick must NOT be what wakes the next emit —
    /// the station change itself must.
    #[tokio::test(start_paused = true)]
    async fn stream_refreshes_immediately_on_station_id_change() {
        let (mock, _counter, id_log) = SeqTflHttp::new_with_id_log(vec![true, true]);
        let client = Arc::new(TflClient::new(mock));
        let svc = BoardService::new(client, make_clock());

        let cfg_a = BoardConfig {
            station_id: "STATION_A".to_string(),
            line_ids: vec![],
            directions: vec![],
            poll_seconds: 30,
            theme: "classic-amber".to_string(),
        };
        let (tx, rx) = cfg_channel(cfg_a.clone());
        let mut stream = Box::pin(svc.stream(rx));

        // First tick fires immediately and resolves to STATION_A.
        let first = stream.next().await.expect("first item");
        assert_eq!(
            first.expect("first must be Ok").station_id,
            "STATION_A",
            "first emit is for the initial station"
        );

        // Publish station B and measure how much paused-clock time elapses
        // before the next emit. With `start_paused`, tokio auto-advances
        // when every task is parked on a timer, so a buggy "wait for next
        // tick" implementation would let the clock jump nearly 30 s. The
        // fix forces an immediate refresh — the next emit must arrive in
        // well under one poll interval.
        let mut cfg_b = cfg_a.clone();
        cfg_b.station_id = "STATION_B".to_string();
        tx.send(cfg_b).expect("send must succeed");

        let before = tokio::time::Instant::now();
        let second = stream.next().await.expect("second item");
        let elapsed = before.elapsed();

        assert_eq!(
            second.expect("second must be Ok").station_id,
            "STATION_B",
            "station change must trigger an immediate refresh against the new id"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "station change should refresh within 5 s; took {elapsed:?} \
             (poll_seconds is 30, so a buggy 'wait for next tick' shows ~30 s here)"
        );

        let log = id_log.lock().expect("id_log lock");
        assert_eq!(log.as_slice(), &["STATION_A", "STATION_B"]);
    }
}
