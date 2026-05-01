// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import ArrivalRow from '$lib/components/ArrivalRow.svelte';
import type { Arrival } from '$lib/ipc/types.js';

vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

function arrival(overrides: Partial<Arrival> = {}): Arrival {
  return {
    id: '1',
    station_name: 'Baker Street Underground Station',
    platform_name: 'Platform 4',
    line_id: 'metropolitan',
    line_name: 'Metropolitan',
    direction: 'Northbound',
    destination_name: 'Amersham',
    towards: 'Amersham',
    current_location: '',
    time_to_station: 120,
    expected_arrival: new Date(Date.now() + 120_000).toISOString(),
    naptan_id: '940GZZLUBST',
    ...overrides,
  };
}

describe('ArrivalRow — compact platform column', () => {
  it('renders just the platform identifier (no "Platform " prefix)', () => {
    render(ArrivalRow, { props: { arrival: arrival(), rank: 1 } });
    const li = screen.getByRole('listitem');
    const cell = li.querySelector('.arrival-row__platform');
    expect(cell?.textContent?.trim()).toBe('4');
    // The redundant word "Platform" never appears on the row — the
    // column header tells the user what the digit means.
    expect(li.textContent ?? '').not.toMatch(/Platform 4/);
  });

  it('strips both the "<direction> - " prefix and the "Platform " literal', () => {
    // TfL often returns "Northbound - Platform 1" for tube stops.
    render(ArrivalRow, {
      props: {
        arrival: arrival({ platform_name: 'Northbound - Platform 1' }),
        rank: 1,
      },
    });
    const li = screen.getByRole('listitem');
    const cell = li.querySelector('.arrival-row__platform');
    expect(cell?.textContent?.trim()).toBe('1');
    // The "Northbound" prefix isn't repeated on the row — it's the
    // column header.
    expect((li.textContent ?? '').match(/Northbound/g)?.length ?? 0).toBeLessThanOrEqual(1);
  });

  it('hides the cell when platform_name equals the direction label', () => {
    // Some single-platform stops have TfL returning just the direction
    // string as the platform name. Rendering "Northbound" under PLAT
    // is noise — drop it.
    render(ArrivalRow, {
      props: { arrival: arrival({ platform_name: 'Northbound' }), rank: 1 },
    });
    const li = screen.getByRole('listitem');
    // The cell renders empty (kept for grid alignment) — assert nothing
    // visible inside it.
    const cell = li.querySelector('.arrival-row__platform');
    expect((cell?.textContent ?? '').trim()).toBe('');
  });

  it('hides the cell when platform_name is empty / whitespace', () => {
    render(ArrivalRow, {
      props: { arrival: arrival({ platform_name: '   ' }), rank: 1 },
    });
    const li = screen.getByRole('listitem');
    const cell = li.querySelector('.arrival-row__platform');
    expect((cell?.textContent ?? '').trim()).toBe('');
  });

  it('hides the cell when platform_name is the bare word "Platform" with no number', () => {
    render(ArrivalRow, {
      props: { arrival: arrival({ platform_name: 'Platform' }), rank: 1 },
    });
    const li = screen.getByRole('listitem');
    const cell = li.querySelector('.arrival-row__platform');
    expect((cell?.textContent ?? '').trim()).toBe('');
  });
});
