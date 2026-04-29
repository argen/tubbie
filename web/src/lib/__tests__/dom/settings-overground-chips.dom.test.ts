// @vitest-environment happy-dom
/**
 * DOM tests for the six new Overground line chips in Settings.
 *
 * TfL split the London Overground into six independently-named lines in
 * November 2024 (Mildmay / Lioness / Suffragette / Windrush / Weaver /
 * Liberty). Each must:
 *   - Appear in the Settings chip list at every station, regardless of
 *     whether that station actually serves Overground (the chip list is
 *     a global roster — visibility is controlled by `aria-disabled`).
 *   - Be enabled at a station whose `Station.lines` field contains its id.
 *   - Be disabled at a tube-only station.
 *   - Toggle into `BoardConfig.line_ids` when clicked while enabled.
 *
 * The DLR chip is also covered here since it landed in the same change set
 * (closing a latent bug where DLR-only stations were excluded from search).
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

const NAMED_OVERGROUND_LINES = [
  'mildmay',
  'lioness',
  'suffragette',
  'windrush',
  'weaver',
  'liberty',
] as const;

async function pickStation(station: Station): Promise<void> {
  setMockHandler('search_stations', () => [station]);
  const combobox = await waitFor(() => screen.getByRole('combobox'));
  await fireEvent.input(combobox, { target: { value: station.common_name.slice(0, 3) } });
  const option = await waitFor(
    () =>
      screen.getByRole('option', {
        name: new RegExp(station.common_name, 'i'),
      }),
    { timeout: 2000 },
  );
  await fireEvent.mouseDown(option);
}

function makeStation(id: string, name: string, lines: { id: string; name: string }[]): Station {
  return {
    id,
    common_name: name,
    modes: ['overground'],
    lat: 51.5,
    lon: -0.1,
    lines,
  };
}

describe('Settings — Overground line chips', () => {
  beforeEach(() => {
    resetMockHandlers();
    configError.set(null);
    config.set({ ...sampleConfig, station_id: '910GHACKNYC', line_ids: [] });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('renders all six named Overground chips plus DLR in the chip list', async () => {
    render(SettingsPage);
    const hackney = makeStation('910GHACKNYC', 'Hackney Central Rail Station', [
      { id: 'mildmay', name: 'Mildmay' },
    ]);
    await pickStation(hackney);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /toggle mildmay line/i })).toBeTruthy();
    });
    for (const lineId of NAMED_OVERGROUND_LINES) {
      const matches = screen.queryAllByRole('button', {
        name: new RegExp(`(toggle ${lineId} line|${lineId} line is not served)`, 'i'),
      });
      expect(matches.length, `expected exactly one chip for ${lineId}`).toBe(1);
    }
    // DLR chip is part of the same rollout.
    const dlrMatches = screen.queryAllByRole('button', {
      name: /(toggle dlr line|dlr line is not served)/i,
    });
    expect(dlrMatches.length, 'expected exactly one DLR chip').toBe(1);
  });

  it('enables only the lines the picked Overground station actually serves', async () => {
    render(SettingsPage);
    // Hackney Central serves only Mildmay (single-line).
    const hackney = makeStation('910GHACKNYC', 'Hackney Central Rail Station', [
      { id: 'mildmay', name: 'Mildmay' },
    ]);
    await pickStation(hackney);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /toggle mildmay line/i })).toBeTruthy();
    });

    const mildmay = screen.getByRole('button', { name: /toggle mildmay line/i });
    expect((mildmay as HTMLButtonElement).disabled).toBe(false);
    expect(mildmay.getAttribute('aria-disabled')).toBe('false');

    // The other five Overground lines must be disabled.
    for (const lineId of NAMED_OVERGROUND_LINES.filter((l) => l !== 'mildmay')) {
      const chip = screen.getByRole('button', {
        name: new RegExp(`${lineId} line is not served by this station`, 'i'),
      });
      expect((chip as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it('disables every Overground chip at a tube-only station', async () => {
    render(SettingsPage);
    const bzp: Station = {
      id: '940GZZLUBZP',
      common_name: 'Belsize Park Underground Station',
      modes: ['tube'],
      lat: 51.5505,
      lon: -0.1648,
      lines: [{ id: 'northern', name: 'Northern' }],
    };
    await pickStation(bzp);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /toggle northern line/i })).toBeTruthy();
    });
    for (const lineId of NAMED_OVERGROUND_LINES) {
      const chip = screen.getByRole('button', {
        name: new RegExp(`${lineId} line is not served by this station`, 'i'),
      });
      expect(chip.getAttribute('aria-disabled')).toBe('true');
      expect((chip as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it('toggling an enabled Mildmay chip writes mildmay into line_ids', async () => {
    let savedCfg: BoardConfig | null = null;
    setMockHandler('save_config', (args) => {
      savedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });

    render(SettingsPage);
    const hackney = makeStation('910GHACKNYC', 'Hackney Central Rail Station', [
      { id: 'mildmay', name: 'Mildmay' },
    ]);
    await pickStation(hackney);

    // Picking the station autosaves first; consume that.
    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    savedCfg = null;

    const mildmay = await waitFor(() =>
      screen.getByRole('button', { name: /toggle mildmay line/i }),
    );
    await fireEvent.click(mildmay);

    await waitFor(() => {
      expect(savedCfg).not.toBeNull();
    });
    expect(savedCfg!.line_ids).toContain('mildmay');
  });
});
