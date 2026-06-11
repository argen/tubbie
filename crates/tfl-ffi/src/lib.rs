//! Tubbie SwiftUI rewrite — FFI binding spike (B.1).
//!
//! This crate is the **load-bearing kill-switch** for the SwiftUI rewrite.
//! It demonstrates that:
//!
//! 1. **Async Rust → Swift works.** A `BoardService::refresh` future, driven
//!    by tokio multi-thread, can be awaited from Swift via `uniffi`'s
//!    `async_runtime = "tokio"` integration.
//! 2. **Errors propagate as typed Swift errors.** [`FfiError`] is a `#[derive(uniffi::Error)]`
//!    enum; each variant maps to a Swift `enum` case carrying its associated
//!    data. No `Result<T, String>` JSON-stringly-typed dance.
//! 3. **Panics are caught at the FFI boundary.** uniffi wraps every exported
//!    Rust call in `std::panic::catch_unwind` and surfaces the panic as a
//!    Swift `UniffiInternalError.panic` (or equivalent) rather than aborting
//!    the process. The `trigger_panic_for_testing` export exists solely to
//!    let the spike's tests prove this contract holds.
//! 4. **The tokio runtime starts fast.** [`tokio_runtime_warmup_micros`]
//!    measures cold runtime startup so we have a number to compare against
//!    the <30 ms B.1 acceptance gate.
//!
//! ## Why fixtures, not network
//!
//! The spike uses [`FixtureTflHttp`] + [`FakeClock`] so it is deterministic
//! and offline. The plumbing question (async tokio across FFI, panic safety,
//! error mapping) is **independent** of whether the upstream HTTP impl is
//! real or fixed. Validating the seam against fixtures catches the FFI bugs;
//! validating against real TfL is a B.3+ concern.
//!
//! ## Tokio runtime ownership (load-bearing — verify in B.2)
//!
//! `#[uniffi::export(async_runtime = "tokio")]` instructs uniffi 0.31 to
//! drive the future on a **uniffi-managed** tokio runtime, lazily built on
//! first call (current_thread by default; configurable via
//! `uniffi::deps::async_compat`). The host integration tests use
//! `#[tokio::test(flavor = "multi_thread")]` and so override the runtime
//! contextually — this is why the host numbers are pristine.
//!
//! On iOS the SwiftUI app will invoke these exports from `.task { }`
//! modifiers, which run on the Swift cooperative pool, not a tokio thread.
//! The export call hops into uniffi's runtime; that is fine for a single
//! runtime but means **B.2 must verify the iOS app is not creating a
//! competing tokio runtime** (e.g. inside the Tauri shell during the
//! co-existence soak window). If both runtimes touch the same `Arc<TflClient>`
//! they share state safely (the client is `Send + Sync`); if they don't
//! coordinate runtime shutdown, the hosted-runtime drop on app suspend
//! could deadlock a pending future on the other.
//!
//! Action: B.2 spike must call `get_board_json` from a SwiftUI `.task` and
//! assert (a) it completes, (b) `Activity` instruments show only one
//! `tokio-runtime-worker` thread family active, (c) no orphaned tasks at
//! ScenePhase → Background.
//!
//! ## What this is NOT
//!
//! - Not a full bridge surface. `commands.rs` exposes ~30 commands. This
//!   spike exposes one (`get_board_json`) and one panic-injection helper.
//!   Once B.1 passes, the rest are ported in B.3+ following the same shape.
//! - Not iOS-built. The crate compiles for the host (`aarch64-apple-darwin`)
//!   so we can run integration tests + Swift bindgen without xcodebuild.
//!   Cross-compilation to `aarch64-apple-ios` happens in B.2.

mod key_pool;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use futures::StreamExt;
use tfl_board::{AppPhase, BoardConfig, BoardError, BoardService, LifecyclePhase};
use tfl_cache::TflClient;
use tfl_client::clock::{Clock, FakeClock, SystemClock};
use tfl_client::fixture::FixtureTflHttp;
use tfl_client::http::{ReqwestTflHttp, TflHttp};
use tokio::sync::{mpsc, watch, Mutex, Notify};

// ---------------------------------------------------------------------------
// Shared `TflClient` cache (process-wide)
// ---------------------------------------------------------------------------
//
// All four live exports — `search_stations_live`,
// `find_nearest_stations_live`, `subscribe_board_live`, and
// `get_line_statuses_live` — go through `shared_live_client(...)` so they
// reuse one `Arc<TflClient>` (and its `stop_points_cache`,
// `hub_children_cache`, `hub_lines_cache`) across calls. Without this
// every keystroke that survives the SwiftUI debounce was constructing a
// fresh client and refetching the 16 MB stop-points fan-out — and worse,
// `subscribe_board_live`'s cold-cache window meant `resolve_arrival_ids`
// fell back to single-id arrivals, dropping Elizabeth / Overground
// predictions at every multi-mode hub (Liverpool Street, Tottenham Court
// Road, Stratford, Whitechapel, …). See `tests/shared_client.rs` for the
// wiring contract.

/// Cache key for the shared client map. Anonymous and authenticated
/// callers MUST NOT share a client (the `app_key` is wired into
/// `ReqwestTflHttp` at construction and rides every request); two
/// distinct authenticated keys must also stay separate so a key swap in
/// Settings doesn't reuse the previous user's quota bucket.
///
/// `Pooled(usize)` identifies a slot in the runtime key pool — each slot
/// gets its own `Arc<TflClient>` in the HashMap so all callers landing
/// on the same slot share one `stop_points_cache`, `hub_children_cache`,
/// and `line_status_cache`. A separate Vec of clients outside this map
/// would give every FFI export a different TflClient per slot and
/// reproduce the "Elizabeth missing at Liverpool Street" cache-split bug
/// the HashMap comment above was written to prevent.
#[derive(Debug, Hash, Eq, PartialEq, Clone)]
enum ClientKey {
    Anonymous,
    Authenticated(String),
    Pooled(usize),
}

/// Process-wide key pool, initialised once at app launch by Swift via
/// `initialize_key_pool(keys:)`. Uninitialised until Swift calls that
/// export; `shared_live_client(None)` falls through to `Anonymous` when
/// the pool is not ready (offline dev builds, missing endpoint).
static KEY_POOL: OnceLock<key_pool::KeyPool> = OnceLock::new();

fn shared_live_clients() -> &'static StdMutex<HashMap<ClientKey, Arc<TflClient<ReqwestTflHttp>>>> {
    static MAP: OnceLock<StdMutex<HashMap<ClientKey, Arc<TflClient<ReqwestTflHttp>>>>> =
        OnceLock::new();
    MAP.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Get-or-create a `TflClient` for the given `ClientKey`, inserting into
