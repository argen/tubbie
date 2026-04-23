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
    ///   If there is no last-known board, the error is emitted as
    ///   `Err(BoardError::Fetch(...))` and the stream terminates.
    /// - Dropping the stream cancels the in-flight refresh future. No tasks leak.
    pub fn stream(self, cfg: BoardConfig) -> impl Stream<Item = Result<Board, BoardError>> + Send {
        let poll_dur = Duration::from_secs(u64::from(cfg.poll_seconds).max(1));

        stream::unfold(
            // State: (service, config, interval, last_ok_board, exhausted)
            //
            // `exhausted` is set to `true` after a fatal error (no `last_ok` to
            // fall back on) is emitted. On the next poll, returning `None` terminates
            // the stream. Without this flag the closure would emit another `Err`
            // forever because `unfold` re-polls a closure that returns `Some`.
            (
                self,
                cfg,
                {
                    let mut ivl = interval(poll_dur);
                    ivl.set_missed_tick_behavior(MissedTickBehavior::Skip);
                    ivl
                },
                None::<Board>,
                false, // exhausted
            ),
            |(svc, cfg, mut ivl, last_ok, exhausted)| async move {
                // Stream was already terminated on the previous poll.
                if exhausted {
                    return None;
                }

                // Wait for the next tick (first tick fires immediately).
                ivl.tick().await;

                match svc.refresh(&cfg).await {
                    Ok(mut board) => {
                        // Success: clear stale_since, record as last_ok.
                        board.stale_since = None;
                        let emit = board.clone();
                        Some((Ok(emit), (svc, cfg, ivl, Some(board), false)))
                    }
                    Err(e) => {
                        if let Some(mut stale) = last_ok {
                            // We have a previous good board — mark it stale and re-emit.
                            // Only set stale_since if not already set (first failure).
                            if stale.stale_since.is_none() {
                                stale.stale_since = Some(svc.clock.now());
                            }
                            let emit = stale.clone();
                            Some((Ok(emit), (svc, cfg, ivl, Some(stale), false)))
                        } else {
                            // No previous good board — emit error then terminate.
                            // Setting exhausted=true causes the next poll to return None.
                            Some((Err(e), (svc, cfg, ivl, None, true)))
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
