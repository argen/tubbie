// @vitest-environment happy-dom
/**
 * DOM tests for Board component station name display — Fix 2.
 *
 * Verifies:
 *   - When arrivals include station_name, the header shows the stripped name.
 *   - When arrivals are empty, the header shows station_id verbatim.
 */
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import Board from '$lib/components/Board.svelte';
import type { Board as BoardType } from '$lib/ipc/types.js';

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

const boardWithArrivals: BoardType = {
  station_id: '940GZZLUBZP',
  platforms: [
    {
      name: 'Northbound - Platform 1',
      arrivals: [
        {
          id: '1',
          station_name: 'Belsize Park Underground Station',
          platform_name: 'Northbound - Platform 1',
          line_id: 'northern',
          line_name: 'Northern',
          direction: 'Northbound',
          destination_name: 'Edgware',
          towards: 'Edgware',
          current_location: 'At station',
          time_to_station: 60,
          expected_arrival: '2025-01-15T10:01:00Z',
          naptan_id: '940GZZLUBZP',
        },
      ],
    },
  ],
  generated_at: '2025-01-15T10:00:00Z',
  stale_since: null,
};

const boardWithEmptyArrivals: BoardType = {
  station_id: '940GZZLUBZP',
  platforms: [],
  generated_at: '2025-01-15T10:00:00Z',
  stale_since: null,
};

describe('Board — Fix 2: station name display', () => {
  it('strips "Underground Station" suffix and uppercases the result', () => {
    render(Board, {
      props: {
        board: boardWithArrivals,
        stationName: 'Belsize Park Underground Station',
      },
    });
    expect(screen.getByText('BELSIZE PARK')).toBeTruthy();
  });

  it('does NOT show raw NaPTAN ID when station_name is available', () => {
    render(Board, {
      props: {
        board: boardWithArrivals,
        stationName: 'Belsize Park Underground Station',
      },
    });
    expect(screen.queryByText('940GZZLUBZP')).toBeNull();
  });

  it('falls back to station_id when stationName is empty (no arrivals)', () => {
    render(Board, {
      props: {
        board: boardWithEmptyArrivals,
        stationName: '',
      },
    });
    // board.station_id is used verbatim as the display name
    const header = screen.getByRole('heading', { level: 1 });
    expect(header.textContent?.trim()).toBe('940GZZLUBZP');
  });

  it('falls back to station_id when stationName is the NaPTAN ID itself', () => {
    // Regression: +page.svelte used to pass station_id directly as stationName.
    // The replace(' Underground Station', '') would not match '940GZZLUBZP',
    // so the raw ID appeared verbatim (uppercased). Now we use station_name
    // from arrivals so this case should not occur in production, but the
    // Board component still handles it gracefully.
    render(Board, {
      props: {
        board: boardWithEmptyArrivals,
        stationName: '940GZZLUBZP',
      },
    });
    const header = screen.getByRole('heading', { level: 1 });
    // The replace call leaves '940GZZLUBZP' as-is (no suffix to strip),
    // then uppercases it. Result is still '940GZZLUBZP'.
    expect(header.textContent?.trim()).toBe('940GZZLUBZP');
  });
});