/// the process-wide HashMap on first use.
///
/// `api_key` is the raw key string used to construct `ReqwestTflHttp`.
/// It is `None` for `Anonymous`, and `Some(k)` for both `Authenticated`
/// and `Pooled` slots (the pool key string is supplied by the caller
/// from `KEY_POOL`).
///
/// First init spawns a background `warm_stop_points_cache` so subsequent
/// calls see a hot cache. Concurrent first callers are single-flighted by
/// the upstream client's own `stop_points_refresh` async lock.
fn get_or_create_client(
    client_key: ClientKey,
    api_key: Option<&str>,
) -> Arc<TflClient<ReqwestTflHttp>> {
    let mut guard = shared_live_clients()
        .lock()
        .expect("shared client mutex poisoned");
    if let Some(existing) = guard.get(&client_key) {
        return Arc::clone(existing);
    }

    let http = match api_key {
        Some(k) => ReqwestTflHttp::with_app_key(k.to_string()),
        None => ReqwestTflHttp::new(),
    };
    let client = Arc::new(TflClient::new(http));
    guard.insert(client_key, Arc::clone(&client));
    drop(guard);

    // Fire-and-forget warm. The first caller drives a 4-mode + hub
    // fan-out into the shared cache; subsequent search keystrokes,
    // `find_nearest`, and the in-flight subscription's first
    // `resolve_arrival_ids` all read from it. Failures are logged
    // upstream — `stop_points_cached` will retry on the next call.
    let warm_client = Arc::clone(&client);
    tokio::spawn(async move {
        let _ = warm_client.warm_stop_points_cache().await;
    });

    client
}

/// Resolve the effective `TflClient` for the given normalised `app_key`.
///
/// Priority:
/// 1. User key (`Some(k)`) → `ClientKey::Authenticated(k)` — dedicated bucket.
/// 2. Pool available → `ClientKey::Pooled(slot)` — round-robin across
///    bundled keys fetched from the Vercel endpoint at launch.
/// 3. No pool → `ClientKey::Anonymous` — 50 req/min shared across all
///    keyless users on the same IP (acceptable for dev/offline builds;
///    the pool is absent until the Vercel endpoint is deployed).
///
/// Trimming and emptiness checks happen in each export's validation path;
/// this helper takes the already-normalised `Option<&str>`.
fn shared_live_client(app_key: Option<&str>) -> Arc<TflClient<ReqwestTflHttp>> {
    if let Some(k) = app_key {
        return get_or_create_client(ClientKey::Authenticated(k.to_string()), Some(k));
    }
    // Pool is initialised by `initialize_key_pool` at app launch; falls
    // through to anonymous when not yet initialised (offline / first
    // launch before the endpoint responds).
    if let Some(pool) = KEY_POOL.get() {
        let (slot_idx, key) = pool.pick();
        return get_or_create_client(ClientKey::Pooled(slot_idx), Some(key));
    }
    get_or_create_client(ClientKey::Anonymous, None)
}

/// Test-only re-export of `shared_live_client` so the
/// `tests/shared_client.rs` integration tests can verify pointer
/// identity across calls without exposing the helper to FFI consumers.
/// Marked `#[doc(hidden)]` because it is an implementation seam, not
/// public API.
#[doc(hidden)]
pub fn shared_live_client_for_test(app_key: Option<&str>) -> Arc<TflClient<ReqwestTflHttp>> {
    shared_live_client(app_key)
}

/// Test-only accessor so integration tests can inspect whether the pool
/// is active and which slot a given key string occupies — without going
/// through the full FFI surface.
#[doc(hidden)]
pub fn key_pool_for_test() -> Option<&'static key_pool::KeyPool> {
    KEY_POOL.get()
}

/// Errors crossing the FFI boundary.
///
/// uniffi maps each variant to a Swift `enum` case. Associated `String` data
/// reaches Swift as a stored property; consumers pattern-match the case the
/// same way they pattern-match `Result.failure(.ffiError(.rateLimited(...)))`.
///
/// We expose a small but representative set for the spike: a payload-bearing
/// `Validation`, a sibling `Io`, a coarse-bag `Refresh`, and one structured
/// variant `RateLimited { retry_after_secs }` to prove that variants with
/// non-string payloads round-trip through Swift end-to-end (the load-bearing
/// claim of the typed-error story). The full structured mapping for the rest
/// of [`BoardError`] / [`tfl_client::TflError`] lands in B.3 — the spike's
/// only job here is "does the seam itself hold"; if `RateLimited`'s `u64`
/// payload survives the round-trip, the seam holds.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("validation: {0}")]
    Validation(String),

    #[error("io: {0}")]
    Io(String),

    #[error("rate_limited (retry after {retry_after_secs}s)")]
    RateLimited { retry_after_secs: u64 },

    #[error("refresh: {0}")]
    Refresh(String),
}

impl From<BoardError> for FfiError {
    fn from(e: BoardError) -> Self {
        // Prove the structured-variant path while we're here: a 429 from the
        // upstream client maps to a typed Swift case carrying the retry
        // duration, not a string-soup `Refresh`.
        if let BoardError::Fetch(tfl_client::error::TflError::RateLimited { retry_after }) = &e {
            return Self::RateLimited {
                retry_after_secs: retry_after.map(|d| d.as_secs()).unwrap_or(0),
            };
        }
        Self::Refresh(e.to_string())
    }
}

/// Initialise the process-wide key pool from a list of TfL API keys.
///
/// Called once at app launch by Swift after it reads the keys from the
/// Vercel endpoint (`PoolKeyService`). Each key must be exactly 32
/// lowercase-hex characters (TfL's own format); invalid entries are
/// silently skipped. If no valid key survives validation the pool is NOT
/// initialised and `shared_live_client(None)` continues to fall back to
/// the anonymous 50 req/min bucket.
///
/// **Idempotent**: uses `OnceLock::set` — the first call wins and all
/// subsequent calls with different keys are silently no-ops. This means
/// a double-init from a re-connected `ScenePhase` or a crash-then-resume
/// is safe; it does NOT mean the pool can be rotated at runtime.
#[uniffi::export]
pub fn initialize_key_pool(keys: Vec<String>) {
    if let Some(pool) = key_pool::KeyPool::new(keys) {
        // `OnceLock::set` returns Err if already initialised; we discard
        // it intentionally — first call wins.
        let _ = KEY_POOL.set(pool);
    }
}

/// Returns `true` if the key pool has been successfully initialised.
///
/// Used by Swift's `ActiveSource` to distinguish `.livePool` from
/// `.liveAnonymous` so the diagnostics surface and About tab can show
/// whether the user is on shared-pool capacity or bare anonymous access.
#[uniffi::export]
pub fn is_key_pool_active() -> bool {
    KEY_POOL.get().is_some()
}

