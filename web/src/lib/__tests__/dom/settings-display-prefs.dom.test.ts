// @vitest-environment happy-dom
/**
 * Settings — opt-in "group same destination" toggle (Phase 3).
 *
 * Asserts:
 *   1. The toggle reflects `$displayPrefs.group_destinations`.
 *   2. Clicking it calls `save_display_prefs` with the next value AND
 *      updates the store immediately (optimistic write so the rendered
 *      board collapses within a frame, not on the next poll tick —
 *      mirrors the chip-filter contract in invariant #22).
 *   3. The toggle MUST NOT cause a `save_config` invocation (the prefs
 *      bypass the cfg pipeline; selecting a station via save_config
 *      would force a stream refetch and the user just wanted to flip
 *      a render flag).
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { mockInvoke, setMockHandler, resetMockHandlers } from '$lib/ipc/mock.js';
import { displayPrefs } from '$lib/stores/displayPrefs.js';
import SettingsPage from '../../../routes/settings/+page.svelte';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

function getToggle(): HTMLInputElement {
  const el = document.querySelector<HTMLInputElement>(
    'input[data-testid="settings-group-destinations"]',
  );
  if (el === null) throw new Error('group_destinations toggle not in DOM');
  return el;
}

describe('Settings — display-prefs toggle', () => {
  beforeEach(() => {
    resetMockHandlers();
    displayPrefs.set({ group_destinations: false });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('reflects $displayPrefs.group_destinations', async () => {
    render(SettingsPage);
    await waitFor(() => getToggle());
    // Wait for the on-mount `initDisplayPrefs()` to resolve and seed the
    // store from IPC (mock returns `false`) — otherwise a `set` here can
    // race the async load.
    await waitFor(() => {
      expect(getToggle().checked).toBe(false);
    });

    displayPrefs.set({ group_destinations: true });
    await waitFor(() => {
      expect(getToggle().checked).toBe(true);
    });
  });

  it('clicking the toggle persists the new value AND updates the store immediately', async () => {
    const saveCalls: { group_destinations: boolean }[] = [];
    setMockHandler('save_display_prefs', (args) => {
      saveCalls.push((args.prefs as { group_destinations: boolean }) ?? { group_destinations: false });
      return null;
    });

    render(SettingsPage);
    await waitFor(() => getToggle());
    // Let the on-mount load settle so the click is the only event the
    // assertions below have to reason about.
    await waitFor(() => {
      expect(get(displayPrefs).group_destinations).toBe(false);
    });

    await fireEvent.click(getToggle());

    await waitFor(() => {
      expect(get(displayPrefs).group_destinations).toBe(true);
    });
    expect(saveCalls.length).toBeGreaterThanOrEqual(1);
    const last = saveCalls[saveCalls.length - 1];
    expect(last?.group_destinations).toBe(true);
  });

  it('does not invoke save_config when toggled (no stream refetch)', async () => {
    const cfgCalls: unknown[] = [];
    setMockHandler('save_config', (args) => {
      cfgCalls.push(args);
      return null;
    });
    setMockHandler('save_display_prefs', () => null);

    render(SettingsPage);
    await waitFor(() => getToggle());

    await fireEvent.click(getToggle());

    // Wait a tick so any (incorrect) save_config invocation has time to land.
    await new Promise((r) => setTimeout(r, 30));
    expect(cfgCalls).toHaveLength(0);
  });
});
