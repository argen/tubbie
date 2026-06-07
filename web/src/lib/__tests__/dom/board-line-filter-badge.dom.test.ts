// @vitest-environment happy-dom
/**
 * When the user has selected one or more lines to filter the board, the
 * Board component must surface that filter in-place so the user knows the
 * list they are seeing is scoped, not broken.
 */
import { describe, expect, it, vi, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
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

function makeBoard(): BoardType {
  return {
    station_id: '940GZZLUOXC',
    platforms: [
      {
        name: 'Northbound - Platform 1',
        arrivals: [
          {
            id: '1',
            station_name: 'Oxford Circus Underground Station',
            platform_name: 'Northbound - Platform 1',
            line_id: 'central',
            line_name: 'Central',
            direction: 'Northbound',
            destination_name: 'Ealing Broadway',
            towards: 'Ealing Broadway',
            current_location: 'At Oxford Circus',
            time_to_station: 60,
            expected_arrival: '2025-01-15T10:01:00Z',
            naptan_id: '940GZZLUOXC',
          },
        ],
      },
    ],
    generated_at: '2025-01-15T10:00:00Z',
    stale_since: null,
  };
}

describe('Board — active line-filter badge', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders a "Filtering" badge listing the selected lines', () => {
    const { container } = render(Board, {
      props: {
        board: makeBoard(),
        stationName: 'Oxford Circus Underground Station',
        lineIds: ['central', 'victoria'],
      },
    });

    const badge = container.querySelector('[data-testid="board-line-filter"]');
    expect(badge).not.toBeNull();
    const text = (badge as HTMLElement).textContent ?? '';
    expect(text).toMatch(/central/i);
    expect(text).toMatch(/victoria/i);
  });

  it('does NOT render the badge when no line filter is active', () => {
    const { container } = render(Board, {
      props: {
        board: makeBoard(),
        stationName: 'Oxford Circus Underground Station',
        lineIds: [],
      },
    });

    expect(container.querySelector('[data-testid="board-line-filter"]')).toBeNull();
  });

  it('omitting the lineIds prop is equivalent to no filter', () => {
    const { container } = render(Board, {
      props: {
        board: makeBoard(),
        stationName: 'Oxford Circus Underground Station',
      },
    });

    expect(container.querySelector('[data-testid="board-line-filter"]')).toBeNull();
  });
});