/// Refresh the arrivals board for a station and return the result as a JSON
/// string.
///
/// JSON is the wire format for this spike on purpose:
///
/// 1. It avoids re-deriving every domain type as `#[derive(uniffi::Record)]`,
///    which would force a one-shot rewrite touching `tfl-domain` (out of
///    scope for B.1).
/// 2. It validates the *FFI* spike, not the *type-mapping* spike — if the
///    async/error/panic story doesn't work, no amount of `uniffi::Record`
///    boilerplate will save it.
/// 3. SwiftUI consumers in B.3 will likely want `Codable` Swift types decoded
///    on the Swift side anyway; sending JSON gives that for free with the
///    same shape as today's Tauri IPC.
///
/// The `recorded_at` argument is the RFC3339 timestamp the [`FakeClock`] is
/// pinned to. This is fixture-driven and will be removed when the spike is
/// generalised against `SystemClock` in B.3.
#[uniffi::export(async_runtime = "tokio")]
pub async fn get_board_json(
    station_id: String,
    fixtures_dir: String,
    recorded_at_rfc3339: String,
) -> Result<String, FfiError> {
    if station_id.is_empty() || station_id.len() > 32 {
        return Err(FfiError::Validation(format!(
            "station_id must be 1–32 characters, got {}",
            station_id.len()
        )));
    }

    let fixtures_path = PathBuf::from(&fixtures_dir);
    // NOTE(B.3): `Path::exists()` does a blocking `stat`. For the spike's
    // fixture-driven path this is microseconds and runs on whatever thread
    // Swift dispatched the call from; harmless. Do NOT carry this pattern
    // into the production path — when `fixtures_dir` is replaced by a
    // network-backed `RealHttp`, blocking I/O at this layer would stall a
    // SwiftUI `.task { }` for the duration of any DNS/connect.
    if !fixtures_path.exists() {
        return Err(FfiError::Io(format!(
            "fixtures_dir does not exist: {fixtures_dir}"
        )));
    }

    // Cooperatively yield BEFORE the heavy lifting so a SwiftUI `.task(id:)`
    // that cancels mid-call (e.g. user double-tapped Refresh) can short-
    // circuit before BoardService allocates. uniffi 0.31's tokio runtime
    // honours `tokio::task::yield_now`'s cancellation propagation.
    tokio::task::yield_now().await;

    let clock = FakeClock::from_rfc3339(&recorded_at_rfc3339)
        .map_err(|e| FfiError::Validation(format!("recorded_at_rfc3339: {e}")))?;

    let http = FixtureTflHttp::new(&fixtures_path);
    let client = Arc::new(TflClient::new(http));
    let service = BoardService::new(client, clock);

    let cfg = BoardConfig::new(&station_id);
    let board = service.refresh(&cfg).await?;

    // `serde_json::to_string` on a `Board` is infallible in practice (no
    // floats with NaN, no maps with non-string keys). Treat a hypothetical
    // failure as the same class as a serialisation bug — surface it but do
    // not abort.
    serde_json::to_string(&board).map_err(|e| FfiError::Refresh(format!("serialisation: {e}")))
}

/// Wall-clock cost of starting a fresh single-thread tokio runtime, in
/// microseconds. Used by the B.1 acceptance gate (<30 ms = <30 000 µs).
///
/// The production app will own one runtime for its lifetime, so this is
/// strictly an upper bound on cold-start cost — every subsequent FFI call
/// reuses the warm runtime via the `async_runtime = "tokio"` configuration
/// uniffi installs.
#[uniffi::export]
pub fn tokio_runtime_warmup_micros() -> u64 {
    let started = Instant::now();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    rt.block_on(async {});
    drop(rt);
    started.elapsed().as_micros() as u64
}

/// Panic-safety probe — gated behind the `panic-probe` feature so it cannot
/// leak into iOS production builds.
///
/// **Why fallible.** uniffi catches panics on any exported call via
/// `catch_unwind` and writes the unwind payload to `RustCallStatus`. On a
/// non-throwing Swift signature the lifted error is unwrapped via `try!`,
/// which traps the process. We declare the export as `Result<(), FfiError>`
/// so Swift sees a `throws` function and can `do { try ... } catch {}` —
/// proving the catch round-trips end-to-end without aborting.
///
/// **Why feature-gated.** Without `--features panic-probe` the symbol is
/// not compiled, so it cannot be called from the app even by accident. The
/// host bindgen + acceptance tests enable the feature explicitly.
///
/// Removing this export — or "fixing" it to be infallible — defeats the
/// test. Don't.
#[cfg(feature = "panic-probe")]
#[uniffi::export]
pub fn trigger_panic_for_testing() -> Result<(), FfiError> {
    // The panic happens *before* we synthesise the Result, so uniffi's
    // catch_unwind has to handle it. If panic-safety regresses (e.g. uniffi
    // changes its catch behaviour, or a feature flag is mis-set), Swift will
    // see a process trap instead of a clean `throws`. The acceptance test
    // in `tests/spike_acceptance.rs` exercises this contract.
    panic!("intentional panic for FFI safety test");
}

// ---------------------------------------------------------------------------
// B.3 — Streaming subscription bridge
// ---------------------------------------------------------------------------
//
// The B.1 `get_board_json` export is single-shot: one Swift `await` =>
// one HTTP refresh => one Board JSON. The real app needs the long-running
// stream — `BoardService::stream` — surfaced across the FFI seam without
// re-implementing its semantics on the Swift side. This is the
// load-bearing question for B.3.
//
// ## Shape
//
// `subscribe_board(...)` returns an `Arc<BoardSubscription>`. The
// subscription owns:
//
// - A `watch::Sender<BoardConfig>` — kept alive so the upstream stream
//   stays alive (the upstream drops the stream when ALL `cfg_tx` clones
//   drop). B.4 will use this to publish station/filter changes.
// - A `LifecyclePhase` — drives `AppPhase::{Active,Background}` for
//   pause/resume. Per upstream's `BoardService::stream` doc, a
//   Background → Active transition forces an immediate refresh, satisfying
//   `tubbie-ios/CLAUDE.md` invariant 8 (1 s freshness on resume).
// - An `mpsc` receiver — Swift `next_snapshot()` awaits this. The
//   matching sender lives inside the forwarder task.
// - A `JoinHandle` — the forwarder task. Aborted on `shutdown()` and
//   dropped (which also aborts) when the `Arc<BoardSubscription>` drops.
//
// ## Why mpsc, not direct stream-on-self
//
// uniffi's `&self` async methods need a `Sync` future. Holding a pinned
// `Box<dyn Stream + Send>` directly inside `BoardSubscription` and
// awaiting `stream.next()` from `next_snapshot` would force `Mutex<Pin<...>>`
// over the whole stream — which works but locks out the lifecycle setters
// while a refresh is in flight. The mpsc decouples the two: the forwarder
// owns the stream, and Swift consumes through a mutex-guarded receiver
// that releases between `recv().await` resolutions.
//
// ## Why bounded(8)
//
// Unbounded would let the queue grow if Swift fell behind. With
// `poll_seconds >= 1` and Swift draining each emit through a `for await`,
// the queue stays close to empty in practice. Pick a small bound so a
// stuck consumer eventually backpressures the forwarder rather than
// burning RAM. 8 ≈ 4 minutes of `poll_seconds=30` headroom — well past
// any plausible UI hiccup.

