/**
 * Hand-written TypeScript types that mirror the Rust domain structs
 * returned by Tauri IPC commands.
 *
 * Rule: no `any`. Every Tauri IPC boundary uses `unknown` + type guards.
 *
 * Rust source of truth:
 *   - crates/tfl-domain/src/types.rs
 *   - crates/tfl-board/src/config.rs
 */

// ---------------------------------------------------------------------------
// Primitive / shared
// ---------------------------------------------------------------------------

/** ISO-8601 date-time string as returned by serde's chrono::DateTime<Utc>. */
export type DateTimeUtc = string;

// ---------------------------------------------------------------------------
// Arrival
// ---------------------------------------------------------------------------

/** Direction enum — mirrors tfl_domain::Direction. */
export type Direction =
  | 'Northbound'
  | 'Southbound'
  | 'Eastbound'
  | 'Westbound'
  | 'Inbound'
  | 'Outbound'
  | 'Unknown';

/** A single train arrival prediction from TfL. */
export interface Arrival {
  id: string;
  station_name: string;
  platform_name: string;
  line_id: string;
  line_name: string;
  direction: Direction;
  destination_name: string;
  towards: string;
  current_location: string;
  time_to_station: number;
  expected_arrival: DateTimeUtc;
  naptan_id: string;
}

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

/** All arrivals for one named platform, sorted by time_to_station. */
export interface Platform {
  name: string;
  arrivals: Arrival[];
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

/**
 * A grouped arrivals board for a station, ready for rendering.
 * Emitted via `board://updated` Tauri event on each stream tick.
 */
export interface Board {
  station_id: string;
  platforms: Platform[];
  generated_at: DateTimeUtc;
  /** Set when the last API call failed and the board is showing stale data. */
  stale_since: DateTimeUtc | null;
}

// ---------------------------------------------------------------------------
// Station / LineRef
// ---------------------------------------------------------------------------

export interface LineRef {
  id: string;
  name: string;
}

export interface Station {
  id: string;
  common_name: string;
  modes: string[];
  lat: number;
  lon: number;
  lines: LineRef[];
}

// ---------------------------------------------------------------------------
// Favorite — saved station (separate `"favorites"` store key)
// ---------------------------------------------------------------------------

/**
 * A station saved as a favorite by the user. Mirrors `tfl_domain::Favorite`.
 * `lines` is snapshotted at save time so the Favorites list can render line
 * chips without requiring a hot station-cache lookup.
 */
export interface Favorite {
  station_id: string;
  common_name: string;
  lines: LineRef[];
}

// ---------------------------------------------------------------------------
// DisplayPrefs — desktop-only render preferences (separate `"display_prefs"`
// store key, NOT part of `BoardConfig`). Mirrors `crate::state::DisplayPrefs`
// in `src-tauri/`. See `feedback_ios_consumer.md` and the `display_mode`
// precedent for why this lives outside the shared `tfl-*` crates.
// ---------------------------------------------------------------------------

export interface DisplayPrefs {
  /**
   * When `true`, `PlatformColumn.svelte` collapses arrivals sharing a
   * `(destination_name, towards)` key into a single row with a comma-
   * separated minutes sequence (e.g. "Edgware · 2, 5, 9 min").
   *
   * Frontend-only display flag. Backend `apply_filters` MUST NOT see this.
   */
  group_destinations: boolean;
}

export function isDisplayPrefs(v: unknown): v is DisplayPrefs {
  if (!isRecord(v)) return false;
  return typeof v.group_destinations === 'boolean';
}

// ---------------------------------------------------------------------------
// LineStatus / StatusEntry
// ---------------------------------------------------------------------------

export interface StatusEntry {
  severity: number;
  description: string;
}

export interface LineStatus {
  line_id: string;
  status: StatusEntry[];
  disruption_text: string | null;
}

// ---------------------------------------------------------------------------
// BoardConfig
// ---------------------------------------------------------------------------

export interface BoardConfig {
  station_id: string;
  line_ids: string[];
  directions: Direction[];
  poll_seconds: number;
  /** Theme ID persisted alongside board config. Defaults to "classic-amber". */
  theme: string;
}

// ---------------------------------------------------------------------------
// Type guards
// ---------------------------------------------------------------------------

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

export function isBoard(v: unknown): v is Board {
  if (!isRecord(v)) return false;
  return (
    typeof v.station_id === 'string' &&
    Array.isArray(v.platforms) &&
    typeof v.generated_at === 'string' &&
    (v.stale_since === null || typeof v.stale_since === 'string')
  );
}

export function isStation(v: unknown): v is Station {
  if (!isRecord(v)) return false;
  return typeof v.id === 'string' && typeof v.common_name === 'string' && Array.isArray(v.modes);
}

export function isBoardConfig(v: unknown): v is BoardConfig {
  if (!isRecord(v)) return false;
  return (
    typeof v.station_id === 'string' &&
    Array.isArray(v.line_ids) &&
    Array.isArray(v.directions) &&
    typeof v.poll_seconds === 'number'
  );
}

export function isLineStatus(v: unknown): v is LineStatus {
  if (!isRecord(v)) return false;
  return (
    typeof v.line_id === 'string' &&
    Array.isArray(v.status) &&
    (v.disruption_text === null || typeof v.disruption_text === 'string')
  );
}

export function isFavorite(v: unknown): v is Favorite {
  if (!isRecord(v)) return false;
  return (
    typeof v.station_id === 'string' &&
    typeof v.common_name === 'string' &&
    Array.isArray(v.lines)
  );
}

// ---------------------------------------------------------------------------
// BoardErrorPayload — emitted by the Rust stream task on a fresh error streak
// when there is no last-ok board to fall back to. Without this, the renderer
// would have no idea that polling is failing.
// ---------------------------------------------------------------------------

export interface BoardErrorPayload {
  message: string;
}

export function isBoardErrorPayload(v: unknown): v is BoardErrorPayload {
  if (!isRecord(v)) return false;
  return typeof v.message === 'string';
}
