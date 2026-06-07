// @vitest-environment happy-dom
/**
 * DOM tests for the station-search overlay embedded in the Board header.
 *
 * What we lock in here:
 *   1. Clicking the magnifier button reveals the StationSearch input overlay.
 *   2. Selecting a station from results calls selectStation and closes the overlay.
 *   3. Pressing Escape closes the overlay without selecting a station.
 *   4. The search button is accessible (aria-label, aria-pressed).
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import Board from '$lib/components/Board.svelte';
import { sampleBoard, mockInvoke, sampleStation } from '$lib/ipc/mock.js';

// ---------------------------------------------------------------------------
// Tauri IPC mock — Board.svelte uses invoke for openSettingsWindow, config etc.
// ---------------------------------------------------------------------------
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

// ---------------------------------------------------------------------------
// board.js store mock — only lastUpdateTs is needed by Board.svelte directly.
// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// settingsForm.js mock — Board.svelte imports selectStation from here.
// vi.mock factories are hoisted, so use vi.hoisted to define the spy before
// the factory runs.
// ---------------------------------------------------------------------------
const { mockSelectStation } = vi.hoisted(() => ({
  mockSelectStation: vi.fn(),
}));
vi.mock('$lib/stores/settingsForm.js', () => ({
  selectStation: mockSelectStation,
}));

describe('Board — station-search overlay', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockSelectStation.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('shows a station-search toggle button with correct aria attributes', () => {
    render(Board, { props: { board: sampleBoard } });
    const btn = screen.getByRole('button', { name: /change station/i });
    expect(btn).toBeTruthy();
    expect(btn.getAttribute('aria-pressed')).toBe('false');
  });

  it('clicking the search toggle reveals the StationSearch input', async () => {
    render(Board, { props: { board: sampleBoard } });

    // Overlay should not be visible initially.
    expect(document.querySelector('[data-testid="board-search-overlay"]')).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: /change station/i }));

    // Overlay is now mounted and contains a search input.
    const overlay = document.querySelector('[data-testid="board-search-overlay"]');
    expect(overlay).not.toBeNull();
    expect(screen.getByRole('combobox')).toBeTruthy();
  });

  it('clicking the search toggle twice closes the overlay', async () => {
    render(Board, { props: { board: sampleBoard } });

    const btn = screen.getByRole('button', { name: /change station/i });
    await fireEvent.click(btn);
    expect(document.querySelector('[data-testid="board-search-overlay"]')).not.toBeNull();

    // Button label changes to "Close station search" when open.
    const closeBtn = screen.getByRole('button', { name: /close station search/i });
    await fireEvent.click(closeBtn);
    expect(document.querySelector('[data-testid="board-search-overlay"]')).toBeNull();
  });

  it('pressing Escape from anywhere closes the overlay', async () => {
    render(Board, { props: { board: sampleBoard } });

    await fireEvent.click(screen.getByRole('button', { name: /change station/i }));
    expect(document.querySelector('[data-testid="board-search-overlay"]')).not.toBeNull();

    // Dispatch Escape on document.body — focus is NOT inside the overlay, so
    // this proves the window-level handler closes it regardless of where focus
    // sits (a real Esc keypress targets whatever has focus, not the overlay).
    await fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(document.querySelector('[data-testid="board-search-overlay"]')).toBeNull();
  });

  it('focuses the search input when the overlay opens', async () => {
    render(Board, { props: { board: sampleBoard } });

    await fireEvent.click(screen.getByRole('button', { name: /change station/i }));
    // The autofocus runs after tick() (a microtask); flush it.
    await vi.advanceTimersByTimeAsync(0);

    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole('combobox'));
    });
  });

  it('selecting a station calls selectStation and closes the overlay', async () => {
    render(Board, { props: { board: sampleBoard } });

    // Open the overlay.
    await fireEvent.click(screen.getByRole('button', { name: /change station/i }));
    expect(document.querySelector('[data-testid="board-search-overlay"]')).not.toBeNull();

    // Type into the search input to trigger results.
    const input = screen.getByRole('combobox');
    await fireEvent.input(input, { target: { value: 'Bel' } });
    // Advance past the 200ms debounce and let the mock IPC resolve.
    await vi.advanceTimersByTimeAsync(300);

    // The mock searchStations returns sampleStation ("Belsize Park").
    await waitFor(() => {
      expect(screen.queryAllByRole('option').length).toBeGreaterThan(0);
    });

    // Click the first result. getAllByRole guarantees ≥1 (the waitFor above),
    // so the non-null assertion is safe under noUncheckedIndexedAccess.
    const option = screen.getAllByRole('option')[0]!;
    await fireEvent.mouseDown(option);

    // selectStation should have been called with the station.
    expect(mockSelectStation).toHaveBeenCalledOnce();
    expect(mockSelectStation).toHaveBeenCalledWith(
      expect.objectContaining({ id: sampleStation.id }),
    );

    // Overlay should be closed.
    expect(document.querySelector('[data-testid="board-search-overlay"]')).toBeNull();
  });
});
