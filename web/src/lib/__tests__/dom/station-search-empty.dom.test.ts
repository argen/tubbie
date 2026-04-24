// @vitest-environment happy-dom
//
// Covers the "no results" and "still searching" UX gap that made the previous
// station search feel broken: the user typed, nothing visible happened, no
// error appeared, no indication the search had even run.
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import StationSearch from '$lib/components/StationSearch.svelte';
import type { Station } from '$lib/ipc/types.js';

const searchStationsMock = vi.fn<(q: string) => Promise<Station[]>>();

vi.mock('$lib/ipc/commands.js', () => ({
  searchStations: (q: string) => searchStationsMock(q),
}));

describe('StationSearch empty + loading states', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    searchStationsMock.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('shows an empty-state message when the search returns zero results', async () => {
    searchStationsMock.mockResolvedValue([]);
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');

    await fireEvent.input(input, { target: { value: 'xyzzy' } });
    vi.advanceTimersByTime(200);
    await vi.runAllTimersAsync();

    const empty = screen.getByTestId('station-search-empty');
    expect(empty.textContent).toMatch(/xyzzy/);
    expect(empty.getAttribute('role')).toBe('status');
  });

  it('does not show the empty state while the debounced search is still pending', async () => {
    // Resolver we control — simulates an in-flight network request.
    type Resolver = (s: Station[]) => void;
    const deferred: { resolve: Resolver | null } = { resolve: null };
    searchStationsMock.mockImplementation(
      () =>
        new Promise<Station[]>((resolve) => {
          deferred.resolve = resolve;
        }),
    );
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');

    await fireEvent.input(input, { target: { value: 'vic' } });
    vi.advanceTimersByTime(200);
    await vi.runAllTimersAsync();

    // At this point, the IPC call has been dispatched but not resolved.
    // The spinner should be visible, and the empty-state message hidden.
    expect(screen.queryByTestId('station-search-empty')).toBeNull();

    deferred.resolve?.([]);
    await vi.runAllTimersAsync();

    // Now the search completed with zero results — the empty state appears.
    expect(screen.getByTestId('station-search-empty')).toBeTruthy();
  });

  it('clears the empty-state message when the query is cleared', async () => {
    searchStationsMock.mockResolvedValue([]);
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');

    await fireEvent.input(input, { target: { value: 'xyzzy' } });
    vi.advanceTimersByTime(200);
    await vi.runAllTimersAsync();
    expect(screen.getByTestId('station-search-empty')).toBeTruthy();

    await fireEvent.input(input, { target: { value: '' } });
    expect(screen.queryByTestId('station-search-empty')).toBeNull();
  });
});
