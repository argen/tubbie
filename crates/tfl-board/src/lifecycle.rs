//! Lifecycle phase signal for the polling stream.
//!
//! Desktop builds never need this — `LifecyclePhase::always_active()`
//! returns a never-changing `Active` signal so the lifecycle parameter
//! is free for existing call sites. iOS uses a real signal driven from
//! the Tauri mobile run-event handler (see tubbie-ios/CLAUDE.md
//! invariant 8).
//!
//! ## iOS mapping
//!
//! - `AppPhase::Active` ≈ `UIApplicationState.active` — the app is in the
//!   foreground and receiving events. Polling should run normally.
//! - `AppPhase::Background` ≈ `UIApplicationState.background` or
//!   `UIApplicationState.inactive` — the app is not visible or is
//!   transitioning. Polling is paused to avoid burning battery and
//!   accumulating TfL rate-limit cooldowns the user never sees.
//!
//! ## Lifetime contract
//!
//! `LifecyclePhase` owns the `watch::Sender`. When the struct is dropped
//! all receivers see a sender-dropped notification. The struct should be
//! placed in application state (e.g. `AppState`) so its lifetime extends
//! for the duration of the app. In practice the sender lives for the app
//! lifetime, so receivers never observe the sender-dropped condition during
//! normal operation.

use tokio::sync::watch;

/// Application lifecycle phase relevant to the polling stream.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AppPhase {
    /// The app is in the foreground — polling runs normally.
    Active,
    /// The app is backgrounded or suspended — polling is paused.
    Background,
}

/// A `tokio::sync::watch`-backed signal that carries the current `AppPhase`.
///
/// The struct holds the `Sender` so its lifetime controls all downstream
/// receivers. Drop the struct and every receiver learns the sender vanished.
pub struct LifecyclePhase {
    tx: watch::Sender<AppPhase>,
}

impl LifecyclePhase {
    /// Create a new `LifecyclePhase` starting at `initial`.
    pub fn new(initial: AppPhase) -> Self {
        let (tx, _rx) = watch::channel(initial);
        Self { tx }
    }

    /// Return a signal that is always `Active`.
    ///
    /// Desktop builds use this so the lifecycle parameter on
    /// `BoardService::stream` is free without requiring callers to construct
    /// a real signal source.
    pub fn always_active() -> Self {
        Self::new(AppPhase::Active)
    }

    /// Subscribe a new receiver. Each receiver independently tracks the
    /// `changed()` mark so multiple streams can subscribe independently.
    pub fn subscribe(&self) -> watch::Receiver<AppPhase> {
        self.tx.subscribe()
    }

    /// Update the phase. A notification is sent only when the new value
    /// differs from the current value (`send_if_modified`).
    pub fn set(&self, phase: AppPhase) {
        self.tx.send_if_modified(|cur| {
            if *cur == phase {
                false
            } else {
                *cur = phase;
                true
            }
        });
    }

    /// Read the current phase without subscribing.
    pub fn current(&self) -> AppPhase {
        *self.tx.borrow()
    }
}
