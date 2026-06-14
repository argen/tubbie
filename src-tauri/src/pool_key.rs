//! Pool-key onboarding — Phase 1 (Mac desktop).
//!
//! Fetches a pool of TfL API keys from the tubbie key-server and selects one
//! via atomic round-robin. Used as a fallback when the user has no personal
//! `app_key` in the Keychain, so anonymous TfL quota (50 req/min shared-IP)
//! is avoided without any user action.
//!
//! ## Fail-open contract
//!
//! Every failure mode (DNS, 5xx, timeout, malformed JSON, empty pool) returns
//! `None` — the caller falls back to `ReqwestTflHttp::new()` (anonymous). The
//! board is never blocked or delayed by this module.
//!
//! ## Priority
//!
//! Personal `app_key` (Keychain) always overrides the pool. The caller in
//! `lib.rs::setup` short-circuits before calling the pool fetcher when a
//! personal key is already loaded.
//!
//! ## Endpoint
//!
//! `https://tubbie.brunobelcastro.com/pool-keys.json` — shared with iOS.
//! JSON schema: `{"schema_version": 1, "keys": ["<32 hex digits>", ...]}`
//! Only `schema_version == 1` is accepted. Invalid keys are silently dropped.
//!
//! ## Selection
//!
//! Atomic round-robin: global `AtomicUsize` cursor advances by 1 on each
//! `pick()` call; `cursor mod len` selects the key. Same algorithm as iOS.
//!
//! ## Non-blocking by construction
//!
//! The board is NEVER blocked on the key service (a hard invariant). The Mac
//! client bakes its key in at construction and has no live-swap path, so the
//! pool key is applied across two launches, not within one:
//!
//!   * **Startup** reads the last-cached pool key *synchronously* from the
//!     config store ([`validated_cached_key`]) — a local read, no network —
//!     and bakes it into the one `Arc<TflClient>` (`lib.rs::setup`).
//!   * **After setup**, a background task ([`fetch_one_pool_key`]) refreshes
//!     the cache from the network for the *next* launch. It does not touch the
//!     already-built client.
//!
//! Consequence: a first-ever launch with an empty cache runs anonymous for
//! that session (anonymous still shows arrivals within seconds); the next
//! launch is keyed. No network call ever sits on the startup critical path,
//! and no dedicated runtime / `block_on` is needed (the refresh runs on the
//! ambient Tauri runtime).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Pool-keys endpoint (shared with iOS spec).
pub(crate) const POOL_KEYS_URL: &str = "https://tubbie.brunobelcastro.com/pool-keys.json";

/// Hard per-request network timeout. Fast enough to not delay startup;
/// long enough for a slow connection to succeed. Matches iOS `fetchTimeout`.
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(3);

/// Only schema_version == 1 is accepted.
const ACCEPTED_SCHEMA_VERSION: u64 = 1;

// ---------------------------------------------------------------------------
// Key validation (mirrors iOS `PoolKeyParser`)
// ---------------------------------------------------------------------------

/// Returns `true` if `key` is exactly 32 ASCII hex digits.
///
/// Mirrors the iOS Swift predicate:
/// `k.count == 32 && k.allSatisfy(\.isHexDigit)`.
pub(crate) fn is_valid_pool_key(key: &str) -> bool {
    key.len() == 32 && key.chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Wire-format structs
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct PoolKeysPayload {
    schema_version: u64,
    keys: Vec<String>,
}

// ---------------------------------------------------------------------------
// KeyPool — in-memory validated key list + round-robin cursor
// ---------------------------------------------------------------------------

/// A validated, immutable set of pool keys. Created once after a successful
/// network fetch; the round-robin cursor is the only mutable state.
pub(crate) struct KeyPool {
    keys: Vec<String>,
    cursor: AtomicUsize,
}

impl KeyPool {
    /// Build from a raw slice. Silently drops entries that fail `is_valid_pool_key`.
    /// Returns `None` if no valid keys remain after filtering.
    pub(crate) fn new(raw: &[String]) -> Option<Self> {
        let keys: Vec<String> = raw
            .iter()
            .filter(|k| is_valid_pool_key(k))
            .cloned()
            .collect();
        if keys.is_empty() {
            None
        } else {
            Some(Self {
                keys,
                cursor: AtomicUsize::new(0),
            })
        }
    }

    /// Atomically advance the round-robin cursor and return the selected key.
    ///
    /// Returns `(slot_index, &key)`. Ordering is `Relaxed` — same as iOS
    /// `AtomicInt.fetchAndAdd`. No synchronisation beyond the fetch is required
    /// because keys are immutable after construction.
    pub(crate) fn pick(&self) -> (usize, &str) {
        let slot = self.cursor.fetch_add(1, Ordering::Relaxed) % self.keys.len();
        (slot, &self.keys[slot])
    }

    /// The validated keys, in fetch order. Used by the `get_pool_keys` command
    /// to hand the (public) list to the renderer for the TypeScript data path.
    pub(crate) fn keys(&self) -> &[String] {
        &self.keys
    }

    /// Number of valid keys in the pool.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.keys.len()
    }
}

