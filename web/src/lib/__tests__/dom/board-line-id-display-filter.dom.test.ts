// @vitest-environment happy-dom
/**
 * The user's line-id chip filter is applied at the display layer in
 * `Board.svelte::linesGrouped`, NOT in the Rust `apply_filters`. This
 * test guards the contract that toggling chips on the Settings page
 * masks the visible board *immediately* — without waiting for the
 * next ~30 s periodic stream tick to re-emit a backend-filtered
 * payload.
 *
 * The board passed in here intentionally contains arrivals from
 * lines OUTSIDE the `lineIds` set — that's the realistic shape the
 * backend now hands through (every line the station serves). The
 * frontend masks them.
 */
import { describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import Board from '$lib/components/Board.svelte';
import type { Arrival, Board as BoardType, Direction, Platform } from '$lib/ipc/types.js';
import { mockInvoke } from '$lib/ipc/mock.js';

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

function arrival(
  lineId: string,
  lineName: string,
  direction: Direction,
  destination: string,
  expectedArrival = '2026-04-30T08:00:00Z',
): Arrival {
  return {
    id: `${lineId}-${destination}-${expectedArrival}`,
    station_name: 'Test',
    platform_name: `${direction} - Platform 1`,
    line_id: lineId,
    line_name: lineName,
    direction,
    destination_name: destination,
    towards: destination,
    current_location: 'On schedule',
    time_to_station: 60,
    expected_arrival: expectedArrival,
    naptan_id: '940TEST',
  };
}

function platform(name: string, arrivals: Arrival[]): Platform {
  return { name, arrivals };
}

function buildBoard(platforms: Platform[]): BoardType {
  return {
    station_id: '940GZZTEST',
    platforms,
    generated_at: '2026-04-30T08:00:00Z',
    stale_since: null,
  };
}

function lineGroupIds(): string[] {
  return Array.from(document.querySelectorAll('.line-group')).map(
    (el) => el.getAttribute('data-line-id') ?? '',
  );
}

describe('Board — line-id display filter (frontend, not backend)', () => {
  it('hides line groups whose id is not in the lineIds prop', () => {
    // Mixed-line King's Cross-style board: northern + victoria + circle.
    // User has narrowed to just `northern` via the chip filter.
    const board = buildBoard([
      platform('Northbound', [
        arrival('northern', 'Northern', 'Northbound', 'Edgware'),
        arrival('victoria', 'Victoria', 'Northbound', 'Walthamstow'),
        arrival('circle', 'Circle', 'Northbound', 'Edgware Road'),
      ]),
    ]);

    render(Board, { props: { board, lineIds: ['northern'] } });

    expect(lineGroupIds()).toEqual(['northern']);
  });

  it('shows ALL line groups when lineIds is empty (no filter)', () => {
    const board = buildBoard([
      platform('Northbound', [
        arrival('northern', 'Northern', 'Northbound', 'Edgware'),
        arrival('victoria', 'Victoria', 'Northbound', 'Walthamstow'),
        arrival('circle', 'Circle', 'Northbound', 'Edgware Road'),
      ]),
    ]);

    render(Board, { props: { board, lineIds: [] } });

    expect(lineGroupIds()).toEqual(['northern', 'victoria', 'circle']);
  });

  it('shows multiple line groups when lineIds contains multiple ids', () => {
    const board = buildBoard([
      platform('Northbound', [
        arrival('northern', 'Northern', 'Northbound', 'Edgware'),
        arrival('victoria', 'Victoria', 'Northbound', 'Walthamstow'),
        arrival('piccadilly', 'Piccadilly', 'Northbound', 'Cockfosters'),
      ]),
    ]);

    render(Board, { props: { board, lineIds: ['northern', 'victoria'] } });

    const ids = lineGroupIds();
    expect(ids).toContain('northern');
    expect(ids).toContain('victoria');
    expect(ids).not.toContain('piccadilly');
  });

  it('a line group disappears when a single arrival is filtered out (no empty stub)', () => {
    // Single Westbound bucket; user filters to a line not present at all
    // — the result should be zero line groups, with the empty-state UI
    // taking over (per Board.svelte's existing `linesGrouped.length === 0`
    // branch).
    const board = buildBoard([
      platform('Westbound', [arrival('central', 'Central', 'Westbound', 'Ealing Broadway')]),
    ]);

    render(Board, { props: { board, lineIds: ['victoria'] } });

    expect(lineGroupIds()).toEqual([]);
  });

  it('the filter badge keeps showing the user’s selected lineIds even when no arrivals match', () => {
    // The badge tells the user "you filtered to X" and stays present
    // regardless of whether the current board's arrivals satisfy the
    // filter. This is the existing badge contract — guarding it here
    // because the new display-filter shouldn't touch the badge logic.
    const board = buildBoard([
      platform('Westbound', [arrival('central', 'Central', 'Westbound', 'Ealing Broadway')]),
    ]);

    const { container } = render(Board, {
      props: { board, lineIds: ['victoria', 'piccadilly'] },
    });

    const badge = container.querySelector('[data-testid="board-line-filter"]');
    expect(badge).not.toBeNull();
    const text = (badge as HTMLElement).textContent ?? '';
    expect(text).toMatch(/victoria/i);
    expect(text).toMatch(/piccadilly/i);
  });
});
