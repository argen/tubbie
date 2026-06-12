/**
 * `groupByPlatform` — ported from `crates/tfl-domain/src/format.rs`.
 *
 * Groups a flat arrival list by verbatim `platform_name`. Platforms appear in
 * first-seen order; within each, arrivals sort by `time_to_station` ascending.
 * `Array.prototype.sort` is stable (ES2019+), so equal-time arrivals keep their
 * insertion order — matching Rust's `sort_by_key`.
 *
 * Display formatting (`formatTimeToStation`) already lives in `utils/format.ts`
 * and is not re-ported here.
 */
import type { Arrival, Platform } from '$lib/ipc/types.js';

/** Group arrivals into platforms, sorted soonest-first within each platform. */
export function groupByPlatform(arrivals: Arrival[]): Platform[] {
  const order: string[] = [];
  const byPlatform = new Map<string, Arrival[]>();

  for (const arrival of arrivals) {
    const key = arrival.platform_name;
    let bucket = byPlatform.get(key);
    if (bucket === undefined) {
      bucket = [];
      byPlatform.set(key, bucket);
      order.push(key);
    }
    bucket.push(arrival);
  }

  return order.map((name) => {
    const items = byPlatform.get(name) ?? [];
    const sorted = [...items].sort((a, b) => a.time_to_station - b.time_to_station);
    return { name, arrivals: sorted };
  });
}
