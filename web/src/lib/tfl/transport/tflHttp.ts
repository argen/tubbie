/**
 * Browser transport for the TfL API — port of
 * `tfl_client::http::{TflHttp, ReqwestTflHttp}`.
 *
 * `FetchTflHttp` is the live implementation backed by the platform `fetch`. It
 * reproduces the Rust client's behaviour exactly where it matters:
 *
 * - **Endpoint → path map** and `app_key` query-param auth ({@link buildUrl}).
 * - **Retry loop**: up to `MAX_RETRIES` (2) extra attempts on 429 / 503 /
 *   network-or-timeout, exponential backoff `base·factor^n` capped at `max`.
 * - **429 handling**: honour `Retry-After` (capped); a value beyond the cap
 *   fails fast and arms a process-wide cooldown gate so concurrent callers
 *   don't pile in during the rate-limit window (Rust invariant #5).
 * - **Redaction**: the URL (hence `app_key`) never appears in a thrown error;
 *   see {@link TflError} / {@link sanitizeUrl}.
 *
 * Differences from Rust, all benign in a browser: no `User-Agent` header (the
 * platform forbids setting it from `fetch`); the connection pool is the
 * browser's, not reqwest's. The `Clock` injection and configurable backoff
 * constants exist so tests stay deterministic without real waits.
 */

import type { Clock } from './clock.js';
import { SystemClock } from './clock.js';
import type { KeyPool } from './poolKey.js';
import { TflError, sanitizeUrl, truncateTo512 } from './tflError.js';

// ---------------------------------------------------------------------------
// Constants (production defaults; overridable per-instance for tests)
// ---------------------------------------------------------------------------

export const BASE_URL = 'https://api.tfl.gov.uk/';
const DEFAULT_TIMEOUT_MS = 10_000;
const MAX_RETRIES = 2;
const BACKOFF_BASE_MS = 500;
const BACKOFF_FACTOR = 2;
const BACKOFF_MAX_MS = 2_000;
const RETRY_AFTER_CAP_SECS = 5;

// ---------------------------------------------------------------------------
// TflHttp interface
// ---------------------------------------------------------------------------

/**
 * Transport-only seam for fetching TfL JSON. `endpoint` is a logical name like
 * `"arrivals"` / `"stop-points"`; `id` is a resource id like `"940GZZLUBNK"`
 * or a mode like `"tube"`. Returns the parsed JSON as `unknown` — the domain
 * parsers narrow it. Rejects with a {@link TflError}.
 */
export interface TflHttp {
  fetch(endpoint: string, id: string): Promise<unknown>;
}

// ---------------------------------------------------------------------------
// URL construction
// ---------------------------------------------------------------------------

/**
 * Percent-encode a path segment, matching Rust `http::percent_encode`: keep the
 * unreserved set `A-Za-z0-9-_.~`, and `%XX`-encode every other character's
 * UTF-8 bytes. In practice TfL ids are alphanumeric, so this is belt-and-braces
 * against scheme/path injection from an unexpected input.
 */
export function percentEncode(s: string): string {
  let out = '';
  for (const ch of s) {
    if (/[A-Za-z0-9\-_.~]/.test(ch)) {
      out += ch;
    } else {
      for (const byte of new TextEncoder().encode(ch)) {
        out += `%${byte.toString(16).toUpperCase().padStart(2, '0')}`;
      }
    }
  }
  return out;
}

/**
 * Build the full request URL for an `(endpoint, id)` pair, mirroring the Rust
 * `build_url` mapping:
 * - `arrivals`    → `/StopPoint/{id}/Arrivals`
 * - `line-status` → `/Line/Mode/{id}/Status`
 * - `stop-points` → `/StopPoint/Mode/{id}`
 * - `stop-point`  → `/StopPoint/{id}` (hub detail)
 * - anything else → `/{endpoint}/{id}`
 *
 * `id` (and an unknown `endpoint`) are percent-encoded. When `appKey` is
 * supplied it is appended as an `app_key` query parameter (simple GET — no CORS
 * preflight).
 */
export function buildUrl(
  endpoint: string,
  id: string,
  opts: { baseUrl?: string; appKey?: string | undefined } = {},
): string {
  const base = opts.baseUrl ?? BASE_URL;
  const eid = percentEncode(id);
  let path: string;
  switch (endpoint) {
    case 'arrivals':
      path = `StopPoint/${eid}/Arrivals`;
      break;
    case 'line-status':
      path = `Line/Mode/${eid}/Status`;
      break;
    case 'stop-points':
      path = `StopPoint/Mode/${eid}`;
      break;
    case 'stop-point':
      path = `StopPoint/${eid}`;
      break;
    default:
      path = `${percentEncode(endpoint)}/${eid}`;
      break;
  }
  const url = new URL(path, base);
  if (opts.appKey !== undefined) {
    url.searchParams.append('app_key', opts.appKey);
  }
  return url.toString();
}

