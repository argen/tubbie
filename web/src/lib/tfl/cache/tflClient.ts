/**
 * `TflClient` — the caching layer over a {@link TflHttp} transport. Port of
 * `crates/tfl-cache/src/cache.rs` (the stop-points / search / nearest surface;
 * arrivals and line-status land alongside it in a follow-up).
 *
 * It owns the merged multi-mode **stop-points cache** (one `Station[]` keyed by
 * id, with hub-line merge) and the per-process **hub-line cache**, and exposes
 * station search, nearest-station ranking, and the per-station allowed-line set
 * the board filter consumes.
 *
 * ## Why this is much smaller than the Rust original
 *
 * The behaviour and every invariant are preserved, but JS removes whole
 * categories of Rust ceremony:
 * - **No mutex / poison recovery.** JS is single-threaded; a synchronous cache
 *   read or write can't race, so the three `Mutex`/`RwLock` fields collapse to
 *   plain fields.
 * - **Single-flight is a stored `Promise`** (the idiomatic JS form) instead of
 *   an async lock plus a re-check — concurrent callers await the same fan-out.
 * - **One retry helper** ({@link TflClient.fetchJsonWithRetry}) replaces three
 *   copies of the hand-rolled backoff loop.
 * - **A `Clock` seam** replaces the `#[cfg(test)]` "force stale" hooks — tests
 *   advance a `FakeClock` instead of reaching into private cache state.
 */

import type { LineRef, NearbyStation, Station } from '$lib/ipc/types.js';
import type { Clock } from '../transport/clock.js';
import { SystemClock } from '../transport/clock.js';
import { TflError } from '../transport/tflError.js';
import type { TflHttp } from '../transport/tflHttp.js';
import { isSupportedLineId, prettyLineName } from '../domain/lines.js';
import { parseStation } from '../domain/station.js';
import { isRecord, rArray } from '../domain/raw.js';
import { rankNearest } from '../nearest.js';

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/**
 * TfL modes tubbie surfaces. `new TflClient(http)` defaults to this set; pass
 * `modes` to {@link TflClient} for a subset (e.g. a memory-constrained host).
 * Adding a mode needs a matching `fixtures/{stop-points,line-status}/{mode}.json`
 * and an entry in `isSupportedLineId`.
 */
export const SUPPORTED_MODES = ['tube', 'overground', 'dlr', 'elizabeth-line'] as const;

const SUPPORTED_MODE_SET: ReadonlySet<string> = new Set(SUPPORTED_MODES);

/**
 * Nominal freshness of a full warm. The cache layer itself serves full entries
 * stale-while-revalidate (it never expires them — see {@link TflClient.stopPointsCached}),
 * so this governs only the *external* periodic-refresh cadence the app wiring
 * runs (Phase 5), kept here as the single source of that number. Partial entries
 * use the shorter {@link PARTIAL_WARM_RETRY_AFTER_MS} window instead.
 */
export const STOP_POINTS_TTL_MS = 15 * 60 * 1000;

/**
 * How long a **partial-warm** entry (a mode or hub-detail fetch failed) is
 * served before the next call retries the failed part. Short enough to self-heal
 * within a search session, long enough to let a 429 cooldown clear. (#26)
 */
const PARTIAL_WARM_RETRY_AFTER_MS = 60 * 1000;

/**
 * Backoff between per-mode / hub fetch retries (ms). Four attempts total
 * (one initial + three waits); ~6.5 s worst case outlasts a typical 429
 * window without the user noticing. (#21)
 */
const STOP_POINTS_FETCH_BACKOFF_MS: readonly number[] = [500, 1500, 4500];

/**
 * Canonical multi-mode interchanges where the user MUST see every mode's lines
 * after hub-merge. `expectedLines` is a **superset** contract (warm must contain
 * each, may contain more). This list is the regression contract shared with the
 * `hub-vectors.json` fixture and the Rust `CANONICAL_MULTI_MODE_HUBS`; adding a
 * hub is one line here plus a matching scenario. Defends against the recurring
 * "Elizabeth missing at TCR / DLR missing at Bank" bug class.
 */