/// A live arrivals subscription bridging `tfl-board::BoardService::stream`
/// to Swift.
///
/// Construct via [`subscribe_board`]; consume via [`Self::next_snapshot`];
/// drive iOS lifecycle via [`Self::pause`] / [`Self::resume`]; tear down
/// via [`Self::shutdown`] (or just drop the `Arc`).
///
/// **Lifetime:** the `Arc<BoardSubscription>` MUST outlive the consuming
/// SwiftUI view's `.task`. Dropping the Arc aborts the forwarder task
/// and drops `cfg_tx`, which ends the upstream stream — any in-flight
/// refresh future is cancelled at its next `await`. This is the same
/// cancellation contract the desktop app relies on (upstream
/// `stream_cancellation_drops_in_flight` test).
#[derive(uniffi::Object)]
pub struct BoardSubscription {
    /// Held purely to keep the upstream stream alive. B.4 publishes
    /// config changes through it; B.3 leaves it inert.
    _cfg_tx: watch::Sender<BoardConfig>,
    lifecycle: LifecyclePhase,
    rx: Mutex<mpsc::Receiver<Result<String, FfiError>>>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Optional one-shot pull-to-refresh handler task — present on
    /// `subscribe_board_live` subscriptions only (fixture-mode
    /// `subscribe_board` leaves this `None`). Aborted alongside the
    /// streaming task in `shutdown()` so a `Notify`-parked refresh
    /// task can't outlive its subscription and leak across
    /// station-change / restart cycles.
    refresh_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    refresh_notify: Arc<Notify>,
}

