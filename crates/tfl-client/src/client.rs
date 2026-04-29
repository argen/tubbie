//! Typed TfL API client.
//!
//! `TflClient<H>` is generic over any `TflHttp` implementation, enabling
//! fully offline, deterministic testing via `FixtureTflHttp` and live
//! network calls via `ReqwestTflHttp`.
//!
//! ## Multi-mode model
//!
//! The client surfaces four TfL modes — Tube, Overground, DLR, Elizabeth
//! — via [`SUPPORTED_MODES`]. `stop_points_cached` and `get_line_status`
//! both fan out across these modes in parallel and merge the per-mode
//! responses into a single cache. Downstream consumers with tighter
//! resource budgets (the iOS shell) can opt to a subset via
//! [`TflClient::with_modes`]; everything else routes through
//! [`TflClient::new`], which uses the full set.
//!
//! See [`docs/ADR/multi-mode-stop-points-cache.md`](../../../docs/ADR/multi-mode-stop-points-cache.md)
//! for the rationale and the trade-offs (single-flight refresh, hub-fetch
//! dedupe, stale-but-usable lookups, search dedupe at interchanges).
//!
//! ## Design decisions
//!
//! ### `get_arrivals` — NotFound propagation
//! When the fixture (or live API) returns `TflError::NotFound`, we propagate
//! it directly. No re-wrapping; the `FixtureTflHttp` already includes the
//! path in the message and `ReqwestTflHttp` will include the station id.
//!
//! ### `get_arrivals` — multi-mode hub merge
//! At hub interchanges (Bank / Whitechapel / Stratford / Highbury &
//! Islington / …) the queried stop-point id only returns its own
//! mode's arrivals — `940GZZLUBNK` returns Tube only, `940GZZDLBNK`
//! returns DLR only. `resolve_arrival_ids` reads the cached station's
//! `hub_naptan_code` (via `read_cache_any`, so this still works past
//! the stop-points TTL), looks up the hub's children once via
//! `hub_children_cached`, and fans out parallel arrivals fetches to
//! every sibling. Failures on individual siblings are dropped rather
//! than nuking the whole board.
//!
//! ### `get_line_status` — disruption text strategy
//! TfL's `lineStatuses` may contain multiple entries (e.g. "Severe Delays"
//! on one segment + "Part Suspended" on another). We collect the non-empty,
//! unique `reason` fields and join them with `" | "`. If all reasons are
//! absent or blank, `disruption_text` is `None`. Using `reason` rather than
//! `disruption.description` because `reason` is always a top-level string
//! (no nesting) and the two fields are typically identical in content.
//!
//! ### `search_stations` — relevance ordering + result cap
//! Relevance: exact-match (case-insensitive) first, then `starts_with`, then
//! `contains`, with alphabetical `common_name` as the tiebreaker within each
//! tier. Capped at 20 results (autocomplete UX: dumping thousands of rows is
//! worse than useless). Empty query returns empty (not all stations).
//! Filters by NaPTAN canonical-prefix whitelist (`940GZZLU` Tube,
//! `940GZZDL` DLR, `910G` filtered to `overground`/`elizabeth-line` to
//! drop NR-only operators) and dedupes by `hub_naptan_code` so multi-
//! mode interchanges show one canonical row each.

use crate::error::TflError;
use crate::http::TflHttp;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tfl_domain::types::{
    is_supported_line_id, pretty_line_name, Arrival, LineRef, LineStatus, Station, StatusEntry,
    TflLine,
};

/// TfL modes tubbie surfaces. `TflClient::new` defaults to this set; the
/// public `TflClient::with_modes` constructor lets downstream consumers
/// (e.g. the iOS shell with a tighter memory budget) opt to a subset.
///
/// Mirrored in `crates/fixture-recorder` (`SURFACED_MODES`) and in the
/// hub-children mode filter inside this file. Adding a mode requires a
/// matching fixture under `fixtures/{stop-points,line-status}/{mode}.json`
/// and an entry in the `is_supported_line_id` whitelist in `tfl-domain`.
pub const SUPPORTED_MODES: &[&str] = &["tube", "overground", "dlr", "elizabeth-line"];

/// How long a `stop-points/tube` response stays cached before the next
/// `search_stations` call refetches. 15 minutes keeps the 16 MB payload off
/// the wire for typical settings-page sessions while still picking up TfL
/// station-metadata edits within a lunchbreak.
const STOP_POINTS_TTL: Duration = Duration::from_secs(15 * 60);

/// How long the `line-status/tube` response is cached.
///
/// 60 s matches the frontend ticker period, so UI calls for each visible
/// line are all served from a single wire fetch per minute instead of one
/// per-line per-tick (typically 3–5× multiplier for multi-line stations).
const LINE_STATUS_TTL: Duration = Duration::from_secs(60);

struct StopPointsCacheEntry {
    fetched_at: Instant,
    stations: Vec<Station>,
}

