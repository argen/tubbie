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
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tfl_domain::types::{Arrival, LineStatus, Station, StatusEntry, TflLine};

/// How long a `stop-points/tube` response stays cached before the next
/// `search_stations` call refetches. 15 minutes keeps the 16 MB payload off
/// the wire for typical settings-page sessions while still picking up TfL
/// station-metadata edits within a lunchbreak.
const STOP_POINTS_TTL: Duration = Duration::from_secs(15 * 60);

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
}

impl<H: TflHttp> TflClient<H> {
    /// Create a new `TflClient` wrapping the given transport.
    pub fn new(http: H) -> Self {
        Self {
            http,
            stop_points_cache: Mutex::new(None),
        }
    }

    /// Fetch the live arrival predictions for a stop point.
    ///
    /// Returns a `Vec<Arrival>` in TfL's natural order (typically sorted by
    /// `timeToStation` ascending, but not guaranteed by the API).
    ///
    /// # Errors
    /// - `TflError::NotFound` — unknown station id or missing fixture.
    /// - `TflError::Parse` — response is valid JSON but not a `Vec<Arrival>`.
    /// - `TflError::ParseAt` — fixture file is invalid JSON (offline only).
    /// - `TflError::Transport` — network failure (live client only).
    pub async fn get_arrivals(&self, stop_point_id: &str) -> Result<Vec<Arrival>, TflError> {
        let value = self.http.fetch("arrivals", stop_point_id).await?;
        let arrivals: Vec<Arrival> = serde_json::from_value(value)?;
        Ok(arrivals)
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
        let value = self.http.fetch("line-status", "tube").await?;
        let lines: Vec<TflLine> = serde_json::from_value(value)?;

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

        // Filter to tube-mode stations that contain the query substring.
        let mut matches: Vec<Station> = stations
            .into_iter()
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
        let stations: Vec<Station> = serde_json::from_value(stop_points_value)?;

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
