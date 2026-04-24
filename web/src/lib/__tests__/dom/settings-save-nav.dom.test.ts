// @vitest-environment happy-dom
/**
 * With autosave, Settings no longer has a Save button and does not navigate
 * on save — the board page subscribes to `$config` and updates in place.
 * These tests cover the autosave path: every user-driven change triggers
 * `save_config`, a "Saving…" / "Saved" chip reflects the status, and failures
 * surface via `$configError` without leaving the page.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { mockInvoke, setMockHandler, resetMockHandlers, sampleConfig } from '$lib/ipc/mock.js';
import { config, configError } from '$lib/stores/config.js';
import { goto } from '$app/navigation';
import SettingsPage from '../../../routes/settings/+page.svelte';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

describe('Settings — autosave', () => {
  beforeEach(() => {
    resetMockHandlers();
    configError.set(null);
    config.set({ ...sampleConfig });
    (goto as unknown as ReturnType<typeof vi.fn>).mockClear();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('does NOT navigate after autosave — the user stays on Settings', async () => {
    setMockHandler('save_config', () => null);

    render(SettingsPage);

    // Trigger an autosave by toggling a direction chip (direction chips are
    // always available; no station search needed).
    const northbound = await waitFor(() =>
      screen.getByRole('button', { name: /toggle northbound direction/i }),
    );
    await fireEvent.click(northbound);

    // Let the save round-trip.
    await new Promise((r) => setTimeout(r, 50));

    expect(goto).not.toHaveBeenCalledWith('/');
  });

  it('shows "Saved" after a successful autosave', async () => {
    setMockHandler('save_config', () => null);

    render(SettingsPage);

    const northbound = await waitFor(() =>
      screen.getByRole('button', { name: /toggle northbound direction/i }),
    );
    await fireEvent.click(northbound);

    const saveState = await waitFor(() => screen.getByTestId('settings-save-state'));
    await waitFor(() => {
      expect(saveState.textContent?.trim()).toBe('Saved');
    });
  });

  it('surfaces a failed autosave via $configError and stays on the page', async () => {
    setMockHandler('save_config', () => {
      throw new Error('validation: station_id invalid');
    });

    render(SettingsPage);

    const northbound = await waitFor(() =>
      screen.getByRole('button', { name: /toggle northbound direction/i }),
    );
    await fireEvent.click(northbound);

    await waitFor(() => {
      const err = get(configError);
      expect(err).not.toBeNull();
      expect(err).toContain('station_id invalid');
    });

    expect(goto).not.toHaveBeenCalledWith('/');
  });
});