// ---------------------------------------------------------------------------
// HTTP fetch (async)
// ---------------------------------------------------------------------------

/// Fetch and parse the pool-keys payload from an explicit URL.
///
/// Returns the validated `KeyPool` or `None` on any error (network, HTTP
/// error status, parse failure, empty/all-invalid key list).
///
/// This is the testable core — tests point `url` at a `wiremock::MockServer`.
/// The production entry point wraps this with the canonical `POOL_KEYS_URL`.
pub(crate) async fn fetch_pool_keys_from_url(
    http_client: &reqwest::Client,
    url: &str,
) -> Option<KeyPool> {
    let response = match http_client.get(url).timeout(FETCH_TIMEOUT).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[tubbie:pool-key] network error: {e}");
            return None;
        }
    };

    if !response.status().is_success() {
        eprintln!(
            "[tubbie:pool-key] server returned {}: fail-open",
            response.status()
        );
        return None;
    }

    let payload: PoolKeysPayload = match response.json().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[tubbie:pool-key] JSON parse error: {e}");
            return None;
        }
    };

    if payload.schema_version != ACCEPTED_SCHEMA_VERSION {
        eprintln!(
            "[tubbie:pool-key] unsupported schema_version {}: fail-open",
            payload.schema_version
        );
        return None;
    }

    let pool = KeyPool::new(&payload.keys);
    if pool.is_none() {
        eprintln!("[tubbie:pool-key] zero valid keys after filter: fail-open");
    }
    pool
}

/// Fetch the pool and return a single selected key string (round-robin pick).
///
/// `None` on any failure (fail-open). This is the async entry point the
/// background cache-refresh task in `lib.rs` runs on the ambient Tauri
/// runtime — no dedicated runtime, no `block_on`, never on the startup path.
pub(crate) async fn fetch_one_pool_key(http_client: &reqwest::Client, url: &str) -> Option<String> {
    let pool = fetch_pool_keys_from_url(http_client, url).await?;
    let (_slot, key) = pool.pick();
    Some(key.to_string())
}