export const CANONICAL_MULTI_MODE_HUBS: readonly {
  stationId: string;
  expectedLines: readonly string[];
}[] = [
  { stationId: '940GZZLUTCR', expectedLines: ['central', 'northern', 'elizabeth'] },
  { stationId: '940GZZLUBNK', expectedLines: ['central', 'northern', 'waterloo-city', 'dlr'] },
  {
    stationId: '940GZZLULVT',
    expectedLines: ['central', 'circle', 'hammersmith-city', 'metropolitan', 'elizabeth', 'weaver'],
  },
  {
    stationId: '940GZZLUSTD',
    expectedLines: ['central', 'jubilee', 'dlr', 'elizabeth', 'mildmay'],
  },
  { stationId: '940GZZLUCYF', expectedLines: ['jubilee', 'dlr', 'elizabeth'] },
  {
    stationId: '940GZZLUWCL',
    expectedLines: ['district', 'hammersmith-city', 'elizabeth', 'mildmay', 'windrush'],
  },
  {
    stationId: '940GZZLUPAC',
    expectedLines: ['bakerloo', 'circle', 'district', 'hammersmith-city', 'elizabeth'],
  },
  {
    stationId: '940GZZLUFRD',
    expectedLines: ['circle', 'hammersmith-city', 'metropolitan', 'elizabeth'],
  },
  { stationId: '940GZZLUBND', expectedLines: ['central', 'jubilee', 'elizabeth'] },
];

// ---------------------------------------------------------------------------
// TflClient
// ---------------------------------------------------------------------------

interface StopPointsCacheEntry {
  fetchedAt: number;
  stations: Station[];
  /** Modes whose fetch failed during this warm. Non-empty → short retry window. */
  failedModes: string[];
  /** A hub-detail fetch failed transiently during this warm → short retry window. */
  hubWarmIncomplete: boolean;
}

export interface TflClientOptions {
  /** Modes to fan out across. Defaults to {@link SUPPORTED_MODES}. */
  modes?: readonly string[];
  /** Clock for cache freshness. Inject a `FakeClock` to test TTL behaviour. */
  clock?: Clock;
  /** Sleep between retries. Defaults to real `setTimeout`; tests pass a no-op. */
  sleep?: (ms: number) => Promise<void>;
}

export class TflClient {
  private readonly http: TflHttp;
  private readonly modes: readonly string[];
  private readonly clock: Clock;
  private readonly sleep: (ms: number) => Promise<void>;

  private cache: StopPointsCacheEntry | null = null;
  /** Single-flight gate: concurrent refreshes await this one fan-out. */
  private refreshInFlight: Promise<Station[]> | null = null;
  /** Per-process hub-id → merged lines (lazily filled; stable for the run). */
  private readonly hubLinesCache = new Map<string, LineRef[]>();

  constructor(http: TflHttp, opts: TflClientOptions = {}) {
    this.http = http;
    this.modes = opts.modes ?? SUPPORTED_MODES;
    this.clock = opts.clock ?? new SystemClock();
    this.sleep = opts.sleep ?? ((ms) => new Promise((r) => setTimeout(r, ms)));
  }

  // -------------------------------------------------------------------------
  // Public API
  // -------------------------------------------------------------------------

  /**
   * Search stations by name (case-insensitive substring), returning at most 20
   * ordered by relevance: exact → prefix → substring, then name. Applies the
   * canonical-prefix whitelist (#13) and hub dedupe (#18) so an interchange
   * shows one row, not one per mode. Empty query → `[]`.
   */
  async searchStations(query: string): Promise<Station[]> {
    const trimmed = query.trim();
    if (trimmed === '') return [];

    const q = trimmed.toLowerCase();
    const stations = await this.stopPointsCached();
    const prefiltered = stations
      .filter(isCanonicalStationId)
      .filter((s) => s.common_name.toLowerCase().includes(q));

    const matches = dedupeByHubNaptan(prefiltered);
    matches.sort((a, b) => {
      const tier =
        relevanceTier(a.common_name.toLowerCase(), q) -
        relevanceTier(b.common_name.toLowerCase(), q);
      return tier !== 0 ? tier : compareCodepoint(a.common_name, b.common_name);
    });
    return matches.slice(0, 20);
  }

  /**
   * Stations within {@link MAX_RADIUS_M} of `(lat, lon)`, nearest first, capped
   * at `limit`. Same whitelist + hub dedupe as {@link searchStations} so the
   * user never sees `Bank` and `Bank` stacked metres apart. Stale-OK (#20).
   */
  async findNearestStations(lat: number, lon: number, limit: number): Promise<NearbyStation[]> {
    const stations = await this.stopPointsCached();
    const candidates = dedupeByHubNaptan(stations.filter(isCanonicalStationId));
    return rankNearest(candidates, lat, lon, limit);
  }

  /**
   * The set of `line_id`s that legitimately serve `stationId`, projected from
   * the hub-merged `Station.lines`. Source of truth for the board's defensive
   * filter (#10). **Fail-open on a cold cache**: returns an empty set the caller
   * treats as "skip filtering" — never triggers a fetch, never drops a real
   * arrival just because the cache hasn't warmed. Reads stale data too (#19).
   */
  allowedLineIdsFor(stationId: string): Set<string> {
    const stations = this.readCacheAny();
    const station = stations?.find((s) => s.id === stationId);
    return new Set(station?.lines.map((l) => l.id) ?? []);
  }

