// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import PlatformColumn from '$lib/components/PlatformColumn.svelte';
import type { Arrival, Platform } from '$lib/ipc/types.js';

vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

/**
 * Production observation (Chalk Farm, 2026-04-27): TfL returned 10 distinct
 * predictions all with `id=1731547612`. A keyed-each on `arrival.id` threw
 * `each_key_duplicate`, the render aborted, and the desktop UI stayed stuck
 * on "Loading arrivals…" forever. Guard the composite key here so that
 * regression cannot happen again silently.
 */
describe('PlatformColumn — TfL non-unique-id payloads', () => {
  function arrival(expectedMin: number, time: number): Arrival {
    return {
      id: '1731547612', // shared sentinel id from real TfL response
      station_name: 'Chalk Farm Underground Station',
      platform_name: 'Northbound - Platform 1',
      line_id: 'northern',
      line_name: 'Northern',
      direction: 'Northbound',
      destination_name: 'Edgware',
      towards: 'Edgware via CX',
      current_location: 'Approaching Chalk Farm',
      time_to_station: time,
      expected_arrival: `2026-04-27T10:${String(expectedMin).padStart(2, '0')}:24Z`,
      naptan_id: '940GZZLUCFM',
    };
  }

  it('renders all distinct trains even when every prediction shares one id', () => {
    const platform: Platform = {
      name: 'Northbound',
      arrivals: [arrival(38, 29), arrival(44, 359), arrival(48, 599), arrival(51, 779)],
    };

    // The component used to throw `each_key_duplicate` here; if it does, the
    // render assertion below would fail (or the test runner would surface
    // the error from the unhandled rejection).
    render(PlatformColumn, { props: { platform } });

    const list = screen.getByRole('list', { name: /Arrivals for Northbound/i });
    expect(list).toBeTruthy();
    // One <li> per distinct prediction — none silently dropped.
    expect(list.querySelectorAll('li')).toHaveLength(4);
  });
});
