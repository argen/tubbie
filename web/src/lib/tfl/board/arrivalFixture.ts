/**
 * `makeArrival` — a domain `Arrival` factory for board tests, defaulting every
 * field so a test only sets what it cares about (direction, line, platform, …).
 * Test support only.
 */

import type { Arrival } from '$lib/ipc/types.js';

export function makeArrival(opts: Partial<Arrival> = {}): Arrival {
  return {
    id: opts.id ?? 'id',
    station_name: opts.station_name ?? 'Test Station',
    platform_name: opts.platform_name ?? 'Platform 1',
    line_id: opts.line_id ?? 'northern',
    line_name: opts.line_name ?? 'Northern',
    direction: opts.direction ?? 'Northbound',
    northern_branch: opts.northern_branch ?? null,
    destination_name: opts.destination_name ?? 'Destination',
    towards: opts.towards ?? 'Destination',
    current_location: opts.current_location ?? '',
    time_to_station: opts.time_to_station ?? 60,
    expected_arrival: opts.expected_arrival ?? '2026-01-01T00:05:00Z',
    naptan_id: opts.naptan_id ?? '940GZZLUTEST',
  };
}
