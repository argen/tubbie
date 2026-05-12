//! `WarmFallback<T: Timer>` — race a `board://updated`-equivalent signal
//! against a deadline measured by a platform-specific timer.
//!
//! ## Why this exists
//!
//! The stop-points warm task must wait until the arrivals stream has produced
//! its first board before hitting TfL's most rate-limited endpoint
//! (`/StopPoint/Mode/tube`). If the warm fires first and triggers a 429, the
//! shared `cooldown_until` gate blocks the stream's first arrivals fetch — the
//! user stares at "Loading arrivals…" for the duration of a cooldown they never
//! caused. Using the `board://updated` event as the "first emit happened"
//! signal keeps the plumbing minimal; the fallback deadline ensures a
//! permanently broken stream doesn't starve the warm forever.
//!
//! ## Timer abstraction
//!
//! The `Timer` trait decouples the deadline measurement from the wait logic:
//!
//! - **Desktop (`TokioSleepTimer`)**: real wall-clock via `tokio::time::sleep`.
//! - **iOS (`ActiveTimeTimer`)**: active-only time that does not accumulate
//!   while the app is backgrounded. Implemented in `tubbie-ios` using the
//!   `LifecyclePhase` watch channel from this crate.
//! - **Tests (`FakeTimer`)**: records the requested duration but never
//!   actually sleeps, keeping tests deterministic.
//!
//! ## Usage
//!
//! ```rust,ignore
//! let fallback = WarmFallback::new(TokioSleepTimer, Duration::from_secs(8));
//! let (tx, rx) = tokio::sync::oneshot::channel::<()>();
//! // ... wire tx into a board://updated listener ...
//! match fallback.wait(rx).await {
//!     WarmOutcome::Event   => { /* stream fired first */ }
//!     WarmOutcome::Timeout => { /* deadline elapsed first */ }
//! }
//! ```

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Public outcome type
// ---------------------------------------------------------------------------

/// Result of [`WarmFallback::wait`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmOutcome {
    /// The event arrived before the deadline.
    Event,
    /// The deadline elapsed before the event.
    Timeout,
}

// ---------------------------------------------------------------------------
// Timer trait
// ---------------------------------------------------------------------------

