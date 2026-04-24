// @vitest-environment happy-dom
/**
 * DOM tests for the Settings page "Current:" station label.
 *
 * The label has to make it obvious which station is saved in config, so the
 * user doesn't accidentally re-select the same one — the previous UI showed
 * a raw NaPTAN ID ("Station ID: 940GZZLUFCN"), which users had no way to
 * map back to a real station name.
 *
 * The displayed name is sourced in this order (see `currentStationName` in
 * `+page.svelte`):
 *   1. Local state set by `handleStationSelect` (freshly-picked station)
 *   2. `station_name` from the first arrival in the `$board` store
 * In both cases the " Underground Station" suffix is stripped.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { mockInvoke, resetMockHandlers, sampleConfig, setMockHandler } from '$lib/ipc/mock.js';
import { config, configError } from '$lib/stores/config.js';
import { board } from '$lib/stores/board.js';
import SettingsPage from '../../../routes/settings/+page.svelte';
import type { Board, Station } from '$lib/ipc/types.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

function makeBoardWithStation(stationName: string): Board {
  return {
    station_id: '940GZZLUBZP',
    platforms: [
      {
        name: 'Northbound - Platform 1',
        arrivals: [
          {
            id: '1',
            station_name: stationName,
            platform_name: 'Northbound - Platform 1',
            line_id: 'northern',
            line_name: 'Northern',
            direction: 'Northbound',
            destination_name: 'Edgware',
            towards: 'Edgware',
            current_location: 'At platform',
            time_to_station: 60,
            expected_arrival: '2025-01-15T10:01:00Z',
            naptan_id: '940GZZLUBZP',
          },
        ],
      },
    ],
    generated_at: '2025-01-15T10:00:00Z',
    stale_since: null,
  };
}

describe('Settings — current station label', () => {
  beforeEach(() => {
    resetMockHandlers();
    configError.set(null);
    config.set({ ...sampleConfig });
    board.set(null);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('shows "Current: {name}" from the board store with the suffix stripped', () => {
    board.set(makeBoardWithStation('Belsize Park Underground Station'));
    render(SettingsPage);

    const label = screen.getByTestId('settings-current-station');
    expect(label.textContent).toContain('Current:');
    expect(label.textContent).toContain('Belsize Park');
    // The raw suffix must not leak through.
    expect(label.textContent).not.toContain('Underground Station');
  });

  it('does NOT show a raw NaPTAN ID as the current station', () => {
    board.set(null);
    render(SettingsPage);

    // No board, no local pick → label should be hidden entirely rather than
    // falling back to showing the station_id.
    expect(screen.queryByTestId('settings-current-station')).toBeNull();
    // Belt-and-braces: the NaPTAN id from sampleConfig must not appear anywhere.
    expect(screen.queryByText(/940GZZLUBZP/)).toBeNull();
  });

  it('updates immediately after the user picks a new station, before the board refreshes', async () => {
    board.set(makeBoardWithStation('Belsize Park Underground Station'));
    const picked: Station = {
      id: '940GZZLUMDN',
      common_name: 'Morden',
      modes: ['tube'],
      lat: 51.4,
      lon: -0.19,
      lines: [{ id: 'northern', name: 'Northern' }],
    };
    setMockHandler('search_stations', () => [picked]);
    setMockHandler('save_config', () => null);

    render(SettingsPage);

    // Type into the search box → wait for the debounced listbox → click.
    const combobox = await waitFor(() => screen.getByRole('combobox'));
    await fireEvent.input(combobox, { target: { value: 'mor' } });

    const option = await waitFor(() => screen.getByRole('option', { name: /Morden/i }), {
      timeout: 2000,
    });
    await fireEvent.mouseDown(option);

    // Label flips to the freshly-picked station even though the board store
    // still has the old one (the stream restart is async in real life).
    await waitFor(() => {
      const label = screen.getByTestId('settings-current-station');
      expect(label.textContent).toContain('Morden');
    });
  });
});
