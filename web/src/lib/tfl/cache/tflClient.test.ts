/**
 * Cache mechanics for `TflClient`, ported from `crates/tfl-cache/src/client_tests.rs`:
 * multi-mode merge (#12), per-mode retry (#21), single-flight (#16), SWR + force
 * refresh (#19/#20), partial-warm + backfill (#26), hub-line merge + NotFound
 * caching (#15/#17), and the search/nearest/allowed-lines surface (#13/#18/#19).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeClock } from '../transport/clock.js';
import { TflError } from '../transport/tflError.js';
import { RecordHttp } from './recordHttp.js';
import { TflClient } from './tflClient.js';
import { modeBody, seedModes, synthHubDetail, synthStation } from './synth.js';

const noop = (): Promise<void> => Promise.resolve();
const EPOCH = new Date('2026-01-01T00:00:00Z');

function client(http: RecordHttp, clock = FakeClock.at(EPOCH)): TflClient {
  return new TflClient(http, { clock, sleep: noop });
}

beforeEach(() => {
  // Warm coverage + parse fallbacks log to console.warn by design; keep quiet.
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});
afterEach(() => {
  vi.restoreAllMocks();
});

// ---------------------------------------------------------------------------
// Multi-mode merge (#12)
// ---------------------------------------------------------------------------

describe('multi-mode merge', () => {
  it('merges stations across modes by id, unioning lines and filling the hub code', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUSTD', { modes: ['tube'], lines: ['central'] })],
      dlr: [synthStation('940GZZLUSTD', { modes: ['dlr'], lines: ['dlr'], hub: 'HUBSTD' })],
    });
    const c = client(http);

    const results = await c.searchStations('940GZZLUSTD');
    expect(results).toHaveLength(1);
    const lineIds = results[0]?.lines.map((l) => l.id).sort();
    expect(lineIds).toEqual(['central', 'dlr']);
    expect(results[0]?.hub_naptan_code).toBe('HUBSTD');
  });
});

// ---------------------------------------------------------------------------
// Per-mode retry (#21)
// ---------------------------------------------------------------------------

describe('per-mode retry', () => {
  it('retries a transient mode failure and recovers within the attempt budget', async () => {
    const http = new RecordHttp();
    seedModes(http, {}); // dlr/overground/elizabeth empty
    const tube = modeBody([synthStation('940GZZLUA', { modes: ['tube'], lines: ['central'] })]);
    http.putHandler('stop-points', 'tube', (n) =>
      n <= 2 ? Promise.reject(TflError.rateLimited(0)) : Promise.resolve(tube),
    );
    const c = client(http);

    await c.warmStopPointsCache();
    expect(c.stopPointsWarmIsPartial()).toBe(false);
    expect(http.callCount('stop-points', 'tube')).toBe(3); // failed twice, succeeded third
    expect(await c.searchStations('940GZZLUA')).toHaveLength(1);
  });

  it('marks the warm partial when a mode exhausts its retries', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUA', { modes: ['tube'], lines: ['central'] })],
    });
    http.putHandler('stop-points', 'dlr', () => Promise.reject(TflError.rateLimited(0)));
    const c = client(http);

    await c.warmStopPointsCache();
    expect(c.stopPointsWarmIsPartial()).toBe(true);
    expect(http.callCount('stop-points', 'dlr')).toBe(4); // 1 + 3 retries
    // The healthy mode still surfaced.
    expect(await c.searchStations('940GZZLUA')).toHaveLength(1);
  });

  it('throws when every mode fails', async () => {
    const http = new RecordHttp(); // nothing registered → all modes NotFound
    const c = client(http);
    await expect(c.warmStopPointsCache()).rejects.toBeInstanceOf(TflError);
  });
});

// ---------------------------------------------------------------------------
// Single-flight (#16)
// ---------------------------------------------------------------------------

describe('single-flight', () => {
  it('coalesces concurrent cold-cache refreshes into one fan-out', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUA', { modes: ['tube'], lines: ['central'] })],
    });
    const c = client(http);

    await Promise.all([c.searchStations('a'), c.searchStations('a'), c.searchStations('a')]);

    // One fan-out total despite three concurrent searches.
    for (const mode of ['tube', 'overground', 'dlr', 'elizabeth-line']) {
      expect(http.callCount('stop-points', mode)).toBe(1);
    }
  });
});

// ---------------------------------------------------------------------------
// SWR + force refresh (#19/#20)
// ---------------------------------------------------------------------------

describe('stale-while-revalidate', () => {
  it('serves a stale full-warm entry without refetching on search', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUA', { modes: ['tube'], lines: ['central'] })],
    });
    const clock = FakeClock.at(EPOCH);
    const c = client(http, clock);

    await c.warmStopPointsCache();
    clock.advance(20 * 60 * 1000); // past the 15-min TTL
    await c.searchStations('a');

    expect(http.callCount('stop-points', 'tube')).toBe(1); // not refetched
  });

  it('forces a refetch even when the cache is fresh', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUA', { modes: ['tube'], lines: ['central'] })],
    });
    const c = client(http);

    await c.warmStopPointsCache();
    await c.refreshStopPointsCache();

    expect(http.callCount('stop-points', 'tube')).toBe(2);
  });
});

// ---------------------------------------------------------------------------
// Partial-warm backfill + short retry window (#26)
// ---------------------------------------------------------------------------

describe('partial warm', () => {
  it('backfills the failed mode from the prior cache and uses the short retry window', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUA', { modes: ['tube'], lines: ['central'] })],
      dlr: [synthStation('940GZZDLB', { modes: ['dlr'], lines: ['dlr'] })],
    });
    const clock = FakeClock.at(EPOCH);
    const c = client(http, clock);

    await c.warmStopPointsCache(); // full warm: both A and B
    expect(c.stopPointsWarmIsPartial()).toBe(false);

    // DLR now fails; a forced refresh must not lose the DLR station.
    http.putHandler('stop-points', 'dlr', () => Promise.reject(TflError.rateLimited(0)));
    await c.refreshStopPointsCache();
    expect(c.stopPointsWarmIsPartial()).toBe(true);
    expect(await c.searchStations('940GZZDLB')).toHaveLength(1); // backfilled

    // Within the 60 s window a search serves the partial entry, no re-fan.
    const dlrCallsAfterPartial = http.callCount('stop-points', 'dlr');
    clock.advance(30 * 1000);
    await c.searchStations('a');
    expect(http.callCount('stop-points', 'dlr')).toBe(dlrCallsAfterPartial);

    // Past the 60 s window the next search re-fans the failed mode.
    clock.advance(40 * 1000);
    await c.searchStations('a');
    expect(http.callCount('stop-points', 'dlr')).toBeGreaterThan(dlrCallsAfterPartial);
  });
});

// ---------------------------------------------------------------------------
// Hub-line merge + NotFound caching (#15/#17)
// ---------------------------------------------------------------------------

describe('hub-line merge', () => {
  it('merges sibling-stop-point lines into a hub station', async () => {
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
    const c = client(http);

    const [bank] = await c.searchStations('940GZZLUBNK');
    expect(bank?.lines.map((l) => l.id).sort()).toEqual(['central', 'dlr']);
  });

  it('caches a NotFound hub as empty and never refetches it', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUX', { modes: ['tube'], lines: ['central'], hub: 'HUBGHOST' })],
    });
    // HUBGHOST is never registered → NotFound.
    const c = client(http);

    await c.warmStopPointsCache();
    await c.refreshStopPointsCache();
    expect(c.stopPointsWarmIsPartial()).toBe(false); // NotFound is not a transient failure
    expect(http.callCount('stop-point', 'HUBGHOST')).toBe(1); // cached after the first miss
  });

  it('marks the warm partial when a hub fetch fails transiently', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUY', { modes: ['tube'], lines: ['central'], hub: 'HUBRL' })],
    });
    http.putHandler('stop-point', 'HUBRL', () => Promise.reject(TflError.rateLimited(0)));
    const c = client(http);

    await c.warmStopPointsCache();
    expect(c.stopPointsWarmIsPartial()).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// search_stations whitelist + dedupe + relevance (#13/#18)
// ---------------------------------------------------------------------------

describe('searchStations', () => {
  it('returns [] for an empty/whitespace query without touching the cache', async () => {
    const http = new RecordHttp();
    const c = client(http);
    expect(await c.searchStations('   ')).toEqual([]);
    expect(http.callCount('stop-points', 'tube')).toBe(0);
  });

  it('applies the NaPTAN whitelist: 940GZZLU / 940GZZDL / 910G(og|elizabeth) in, rest out', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [
        synthStation('940GZZLUBST', { modes: ['tube'], lines: ['bakerloo'], name: 'Baker x' }),
      ],
      dlr: [synthStation('940GZZDLPOP', { modes: ['dlr'], lines: ['dlr'], name: 'Poplar x' })],
      overground: [
        synthStation('910GHACKNYC', { modes: ['overground'], lines: ['mildmay'], name: 'Hack x' }),
        // 910G but National-Rail-only (no og/elizabeth mode) → excluded.
        synthStation('910GGTWK', { modes: ['national-rail'], lines: [], name: 'Gatwick x' }),
      ],
    });
    const c = client(http);

    const names = (await c.searchStations('x')).map((s) => s.common_name);
    expect(names).toContain('Baker x');
    expect(names).toContain('Poplar x');
    expect(names).toContain('Hack x');
    expect(names).not.toContain('Gatwick x');
  });

  it('dedupes interchange entries sharing a hub, preferring the tube id', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [
        synthStation('940GZZLUBNK', {
          modes: ['tube'],
          lines: ['central'],
          hub: 'HUBBAN',
          name: 'Bank',
        }),
      ],
      dlr: [
        synthStation('940GZZDLBNK', {
          modes: ['dlr'],
          lines: ['dlr'],
          hub: 'HUBBAN',
          name: 'Bank',
        }),
      ],
    });
    const c = client(http);

    const banks = await c.searchStations('Bank');
    expect(banks).toHaveLength(1);
    expect(banks[0]?.id).toBe('940GZZLUBNK'); // tube wins
  });

  it('orders exact > prefix > substring, then by name, and truncates to 20', async () => {
    const http = new RecordHttp();
    const stations = [
      synthStation('940GZZLU1', { modes: ['tube'], lines: ['central'], name: 'Oxford Circus' }),
      synthStation('940GZZLU2', { modes: ['tube'], lines: ['central'], name: 'Oxford' }),
      synthStation('940GZZLU3', { modes: ['tube'], lines: ['central'], name: 'East Oxford Road' }),
    ];
    seedModes(http, { tube: stations });
    const c = client(http);

    const names = (await c.searchStations('Oxford')).map((s) => s.common_name);
    expect(names).toEqual(['Oxford', 'Oxford Circus', 'East Oxford Road']);
  });
});

// ---------------------------------------------------------------------------
// findNearestStations + allowedLineIdsFor (#19)
// ---------------------------------------------------------------------------

describe('findNearestStations', () => {
  it('ranks by distance through the whitelist + hub dedupe', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [
        synthStation('940GZZLUBNK', {
          modes: ['tube'],
          lines: ['central'],
          lat: 51.5133,
          lon: -0.0886,
          name: 'Bank',
        }),
        synthStation('940GZZLUOXC', {
          modes: ['tube'],
          lines: ['central'],
          lat: 51.5152,
          lon: -0.1419,
          name: 'Oxford Circus',
        }),
      ],
    });
    const c = client(http);

    const near = await c.findNearestStations(51.5133, -0.0886, 5);
    expect(near.map((n) => n.station.common_name)).toEqual(['Bank', 'Oxford Circus']);
  });
});

describe('allowedLineIdsFor', () => {
  it('projects the hub-merged line set for a station', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUBNK', { modes: ['tube'], lines: ['central'], hub: 'HUBBAN' })],
    });
    http.put(
      'stop-point',
      'HUBBAN',
      synthHubDetail([{ id: '940GZZDLBNK', modes: ['dlr'], lines: ['dlr'] }]),
    );
    const c = client(http);

    await c.warmStopPointsCache();
    expect([...c.allowedLineIdsFor('940GZZLUBNK')].sort()).toEqual(['central', 'dlr']);
  });

  it('fails open with an empty set on a cold cache', () => {
    const c = client(new RecordHttp());
    expect(c.allowedLineIdsFor('940GZZLUBNK').size).toBe(0);
  });

  it('serves a stale cache past the TTL (#19)', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [synthStation('940GZZLUA', { modes: ['tube'], lines: ['central'] })],
    });
    const clock = FakeClock.at(EPOCH);
    const c = client(http, clock);

    await c.warmStopPointsCache();
    clock.advance(60 * 60 * 1000); // an hour past TTL
    expect([...c.allowedLineIdsFor('940GZZLUA')]).toEqual(['central']);
  });
});