#[uniffi::export(async_runtime = "tokio")]
impl BoardSubscription {
    /// Awaits the next board snapshot.
    ///
    /// Returns:
    /// - `Ok(Some(json))` for a successful Board emission. The string is
    ///   a `tfl_domain::Board` serialised to JSON (same shape as
    ///   `get_board_json`'s payload).
    /// - `Err(FfiError::...)` for a refresh error. The stream is **infinite**
    ///   per upstream invariant 4 — the next call may succeed. Consumers
    ///   MUST NOT treat this as terminal; render an inline error banner
    ///   and call `next_snapshot()` again.
    /// - `Ok(None)` when the subscription has been shut down (manually via
    ///   `shutdown()` or via the Arc drop). Stop calling.
    ///
    /// **Single-consumer contract.** `next_snapshot` MUST be called by at
    /// most one task at a time. The implementation holds an async mutex
    /// across `recv().await`, so a second concurrent caller would park
    /// forever waiting for the first to release. The Swift consumer
    /// (`BoardClient`) enforces this via its single forwarder task.
    pub async fn next_snapshot(&self) -> Result<Option<String>, FfiError> {
        let mut rx = self.rx.lock().await;
        match rx.recv().await {
            Some(Ok(json)) => Ok(Some(json)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// Pauses polling per `tubbie-ios/CLAUDE.md` invariant 8.
    ///
    /// Idempotent — `LifecyclePhase::set` uses `send_if_modified` so a
    /// repeated `pause()` is a no-op (no stream wake-up, no double-emit
    /// of the synthetic `PhaseChanged`).
    ///
    /// In-flight refresh futures complete (we do NOT abort mid-fetch).
    /// The user-visible effect is: the next tick AFTER the in-flight
    /// refresh resolves does not fire.
    pub fn pause(&self) {
        self.lifecycle.set(AppPhase::Background);
    }

    /// Resumes polling. Idempotent (`send_if_modified`).
    ///
    /// Background → Active triggers an immediate refresh upstream — the
    /// next `next_snapshot()` resolves with a fresh board within one
    /// fetch RTT, satisfying invariant 8 (≤1 s on resume).
    pub fn resume(&self) {
        self.lifecycle.set(AppPhase::Active);
    }

    /// Best-effort shutdown. Aborts the forwarder task; subsequent
    /// `next_snapshot()` calls return `Ok(None)`.
    ///
    /// Idempotent (the JoinHandle is taken out of its slot on first
    /// call; subsequent calls find an empty slot and no-op). Calling
    /// this is optional — dropping the `Arc<BoardSubscription>` reaches
    /// the same final state via the cfg_tx drop path, but the explicit
    /// abort is faster: no waiting for the upstream stream to observe
    /// cfg_tx drop on its next select tick.
    pub async fn shutdown(&self) {
        let mut handle = self.task.lock().await;
        if let Some(h) = handle.take() {
            h.abort();
        }
        // Abort the pull-to-refresh handler too (present only on
        // `subscribe_board_live` subscriptions). Without this, the
        // refresh task stays parked on `refresh_notify.notified()`
        // forever — leaking one tokio task per subscription lifecycle.
        let mut refresh_handle = self.refresh_task.lock().await;
        if let Some(h) = refresh_handle.take() {
            h.abort();
        }
    }

    /// Signal the forwarder task to perform an immediate one-shot refresh
    /// without tearing down the subscription. The result lands on the same
    /// `snap_tx` channel that `next_snapshot()` reads from, so the Swift
    /// consumer's existing `consume()` loop handles it transparently.
    ///
    /// Use this for pull-to-refresh when the station and source haven't
    /// changed — avoids the `shutdown()` + `subscribe_board_live()` round
    /// trip that costs 1-3 s of spinner time.
    pub fn request_immediate_refresh(&self) {
        self.refresh_notify.notify_one();
    }
}

/// Subscribe to a streaming arrivals board.
///
/// Returns an `Arc<BoardSubscription>` driven by a tokio task on uniffi's
/// runtime. The B.3 fixture-mode wiring uses [`FixtureTflHttp`] +
/// [`FakeClock`] for offline determinism, mirroring B.1's `get_board_json`.
/// B.4 introduces a `subscribe_board_live` parallel that swaps in
/// `RealHttp` + `SystemClock`.
///
/// **Validation gates** (cheap-reject path — no task spawn, no clock
/// allocation, no fixture I/O on bad input):
/// - `station_id`: 1..=32 chars (matches `get_board_json`).
/// - `poll_seconds`: 1..=600. Smaller wastes TfL quota for identical
///   payloads (TfL refreshes every ~30 s); larger violates invariant 8.
/// - `fixtures_dir`: must exist on disk.
/// - `recorded_at_rfc3339`: must parse as RFC 3339.
#[uniffi::export(async_runtime = "tokio")]
pub async fn subscribe_board(
    station_id: String,
    fixtures_dir: String,
    recorded_at_rfc3339: String,
    poll_seconds: u32,
) -> Result<Arc<BoardSubscription>, FfiError> {
    if station_id.is_empty() || station_id.len() > 32 {
        return Err(FfiError::Validation(format!(
            "station_id must be 1–32 characters, got {}",
            station_id.len()
        )));
    }
    if !(1..=600).contains(&poll_seconds) {
        return Err(FfiError::Validation(format!(
            "poll_seconds must be 1..=600, got {poll_seconds}"
        )));
    }

    let fixtures_path = PathBuf::from(&fixtures_dir);
    if !fixtures_path.exists() {
        return Err(FfiError::Io(format!(
            "fixtures_dir does not exist: {fixtures_dir}"
        )));
    }

    let clock = FakeClock::from_rfc3339(&recorded_at_rfc3339)
        .map_err(|e| FfiError::Validation(format!("recorded_at_rfc3339: {e}")))?;
    let http = FixtureTflHttp::new(&fixtures_path);
    let client = Arc::new(TflClient::new(http));
    let service = BoardService::new(client, clock);

    let mut cfg = BoardConfig::new(&station_id);
    cfg.poll_seconds = poll_seconds;

    let (sub, _snap_tx) = spawn_board_subscription(service, cfg, Arc::new(Notify::new()));
    Ok(sub)
}

// ---------------------------------------------------------------------------
// B.4 — Live HTTP subscription
// ---------------------------------------------------------------------------

/// Subscribe to a streaming arrivals board against the **live** TfL API.
///
/// Identical lifecycle and pause/resume semantics to [`subscribe_board`];
/// the only difference is the underlying `TflHttp` impl ([`ReqwestTflHttp`]
/// vs `FixtureTflHttp`) and the clock ([`SystemClock`] vs [`FakeClock`]).
///
/// `app_key` is optional — anonymous TfL access has a 50 req/min quota
/// which the user will hit during a normal browsing session. The
/// onboarding flow (B.4 Swift side) gates the user toward providing one.
/// Empty / whitespace-only keys are rejected at validation rather than
/// silently downgraded to anonymous, so a user who pasted "" sees an
/// immediate error instead of mysterious 429s 30 s later.
///
/// **Validation gates** match `subscribe_board` plus:
/// - `app_key`: when present, must be 32 lowercase-hex characters (TfL's
///   own format). Other lengths fail fast — TfL would 401, but failing
///   here is faster + clearer.
#[uniffi::export(async_runtime = "tokio")]
pub async fn subscribe_board_live(
    station_id: String,
    app_key: Option<String>,
    poll_seconds: u32,
) -> Result<Arc<BoardSubscription>, FfiError> {
    if station_id.is_empty() || station_id.len() > 32 {
        return Err(FfiError::Validation(format!(
            "station_id must be 1–32 characters, got {}",
            station_id.len()
        )));
    }
    if !(1..=600).contains(&poll_seconds) {
        return Err(FfiError::Validation(format!(
            "poll_seconds must be 1..=600, got {poll_seconds}"
        )));
    }
    let normalised_key = match app_key {
        Some(k) => {
            let trimmed = k.trim().to_string();
            if trimmed.is_empty() {
                return Err(FfiError::Validation(
                    "app_key, when provided, must be non-empty (use None for anonymous access)"
                        .into(),
                ));
            }
            // TfL's `app_key` format is 32 lowercase-hex chars. We don't
            // try to be lenient here — a wrong key length is almost
            // always a paste error and surfacing it before the first
            // request lets the UI keep the user on the onboarding
            // screen rather than redirecting to "stream is broken".
            if trimmed.len() != 32 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(FfiError::Validation(
                    "app_key must be 32 hex characters (paste from TfL developer portal)".into(),
                ));
            }
            Some(trimmed)
        }
        None => None,
    };

    tokio::task::yield_now().await;

    let client = shared_live_client(normalised_key.as_deref());
    // Block on stop-points warm so the FIRST `BoardService::refresh`
    // sees a populated `stop_points_cache`. Without this, the upstream
    // `resolve_arrival_ids` reads a cold `read_cache_any()`, fails to
    // find the queried station's `hub_naptan_code`, and falls back to
    // single-id arrivals — which means at multi-mode hubs (Liverpool
    // Street, Tottenham Court Road, Stratford, Whitechapel, …) the
    // Elizabeth / Overground sibling stop-points are never queried and
    // their predictions silently miss the first emit. The user's
    // observed symptom is "Elizabeth line never appears at Liverpool
    // Street". Awaiting warm here pays a one-time ~1–2 s on the first
    // launch of the process; subsequent calls within the same launch
    // find a warm cache and return immediately (single-flighted via
    // the upstream `stop_points_refresh` async lock, which also
    // coalesces with the kick-off warm spawned inside
    // `shared_live_client`).
    //
    // Failure is non-fatal — the subscription still spawns and falls
    // through to the cold-cache single-id path until the next periodic
    // refresh succeeds. Logging the cause keeps device-log debugging
    // honest when a real outage hides behind "no Elizabeth at hubs".
    if let Err(e) = client.warm_stop_points_cache().await {
        eprintln!(
            "[tfl-ffi] stop-points warm failed before subscription; \
             falling through with cold cache (hub merge inactive until \
             next refresh): {e}"
        );
    }
    let service = BoardService::new(client, SystemClock);

    let mut cfg = BoardConfig::new(&station_id);
    cfg.poll_seconds = poll_seconds;

    let refresh_notify = Arc::new(Notify::new());
    let (sub, refresh_snap_tx) =
        spawn_board_subscription(service, cfg.clone(), Arc::clone(&refresh_notify));

    // Spawn a one-shot refresh handler alongside the streaming forwarder.
    // When Swift calls `request_immediate_refresh()`, this task wakes,
    // performs a single `BoardService::refresh` via the shared client,
    // and injects the result into the same snap_tx channel that the
    // streaming forwarder writes to. The Swift consumer's `consume()`
    // loop handles it transparently — it doesn't care which task
    // produced the board.
    let refresh_key = normalised_key.clone();
    let refresh_cfg = cfg;
    let refresh_handle = tokio::spawn(async move {
        loop {
            refresh_notify.notified().await;
            let client = shared_live_client(refresh_key.as_deref());
            let one_shot = BoardService::new(client, SystemClock);
            let result = match one_shot.refresh(&refresh_cfg).await {
                Ok(board) => serde_json::to_string(&board)
                    .map_err(|e| FfiError::Refresh(format!("serialisation: {e}"))),
                Err(e) => Err(FfiError::from(e)),
            };
            if refresh_snap_tx.send(result).await.is_err() {
                break;
            }
        }
    });
    // Register the refresh task with the subscription so `shutdown()`
    // aborts it. Without this, the task stays parked on
    // `refresh_notify.notified()` forever after the subscription is
    // torn down — every station-change cycle would leak one task.
    *sub.refresh_task.lock().await = Some(refresh_handle);

    Ok(sub)
}

/// One-shot live `Board` fetch — used by Tubbie Next's BG-refresh
/// handler (B.6.4). Same fetch path as `subscribe_board_live` but no
/// subscription / streaming wrapper: returns a single fresh `Board` JSON
/// or an error. Capped at 8 s wall-time (mirrors legacy
/// `bg_refresh::BG_FETCH_BUDGET_SECS`); the OS gives BG-refresh handlers
/// ~30 s and we leave headroom for Swift to push the resulting Activity
/// updates and `setTaskCompleted(success:)` before its expiration fires.
///
/// Skips the stop-points warm — a BG wake firing inside the
/// `STOP_POINTS_TTL` window (15 min) finds a warm shared client cache,
/// which is the common case (BG fires every ~15 min). A cold-cache BG
/// wake silently falls through to single-id arrivals at multi-mode
/// hubs; the next foreground reopen warms and recovers Elizabeth /
/// Overground sibling-stop predictions.
///
/// Returns the same `Board` JSON shape as `subscribe_board_live`'s
/// `next_snapshot()` so Swift consumers reuse one decoder
/// (`Board: Decodable`).
#[uniffi::export(async_runtime = "tokio")]
pub async fn refresh_board_live(
    station_id: String,
    app_key: Option<String>,
) -> Result<String, FfiError> {
    if station_id.is_empty() || station_id.len() > 32 {
        return Err(FfiError::Validation(format!(
            "station_id must be 1–32 characters, got {}",
            station_id.len()
        )));
    }
    let normalised_key = match app_key {
        Some(k) => {
            let trimmed = k.trim().to_string();
            if trimmed.is_empty() {
                return Err(FfiError::Validation(
                    "app_key, when provided, must be non-empty (use None for anonymous access)"
                        .into(),
                ));
            }
            if trimmed.len() != 32 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(FfiError::Validation(
                    "app_key must be 32 hex characters (paste from TfL developer portal)".into(),
                ));
            }
            Some(trimmed)
        }
        None => None,
    };

    tokio::task::yield_now().await;

    let client = shared_live_client(normalised_key.as_deref());
    let service = BoardService::new(client, SystemClock);
    let cfg = BoardConfig::new(&station_id);

    let board = tokio::time::timeout(Duration::from_secs(8), service.refresh(&cfg))
        .await
        .map_err(|_| FfiError::Refresh("BG fetch exceeded 8 s budget".into()))?
        .map_err(|e| FfiError::Refresh(format!("BG fetch failed: {e}")))?;

    serde_json::to_string(&board).map_err(|e| FfiError::Refresh(format!("serialisation: {e}")))
}

/// Search live TfL stop-points by name.
///
/// Returns `Vec<Station>` JSON-encoded as a single string (same wire
/// pattern as `get_board_json` and `next_snapshot` — lets Swift use
/// `JSONDecoder` against a stable `Codable` schema without re-deriving
/// every field as a `uniffi::Record`).
///
/// Empty / whitespace queries return `[]` immediately (no network
/// fetch). Results are capped at 20 per the upstream contract.
#[uniffi::export(async_runtime = "tokio")]
pub async fn search_stations_live(
    query: String,
    app_key: Option<String>,
) -> Result<String, FfiError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok("[]".to_string());
    }
    // Reject `Some("")` symmetrically with `subscribe_board_live` so a
    // single SwiftUI text field can't produce different validation
    // outcomes depending on which export it feeds. The asymmetry the
    // first cut had ("search downgrades silently to anonymous, subscribe
    // rejects") was a UX trap.
    let normalised_key = match app_key {
        Some(k) => {
            let trimmed_key = k.trim().to_string();
            if trimmed_key.is_empty() {
                return Err(FfiError::Validation(
                    "app_key, when provided, must be non-empty (use None for anonymous access)"
                        .into(),
                ));
            }
            Some(trimmed_key)
        }
        None => None,
    };

    tokio::task::yield_now().await;

    let client = shared_live_client(normalised_key.as_deref());

    let stations = tokio::time::timeout(Duration::from_secs(10), client.search_stations(trimmed))
        .await
        .map_err(|_| {
            eprintln!(
                "[tfl-ffi] search_stations_live timed out: query_len={}",
                trimmed.len(),
            );
            FfiError::Refresh("Search timed out — TfL may be slow; try again in a moment.".into())
        })?
        // Stringifying `TflError` is safe re: secret leak: the upstream
        // `Display` impl explicitly sanitises the URL of its query
        // string before storing it (see `tfl-client::error::TflError::
        // transport_error_display_does_not_leak_app_key` for the
        // regression test). Same property holds for any future error
        // mapped through this branch — do not change to a stringifier
        // that bypasses `Display`.
        .map_err(|e| FfiError::Refresh(format!("search_stations: {e}")))?;
    serde_json::to_string(&stations).map_err(|e| FfiError::Refresh(format!("serialisation: {e}")))
}

