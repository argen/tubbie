/**
 * Ports `crates/tfl-client/tests/http_retry.rs` and the unit-level URL /
 * backoff / Retry-After tests from `http.rs`, driving a mocked `fetch`:
 *
 * - retry counts (429 / 503 retried up to MAX_RETRIES; 500 / 404 not retried)
 * - Retry-After beyond the cap → immediate fail + process-wide cooldown gate
 * - app_key never appears in a thrown error's message
 * - app_key (static or pooled) reaches the request as a query parameter
 */

import { describe, expect, it, vi } from 'vitest';
import { FakeClock } from './clock.js';
import { KeyPool } from './poolKey.js';
import { TflError } from './tflError.js';
import {
  FetchTflHttp,
  buildUrl,
  computeBackoff,
  parseRetryAfter,
  percentEncode,
} from './tflHttp.js';

const NOW = new Date('2026-04-24T12:00:00Z');

function resp(
  status: number,
  opts: { body?: unknown; text?: string; headers?: Record<string, string> } = {},
): Response {
  const h = new Map(
    Object.entries(opts.headers ?? {}).map(([k, v]) => [k.toLowerCase(), v] as const),
  );
  return {
    status,
    ok: status >= 200 && status < 300,
    headers: { get: (n: string) => h.get(n.toLowerCase()) ?? null },
    json: () => Promise.resolve(opts.body ?? {}),
    text: () =>
      Promise.resolve(opts.text ?? (opts.body !== undefined ? JSON.stringify(opts.body) : '')),
  } as unknown as Response;
}

/** A client wired to a mock, with negligible backoff so 503 retries are fast. */
function client(fetchMock: ReturnType<typeof vi.fn>): FetchTflHttp {
  return new FetchTflHttp({
    baseUrl: 'http://mock/',
    fetchImpl: fetchMock as unknown as typeof fetch,
    clock: FakeClock.at(NOW),
    backoffBaseMs: 1,
    backoffMaxMs: 1,
  });
}

// ---------------------------------------------------------------------------
// percentEncode
// ---------------------------------------------------------------------------

describe('percentEncode', () => {
  it('leaves the unreserved set untouched', () => {
    expect(percentEncode('940GZZLUBNK-_.~')).toBe('940GZZLUBNK-_.~');
  });

  it('encodes spaces and slashes as their UTF-8 bytes', () => {
    expect(percentEncode('a b')).toBe('a%20b');
    expect(percentEncode('a/b')).toBe('a%2Fb');
  });
});

// ---------------------------------------------------------------------------
// buildUrl
// ---------------------------------------------------------------------------

describe('buildUrl', () => {
  it('maps each endpoint to the TfL path', () => {
    expect(buildUrl('arrivals', '940GZZLUBZP')).toBe(
      'https://api.tfl.gov.uk/StopPoint/940GZZLUBZP/Arrivals',
    );
    expect(buildUrl('line-status', 'tube')).toBe('https://api.tfl.gov.uk/Line/Mode/tube/Status');
    expect(buildUrl('stop-points', 'tube')).toBe('https://api.tfl.gov.uk/StopPoint/Mode/tube');
    expect(buildUrl('stop-point', 'HUBKGX')).toBe('https://api.tfl.gov.uk/StopPoint/HUBKGX');
  });

  it('appends app_key as a query parameter when present', () => {
    expect(buildUrl('arrivals', 'X', { appKey: 'MYKEY' })).toContain('app_key=MYKEY');
  });

  it('omits app_key for an anonymous request', () => {
    expect(buildUrl('arrivals', 'X')).not.toContain('app_key');
  });

  it('percent-encodes the id', () => {
    expect(buildUrl('arrivals', 'a b', { baseUrl: 'http://mock/' })).toContain('a%20b');
  });
});

// ---------------------------------------------------------------------------
// parseRetryAfter
// ---------------------------------------------------------------------------

