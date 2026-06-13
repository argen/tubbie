// @vitest-environment happy-dom
/**
 * Phase 5 — IPC command routing behind `USE_TS_TFL`.
 *
 * The four read commands (`searchStations`, `findNearestStations`,
 * `getLineStatus`, `getAllLineStatuses`) drive the UI through the TypeScript
 * `TflClient` when the flag is on, and through the existing Rust `invoke` path
 * when it is off. Same return types either way, so the calling components are
 * untouched (plan item 5.3). The runtime is mocked at the boundary — this test
 * proves the *routing*, not the live wiring.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LineStatus, NearbyStation, Station } from '$lib/ipc/types.js';

// --- Boundary mocks (hoisted) ----------------------------------------------

const invokeSpy = vi.fn(
  (_cmd: string, _args?: Record<string, unknown>): Promise<unknown> => Promise.resolve([]),
);
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeSpy(cmd, args),
}));

let flagOn = false;
vi.mock('$lib/tfl/flag.js', () => ({
  useTsTfl: () => flagOn,
}));

const tsStation: Station = {
  id: 'TS1',
  common_name: 'TS Station',
  modes: ['tube'],
  lat: 51.5,
  lon: -0.1,
  lines: [],
};
const tsNearby: NearbyStation = { station: tsStation, distance_m: 12 };
const tsLineStatus: LineStatus = {
  line_id: 'victoria',
  status: [],
  disruption_text: null,
};

const clientStub = {
  searchStations: vi.fn((): Promise<Station[]> => Promise.resolve([tsStation])),
  findNearestStations: vi.fn((): Promise<NearbyStation[]> => Promise.resolve([tsNearby])),
  getLineStatus: vi.fn((): Promise<LineStatus> => Promise.resolve(tsLineStatus)),
  getAllLineStatuses: vi.fn((): Promise<LineStatus[]> => Promise.resolve([tsLineStatus])),
};
vi.mock('$lib/tfl/runtime.js', () => ({
  tflRuntime: () => Promise.resolve({ client: clientStub, service: {} }),
}));

import {
  searchStations,
  findNearestStations,
  getLineStatus,
  getAllLineStatuses,
} from '$lib/ipc/commands.js';

beforeEach(() => {
  flagOn = false;
  invokeSpy.mockReset();
  invokeSpy.mockResolvedValue([]);
  for (const fn of Object.values(clientStub)) fn.mockClear();
});
afterEach(() => {
  vi.clearAllMocks();
});

describe('IPC routing — flag OFF (Rust path)', () => {
  it('searchStations invokes the Rust command, not the TS client', async () => {
    await searchStations('bank');
    expect(invokeSpy).toHaveBeenCalledWith('search_stations', { query: 'bank' });
    expect(clientStub.searchStations).not.toHaveBeenCalled();
  });

  it('getAllLineStatuses invokes the Rust command', async () => {
    await getAllLineStatuses();
    expect(invokeSpy).toHaveBeenCalledWith('get_all_line_statuses', undefined);
    expect(clientStub.getAllLineStatuses).not.toHaveBeenCalled();
  });
});

describe('IPC routing — flag ON (TS path)', () => {
  beforeEach(() => {
    flagOn = true;
  });

  it('searchStations calls the TS client and returns its result', async () => {
    const out = await searchStations('bank');
    expect(clientStub.searchStations).toHaveBeenCalledWith('bank');
    expect(invokeSpy).not.toHaveBeenCalled();
    expect(out).toEqual([tsStation]);
  });

  it('findNearestStations calls the TS client with lat/lon/limit', async () => {
    const out = await findNearestStations(51.5, -0.1, 5);
    expect(clientStub.findNearestStations).toHaveBeenCalledWith(51.5, -0.1, 5);
    expect(invokeSpy).not.toHaveBeenCalled();
    expect(out).toEqual([tsNearby]);
  });

  it('getLineStatus calls the TS client', async () => {
    const out = await getLineStatus('victoria');
    expect(clientStub.getLineStatus).toHaveBeenCalledWith('victoria');
    expect(invokeSpy).not.toHaveBeenCalled();
    expect(out).toEqual(tsLineStatus);
  });

  it('getAllLineStatuses calls the TS client', async () => {
    const out = await getAllLineStatuses();
    expect(clientStub.getAllLineStatuses).toHaveBeenCalled();
    expect(invokeSpy).not.toHaveBeenCalled();
    expect(out).toEqual([tsLineStatus]);
  });
});
