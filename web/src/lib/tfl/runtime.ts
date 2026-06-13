/**
 * TfL runtime — the composition root for the TypeScript data path.
 *
 * This is the one place the real transport, cache client, and board service are
 * wired together with live URLs. Everything it touches ({@link FetchTflHttp},
 * {@link TflClient}, {@link BoardService}, {@link fetchPoolKeys}) is unit-tested
 * in isolation; this module is glue, so consumers mock it at the boundary
 * (`vi.mock('../tfl/runtime.js')`) rather than exercising the live wiring.
 *
 * A single shared {@link TflClient} backs every caller — search, nearest,
 * status, and the board stream — so they share one stop-points cache, one hub
 * cache, and one process-wide 429 cooldown (Rust invariant #5,
 * `Arc<TflClient>`). The instance is built once, lazily, and memoized.
 *
 * Built behind the `USE_TS_TFL` flag (default off): nothing here runs, and no
 * network is touched, until a caller on the TS path first awaits
 * {@link tflRuntime}.
 */

import { FetchTflHttp } from './transport/tflHttp.js';
import { fetchPoolKeys, POOL_KEYS_URL } from './transport/poolKey.js';
import { TflClient } from './cache/tflClient.js';
import { BoardService } from './board/boardService.js';

/** Re-fan the stop-points cache just under its 15-minute TTL (Rust invariant #20). */
export const STOP_POINTS_REFRESH_INTERVAL_MS = 14 * 60 * 1000;

export interface TflRuntime {
  /** Shared cache client — search, nearest, arrivals, line status (#5). */
  readonly client: TflClient;
  /** Board pipeline wrapping the shared client; the stream's per-tick driver. */
  readonly service: BoardService;
}

let runtimePromise: Promise<TflRuntime> | null = null;
/** Owner of the periodic-refresh timer so it can be cleared (HMR / re-build). */
let refreshTimer: ReturnType<typeof setInterval> | null = null;

/**
 * The shared TfL runtime, built once and memoized. Safe to call concurrently:
 * the first call starts the build, every later call awaits the same promise.
 */
export function tflRuntime(): Promise<TflRuntime> {
  runtimePromise ??= buildRuntime();
  return runtimePromise;
}

async function buildRuntime(): Promise<TflRuntime> {
  // Defensive: never run two refresh timers at once (a re-build after an HMR
  // dispose resets `runtimePromise`).
  clearRefreshTimer();

  // Pool keys are public and fail-open: a null pool just means unauthenticated
  // requests (the anonymous TfL budget), never a hard failure at boot.
  const keyPool = await fetchPoolKeys(POOL_KEYS_URL);
  const http = new FetchTflHttp(keyPool !== null ? { keyPool } : {});
  const client = new TflClient(http);
  const service = new BoardService(client);

  // Warm the stop-points cache in the background so the first search / arrivals
  // call hits a populated cache, and keep it fresh on the same cadence as the
  // Rust periodic task (#20). Both are fire-and-forget — a failed warm or
  // refresh leaves the cache to heal on its own SWR schedule.
  void client.warmStopPointsCache().catch(() => undefined);
  refreshTimer = setInterval(() => {
    void client.refreshStopPointsCache().catch(() => undefined);
  }, STOP_POINTS_REFRESH_INTERVAL_MS);

  return { client, service };
}

function clearRefreshTimer(): void {
  if (refreshTimer !== null) {
    clearInterval(refreshTimer);
    refreshTimer = null;
  }
}

// Vite HMR: a module reload would otherwise orphan the refresh timer (which
// fires real network fans every 14 min) and strand the memoized singleton.
// Clear both on dispose so the next `tflRuntime()` rebuilds cleanly. Inert under
// vitest / production (`import.meta.hot` is undefined).
import.meta.hot?.dispose(() => {
  clearRefreshTimer();
  runtimePromise = null;
});
