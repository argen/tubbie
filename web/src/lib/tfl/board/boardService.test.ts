/**
 * `BoardService.refresh` end-to-end against a warmed `TflClient`: the arrivals
 * fetch runs through the directions → not-serving (#10) → off-axis →
 * terminating (#24) chain into a grouped `Board`.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BoardConfig } from '$lib/ipc/types.js';
import { FakeClock } from '../transport/clock.js';
import { RecordHttp } from '../cache/recordHttp.js';
import { TflClient } from '../cache/tflClient.js';
import { seedModes, synthStation } from '../cache/synth.js';
import { BoardService } from './boardService.js';

const EPOCH = new Date('2026-01-01T00:00:00Z');

function rawArrival(o: {
  id: string;
  lineId: string;
  station: string;
  destination: string;
  platformName: string;
  direction: string;
  timeToStation: number;
}): unknown {
  return {
    id: o.id,
    lineId: o.lineId,
    lineName: o.lineId,
    stationName: o.station,
    platformName: o.platformName,
    direction: o.direction,
    destinationName: o.destination,
    towards: o.destination,
    currentLocation: '',
    timeToStation: o.timeToStation,
    expectedArrival: '2026-01-01T00:05:00Z',
    naptanId: '940GZZLUEDG',
  };
}

function cfg(over: Partial<BoardConfig> = {}): BoardConfig {
  return {
    station_id: '940GZZLUEDG',
    line_ids: [],
    directions: [],
    poll_seconds: 20,
    theme: 'classic-amber',
    ...over,
  };
}

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe('BoardService.refresh', () => {
  it('runs arrivals through the filter chain into a grouped board', async () => {
    const http = new RecordHttp();
    // Edgware serves Northern only (terminus of the Edgware branch).
    seedModes(http, {
      tube: [
        synthStation('940GZZLUEDG', { modes: ['tube'], lines: ['northern'], name: 'Edgware' }),
      ],
    });
    http.put('arrivals', '940GZZLUEDG', [
      // Legit southbound Northern train → survives every filter.
      rawArrival({
        id: 'keep',
        lineId: 'northern',
        station: 'Edgware',
        destination: 'Kennington',
        platformName: 'Southbound - Platform 1',
        direction: 'outbound',
        timeToStation: 120,
      }),
      // Phantom Bakerloo (not served at Edgware) → dropped by not-serving (#10).
      rawArrival({
        id: 'phantom',
        lineId: 'bakerloo',
        station: 'Edgware',
        destination: 'Elephant',
        platformName: 'Northbound - Platform 2',
        direction: 'inbound',
        timeToStation: 60,
      }),
      // Train terminating here (destination == station) → dropped by #24.
      rawArrival({
        id: 'terminating',
        lineId: 'northern',
        station: 'Edgware',
        destination: 'Edgware',
        platformName: 'Southbound - Platform 1',
        direction: 'outbound',
        timeToStation: 90,
      }),
    ]);

    const clock = FakeClock.at(EPOCH);
    const client = new TflClient(http, { clock, sleep: () => Promise.resolve() });
    await client.warmStopPointsCache(); // populate allowedLineIdsFor
    const service = new BoardService(client, clock);

    const board = await service.refresh(cfg());

    expect(board.station_id).toBe('940GZZLUEDG');
    expect(board.generated_at).toBe('2026-01-01T00:00:00.000Z');
    expect(board.stale_since).toBeNull();
    expect(board.platforms).toHaveLength(1);
    expect(board.platforms[0]?.name).toBe('Southbound');
    expect(board.platforms[0]?.arrivals.map((a) => a.id)).toEqual(['keep']);
  });

  it('applies the directions filter when set', async () => {
    const http = new RecordHttp();
    seedModes(http, {
      tube: [
        synthStation('940GZZLUEDG', { modes: ['tube'], lines: ['northern'], name: 'Edgware' }),
      ],
    });
    http.put('arrivals', '940GZZLUEDG', [
      rawArrival({
        id: 'nb',
        lineId: 'northern',
        station: 'Edgware',
        destination: 'High Barnet',
        platformName: 'Northbound - Platform 1',
        direction: 'inbound',
        timeToStation: 120,
      }),
      rawArrival({
        id: 'sb',
        lineId: 'northern',
        station: 'Edgware',
        destination: 'Morden',
        platformName: 'Southbound - Platform 2',
        direction: 'outbound',
        timeToStation: 60,
      }),
    ]);

    const clock = FakeClock.at(EPOCH);
    const client = new TflClient(http, { clock, sleep: () => Promise.resolve() });
    await client.warmStopPointsCache();
    const service = new BoardService(client, clock);

    const board = await service.refresh(cfg({ directions: ['Northbound'] }));
    expect(board.platforms.map((p) => p.name)).toEqual(['Northbound']);
    expect(board.platforms[0]?.arrivals.map((a) => a.id)).toEqual(['nb']);
  });
});
