//! Process-wide shared `TflClient` cache.
//!
//! ## Why this exists
//!
//! Before this fix, every live FFI export (`search_stations_live`,
//! `find_nearest_stations_live`, `subscribe_board_live`,
//! `get_line_statuses_live`) constructed a fresh `TflClient` per call.
//! That meant a brand-new `stop_points_cache` per call. Two user-visible
//! consequences observed on the SwiftUI build:
//!
//! 1. **Search felt slow.** Each non-debounced keystroke that survived
//!    the 250 ms debounce in `StationSearchClient` triggered a 4-mode
//!    parallel fan-out (~16 MB) plus hub fan-out — every keystroke. The
//!    upstream stale-while-revalidate caching was effectively disabled.
//!
//! 2. **Elizabeth / Overground lines went missing at hub stations.**
//!    `resolve_arrival_ids` reads `read_cache_any()` to find a
//!    `hub_naptan_code`. Cold cache → no hub id → fall back to single-id
//!    arrivals fetch → only the tube parent's predictions reach the
//!    board. At Liverpool Street and Tottenham Court Road this presents
//!    as "Elizabeth line never shows up". Same shape at every multi-mode
//!    hub.
//!
//! The fix is a process-wide `Arc<TflClient>` keyed by `app_key` state
//! (`Anonymous` vs `Authenticated`). All four live exports go through
//! `shared_live_client` so they share the cache. First init schedules a
//! background `warm_stop_points_cache` so subsequent calls hit a hot
//! cache.
//!
//! These tests guard the wiring contract: same key → same `Arc`,
//! different keys → different `Arc`. The data-correctness story (hub
//! merge produces Elizabeth lines at Liverpool Street) is covered
//! upstream by `tfl-cache::client_tests` and by the iOS manual smoke.

use std::sync::Arc;

use tfl_ffi::shared_live_client_for_test;

#[tokio::test(flavor = "multi_thread")]
async fn shared_client_anonymous_returns_same_instance_across_calls() {
    // The slowness fix is "reuse the same client across FFI calls".
    // Pointer-equality is the strictest form of "same client" — if two
    // calls return different `Arc`s the cache is split and the upstream
    // `stop_points_cache` is per-client, so the bug silently regresses.
    let a = shared_live_client_for_test(None);
    let b = shared_live_client_for_test(None);
    assert!(
        Arc::ptr_eq(&a, &b),
        "anonymous shared_live_client must return the same Arc across calls",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_client_same_app_key_returns_same_instance() {
    let key = "0123456789abcdef0123456789abcdef".to_string();
    let a = shared_live_client_for_test(Some(&key));
    let b = shared_live_client_for_test(Some(&key));
    assert!(
        Arc::ptr_eq(&a, &b),
        "same app_key must reuse the same Arc<TflClient>",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_client_anon_and_authenticated_are_distinct() {
    // Anonymous and authenticated paths CANNOT share a client — the
    // `app_key` is wired into `ReqwestTflHttp` at construction and shows
    // up on every outbound request. Sharing the anon client for a keyed
    // user would silently downgrade them to anonymous and burn the
    // shared 50 req/min anonymous bucket; sharing the keyed client for
    // anonymous users would leak the secret onto requests they didn't
    // authorise. Hard separation is load-bearing.
    let key = "0123456789abcdef0123456789abcdef".to_string();
    let anon = shared_live_client_for_test(None);
    let authed = shared_live_client_for_test(Some(&key));
    assert!(
        !Arc::ptr_eq(&anon, &authed),
        "anonymous and authenticated clients must be different Arc instances",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_client_distinct_app_keys_are_distinct() {
    // Two different users on the same device (key swap in Settings)
    // must NOT share a `TflClient` — same secret-leak / quota-attribution
    // story as anon-vs-keyed. Each `app_key` keys its own cache slot.
    let a = "0123456789abcdef0123456789abcdef".to_string();
    let b = "fedcba9876543210fedcba9876543210".to_string();
    let client_a = shared_live_client_for_test(Some(&a));
    let client_b = shared_live_client_for_test(Some(&b));
    assert!(
        !Arc::ptr_eq(&client_a, &client_b),
        "different app_keys must produce different Arc<TflClient> instances",
    );
}
