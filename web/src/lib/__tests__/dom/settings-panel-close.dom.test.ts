// @vitest-environment happy-dom
/**
 * The in-frame Settings panel (PR2) closes via the Back button and the Escape
 * key, both flipping the shared `settingsOpen` store back to false. Replaces
 * the old "Back closes the settings window" behaviour.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { mockInvoke, sampleConfig } from '$lib/ipc/mock.js';
import { config } from '$lib/stores/config.js';
import { settingsOpen } from '$lib/stores/settingsView.js';
import SettingsView from '$lib/components/SettingsView.svelte';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));
vi.mock('@tauri-apps/api/app', () => ({
  getVersion: () => Promise.resolve('1.0.0'),
}));

describe('Settings panel — close affordances', () => {
  beforeEach(() => {
    config.set({ ...sampleConfig });
    settingsOpen.set(true);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('Back button closes the panel (settingsOpen → false)', async () => {
    render(SettingsView);
    expect(get(settingsOpen)).toBe(true);
    const back = screen.getByRole('button', { name: /back to arrivals board/i });
    await fireEvent.click(back);
    expect(get(settingsOpen)).toBe(false);
  });

  it('Escape closes the panel from anywhere', async () => {
    render(SettingsView);
    expect(get(settingsOpen)).toBe(true);
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(get(settingsOpen)).toBe(false);
  });

  it('renders as a modal dialog labelled "Settings panel"', () => {
    render(SettingsView);
    const dialog = screen.getByRole('dialog', { name: /settings panel/i });
    expect(dialog).toBeTruthy();
    expect(dialog.getAttribute('aria-modal')).toBe('true');
  });
});
