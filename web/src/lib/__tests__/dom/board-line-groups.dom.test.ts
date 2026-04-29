// @vitest-environment happy-dom
/**
 * DOM tests for the line-grouped board layout.
 *
 * The Rust backend buckets arrivals by Direction (`Board.platforms[]` =
 * Northbound/Southbound/Eastbound/.../Unknown), and at multi-line
 * interchanges a single direction bucket explicitly mixes lines — King's
 * Cross "Westbound" carries hammersmith-city + metropolitan, Baker Street
 * southbound carries Bakerloo + Jubilee. The frontend has to invert the
 * grouping (line first, then direction) so the line-coloured stripe on
 * each row always matches the train.
 *
 * What we lock in here:
 *
 *   1. A single platform with arrivals from multiple lines splits into one
 *      LineGroup per line. This is the headline regression — the previous
 *      "use arrivals[0].line_id" implementation labelled the entire
 *      column by whichever line happened to be first.
 *   2. LineGroup ordering follows first-seen `line_id`, not alphabetical.
 *   3. Within a line, direction columns are sorted by canonical compass
 *      order (Northbound, Southbound, Eastbound, Westbound, Inbound,
 *      Outbound) regardless of how the backend ordered platforms.
 *   4. Arrivals from different physical platforms but the same
 *      (line, direction) bucket merge into one column — that is the
 *      backend's contract and we MUST preserve it.
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
  platformName = `${direction} - Platform 1`,
  expectedArrival = '2026-04-27T19:00:00Z',
): Arrival {
  return {
    id: `${lineId}-${destination}-${expectedArrival}`,
    station_name: 'Test',
    platform_name: platformName,
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
    generated_at: '2026-04-27T19:00:00Z',
    stale_since: null,
  };
}

function lineGroupIds(): string[] {
  return Array.from(document.querySelectorAll('.line-group')).map(
    (el) => el.getAttribute('data-line-id') ?? '',
  );
}

function directionLabelsFor(lineId: string): string[] {
  const group = document.querySelector(`.line-group[data-line-id="${lineId}"]`);
  if (group === null) return [];
  return Array.from(group.querySelectorAll('.platform-col__label')).map(
    (el) => el.textContent?.trim() ?? '',
  );
}

describe('Board — per-arrival line grouping', () => {
  it('splits a mixed-line direction bucket into one LineGroup per line', () => {
    // King's Cross-style payload: backend returned ONE Westbound platform
    // with arrivals from two lines mixed together. The frontend must
    // ungroup by line so each train shows under its own line header.
    const board = buildBoard([
      platform('Westbound', [
        arrival('hammersmith-city', 'Hammersmith & City', 'Westbound', 'Hammersmith'),
        arrival('metropolitan', 'Metropolitan', 'Westbound', 'Uxbridge'),
        arrival(
          'hammersmith-city',
          'Hammersmith & City',
          'Westbound',
          'Hammersmith',
          'Westbound',
          '2026-04-27T19:05:00Z',
        ),
      ]),
    ]);

    render(Board, { props: { board } });

    expect(lineGroupIds()).toEqual(['hammersmith-city', 'metropolitan']);
    expect(directionLabelsFor('hammersmith-city')).toEqual(['Westbound']);
    expect(directionLabelsFor('metropolitan')).toEqual(['Westbound']);
  });

  it('renders a 5-line interchange (Baker Street) with one group per line', () => {
    // Bakerloo + Jubilee share southbound platforms at Baker Street; the
    // backend returns them in one direction bucket. Verifies we don't
    // mis-attribute the Jubilee Stratford trains to Bakerloo.
    const board = buildBoard([
      platform('Northbound', [
        arrival('metropolitan', 'Metropolitan', 'Northbound', 'Wembley Park'),
        arrival('jubilee', 'Jubilee', 'Northbound', 'Stanmore'),
        arrival('bakerloo', 'Bakerloo', 'Northbound', 'Harrow & Wealdstone'),
      ]),
      platform('Southbound', [
        arrival('bakerloo', 'Bakerloo', 'Southbound', 'Elephant & Castle'),
        arrival('jubilee', 'Jubilee', 'Southbound', 'Stratford'),
        arrival('metropolitan', 'Metropolitan', 'Southbound', 'Aldgate'),
      ]),
      platform('Eastbound', [
        arrival('circle', 'Circle', 'Eastbound', 'Edgware Road'),
        arrival('hammersmith-city', 'Hammersmith & City', 'Eastbound', 'Barking'),
      ]),
      platform('Westbound', [
        arrival('circle', 'Circle', 'Westbound', 'Edgware Road'),
        arrival('hammersmith-city', 'Hammersmith & City', 'Westbound', 'Hammersmith'),
      ]),
    ]);

    render(Board, { props: { board } });

    // First-seen order from the backend's Direction-major ordering.
    expect(lineGroupIds()).toEqual([
      'metropolitan',
      'jubilee',
      'bakerloo',
      'circle',
      'hammersmith-city',
    ]);
  });

  it('sorts directions inside a line by canonical compass order', () => {
    // Backend can hand us Westbound before Eastbound (or any order); we
    // re-sort so the user always sees the same compass progression.
    const board = buildBoard([
      platform('Westbound', [arrival('central', 'Central', 'Westbound', 'Ealing Broadway')]),
      platform('Eastbound', [arrival('central', 'Central', 'Eastbound', 'Epping')]),
      platform('Northbound', [arrival('victoria', 'Victoria', 'Northbound', 'Walthamstow')]),
      platform('Southbound', [arrival('victoria', 'Victoria', 'Southbound', 'Brixton')]),
    ]);

    render(Board, { props: { board } });

    expect(directionLabelsFor('central')).toEqual(['Eastbound', 'Westbound']);
    expect(directionLabelsFor('victoria')).toEqual(['Northbound', 'Southbound']);
  });

  it('merges arrivals from different physical platforms into one direction column', () => {
    // The backend already does this (Tottenham Court Road test in the
    // tfl-board crate); we just have to not undo it. Two arrivals on the
    // same (line, direction) but different `platform_name` should land
    // in the same column.
    const board = buildBoard([
      platform('Eastbound', [
        arrival('central', 'Central', 'Eastbound', 'Epping', 'Eastbound - Platform 3'),
        arrival(
          'central',
          'Central',
          'Eastbound',
          'Hainault',
          'Eastbound - Platform 5',
          '2026-04-27T19:03:00Z',
        ),
      ]),
    ]);

    render(Board, { props: { board } });

    expect(lineGroupIds()).toEqual(['central']);
    const rows = document.querySelectorAll('.line-group[data-line-id="central"] .arrival-row');
    expect(rows.length).toBe(2);
  });

  // Overground arrivals carry `direction: "Inbound"` / `"Outbound"` (not
  // compass headings) and have their own line ids — Mildmay, Windrush, etc.
  // The frontend grouping must treat them identically to tube lines: one
  // LineGroup per line id, with directions sorted into the canonical compass
  // order that places Inbound/Outbound after the four headings.
  it('renders Mildmay + Windrush at a multi-line Overground hub', () => {
    const board = buildBoard([
      platform('Inbound', [
        arrival('mildmay', 'Mildmay', 'Inbound', 'Stratford', 'Inbound - Platform 2'),
        arrival('windrush', 'Windrush', 'Inbound', 'Highbury & Islington', 'Inbound - Platform 1'),
      ]),
      platform('Outbound', [
        arrival(
          'mildmay',
          'Mildmay',
          'Outbound',
          'Richmond',
          'Outbound - Platform 3',
          '2026-04-27T19:04:00Z',
        ),
        arrival(
          'windrush',
          'Windrush',
          'Outbound',
          'New Cross Gate',
          'Outbound - Platform 4',
          '2026-04-27T19:06:00Z',
        ),
      ]),
    ]);

    render(Board, { props: { board } });

    expect(lineGroupIds()).toEqual(['mildmay', 'windrush']);
    expect(directionLabelsFor('mildmay')).toEqual(['Inbound', 'Outbound']);
    expect(directionLabelsFor('windrush')).toEqual(['Inbound', 'Outbound']);

    // Stripe colour MUST resolve to the Mildmay CSS variable, not a
    // generic Overground orange — the per-line stripe is what tells the
    // user which train they're looking at when two lines share a column.
    const mildmayRow = document.querySelector(
      '.line-group[data-line-id="mildmay"] .arrival-row',
    ) as HTMLElement | null;
    expect(mildmayRow).not.toBeNull();
    expect(mildmayRow!.style.getPropertyValue('--line-color')).toBe('var(--line-mildmay)');
    const windrushRow = document.querySelector(
      '.line-group[data-line-id="windrush"] .arrival-row',
    ) as HTMLElement | null;
    expect(windrushRow).not.toBeNull();
    expect(windrushRow!.style.getPropertyValue('--line-color')).toBe('var(--line-windrush)');
  });

  it('renders nothing meaningful for an empty board (no fake "unknown" group)', () => {
    const board = buildBoard([]);
    render(Board, { props: { board } });
    expect(lineGroupIds()).toEqual([]);
    expect(document.querySelector('[role="status"]')?.textContent).toMatch(/No arrivals/i);
  });
});