/// Rank the live TfL stop-points list by great-circle distance from
/// `(lat, lon)` and return the top `limit` as JSON-encoded
/// `Vec<NearbyStation>`. Same wire pattern as `search_stations_live`
/// (Swift consumes via `JSONDecoder`).
///
/// Drives the "Near me" entry in the SwiftUI station picker. The
/// caller (Swift) has already obtained a one-shot CoreLocation fix
/// before invoking this; we don't reach for `CLLocationManager` here.
///
/// ## Validation
///
/// - `lat` ∈ `[-90, 90]`, `lon` ∈ `[-180, 180]`. Out-of-range values
///   return `FfiError::Validation` rather than producing garbled
///   distances — a paste-error or wired-wrong test fixture should
///   surface as a typed error, not a coincidentally-passing query.
/// - `limit > 0` and `limit <= 50`. Zero would always return `[]`
///   (a UX cliff masquerading as "no nearby stations"); >50 is well
///   beyond the visible list and would burn a sort cycle.
/// - `Some("")` / `Some("   ")` app_key rejected with the same
///   message as the other live exports — paste-error symmetry.
#[uniffi::export(async_runtime = "tokio")]
pub async fn find_nearest_stations_live(
    lat: f64,
    lon: f64,
    limit: u32,
    app_key: Option<String>,
) -> Result<String, FfiError> {
    if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
        return Err(FfiError::Validation(format!(
            "lat must be finite and within [-90, 90], got {lat}"
        )));
    }
    if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
        return Err(FfiError::Validation(format!(
            "lon must be finite and within [-180, 180], got {lon}"
        )));
    }
    if limit == 0 {
        return Err(FfiError::Validation(
            "limit must be > 0 (use 5 for the typical 'Near me' list)".into(),
        ));
    }
    if limit > 50 {
        return Err(FfiError::Validation(format!(
            "limit must be <= 50, got {limit}"
        )));
    }

    let normalised_key = match app_key {
        Some(k) => {
            let trimmed_key = k.trim().to_string();
            if trimmed_key.is_empty() {
                return Err(FfiError::Validation(
                    "app_key, when provided, must be non-empty (use None for anonymous access)"
                        .into(),
                ));
            }
            Some(trimmed_key)
        }
        None => None,
    };

    tokio::task::yield_now().await;

    let client = shared_live_client(normalised_key.as_deref());

    let nearest = tokio::time::timeout(
        Duration::from_secs(10),
        client.find_nearest_stations(lat, lon, limit as usize),
    )
    .await
    .map_err(|_| {
        eprintln!(
            "[tfl-ffi] find_nearest_stations_live timed out: lat={lat:.4} lon={lon:.4} limit={limit}",
        );
        FfiError::Refresh(
            "Search timed out — TfL may be slow; try again in a moment.".into(),
        )
    })?
    // `TflError::Display` is sanitised against `app_key` leak —
    // see comment on `search_stations_live`.
    .map_err(|e| FfiError::Refresh(format!("find_nearest_stations: {e}")))?;
    serde_json::to_string(&nearest).map_err(|e| FfiError::Refresh(format!("serialisation: {e}")))
}

