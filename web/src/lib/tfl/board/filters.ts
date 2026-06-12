/**
 * Pure arrival filters — ported from `crates/tfl-board/src/filter.rs` and the
 * defensive filters in `service.rs`. No IO; every function takes and returns a
 * plain `Arrival[]`. The `BoardService` orchestrates them and supplies the
 * allowed-line set (so this module stays decoupled from the cache layer).
 *
 * `line_ids` is deliberately NOT filtered here — the chip filter is a
 * frontend-only display mask (`Board.svelte`) so a toggle updates the board in a
 * frame instead of waiting for the next tick (invariants #3 / #22).
 */

import type { Arrival, BoardConfig, Direction } from '$lib/ipc/types.js';
import { lineFamilyKey } from '../domain/lines.js';
import type { CompassAxis } from '../domain/direction.js';
import { lineCompassAxis } from '../domain/direction.js';

/**
 * Apply the `directions` filter (the only preference filter kept server-side —
 * direction toggles are infrequent, #3). An empty list passes everything.
 * `line_ids` is intentionally a no-op (#22).
 */
export function applyFilters(arrivals: Arrival[], cfg: BoardConfig): Arrival[] {
  if (cfg.directions.length === 0) return arrivals;
  return arrivals.filter((a) => cfg.directions.includes(a.direction));
}

/**
 * Drop arrivals whose line isn't in the station's allowed set — a defensive
 * integrity filter against TfL surfacing a prediction for a line that doesn't
 * physically serve the station (typically a hub-merge leak). Compares on the
 * line *family* (`lineFamilyKey`) so a Windrush train survives at a station
 * whose metadata only advertised Mildmay, while a cross-mode phantom (a tube
 * line at an Overground station) is still dropped (#10). **Fail-open**: an empty
 * `allowed` set (cold cache) passes everything — dropping a real arrival is
 * worse than letting one phantom through.
 */
export function dropArrivalsForLinesNotServing(
  allowed: ReadonlySet<string>,
  stationId: string,
  arrivals: Arrival[],
): Arrival[] {
  if (allowed.size === 0) return arrivals;

  const allowedFamilies = new Set([...allowed].map(lineFamilyKey));
  const warned = new Set<string>();
  const kept: Arrival[] = [];
  for (const arrival of arrivals) {
    if (allowedFamilies.has(lineFamilyKey(arrival.line_id))) {
      kept.push(arrival);
      continue;
    }
    if (!warned.has(arrival.line_id)) {
      warned.add(arrival.line_id);
      console.warn(
        `[tfl-board] dropping arrival for line ${arrival.line_id} at ${stationId}: not in the station's allowed set`,
      );
    }
  }
  return kept;
}

/**
 * Drop predictions whose compass direction is off the line's axis at this
 * station. Each line runs one axis (N/S or E/W) at a given station; an off-axis
 * prediction is a TfL data quirk (an unsigned starter train parked on a sibling
 * line's platform). The axis is pinned per refresh: a network-wide
 * {@link lineCompassAxis} override wins, else the dominant platform-prefix axis
 * (strict majority, ≥3×); a tie or no signal skips the line (fail-open).
 */
export function dropOffAxisPredictions(arrivals: Arrival[]): Arrival[] {
  const prefixCounts = new Map<string, { ns: number; ew: number }>();
  for (const arrival of arrivals) {
    const pl = arrival.platform_name.toLowerCase();
    const counts = prefixCounts.get(arrival.line_id) ?? { ns: 0, ew: 0 };
    if (pl.startsWith('northbound') || pl.startsWith('southbound')) counts.ns += 1;
    else if (pl.startsWith('eastbound') || pl.startsWith('westbound')) counts.ew += 1;
    prefixCounts.set(arrival.line_id, counts);
  }

  const axisForLine = new Map<string, CompassAxis>();
  for (const [lineId, { ns, ew }] of prefixCounts) {
    const override = lineCompassAxis(lineId);
    if (override !== null) {
      axisForLine.set(lineId, override);
      continue;
    }
    const inferred = inferAxis(ns, ew);
    if (inferred !== null) axisForLine.set(lineId, inferred);
  }

  const warned = new Set<string>();
  const kept: Arrival[] = [];
  for (const arrival of arrivals) {
    const axis = axisForLine.get(arrival.line_id);
    if (axis === undefined || directionMatchesAxis(axis, arrival.direction)) {
      kept.push(arrival);
      continue;
    }
    const key = `${arrival.line_id}|${arrival.direction}`;
    if (!warned.has(key)) {
      warned.add(key);
      console.warn(
        `[tfl-board] dropping off-axis prediction line=${arrival.line_id} direction=${arrival.direction} (axis ${axis})`,
      );
    }
  }
  return kept;
}

/**
 * Drop predictions whose destination is the queried station itself. At a
 * terminus every inbound prediction has `destination_name === station_name`;
 * showing "Northbound: Edgware" at Edgware is a tautology. Fully data-driven
 * (case-insensitive trimmed compare); fail-open when either field is empty
 * (#24).
 */
export function dropArrivalsTerminatingAtQueriedStation(arrivals: Arrival[]): Arrival[] {
  return arrivals.filter((a) => {
    const lhs = a.station_name.trim().toLowerCase();
    const rhs = a.destination_name.trim().toLowerCase();
    if (lhs === '' || rhs === '') return true;
    return lhs !== rhs;
  });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Infer a line's axis from its platform-prefix counts. The `ns + ew === 0` guard
 * comes first: without it a zero-signal line (Overground/Elizabeth emit bare
 * `"Platform N"`) would fall through to `ns >= ew*3` (`0 >= 0`) and be falsely
 * pinned N/S. A strict majority is then required: 1-vs-0 drops the phantom,
 * 1-vs-1 passes through, 3-to-1 pins.
 */
function inferAxis(ns: number, ew: number): CompassAxis | null {
  if (ns + ew === 0) return null;
  if (ew === 0) return 'NorthSouth';
  if (ns === 0) return 'EastWest';
  if (ns >= ew * 3) return 'NorthSouth';
  if (ew >= ns * 3) return 'EastWest';
  return null;
}

/** Does `direction` lie on `axis`? Inbound/Outbound/Unknown never do. */
function directionMatchesAxis(axis: CompassAxis, direction: Direction): boolean {
  if (axis === 'EastWest') return direction === 'Eastbound' || direction === 'Westbound';
  return direction === 'Northbound' || direction === 'Southbound';
}
