// @vitest-environment happy-dom
/**
 * Settings autosave debounce — item 5 of the rate-limit-reduction plan.
 *
 * Each chip click in Settings (line / direction / theme) used to trigger an
 * immediate `save_config`. After PR B the click only mutates local state and
 * schedules a 400 ms trailing-edge persist. A burst of rapid clicks must
 * coalesce into a single `save_config` carrying the final state — even
 * though the watch-channel refactor removed the stream-respawn cost, every
 * `save_config` still does a synchronous disk write + IPC round-trip and
 * fires a `cfg_tx.send`, so coalescing is the right thing to do.
 *
 * `vi.useFakeTimers()` makes the 400 ms debounce window deterministic; we
 * advance the clock manually rather than waiting on real time.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { mockInvoke, setMockHandler, resetMockHandlers, sampleConfig } from '$lib/ipc/mock.js';
import { config, configError } from '$lib/stores/config.js';
import SettingsPage from '../../../routes/settings/+page.svelte';
import type { BoardConfig } from '$lib/ipc/types.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

describe('Settings — debounced chip persists (item 5)', () => {
  let saveCallCount = 0;
  let lastSavedCfg: BoardConfig | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    resetMockHandlers();
    configError.set(null);
    config.set({ ...sampleConfig, station_id: '940GZZLUBZP', line_ids: [], directions: [] });
    saveCallCount = 0;
    lastSavedCfg = null;
    setMockHandler('save_config', (args) => {
      saveCallCount++;
      lastSavedCfg = (args as { cfg: BoardConfig }).cfg;
      return null;
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('coalesces rapid direction-chip toggles into one save_config', async () => {
    render(SettingsPage);

    // Direction chips don't depend on station metadata, so the fail-open
    // path leaves all six interactive on first mount.
    const northbound = await waitFor(() =>
      screen.getByRole('button', { name: /toggle northbound direction/i }),
    );
    const southbound = await waitFor(() =>
      screen.getByRole('button', { name: /toggle southbound direction/i }),
    );
    const eastbound = await waitFor(() =>
      screen.getByRole('button', { name: /toggle eastbound direction/i }),
    );

    // Three clicks within ~50 ms. Without debounce this fires 3 saves.
    await fireEvent.click(northbound);
    vi.advanceTimersByTime(20);
    await fireEvent.click(southbound);
    vi.advanceTimersByTime(20);
    await fireEvent.click(eastbound);

    // Inside the 400 ms window — no save yet.
    vi.advanceTimersByTime(200);
    await Promise.resolve(); // drain microtasks
    expect(saveCallCount).toBe(0);

    // Trailing edge fires.
    vi.advanceTimersByTime(250);
    await vi.runAllTimersAsync();
    for (let i = 0; i < 5; i++) await Promise.resolve();

    expect(saveCallCount).toBe(1);
    expect(lastSavedCfg).not.toBeNull();
    // Final state contains all three directions, in click order.
    expect(lastSavedCfg!.directions).toEqual(['Northbound', 'Southbound', 'Eastbound']);
  });

  it('flushes a pending persist on beforeunload (no save dropped)', async () => {
    render(SettingsPage);

    const northbound = await waitFor(() =>
      screen.getByRole('button', { name: /toggle northbound direction/i }),
    );
    await fireEvent.click(northbound);

    // Still inside the debounce window — no save.
    vi.advanceTimersByTime(100);
    await Promise.resolve();
    expect(saveCallCount).toBe(0);

    // User closes the window (or navigates away). beforeunload must
    // synchronously flush the pending persist so the click isn't lost.
    window.dispatchEvent(new Event('beforeunload'));
    for (let i = 0; i < 5; i++) await Promise.resolve();

    expect(saveCallCount).toBe(1);
    expect(lastSavedCfg).not.toBeNull();
    expect(lastSavedCfg!.directions).toEqual(['Northbound']);
  });
});
