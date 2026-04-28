// @vitest-environment happy-dom
/**
 * DOM tests for the renderer-driven adaptive window resize.
 *
 * The Board picks a (width, height) tier from the line count (derived
 * from per-arrival grouping) and the live `displayMode`, and pushes it
 * through `apply_board_size` only when the picked tier changes. Every
 * poll tick produces a board update (~30 s); without dedupe we'd hammer
 * the macOS main thread for nothing on every tick because the line
 * count rarely changes.
 *
 * What we lock in here:
 *
 *   1. Each preset tier is reachable from a synthesised board with the
 *      right line count (1 / 2 / 3 / 4+ lines, both modes).
 *   2. Re-rendering with the same board only invokes `apply_board_size`
 *      once. The renderer-side dedupe protects the main-thread Cocoa
 *      dispatch from per-tick traffic.
 *   3. Switching from a small station to a busier one triggers a fresh
 *      `apply_board_size` call — so users get the room they need.
 *   4. Menubar width stays at 380 across all tiers (the popover is
 *      anchored under the tray; horizontal resize would re-trigger
 *      anchoring and risk flicker on macOS).
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import { tick } from 'svelte';
import Board from '$lib/components/Board.svelte';
import type { Arrival, Board as BoardType, Direction, Platform } from '$lib/ipc/types.js';
import { mockInvoke, setMockHandler, resetMockHandlers } from '$lib/ipc/mock.js';
import { displayMode } from '$lib/stores/displayMode.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));
vi.mock('$lib/stores/board.js', () => ({
  lastUpdateTs: {
    subscribe: (fn: (v: number) => void) => {
      fn(0);
      return () => undefined;
    },
  },
}));
vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

interface ResizeArgs {
  width: number;
  height: number;
}

function arrival(lineId: string, direction: Direction, dest: string): Arrival {
  return {
    id: `${lineId}-${direction}-${dest}`,
    station_name: 'Test',
    platform_name: `${direction} - Platform`,
    line_id: lineId,
    line_name: lineId,
    direction,
    destination_name: dest,
    towards: dest,
    current_location: 'On schedule',
    time_to_station: 60,
    expected_arrival: '2026-04-27T19:00:00Z',
    naptan_id: '940TEST',
  };
}

function platform(name: string, arrivals: Arrival[]): Platform {
  return { name, arrivals };
}

/**
 * Build a station with `lineCount` distinct lines, each serving
 * Northbound + Southbound. The exact destinations don't matter for the
 * resize logic — what matters is the count of distinct `line_id` values.
 */
function stationWithLines(lineCount: number, generated_at = '2026-04-27T19:00:00Z'): BoardType {
  const lineIds = ['northern', 'central', 'piccadilly', 'circle', 'jubilee', 'metropolitan'];
  const ids = lineIds.slice(0, lineCount);
  const nb: Arrival[] = ids.map((id) => arrival(id, 'Northbound', `${id}-N`));
  const sb: Arrival[] = ids.map((id) => arrival(id, 'Southbound', `${id}-S`));
  return {
    station_id: '940GZZTEST',
    platforms: [platform('Northbound', nb), platform('Southbound', sb)],
    generated_at,
    stale_since: null,
  };
}

describe('Board — adaptive resize requests', () => {
  let calls: ResizeArgs[];

  beforeEach(() => {
    resetMockHandlers();
    calls = [];
    setMockHandler('apply_board_size', (args) => {
      calls.push({ width: args.width as number, height: args.height as number });
      return null;
    });
    displayMode.set('window');
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it.each([
    [1, { width: 700, height: 560 }],
    [2, { width: 980, height: 680 }],
    [3, { width: 1200, height: 760 }],
    [5, { width: 1200, height: 880 }], // 4+ tier
  ])('window mode + %d-line station → %o', async (lineCount, expected) => {
    render(Board, { props: { board: stationWithLines(lineCount) } });
    await tick();
    expect(calls.at(-1)).toEqual(expected);
  });

  it.each([
    [1, { width: 380, height: 520 }],
    [2, { width: 380, height: 620 }],
    [3, { width: 380, height: 720 }],
    [5, { width: 380, height: 800 }], // 4+ tier
  ])('menubar mode + %d-line station → %o', async (lineCount, expected) => {
    displayMode.set('menubar');
    render(Board, { props: { board: stationWithLines(lineCount) } });
    await tick();
    expect(calls.at(-1)).toEqual(expected);
  });

  it('only one resize call when re-rendering the same board (tier dedupe)', async () => {
    const { rerender } = render(Board, { props: { board: stationWithLines(1) } });
    await tick();
    const baselineCount = calls.length;

    // A fresh `generated_at` simulates a poll tick: same station, new
    // board snapshot. The picked tier hasn't changed, so we MUST NOT
    // re-issue the resize request.
    await rerender({ board: stationWithLines(1, '2026-04-27T19:00:30Z') });
    await tick();

    expect(calls.length).toBe(baselineCount);
  });

  it('switching to a busier station fires a fresh resize request', async () => {
    const { rerender } = render(Board, { props: { board: stationWithLines(1) } });
    await tick();
    expect(calls.at(-1)).toEqual({ width: 700, height: 560 });

    await rerender({ board: stationWithLines(4) });
    await tick();
    expect(calls.at(-1)).toEqual({ width: 1200, height: 880 });
  });

  it('menubar tier transitions issue a fresh resize (4-tier ladder)', async () => {
    displayMode.set('menubar');
    const { rerender } = render(Board, { props: { board: stationWithLines(1) } });
    await tick();
    expect(calls.at(-1)).toEqual({ width: 380, height: 520 });

    await rerender({ board: stationWithLines(3) });
    await tick();
    expect(calls.at(-1)).toEqual({ width: 380, height: 720 });
  });
});
