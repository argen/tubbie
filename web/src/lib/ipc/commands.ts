/**
 * Typed wrappers around Tauri IPC `invoke` calls.
 *
 * Every call is typed with explicit generic parameter; no `any` used.
 * At the IPC boundary we receive `unknown` and assert via type guards.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  type Board,
  type BoardConfig,
  type LineStatus,
  type Station,
  isBoard,
  isBoardConfig,
  isLineStatus,
  isStation,
} from './types.js';

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/** Search for tube stations matching `query`. Debounced at the call site. */
export async function searchStations(query: string): Promise<Station[]> {
  const raw = await invoke<unknown>('search_stations', { query });
  if (!Array.isArray(raw)) {
    throw new TypeError(`search_stations: expected array, got ${typeof raw}`);
  }
  return raw.filter((item): item is Station => isStation(item));
}

/** Fetch the arrivals board for the currently saved config (one-shot). */
export async function getBoard(): Promise<Board> {
  const raw = await invoke<unknown>('get_board');
  if (!isBoard(raw)) {
    throw new TypeError('get_board: unexpected response shape');
  }
  return raw;
}

/** Save a BoardConfig to the Tauri store. */
export async function saveConfig(cfg: BoardConfig): Promise<void> {
  await invoke<undefined>('save_config', { cfg });
}

/** Load the saved BoardConfig (or the default). */
export async function loadConfig(): Promise<BoardConfig> {
  const raw = await invoke<unknown>('load_config');
  if (!isBoardConfig(raw)) {
    throw new TypeError('load_config: unexpected response shape');
  }
  // Backfill theme default for configs saved before M6 (theme may be absent in older persisted data)
  return { ...raw, theme: raw.theme.length > 0 ? raw.theme : 'classic-amber' };
}

/** Persist an optional TfL API key. Returns "restart to apply". */
export async function saveAppKey(key: string | null): Promise<string> {
  const raw = await invoke<unknown>('save_app_key', { key });
  if (typeof raw !== 'string') {
    throw new TypeError('save_app_key: expected string response');
  }
  return raw;
}

/** Load the stored TfL API key (`null` if none). */
export async function loadAppKey(): Promise<string | null> {
  const raw = await invoke<unknown>('load_app_key');
  if (raw === null || typeof raw === 'string') return raw;
  throw new TypeError('load_app_key: expected string | null');
}

/**
 * Returns true if a TfL API key has been stored, false otherwise.
 * Use this instead of loadAppKey when you only need to know presence —
 * the actual key value is never sent to the renderer.
 */
export async function hasAppKey(): Promise<boolean> {
  const raw = await invoke<unknown>('has_app_key');
  if (typeof raw !== 'boolean') {
    throw new TypeError('has_app_key: expected boolean response');
  }
  return raw;
}

/** Fetch the current status for a single TfL line. */
export async function getLineStatus(lineId: string): Promise<LineStatus> {
  const raw = await invoke<unknown>('get_line_status', { lineId });
  if (!isLineStatus(raw)) {
    throw new TypeError('get_line_status: unexpected response shape');
  }
  return raw;
}

/** UI-level display mode: floating window or menubar popover. */
export type DisplayMode = 'window' | 'menubar';

function isDisplayMode(value: unknown): value is DisplayMode {
  return value === 'window' || value === 'menubar';
}

/** Load the persisted display mode. Defaults to `"window"` if absent. */
export async function loadDisplayMode(): Promise<DisplayMode> {
  const raw = await invoke<unknown>('load_display_mode');
  if (!isDisplayMode(raw)) {
    throw new TypeError(`load_display_mode: unexpected value ${String(raw)}`);
  }
  return raw;
}

/**
 * Persist the display mode. Returns `"restart to apply"` because the
 * window chrome and tray icon are decided at startup — the renderer
 * should surface that hint to the user.
 */
export async function saveDisplayMode(mode: DisplayMode): Promise<string> {
  const raw = await invoke<unknown>('save_display_mode', { mode });
  if (typeof raw !== 'string') {
    throw new TypeError('save_display_mode: expected string response');
  }
  return raw;
}

// ---------------------------------------------------------------------------
// Event subscription
// ---------------------------------------------------------------------------

/**
 * Subscribe to the `board://updated` event emitted by the Rust stream wiring.
 *
 * @returns an unlisten function — call it to stop listening (e.g. in onDestroy).
 */
export async function onBoardUpdated(handler: (board: Board) => void): Promise<UnlistenFn> {
  return listen<unknown>('board://updated', (event) => {
    if (isBoard(event.payload)) {
      handler(event.payload);
    }
  });
}
