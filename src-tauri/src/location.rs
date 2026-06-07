//! Native Core Location bridge for the "find nearest station" feature.
//!
//! `WKWebView`'s `navigator.geolocation` is gated by a `WKUIDelegate`
//! permission callback that Tauri / `wry` does not wire — JS geolocation
//! silently fails or hangs in production. We therefore acquire the user's
//! location in Rust via `CLLocationManager` and expose a `LocationFix`
//! to the renderer through a Tauri IPC command. JS only provides the
//! user gesture.
//!
//! ## Lifetime model
//!
//! Each call to [`request_current_location`] is single-flight (a
//! double-tap waits behind the first request) and single-shot. The
//! manager is allocated on the macOS main thread, kept alive in a
//! global slot for the duration of the request, and dropped as soon as
//! the delegate fires (success, failure, or timeout). No background
//! tracking. No retained delegate across calls.
//!
//! ## Privacy
//!
//! `lat` / `lon` are never logged. Diagnostic eprintlns include only
//! `accuracy_m` and `elapsed_ms`. The fix is not persisted anywhere on
//! disk — it lives in the IPC response and dies with the page.

#![cfg(target_os = "macos")]
#![allow(unsafe_code)]

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AllocAnyThread};
use objc2_core_location::{
    CLAuthorizationStatus, CLError, CLLocation, CLLocationManager, CLLocationManagerDelegate,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Public wire types
// ---------------------------------------------------------------------------

/// One resolved location fix. `accuracy_m` is the horizontal radius (in
/// metres) within which the true position lies with ~68% confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationFix {
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
}

/// One reason the location request did not resolve into a `LocationFix`.
///
/// Each variant maps 1:1 to a listbox row in `StationSearch.svelte`:
/// the Rust side is the source of truth for *which* error happened,
/// the renderer is the source of truth for *how it reads*. This keeps
/// copy decisions out of platform code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum LocationError {
    /// User explicitly denied (or revoked) permission.
    PermissionDenied,
    /// Permission is restricted by parental controls / MDM. The user
    /// can't grant — they have to clear the restriction in Settings.
    PermissionRestricted,
    /// Location Services is disabled system-wide.
    ServicesDisabled,
    /// 8s elapsed without a fix.
    Timeout,
    /// CoreLocation reported `horizontalAccuracy < 0` — the receiver
    /// has no idea where it is. Indistinguishable from a network /
    /// indoor failure; UX is identical to Timeout.
    LowAccuracy,
    /// iOS-only: the app was backgrounded when the request fired. Set
    /// by the iOS bridge before any CLLocationManager touch — see
    /// the iOS-side `location.rs` (invariant #8).
    AppBackground,
    /// Any other CL error. `message` is for diagnostic logs only — the
    /// renderer collapses this onto the `Timeout` row.
    Internal { message: String },
}

// ---------------------------------------------------------------------------
// Single-flight + main-thread state
// ---------------------------------------------------------------------------

type ResultSender = oneshot::Sender<Result<LocationFix, LocationError>>;

/// Wrap a `Retained<T>` so it can be moved across threads without the
/// compiler enforcing `T: Send`. We only ever call methods on the
/// inner pointer from the macOS main thread (where Cocoa expects
/// CLLocationManager), so the cross-thread move is just bookkeeping.
struct AssertSend<T>(T);
unsafe impl<T> Send for AssertSend<T> {}

struct State {
    /// The `tx` end of the in-flight request. `None` between requests.
    sender: Option<ResultSender>,
    /// Manager + delegate are owned here so they outlive the
    /// `requestLocation` call. Cleared on completion.
    manager: Option<AssertSend<Retained<CLLocationManager>>>,
    delegate: Option<AssertSend<Retained<TubbieLocationDelegate>>>,
    /// Wall-clock instant the request kicked off, used for diagnostic
    /// `elapsed_ms` logging without leaking lat/lon.
    started_at: Option<Instant>,
}

fn state_slot() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(State {
            sender: None,
            manager: None,
            delegate: None,
            started_at: None,
        })
    })
}

fn serial_lock() -> &'static tokio::sync::Mutex<()> {
    static L: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(()))
}

// ---------------------------------------------------------------------------
// Delegate
// ---------------------------------------------------------------------------