/// Fetch the full validated pool-key list for the renderer (the `USE_TS_TFL`
/// TypeScript data path). Returns an empty `Vec` on any failure (fail-open) —
/// the caller treats empty as "run unauthenticated".
///
/// Why this exists: the keys are **public** (published at [`POOL_KEYS_URL`],
/// shared with iOS), so handing them to the webview leaks nothing. The webview
/// can't read `POOL_KEYS_URL` itself — the endpoint sends no
/// `Access-Control-Allow-Origin`, so a cross-origin `fetch` can't read the body
/// — but the Rust shell's `reqwest` is immune to webview CORS. So Rust proxies
/// the list. Builds its own short-lived client like `refresh_pool_key_cache`;
/// `AppState` holds no shared `reqwest::Client`.
pub(crate) async fn fetch_all_pool_keys(url: &str) -> Vec<String> {
    let client = match reqwest::Client::builder().timeout(FETCH_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tubbie:pool-key] client build error: {e}");
            return Vec::new();
        }
    };
    match fetch_pool_keys_from_url(&client, url).await {
        Some(pool) => pool.keys().to_vec(),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Cached-key read (called synchronously from lib.rs::setup — no network)
// ---------------------------------------------------------------------------

/// Extract a valid pool key from a cached config-store value.
///
/// `raw` is whatever `StorePluginConfigStore::raw_get("pool_key_cache")`
/// returned (the JSON string written by the last background refresh, or
/// `None`/`Null`/garbage). Returns `Some(key)` only if it is a syntactically
/// valid pool key — a corrupt or stale-format cache entry fails closed to
/// `None` (anonymous), never poisons the startup client with a malformed key.
pub(crate) fn validated_cached_key(raw: Option<serde_json::Value>) -> Option<String> {
    let key = raw?.as_str()?.to_string();
    if is_valid_pool_key(&key) {
        Some(key)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Priority selector (the seam lib.rs calls)
// ---------------------------------------------------------------------------

/// Select the startup key using priority: personal key > cached pool key > None.
///
/// `pool_fetcher` is called only when `personal_key` is `None`. At startup it
/// is a *synchronous, non-network* closure that reads the last-cached pool key
/// ([`validated_cached_key`]); passing a closure keeps `lib.rs` simple and lets
/// tests inject a mock without touching the store.
///
/// In production, `lib.rs::setup` calls (roughly):
/// ```ignore
/// let saved_key = select_startup_key(saved_key, || {
///     validated_cached_key(plugin_store.raw_get("pool_key_cache"))
/// });
/// ```
pub(crate) fn select_startup_key(
    personal_key: Option<String>,
    pool_fetcher: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if personal_key.is_some() {
        personal_key
    } else {
        pool_fetcher()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // -----------------------------------------------------------------------
    // is_valid_pool_key
    // -----------------------------------------------------------------------

    #[test]
    fn valid_key_32_hex_digits_lowercase() {
        assert!(is_valid_pool_key("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"));
    }

    #[test]
    fn valid_key_32_hex_digits_uppercase_also_accepted() {
        // is_ascii_hexdigit accepts both cases (matches iOS behaviour).
        assert!(is_valid_pool_key("A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4"));
    }

    #[test]
    fn invalid_key_too_short() {
        assert!(!is_valid_pool_key("a1b2c3d4e5f6"));
    }

    #[test]
    fn invalid_key_too_long() {
        assert!(!is_valid_pool_key("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4ff"));
    }

    #[test]
    fn invalid_key_non_hex_char() {
        assert!(!is_valid_pool_key("z1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"));
    }

    // -----------------------------------------------------------------------
    // KeyPool
    // -----------------------------------------------------------------------

    #[test]
    fn key_pool_new_filters_invalid_entries() {
        let raw = vec![
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4".to_string(), // valid
            "tooshort".to_string(),                         // invalid
            "b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5".to_string(), // valid
        ];
        let pool = KeyPool::new(&raw).expect("should have 2 valid keys");
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn key_pool_new_all_invalid_returns_none() {
        let raw = vec!["bad".to_string(), "alsoBad".to_string()];
        assert!(KeyPool::new(&raw).is_none());
    }

    #[test]
    fn key_pool_new_empty_returns_none() {
        assert!(KeyPool::new(&[]).is_none());
    }

    #[test]
    fn key_pool_round_robin_wraps() {
        let raw = vec![
            "aaaabbbbccccddddaaaabbbbccccdddd".to_string(),
            "1111222233334444111122223333444b".to_string(),
        ];
        let pool = KeyPool::new(&raw).unwrap();
        let (slot0, key0) = pool.pick();
        let (slot1, key1) = pool.pick();
        let (slot2, key2) = pool.pick(); // wraps back to slot 0

        assert_eq!(slot0, 0);
        assert_eq!(key0, "aaaabbbbccccddddaaaabbbbccccdddd");
        assert_eq!(slot1, 1);
        assert_eq!(key1, "1111222233334444111122223333444b");
        assert_eq!(slot2, 0);
        assert_eq!(key2, "aaaabbbbccccddddaaaabbbbccccdddd");
    }

    // -----------------------------------------------------------------------
    // Test 1: cold install + reachable pool endpoint → keyed client (selected
    // key applied). Tests that `fetch_pool_keys_from_url` returns a valid pool
    // when the server responds 200 with a valid payload.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn pool_key_cold_install_reachable_endpoint_returns_key() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "schema_version": 1,
                "keys": ["a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let pool = fetch_pool_keys_from_url(&client, &url)
            .await
            .expect("should return a pool with one valid key");

        let (_slot, key) = pool.pick();
        assert_eq!(key, "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4");
    }

    /// Multiple keys → round-robin selects first key on first call.
    #[tokio::test]
    async fn pool_key_multiple_keys_first_pick_is_slot_zero() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "schema_version": 1,
                "keys": [
                    "aaaabbbbccccddddaaaabbbbccccdddd",
                    "1111222233334444111122223333444b"
                ]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let pool = fetch_pool_keys_from_url(&client, &url)
            .await
            .expect("should return a pool with two valid keys");

        let (slot, key) = pool.pick();
        assert_eq!(slot, 0, "first pick must be slot 0");
        assert_eq!(key, "aaaabbbbccccddddaaaabbbbccccdddd");
    }

    // -----------------------------------------------------------------------
    // `fetch_all_pool_keys` — full validated list for the renderer (TS path).
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_all_pool_keys_returns_full_validated_list() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "schema_version": 1,
                "keys": [
                    "aaaabbbbccccddddaaaabbbbccccdddd",
                    "1111222233334444111122223333444b",
                    "not-a-valid-key"
                ]
            })))
            .mount(&server)
            .await;

        let url = format!("{}/pool-keys.json", server.uri());
        let keys = fetch_all_pool_keys(&url).await;

        // The invalid entry is dropped; both valid keys come through in order.
        assert_eq!(
            keys,
            vec![
                "aaaabbbbccccddddaaaabbbbccccdddd".to_string(),
                "1111222233334444111122223333444b".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn fetch_all_pool_keys_returns_empty_on_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let url = format!("{}/pool-keys.json", server.uri());
        assert!(
            fetch_all_pool_keys(&url).await.is_empty(),
            "a failed fetch must fail-open to an empty list (unauthenticated)"
        );
    }

    // -----------------------------------------------------------------------
    // Test 2: pool endpoint 5xx / timeout → anonymous fallback, no panic.
    // Tests that `fetch_pool_keys_from_url` returns `None` on HTTP errors.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn pool_key_server_5xx_returns_none_no_panic() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let result = fetch_pool_keys_from_url(&client, &url).await;
        assert!(result.is_none(), "5xx must fail-open (None)");
    }

    #[tokio::test]
    async fn pool_key_server_429_returns_none_no_panic() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(429).set_body_string("Too Many Requests"))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let result = fetch_pool_keys_from_url(&client, &url).await;
        assert!(result.is_none(), "429 must fail-open (None)");
    }

    #[tokio::test]
    async fn pool_key_connection_refused_returns_none_no_panic() {
        // Port 1 is almost certainly refused / unreachable on any dev machine.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .unwrap();
        let result = fetch_pool_keys_from_url(&client, "http://127.0.0.1:1/pool-keys.json").await;
        assert!(result.is_none(), "connection refused must fail-open (None)");
    }

    // -----------------------------------------------------------------------
    // Test 3: malformed / empty JSON → anonymous fallback, no panic.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn pool_key_malformed_json_returns_none_no_panic() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("not json at all {{{{")
                    .insert_header("content-type", "application/json"),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let result = fetch_pool_keys_from_url(&client, &url).await;
        assert!(result.is_none(), "malformed JSON must fail-open (None)");
    }

    #[tokio::test]
    async fn pool_key_empty_keys_array_returns_none_no_panic() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "schema_version": 1,
                "keys": []
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let result = fetch_pool_keys_from_url(&client, &url).await;
        assert!(result.is_none(), "empty keys array must fail-open (None)");
    }

    #[tokio::test]
    async fn pool_key_all_invalid_keys_returns_none_no_panic() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "schema_version": 1,
                "keys": ["tooshort", "notvalidhex!!!!!!!!!!!!!!!!!!"]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let result = fetch_pool_keys_from_url(&client, &url).await;
        assert!(result.is_none(), "all-invalid keys must fail-open (None)");
    }

    #[tokio::test]
    async fn pool_key_wrong_schema_version_returns_none_no_panic() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "schema_version": 2,
                "keys": ["a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let result = fetch_pool_keys_from_url(&client, &url).await;
        assert!(
            result.is_none(),
            "schema_version != 1 must fail-open (None)"
        );
    }

    // -----------------------------------------------------------------------
    // Test 4: manual key present in Keychain → overrides pool.
    // Tested via `select_startup_key` which is the seam lib.rs calls.
    // -----------------------------------------------------------------------

    /// Personal key present → pool fetcher is NEVER called (absolute priority).
    #[test]
    fn select_key_personal_key_overrides_pool_fetcher_not_called() {
        let personal_key = Some("personalkey1234567890123456789012".to_string());
        let selected = select_startup_key(personal_key.clone(), || {
            panic!("pool fetch MUST NOT be called when personal key is present")
        });
        assert_eq!(selected, personal_key);
    }

    /// No personal key → pool fetcher IS called and its result is used.
    #[test]
    fn select_key_no_personal_key_uses_pool() {
        let pool_key = "poolkeyxxxxxxxxxxxxxxxxxxxxx1234a".to_string();
        let pool_key_clone = pool_key.clone();
        let selected = select_startup_key(None, || Some(pool_key_clone));
        assert_eq!(selected, Some(pool_key));
    }

    /// No personal key + pool returns None → result is None (anonymous).
    #[test]
    fn select_key_no_personal_no_pool_returns_none() {
        let selected = select_startup_key(None, || None);
        assert!(
            selected.is_none(),
            "no personal key + empty pool must fall back to anonymous (None)"
        );
    }

    /// Invalid keys in pool are silently filtered; valid ones survive.
    #[tokio::test]
    async fn pool_key_mixed_valid_and_invalid_keys_keeps_valid_ones() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "schema_version": 1,
                "keys": [
                    "badkey",                                    // invalid
                    "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",        // valid
                    "gggggggggggggggggggggggggggggggg",         // invalid (not hex)
                    "b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5"         // valid
                ]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let pool = fetch_pool_keys_from_url(&client, &url)
            .await
            .expect("should return a pool with 2 valid keys");
        assert_eq!(pool.len(), 2, "only valid keys survive the filter");
    }

    // -----------------------------------------------------------------------
    // validated_cached_key — the synchronous, no-network startup read
    // -----------------------------------------------------------------------

    #[test]
    fn validated_cached_key_accepts_valid_cached_string() {
        let raw = Some(serde_json::json!("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"));
        assert_eq!(
            validated_cached_key(raw).as_deref(),
            Some("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4")
        );
    }

    #[test]
    fn validated_cached_key_none_when_absent_or_null() {
        assert!(validated_cached_key(None).is_none());
        assert!(validated_cached_key(Some(serde_json::Value::Null)).is_none());
    }

    #[test]
    fn validated_cached_key_fails_closed_on_corrupt_entry() {
        // A stale-format / corrupt cache entry must NOT poison the startup
        // client with a malformed key — it fails closed to None (anonymous).
        assert!(validated_cached_key(Some(serde_json::json!("tooshort"))).is_none());
        assert!(validated_cached_key(Some(serde_json::json!(12345))).is_none());
        assert!(validated_cached_key(Some(serde_json::json!({"k": "v"}))).is_none());
    }

    // -----------------------------------------------------------------------
    // fetch_one_pool_key — the async entry point the background refresh runs
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn fetch_one_pool_key_returns_selected_key_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "schema_version": 1,
                "keys": ["a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        let key = fetch_one_pool_key(&client, &url).await;
        assert_eq!(key.as_deref(), Some("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"));
    }

    #[tokio::test]
    async fn fetch_one_pool_key_none_on_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pool-keys.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/pool-keys.json", server.uri());
        assert!(
            fetch_one_pool_key(&client, &url).await.is_none(),
            "server error must fail-open to None"
        );
    }
}