  /** Pre-warm the cache (fire-and-forget at startup). Returns the station count. */
  async warmStopPointsCache(): Promise<number> {
    return (await this.stopPointsCached()).length;
  }

  /** Force a fan-out regardless of cache state (periodic background refresh). */
  async refreshStopPointsCache(): Promise<number> {
    return (await this.refreshStopPoints(true)).length;
  }

  /** Was the current entry a partial warm (a mode or hub fetch failed)? (#26) */
  stopPointsWarmIsPartial(): boolean {
    return (
      this.cache !== null && (this.cache.failedModes.length > 0 || this.cache.hubWarmIncomplete)
    );
  }

  // -------------------------------------------------------------------------
  // Stop-points cache (SWR + single-flight + partial-warm)
  // -------------------------------------------------------------------------

  /**
   * Serve the merged station list. Full entries are stale-while-revalidate
   * (returned regardless of TTL; the periodic task refreshes out-of-band, #20).
   * Partial entries serve for {@link PARTIAL_WARM_RETRY_AFTER_MS} only, then
   * fall through to a refresh so the failed mode is retried (#26). A cold cache
   * blocks on the fan-out.
   */
  private async stopPointsCached(): Promise<Station[]> {
    const entry = this.cache;
    if (entry !== null) {
      const partial = entry.failedModes.length > 0 || entry.hubWarmIncomplete;
      const stalePartial = partial && this.elapsed(entry.fetchedAt) >= PARTIAL_WARM_RETRY_AFTER_MS;
      if (!stalePartial) return entry.stations;
    }
    return this.refreshStopPoints(false);
  }

  /**
   * Single-flight refresh. Concurrent non-forced callers coalesce onto the
   * in-flight fan-out; a forced refresh chains after any in-flight one, then
   * runs its own. `startRefresh` sets {@link refreshInFlight} synchronously
   * before yielding, so two cold callers in the same tick share one fan-out.
   */
  private refreshStopPoints(force: boolean): Promise<Station[]> {
    if (this.refreshInFlight !== null) {
      if (!force) return this.refreshInFlight;
      return this.refreshInFlight.then(() => this.startRefresh());
    }
    return this.startRefresh();
  }

  private startRefresh(): Promise<Station[]> {
    const run = this.doRefresh().finally(() => {
      if (this.refreshInFlight === run) this.refreshInFlight = null;
    });
    this.refreshInFlight = run;
    return run;
  }

  /** Fan out per-mode + hub fetches, merge, stamp the cache, return stations. */
  private async doRefresh(): Promise<Station[]> {
    const perMode = await Promise.all(
      this.modes.map(async (mode) => ({ mode, ...(await this.fetchStopPointsForMode(mode)) })),
    );

    const byId = new Map<string, Station>();
    const failedModes: string[] = [];
    let lastErr: TflError | null = null;
    for (const { mode, stations, error } of perMode) {
      if (error !== undefined) {
        lastErr = error;
        failedModes.push(mode);
      }
      for (const s of stations ?? []) mergeStation(byId, s);
    }

    if (byId.size === 0) {
      throw lastErr ?? TflError.notFound('stop-points: all modes failed');
    }

    // Partial warm: a failed mode must not shrink the cache below what the user
    // already had — backfill missing stations from the prior entry. (#26)
    if (failedModes.length > 0) {
      for (const prior of this.readCacheAny() ?? []) {
        if (!byId.has(prior.id)) byId.set(prior.id, prior);
      }
    }

    const stations = [...byId.values()];
    const hubWarmIncomplete = await this.mergeHubLines(stations);

    this.cache = {
      fetchedAt: this.clock.now().getTime(),
      stations,
      failedModes,
      hubWarmIncomplete,
    };
    warnIncompleteHubCoverage(stations);
    return stations;
  }

  /** Fetch one mode's stop-points with bounded retries; parse into stations. */
  private async fetchStopPointsForMode(
    mode: string,
  ): Promise<{ stations?: Station[]; error?: TflError }> {
    let value: unknown;
    try {
      value = await this.fetchJsonWithRetry('stop-points', mode);
    } catch (e) {
      return { error: asTflError(e) };
    }
    // TfL wraps the list in `{ stopPoints: [...] }`; some fixtures are bare arrays.
    const body = isRecord(value) ? (value.stopPoints ?? value) : value;
    if (!Array.isArray(body)) {
      console.warn(`[tfl-cache] stop-points/${mode}: response was not an array`);
      // Parse failure is not a retryable mode failure — contribute nothing.
      return {};
    }
    return { stations: body.map(parseStation) };
  }

