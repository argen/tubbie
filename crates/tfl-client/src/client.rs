//! Typed TfL API client.
//!
//! `TflClient<H>` is generic over any `TflHttp` implementation, enabling
//! fully offline, deterministic testing via `FixtureTflHttp` in M2, and
//! live network calls via `ReqwestTflHttp` in M3+.
//!
//! ## Design decisions
//!
//! ### `get_arrivals` — NotFound propagation
//! When the fixture (or live API) returns `TflError::NotFound`, we propagate
//! it directly. No re-wrapping; the `FixtureTflHttp` already includes the
//! path in the message and `ReqwestTflHttp` will include the station id.
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
//! tier. Capped at 20 results (autocomplete UX: dumping 1682 rows is worse
//! than useless). Only tube-mode stations are returned; empty query returns
//! empty (not all stations).

use crate::error::TflError;
use crate::http::TflHttp;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tfl_domain::types::{
    is_supported_line_id, pretty_line_name, Arrival, LineRef, LineStatus, Station, StatusEntry,
    TflLine,
};

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
    /// Create a new `TflClient` wrapping the given transport.
    pub fn new(http: H) -> Self {
        Self {
            http,
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
    async fn resolve_arrival_ids(&self, stop_point_id: &str) -> Vec<String> {
        let hub_id = self.read_fresh_cache().and_then(|stations| {
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
    /// Failures are silenced and return an empty `Vec` — a missing hub
    /// fixture or a transient 404 should not block the stop-points load.
    /// Result is cached per hub id for the process lifetime.
    async fn hub_lines_cached(&self, hub_id: &str) -> Vec<LineRef> {
        if let Ok(guard) = self.hub_lines_cache.lock() {
            if let Some(cached) = guard.get(hub_id) {
                return cached.clone();
            }
        }

        let value = match self.http.fetch("stop-point", hub_id).await {
            Ok(v) => v,
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

    /// Fetch the current status for a tube line.
    ///
    /// Issues one request for all tube lines (`line-status/tube`) and finds
    /// the entry matching `line_id`. This means a call for any line pays the
    /// same cost; caching across calls is M4's job (`BoardService`).
    ///
    /// # Disruption text strategy
    /// Non-empty, unique `reason` strings from all `lineStatuses` entries are
    /// joined with `" | "`. Good-service lines have no reasons, so
    /// `disruption_text` is `None`.
    ///
    /// # Errors
    /// - `TflError::NotFound` — `line_id` not found in the tube line list.
    /// - `TflError::Parse` — response is not a `Vec<TflLine>`.
    /// - `TflError::ParseAt` — fixture file is invalid JSON (offline only).
    /// - `TflError::Transport` — network failure (live client only).
    pub async fn get_line_status(&self, line_id: &str) -> Result<LineStatus, TflError> {
        // TODO: extend to `tube,dlr,overground,elizabeth-line` once the
        // `line-status` fixture is re-recorded with multi-mode data and
        // the path validator allows commas. For now the live API still
        // returns useful arrivals data for non-tube lines because the
        // arrivals endpoint is mode-agnostic; only the per-line status
        // ticker is tube-only.

        // Serve from the TTL cache when fresh. The entire line list is
        // fetched once per LINE_STATUS_TTL window; per-line lookups all
        // run against the cached Vec<TflLine> in memory.
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
            let value = self.http.fetch("line-status", "tube").await?;
            let fresh: Vec<TflLine> = serde_json::from_value(value)?;
            // Store in cache for the next call within the TTL window.
            match self.line_status_cache.lock() {
                Ok(mut guard) => {
                    *guard = Some((Instant::now(), fresh.clone()));
                }
                Err(poison) => {
                    eprintln!(
                        "[tfl-client] line_status_cache mutex poisoned on write; recovering"
                    );
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

        // Filter to tube-mode stations whose id is the canonical group parent
        // (TfL's `940GZZLU{CODE}` prefix). This drops:
        //   - Platform-level children `9400ZZLU*` — same common name, would
        //     produce duplicate rows in the dropdown.
        //   - NaPTAN bus-stop-at-station records `4900ZZLU*` — same location
        //     but no tube line info.
        //   - Hub stop-points `HUB*` — multi-mode aggregators mixing bus and
        //     national-rail services with no stable tube id for arrivals.
        // ~272 canonical entries remain (one per London Underground station),
        // which is the shape the user expects in the dropdown.
        let mut matches: Vec<Station> = stations
            .into_iter()
            .filter(|s| s.id.starts_with("940GZZLU"))
            .filter(|s| s.modes.iter().any(|m| m == "tube"))
            .filter(|s| s.common_name.to_lowercase().contains(&q))
            .collect();

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
    /// Idempotent: if the cache is already fresh, returns its size without a
    /// network round-trip. Returns the cached station count.
    pub async fn warm_stop_points_cache(&self) -> Result<usize, TflError> {
        let stations = self.stop_points_cached().await?;
        Ok(stations.len())
    }

    /// Fetch the full tube stop-points list, serving from cache when fresh.
    ///
    /// Cache TTL is [`STOP_POINTS_TTL`]. On miss, fetches `/StopPoint/Mode/tube`
    /// once and stores the deserialized `Vec<Station>` under a mutex.
    ///
    /// The lock is held only for a synchronous read/write around the Mutex —
    /// the network call happens outside the critical section, so two concurrent
    /// callers may briefly both fetch on a cold cache. That is acceptable:
    /// the duplicate work is paid once per process start, not per keystroke.
    async fn stop_points_cached(&self) -> Result<Vec<Station>, TflError> {
        if let Some(cached) = self.read_fresh_cache() {
            return Ok(cached);
        }

        let value = self.http.fetch("stop-points", "tube").await?;
        let stop_points_value = value.get("stopPoints").unwrap_or(&value).clone();
        let mut stations: Vec<Station> = serde_json::from_value(stop_points_value)?;

        // For multi-mode stations that carry a hub NaPTAN code, merge lines
        // from sibling stop-points (DLR, Elizabeth, Overground) so the
        // Settings chip UI shows all lines, not just the tube parent's lines.
        let hub_jobs: Vec<(usize, String)> = stations
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.hub_naptan_code.clone().map(|hub_id| (i, hub_id)))
            .collect();

        if !hub_jobs.is_empty() {
            let hub_results =
                futures::future::join_all(hub_jobs.iter().map(|(i, hub_id)| async move {
                    let lines = self.hub_lines_cached(hub_id).await;
                    (*i, lines)
                }))
                .await;

            for (i, hub_lines) in hub_results {
                let station = &mut stations[i];
                for line in hub_lines {
                    if !station.lines.iter().any(|l| l.id == line.id) {
                        station.lines.push(line);
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

    /// Drop any cached stop-points response. Test-only — production code
    /// relies on the TTL.
    #[cfg(test)]
    pub fn invalidate_stop_points_cache(&self) {
        if let Ok(mut guard) = self.stop_points_cache.lock() {
            *guard = None;
        }
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
