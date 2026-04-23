// @vitest-environment happy-dom
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import StationSearch from '$lib/components/StationSearch.svelte';
import type { Station } from '$lib/ipc/types.js';
import { sampleStation } from '$lib/ipc/mock.js';

// Mock the IPC commands
vi.mock('$lib/ipc/commands.js', () => ({
  searchStations: vi.fn(async (_q: string): Promise<Station[]> => [sampleStation]),
}));

describe('StationSearch', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('renders a search input', () => {
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');
    expect(input).toBeTruthy();
    expect(input.getAttribute('aria-label')).toContain('station');
  });

  it('debounces search — does not call searchStations immediately', async () => {
    const { searchStations } = await import('$lib/ipc/commands.js');
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');
    await fireEvent.input(input, { target: { value: 'Bel' } });
    // Should NOT have called searchStations yet (debounce is 200ms)
    expect(searchStations).not.toHaveBeenCalled();
  });

  it('calls searchStations after 200ms debounce', async () => {
    const { searchStations } = await import('$lib/ipc/commands.js');
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');
    await fireEvent.input(input, { target: { value: 'Belsize' } });
    vi.advanceTimersByTime(200);
    await vi.runAllTimersAsync();
    expect(searchStations).toHaveBeenCalledWith('Belsize');
  });

  it('has correct ARIA combobox attributes', () => {
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const input = screen.getByRole('combobox');
    expect(input.getAttribute('aria-autocomplete')).toBe('list');
    expect(input.getAttribute('aria-controls')).toBe('station-listbox');
  });

  it('has search role on container', () => {
    render(StationSearch, {
      props: { selectedId: '', onSelect: vi.fn() },
    });
    const search = screen.getByRole('search');
    expect(search.getAttribute('aria-label')).toContain('search');
  });
});