// ---------------------------------------------------------------------------
// Retry-After parsing
// ---------------------------------------------------------------------------

/**
 * Parse a `Retry-After` header into milliseconds. RFC 7231 allows two forms:
 * a non-negative integer (seconds) or an HTTP-date. For the date form the
 * delay is `date - now`, clamped to 0 if in the past. Returns `null` when
 * neither form parses. `now` is injected for deterministic tests.
 */
export function parseRetryAfter(header: string, now: Date): number | null {
  const h = header.trim();
  if (h === '') return null;
  if (/^\d+$/.test(h)) {
    return Number(h) * 1000;
  }
  const target = new Date(h);
  if (Number.isNaN(target.getTime())) return null;
  const delta = target.getTime() - now.getTime();
  return delta <= 0 ? 0 : delta;
}

/**
 * Backoff for retry attempt `attempt` (0-indexed). An explicit server delay
 * (`serverAfterMs`, already pre-filtered to ≤ cap at the call site) is honoured
 * verbatim; otherwise exponential `base·factor^attempt` capped at `max`.
 */
export function computeBackoff(
  attempt: number,
  serverAfterMs: number | null,
  cfg: { baseMs: number; factor: number; maxMs: number },
): number {
  if (serverAfterMs !== null) return serverAfterMs;
  return Math.min(cfg.baseMs * cfg.factor ** attempt, cfg.maxMs);
}

// ---------------------------------------------------------------------------
// FetchTflHttp
// ---------------------------------------------------------------------------

export interface FetchTflHttpOptions {
  /** Base URL; defaults to the live TfL API. Override to point at a mock. */
  baseUrl?: string;
  /** Static `app_key`. Ignored when `keyPool` is set (the pool wins). */
  appKey?: string;
  /** When set, a fresh key is round-robin-picked per `fetch` call. */
  keyPool?: KeyPool;
  /** Injectable `fetch` for tests. Defaults to the platform `fetch`. */
  fetchImpl?: typeof fetch;
  /** Injectable clock (cooldown math + `Retry-After` date arithmetic). */
  clock?: Clock;
  /** Per-request timeout in ms (default 10 000). */
  timeoutMs?: number;
  /** Max extra retries after the first attempt (default 2). */
  maxRetries?: number;
  /** Exponential-backoff base in ms (default 500). Lower it in tests. */
  backoffBaseMs?: number;
  /** Exponential-backoff cap in ms (default 2000). */
  backoffMaxMs?: number;
}

/** One HTTP attempt's outcome, fed back to the retry loop. */
type Attempt =
  | { ok: true; value: unknown }
  | { ok: false; retry: true; afterMs: number | null; err: TflError }
  | { ok: false; retry: false; err: TflError };

export class FetchTflHttp implements TflHttp {
  private readonly baseUrl: string;
  /**
   * The static `app_key`. The Rust `AppKey` wrapper zeroizes on drop and
   * redacts its `Debug`; neither is achievable for a GC'd JS string, so the
   * defence here is by discipline instead: this field is only ever appended to
   * a local request URL ({@link buildUrl}) and is never logged, thrown, or
   * interpolated into a {@link TflError} message. Keep it that way.
   */
  private readonly appKey: string | undefined;
  private readonly keyPool: KeyPool | undefined;
  private readonly fetchImpl: typeof fetch;
  private readonly clock: Clock;
  private readonly timeoutMs: number;
  private readonly maxRetries: number;
  private readonly backoffBaseMs: number;
  private readonly backoffMaxMs: number;

  /**
   * Process-wide 429 cooldown gate as an epoch-ms deadline (`null` = open).
   * Set when a 429 carries a `Retry-After` beyond the cap; the next `fetch`
   * sleeps until it clears before touching the wire. Lives on the instance,
   * which is a shared singleton in production (Rust invariant #5).
   */
  private cooldownUntil: number | null = null;

  constructor(opts: FetchTflHttpOptions = {}) {
    this.baseUrl = opts.baseUrl ?? BASE_URL;
    this.appKey = opts.appKey;
    this.keyPool = opts.keyPool;
    this.fetchImpl = opts.fetchImpl ?? ((input, init) => fetch(input, init));
    this.clock = opts.clock ?? new SystemClock();
    this.timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    this.maxRetries = opts.maxRetries ?? MAX_RETRIES;
    this.backoffBaseMs = opts.backoffBaseMs ?? BACKOFF_BASE_MS;
    this.backoffMaxMs = opts.backoffMaxMs ?? BACKOFF_MAX_MS;
  }

