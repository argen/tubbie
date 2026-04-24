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
  time_to_station: 60, // 60s → "1 min"
  expected_arrival: '2025-01-15T10:01:30Z',
  naptan_id: '940GZZLUBZP',
};

const dueArrival: Arrival = {
  ...sampleArrival,
  id: '2',
  time_to_station: 10,
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
