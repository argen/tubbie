// @vitest-environment happy-dom
/**
 * DOM tests for the live display-mode toggle.
 *
 * The Rust side now applies the swap in place (tray, dock icon, window
 * chrome) — no restart needed. The renderer must:
 *   1. Reflect `$displayMode` on the radio (no separate `pendingDisplayMode`
 *      mirror that could drift from the runtime value).
 *   2. Update the `$displayMode` store as soon as `save_display_mode`
 *      resolves so downstream subscribers (popover-root chrome,
 *      Board.svelte rowsPerPlatform) react instantly.
 *   3. Roll back the optimistic store update if the IPC rejects, so a
 *      validation error doesn't leave the UI showing a mode that never
 *      took effect on the Rust side.
 *
 * Guards the regression class "user toggles mode, IPC succeeds, but the
 * frontend still thinks it's in the old mode until refresh" — exactly
 * the failure mode the watch-channel refactor exists to prevent for
 * config; same idea here for display mode.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { mockInvoke, setMockHandler, resetMockHandlers } from '$lib/ipc/mock.js';
import { displayMode } from '$lib/stores/displayMode.js';
import SettingsPage from '../../../routes/settings/+page.svelte';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

function getRadio(value: 'window' | 'menubar'): HTMLInputElement {
  const radio = document.querySelector<HTMLInputElement>(
    `input[name="display-mode"][value="${value}"]`,
  );
  if (radio === null) throw new Error(`radio for ${value} not in DOM`);
  return radio;
}

describe('Settings — live display-mode toggle', () => {
  beforeEach(() => {
    resetMockHandlers();
    displayMode.set('window');
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('radios mirror the live $displayMode store, not a stale local copy', async () => {
    render(SettingsPage);
    await waitFor(() => getRadio('window'));

    expect(getRadio('window').checked).toBe(true);
    expect(getRadio('menubar').checked).toBe(false);

    // Simulate the Rust side telling us the runtime mode changed (e.g. a
    // future code path that emits an event on tray-driven mode swap).
    // Settings should follow the store, not hold a local snapshot.
    displayMode.set('menubar');
    await waitFor(() => {
      expect(getRadio('menubar').checked).toBe(true);
    });
    expect(getRadio('window').checked).toBe(false);
  });

  it('clicking a new mode updates the displayMode store immediately on success', async () => {
    setMockHandler('save_display_mode', () => 'saved');

    render(SettingsPage);
    await waitFor(() => getRadio('menubar'));

    expect(get(displayMode)).toBe('window');

    await fireEvent.click(getRadio('menubar'));

    await waitFor(() => {
      expect(get(displayMode)).toBe('menubar');
    });
  });

  it('rolls back the displayMode store when save_display_mode rejects', async () => {
    setMockHandler('save_display_mode', () => {
      throw new Error('validation: display_mode must be …');
    });

    render(SettingsPage);
    await waitFor(() => getRadio('menubar'));

    await fireEvent.click(getRadio('menubar'));

    // Optimistic update flipped to 'menubar' synchronously, then the
    // rejected IPC must roll it back to 'window'.
    await waitFor(() => {
      expect(get(displayMode)).toBe('window');
    });
    expect(getRadio('window').checked).toBe(true);
  });

  it('does not invoke save_display_mode when re-selecting the current mode', async () => {
    const saveMock = vi.fn(() => 'saved');
    setMockHandler('save_display_mode', saveMock);

    render(SettingsPage);
    await waitFor(() => getRadio('window'));

    // Already in window mode; clicking the same radio must short-circuit.
    await fireEvent.click(getRadio('window'));

    // Give any pending microtask a chance to fire.
    await Promise.resolve();
    await Promise.resolve();

    expect(saveMock).not.toHaveBeenCalled();
  });
});