  /**
   * Merge sibling-stop-point lines into each hub station's `lines` so the chip
   * UI shows DLR / Elizabeth / Overground alongside tube. Dedupes the fan-out by
   * hub id first (#17). Returns whether any hub fetch failed transiently (#26).
   * Mutates `stations` in place.
   */
  private async mergeHubLines(stations: Station[]): Promise<boolean> {
    const indicesByHub = new Map<string, number[]>();
    stations.forEach((s, i) => {
      const hub = s.hub_naptan_code;
      if (hub === undefined) return;
      const list = indicesByHub.get(hub) ?? [];
      list.push(i);
      indicesByHub.set(hub, list);
    });
    if (indicesByHub.size === 0) return false;

    const results = await Promise.all(
      [...indicesByHub].map(async ([hubId, indices]) => ({
        indices,
        ...(await this.hubLinesCached(hubId)),
      })),
    );

    let hubWarmIncomplete = false;
    for (const { indices, lines, transientFailure } of results) {
      if (transientFailure) hubWarmIncomplete = true;
      for (const i of indices) {
        const station = stations[i];
        if (station === undefined) continue;
        for (const line of lines) {
          if (!station.lines.some((l) => l.id === line.id)) station.lines.push(line);
        }
      }
    }
    return hubWarmIncomplete;
  }

  /**
   * The lines served by a hub's children, cached for the process lifetime. A
   * genuinely-absent hub (`NotFound`) is cached as empty so we don't refetch a
   * known-404 hub every warm (#15); a transient-exhausted fetch is NOT cached
   * and reports `transientFailure` so the warm is marked partial (#26).
   */
  private async hubLinesCached(
    hubId: string,
  ): Promise<{ lines: LineRef[]; transientFailure: boolean }> {
    const cached = this.hubLinesCache.get(hubId);
    if (cached !== undefined) return { lines: cached, transientFailure: false };

    let value: unknown;
    try {
      value = await this.fetchJsonWithRetry('stop-point', hubId);
    } catch (e) {
      const err = asTflError(e);
      if (err.kind === 'NotFound') {
        this.hubLinesCache.set(hubId, []); // cache known-missing hub
        return { lines: [], transientFailure: false };
      }
      // Parse → bad data, retrying won't help, don't cache. Anything else is a
      // transient exhaustion → signal partial so the short retry window applies.
      return { lines: [], transientFailure: err.kind !== 'Parse' };
    }

    const lines = extractHubLines(value);
    this.hubLinesCache.set(hubId, lines);
    return { lines, transientFailure: false };
  }

  /**
   * Fetch raw JSON with bounded retries on transient errors. Terminal errors
   * (`NotFound` / `Parse`) throw immediately; transient ones (`RateLimited` /
   * `Transport` / `Http`) retry on the {@link STOP_POINTS_FETCH_BACKOFF_MS}
   * schedule, then throw the last error. (Shared by the per-mode and hub paths
   * — the single copy the Rust original spreads across three call sites.)
   */
  private async fetchJsonWithRetry(endpoint: string, id: string): Promise<unknown> {
    let lastErr: TflError | null = null;
    for (let attempt = 0; attempt <= STOP_POINTS_FETCH_BACKOFF_MS.length; attempt++) {
      try {
        return await this.http.fetch(endpoint, id);
      } catch (e) {
        const err = asTflError(e);
        if (err.kind === 'NotFound' || err.kind === 'Parse') throw err;
        lastErr = err;
        const backoff = STOP_POINTS_FETCH_BACKOFF_MS[attempt];
        if (backoff !== undefined) await this.sleep(backoff);
      }
    }
    throw lastErr ?? TflError.notFound(`${endpoint}/${id}: all attempts failed`);
  }

  /** Cached stations regardless of TTL — for stale-OK hub/line lookups (#19). */
  private readCacheAny(): Station[] | null {
    return this.cache?.stations ?? null;
  }

