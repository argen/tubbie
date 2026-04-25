/**
 * Display mode store.
 *
 * The Tauri Rust side decides at startup whether to launch as a floating
 * window or a menubar popover. The frontend reads that decision once and
 * exposes it as a Svelte store so layout / Board components can switch
 * chrome (rounded popover card vs. plain board) and density (rows per
 * direction) accordingly.
 *
 * Saving a new mode from Settings persists to the store but does NOT
 * mutate this store value — the change only takes effect on restart.
 */

import { writable } from 'svelte/store';
import { loadDisplayMode, type DisplayMode } from '$lib/ipc/commands.js';

export const displayMode = writable<DisplayMode>('window');

/**
 * Load the persisted display mode and seed the store. Call once at
 * app init (layout's onMount).
 *
 * Falls back to `"window"` on any IPC error so the UI never gets
 * stuck in popover styling without a real Rust signal.
 */
export async function initDisplayMode(): Promise<DisplayMode> {
  try {
    const mode = await loadDisplayMode();
    displayMode.set(mode);
    return mode;
  } catch {
    displayMode.set('window');
    return 'window';
  }
}