define_class!(
    /// `CLLocationManagerDelegate` implementation that funnels all
    /// callbacks into the static state slot. The delegate carries no
    /// instance state of its own — it just looks up the in-flight
    /// sender and resolves it.
    #[unsafe(super(NSObject))]
    #[name = "TubbieLocationDelegate"]
    #[derive(Debug)]
    pub struct TubbieLocationDelegate;

    unsafe impl NSObjectProtocol for TubbieLocationDelegate {}

    unsafe impl CLLocationManagerDelegate for TubbieLocationDelegate {
        #[unsafe(method(locationManager:didUpdateLocations:))]
        fn did_update_locations(
            &self,
            _manager: &CLLocationManager,
            locations: &NSArray<CLLocation>,
        ) {
            on_did_update_locations(locations);
        }

        #[unsafe(method(locationManager:didFailWithError:))]
        fn did_fail_with_error(&self, _manager: &CLLocationManager, error: &NSError) {
            on_did_fail(error);
        }

        #[unsafe(method(locationManagerDidChangeAuthorization:))]
        fn did_change_authorization(&self, manager: &CLLocationManager) {
            on_did_change_authorization(manager);
        }
    }
);

impl TubbieLocationDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc();
        unsafe { msg_send![this, init] }
    }
}

// ---------------------------------------------------------------------------
// Delegate-side handlers (run on the macOS main thread)
// ---------------------------------------------------------------------------

fn on_did_update_locations(locations: &NSArray<CLLocation>) {
    let Some(loc) = locations.firstObject() else {
        // Empty array — keep waiting for the next callback.
        return;
    };
    let coord = unsafe { loc.coordinate() };
    let acc = unsafe { loc.horizontalAccuracy() };

    if acc < 0.0 {
        complete_with(Err(LocationError::LowAccuracy));
        return;
    }

    complete_with(Ok(LocationFix {
        lat: coord.latitude,
        lon: coord.longitude,
        accuracy_m: acc,
    }));
}

fn on_did_fail(error: &NSError) {
    let code = error.code();
    let mapped = if code == CLError::Denied.0 {
        LocationError::PermissionDenied
    } else if code == CLError::LocationUnknown.0 {
        // CoreLocation will retry internally; we treat the first surfaced
        // LocationUnknown as a soft failure and let the timeout drive UX.
        LocationError::Timeout
    } else {
        LocationError::Internal {
            message: format!("CLError code={code}"),
        }
    };
    complete_with(Err(mapped));
}

fn on_did_change_authorization(manager: &CLLocationManager) {
    let status = unsafe { manager.authorizationStatus() };
    if status == CLAuthorizationStatus::AuthorizedAlways
        || status == CLAuthorizationStatus::AuthorizedWhenInUse
    {
        unsafe { manager.requestLocation() };
    } else if status == CLAuthorizationStatus::Denied {
        complete_with(Err(LocationError::PermissionDenied));
    } else if status == CLAuthorizationStatus::Restricted {
        complete_with(Err(LocationError::PermissionRestricted));
    } else if status == CLAuthorizationStatus::NotDetermined {
        // Undecided app: trigger the system prompt. This is the ONLY place we
        // request authorization — driving it from the delegate (rather than a
        // synchronous read in `start_request_on_main`) is what fixes the
        // "denied permission times out instead of reporting PermissionDenied"
        // bug: a fresh manager's `authorizationStatus` reads NotDetermined
        // until this callback fires with the true value, so a synchronous read
        // mis-classified an already-Denied app as NotDetermined and then sat on
        // `requestWhenInUseAuthorization` (which shows no prompt for a decided
        // app) until the 8s timeout. The user's choice re-fires this callback
        // with Authorized (→ requestLocation) or Denied (→ PermissionDenied).
        // `requestWhenInUseAuthorization` on an already-decided app is a no-op
        // and does not re-fire, so there is no loop.
        unsafe { manager.requestWhenInUseAuthorization() };
    }
}