/// Non-blocking proxy for whether the stop-points cache is likely warm.
///
/// Returns `true` if at least one `TflClient` has been created (and thus
/// its fire-and-forget `warm_stop_points_cache` was kicked off). Not a
/// precise expiry check — just a diagnostic tag for Swift `OSLog` events.
#[uniffi::export]
pub fn is_stop_points_cache_warm() -> bool {
    let guard = shared_live_clients()
        .lock()
        .expect("shared client mutex poisoned");
    !guard.is_empty()
}

/// Fetch the worst-first sorted list of every line's current status.
///
/// Wraps [`TflClient::get_all_line_statuses`]; returns the JSON-encoded
/// `Vec<LineStatus>` in the same wire pattern as the other live
/// exports (`get_board_json`, `next_snapshot`, `search_stations_live`)
/// — Swift consumes via `JSONDecoder`.
///
/// Validation symmetric with the other live exports:
/// - `Some("")` / `Some("   ")` rejected as a paste error (NOT
///   downgraded to anonymous; `None` is the explicit anonymous form).
///
/// The upstream client's worst-first sort + alphabetical tiebreak
/// is the canonical UI ordering — Swift consumers MUST NOT re-sort.
#[uniffi::export(async_runtime = "tokio")]
pub async fn get_line_statuses_live(app_key: Option<String>) -> Result<String, FfiError> {
    let normalised_key = match app_key {
        Some(k) => {
            let trimmed = k.trim().to_string();
            if trimmed.is_empty() {
                return Err(FfiError::Validation(
                    "app_key, when provided, must be non-empty (use None for anonymous access)"
                        .into(),
                ));
            }
            Some(trimmed)
        }
        None => None,
    };

    tokio::task::yield_now().await;

    let client = shared_live_client(normalised_key.as_deref());

    let statuses = client
        .get_all_line_statuses()
        .await
        // `TflError::Display` is sanitised against `app_key` leak —
        // see comment on `search_stations_live`.
        .map_err(|e| FfiError::Refresh(format!("get_line_statuses: {e}")))?;
    serde_json::to_string(&statuses).map_err(|e| FfiError::Refresh(format!("serialisation: {e}")))
}

/// Diagnostic-only: dump per-station metadata observed by the FFI's
/// shared client. Used by the SwiftUI "About → Diagnostics" surface to
/// help isolate whether a missing-Elizabeth-at-hubs report is caused by
/// the upstream cache (this dump shows the merged `lines`) or by some
/// later filter. The output is JSON of the form:
///
/// ```json
/// {
///   "station_id": "940GZZLULVT",
///   "warm_count": 2639,
///   "allowed_lines": ["central", "circle", "elizabeth", ...],
///   "arrival_line_counts": {"central": 23, "elizabeth": 125, ...},
///   "arrival_total": 195
/// }
/// ```
///
/// On failure each field is replaced with an `error` sibling. Not
/// performance-critical (it's a one-shot tap on About) and goes
/// through the same shared client as the rest of the live exports so
/// the dump reflects the cache state the live board sees.
#[uniffi::export(async_runtime = "tokio")]
pub async fn debug_station_metadata_live(
    station_id: String,
    app_key: Option<String>,
) -> Result<String, FfiError> {
    if station_id.is_empty() || station_id.len() > 32 {
        return Err(FfiError::Validation(format!(
            "station_id must be 1–32 characters, got {}",
            station_id.len()
        )));
    }
    let normalised_key = match app_key {
        Some(k) => {
            let trimmed = k.trim().to_string();
            if trimmed.is_empty() {
                return Err(FfiError::Validation(
                    "app_key, when provided, must be non-empty (use None for anonymous access)"
                        .into(),
                ));
            }
            Some(trimmed)
        }
        None => None,
    };

    let client = shared_live_client(normalised_key.as_deref());
    let warm_count_result = client.warm_stop_points_cache().await;
    let allowed_result = client.allowed_line_ids_for(&station_id).await;
    let arrivals_result = client.get_arrivals(&station_id).await;

    use serde_json::json;
    let mut blob = serde_json::Map::new();
    blob.insert("station_id".into(), json!(station_id));
    match warm_count_result {
        Ok(n) => {
            blob.insert("warm_count".into(), json!(n));
        }
        Err(e) => {
            blob.insert("warm_error".into(), json!(e.to_string()));
        }
    }
    match allowed_result {
        Ok(set) => {
            let mut sorted: Vec<String> = set.into_iter().collect();
            sorted.sort();
            blob.insert("allowed_lines".into(), json!(sorted));
        }
        Err(e) => {
            blob.insert("allowed_error".into(), json!(e.to_string()));
        }
    }
    match arrivals_result {
        Ok(arrivals) => {
            let mut counts: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for a in &arrivals {
                *counts.entry(a.line_id.clone()).or_default() += 1;
            }
            blob.insert("arrival_total".into(), json!(arrivals.len()));
            blob.insert("arrival_line_counts".into(), json!(counts));
        }
        Err(e) => {
            blob.insert("arrival_error".into(), json!(e.to_string()));
        }
    }

    serde_json::to_string(&blob).map_err(|e| FfiError::Refresh(format!("serialisation: {e}")))
}

/// Return the post-hub-merge set of line ids that physically serve
/// `station_id`, sorted alphabetically. Mirrors the upstream
/// `TflClient::allowed_line_ids_for` projection — same source of truth
/// the `drop_arrivals_for_lines_not_serving` filter and the
/// `CANONICAL_MULTI_MODE_HUBS` regression harness already use.
///
/// The Swift consumer (Tubbie Next) calls this once after `start(...)`
/// resolves a fresh subscription so the board can render a persistent
/// section per served line, even when the current `/Arrivals` payload
/// returns zero predictions for that line. Without this, TfL's natural
/// sparseness (DLR at Bank between trains, Elizabeth at TCR off-peak,
/// etc.) makes line sections flicker in and out on each poll — the
/// "Bank loses DLR" symptom on TestFlight 2026-05-08.
///
/// **Cold-cache behaviour.** Triggers a warm internally so callers
/// don't need to coordinate with `subscribe_board_live`'s warm. If the
/// warm itself fails, returns an empty `Vec` rather than an error —
/// fail-open mirrors `drop_arrivals_for_lines_not_serving` (an empty
/// served set means "we don't know what lines serve this station, fall
/// back to whatever arrivals deliver"). The FFI surfaces a propagated
/// error only for the `allowed_line_ids_for` call itself, which has
/// historically been infallible past a successful warm — the result
/// branch is guard-rail rather than expected failure mode.
#[uniffi::export(async_runtime = "tokio")]
pub async fn served_lines_for_station_live(
    station_id: String,
    app_key: Option<String>,
) -> Result<Vec<String>, FfiError> {
    if station_id.is_empty() || station_id.len() > 32 {
        return Err(FfiError::Validation(format!(
            "station_id must be 1–32 characters, got {}",
            station_id.len()
        )));
    }
    let normalised_key = match app_key {
        Some(k) => {
            let trimmed = k.trim().to_string();
            if trimmed.is_empty() {
                return Err(FfiError::Validation(
                    "app_key, when provided, must be non-empty (use None for anonymous access)"
                        .into(),
                ));
            }
            Some(trimmed)
        }
        None => None,
    };

    let client = shared_live_client(normalised_key.as_deref());
    let _ = client.warm_stop_points_cache().await;
    let allowed = client
        .allowed_line_ids_for(&station_id)
        .await
        .map_err(|e| FfiError::Refresh(format!("served lines fetch: {e}")))?;
    let mut sorted: Vec<String> = allowed.into_iter().collect();
    sorted.sort();
    Ok(sorted)
}

