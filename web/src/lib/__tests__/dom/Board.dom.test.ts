// @vitest-environment happy-dom
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import Board from '$lib/components/Board.svelte';
import { sampleBoard, sampleStaleBoard } from '$lib/ipc/mock.js';

// Mock board store to prevent side-effects
vi.mock('$lib/stores/board.js', () => ({
  lastUpdateTs: {
    subscribe: (fn: (v: number) => void) => {
      fn(0);
      return () => undefined;
    },
  },
}));

vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

describe('Board', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders a board without crashing', () => {
    render(Board, { props: { board: sampleBoard } });
    // Check the main element is present
    expect(screen.getByRole('main')).toBeTruthy();
  });

  it('has correct aria-label on main element', () => {
    render(Board, {
      props: { board: sampleBoard, stationName: '940GZZLUBZP' },
    });
    const main = screen.getByRole('main');
    expect(main.getAttribute('aria-label')).toContain('Arrivals board');
  });

  it('shows station name in header', () => {
    render(Board, {
      props: { board: sampleBoard, stationName: 'Belsize Park Underground Station' },
    });
    expect(screen.getByText('BELSIZE PARK')).toBeTruthy();
  });

  it('shows settings link', () => {
    render(Board, { props: { board: sampleBoard } });
    const settingsLink = screen.getByRole('link', { name: /settings/i });
    expect(settingsLink.getAttribute('href')).toBe('/settings');
  });

  it('renders platform columns', () => {
    render(Board, { props: { board: sampleBoard } });
    const platformRegion = screen.getByRole('region', { name: /platform arrivals/i });
    expect(platformRegion).toBeTruthy();
  });

  it('shows stale badge when board is stale', () => {
    render(Board, { props: { board: sampleStaleBoard } });
    // stale_since = '2025-01-15T10:00:30Z' → stale badge should appear
    const alert = screen.queryByRole('alert');
    expect(alert).toBeTruthy();
    // Badge contains STALE text
    expect(alert?.textContent).toMatch(/STALE/);
  });

  it('does not show stale badge for fresh board', () => {
    render(Board, { props: { board: sampleBoard } });
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('renders a clock element', () => {
    render(Board, { props: { board: sampleBoard } });
    const timeEl = screen.getByRole('time');
    expect(timeEl).toBeTruthy();
    // Should have datetime attribute
    expect(timeEl.getAttribute('datetime')).toBeTruthy();
  });
});

describe('Board — reduced-motion', () => {
  it('renders correctly (reduced motion is honoured via CSS media query)', () => {
    // The reducedMotion store is mocked at the top of this file (returns false).
    // Actual OS-level reduced motion is tested in the unit store tests.
    // Here we just verify the Board renders without errors.
    render(Board, { props: { board: sampleBoard } });
    expect(screen.getByRole('main')).toBeTruthy();
  });
});
