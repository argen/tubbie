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
    // Just enough to fire the 200 ms debounce timer — do NOT advance enough
    // to trip the 12 s search timeout, or we'd end up asserting the error
    // path instead of the pending path.
    await vi.advanceTimersByTimeAsync(210);

    // At this point, the IPC call has been dispatched but not resolved.
    // The spinner should be visible, and the empty-state message hidden.
    expect(screen.queryByTestId('station-search-empty')).toBeNull();

    deferred.resolve?.([]);
    // Drain microtasks without advancing wall-clock time far enough to hit
    // the 12 s timeout. A handful of microtask checkpoints is plenty — the
    // resolve -> debounceAsync.then -> Svelte reactivity chain is shallow.
    for (let i = 0; i < 5; i++) await Promise.resolve();

    // Now the search completed with zero results — the empty state appears.
    expect(screen.getByTestId('station-search-empty')).toBeTruthy();
  });

  it('shows the spinner during the debounce window, before the IPC call fires', async () => {
    // searchStations returns immediately; we only care about the 200 ms debounce gap.
    searchStationsMock.mockResolvedValue([]);
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');

    await fireEvent.input(input, { target: { value: 'v' } });

    // Before advancing timers: the spinner should already be visible so the
    // user sees the 200 ms debounce as "still searching", not dead UI.
    const search = screen.getByRole('search');
    expect(search.textContent).toContain('⠿');

    // searchStations should not yet have been called.
    expect(searchStationsMock).not.toHaveBeenCalled();
  });

  it('does NOT show the empty state after the user picks a result', async () => {
    // Regression: previously `selectStation` left `searched=true` with
    // `results=[]` and the query still filled with the selected station
    // name — so the empty-state branch fired, and the UI showed
    // "No tube stations match 'Victoria'" *under* a selected Victoria.
    const victoria = {
      id: '940GZZLUVIC',
      common_name: 'Victoria',
      modes: ['tube'],
      lat: 51.495,
      lon: -0.144,
      lines: [{ id: 'victoria', name: 'Victoria' }],
    };
    searchStationsMock.mockResolvedValue([victoria]);

    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');

    await fireEvent.input(input, { target: { value: 'vic' } });
    vi.advanceTimersByTime(200);
    await vi.runAllTimersAsync();

    // Dropdown option appears — pick it.
    const option = screen.getByRole('option', { name: /Victoria/i });
    await fireEvent.mouseDown(option);

    // After selection, the input carries the station name but the empty-state
    // must NOT render (even though results is now []).
    expect(screen.queryByTestId('station-search-empty')).toBeNull();
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
