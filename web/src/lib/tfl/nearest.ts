/**
 * Pure ranking helpers for "find nearest station" — port of
 * `tfl_client::nearest`.
 *
 * Operates on the same `Station` shape the stop-points cache holds; touches no
 * network, cache, or filesystem. `rankNearest` takes stations and returns a
 * ranked subset, period.
 *
 * Deliberately does NOT apply the NaPTAN-prefix whitelist / hub dedupe (that is
 * the cache layer's `whitelist_and_dedupe`, run *before* this) nor the 1.3×
 * walking-distance fudge factor (the renderer scales the raw geodesic
 * `distance_m` at format time).
 */

import type { NearbyStation, Station } from '$lib/ipc/types.js';

/**
 * Hard cap on how far a station may be from the query and still rank. Picked so
 * an out-of-network query (Paris, Manchester, …) returns an empty list rather
 * than an arbitrarily-distant Heathrow row. London's surfaced network reaches
 * ~24 km from centre (Amersham, Upminster); 25 km covers it without inviting
 * Reading.
 */
export const MAX_RADIUS_M = 25_000;

/** Mean Earth radius in metres (haversine at city scale). */
const EARTH_RADIUS_M = 6_371_000;

function toRadians(deg: number): number {
  return (deg * Math.PI) / 180;
}

/**
 * Great-circle distance between two `(lat, lon)` pairs in metres (haversine).
 * Inputs in degrees. Plenty of precision for ranking at city scale.
 */
export function haversineM(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const phi1 = toRadians(lat1);
  const phi2 = toRadians(lat2);
  const dphi = toRadians(lat2 - lat1);
  const dlambda = toRadians(lon2 - lon1);
  const a = Math.sin(dphi / 2) ** 2 + Math.cos(phi1) * Math.cos(phi2) * Math.sin(dlambda / 2) ** 2;
  const c = 2 * Math.asin(Math.sqrt(a));
  return EARTH_RADIUS_M * c;
}

/**
 * Rank stations by haversine distance from `(lat, lon)`, drop anything beyond
 * {@link MAX_RADIUS_M}, and return the closest `limit` in ascending-distance
 * order. Stations at exactly `(0, 0)` are treated as missing coordinates and
 * skipped — TfL occasionally serves a zeroed location, and "Null Island" must
 * never rank ahead of a real station on an out-of-network query.
 */
export function rankNearest(
  stations: Iterable<Station>,
  lat: number,
  lon: number,
  limit: number,
): NearbyStation[] {
  const scored: NearbyStation[] = [];
  for (const s of stations) {
    if (s.lat === 0 && s.lon === 0) continue;
    const distance_m = haversineM(lat, lon, s.lat, s.lon);
    if (distance_m <= MAX_RADIUS_M) {
      scored.push({ station: s, distance_m });
    }
  }
  // Stable ascending sort (ES2019+); haversine never yields NaN for finite
  // inputs, so a plain numeric comparator matches Rust's `total_cmp`.
  scored.sort((a, b) => a.distance_m - b.distance_m);
  return scored.slice(0, limit);
}