/// Abstraction over the clock so `WarmFallback` can be tested without real
/// sleeps and so iOS can substitute an active-only timer that does not count
/// time spent in the background.
///
/// Each call to `elapsed` is a *fresh* future; callers may drop it at any
/// time without side effects (it is cancellation-safe by construction).
pub trait Timer: Send + Sync + 'static {
    /// Returns a future that resolves after `duration` has elapsed by this
    /// timer's clock. The future is `Send` so it can be driven from any
    /// Tokio worker thread.
    fn elapsed(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// Production timer — real wall-clock via tokio
// ---------------------------------------------------------------------------

/// Production [`Timer`] for desktop builds. Delegates directly to
/// `tokio::time::sleep`, which measures real (wall-clock) elapsed time.
pub struct TokioSleepTimer;

impl Timer for TokioSleepTimer {
    fn elapsed(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

// ---------------------------------------------------------------------------
// WarmFallback
// ---------------------------------------------------------------------------

/// Races a `board://updated`-equivalent signal against a deadline.
///
/// Generic over `T: Timer` so the deadline measurement can be swapped between
/// real wall-clock (desktop) and active-only time (iOS) without duplicating
/// the wait logic.
pub struct WarmFallback<T: Timer> {
    timer: T,
    deadline: Duration,
}

impl<T: Timer> WarmFallback<T> {
    /// Construct a new `WarmFallback` that will race events against `deadline`
    /// measured by `timer`.
    pub fn new(timer: T, deadline: Duration) -> Self {
        Self { timer, deadline }
    }

    /// Wait for whichever comes first:
    /// - `event_rx` delivers a signal (the `board://updated`-equivalent), OR
    /// - `deadline` elapses by `timer`.
    ///
    /// Returns [`WarmOutcome::Event`] if the event won, [`WarmOutcome::Timeout`]
    /// if the deadline won. A dropped or closed `event_rx` is treated the same
    /// as a timeout (fail-open: the warm proceeds rather than waiting forever).
    pub async fn wait(&self, event_rx: oneshot::Receiver<()>) -> WarmOutcome {
        tokio::select! {
            result = event_rx => {
                // Treat a dropped sender (Err) as a timeout to fail-open.
                if result.is_ok() {
                    WarmOutcome::Event
                } else {
                    WarmOutcome::Timeout
                }
            }
            _ = self.timer.elapsed(self.deadline) => {
                WarmOutcome::Timeout
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::sync::oneshot;

    use super::*;

    // -----------------------------------------------------------------------
    // FakeTimer — records the requested duration, never actually sleeps.
    // -----------------------------------------------------------------------

    /// Test double for [`Timer`]. Records the duration passed to `elapsed` and
    /// blocks until the `release` handle is signalled (or dropped). The caller
    /// controls exactly when the "timer fires" by dropping or using the release
    /// handle, keeping tests fully deterministic without `tokio::time::pause`.
    struct FakeTimer {
        /// The duration passed to the most recent `elapsed` call.
        recorded: Arc<Mutex<Option<Duration>>>,
        /// When this receives `()` (or is dropped), `elapsed` resolves.
        release_rx: Arc<tokio::sync::Mutex<oneshot::Receiver<()>>>,
    }

    impl FakeTimer {
        /// Create a new `FakeTimer` and a release handle. Call `release()` on
        /// the handle (or drop it) to make the next `elapsed` future resolve.
        fn new() -> (Self, FakeTimerRelease) {
            let (tx, rx) = oneshot::channel::<()>();
            let recorded = Arc::new(Mutex::new(None));
            let timer = FakeTimer {
                recorded: Arc::clone(&recorded),
                release_rx: Arc::new(tokio::sync::Mutex::new(rx)),
            };
            let release = FakeTimerRelease {
                tx: Some(tx),
                recorded,
            };
            (timer, release)
        }
    }

    impl Timer for FakeTimer {
        fn elapsed(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            // Record the requested duration so the test can assert it.
            if let Ok(mut guard) = self.recorded.lock() {
                *guard = Some(duration);
            }
            let rx = Arc::clone(&self.release_rx);
            Box::pin(async move {
                // Wait until the release handle fires (or is dropped).
                let mut rx_guard = rx.lock().await;
                let _ = (&mut *rx_guard).await;
            })
        }
    }

    /// Handle that controls when the `FakeTimer`'s `elapsed` future resolves.
    struct FakeTimerRelease {
        tx: Option<oneshot::Sender<()>>,
        recorded: Arc<Mutex<Option<Duration>>>,
    }

    impl FakeTimerRelease {
        /// Resolve the timer's pending `elapsed` future.
        fn release(mut self) {
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(());
            }
        }

        /// Read back the duration that was passed to `elapsed`.
        fn recorded_duration(&self) -> Option<Duration> {
            self.recorded.lock().ok().and_then(|g| *g)
        }
    }

    // Drop also releases, matching "timer expired naturally" for the timeout case.
    impl Drop for FakeTimerRelease {
        fn drop(&mut self) {
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Test cases
    // -----------------------------------------------------------------------

    /// Case 1: Event arrives before the deadline.
    ///
    /// The outcome must be `WarmOutcome::Event`. The timer's `elapsed` future
    /// was started with the full deadline duration — the FakeTimer records it —
    /// but was cancelled by `tokio::select!` when the event won, so the
    /// release handle is never consumed (the drop in `FakeTimerRelease::drop`
    /// fires after the test assertion, not before).
    #[tokio::test]
    async fn event_arrives_before_deadline() {
        let deadline = Duration::from_secs(8);
        let (timer, release) = FakeTimer::new();
        let fallback = WarmFallback::new(timer, deadline);

        let (event_tx, event_rx) = oneshot::channel::<()>();

        // Resolve the event immediately — before the timer.
        let _ = event_tx.send(());

        let outcome = fallback.wait(event_rx).await;

        // The timer was started (elapsed called with the deadline)…
        assert_eq!(
            release.recorded_duration(),
            Some(deadline),
            "timer should have been started with the full deadline"
        );
        // …but the event won.
        assert_eq!(outcome, WarmOutcome::Event);
        // release drops here, which is fine — the select already resolved.
    }

    /// Case 2: Deadline elapses before the event.
    ///
    /// The outcome must be `WarmOutcome::Timeout`. We release the timer
    /// explicitly before sending the event (or not sending it at all — the
    /// event_rx is dropped unused).
    #[tokio::test]
    async fn deadline_elapses_before_event() {
        let deadline = Duration::from_secs(8);
        let (timer, release) = FakeTimer::new();
        let fallback = WarmFallback::new(timer, deadline);

        let (_event_tx, event_rx) = oneshot::channel::<()>();
        // Keep event_tx alive but don't send — the timer should win.

        // Spawn the wait future.
        let wait_fut = fallback.wait(event_rx);

        // Release the timer first (simulates deadline elapsing).
        release.release();

        let outcome = wait_fut.await;

        assert_eq!(outcome, WarmOutcome::Timeout);
    }

    /// Dropped sender → fail-open as Timeout, not Event.
    ///
    /// When the sender half of the event channel is dropped without ever
    /// sending, `event_rx` resolves to `Err` immediately. `WarmFallback`
    /// must treat that as fail-open (`WarmOutcome::Timeout`) rather than
    /// hanging forever or panicking. The timer is configured to NOT fire
    /// (its release handle is kept alive), so the only resolution path is
    /// the closed-channel branch.
    #[tokio::test]
    async fn receiver_dropped_falls_open_to_timeout() {
        let deadline = Duration::from_secs(8);
        let (timer, _release) = FakeTimer::new();
        // _release is intentionally kept alive — the timer will NOT fire.
        // The only path to resolution is the dropped-sender branch.
        let fallback = WarmFallback::new(timer, deadline);

        let (tx, rx) = oneshot::channel::<()>();
        // Drop tx immediately without sending — closes the channel.
        drop(tx);

        let outcome = fallback.wait(rx).await;

        assert_eq!(
            outcome,
            WarmOutcome::Timeout,
            "a dropped sender must fail-open as Timeout, not block or return Event"
        );
    }
}
