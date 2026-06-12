import { describe, expect, it } from 'vitest';
import type { Arrival } from '$lib/ipc/types.js';
import { groupByPlatform } from '$lib/tfl/domain/format.js';

function arrival(
  partial: Partial<Arrival> & { platform_name: string; time_to_station: number },
): Arrival {
  return {
    id: partial.id ?? `${partial.platform_name}-${String(partial.time_to_station)}`,
    station_name: 'Test',
    platform_name: partial.platform_name,
    line_id: 'central',
    line_name: 'Central',
    direction: 'Eastbound',
    northern_branch: null,
    destination_name: 'End',
    towards: 'End',
    current_location: '',
    time_to_station: partial.time_to_station,
    expected_arrival: '2026-06-12T10:00:00Z',
    naptan_id: 'X',
  };
}

describe('groupByPlatform', () => {
  it('groups by platform_name and sorts each platform soonest-first', () => {
    const platforms = groupByPlatform([
      arrival({ platform_name: 'Eastbound - Platform 1', time_to_station: 300 }),
      arrival({ platform_name: 'Westbound - Platform 2', time_to_station: 120 }),
      arrival({ platform_name: 'Eastbound - Platform 1', time_to_station: 60 }),
    ]);
    expect(platforms.map((p) => p.name)).toEqual([
      'Eastbound - Platform 1',
      'Westbound - Platform 2',
    ]);
    expect(platforms[0]!.arrivals.map((a) => a.time_to_station)).toEqual([60, 300]);
  });

  it('preserves first-seen platform order and is stable for equal times', () => {
    const platforms = groupByPlatform([
      arrival({ id: 'b', platform_name: 'P2', time_to_station: 100 }),
      arrival({ id: 'a', platform_name: 'P1', time_to_station: 100 }),
      arrival({ id: 'a2', platform_name: 'P1', time_to_station: 100 }),
    ]);
    expect(platforms.map((p) => p.name)).toEqual(['P2', 'P1']);
    // Equal times keep insertion order (stable sort).
    expect(platforms[1]!.arrivals.map((a) => a.id)).toEqual(['a', 'a2']);
  });

  it('returns empty output for empty input', () => {
    expect(groupByPlatform([])).toEqual([]);
  });
});
