// @vitest-environment happy-dom
/**
 * After a successful Save, the Settings page must send the user back to
 * the board view so they see the new arrivals load. A failed save keeps
 * them on the page so they can see and retry the error.
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

describe('Settings — save navigates back to /', () => {
  beforeEach(() => {
    resetMockHandlers();
    configError.set(null);
    config.set({ ...sampleConfig });
    (goto as unknown as ReturnType<typeof vi.fn>).mockClear();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('navigates to / after a successful save', async () => {
    setMockHandler('save_config', () => null);

    render(SettingsPage);

    const saveBtn = await waitFor(() => screen.getByRole('button', { name: /save settings/i }));
    await fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(goto).toHaveBeenCalledWith('/');
    });
  });

  it('does NOT navigate when save fails', async () => {
    setMockHandler('save_config', () => {
      throw new Error('validation: station_id invalid');
    });

    render(SettingsPage);

    const saveBtn = await waitFor(() => screen.getByRole('button', { name: /save settings/i }));
    await fireEvent.click(saveBtn);

    // The configError store receives the wrapped message from updateConfig.
    await waitFor(() => {
      const err = get(configError);
      expect(err).not.toBeNull();
      expect(err).toContain('station_id invalid');
    });

    expect(goto).not.toHaveBeenCalledWith('/');
  });
});
