// @vitest-environment happy-dom
/**
 * DOM tests for the Settings page line-filter chips.
 *
 * The chip list must reflect the currently-selected station: picking a
 * station with `lines = [central, northern]` should render only two chips,
 * not the global 12-line list. Switching to a station whose `lines` does
 * not include a currently-selected line must prune it from `line_ids` so
 * we never persist a line the station doesn't serve.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { mockInvoke, setMockHandler, resetMockHandlers, sampleConfig } from '$lib/ipc/mock.js';
import { config, configError } from '$lib/stores/config.js';
import SettingsPage from '../../../routes/settings/+page.svelte';
import type { BoardConfig, Station } from '$lib/ipc/types.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

async function pickStation(station: Station): Promise<void> {
  // Override the search handler to return only this station and trigger
  // the combobox flow end-to-end (typing → debounce → click).
  setMockHandler('search_stations', () => [station]);

  const combobox = await waitFor(() => screen.getByRole('combobox'));
  await fireEvent.input(combobox, { target: { value: station.common_name.slice(0, 3) } });

  // StationSearch debounces 200ms — wait for the listbox to appear.
  const option = await waitFor(
    () =>
      screen.getByRole('option', {
        name: new RegExp(station.common_name, 'i'),
      }),
    { timeout: 2000 },
  );

  // onmousedown drives selection in StationSearch (pre-blur).
  await fireEvent.mouseDown(option);
}

function makeStation(id: string, name: string, lines: { id: string; name: string }[]): Station {
  return {
    id,
    common_name: name,
    modes: ['tube'],
    lat: 51.5,
    lon: -0.1,
    lines,
  };
}

describe('Settings — station-scoped line chips', () => {
  beforeEach(() => {
    resetMockHandlers();
    configError.set(null);
    config.set({ ...sampleConfig, station_id: '940GZZLUBZP', line_ids: [] });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('enables only the chips for the selected station; disables the rest', async () => {
    render(SettingsPage);

    const oxc = makeStation('940GZZLUOXC', 'Oxford Circus', [
      { id: 'bakerloo', name: 'Bakerloo' },
      { id: 'central', name: 'Central' },
      { id: 'victoria', name: 'Victoria' },
    ]);
    await pickStation(oxc);

    // All 12 chips always render; only the three OXC lines are enabled.
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /toggle bakerloo line/i })).toBeTruthy();
    });
    const enabled = ['bakerloo', 'central', 'victoria'];
    const disabled = [
      'circle',
      'district',
      'elizabeth',
      'hammersmith & city',
      'jubilee',
      'metropolitan',
      'northern',
      'piccadilly',
      'waterloo & city',
    ];
    for (const name of enabled) {
      const chip = screen.getByRole('button', { name: new RegExp(`toggle ${name} line`, 'i') });
      expect(chip.getAttribute('aria-disabled')).toBe('false');
      expect((chip as HTMLButtonElement).disabled).toBe(false);
    }
    for (const name of disabled) {
      const chip = screen.getByRole('button', {
        name: new RegExp(`${name} line is not served by this station`, 'i'),
      });
      expect(chip.getAttribute('aria-disabled')).toBe('true');
      expect((chip as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it('keeps all 12 chips enabled when the station has no line metadata (fail open)', async () => {
    render(SettingsPage);

    const bare = makeStation('940GZZLUBARE', 'Bare Station', []);
    await pickStation(bare);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /toggle bakerloo line/i })).toBeTruthy();
    });
    for (const lineName of [
      'bakerloo',
      'central',
      'circle',
      'district',
      'elizabeth',
      'hammersmith & city',
      'jubilee',
      'metropolitan',
      'northern',
      'piccadilly',
      'victoria',
      'waterloo & city',
    ]) {
      const chip = screen.getByRole('button', {
        name: new RegExp(`toggle ${lineName} line`, 'i'),
      });
      expect(chip.getAttribute('aria-disabled')).toBe('false');
      expect((chip as HTMLButtonElement).disabled).toBe(false);
    }
  });

  it('clicking a disabled chip does not add it to line_ids', async () => {
    render(SettingsPage);

    const bzp = makeStation('940GZZLUBZP', 'Belsize Park', [{ id: 'northern', name: 'Northern' }]);
    await pickStation(bzp);

    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    // The central chip must be disabled at Belsize Park.
    const centralChip = await waitFor(() =>
      screen.getByRole('button', { name: /central line is not served by this station/i }),
    );
    await fireEvent.click(centralChip);

    const saveBtn = await waitFor(() => screen.getByRole('button', { name: /save settings/i }));
    await fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    expect(savedCfg!.line_ids).toEqual([]);
  });

  it('prunes line_ids on station switch: saved set becomes intersection with new station lines', async () => {
    config.set({
      ...sampleConfig,
      station_id: '940GZZLUOXC',
      line_ids: ['central', 'northern'],
    });
    render(SettingsPage);

    // Now pick a station that serves only Bakerloo.
    const baker = makeStation('940GZZLUBKE', 'Baker Street', [
      { id: 'bakerloo', name: 'Bakerloo' },
    ]);
    await pickStation(baker);

    // Capture what save_config receives.
    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    const saveBtn = await waitFor(() => screen.getByRole('button', { name: /save settings/i }));
    await fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    expect(savedCfg!.line_ids).toEqual([]); // neither 'central' nor 'northern' survives.
    expect(savedCfg!.station_id).toBe('940GZZLUBKE');
  });

  it('keeps line_ids that still appear in the new station lines', async () => {
    config.set({
      ...sampleConfig,
      station_id: '940GZZLUOXC',
      line_ids: ['central', 'victoria'],
    });
    render(SettingsPage);

    // New station serves Victoria (a subset of current selection).
    const newStation = makeStation('940GZZLUVIC', 'Victoria', [
      { id: 'victoria', name: 'Victoria' },
      { id: 'district', name: 'District' },
      { id: 'circle', name: 'Circle' },
    ]);
    await pickStation(newStation);

    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    const saveBtn = await waitFor(() => screen.getByRole('button', { name: /save settings/i }));
    await fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    expect(savedCfg!.line_ids).toEqual(['victoria']);
  });
});
