/**
 * Board filters, ported from `filter.rs` + the defensive filters in `service.rs`:
 * directions-only filter with `line_ids` a no-op (#3/#22), not-serving filter on
 * the line family (#10), off-axis drop, and terminating-at-station drop (#24).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BoardConfig } from '$lib/ipc/types.js';
import {
  applyFilters,
  dropArrivalsForLinesNotServing,
  dropArrivalsTerminatingAtQueriedStation,
  dropOffAxisPredictions,
} from './filters.js';
import { makeArrival } from './arrivalFixture.js';

function cfg(over: Partial<BoardConfig> = {}): BoardConfig {
  return {
    station_id: 'TEST',
    line_ids: [],
    directions: [],
    poll_seconds: 20,
    theme: 'classic-amber',
    ...over,
  };
}

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe('applyFilters', () => {
  it('passes everything when directions is empty', () => {
    const arrivals = [
      makeArrival({ direction: 'Northbound' }),
      makeArrival({ direction: 'Southbound' }),
    ];
    expect(applyFilters(arrivals, cfg())).toHaveLength(2);
  });

  it('keeps only the selected directions', () => {
    const arrivals = [
      makeArrival({ direction: 'Northbound' }),
      makeArrival({ direction: 'Southbound' }),
      makeArrival({ direction: 'Northbound' }),
    ];
    expect(applyFilters(arrivals, cfg({ directions: ['Northbound'] }))).toHaveLength(2);
  });

  it('treats line_ids as a no-op (frontend-only display mask, #22)', () => {
    const arrivals = [
      makeArrival({ line_id: 'northern' }),
      makeArrival({ line_id: 'victoria' }),
      makeArrival({ line_id: 'piccadilly' }),
    ];
    // User narrowed to northern on the frontend; the backend must pass all.
    expect(applyFilters(arrivals, cfg({ line_ids: ['northern'] }))).toHaveLength(3);
  });
});

describe('dropArrivalsForLinesNotServing', () => {
  it('fails open (passes everything) when the allowed set is empty', () => {
    const arrivals = [makeArrival({ line_id: 'bakerloo' })];
    expect(dropArrivalsForLinesNotServing(new Set(), 'TEST', arrivals)).toHaveLength(1);
  });

  it('keeps a sibling Overground line via the family key (#10)', () => {
    // Station metadata advertised only Mildmay; a Windrush train must survive.
    const arrivals = [makeArrival({ line_id: 'windrush' })];
    expect(dropArrivalsForLinesNotServing(new Set(['mildmay']), 'TEST', arrivals)).toHaveLength(1);
  });

  it('drops a cross-mode phantom (different family) and warns once', () => {
    const arrivals = [makeArrival({ line_id: 'bakerloo' }), makeArrival({ line_id: 'windrush' })];
    const kept = dropArrivalsForLinesNotServing(new Set(['mildmay']), 'TEST', arrivals);
    expect(kept.map((a) => a.line_id)).toEqual(['windrush']);
    // The drop warning is our only signal of upstream data drift (#10).
    expect(console.warn).toHaveBeenCalledTimes(1);
  });
});

describe('dropOffAxisPredictions', () => {
  it('drops an off-axis prediction on a network-pinned line (Central is E/W)', () => {
    const arrivals = [
      makeArrival({ line_id: 'central', direction: 'Eastbound' }),
      makeArrival({ line_id: 'central', direction: 'Northbound' }), // phantom
    ];
    const kept = dropOffAxisPredictions(arrivals);
    expect(kept.map((a) => a.direction)).toEqual(['Eastbound']);
  });

  it('infers the axis from a dominant platform prefix and drops the minority phantom', () => {
    // Metropolitan is not network-pinned; 3 NB vs 1 WB → N/S, so WB is off-axis.
    const arrivals = [
      makeArrival({
        line_id: 'metropolitan',
        direction: 'Northbound',
        platform_name: 'Northbound - Platform 1',
      }),
      makeArrival({
        line_id: 'metropolitan',
        direction: 'Northbound',
        platform_name: 'Northbound - Platform 1',
      }),
      makeArrival({
        line_id: 'metropolitan',
        direction: 'Southbound',
        platform_name: 'Southbound - Platform 2',
      }),
      makeArrival({
        line_id: 'metropolitan',
        direction: 'Westbound',
        platform_name: 'Westbound - Platform 3',
      }),
    ];
    const kept = dropOffAxisPredictions(arrivals);
    expect(kept).toHaveLength(3);
    expect(kept.every((a) => a.direction !== 'Westbound')).toBe(true);
  });

  it('passes a line through when there is no axis signal (tie or no prefix)', () => {
    const arrivals = [
      makeArrival({ line_id: 'mildmay', direction: 'Inbound', platform_name: 'Platform 1' }),
      makeArrival({ line_id: 'mildmay', direction: 'Outbound', platform_name: 'Platform 2' }),
    ];
    expect(dropOffAxisPredictions(arrivals)).toHaveLength(2);
  });

  it('pins the axis from a lone compass prefix (1-vs-0) and drops a bare-platform phantom', () => {
    // One NB-prefixed metropolitan arrival pins N/S; a peer on an unlabelled
    // platform tagged Westbound is the phantom the axis pin is there to catch.
    const arrivals = [
      makeArrival({
        line_id: 'metropolitan',
        direction: 'Northbound',
        platform_name: 'Northbound - Platform 1',
      }),
      makeArrival({ line_id: 'metropolitan', direction: 'Westbound', platform_name: 'Platform 3' }),
    ];
    const kept = dropOffAxisPredictions(arrivals);
    expect(kept.map((a) => a.direction)).toEqual(['Northbound']);
  });

  it('keeps an Unknown-direction prediction on a pinned line (routes to Other, not dropped)', () => {
    // Circle is network-pinned E/W. A loop train on an "Inner Rail" platform
    // has no compass direction (Unknown) — it is a real train, not an off-axis
    // phantom, so it must survive (the board buckets it as "Other"). Only a
    // definite *perpendicular* compass direction is a phantom worth dropping.
    const arrivals = [
      makeArrival({
        line_id: 'circle',
        direction: 'Eastbound',
        platform_name: 'Eastbound - Platform 2',
      }),
      makeArrival({
        line_id: 'circle',
        direction: 'Unknown',
        platform_name: 'Inner Rail - Platform 1',
      }),
    ];
    const kept = dropOffAxisPredictions(arrivals);
    expect(kept).toHaveLength(2);
    expect(kept.some((a) => a.direction === 'Unknown')).toBe(true);
  });

  it('drops an Unknown phantom whose platform carries a perpendicular compass prefix', () => {
    // An H&C/Circle train mis-parked on a "Northbound" platform at an E/W
    // station infers Unknown (the prefix is gated out by the line's axis), but
    // the platform prefix marks it as the sibling-platform phantom — drop it,
    // unlike the bare-platform loop train above.
    const arrivals = [
      makeArrival({
        line_id: 'circle',
        direction: 'Eastbound',
        platform_name: 'Eastbound - Platform 2',
      }),
      makeArrival({
        line_id: 'circle',
        direction: 'Unknown',
        platform_name: 'Northbound - Platform 1',
      }),
    ];
    const kept = dropOffAxisPredictions(arrivals);
    expect(kept.map((a) => a.direction)).toEqual(['Eastbound']);
  });

  it('drops Inbound/Outbound predictions on a compass-pinned line', () => {
    // Inbound/Outbound is the wrong *kind* of direction for a compass-pinned
    // line (Circle is E/W) — the unsigned-starter phantom. Only `Unknown`
    // (no signal) gets the bare-platform benefit of the doubt, not in/out.
    const arrivals = [
      makeArrival({ line_id: 'circle', direction: 'Eastbound' }),
      makeArrival({ line_id: 'circle', direction: 'Inbound', platform_name: 'Platform 1' }),
      makeArrival({ line_id: 'circle', direction: 'Outbound', platform_name: 'Platform 2' }),
    ];
    expect(dropOffAxisPredictions(arrivals).map((a) => a.direction)).toEqual(['Eastbound']);
  });

  it('still drops a definite perpendicular-compass phantom on a pinned line', () => {
    // The phantom-catch intent is preserved: a Northbound prediction on the
    // E/W-pinned Circle is genuinely off-axis and dropped.
    const arrivals = [
      makeArrival({ line_id: 'circle', direction: 'Eastbound' }),
      makeArrival({ line_id: 'circle', direction: 'Northbound' }),
    ];
    expect(dropOffAxisPredictions(arrivals).map((a) => a.direction)).toEqual(['Eastbound']);
  });

  it('passes both through on a 1-vs-1 prefix tie (no strict majority)', () => {
    const arrivals = [
      makeArrival({
        line_id: 'metropolitan',
        direction: 'Northbound',
        platform_name: 'Northbound - Platform 1',
      }),
      makeArrival({
        line_id: 'metropolitan',
        direction: 'Westbound',
        platform_name: 'Westbound - Platform 2',
      }),
    ];
    expect(dropOffAxisPredictions(arrivals)).toHaveLength(2);
  });
});

describe('dropArrivalsTerminatingAtQueriedStation', () => {
  it('drops an arrival whose destination is the queried station (#24)', () => {
    const arrivals = [
      makeArrival({ station_name: 'Edgware', destination_name: 'Edgware' }),
      makeArrival({ station_name: 'Edgware', destination_name: 'Kennington' }),
    ];
    const kept = dropArrivalsTerminatingAtQueriedStation(arrivals);
    expect(kept.map((a) => a.destination_name)).toEqual(['Kennington']);
  });

  it('compares case-insensitively after trimming', () => {
    const arrivals = [makeArrival({ station_name: '  Edgware ', destination_name: 'EDGWARE' })];
    expect(dropArrivalsTerminatingAtQueriedStation(arrivals)).toHaveLength(0);
  });

  it('fails open when either name is empty', () => {
    const arrivals = [makeArrival({ station_name: '', destination_name: 'Edgware' })];
    expect(dropArrivalsTerminatingAtQueriedStation(arrivals)).toHaveLength(1);
  });
});