  private elapsed(fetchedAt: number): number {
    return this.clock.now().getTime() - fetchedAt;
  }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/** Merge a station into the by-id map: union lines, fill an absent hub code. */
function mergeStation(byId: Map<string, Station>, s: Station): void {
  const existing = byId.get(s.id);
  if (existing === undefined) {
    byId.set(s.id, s);
    return;
  }
  for (const line of s.lines) {
    if (!existing.lines.some((l) => l.id === line.id)) existing.lines.push(line);
  }
  if (existing.hub_naptan_code === undefined && s.hub_naptan_code !== undefined) {
    existing.hub_naptan_code = s.hub_naptan_code;
  }
}

/** Does a hub child serve a mode we surface? Gates the line projection. */
function childServesSupportedMode(child: Record<string, unknown>): boolean {
  return rArray(child, 'modes').some((m) => typeof m === 'string' && SUPPORTED_MODE_SET.has(m));
}

/**
 * Project a hub-detail JSON (`children[].lineModeGroups[].lineIdentifier`) into
 * the deduped, supported `LineRef`s the hub serves. Groups with an absent/empty
 * `modeName` are accepted (our trimmed fixtures drop the field); bus / rail
 * groups are filtered out by mode then by `isSupportedLineId`.
 */
function extractHubLines(value: unknown): LineRef[] {
  if (!isRecord(value)) return [];
  const seen = new Set<string>();
  const out: LineRef[] = [];
  for (const child of rArray(value, 'children')) {
    if (!isRecord(child) || !childServesSupportedMode(child)) continue;
    for (const group of rArray(child, 'lineModeGroups')) {
      if (!isRecord(group)) continue;
      const mode = typeof group.modeName === 'string' ? group.modeName : '';
      if (mode.length > 0 && !SUPPORTED_MODE_SET.has(mode)) continue;
      for (const id of rArray(group, 'lineIdentifier')) {
        if (typeof id !== 'string' || !isSupportedLineId(id) || seen.has(id)) continue;
        seen.add(id);
        out.push({ id, name: prettyLineName(id) });
      }
    }
  }
  return out;
}

/**
 * True iff `s` carries a canonical NaPTAN prefix we surface: `940GZZLU` (tube)
 * or `940GZZDL` (DLR) unconditionally, or `910G` (National Rail group) only when
 * its modes include `overground` or `elizabeth-line`. (#13)
 */
export function isCanonicalStationId(s: Station): boolean {
  if (s.id.startsWith('940GZZLU') || s.id.startsWith('940GZZDL')) return true;
  if (s.id.startsWith('910G')) {
    return s.modes.some((m) => m === 'overground' || m === 'elizabeth-line');
  }
  return false;
}

/**
 * Collapse stations sharing a `hub_naptan_code` to one per hub, preferring
 * tube (`940GZZLU`) > DLR (`940GZZDL`) > Overground/Elizabeth (`910G`).
 * Hubless stations are never deduped. (#18)
 */
export function dedupeByHubNaptan(stations: Station[]): Station[] {
  const prefixPriority = (id: string): number =>
    id.startsWith('940GZZLU') ? 0 : id.startsWith('940GZZDL') ? 1 : 2;

  const byHub = new Map<string, Station>();
  const withoutHub: Station[] = [];
  for (const s of stations) {
    const hub = s.hub_naptan_code;
    if (hub === undefined) {
      withoutHub.push(s);
      continue;
    }
    const existing = byHub.get(hub);
    if (existing === undefined || prefixPriority(s.id) < prefixPriority(existing.id)) {
      byHub.set(hub, s);
    }
  }
  return [...byHub.values(), ...withoutHub];
}

/** Relevance tier for a lowercased name vs query: 0 exact, 1 prefix, 2 substring. */
function relevanceTier(nameLower: string, queryLower: string): number {
  if (nameLower === queryLower) return 0;
  if (nameLower.startsWith(queryLower)) return 1;
  return 2;
}

/** Codepoint (byte-order) string compare, matching Rust `String::cmp`. */
function compareCodepoint(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Narrow an unknown thrown value to a {@link TflError}. */
function asTflError(e: unknown): TflError {
  return e instanceof TflError ? e : TflError.transport('unexpected error', '(no url)');
}

/**
 * Warn (once per missing line) when a freshly-warmed station list doesn't cover
 * a {@link CANONICAL_MULTI_MODE_HUBS} contract — the only signal of a hub-merge
 * regression or upstream TfL data drift.
 */
export function warnIncompleteHubCoverage(stations: Station[]): void {
  for (const { stationId, expectedLines } of CANONICAL_MULTI_MODE_HUBS) {
    const station = stations.find((s) => s.id === stationId);
    if (station === undefined) {
      console.warn(`[tfl-cache] canonical multi-mode hub ${stationId} absent from warm result`);
      continue;
    }
    for (const expected of expectedLines) {
      if (!station.lines.some((l) => l.id === expected)) {
        console.warn(
          `[tfl-cache] expected line \`${expected}\` missing from \`${stationId}\` after warm`,
        );
      }
    }
  }
}
