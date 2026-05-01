// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import ArrivalRow from '$lib/components/ArrivalRow.svelte';
import type { Arrival } from '$lib/ipc/types.js';

vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

// Wall-clock anchor for these tests. Every test sets fake time to this value
// and constructs `expected_arrival` strings relative to it. `time_to_station`
// in the fixture is intentionally stale (the production bug: it's frozen
// between polls) — the live tick must NOT trust it.
const NOW_MS = Date.parse('2025-01-15T10:00:00Z');

function arrivalAt(offsetSec: number, overrides: Partial<Arrival> = {}): Arrival {
  return {
    id: '1',
    station_name: 'Belsize Park Underground Station',
    platform_name: 'Northbound - Platform 1',
    line_id: 'northern',
    line_name: 'Northern',
    direction: 'Northbound',
    destination_name: 'Edgware',
    towards: 'Edgware via CX',
    current_location: 'Approaching Belsize Park',
    // Stale on purpose — production sends this value frozen between polls.
    // The live tick should be driven by `expected_arrival`, not this.
    time_to_station: 9999,
    expected_arrival: new Date(NOW_MS + offsetSec * 1000).toISOString(),
    naptan_id: '940GZZLUBZP',
    ...overrides,
  };
}

describe('ArrivalRow — live ticking minutes', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW_MS);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('ticks down every second using expected_arrival as the anchor', async () => {
    // 90s out → "1 min" (formatTimeToStation: <90s → "1 min")
    // We pick 91s so the boundary is unambiguous (91 >= 90 → "1 min", but
    // really we want >90 to render "1 min" not "2 mins"). The format fn:
    //   <30 -> Due, <90 -> "1 min", else "N mins" with floor(s/60).
    // 91s -> "1 mins" (since floor(91/60)=1)
    render(ArrivalRow, { props: { arrival: arrivalAt(91), rank: 1 } });
    await tick();

    // Initially renders "1 mins" (floor(91/60))
    expect(screen.getByText('1 mins')).toBeTruthy();

    // Advance wall-clock by 60s without re-emitting the board.
    // The live derivation should now compute (91 - 60) = 31s remaining.
    // 31s is in the (<90 && >=30) bucket -> "1 min".
    await vi.advanceTimersByTimeAsync(60_000);
    await tick();
    expect(screen.getByText('1 min')).toBeTruthy();

    // Advance another 5s -> 26s remaining -> "Due".
    await vi.advanceTimersByTimeAsync(5_000);
    await tick();
    expect(screen.getByText('Due')).toBeTruthy();
  });

  it('keeps showing "Due" past expected_arrival (no client-side drop)', async () => {
    render(ArrivalRow, { props: { arrival: arrivalAt(10), rank: 1 } });
    await tick();
    expect(screen.getByText('Due')).toBeTruthy();

    // Way past expected_arrival
    await vi.advanceTimersByTimeAsync(120_000);
    await tick();
    expect(screen.getByText('Due')).toBeTruthy();
  });

  it('falls back to time_to_station when expected_arrival is malformed', async () => {
    const malformed = arrivalAt(0, {
      expected_arrival: 'not-a-date',
      time_to_station: 300,
    });
    render(ArrivalRow, { props: { arrival: malformed, rank: 1 } });
    await tick();

    // 300s -> "5 mins"
    expect(screen.getByText('5 mins')).toBeTruthy();

    // No throw on tick
    await vi.advanceTimersByTimeAsync(1_000);
    await tick();
  });

  it('is poll-interval agnostic — ticking does not depend on poll_seconds', async () => {
    // 121s out → "2 mins". After 60s of fake-clock advance with no new
    // board emit, we should be at "1 min" — independent of any poll
    // interval.
    render(ArrivalRow, { props: { arrival: arrivalAt(121), rank: 1 } });
    await tick();
    expect(screen.getByText('2 mins')).toBeTruthy();

    await vi.advanceTimersByTimeAsync(60_000);
    await tick();
    expect(screen.getByText('1 min')).toBeTruthy();
  });
});
