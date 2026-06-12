/**
 * Arrivals + line-status reads on `TflClient`, ported from
 * `crates/tfl-cache/src/client_tests.rs`: the single-id arrivals fast path,
 * hub fan-out with dedupe-by-id + soonest-first sort, line-status 60s cache,
 * worst-first ordering, and the NotFound contracts.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeClock } from '../transport/clock.js';
import { TflError } from '../transport/tflError.js';
import { RecordHttp } from './recordHttp.js';
import { TflClient } from './tflClient.js';
import { seedModes, synthHubDetail, synthStation } from './synth.js';

const noop = (): Promise<void> => Promise.resolve();
const EPOCH = new Date('2026-01-01T00:00:00Z');

function client(http: RecordHttp, clock = FakeClock.at(EPOCH)): TflClient {
  return new TflClient(http, { clock, sleep: noop });
}

function arrival(id: string, timeToStation: number, lineId = 'central'): unknown {
  return {
    id,
    lineId,
    lineName: lineId,
    timeToStation,
    stationName: 'Station',
    platformName: 'Platform 1',
    direction: 'inbound',
    destinationName: 'Somewhere',
    towards: 'Somewhere',
    currentLocation: '',
    expectedArrival: '2026-01-01T00:05:00Z',
    naptanId: '940GZZLUBNK',
  };
}

function line(id: string, severity: number): unknown {
  return {
    id,
    lineStatuses: [
      { statusSeverity: severity, statusSeverityDescription: `severity ${String(severity)}` },
    ],
  };
}

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});
afterEach(() => {
  vi.restoreAllMocks();
});

// ---------------------------------------------------------------------------
// getArrivals
// ---------------------------------------------------------------------------

describe('getArrivals', () => {
  it('uses the single-id fast path for a non-hub station (cold cache)', async () => {
    const http = new RecordHttp();
    http.put('arrivals', '940GZZLUBZP', [arrival('a1', 120), arrival('a2', 60)]);
    const c = client(http);

    const arrivals = await c.getArrivals('940GZZLUBZP');
    expect(arrivals.map((a) => a.id)).toEqual(['a1', 'a2']); // single-id: TfL order preserved
    expect(http.callCount('arrivals', '940GZZLUBZP')).toBe(1);
  });

  it('propagates NotFound from the single-id path', async () => {
    const c = client(new RecordHttp());
    await expect(c.getArrivals('940GZZLUNONE')).rejects.toMatchObject({ kind: 'NotFound' });
  });

  it('fans out a hub station to its children, deduping by id and sorting soonest-first', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUBNK', { modes: ['tube'], lines: ['central'], hub: 'HUBBAN' })],
    });
    http.put(
      'stop-point',
      'HUBBAN',
      synthHubDetail([
        { id: '940GZZLUBNK', modes: ['tube'], lines: ['central'] },
        { id: '940GZZDLBNK', modes: ['dlr'], lines: ['dlr'] },
      ]),
    );
    http.put('arrivals', '940GZZLUBNK', [arrival('dup', 300), arrival('tube1', 120)]);
    // The DLR sibling repeats `dup` (TfL shares a prediction across children).
    http.put('arrivals', '940GZZDLBNK', [arrival('dup', 300, 'dlr'), arrival('dlr1', 30, 'dlr')]);
    const c = client(http);
    await c.warmStopPointsCache();

    const arrivals = await c.getArrivals('940GZZLUBNK');
    expect(arrivals.map((a) => a.id)).toEqual(['dlr1', 'tube1', 'dup']); // soonest-first, deduped
  });

  it('drops a failing sibling rather than blanking the board', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUBNK', { modes: ['tube'], lines: ['central'], hub: 'HUBBAN' })],
    });
    http.put(
      'stop-point',
      'HUBBAN',
      synthHubDetail([
        { id: '940GZZLUBNK', modes: ['tube'], lines: ['central'] },
        { id: '940GZZDLBNK', modes: ['dlr'], lines: ['dlr'] },
      ]),
    );
    http.put('arrivals', '940GZZLUBNK', [arrival('tube1', 120)]);
    // DLR sibling not registered → NotFound, must be dropped.
    const c = client(http);
    await c.warmStopPointsCache();

    expect((await c.getArrivals('940GZZLUBNK')).map((a) => a.id)).toEqual(['tube1']);
  });
});

// ---------------------------------------------------------------------------
// getLineStatus / getAllLineStatuses
// ---------------------------------------------------------------------------

describe('line status', () => {
  it('finds one line across the surfaced modes', async () => {
    const http = new RecordHttp();
    seedModes(http, {}); // not used for line-status
    http.put('line-status', 'tube', [line('central', 6), line('victoria', 10)]);
    const c = client(http);

    const central = await c.getLineStatus('central');
    expect(central.status[0]?.bucket).toBe('SevereDelays');
  });

  it('throws NotFound for an unknown line', async () => {
    const http = new RecordHttp();
    http.put('line-status', 'tube', [line('central', 10)]);
    const c = client(http);
    await expect(c.getLineStatus('nope')).rejects.toMatchObject({ kind: 'NotFound' });
  });

  it('throws when every mode fails', async () => {
    const c = client(new RecordHttp()); // no line-status registered
    await expect(c.getAllLineStatuses()).rejects.toBeInstanceOf(TflError);
  });

  it('sorts all statuses worst-first then alphabetically', async () => {
    const http = new RecordHttp();
    http.put('line-status', 'tube', [
      line('victoria', 10), // GoodService
      line('central', 6), // SevereDelays (worst)
      line('bakerloo', 9), // MinorDelays
    ]);
    const c = client(http);

    const ids = (await c.getAllLineStatuses()).map((l) => l.line_id);
    expect(ids).toEqual(['central', 'bakerloo', 'victoria']);
  });

  it('serves repeat calls from the 60s cache, refetching past the TTL', async () => {
    const http = new RecordHttp();
    http.put('line-status', 'tube', [line('central', 10)]);
    const clock = FakeClock.at(EPOCH);
    const c = client(http, clock);

    await c.getAllLineStatuses();
    await c.getLineStatus('central');
    expect(http.callCount('line-status', 'tube')).toBe(1); // both served from cache

    clock.advance(61 * 1000);
    await c.getAllLineStatuses();
    expect(http.callCount('line-status', 'tube')).toBe(2); // refetched past TTL
  });
});
