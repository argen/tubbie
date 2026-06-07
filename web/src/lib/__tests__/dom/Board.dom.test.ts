// @vitest-environment happy-dom
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import { get } from 'svelte/store';
import Board from '$lib/components/Board.svelte';
import { sampleBoard, sampleStaleBoard, mockInvoke } from '$lib/ipc/mock.js';
import { settingsOpen } from '$lib/stores/settingsView.js';

// Wire Tauri IPC mock (Board.svelte uses invoke for applyBoardSize, etc.).
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));

// Mock board store to prevent side-effects
vi.mock('$lib/stores/board.js', () => ({
  lastUpdateTs: {
    subscribe: (fn: (v: number) => void) => {
      fn(0);
      return () => undefined;
    },
  },
}));

vi.mock('$lib/stores/settingsForm.js', () => ({
  selectStation: vi.fn(),
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

  it('settings button opens the in-frame Settings panel (store, not a window)', async () => {
    // The gear is a <button> that flips the `settingsOpen` store — Settings is
    // an in-frame overlay now, not a separate webview window (PR2). It must NOT
    // be an <a href="/settings"> (that route no longer exists).
    settingsOpen.set(false);
    render(Board, { props: { board: sampleBoard } });
    const settingsBtn = screen.getByRole('button', { name: /settings/i });
    expect(settingsBtn.tagName.toLowerCase()).toBe('button');
    expect(get(settingsOpen)).toBe(false);
    await fireEvent.click(settingsBtn);
    expect(get(settingsOpen)).toBe(true);
  });

  it('renders the arrivals region', () => {
    render(Board, { props: { board: sampleBoard } });
    const arrivalsRegion = screen.getByRole('region', { name: /arrivals/i });
    expect(arrivalsRegion).toBeTruthy();
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

describe('Board — Status view toggle', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  const disruptedStatuses = [
    {
      line_id: 'central',
      disruption_text: 'Severe delays',
      status: [{ severity: 6, description: 'Severe delays', bucket: 'SevereDelays' as const }],
    },
  ];

  it('shows a service-status toggle with a disruption-count badge', () => {
    render(Board, { props: { board: sampleBoard, statuses: disruptedStatuses } });
    const btn = screen.getByRole('button', { name: /service status/i });
    expect(btn).toBeTruthy();
    expect(btn.textContent).toContain('1'); // one disrupted line
  });

  it('toggles the body from arrivals to the Status view and back', async () => {
    const { getByRole, queryByRole } = render(Board, {
      props: { board: sampleBoard, statuses: disruptedStatuses },
    });
    // Arrivals visible initially.
    expect(getByRole('region', { name: /arrivals/i })).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: /show service status/i }));

    // Status view replaced the arrivals body.
    expect(getByRole('region', { name: /^service status$/i })).toBeTruthy();
    expect(queryByRole('region', { name: /arrivals/i })).toBeNull();

    // Toggle back.
    await fireEvent.click(getByRole('button', { name: /hide service status/i }));
    expect(getByRole('region', { name: /arrivals/i })).toBeTruthy();
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
