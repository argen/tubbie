// @vitest-environment happy-dom
/**
 * DOM tests for the Settings page Favorites UI.
 *
 * Three cuts:
 *   1. Star toggle adds + removes via the IPC commands. The star reflects
 *      whether the current station is in `$favorites`.
 *   2. Clicking a favorite row routes through `save_config` (so the watch
 *      channel publishes the new station_id and invariant #2 fires).
 *   3. Each favorite row renders chips from the snapshotted `Favorite.lines`
 *      so a cold stop-points cache doesn't leave the row chip-less.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';
import { mockInvoke, resetMockHandlers, sampleConfig, setMockHandler } from '$lib/ipc/mock.js';
import { config, configError } from '$lib/stores/config.js';
import { board } from '$lib/stores/board.js';
import { favorites, favoritesError } from '$lib/stores/favorites.js';
import SettingsPage from '../../../routes/settings/+page.svelte';
import type { BoardConfig, Favorite } from '$lib/ipc/types.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

const sampleFavorite: Favorite = {
  station_id: '940GZZLUKSX',
  common_name: "King's Cross St. Pancras Underground Station",
  lines: [
    { id: 'northern', name: 'Northern' },
    { id: 'victoria', name: 'Victoria' },
    { id: 'piccadilly', name: 'Piccadilly' },
  ],
};

describe('Settings — Favorites UI', () => {
  beforeEach(() => {
    resetMockHandlers();
    configError.set(null);
    favoritesError.set(null);
    favorites.set([]);
    config.set({ ...sampleConfig });
    board.set({
      station_id: '940GZZLUBZP',
      platforms: [
        {
          name: 'Northbound - Platform 1',
          arrivals: [
            {
              id: '1',
              station_name: 'Belsize Park Underground Station',
              platform_name: 'Northbound - Platform 1',
              line_id: 'northern',
              line_name: 'Northern',
              direction: 'Northbound',
              destination_name: 'Edgware',
              towards: 'Edgware',
              current_location: 'At platform',
              time_to_station: 60,
              expected_arrival: '2026-01-15T10:01:00Z',
              naptan_id: '940GZZLUBZP',
            },
          ],
        },
      ],
      generated_at: '2026-01-15T10:00:00Z',
      stale_since: null,
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  // --------------------------------------------------------------------------
  // 1. Star toggle adds + removes via add_favorite / remove_favorite IPC
  // --------------------------------------------------------------------------

  it('star_toggle_adds_and_removes', async () => {
    let stored: Favorite[] = [];
    setMockHandler('list_favorites', () => stored);
    setMockHandler('add_favorite', (args) => {
      const { stationId, commonName, lines } = args as {
        stationId: string;
        commonName: string;
        lines: Favorite['lines'];
      };
      if (!stored.some((f) => f.station_id === stationId)) {
        stored = [...stored, { station_id: stationId, common_name: commonName, lines }];
      }
      return stored;
    });
    setMockHandler('remove_favorite', (args) => {
      const { stationId } = args as { stationId: string };
      stored = stored.filter((f) => f.station_id !== stationId);
      return stored;
    });

    render(SettingsPage);

    // Star starts inactive (current station not in favorites).
    const star = await waitFor(() => screen.getByTestId('settings-star'));
    expect(star.textContent?.trim()).toBe('☆');
    expect(star.getAttribute('aria-pressed')).toBe('false');

    // Click → adds. Star flips to filled.
    await fireEvent.click(star);
    await waitFor(() => {
      const updated = screen.getByTestId('settings-star');
      expect(updated.textContent?.trim()).toBe('★');
      expect(updated.getAttribute('aria-pressed')).toBe('true');
    });
    expect(stored).toHaveLength(1);
    expect(stored[0]?.station_id).toBe('940GZZLUBZP');

    // Click again → removes. Star flips back to outline.
    await fireEvent.click(screen.getByTestId('settings-star'));
    await waitFor(() => {
      const updated = screen.getByTestId('settings-star');
      expect(updated.textContent?.trim()).toBe('☆');
      expect(updated.getAttribute('aria-pressed')).toBe('false');
    });
    expect(stored).toHaveLength(0);
  });

  // --------------------------------------------------------------------------
  // 2. Clicking a favorite row drives save_config with the new station_id
  // --------------------------------------------------------------------------

  it('clicking_favorite_calls_save_config_with_new_station_id', async () => {
    setMockHandler('list_favorites', () => [sampleFavorite]);

    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    render(SettingsPage);

    // Wait for the favorite row to render after onMount → initFavorites.
    const row = await waitFor(() => {
      const els = screen.getAllByTestId('favorite-row');
      const found = els.find((e) => e.getAttribute('data-station-id') === '940GZZLUKSX');
      if (!found) throw new Error('favorite row not rendered yet');
      return found;
    });

    await fireEvent.click(row);

    // Selection routes through updateConfig → saveConfig with the new id.
    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
      expect(savedCfg!.station_id).toBe('940GZZLUKSX');
    });
  });

  // --------------------------------------------------------------------------
  // 3. Favorite row chips come from the snapshotted `Favorite.lines`
  // --------------------------------------------------------------------------

  it('favorite_row_renders_line_chips_from_snapshot', async () => {
    setMockHandler('list_favorites', () => [sampleFavorite]);

    render(SettingsPage);

    const row = await waitFor(() => {
      const els = screen.getAllByTestId('favorite-row');
      const found = els.find((e) => e.getAttribute('data-station-id') === '940GZZLUKSX');
      if (!found) throw new Error('favorite row not rendered yet');
      return found;
    });

    // All three lines from the snapshot must appear as chips inside the row.
    const scope = within(row);
    expect(scope.getByText('Northern')).toBeTruthy();
    expect(scope.getByText('Victoria')).toBeTruthy();
    expect(scope.getByText('Piccadilly')).toBeTruthy();
  });

  // --------------------------------------------------------------------------
  // 4. Empty state copy
  // --------------------------------------------------------------------------

  it('empty_state_message_when_no_favorites', async () => {
    setMockHandler('list_favorites', () => []);

    render(SettingsPage);

    // The current station is not in favorites and there are no rows, so the
    // empty-state copy should be visible.
    const empty = await waitFor(() => screen.getByTestId('favorites-empty'));
    expect(empty.textContent ?? '').toMatch(/star a station/i);
    expect(screen.queryByTestId('favorites-list')).toBeNull();
  });

  // --------------------------------------------------------------------------
  // 5. Trash button removes a favorite
  // --------------------------------------------------------------------------

  it('trash_button_removes_favorite', async () => {
    let stored: Favorite[] = [sampleFavorite];
    setMockHandler('list_favorites', () => stored);
    setMockHandler('remove_favorite', (args) => {
      const { stationId } = args as { stationId: string };
      stored = stored.filter((f) => f.station_id !== stationId);
      return stored;
    });

    render(SettingsPage);

    const trash = await waitFor(() => {
      const all = screen.getAllByTestId('favorite-trash');
      const found = all.find((e) => e.getAttribute('data-station-id') === '940GZZLUKSX');
      if (!found) throw new Error('trash button not rendered yet');
      return found;
    });

    await fireEvent.click(trash);

    await waitFor(() => {
      expect(screen.queryByTestId('favorite-row')).toBeNull();
      expect(screen.getByTestId('favorites-empty')).toBeTruthy();
    });
  });
});
