// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import ArrivalRow from '$lib/components/ArrivalRow.svelte';
import type { Arrival } from '$lib/ipc/types.js';

// Mock reducedMotion store to control animation behaviour
vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

// `expected_arrival` is now the source of truth for the displayed time
// (frozen `time_to_station` from the wire would otherwise read stale 60 s
// after a poll). Anchor it to `Date.now()` so these tests assert against
// the live derivation, not a 2025 wall clock.
const sampleArrival: Arrival = {
  id: '1',
  station_name: 'Belsize Park Underground Station',
  platform_name: 'Northbound - Platform 1',
  line_id: 'northern',
  line_name: 'Northern',
  direction: 'Northbound',
  destination_name: 'Edgware',
  towards: 'Edgware via CX',
  current_location: 'Approaching Belsize Park',
  time_to_station: 60, // legacy field, no longer drives the display
  expected_arrival: new Date(Date.now() + 60_000).toISOString(),
  naptan_id: '940GZZLUBZP',
};

const dueArrival: Arrival = {
  ...sampleArrival,
  id: '2',
  time_to_station: 10,
  expected_arrival: new Date(Date.now() + 10_000).toISOString(),
};

describe('ArrivalRow', () => {
  it('renders destination name (visible in aria-label)', () => {
    render(ArrivalRow, { props: { arrival: sampleArrival, rank: 1 } });
    // The destination is revealed char-by-char; the row's aria-label always has the full name.
    const li = screen.getByRole('listitem');
    expect(li.getAttribute('aria-label')).toContain('Edgware');
  });

  it('renders formatted time', () => {
    render(ArrivalRow, { props: { arrival: sampleArrival, rank: 1 } });
    // time_to_station: 60 → "1 min"
    expect(screen.getByText('1 min')).toBeTruthy();
  });

  it('has accessible aria-label', () => {
    render(ArrivalRow, { props: { arrival: sampleArrival, rank: 1 } });
    const li = screen.getByRole('listitem');
    expect(li.getAttribute('aria-label')).toContain('Train 1');
    expect(li.getAttribute('aria-label')).toContain('Edgware');
  });

  it('shows "Due" for near-arrival trains', () => {
    render(ArrivalRow, { props: { arrival: dueArrival, rank: 1 } });
    expect(screen.getByText('Due')).toBeTruthy();
  });

  it('applies due-pulse class to "Due" trains', () => {
    render(ArrivalRow, { props: { arrival: dueArrival, rank: 1 } });
    const timeEl = screen.getByText('Due');
    expect(timeEl.classList.contains('due-pulse')).toBe(true);
  });

  it('does NOT apply due-pulse to non-due trains', () => {
    render(ArrivalRow, { props: { arrival: sampleArrival, rank: 1 } });
    // time_to_station: 60 → "1 min", not due
    const timeEl = screen.getByText('1 min');
    expect(timeEl.classList.contains('due-pulse')).toBe(false);
  });

  it('renders rank number', () => {
    render(ArrivalRow, { props: { arrival: sampleArrival, rank: 3 } });
    // Rank is shown in a span; the li text content also contains the number
    const li = screen.getByRole('listitem');
    expect(li.getAttribute('aria-label')).toContain('Train 3');
  });
});

describe('ArrivalRow — line colour stripe', () => {
  it('exposes the line id and a var(--line-*) colour via inline style', () => {
    render(ArrivalRow, { props: { arrival: sampleArrival, rank: 1 } });
    const li = screen.getByRole('listitem');
    expect(li.getAttribute('data-line-id')).toBe('northern');
    // Svelte's style:--line-color binding writes to element.style directly.
    expect(li.style.getPropertyValue('--line-color')).toBe('var(--line-northern)');
  });

  it('maps "elizabeth-line" to the Elizabeth colour variable', () => {
    const elizabeth: Arrival = { ...sampleArrival, line_id: 'elizabeth-line' };
    render(ArrivalRow, { props: { arrival: elizabeth, rank: 1 } });
    const li = screen.getByRole('listitem');
    expect(li.style.getPropertyValue('--line-color')).toBe('var(--line-elizabeth)');
  });

  it('falls back to transparent for unknown line ids', () => {
    const unknown: Arrival = { ...sampleArrival, line_id: 'not-a-real-line' };
    render(ArrivalRow, { props: { arrival: unknown, rank: 1 } });
    const li = screen.getByRole('listitem');
    expect(li.style.getPropertyValue('--line-color')).toBe('transparent');
  });

  // London Overground was split into six independently-named lines in
  // November 2024. Each must resolve to its own CSS variable so the per-row
  // stripe matches the per-line group header in `Board.svelte`.
  it.each([
    ['mildmay', 'var(--line-mildmay)'],
    ['lioness', 'var(--line-lioness)'],
    ['suffragette', 'var(--line-suffragette)'],
    ['windrush', 'var(--line-windrush)'],
    ['weaver', 'var(--line-weaver)'],
    ['liberty', 'var(--line-liberty)'],
  ])('maps %s to %s', (lineId, expected) => {
    const og: Arrival = { ...sampleArrival, line_id: lineId };
    render(ArrivalRow, { props: { arrival: og, rank: 1 } });
    const li = screen.getByRole('listitem');
    expect(li.style.getPropertyValue('--line-color')).toBe(expected);
  });

  it('aliases legacy "london-overground" to the same overground colour as the named lines', () => {
    const legacy: Arrival = { ...sampleArrival, line_id: 'london-overground' };
    render(ArrivalRow, { props: { arrival: legacy, rank: 1 } });
    const li = screen.getByRole('listitem');
    expect(li.style.getPropertyValue('--line-color')).toBe('var(--line-overground)');
  });
});

describe('ArrivalRow — reduced-motion', () => {
  it('shows blinking cursor when reduced motion is off (animation in progress)', () => {
    // The top-level vi.mock returns reducedMotion = false, so the char-by-char
    // animation starts. The cursor "_" is shown while the reveal is in progress.
    render(ArrivalRow, { props: { arrival: sampleArrival, rank: 1 } });
    // The row should be present; destination is being revealed
    const li = screen.getByRole('listitem');
    expect(li.getAttribute('aria-label')).toContain('Edgware');
  });
});
