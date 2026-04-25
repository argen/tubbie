/**
 * Config store — wraps loadConfig / saveConfig IPC calls.
 *
 * The theme is part of BoardConfig (persisted to Rust store) so theme
 * changes persist across restarts.
 */

import { writable, get } from 'svelte/store';
import { loadConfig, saveConfig } from '$lib/ipc/commands.js';
import type { BoardConfig } from '$lib/ipc/types.js';

export const VALID_THEME_IDS = [
  'classic-amber',
  'classic-orange',
  'modern-white',
  'high-contrast',
] as const;

export type ThemeId = (typeof VALID_THEME_IDS)[number];

const DEFAULT_CONFIG: BoardConfig = {
  station_id: '940GZZLUBZP',
  line_ids: [],
  directions: [],
  poll_seconds: 30,
  theme: 'classic-amber',
};

export const config = writable<BoardConfig>(DEFAULT_CONFIG);
export const configError = writable<string | null>(null);

/**
 * Load the config from the Rust backend and populate the store.
 * Call once on app init.
 */
export async function initConfig(): Promise<void> {
  try {
    const loaded = await loadConfig();
    config.set(loaded);
    configError.set(null);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    configError.set(`Failed to load config: ${msg}`);
  }
}

/**
 * Save a partial config update. Merges with current, then persists.
 */
export async function updateConfig(partial: Partial<BoardConfig>): Promise<void> {
  const current = get(config);
  const updated: BoardConfig = { ...current, ...partial };
  config.set(updated);
  try {
    await saveConfig(updated);
    configError.set(null);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    configError.set(`Failed to save config: ${msg}`);
    // Rollback
    config.set(current);
  }
}

/**
 * Apply theme without saving (for live preview). Use `updateConfig` to persist.
 */
export function applyTheme(themeId: string): void {
  const valid = VALID_THEME_IDS.includes(themeId as ThemeId) ? themeId : 'classic-amber';
  document.documentElement.className = `theme-${valid}`;
}