describe('parseRetryAfter', () => {
  it('parses integer seconds into milliseconds', () => {
    expect(parseRetryAfter('5', NOW)).toBe(5000);
    expect(parseRetryAfter('120', NOW)).toBe(120_000);
    expect(parseRetryAfter('0', NOW)).toBe(0);
  });

  it('parses a future HTTP-date as the delay until then', () => {
    const ms = parseRetryAfter('Fri, 24 Apr 2026 12:01:00 GMT', NOW);
    expect(ms).not.toBeNull();
    expect(ms ?? 0).toBeGreaterThanOrEqual(59_000);
    expect(ms ?? 0).toBeLessThanOrEqual(61_000);
  });

  it('clamps a past HTTP-date to zero', () => {
    expect(parseRetryAfter('Fri, 24 Apr 2026 11:59:00 GMT', NOW)).toBe(0);
  });

  it('returns null for a non-numeric, non-date, empty, or whitespace header', () => {
    expect(parseRetryAfter('not-a-number', NOW)).toBeNull();
    expect(parseRetryAfter('', NOW)).toBeNull();
    expect(parseRetryAfter('   ', NOW)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// computeBackoff
// ---------------------------------------------------------------------------

describe('computeBackoff', () => {
  const cfg = { baseMs: 500, factor: 2, maxMs: 2000 };

  it('grows exponentially and caps without a server value', () => {
    expect(computeBackoff(0, null, cfg)).toBe(500);
    expect(computeBackoff(1, null, cfg)).toBe(1000);
    expect(computeBackoff(2, null, cfg)).toBe(2000);
    expect(computeBackoff(10, null, cfg)).toBe(2000);
  });

  it('honours a server Retry-After verbatim (already pre-filtered to ≤ cap)', () => {
    expect(computeBackoff(0, 3000, cfg)).toBe(3000);
  });
});

// ---------------------------------------------------------------------------
// FetchTflHttp — success + retry behaviour
// ---------------------------------------------------------------------------

describe('FetchTflHttp', () => {
  it('returns parsed JSON on first-attempt success (one request)', async () => {
    const body = [{ id: 'x', timeToStation: 120 }];
    const fetchMock = vi.fn().mockResolvedValue(resp(200, { body }));
    const value = await client(fetchMock).fetch('arrivals', 'TEST');
    expect(value).toEqual(body);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('retries a 429 up to MAX_RETRIES then returns RateLimited (3 requests)', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(resp(429, { headers: { 'retry-after': '0' }, text: 'rate limited' }));
    await expect(client(fetchMock).fetch('arrivals', 'TEST')).rejects.toMatchObject({
      kind: 'RateLimited',
    });
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('succeeds when a 429 is followed by a 200 (two requests)', async () => {
    const body = [{ id: 'y' }];
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(resp(429, { headers: { 'retry-after': '0' } }))
      .mockResolvedValueOnce(resp(200, { body }));
    const value = await client(fetchMock).fetch('arrivals', 'RETRY');
    expect(value).toEqual(body);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('fails immediately when Retry-After exceeds the cap (one request)', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(resp(429, { headers: { 'retry-after': '60' }, text: 'rate limited' }));
    const err = await client(fetchMock)
      .fetch('arrivals', 'TEST')
      .catch((e: unknown) => e);
    expect(err).toBeInstanceOf(TflError);
    expect((err as TflError).kind).toBe('RateLimited');
    expect((err as TflError).retryAfterMs).toBe(60_000);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('retries a 503 up to MAX_RETRIES then returns Http 503 (3 requests)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(resp(503, { text: 'service unavailable' }));
    const err = await client(fetchMock)
      .fetch('arrivals', 'TEST')
      .catch((e: unknown) => e);
    expect((err as TflError).kind).toBe('Http');
    expect((err as TflError).status).toBe(503);
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('succeeds when a 503 is followed by a 200 (two requests)', async () => {
    const body = [{ id: 'z' }];
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(resp(503, { text: 'unavailable' }))
      .mockResolvedValueOnce(resp(200, { body }));
    expect(await client(fetchMock).fetch('arrivals', 'R503')).toEqual(body);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('does NOT retry a 500 (one request)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(resp(500, { text: 'internal error' }));
    const err = await client(fetchMock)
      .fetch('arrivals', 'TEST')
      .catch((e: unknown) => e);
    expect((err as TflError).kind).toBe('Http');
    expect((err as TflError).status).toBe(500);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('does NOT retry a 404, returning NotFound (one request)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(resp(404, { text: 'not found' }));
    const err = await client(fetchMock)
      .fetch('arrivals', 'MISSING')
      .catch((e: unknown) => e);
    expect((err as TflError).kind).toBe('NotFound');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  // -------------------------------------------------------------------------
  // app_key redaction + delivery
  // -------------------------------------------------------------------------

  it('never leaks app_key in a RateLimited error message', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(resp(429, { headers: { 'retry-after': '60' }, text: 'rate limited' }));
    const withKey = new FetchTflHttp({
      baseUrl: 'http://mock/',
      appKey: 'DEADBEEF',
      fetchImpl: fetchMock as unknown as typeof fetch,
      clock: FakeClock.at(NOW),
    });
    const err = await withKey.fetch('arrivals', 'TEST').catch((e: unknown) => e);
    expect((err as TflError).message).not.toContain('DEADBEEF');
  });

  it('never leaks app_key in an Http error message', async () => {
    const fetchMock = vi.fn().mockResolvedValue(resp(500, { text: 'internal error body' }));
    const withKey = new FetchTflHttp({
      baseUrl: 'http://mock/',
      appKey: 'DEADBEEF',
      fetchImpl: fetchMock as unknown as typeof fetch,
      clock: FakeClock.at(NOW),
    });
    const err = await withKey.fetch('arrivals', 'TEST').catch((e: unknown) => e);
    expect((err as TflError).message).not.toContain('DEADBEEF');
  });

  it('sends a static app_key as a query parameter', async () => {
    const fetchMock = vi.fn().mockResolvedValue(resp(200, { body: [] }));
    const withKey = new FetchTflHttp({
      baseUrl: 'http://mock/',
      appKey: 'DEADBEEF',
      fetchImpl: fetchMock as unknown as typeof fetch,
      clock: FakeClock.at(NOW),
    });
    await withKey.fetch('arrivals', 'X');
    expect(String(fetchMock.mock.calls[0]?.[0])).toContain('app_key=DEADBEEF');
  });

  it('sends a pooled key as a query parameter', async () => {
    const fetchMock = vi.fn().mockResolvedValue(resp(200, { body: [] }));
    const keyPool = KeyPool.create(['a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4']);
    if (keyPool === null) throw new Error('expected a pool');
    const pooled = new FetchTflHttp({
      baseUrl: 'http://mock/',
      keyPool,
      fetchImpl: fetchMock as unknown as typeof fetch,
      clock: FakeClock.at(NOW),
    });
    await pooled.fetch('arrivals', 'X');
    expect(String(fetchMock.mock.calls[0]?.[0])).toContain(
      'app_key=a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4',
    );
  });

  // -------------------------------------------------------------------------
  // 429 cooldown gate (Rust invariant #5)
  // -------------------------------------------------------------------------

  it('arms a process-wide cooldown gate that holds the wire for the full window', async () => {
    vi.useFakeTimers();
    try {
      const fetchMock = vi
        .fn()
        .mockResolvedValue(resp(429, { headers: { 'retry-after': '6' }, text: 'rate limited' }));
      const c = new FetchTflHttp({
        baseUrl: 'http://mock/',
        fetchImpl: fetchMock as unknown as typeof fetch,
        clock: FakeClock.at(NOW),
      });

      // First call: 429 with Retry-After 6s (> cap) → immediate RateLimited,
      // arming the cooldown gate (deadline = now + 6s).
      await expect(c.fetch('arrivals', 'COOL')).rejects.toMatchObject({ kind: 'RateLimited' });
      expect(fetchMock).toHaveBeenCalledTimes(1);

      // Second call blocks on the gate. Assert the observable — a wire hit —
      // by advancing the clock to the boundary, not by flushing microtasks.
      const second = c.fetch('arrivals', 'COOL').catch(() => undefined);
      await vi.advanceTimersByTimeAsync(5999);
      expect(fetchMock).toHaveBeenCalledTimes(1); // still gated at 5999 ms
      await vi.advanceTimersByTimeAsync(1);
      await second;
      expect(fetchMock).toHaveBeenCalledTimes(2); // gate released at 6000 ms
    } finally {
      vi.useRealTimers();
    }
  });

  it('treats a sub-second-over Retry-After as within cap (whole-second truncation like Rust)', async () => {
    vi.useFakeTimers();
    try {
      // Clock at .500 so an HTTP-date 6s boundary resolves to 5500 ms. Rust
      // truncates to 5s (≤ cap → retry); a naive `> 5000ms` compare would
      // wrongly fail fast. The retry must fire and succeed on the 200.
      const clock = FakeClock.at(new Date('2026-04-24T12:00:00.500Z'));
      const body = [{ id: 'ok' }];
      const fetchMock = vi
        .fn()
        .mockResolvedValueOnce(
          resp(429, { headers: { 'retry-after': 'Fri, 24 Apr 2026 12:00:06 GMT' } }),
        )
        .mockResolvedValueOnce(resp(200, { body }));
      const c = new FetchTflHttp({
        baseUrl: 'http://mock/',
        fetchImpl: fetchMock as unknown as typeof fetch,
        clock,
      });

      const p = c.fetch('arrivals', 'EDGE');
      await vi.advanceTimersByTimeAsync(5500); // honour the server Retry-After verbatim
      expect(await p).toEqual(body);
      expect(fetchMock).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});
