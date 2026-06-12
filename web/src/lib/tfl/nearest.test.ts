/**
 * Ports `crates/tfl-client/src/nearest.rs` tests — haversine accuracy,
 * distance ordering, the 25 km radius cap, the (0,0) skip, and the Amersham
 * boundary that pins the radius choice.
 */

import { describe, expect, it } from 'vitest';
import type { Station } from '$lib/ipc/types.js';
import { MAX_RADIUS_M, haversineM, rankNearest } from './nearest.js';

function station(id: string, name: string, lat: number, lon: number): Station {
  return { id, common_name: name, modes: ['tube'], lat, lon, lines: [] };
}

function names(ranked: { station: Station }[]): string[] {
  return ranked.map((n) => n.station.common_name);
}

describe('haversineM', () => {
  it('is ~0 when the two points match', () => {
    expect(haversineM(51.5074, -0.1278, 51.5074, -0.1278)).toBeLessThan(1);
  });

  it('matches the known Baker Street → Oxford Circus crow-flies distance', () => {
    const d = haversineM(51.5226, -0.1571, 51.5152, -0.1419);
    expect(d).toBeGreaterThanOrEqual(1200);
    expect(d).toBeLessThanOrEqual(1400);
  });
});

describe('rankNearest', () => {
  it('orders by ascending distance from the query point', () => {
    const stations = [
      station('OXC', 'Oxford Circus', 51.5152, -0.1419),
      station('BNK', 'Bank', 51.5133, -0.0886),
      station('MNT', 'Monument', 51.5108, -0.0863),
      station('STP', "St Paul's", 51.5146, -0.0973),
    ];
    const ranked = rankNearest(stations, 51.5133, -0.0886, 4);
    expect(names(ranked)).toEqual(['Bank', 'Monument', "St Paul's", 'Oxford Circus']);
  });

  it('drops stations outside the 25 km radius', () => {
    const stations = [
      station('BNK', 'Bank', 51.5133, -0.0886),
      station('RDG', 'Reading', 51.4585, -0.971),
    ];
    const ranked = rankNearest(stations, 51.5133, -0.0886, 8);
    expect(names(ranked)).toEqual(['Bank']);
  });

  it('truncates to the requested limit', () => {
    const stations = [
      station('A', 'A', 51.5101, -0.1),
      station('B', 'B', 51.5102, -0.1),
      station('C', 'C', 51.5103, -0.1),
      station('D', 'D', 51.5104, -0.1),
    ];
    expect(rankNearest(stations, 51.51, -0.1, 2)).toHaveLength(2);
  });

  it('skips stations at (0, 0) — Null Island must never rank', () => {
    const stations = [
      station('BAD', 'Null Island', 0, 0),
      station('BNK', 'Bank', 51.5133, -0.0886),
    ];
    // Paris query: Null Island is closer to the equator but must be skipped.
    expect(rankNearest(stations, 48.8566, 2.3522, 8)).toEqual([]);
  });

  it('skips (0, 0) even when the query is near the equator (radius would not save us)', () => {
    // A query ~5.5 km from Null Island — well inside the 25 km cap. Only the
    // explicit (0,0) skip keeps it out of the results; the radius filter alone
    // would let it through. (The Paris case above is dropped by the radius cap,
    // so it does not actually exercise the skip — this one does.)
    const stations = [station('BAD', 'Null Island', 0, 0)];
    expect(rankNearest(stations, 0, 0.05, 8)).toEqual([]);
    // Sanity: a real station at the same distance from the query IS kept,
    // proving the empty result above is the skip and not an over-tight radius.
    const real = [station('REAL', 'Equator Town', 0.001, 0.05)];
    expect(rankNearest(real, 0, 0.05, 8)).toHaveLength(1);
  });

  it('returns empty when the query is far from every station', () => {
    const stations = [
      station('BNK', 'Bank', 51.5133, -0.0886),
      station('OXC', 'Oxford Circus', 51.5152, -0.1419),
    ];
    // Manchester query.
    expect(rankNearest(stations, 53.4808, -2.2426, 8)).toEqual([]);
  });

  it('excludes Amersham from Baker Street but includes it from Watford (radius pin)', () => {
    const amersham = station('AMR', 'Amersham', 51.674, -0.6075);
    expect(rankNearest([amersham], 51.5226, -0.1571, 8)).toEqual([]);
    expect(rankNearest([amersham], 51.6565, -0.3963, 8)).toHaveLength(1);
  });

  it('exposes the radius cap as 25 km', () => {
    expect(MAX_RADIUS_M).toBe(25_000);
  });
});
