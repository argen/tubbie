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
  type BoardErrorPayload,
  type DisplayPrefs,
  type Favorite,
  type LineRef,
  type LineStatus,
  type LocationError,
  type LocationFix,
  type NearbyStation,
  type Station,
  type UpdateInfo,
  type UpdatePrefs,
  isBoard,
  isBoardConfig,
  isBoardErrorPayload,
  isDisplayPrefs,
  isFavorite,
  isLineStatus,
  isLocationFix,
  isNearbyStation,
  isStation,
  isUpdateInfo,
  isUpdatePrefs,
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

/**
 * Find the closest stations to a `(lat, lon)` query point.
 *
 * The Rust side validates lat/lon ranges, NaN/infinity, and clamps
 * `limit` to `[1, 20]`. Results are sorted ascending by haversine
 * distance and dropped past the 25 km radius defined in
 * `crates/tfl-client/src/nearest.rs`.
 */
export async function findNearestStations(
  lat: number,
  lon: number,
  limit: number,
): Promise<NearbyStation[]> {
  const raw = await invoke<unknown>('find_nearest_stations', { lat, lon, limit });
  if (!Array.isArray(raw)) {
    throw new TypeError(`find_nearest_stations: expected array, got ${typeof raw}`);
  }
  return raw.filter((item): item is NearbyStation => isNearbyStation(item));
}

/**
 * Acquire a single CoreLocation fix via the macOS native bridge.
 * Single-flight, single-shot, 8 s timeout.
 *
 * On error, resolves to a `LocationError` discriminated union — never
 * a thrown string. Each variant maps 1:1 to a listbox row in
 * `StationSearch.svelte`.
 */
export async function requestCurrentLocation(): Promise<
  { ok: true; fix: LocationFix } | { ok: false; error: LocationError }
