/**
 * Desktop display preferences store.
 *
 * Hydrated from `loadDisplayPrefs()` on app boot, written via
 * `saveDisplayPrefs()`. Renderer-only flags — toggling these MUST NOT
 * trigger a backend stream refetch (mirrors the `line_ids` chip-filter
 * contract in invariant #22).
 */

import { writable } from 'svelte/store';
import {
  loadDisplayPrefs,
  saveDisplayPrefs,
} from '$lib/ipc/commands.js';
import type { DisplayPrefs } from '$lib/ipc/types.js';

const DEFAULT_PREFS: DisplayPrefs = { group_destinations: false };

export const displayPrefs = writable<DisplayPrefs>(DEFAULT_PREFS);

/**
 * Load persisted prefs and seed the store. Called once at app init.
 * On any IPC error we keep the default so a missing/unwritten key never
 * leaves the UI stuck in a non-default render mode.
 */
export async function initDisplayPrefs(): Promise<DisplayPrefs> {
  try {
    const prefs = await loadDisplayPrefs();
    displayPrefs.set(prefs);
    return prefs;
  } catch {
    displayPrefs.set(DEFAULT_PREFS);
    return DEFAULT_PREFS;
  }
}

/**
 * Optimistically update the store, then persist. Rolls back on IPC error
 * so the UI never shows a value that wasn't actually saved. Never throws.
 */
export async function updateDisplayPrefs(next: DisplayPrefs): Promise<void> {
  let previous: DisplayPrefs = DEFAULT_PREFS;
  displayPrefs.update((cur) => {
    previous = cur;
    return next;
  });
  try {
    await saveDisplayPrefs(next);
  } catch {
    displayPrefs.set(previous);
  }
}