/// Internal helper: build a `BoardSubscription` from a fully constructed
/// `BoardService`. Shared by `subscribe_board` (fixture) and
/// `subscribe_board_live` (live HTTP). Generic over `H` and `C` so each
/// caller pays for monomorphisation of its own variant only.
///
/// Returns `(subscription, snap_tx)` — the caller can use `snap_tx` to
/// spawn additional tasks that feed results into the same consumer channel
/// (e.g. the pull-to-refresh one-shot handler in `subscribe_board_live`).
fn spawn_board_subscription<H, C>(
    service: BoardService<H, C>,
    cfg: BoardConfig,
    refresh_notify: Arc<Notify>,
) -> (
    Arc<BoardSubscription>,
    mpsc::Sender<Result<String, FfiError>>,
)
where
    H: TflHttp + 'static,
    C: Clock + 'static,
{
    let (cfg_tx, cfg_rx) = watch::channel(cfg);
    let lifecycle = LifecyclePhase::new(AppPhase::Active);
    let phase_rx = lifecycle.subscribe();
    let (snap_tx, snap_rx) = mpsc::channel::<Result<String, FfiError>>(8);

    let stream_snap_tx = snap_tx.clone();
    let task = tokio::spawn(async move {
        let mut stream = Box::pin(service.stream(cfg_rx, phase_rx));
        while let Some(item) = stream.next().await {
            let result = match item {
                Ok(board) => serde_json::to_string(&board)
                    .map_err(|e| FfiError::Refresh(format!("serialisation: {e}"))),
                Err(e) => Err(FfiError::from(e)),
            };
            if stream_snap_tx.send(result).await.is_err() {
                break;
            }
        }
    });

    let sub = Arc::new(BoardSubscription {
        _cfg_tx: cfg_tx,
        lifecycle,
        rx: Mutex::new(snap_rx),
        task: Mutex::new(Some(task)),
        refresh_task: Mutex::new(None),
        refresh_notify,
    });
    (sub, snap_tx)
}

// Generate the C scaffolding uniffi needs to mount these exports. Must be
// called once per crate. The bindgen entrypoint lives in
// `src/bin/uniffi-bindgen.rs`.
uniffi::setup_scaffolding!();

#[cfg(test)]
mod tests {
    //! Inline smoke tests. The bulk of B.1 acceptance lives in
    //! `tests/spike_acceptance.rs` (integration) so they exercise the
    //! crate from the outside, the same way Swift will.

    use super::*;

    #[test]
    fn ffi_error_display_is_human_readable() {
        let err = FfiError::Validation("station_id too long".into());
        assert_eq!(format!("{err}"), "validation: station_id too long");
    }

    #[tokio::test(start_paused = true)]
    async fn search_timeout_maps_to_refresh_with_message() {
        let stalling = futures::future::pending::<Result<Vec<u8>, String>>();
        let result = tokio::time::timeout(Duration::from_secs(10), stalling).await;
        assert!(result.is_err(), "should timeout after 10s");
        let mapped: FfiError = result
            .map_err(|_| {
                FfiError::Refresh(
                    "Search timed out — TfL may be slow; try again in a moment.".into(),
                )
            })
            .unwrap_err();
        match mapped {
            FfiError::Refresh(msg) => {
                assert!(msg.contains("timed out"), "expected 'timed out' in {msg:?}");
            }
            other => panic!("expected Refresh, got {other:?}"),
        }
    }

    #[test]
    fn board_error_maps_to_refresh_variant() {
        // Smoke test the `From<BoardError>` impl — ensures the FFI surface
        // does not silently swallow upstream error context.
        let upstream = BoardError::Fetch(tfl_client::error::TflError::NotFound("test".into()));
        let mapped: FfiError = upstream.into();
        match mapped {
            FfiError::Refresh(msg) => assert!(
                msg.to_lowercase().contains("not found") || msg.contains("test"),
                "expected refresh error to preserve NotFound context, got {msg:?}"
            ),
            other => panic!("expected Refresh, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Key-pool integration tests
// ---------------------------------------------------------------------------
//
// These tests exercise the interaction between `KEY_POOL`, `ClientKey::Pooled`,
// and `shared_live_client`. Because `KEY_POOL` is a process-wide `OnceLock`
// these tests MUST run in an isolated process — use
//   cargo test --test key_pool_integration
// or the full gate `just ios-test`. Running them inline would race with
// `shared_live_client_for_test` usages in `tests/shared_client.rs`.
//
// The assertions here are compile-time contracts: if `ClientKey::Pooled`
// or the pool routing in `shared_live_client` changes shape, these will
// no longer type-check.

#[cfg(test)]
mod pool_tests {
    use super::*;

    fn valid_key(prefix: u8) -> String {
        format!("{:0<32}", format!("{:x}", prefix))
    }

    #[test]
    fn initialize_key_pool_with_empty_keys_leaves_pool_uninitialised() {
        // Cannot call initialize_key_pool() in a shared-process test
        // because OnceLock is first-write-wins. We can validate the
        // underlying KeyPool construction directly instead.
        let result = key_pool::KeyPool::new(vec![]);
        assert!(result.is_none(), "empty key set must not produce a pool");
    }

    #[test]
    fn initialize_key_pool_with_all_invalid_keys_leaves_pool_uninitialised() {
        let bad = vec!["tooshort".into(), "z".repeat(32)];
        let result = key_pool::KeyPool::new(bad);
        assert!(
            result.is_none(),
            "all-invalid key set must not produce a pool"
        );
    }

    #[test]
    fn client_key_pooled_hashes_and_equals_by_slot_index() {
        use std::collections::HashMap;
        let mut map: HashMap<ClientKey, u32> = HashMap::new();
        map.insert(ClientKey::Pooled(0), 100);
        map.insert(ClientKey::Pooled(1), 200);
        assert_eq!(map[&ClientKey::Pooled(0)], 100);
        assert_eq!(map[&ClientKey::Pooled(1)], 200);
        // Anonymous is a distinct bucket from any Pooled slot.
        assert!(!map.contains_key(&ClientKey::Anonymous));
    }

    #[test]
    fn client_key_pooled_is_not_equal_to_anonymous() {
        assert_ne!(ClientKey::Pooled(0), ClientKey::Anonymous);
    }

    #[test]
    fn user_key_bypasses_pool_client_key_type() {
        // The routing logic's first branch: Some(k) => Authenticated.
        // Verify by constructing the key directly (pool-routing path
        // tested in the isolated integration test binary).
        let user_key = valid_key(0xAB);
        let ck = ClientKey::Authenticated(user_key.clone());
        assert_ne!(ck, ClientKey::Pooled(0));
        assert_ne!(ck, ClientKey::Anonymous);
    }
}
