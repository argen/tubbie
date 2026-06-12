/**
 * `buildBoard`, ported from `service.rs`: group by compass direction in a fixed
 * reading order, omit empty directions, sort soonest-first, and preserve
 * distinct arrivals that share an `Arrival.id` (TfL ids aren't unique).
 */

import { describe, expect, it } from 'vitest';
import { buildBoard } from './buildBoard.js';
import { makeArrival } from './arrivalFixture.js';

const T = new Date('2026-01-01T00:00:00Z');

describe('buildBoard', () => {
  it('groups by direction in the fixed reading order, omitting empties', () => {
    const board = buildBoard(
      'TEST',
      [
        makeArrival({ direction: 'Westbound' }),
        makeArrival({ direction: 'Northbound' }),
        makeArrival({ direction: 'Unknown' }),
      ],
      T,
      null,
    );
    expect(board.platforms.map((p) => p.name)).toEqual(['Northbound', 'Westbound', 'Other']);
  });

  it('sorts arrivals within a column soonest-first', () => {
    const board = buildBoard(
      'TEST',
      [
        makeArrival({ direction: 'Northbound', time_to_station: 300, id: 'c' }),
        makeArrival({ direction: 'Northbound', time_to_station: 60, id: 'a' }),
        makeArrival({ direction: 'Northbound', time_to_station: 120, id: 'b' }),
      ],
      T,
      null,
    );
    expect(board.platforms[0]?.arrivals.map((a) => a.id)).toEqual(['a', 'b', 'c']);
  });

  it('preserves distinct arrivals that share an id (TfL ids are not unique)', () => {
    const board = buildBoard(
      'TEST',
      [
        makeArrival({
          direction: 'Northbound',
          id: 'dup',
          platform_name: 'P1',
          time_to_station: 60,
        }),
        makeArrival({
          direction: 'Northbound',
          id: 'dup',
          platform_name: 'P2',
          time_to_station: 120,
        }),
      ],
      T,
      null,
    );
    expect(board.platforms[0]?.arrivals).toHaveLength(2);
  });

  it('stamps generated_at from the clock and serialises stale_since', () => {
    const fresh = buildBoard('TEST', [], T, null);
    expect(fresh.generated_at).toBe('2026-01-01T00:00:00.000Z');
    expect(fresh.stale_since).toBeNull();

    const stale = buildBoard('TEST', [], T, new Date('2026-01-01T00:05:00Z'));
    expect(stale.stale_since).toBe('2026-01-01T00:05:00.000Z');
  });
});