/// Resolve the in-flight oneshot, drop the manager + delegate, and log
/// `elapsed_ms` (never coords).
fn complete_with(result: Result<LocationFix, LocationError>) {
    let (sender, manager, delegate, started_at) = {
        let mut st = state_slot().lock().expect("location state");
        (
            st.sender.take(),
            st.manager.take(),
            st.delegate.take(),
            st.started_at.take(),
        )
    };
    if let Some(mgr) = manager.as_ref() {
        unsafe { mgr.0.stopUpdatingLocation() };
    }
    drop(manager);
    drop(delegate);
    if let Some(tx) = sender {
        if let Some(t0) = started_at {
            let elapsed_ms = t0.elapsed().as_millis();
            match &result {
                Ok(fix) => eprintln!(
                    "[location] fix accuracy_m={:.1} elapsed_ms={}",
                    fix.accuracy_m, elapsed_ms
                ),
                Err(e) => eprintln!("[location] error elapsed_ms={} kind={:?}", elapsed_ms, e),
            }
        }
        let _ = tx.send(result);
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Request a single location fix. Single-flight, single-shot, 8 s timeout.
///
/// Concurrent callers serialise behind [`serial_lock`] — a double-tap on
/// the crosshair button is processed as two requests one after the other,
/// not as two parallel `CLLocationManager` instances and definitely not
/// as two system permission prompts.
pub async fn request_current_location<R: Runtime>(
    app: AppHandle<R>,
) -> Result<LocationFix, LocationError> {
    let _serial = serial_lock().lock().await;

    let (tx, rx) = oneshot::channel::<Result<LocationFix, LocationError>>();
    {
        let mut st = state_slot().lock().expect("location state");
        st.sender = Some(tx);
        st.manager = None;
        st.delegate = None;
        st.started_at = Some(Instant::now());
    }

    let dispatch = app.run_on_main_thread(start_request_on_main);
    if dispatch.is_err() {
        // Main-thread queue refused us — the app is probably tearing
        // down. Drain the slot so the next caller starts clean.
        let mut st = state_slot().lock().expect("location state");
        st.sender = None;
        st.started_at = None;
        return Err(LocationError::Internal {
            message: "main thread dispatch failed".into(),
        });
    }

    match tokio::time::timeout(Duration::from_secs(8), rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(LocationError::Internal {
            message: "oneshot dropped".into(),
        }),
        Err(_) => {
            // Timeout. Tear down on main thread so the next caller's
            // `requestLocation` sees a clean slate.
            let _ = app.run_on_main_thread(|| {
                complete_with(Err(LocationError::Timeout));
            });
            Err(LocationError::Timeout)
        }
    }
}

/// Setup work scheduled onto the macOS main thread. Allocates the
/// manager + delegate, decides whether to prompt or fire, and stashes
/// both into the state slot so the delegate methods can find them.
fn start_request_on_main() {
    let manager: Retained<CLLocationManager> = unsafe { CLLocationManager::new() };
    let delegate = TubbieLocationDelegate::new();

    if !unsafe { CLLocationManager::locationServicesEnabled_class() } {
        complete_with(Err(LocationError::ServicesDisabled));
        return;
    }

    unsafe {
        // ~100 m is plenty to rank stations: we're not computing a route,
        // we're picking the nearest of ~600 candidates.
        manager.setDesiredAccuracy(100.0);
    }

    {
        // Stash the owning handles BEFORE wiring the delegate. Setting the
        // delegate fires `locationManagerDidChangeAuthorization` (synchronously
        // on macOS), and that callback looks the manager up in this slot to
        // call `requestLocation` / `requestWhenInUseAuthorization`. It must
        // already be populated when that fires.
        let mut st = state_slot().lock().expect("location state");
        st.manager = Some(AssertSend(manager));
        st.delegate = Some(AssertSend(delegate));
    }

    // Re-borrow the manager + delegate via the slot so we can wire them without
    // moving back out. We know they're `Some` because we just put them there.
    let (manager_ptr, delegate_ptr) = {
        let st = state_slot().lock().expect("location state");
        (
            st.manager.as_ref().expect("manager just stashed").0.clone(),
            st.delegate
                .as_ref()
                .expect("delegate just stashed")
                .0
                .clone(),
        )
    };

    // Wire the delegate. We do NOT read `authorizationStatus` synchronously
    // here — a fresh manager reports NotDetermined until the framework fires
    // the initial `didChangeAuthorization` with the real value, so a sync read
    // mis-classifies an already-Denied app as NotDetermined and then hangs on a
    // (no-op) authorization prompt until the 8s timeout. Instead, the delegate
    // callback `on_did_change_authorization` is the single source of truth: it
    // fires once after the delegate is set (macOS 11+) and decides whether to
    // fetch, prompt, or report the denial.
    unsafe {
        let proto: &ProtocolObject<dyn CLLocationManagerDelegate> =
            ProtocolObject::from_ref(&*delegate_ptr);
        manager_ptr.setDelegate(Some(proto));
    }
}