/// Typed TfL API client, generic over any `TflHttp` transport.
///
/// Instantiate with a `FixtureTflHttp` for offline/test use, or a
/// `ReqwestTflHttp` for live network calls (M3+).
///
/// ```rust,ignore
/// let client = TflClient::new(FixtureTflHttp::new("fixtures/"));
/// let arrivals = client.get_arrivals("940GZZLUBZP").await?;
/// ```
pub struct TflClient<H: TflHttp> {
    http: H,
    /// Modes this client surfaces. Defaults to [`SUPPORTED_MODES`]; use
    /// [`TflClient::with_modes`] to construct a subset client.
    modes: &'static [&'static str],
    /// Single-flight gate around `stop_points_cached` refreshes. Without
    /// this, debounced search keystrokes (200 ms) that all land in the
    /// same cold-cache or post-TTL window each see an empty
    /// `stop_points_cache` and fire their own full per-mode + hub
    /// fan-out — three keystrokes ⇒ three parallel ~3-second warms
    /// against TfL. The async mutex serialises refreshes: the first
    /// caller does the work; the rest await, then re-check the cache
    /// and return immediately.
    ///
    /// Held for the duration of the network fan-out, NOT during the
    /// quick cache-fresh check. Read-only callers (search after warm)
    /// never touch this lock.
    stop_points_refresh: tokio::sync::Mutex<()>,
    stop_points_cache: Mutex<Option<StopPointsCacheEntry>>,
    /// Per-process map from a hub NaPTAN id (e.g. `HUBBAN`) to the list
    /// of child stop-point ids whose arrivals we want to merge — tube,
    /// DLR, Overground, Elizabeth. Populated lazily the first time we
    /// resolve arrivals for a station that has a hub. Entries are stable
    /// for the lifetime of the process; TfL doesn't restructure hubs at
    /// runtime.
    hub_children_cache: Mutex<HashMap<String, Vec<String>>>,
    /// Per-process map from a hub NaPTAN id to the merged set of
    /// `LineRef`s served by its children. Populated lazily when
    /// `stop_points_cached` enriches a hub station's `lines` field so
    /// the Settings chip UI shows DLR / Elizabeth / Overground chips
    /// alongside tube lines. Stable for the process lifetime.
    hub_lines_cache: Mutex<HashMap<String, Vec<LineRef>>>,
    /// Short-lived cache for the `/Line/Mode/tube/Status` response.
    ///
    /// The frontend calls `get_line_status` once per visible line on each
    /// ticker cycle. Without this cache, every call fetches the entire
    /// tube line list, meaning 3–5 identical 16 kB requests per minute at
    /// a typical multi-line station. With a 60 s TTL (matching the ticker
    /// period) the list is fetched once and all per-line lookups are
    /// served from memory.
    line_status_cache: Mutex<Option<(Instant, Vec<TflLine>)>>,
}

impl<H: TflHttp> TflClient<H> {
    /// Create a new `TflClient` wrapping the given transport. Surfaces
    /// every mode in [`SUPPORTED_MODES`]. For an opt-down client (e.g.
    /// memory-constrained iOS shell), use [`TflClient::with_modes`].
    pub fn new(http: H) -> Self {
        Self::with_modes(http, SUPPORTED_MODES)
    }

