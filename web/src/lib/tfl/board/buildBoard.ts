/**
 * `buildBoard` — group filtered arrivals into a render-ready `Board`, ported
 * from `crates/tfl-board/src/service.rs`.
 *
 * Columns are by compass `Direction` (NOT raw `platform_name`, which carries a
 * per-line suffix like "Westbound - Platform 3" that would split one compass
 * direction into multiple columns). Directions appear in a fixed reading order;
 * empty ones are omitted; arrivals within a column sort soonest-first.
 *
 * **No dedupe by `Arrival.id`.** TfL's `id` is not unique — observed at Chalk
 * Farm, ten distinct trains shared one id. Dropping by id would lose real
 * arrivals; the frontend keys on a composite instead.
 */

import type { Arrival, Board, Direction, Platform } from '$lib/ipc/types.js';

const DISPLAY_ORDER: readonly Direction[] = [
  'Northbound',
  'Southbound',
  'Eastbound',
  'Westbound',
  'Inbound',
  'Outbound',
  'Unknown',
];

/** Column label for a direction (`Unknown` renders as "Other"). */
function directionLabel(d: Direction): string {
  return d === 'Unknown' ? 'Other' : d;
}

export function buildBoard(
  stationId: string,
  arrivals: Arrival[],
  generatedAt: Date,
  staleSince: Date | null,
): Board {
  const byDirection = new Map<Direction, Arrival[]>();
  for (const arrival of arrivals) {
    const list = byDirection.get(arrival.direction) ?? [];
    list.push(arrival);
    byDirection.set(arrival.direction, list);
  }

  const platforms: Platform[] = [];
  for (const dir of DISPLAY_ORDER) {
    const group = byDirection.get(dir);
    if (group === undefined) continue;
    group.sort((a, b) => a.time_to_station - b.time_to_station);
    platforms.push({ name: directionLabel(dir), arrivals: group });
  }

  return {
    station_id: stationId,
    platforms,
    generated_at: generatedAt.toISOString(),
    stale_since: staleSince === null ? null : staleSince.toISOString(),
  };
}
