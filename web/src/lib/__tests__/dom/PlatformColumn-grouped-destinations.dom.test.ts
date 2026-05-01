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
 * Phase 3 of the arrival-feedback plan: an opt-in "group same destination"
 * preference (desktop-only, NOT in BoardConfig) that collapses arrivals
 * sharing a `(destination_name, towards)` key into a single row with a
 * comma-separated minutes sequence ("Edgware · 2, 5, 9 min"). Different
 * `towards` (e.g. "via Bank" vs "via CX") MUST stay split — the via-path
 * is a real distinction for users.
 */
describe('PlatformColumn — group-same-destination', () => {
  function edgware(secs: number, towards = 'Edgware via CX'): Arrival {
    return {
      id: `e-${String(secs)}`,
      station_name: 'Belsize Park Underground Station',
      platform_name: 'Northbound - Platform 1',
      line_id: 'northern',
      line_name: 'Northern',
      direction: 'Northbound',
      destination_name: 'Edgware',
      towards,
      current_location: '',
      time_to_station: secs,
      expected_arrival: new Date(Date.now() + secs * 1000).toISOString(),
      naptan_id: '940GZZLUBZP',
    };
  }

  it('collapses three Edgware-via-CX trains into one row when on', () => {
    const platform: Platform = {
      name: 'Northbound',
      arrivals: [edgware(120), edgware(300), edgware(540)],
    };
    render(PlatformColumn, { props: { platform, groupDestinations: true } });

    const list = screen.getByRole('list', { name: /Arrivals for Northbound/i });
    expect(list.querySelectorAll('li')).toHaveLength(1);
    // The ticking minutes are computed live from `expected_arrival` per
    // Phase 1, so we assert against "2, 5, 9 min" directly — that's the
    // user-visible payload.
    const li = list.querySelector('li');
    const text = li?.textContent ?? '';
    expect(text).toMatch(/Edgware/);
    expect(text).toMatch(/2.*5.*9.*min/);
  });

  it('renders one row per arrival when off', () => {
    const platform: Platform = {
      name: 'Northbound',
      arrivals: [edgware(120), edgware(300), edgware(540)],
    };
    render(PlatformColumn, { props: { platform, groupDestinations: false } });

    const list = screen.getByRole('list', { name: /Arrivals for Northbound/i });
    expect(list.querySelectorAll('li')).toHaveLength(3);
  });

  it('keeps different `towards` paths split even when grouping is on', () => {
    // Same destination "Edgware" but distinct via-paths ("via CX" vs "via Bank")
    // — at Camden Town the user genuinely needs to see both as separate rows.
    const platform: Platform = {
      name: 'Northbound',
      arrivals: [
        edgware(120, 'Edgware via CX'),
        edgware(180, 'Edgware via Bank'),
        edgware(420, 'Edgware via CX'),
      ],
    };
    render(PlatformColumn, { props: { platform, groupDestinations: true } });

    const list = screen.getByRole('list', { name: /Arrivals for Northbound/i });
    expect(list.querySelectorAll('li')).toHaveLength(2);
  });
});