> {
  try {
    const raw = await invoke<unknown>('request_current_location');
    if (isLocationFix(raw)) {
      return { ok: true, fix: raw };
    }
    return {
      ok: false,
      error: { kind: 'Internal', message: 'unexpected response shape' },
    };
  } catch (e: unknown) {
    // Tauri serialises `Result::Err(LocationError)` as a JSON object
    // matching the discriminated union. If something else came back
    // (string error from a panic, network failure, etc.) we collapse
    // it onto `Timeout` so the UI shows the retry-prone row.
    if (typeof e === 'object' && e !== null && 'kind' in e) {
      const kind = e.kind;
      switch (kind) {
        case 'PermissionDenied':
        case 'PermissionRestricted':
        case 'ServicesDisabled':
        case 'Timeout':
        case 'LowAccuracy':
        case 'AppBackground':
          return { ok: false, error: { kind } };
        case 'Internal': {
          const msg = (e as { message?: unknown }).message;
          return {
            ok: false,
            error: { kind: 'Internal', message: typeof msg === 'string' ? msg : '' },
          };
        }
      }
    }
    return {
      ok: false,
      error: {
        kind: 'Internal',
        message: e instanceof Error ? e.message : String(e),
      },
    };
  }
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

/**
 * Open (or focus) the Settings webview window.
 *
 * The Settings window is a separate OS window; the main board window cannot
 * navigate to /settings in-place because `load_app_key` is gated to the
 * "settings" window only (MEDIUM-2 / M7 TODO fix).
 *
 * Subsequent calls focus the already-open window rather than stacking a
 * second instance. Closing the Settings window does not close the app.
 */
export async function openSettingsWindow(): Promise<void> {
  await invoke<undefined>('open_settings_window');
}

/** Fetch the current status for a single TfL line. */
export async function getLineStatus(lineId: string): Promise<LineStatus> {
  const raw = await invoke<unknown>('get_line_status', { lineId });
  if (!isLineStatus(raw)) {
    throw new TypeError('get_line_status: unexpected response shape');
  }
  return raw;
}

/** Fetch the current status of EVERY TfL line (network-wide), worst-first. */
export async function getAllLineStatuses(): Promise<LineStatus[]> {
  const raw = await invoke<unknown>('get_all_line_statuses');
  if (!Array.isArray(raw)) {
    throw new TypeError('get_all_line_statuses: expected array response');
  }
  return raw.filter((item): item is LineStatus => isLineStatus(item));
}

/**
 * Set the menu-bar disruption indicator (swaps the tray icon to the monochrome
 * "disrupted" variant). No-op in window mode. Fire-and-forget.
 */
export async function setTrayDisruption(disrupted: boolean): Promise<void> {
  await invoke<undefined>('set_tray_disruption', { disrupted });
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
 * Persist the display mode and apply it live. The Rust side toggles the
 * tray icon, dock icon (macOS activation policy), and window chrome /
 * size / always-on-top in place — no restart needed. Returns `"saved"`
 * for a transient confirmation chip.
 */
export async function saveDisplayMode(mode: DisplayMode): Promise<string> {
  const raw = await invoke<unknown>('save_display_mode', { mode });
  if (typeof raw !== 'string') {
    throw new TypeError('save_display_mode: expected string response');
  }
  return raw;
}

/**
 * Load the persisted desktop display preferences. Defaults to
 * `{ group_destinations: false }` when the key is missing (upgrade path).
 */
export async function loadDisplayPrefs(): Promise<DisplayPrefs> {
  const raw = await invoke<unknown>('load_display_prefs');
  if (!isDisplayPrefs(raw)) {
    throw new TypeError('load_display_prefs: unexpected response shape');
  }
  return raw;
}

/**
 * Persist the desktop display preferences. Does NOT publish through the
 * config watch channel — the renderer applies the change locally as soon
 * as the IPC returns.
 */
export async function saveDisplayPrefs(prefs: DisplayPrefs): Promise<void> {
  await invoke<undefined>('save_display_prefs', { prefs });
}

/**
 * Resize the main window to the given logical-pixel dimensions. The Rust
 * side hops to the macOS main thread before reaching `NSWindow::setFrame:`
 * (Cocoa asserts main-thread-only). Validation runs Rust-side, so out-of-
 * range numbers reject before any Cocoa call is made.
 *
 * Called by `Board.svelte` whenever the line/platform count crosses a
 * preset tier — see the table in that component. Renderer-side dedupe
 * keeps the IPC quiet on every board tick; only tier transitions hit it.
 */
export async function applyBoardSize(width: number, height: number): Promise<void> {
  await invoke<undefined>('apply_board_size', { width, height });
}

// ---------------------------------------------------------------------------
// Favorites
// ---------------------------------------------------------------------------

/** Coerce an unknown IPC response into a `Favorite[]`, stripping malformed entries. */
function asFavoriteList(raw: unknown): Favorite[] {
  if (!Array.isArray(raw)) {
    throw new TypeError('favorites: expected array response');
  }
  return raw.filter((item): item is Favorite => isFavorite(item));
}

/** Return the saved favorites list (empty if none). */
export async function listFavorites(): Promise<Favorite[]> {
  const raw = await invoke<unknown>('list_favorites');
  return asFavoriteList(raw);
}

/**
 * Add a station to favorites. Idempotent on duplicate `station_id`.
 * Does NOT touch the cfg pipeline — selecting a favorite goes through
 * the existing `saveConfig` path.
 *
 * Returns the updated list.
 */
export async function addFavorite(
  stationId: string,
  commonName: string,
  lines: LineRef[],
): Promise<Favorite[]> {
  const raw = await invoke<unknown>('add_favorite', {
    stationId,
    commonName,
    lines,
  });
  return asFavoriteList(raw);
}

/** Remove a station from favorites. Returns the updated list. */
export async function removeFavorite(stationId: string): Promise<Favorite[]> {
  const raw = await invoke<unknown>('remove_favorite', { stationId });
  return asFavoriteList(raw);
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

/**
 * Subscribe to the `board://error` event. The Rust stream task emits this
 * once per fresh error streak when there is no last-ok board to fall back to —
 * the renderer is the only place a user can find out that polling is broken,
 * so we surface it as `boardError` and let the existing error UI take over.
 *
 * @returns an unlisten function — call it to stop listening.
 */
export async function onBoardError(
  handler: (payload: BoardErrorPayload) => void,
): Promise<UnlistenFn> {
  return listen<unknown>('board://error', (event) => {
    if (isBoardErrorPayload(event.payload)) {
      handler(event.payload);
    }
  });
}

// ---------------------------------------------------------------------------
// Updater (M8 PR-D)
// ---------------------------------------------------------------------------

/**
 * Check the configured updater endpoint for a newer signed version.
 *
 * Returns `null` when no update is available or when the plugin is
 * `active: false`. Returns an `UpdateInfo` when one is available.
 * Throws when the network or signature check fails — callers in
 * Settings inspect the error message for the substring `"signature"`
 * to route to the security-event copy (distinct from network-error).
 */
export async function checkForUpdates(): Promise<UpdateInfo | null> {
  const raw = await invoke<unknown>('check_for_updates');
  if (raw === null || raw === undefined) return null;
  if (!isUpdateInfo(raw)) {
    throw new TypeError('check_for_updates: unexpected response shape');
  }
  return raw;
}

/**
 * Download and install the latest signed update, then restart the app.
 *
 * The Rust side re-checks the endpoint inside the command so the install
 * operates on the currently-signed-and-published artifact — avoids
 * state-management of a held `Update` handle across IPC calls. Worst case
 * (publisher pulled the release between `checkForUpdates` and
 * `installUpdate`): the wrapper rejects and the renderer surfaces it as
 * a network-error state.
 */
export async function installUpdate(): Promise<void> {
  await invoke<undefined>('install_update');
}

/**
 * Load auto-update preferences. Defaults to `{ auto_check: true }` —
 * opt-OUT, not opt-in. Stale binaries with old WKWebView CVEs is the
 * wrong default for a live-data app.
 */
export async function loadUpdatePrefs(): Promise<UpdatePrefs> {
  const raw = await invoke<unknown>('load_update_prefs');
  if (!isUpdatePrefs(raw)) {
    throw new TypeError('load_update_prefs: unexpected response shape');
  }
  return raw;
}

/** Persist auto-update preferences. */
export async function saveUpdatePrefs(prefs: UpdatePrefs): Promise<void> {
  await invoke<undefined>('save_update_prefs', { prefs });
}