  async fetch(endpoint: string, id: string): Promise<unknown> {
    await this.waitForCooldown();

    // Resolve the key once per fetch (Rust builds the URL once, before retries),
    // so a pooled key advances the round-robin cursor exactly once per call.
    const appKey = this.keyPool !== undefined ? this.keyPool.pick().key : this.appKey;
    const url = buildUrl(endpoint, id, { baseUrl: this.baseUrl, appKey });

    let lastErr: TflError | null = null;
    for (let attempt = 0; attempt <= this.maxRetries; attempt++) {
      const outcome = await this.doRequest(url);
      if (outcome.ok) return outcome.value;
      if (outcome.retry) {
        lastErr = outcome.err;
        if (attempt < this.maxRetries) {
          await sleep(
            computeBackoff(attempt, outcome.afterMs, {
              baseMs: this.backoffBaseMs,
              factor: BACKOFF_FACTOR,
              maxMs: this.backoffMaxMs,
            }),
          );
        }
      } else {
        throw outcome.err;
      }
    }
    throw lastErr ?? TflError.transport('retry loop exhausted', sanitizeUrl(url));
  }

  /** Sleep out a live cooldown gate before issuing a request, then clear it. */
  private async waitForCooldown(): Promise<void> {
    if (this.cooldownUntil === null) return;
    const remaining = this.cooldownUntil - this.clock.now().getTime();
    if (remaining > 0) {
      await sleep(remaining);
    }
    // Always clear once observed: a still-live gate has now been slept past, and
    // an already-expired one was a no-op — either way it must not linger and
    // make every future call re-check a stale deadline.
    this.cooldownUntil = null;
  }

  /** Perform one GET, mapping the response (or throw) to an {@link Attempt}. */
  private async doRequest(url: string): Promise<Attempt> {
    const controller = new AbortController();
    const timer = setTimeout(() => {
      controller.abort();
    }, this.timeoutMs);

    let response: Response;
    try {
      response = await this.fetchImpl(url, { signal: controller.signal });
    } catch (e) {
      // Network failure or abort (timeout) → retry, like Rust connect/timeout.
      return {
        ok: false,
        retry: true,
        afterMs: null,
        err: TflError.transport(describeFetchError(e), sanitizeUrl(url)),
      };
    } finally {
      clearTimeout(timer);
    }

    const status = response.status;

    if (status >= 200 && status < 300) {
      try {
        const value: unknown = await response.json();
        return { ok: true, value };
      } catch {
        return {
          ok: false,
          retry: false,
          err: TflError.transport('response decode error', sanitizeUrl(url)),
        };
      }
    }

    if (status === 404) {
      return {
        ok: false,
        retry: false,
        err: TflError.notFound(`TfL returned 404 for ${safePathname(url)}`),
      };
    }

    if (status === 429) {
      const header = response.headers.get('retry-after');
      const afterMs = header !== null ? parseRetryAfter(header, this.clock.now()) : null;
      // Retry-After beyond the cap: give up now and arm the cooldown gate so
      // concurrent callers don't hammer TfL during the window. Compare on
      // whole seconds — Rust truncates via `Duration::as_secs()` before the
      // `> RETRY_AFTER_CAP_SECS` check, so a sub-second-over value (e.g. 5500ms
      // from an HTTP-date against a clock with millis) must still retry, not
      // fail fast.
      if (afterMs !== null && Math.floor(afterMs / 1000) > RETRY_AFTER_CAP_SECS) {
        this.cooldownUntil = this.clock.now().getTime() + afterMs;
        return { ok: false, retry: false, err: TflError.rateLimited(afterMs) };
      }
      return { ok: false, retry: true, afterMs, err: TflError.rateLimited(afterMs) };
    }

    if (status === 503) {
      return {
        ok: false,
        retry: true,
        afterMs: null,
        err: TflError.http(503, await bodySnippet(response)),
      };
    }

    return {
      ok: false,
      retry: false,
      err: TflError.http(status, await bodySnippet(response)),
    };
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** A short, URL-free description of a thrown `fetch` error. */
function describeFetchError(e: unknown): string {
  if (e instanceof Error && e.name === 'AbortError') return 'timeout';
  return 'connection failed';
}

/** The pathname of a URL for error context, or `?` if it won't parse. */
function safePathname(url: string): string {
  try {
    return new URL(url).pathname;
  } catch {
    return '?';
  }
}

/** Read ≤512 chars of a response body for error context (URL never included). */
async function bodySnippet(response: Response): Promise<string> {
  try {
    return truncateTo512(await response.text());
  } catch {
    return '(could not read body)';
  }
}
