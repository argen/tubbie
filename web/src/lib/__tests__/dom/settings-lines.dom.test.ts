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

  it('autosaves after clicking a disabled chip: line_ids stays empty', async () => {
    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    render(SettingsPage);

    const bzp = makeStation('940GZZLUBZP', 'Belsize Park', [{ id: 'northern', name: 'Northern' }]);
    await pickStation(bzp);

    // Picking the station already autosaved once.
    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    savedCfg = null;

    // The central chip must be disabled at Belsize Park — clicking it is a no-op
    // and so must not trigger a save.
    const centralChip = await waitFor(() =>
      screen.getByRole('button', { name: /central line is not served by this station/i }),
    );
    await fireEvent.click(centralChip);

    // Give any erroneous autosave a chance to land, then confirm none did.
    await new Promise((r) => setTimeout(r, 50));
    expect(savedCfg).toBeNull();
  });

  it('autosaves on station switch with line_ids pruned to the intersection', async () => {
    config.set({
      ...sampleConfig,
      station_id: '940GZZLUOXC',
      line_ids: ['central', 'northern'],
    });

    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    render(SettingsPage);

    // Now pick a station that serves only Bakerloo.
    const baker = makeStation('940GZZLUBKE', 'Baker Street', [
      { id: 'bakerloo', name: 'Bakerloo' },
    ]);
    await pickStation(baker);

    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    expect(savedCfg!.line_ids).toEqual([]); // neither 'central' nor 'northern' survives.
    expect(savedCfg!.station_id).toBe('940GZZLUBKE');
  });

  it('autosaves on station switch, keeping line_ids present in the new station', async () => {
    config.set({
      ...sampleConfig,
      station_id: '940GZZLUOXC',
      line_ids: ['central', 'victoria'],
    });

    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    render(SettingsPage);

    const newStation = makeStation('940GZZLUVIC', 'Victoria', [
      { id: 'victoria', name: 'Victoria' },
      { id: 'district', name: 'District' },
      { id: 'circle', name: 'Circle' },
    ]);
    await pickStation(newStation);

    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    expect(savedCfg!.line_ids).toEqual(['victoria']);
  });

  it('autosaves when toggling a line chip', async () => {
    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    render(SettingsPage);

    const oxc = makeStation('940GZZLUOXC', 'Oxford Circus', [
      { id: 'bakerloo', name: 'Bakerloo' },
      { id: 'central', name: 'Central' },
      { id: 'victoria', name: 'Victoria' },
    ]);
    await pickStation(oxc);

    // Clear the station-select save so we can observe the chip-toggle save.
    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    savedCfg = null;

    const central = await waitFor(() =>
      screen.getByRole('button', { name: /toggle central line/i }),
    );
    await fireEvent.click(central);

    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    expect(savedCfg!.line_ids).toEqual(['central']);
  });

  it('shows a DLR chip and enables it when Bank (which serves DLR) is selected', async () => {
    render(SettingsPage);

    const bank = makeStation('940GZZLUBNK', 'Bank Underground Station', [
      { id: 'central', name: 'Central' },
      { id: 'northern', name: 'Northern' },
      { id: 'waterloo-city', name: 'Waterloo & City' },
      { id: 'dlr', name: 'DLR' },
    ]);
    await pickStation(bank);

    // DLR chip must appear (it's not in KNOWN_LINES but is in stationLines).
    const dlrChip = await waitFor(() => screen.getByRole('button', { name: /toggle dlr line/i }));
    expect(dlrChip.getAttribute('aria-disabled')).toBe('false');
    expect((dlrChip as HTMLButtonElement).disabled).toBe(false);

    // Tube lines the station serves must also be enabled.
    const centralChip = screen.getByRole('button', { name: /toggle central line/i });
    expect(centralChip.getAttribute('aria-disabled')).toBe('false');
  });

  it('shows a Mildmay chip and enables it when Whitechapel (which serves Overground) is selected', async () => {
    render(SettingsPage);

    const whitechapel = makeStation('940GZZLUWPL', 'Whitechapel Underground Station', [
      { id: 'hammersmith-city', name: 'Hammersmith & City' },
      { id: 'district', name: 'District' },
      { id: 'elizabeth-line', name: 'Elizabeth' },
      { id: 'mildmay', name: 'Mildmay' },
    ]);
    await pickStation(whitechapel);

    // Mildmay chip must appear (not in KNOWN_LINES, but in stationLines).
    const mildmayChip = await waitFor(() =>
      screen.getByRole('button', { name: /toggle mildmay line/i }),
    );
    expect(mildmayChip.getAttribute('aria-disabled')).toBe('false');

    // Elizabeth chip was already in KNOWN_LINES; must be enabled.
    const elizabethChip = screen.getByRole('button', { name: /toggle elizabeth line/i });
    expect(elizabethChip.getAttribute('aria-disabled')).toBe('false');
  });

  it('DLR chip is pruned from line_ids when switching to a tube-only station', async () => {
    // Start with Bank selected and DLR in line_ids.
    config.set({
      ...sampleConfig,
      station_id: '940GZZLUBNK',
      line_ids: ['dlr', 'central'],
    });

    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    render(SettingsPage);

    // Switch to Belsize Park (Northern-only, no DLR).
    const belsize = makeStation('940GZZLUBZP', 'Belsize Park Underground Station', [
      { id: 'northern', name: 'Northern' },
    ]);
    await pickStation(belsize);

    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    // Both 'dlr' and 'central' must be pruned since neither is served.
    expect(savedCfg!.line_ids).toEqual([]);
  });
});