    /// Create a `TflClient` that only fetches the supplied modes.
    ///
    /// `modes` is borrowed as `&'static` because the slice lifetime must
    /// outlive every cache entry. Pass a `const` slice (e.g.
    /// `&["tube", "overground"]`) — string allocation is unnecessary here
    /// and would defeat the cheap-Copy guarantee.
    ///
    /// Modes outside [`SUPPORTED_MODES`] are accepted by the constructor
    /// but will produce `NotFound` from the fixture transport when their
    /// stop-points / line-status JSON is absent. This is intentional —
    /// it lets a downstream consumer ship its own fixtures for a mode
    /// without us having to teach the constructor about it.
    pub fn with_modes(http: H, modes: &'static [&'static str]) -> Self {
        Self {
            http,
            modes,
            stop_points_refresh: tokio::sync::Mutex::new(()),
            stop_points_cache: Mutex::new(None),
            hub_children_cache: Mutex::new(HashMap::new()),
            hub_lines_cache: Mutex::new(HashMap::new()),
            line_status_cache: Mutex::new(None),
        }
    }

    /// Fetch the live arrival predictions for a stop point.
    ///
    /// Returns a `Vec<Arrival>` in TfL's natural order (typically sorted by
    /// `timeToStation` ascending, but not guaranteed by the API).
    ///
    /// ## Multi-mode hub stations
    ///
    /// At Tottenham Court Road / Bank / Whitechapel / Stratford and similar
    /// shared stations the tube parent id only returns tube arrivals —
    /// DLR, Overground, and Elizabeth predictions sit on sibling stop-points
    /// (`940GZZDLBNK`, `910GTOTCTRD`, …). The hub id (`HUBBAN`, `HUBTCR`)
    /// itself returns nothing because TfL hubs aggregate physical platforms,
    /// not predictions.
    ///
    /// So when the requested station has a `hubNaptanCode`, we look up the
    /// hub's children once (and cache the result for the lifetime of the
    /// process), filter to the modes we surface, fan out a parallel arrivals
    /// fetch to every sibling, and concatenate the results. Cold-cache or
    /// non-hub stations fall back to the single-id path unchanged.
    ///
    /// # Errors
    /// - `TflError::NotFound` — unknown station id or missing fixture.
    /// - `TflError::Parse` — response is valid JSON but not a `Vec<Arrival>`.
    /// - `TflError::ParseAt` — fixture file is invalid JSON (offline only).
    /// - `TflError::Transport` — network failure (live client only).
    pub async fn get_arrivals(&self, stop_point_id: &str) -> Result<Vec<Arrival>, TflError> {
        let ids = self.resolve_arrival_ids(stop_point_id).await;

        // Single-id fast path — preserves error semantics (NotFound / Parse
        // propagate) for tube-only stations and for the cold-cache window.
        if ids.len() == 1 {
            let value = self.http.fetch("arrivals", ids[0].as_str()).await?;
            let arrivals: Vec<Arrival> = serde_json::from_value(value)?;
            return Ok(arrivals);
        }

        // Multi-id parallel fetch (hub stations). Drop individual failures
        // rather than nuking the whole board if e.g. the Elizabeth-line
        // sub-stop is briefly 404 — the user still sees tube arrivals.
        let fetches = ids.iter().map(|id| async move {
            let value = self.http.fetch("arrivals", id).await.ok()?;
            serde_json::from_value::<Vec<Arrival>>(value).ok()
        });
        let results = futures::future::join_all(fetches).await;

        // Dedupe by `Arrival.id` — TfL occasionally returns the same
        // prediction across two child stop-points (typically when the tube
        // parent and a sibling both list a shared bay), and Svelte's
        // keyed `{#each}` block crashes with `each_key_duplicate` if the
        // same id reaches the UI twice. First-seen wins.
        let mut seen = std::collections::HashSet::new();
        let mut merged: Vec<Arrival> = results
            .into_iter()
            .flatten()
            .flatten()
            .filter(|a| seen.insert(a.id.clone()))
            .collect();
        // TfL doesn't promise ordering even per-id; sort by timeToStation so
        // the platform columns stay coherent after merge.
        merged.sort_by_key(|a| a.time_to_station);
        Ok(merged)
    }

    /// Resolve the list of stop-point ids whose arrivals we should query.
    /// Returns `[stop_point_id]` for tube-only stations or cold cache.
    /// Returns the hub's filtered children for multi-mode stations.
    ///
    /// Uses `read_cache_any` (not `read_fresh_cache`) so a TTL-stale cache
    /// entry still serves the `hub_naptan_code` lookup. Without this, the
    /// first stream tick after a 15-min TTL expiry would lose hub-merge
    /// for hub stations (Bank/Euston/Whitechapel) — `read_fresh_cache`
    /// returns `None` past TTL, falls back to single-id, and the user's
    /// chip filter sees zero arrivals because Overground/DLR siblings
    /// were never fetched.
    async fn resolve_arrival_ids(&self, stop_point_id: &str) -> Vec<String> {
        let hub_id = self.read_cache_any().and_then(|stations| {
            stations
                .iter()
                .find(|s| s.id == stop_point_id)
                .and_then(|s| s.hub_naptan_code.clone())
        });

        let Some(hub_id) = hub_id else {
            return vec![stop_point_id.to_string()];
        };

        match self.hub_children_cached(&hub_id).await {
            Ok(children) if !children.is_empty() => children,
            _ => vec![stop_point_id.to_string()],
        }
    }

    /// Look up (and lazily populate) the cached child stop-point ids for a
    /// hub. Children are filtered to modes we surface (tube / DLR /
    /// Overground / Elizabeth) so we don't waste an arrivals fetch on the
    /// bus stops that hubs also enumerate.
    async fn hub_children_cached(&self, hub_id: &str) -> Result<Vec<String>, TflError> {
        if let Ok(guard) = self.hub_children_cache.lock() {
            if let Some(cached) = guard.get(hub_id) {
                return Ok(cached.clone());
            }
        }

        let value = self.http.fetch("stop-point", hub_id).await?;
        // Hub StopPoint detail JSON has `children: [ { id, modes, ... } ]`.
        let children = value
            .get("children")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|c| {
                        c.get("modes")
                            .and_then(|m| m.as_array())
                            .map(|m| {
                                m.iter().any(|mode| {
                                    matches!(
                                        mode.as_str(),
                                        Some("tube")
                                            | Some("dlr")
                                            | Some("overground")
                                            | Some("elizabeth-line")
                                    )
                                })
                            })
                            .unwrap_or(false)
                    })
                    .filter_map(|c| c.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        if let Ok(mut guard) = self.hub_children_cache.lock() {
            guard.insert(hub_id.to_string(), children.clone());
        }
        Ok(children)
    }

    /// Return the merged set of lines served by a hub's children, for use in
    /// the Settings chip UI. Reads `lineModeGroups` from every child whose
    /// `modes` list contains a mode we surface (tube / DLR / Overground /
    /// Elizabeth), then deduplicates by line id and filters through
    /// `is_supported_line_id`.
    ///
    /// **Caching policy:** every successful resolution is cached for the
    /// process lifetime, AND a `TflError::NotFound` (genuinely-absent hub
    /// at TfL) is cached as an empty Vec so we don't re-fetch a known-404
    /// hub on every cold-warm. Transient errors (transport, rate-limited)
    /// are NOT cached — those will retry on the next warm cycle. The
    /// canonical `~190 tube hubs in the fixture vs. 4 recorded HUB*.json
    /// files` situation hits this path: the 186 missing hubs return
    /// `NotFound` once, get cached as empty, and never re-fetch.
    async fn hub_lines_cached(&self, hub_id: &str) -> Vec<LineRef> {
        if let Ok(guard) = self.hub_lines_cache.lock() {
            if let Some(cached) = guard.get(hub_id) {
                return cached.clone();
            }
        }

        let value = match self.http.fetch("stop-point", hub_id).await {
            Ok(v) => v,
            Err(TflError::NotFound(_)) => {
                if let Ok(mut guard) = self.hub_lines_cache.lock() {
                    guard.insert(hub_id.to_string(), vec![]);
                }
                return vec![];
            }
            Err(_) => return vec![],
        };

        let mut seen = std::collections::HashSet::new();
        let lines: Vec<LineRef> = value
            .get("children")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|c| {
                        c.get("modes")
                            .and_then(|m| m.as_array())
                            .map(|m| {
                                m.iter().any(|mode| {
                                    matches!(
                                        mode.as_str(),
                                        Some("tube")
                                            | Some("dlr")
                                            | Some("overground")
                                            | Some("elizabeth-line")
                                    )
                                })
                            })
                            .unwrap_or(false)
                    })
                    .flat_map(|c| {
                        c.get("lineModeGroups")
                            .and_then(|g| g.as_array())
                            .map(|groups| {
                                groups
                                    .iter()
                                    .filter(|g| {
                                        let mode = g
                                            .get("modeName")
                                            .and_then(|m| m.as_str())
                                            .unwrap_or("");
                                        mode.is_empty()
                                            || matches!(
                                                mode,
                                                "tube" | "dlr" | "overground" | "elizabeth-line"
                                            )
                                    })
                                    .flat_map(|g| {
                                        g.get("lineIdentifier")
                                            .and_then(|l| l.as_array())
                                            .map(|ids| {
                                                ids.iter()
                                                    .filter_map(|id| id.as_str().map(String::from))
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_default()
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    })
                    .filter(|id| is_supported_line_id(id))
                    .filter(|id| seen.insert(id.clone()))
                    .map(|id| {
                        let name = pretty_line_name(&id).to_string();
                        LineRef { id, name }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if let Ok(mut guard) = self.hub_lines_cache.lock() {
            guard.insert(hub_id.to_string(), lines.clone());
        }
        lines
    }

    /// Fetch the current status for a line on any surfaced mode.
    ///
    /// On cold cache, fans out one `line-status/{mode}` fetch per entry in
    /// `self.modes` in parallel via `futures::future::join_all`, concatenates
    /// the resulting `Vec<TflLine>`s into a single in-memory Vec stamped with
    /// one [`Instant`], and serves the per-line lookup against that union.
    ///
    /// Per-mode fetch failures are logged once and the mode is skipped —
    /// a stale or missing fixture for one mode must not poison the whole
    /// cache (e.g. early-development states where `line-status/dlr.json`
    /// hasn't been recorded yet). The cache is still stamped on partial
    /// success so we don't refetch on every keystroke.
    ///
    /// # Disruption text strategy
    /// Non-empty, unique `reason` strings from all `lineStatuses` entries are
    /// joined with `" | "`. Good-service lines have no reasons, so
    /// `disruption_text` is `None`.
    ///
    /// # Errors
    /// - `TflError::NotFound` — `line_id` not found across any surfaced mode.
    /// - `TflError::Transport` — every mode's fetch failed (network only).
    pub async fn get_line_status(&self, line_id: &str) -> Result<LineStatus, TflError> {
        // Serve from the TTL cache when fresh. The merged line list is
        // fetched once per LINE_STATUS_TTL window across all surfaced
        // modes; per-line lookups all run against the cached Vec<TflLine>.
        let cached_lines = {
            let guard = self.line_status_cache.lock().unwrap_or_else(|p| {
                eprintln!("[tfl-client] line_status_cache mutex poisoned; recovering");
                p.into_inner()
            });
            guard.as_ref().and_then(|(fetched_at, lines)| {
                if fetched_at.elapsed() < LINE_STATUS_TTL {
                    Some(lines.clone())
                } else {
                    None
                }
            })
        };

        let lines = if let Some(lines) = cached_lines {
            lines
        } else {
            let fresh = self.fetch_line_status_all_modes().await?;
            match self.line_status_cache.lock() {
                Ok(mut guard) => {
                    *guard = Some((Instant::now(), fresh.clone()));
                }
                Err(poison) => {
                    eprintln!("[tfl-client] line_status_cache mutex poisoned on write; recovering");
                    let mut guard = poison.into_inner();
                    *guard = Some((Instant::now(), fresh.clone()));
                }
            }
            fresh
        };

        let tfl_line = lines
            .into_iter()
            .find(|l| l.id == line_id)
            .ok_or_else(|| TflError::NotFound(format!("line not found: {line_id}")))?;

        Ok(tfl_line_to_line_status(tfl_line))
    }

    /// Fan out `line-status/{mode}` fetches for every configured mode and
    /// concatenate the parsed `Vec<TflLine>`s. Per-mode failures are logged
    /// and skipped; only an *entirely* failed cycle propagates as `Err`.
    async fn fetch_line_status_all_modes(&self) -> Result<Vec<TflLine>, TflError> {
        let fetches = self.modes.iter().map(|mode| async move {
            match self.http.fetch("line-status", mode).await {
                Ok(value) => match serde_json::from_value::<Vec<TflLine>>(value) {
                    Ok(lines) => Some(lines),
                    Err(e) => {
                        eprintln!("[tfl-client] line-status/{mode} parse failed: {e}");
                        None
                    }
                },
                Err(e) => {
                    // Single-mode fetch failure is non-fatal — log and continue.
                    // Cold-cache start-up regularly hits a 404 here when a fixture
                    // is missing in dev; production logs a transport error once
                    // per failed mode per TTL window (60 s).
                    eprintln!("[tfl-client] line-status/{mode} fetch failed: {e}");
                    None
                }
            }
        });
        let per_mode = futures::future::join_all(fetches).await;

        let merged: Vec<TflLine> = per_mode.into_iter().flatten().flatten().collect();
        if merged.is_empty() {
            // Every mode fetch failed — propagate as a Transport error so the
            // caller can surface the outage. Use `NotFound` with a crisp
            // message because we don't have a single underlying error to
            // report and the caller's fallback path treats this the same way.
            return Err(TflError::NotFound(
                "line-status: all modes failed".to_string(),
            ));
        }
        Ok(merged)
    }

    /// Search for tube stations by name.
    ///
    /// Fetches the full stop-points list (one round trip), filters to
    /// tube-mode stations matching `query` (case-insensitive substring), and
    /// returns at most 20 results ordered by relevance.
    ///
    /// An empty `query` returns an empty `Vec` — callers should not trigger
    /// a search until the user has typed at least one character.
    ///
    /// # Relevance ordering (stable within each tier)
    /// 1. Exact match on `common_name` (case-insensitive) — tier 0.
    /// 2. `common_name` starts with `query` — tier 1.
    /// 3. `common_name` contains `query` — tier 2.
    /// 4. Alphabetical by `common_name` within each tier.
    ///
    /// # Errors
    /// - `TflError::Parse` — stop-points response lacks a `stopPoints` array
    ///   or individual station entries cannot be deserialized.
    /// - `TflError::ParseAt` — fixture file is invalid JSON (offline only).
    /// - `TflError::Transport` — network failure (live client only).
    pub async fn search_stations(&self, query: &str) -> Result<Vec<Station>, TflError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(vec![]);
        }

        let stations = self.stop_points_cached().await?;
        let q = trimmed.to_lowercase();

        // Prefix-aware whitelist for canonical station group ids:
        //   - `940GZZLU*` — London Underground canonical
        //   - `940GZZDL*` — DLR canonical
        //   - `910G*` — National Rail group; admit only those whose modes
        //     include `overground` or `elizabeth-line` (the 910G NaPTAN
        //     range overlaps with NR-only operators like Gatwick Express
        //     and Thameslink, which we don't surface).
        //
        // Excluded by absence:
        //   - `9400ZZLU*`, `4900*`, `2100*` — platform-level children that
        //     would duplicate rows in the dropdown
        //   - `HUB*` — multi-mode aggregators with no stable arrivals id
        let prefiltered: Vec<Station> = stations
            .into_iter()
            .filter(|s| {
                if s.id.starts_with("940GZZLU") || s.id.starts_with("940GZZDL") {
                    true
                } else if s.id.starts_with("910G") {
                    s.modes
                        .iter()
                        .any(|m| matches!(m.as_str(), "overground" | "elizabeth-line"))
                } else {
                    false
                }
            })
            .filter(|s| s.common_name.to_lowercase().contains(&q))
            .collect();

        // Dedupe by `hub_naptan_code`: at multi-mode interchanges (Bank,
        // Farringdon, …) the per-mode `/StopPoint/Mode/{mode}` feeds each
        // return their own canonical entry — `940GZZLUBNK` and `940GZZDLBNK`
        // both have `hubNaptanCode: HUBBAN` because they're the same
        // physical station. After the hub-merge step in `stop_points_cached`
        // both entries also carry the same union of lines, so the dropdown
        // shows two near-identical rows that route to the same arrivals via
        // the hub-merge fan-out in `get_arrivals`. Keep one canonical entry
        // per hub code, preferring 940GZZLU (tube) > 940GZZDL (DLR) > 910G
        // (Overground / Elizabeth) — the user's mental model maps a hub to
        // its tube parent at every interchange we surface today.
        //
        // Stations whose `hub_naptan_code` is `None` (Hampstead Heath,
        // Belsize Park, most single-mode stops) are passed through
        // unchanged — they have no hub partner to dedupe against.
        let prefix_priority = |id: &str| -> u8 {
            if id.starts_with("940GZZLU") {
                0
            } else if id.starts_with("940GZZDL") {
                1
            } else {
                2
            }
        };
        let mut by_hub: HashMap<String, Station> = HashMap::new();
        let mut without_hub: Vec<Station> = Vec::new();
        for s in prefiltered {
            match s.hub_naptan_code.clone() {
                Some(hub_id) => match by_hub.get(&hub_id) {
                    Some(existing) if prefix_priority(&existing.id) <= prefix_priority(&s.id) => {
                        // Existing entry is higher- or equal-priority; drop the new one.
                    }
                    _ => {
                        by_hub.insert(hub_id, s);
                    }
                },
                None => without_hub.push(s),
            }
        }
        let mut matches: Vec<Station> = by_hub.into_values().chain(without_hub).collect();

        // Sort by relevance tier, then alphabetically within each tier.
        matches.sort_by(|a, b| {
            let a_lower = a.common_name.to_lowercase();
            let b_lower = b.common_name.to_lowercase();
            let a_tier = relevance_tier(&a_lower, &q);
            let b_tier = relevance_tier(&b_lower, &q);
            a_tier
                .cmp(&b_tier)
                .then_with(|| a.common_name.cmp(&b.common_name))
        });

        matches.truncate(20);
        Ok(matches)
    }

    /// Pre-fetch and cache the stop-points list. Fire-and-forget from app
    /// startup so the first `search_stations` call is instant.
    ///
    /// Idempotent: if the cache has any data (fresh or stale), returns its
    /// size without a network round-trip. Use [`refresh_stop_points_cache`]
    /// to force a refresh regardless of cache state.
    pub async fn warm_stop_points_cache(&self) -> Result<usize, TflError> {
        let stations = self.stop_points_cached().await?;
        Ok(stations.len())
    }

    /// Force a stop-points fan-out + hub-merge regardless of current cache
    /// state. Used by the periodic background refresh task in
    /// `lib.rs::run` to keep the cache fresh without blocking any user
    /// action — the steady-state refresh runs out-of-band with respect to
    /// `search_stations`, which always returns whatever's currently
    /// cached (even if stale).
    ///
    /// Single-flighted via the async `stop_points_refresh` mutex so a
    /// concurrent search-triggered cold-warm and the periodic tick can't
    /// both fan out at once.
    ///
    /// Returns the cached station count after the refresh stamps.
    pub async fn refresh_stop_points_cache(&self) -> Result<usize, TflError> {
        let stations = self.refresh_stop_points_inner(true).await?;
        Ok(stations.len())
    }

    /// Return the set of `line_id`s that legitimately serve `station_id`.
    ///
    /// Source of truth for `BoardService::refresh`'s defensive filter:
    /// any arrival whose `line_id` is not in this set is dropped before
    /// the board is built. TfL occasionally surfaces predictions for
    /// lines that don't physically serve the queried station (most
    /// commonly via the hub-merge path — a sibling stop-point's
    /// prediction leaks through the parent and confuses the line
    /// grouping in the UI). Filtering at the boundary means the user
    /// can never see a phantom Hammersmith & City group at Monument or
    /// a Bakerloo train at Belsize Park.
    ///
    /// **Hub-aware** by construction: `Station.lines` is already
    /// populated with the union across every hub child by
    /// `stop_points_cached` (which merges DLR / Elizabeth / Overground
    /// siblings at warm time), so we just project that field.
    ///
    /// **Fail-open if the cache is cold.** This method deliberately
    /// does NOT trigger a network fetch — if `stop-points/tube` hasn't
    /// been warmed yet we return an empty set, which the caller MUST
    /// treat as "skip filtering". Two reasons:
    /// (1) triggering a 16 MB fetch on every refresh would defeat the
    /// whole point of the TTL cache and burn TfL's rate limit; and
    /// (2) dropping a legitimate arrival because the cache hasn't
    /// warmed is much worse UX than letting through one phantom
    /// arrival until the next refresh. Production warms the cache
    /// once at startup (`lib.rs::run` calls `warm_stop_points_cache`
    /// after the first board emit), so the filter is active by the
    /// second tick at the latest.
    pub async fn allowed_line_ids_for(
        &self,
        station_id: &str,
    ) -> Result<std::collections::HashSet<String>, TflError> {
        // Uses `read_cache_any` (not `read_fresh_cache`) so a TTL-stale
        // cache entry still serves the per-station allowed line set.
        // Without this, the first stream tick after a 15-min TTL expiry
        // returns the empty set and the defensive filter would
        // (correctly) fail-open and skip itself — but at hub stations,
        // `resolve_arrival_ids` ALSO needs `read_cache_any`, and the
        // combination preserves the full multi-mode chip-filter
        // behaviour through the TTL boundary instead of dropping every
        // legitimate Overground/DLR arrival when a user has them
        // selected at a hub.
        let Some(stations) = self.read_cache_any() else {
            return Ok(std::collections::HashSet::new());
        };
        Ok(stations
            .iter()
            .find(|s| s.id == station_id)
            .map(|s| s.lines.iter().map(|l| l.id.clone()).collect())
            .unwrap_or_default())
    }

    /// Fetch the full per-mode stop-points lists, merge into a single
    /// `Vec<Station>`, serve from cache when fresh.
    ///
    /// Cache TTL is [`STOP_POINTS_TTL`]. On miss, fans out one
    /// `/StopPoint/Mode/{mode}` fetch per entry in `self.modes` in parallel
    /// via `futures::future::join_all` and merges the results — keyed by
    /// `Station.id`, with line-list union on collision (a tube hub like
    /// Stratford appears under multiple mode endpoints).
    ///
    /// Per-mode fetch failures are logged once per stale fixture and the
    /// mode is skipped — a missing or stale fixture must not poison the
    /// whole cache. The cache is still stamped on partial success so
    /// search results stay responsive while a transient mode is recovering.
    ///
    /// The lock is held only for a synchronous read/write around the Mutex —
    /// the network calls happen outside the critical section, so two concurrent
    /// callers may briefly both fetch on a cold cache. That is acceptable:
    /// the duplicate work is paid once per process start, not per keystroke.
    async fn stop_points_cached(&self) -> Result<Vec<Station>, TflError> {
        // Stale-while-revalidate: if anything is cached (fresh OR stale),
        // return it immediately — search must never block on a refresh
        // past the initial warm. The periodic background task in
        // `lib.rs::run` calls `refresh_stop_points_cache` before each TTL
        // boundary so the cache stays fresh out-of-band; if that misses
        // (laptop sleep, transient TfL outage) the user just sees
        // slightly older station metadata until the next periodic tick,
        // which is fine because TfL station metadata is stable for
        // months.
        if let Some(cached) = self.read_cache_any() {
            return Ok(cached);
        }

        // Cold cache (first call, never warmed). Block on a refresh.
        // `force = false` so a concurrent caller that already finished
        // refreshing while we were waiting on the lock short-circuits us.
        self.refresh_stop_points_inner(false).await
    }

    /// Single-flighted refresh: acquires the async lock, optionally
    /// short-circuits if a prior holder already produced fresh data,
    /// then runs the per-mode + hub fan-out and writes the result back
    /// into the cache. Used by both the cold-cache path in
    /// `stop_points_cached` (force=false) and the public
    /// `refresh_stop_points_cache` method (force=true).
    async fn refresh_stop_points_inner(&self, force: bool) -> Result<Vec<Station>, TflError> {
        // Single-flight: serialise concurrent refreshes so a burst of
        // debounced search keystrokes during a cold-cache window doesn't
        // each fire a full per-mode + hub fan-out. Acquire the async
        // refresh lock; once we have it, re-check the cache — a previous
        // holder may have just stamped it while we were waiting. The
        // periodic background refresh path passes `force = true` so it
        // always runs the fan-out even when the cache is already fresh
        // — that's what keeps "stale-while-revalidate" actually
        // revalidating.
        let _refresh_guard = self.stop_points_refresh.lock().await;
        if !force {
            if let Some(cached) = self.read_fresh_cache() {
                return Ok(cached);
            }
        }

        // Parallel fan-out across surfaced modes. Each per-mode fetch is
        // independent; failures are isolated.
        let fetches = self.modes.iter().map(|mode| async move {
            match self.http.fetch("stop-points", mode).await {
                Ok(value) => {
                    let arr = value.get("stopPoints").unwrap_or(&value).clone();
                    match serde_json::from_value::<Vec<Station>>(arr) {
                        Ok(s) => (Some(s), None),
                        Err(e) => {
                            eprintln!("[tfl-client] stop-points/{mode} parse failed: {e}");
                            (None, None)
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[tfl-client] stop-points/{mode} fetch failed: {e}");
                    // Surface the first error so an entirely-failed fan-out
                    // can propagate something meaningful.
                    (None, Some(e))
                }
            }
        });
        let per_mode = futures::future::join_all(fetches).await;

        // Merge by id (tube hubs like Stratford appear in multiple mode
        // feeds); union line lists when ids collide. First-seen wins for
        // metadata other than `lines` — modes/hubNaptanCode/lat/lon are
        // typically identical across mode feeds for a given canonical id.
        let mut by_id: HashMap<String, Station> = HashMap::new();
        let mut last_err: Option<TflError> = None;
        for (per, err) in per_mode {
            if let Some(err) = err {
                last_err = Some(err);
            }
            let Some(list) = per else { continue };
            for s in list {
                match by_id.get_mut(&s.id) {
                    Some(existing) => {
                        for line in s.lines {
                            if !existing.lines.iter().any(|l| l.id == line.id) {
                                existing.lines.push(line);
                            }
                        }
                        // Backfill hub_naptan_code if the first feed didn't carry it.
                        if existing.hub_naptan_code.is_none() && s.hub_naptan_code.is_some() {
                            existing.hub_naptan_code = s.hub_naptan_code;
                        }
                    }
                    None => {
                        by_id.insert(s.id.clone(), s);
                    }
                }
            }
        }

        if by_id.is_empty() {
            // Every mode failed to load — propagate the last underlying error
            // so the caller (warm task / search command) can log a meaningful
            // outage signal instead of a silent empty cache.
            return Err(last_err.unwrap_or_else(|| {
                TflError::NotFound("stop-points: all modes failed".to_string())
            }));
        }

        let mut stations: Vec<Station> = by_id.into_values().collect();

        // For multi-mode stations that carry a hub NaPTAN code, merge lines
        // from sibling stop-points (DLR, Elizabeth, Overground) so the
        // Settings chip UI shows all lines, not just the tube parent's lines.
        //
        // **Dedupe by hub_id before fan-out.** The naive `iter().enumerate()`
        // approach fires one fetch per station with a hub code — but a single
        // hub like `HUBKGX` is referenced by ~23 stations (multi-mode hubs
        // appear under tube + DLR + Elizabeth + Overground feeds). Without
        // deduping, that's 757 simultaneous TfL requests for 90 unique hubs
        // (8.4× redundancy). All 23 racers see an empty `hub_lines_cache` and
        // each fires its own HTTP request before any of them populate it.
        // Deduping first cuts the warm-time burst to one fetch per hub.
        let stations_per_hub: HashMap<String, Vec<usize>> = {
            let mut map: HashMap<String, Vec<usize>> = HashMap::new();
            for (i, s) in stations.iter().enumerate() {
                if let Some(hub_id) = &s.hub_naptan_code {
                    map.entry(hub_id.clone()).or_default().push(i);
                }
            }
            map
        };

        if !stations_per_hub.is_empty() {
            let hub_results = futures::future::join_all(stations_per_hub.iter().map(
                |(hub_id, indices)| async move {
                    let lines = self.hub_lines_cached(hub_id).await;
                    (indices.clone(), lines)
                },
            ))
            .await;

            for (indices, hub_lines) in hub_results {
                for i in indices {
                    let station = &mut stations[i];
                    for line in &hub_lines {
                        if !station.lines.iter().any(|l| l.id == line.id) {
                            station.lines.push(line.clone());
                        }
                    }
                }
            }
        }

        match self.stop_points_cache.lock() {
            Ok(mut guard) => {
                *guard = Some(StopPointsCacheEntry {
                    fetched_at: Instant::now(),
                    stations: stations.clone(),
                });
            }
            Err(poison) => {
                // A previous panic poisoned the mutex. Surface it so the bug
                // is observable rather than silently refetching 16 MB forever.
                eprintln!("[tfl-client] stop-points cache mutex poisoned; recovering: {poison}");
                let mut guard = poison.into_inner();
                *guard = Some(StopPointsCacheEntry {
                    fetched_at: Instant::now(),
                    stations: stations.clone(),
                });
            }
        }

        Ok(stations)
    }

    /// Returns the cached station list only if it's still within
    /// [`STOP_POINTS_TTL`]. Used by `stop_points_cached` to decide whether
    /// to refresh — stale entries return `None` so a refresh fires.
    fn read_fresh_cache(&self) -> Option<Vec<Station>> {
        let guard = match self.stop_points_cache.lock() {
            Ok(g) => g,
            Err(poison) => {
                eprintln!("[tfl-client] stop-points cache mutex poisoned on read; recovering");
                poison.into_inner()
            }
        };
        let entry = guard.as_ref()?;
        if entry.fetched_at.elapsed() < STOP_POINTS_TTL {
            Some(entry.stations.clone())
        } else {
            None
        }
    }

    /// Returns the cached station list whenever any cached data exists,
    /// regardless of TTL freshness. Used by `resolve_arrival_ids` and
    /// `allowed_line_ids_for` — both need a station's `hub_naptan_code`
    /// or `lines` field to continue working past the 15-min TTL boundary,
    /// otherwise the next stream tick after expiry loses hub-merge for
    /// arrivals (Bank/Euston/Whitechapel sibling fetch) and the
    /// defensive filter silently drops legitimate Overground/DLR
    /// arrivals at hub stations because their line ids fall out of the
    /// per-station allowed set.
    ///
    /// TfL station metadata changes infrequently (new stations are rare;
    /// the `lines` and `hubNaptanCode` fields are essentially stable for
    /// months), so serving "stale but usable" data here is safe. The
    /// next caller of `stop_points_cached` (typically `search_stations`
    /// or `warm_stop_points_cache`) will refresh on its own schedule.
    fn read_cache_any(&self) -> Option<Vec<Station>> {
        let guard = match self.stop_points_cache.lock() {
            Ok(g) => g,
            Err(poison) => {
                eprintln!("[tfl-client] stop-points cache mutex poisoned on read; recovering");
                poison.into_inner()
            }
        };
        let entry = guard.as_ref()?;
        Some(entry.stations.clone())
    }

    /// Drop any cached stop-points response. Test-only — production code
    /// relies on the TTL.
    #[cfg(test)]
    pub fn invalidate_stop_points_cache(&self) {
        if let Ok(mut guard) = self.stop_points_cache.lock() {
            *guard = None;
        }
    }

    /// Push the cached entry's `fetched_at` far enough into the past that
    /// `read_fresh_cache` will return `None` while `read_cache_any` still
    /// returns the entry. Used to test that stale-but-usable lookups
    /// (`resolve_arrival_ids`, `allowed_line_ids_for`) survive the TTL
    /// boundary. Test-only.
    #[cfg(test)]
    pub fn __test_force_stale_stop_points_cache(&self) -> Result<(), &'static str> {
        let mut guard = self
            .stop_points_cache
            .lock()
            .map_err(|_| "mutex poisoned")?;
        let entry = guard.as_mut().ok_or("cache empty — warm first")?;
        entry.fetched_at = Instant::now()
            .checked_sub(STOP_POINTS_TTL * 2)
            .ok_or("cannot subtract TTL from current Instant")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Assign a sort key (lower = more relevant) to a lowercased station name.
///
/// Tier 0: exact match.
/// Tier 1: starts with query.
/// Tier 2: contains query (catch-all, already pre-filtered upstream).
fn relevance_tier(name_lower: &str, query_lower: &str) -> u8 {
    if name_lower == query_lower {
        0
    } else if name_lower.starts_with(query_lower) {
        1
    } else {
        2
    }
}

/// Convert a TfL wire-format `TflLine` into a domain `LineStatus`.
///
/// Status entries are derived from `lineStatuses`.
/// Disruption text is assembled from unique, non-empty `reason` fields.
fn tfl_line_to_line_status(line: TflLine) -> LineStatus {
    let status: Vec<StatusEntry> = line
        .line_statuses
        .iter()
        .map(|s| StatusEntry {
            severity: s.status_severity,
            description: s.status_severity_description.clone(),
        })
        .collect();

    // Collect unique non-empty reason strings.
    let mut seen = std::collections::HashSet::new();
    let disruption_parts: Vec<String> = line
        .line_statuses
        .iter()
        .filter_map(|s| {
            let reason = s.reason.as_deref().unwrap_or("").trim().to_string();
            if reason.is_empty() || !seen.insert(reason.clone()) {
                None
            } else {
                Some(reason)
            }
        })
        .collect();

    let disruption_text = if disruption_parts.is_empty() {
        None
    } else {
        Some(disruption_parts.join(" | "))
    };

    LineStatus {
        line_id: line.id,
        status,
        disruption_text,
    }
}
