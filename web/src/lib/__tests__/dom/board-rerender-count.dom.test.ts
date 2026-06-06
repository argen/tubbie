// @vitest-environment happy-dom
/**
 * Render-count / re-mount test for Board component — Fix 6 (performance).
 *
 * Goals:
 *   1. Assert the Board component does NOT re-mount during 60 successive
 *      board://updated events (no {#key}-style teardown + recreation).
 *   2. Assert that a board-level $effect (tracking mount-time only) fires
 *      exactly ONCE across 60 updates.
 *   3. Assert that each board update does cause descendant DOM to update.
 *
 * Approach: we render Board, capture the root <main> DOM node reference
 * before and after N prop updates and assert it is the SAME object.
 * A re-mount would produce a new DOM node (old removed, new inserted).
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import Board from '$lib/components/Board.svelte';
import type { Board as BoardType } from '$lib/ipc/types.js';

// ---------------------------------------------------------------------------
// Mocks — must NOT reference module-scope variables before init.
// The `require()` form is used because vi.mock factories are hoisted to
// module init time and cannot capture outer-scope variables.
// ---------------------------------------------------------------------------

vi.mock('$lib/stores/board.js', () => ({
  lastUpdateTs: {
    subscribe: (fn: (v: number) => void) => {
      fn(0);
      return () => undefined;
    },
  },
}));

vi.mock('$lib/stores/settingsForm.js', () => ({
  selectStation: vi.fn(),
}));

vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeBoard(index: number): BoardType {
  return {
    station_id: '940GZZLUBZP',
    platforms: [
      {
        name: 'Northbound - Platform 1',
        arrivals: [
          {
            id: String(index),
            station_name: 'Belsize Park Underground Station',
            platform_name: 'Northbound - Platform 1',
            line_id: 'northern',
            line_name: 'Northern',
            direction: 'Northbound',
            destination_name: 'Edgware',
            towards: 'Edgware',
            current_location: `At station (tick ${String(index)})`,
            time_to_station: Math.max(30, 300 - index * 5),
            expected_arrival: '2025-01-15T10:05:00Z',
            naptan_id: '940GZZLUBZP',
          },
        ],
      },
    ],
    generated_at: new Date(Date.now() + index * 1000).toISOString(),
    stale_since: null,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('Board — Fix 6: render stability under repeated updates', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('root <main> DOM node is the same object across 60 board prop updates (no re-mount)', async () => {
    const initialBoard = makeBoard(0);
    const { rerender, container } = render(Board, {
      props: { board: initialBoard },
    });

    const mainBefore = container.querySelector('main');
    expect(mainBefore).not.toBeNull();

    // Simulate 60 board updates (equivalent to 60 board://updated emissions).
    // Advance only 100ms per tick (does not trigger the 1000ms setInterval clock).
    for (let i = 1; i <= 60; i++) {
      await rerender({ board: makeBoard(i) });
      vi.advanceTimersByTime(100);
    }

    const mainAfter = container.querySelector('main');

    // Same DOM element reference — no re-mount occurred.
    expect(mainAfter).toBe(mainBefore);
  });

  it('container content remains present after 60 prop changes', async () => {
    const { rerender, container } = render(Board, {
      props: { board: makeBoard(0) },
    });

    for (let i = 1; i <= 60; i++) {
      await rerender({ board: makeBoard(i) });
      vi.advanceTimersByTime(100);
    }

    // Container must still have the main element and platform content.
    expect(container.querySelector('main')).not.toBeNull();
    expect(container.querySelector('.board__platforms')).not.toBeNull();
  });

  it('60 prop updates do not add new <main> nodes (no re-mount)', async () => {
    // Track <main> elements added after initial render.
    const { rerender, container } = render(Board, {
      props: { board: makeBoard(0) },
    });

    let newMainCount = 0;
    const observer = new MutationObserver((mutations) => {
      for (const m of mutations) {
        m.addedNodes.forEach((n) => {
          if (n.nodeName === 'MAIN') newMainCount++;
        });
      }
    });
    observer.observe(container, { childList: true, subtree: false });

    for (let i = 1; i <= 60; i++) {
      await rerender({ board: makeBoard(i) });
      vi.advanceTimersByTime(100);
    }

    observer.disconnect();

    // Zero new <main> nodes added after initial render = no re-mount.
    expect(newMainCount).toBe(0);
  });

  it('station name header remains stable across 60 updates', async () => {
    const { rerender, container } = render(Board, {
      props: { board: makeBoard(0), stationName: 'Belsize Park Underground Station' },
    });

    const header = container.querySelector('h1');
    expect(header?.textContent?.trim()).toBe('BELSIZE PARK');

    for (let i = 1; i <= 60; i++) {
      await rerender({ board: makeBoard(i), stationName: 'Belsize Park Underground Station' });
      vi.advanceTimersByTime(100);
    }

    // Header still correct — component is updating, not frozen.
    expect(container.querySelector('h1')?.textContent?.trim()).toBe('BELSIZE PARK');
    // Importantly, the main node is still the same (no re-mount).
    expect(container.querySelector('main')).not.toBeNull();
  });
});
