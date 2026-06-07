/**
 * Mock IPC layer for tests.
 *
 * Tests import from here instead of `@tauri-apps/api` to avoid requiring a
 * real Tauri runtime.
 *
 * Usage:
 *   vi.mock('@tauri-apps/api/core', () => ({ invoke: mockInvoke }));
 *   vi.mock('@tauri-apps/api/event', () => ({ listen: mockListen }));
 */
import type {
  Board,
  BoardConfig,
  LineStatus,
  LocationFix,
  NearbyStation,
  Station,
  UpdateInfo,
  UpdatePrefs,
} from './types.js';

// ---------------------------------------------------------------------------
// Sample fixture data (valid instances of every IPC type)
// ---------------------------------------------------------------------------

export const sampleArrival = {
  id: '100',
  station_name: 'Belsize Park Underground Station',
  platform_name: 'Northbound - Platform 1',
  line_id: 'northern',
  line_name: 'Northern',
  direction: 'Northbound' as const,
  destination_name: 'Edgware Underground Station',
  towards: 'Edgware via CX',
  current_location: 'Approaching Belsize Park',
  time_to_station: 90,
  expected_arrival: '2025-01-15T10:01:30Z',
  naptan_id: '940GZZLUBZP',
};

export const samplePlatform = {
  name: 'Northbound - Platform 1',
  arrivals: [sampleArrival],
};

export const sampleBoard: Board = {
  station_id: '940GZZLUBZP',
  platforms: [samplePlatform],
  generated_at: '2025-01-15T10:00:00Z',
  stale_since: null,
};

export const sampleStaleBoard: Board = {
  ...sampleBoard,
  stale_since: '2025-01-15T10:00:30Z',
};

export const sampleStation: Station = {
  id: '940GZZLUBZP',
  common_name: 'Belsize Park',
  modes: ['tube'],
  lat: 51.5501,
  lon: -0.1641,
  lines: [{ id: 'northern', name: 'Northern' }],
};

export const sampleConfig: BoardConfig = {
  station_id: '940GZZLUBZP',
  line_ids: [],
  directions: [],
  poll_seconds: 20,
  theme: 'classic-amber',
};

export const sampleLineStatus: LineStatus = {
  line_id: 'northern',
  status: [{ severity: 10, description: 'Good Service' }],
  disruption_text: null,
};

export const sampleLocationFix: LocationFix = {
  // Bank station coordinates — useful for fixture tests that want a real
  // central-London point.
  lat: 51.5133,
  lon: -0.0886,
  accuracy_m: 12,
};

export const sampleNearbyStations: NearbyStation[] = [
  {
    station: {
      id: '940GZZLUBNK',
      common_name: 'Bank Underground Station',
      modes: ['tube'],
      lat: 51.5133,
      lon: -0.0886,
      lines: [{ id: 'central', name: 'Central' }],
    },
    distance_m: 0,
  },
  {
    station: {
      id: '940GZZLUMTC',
      common_name: 'Monument Underground Station',
      modes: ['tube'],
      lat: 51.5108,
      lon: -0.0863,
      lines: [{ id: 'district', name: 'District' }],
    },
    distance_m: 320,
  },
  {
    station: {
      id: '940GZZLUSPL',
      common_name: "St Paul's Underground Station",
      modes: ['tube'],
      lat: 51.5146,
      lon: -0.0973,
      lines: [{ id: 'central', name: 'Central' }],
    },
    distance_m: 620,
  },
];

export const sampleDisruptedLineStatus: LineStatus = {
  line_id: 'northern',
  status: [{ severity: 5, description: 'Minor Delays' }],
  disruption_text: 'NORTHERN LINE: Minor delays due to an earlier signal failure at London Bridge.',
};

/** Default update prefs as the Rust side returns when nothing has been saved. */
export const sampleUpdatePrefsDefault: UpdatePrefs = {
  auto_check: true,
};

/** Sample available-update info — used by Settings DOM tests in PR-E. */
export const sampleUpdateInfo: UpdateInfo = {
  version: '0.1.1',
  current_version: '0.1.0',
  body: '### Tubbie 0.1.1\n\n- Bug fixes\n',
};

// ---------------------------------------------------------------------------
// Mock invoke / listen
// ---------------------------------------------------------------------------

type MockInvokeHandler = (args: Record<string, unknown>) => unknown;

const defaultHandlers: Record<string, MockInvokeHandler> = {
  search_stations: () => [sampleStation],
  get_board: () => sampleBoard,
  save_config: () => null,
  load_config: () => sampleConfig,
  save_app_key: () => 'restart to apply',
  load_app_key: () => null,
  has_app_key: () => false,
  open_settings_window: () => null,
  get_line_status: () => sampleLineStatus,
  get_all_line_statuses: () => [sampleLineStatus],
  load_display_mode: () => 'window',
  save_display_mode: () => 'saved',
  load_display_prefs: () => ({ group_destinations: false }),
  save_display_prefs: () => null,
  apply_board_size: () => null,
  list_favorites: () => [],
  add_favorite: () => [],
  remove_favorite: () => [],
  find_nearest_stations: () => sampleNearbyStations,
  request_current_location: () => sampleLocationFix,
  // Updater (M8 PR-D). Default: no update available; default prefs.
  // DOM tests in PR-E override these via setMockHandler() to exercise
  // the eight Settings states.
  check_for_updates: () => null,
  install_update: () => null,
  load_update_prefs: () => sampleUpdatePrefsDefault,
  save_update_prefs: () => null,
};

let handlers: Record<string, MockInvokeHandler> = { ...defaultHandlers };

/** Override a command handler for a test. */
export function setMockHandler(cmd: string, fn: MockInvokeHandler): void {
  handlers[cmd] = fn;
}

/** Reset all handlers back to defaults. */
export function resetMockHandlers(): void {
  handlers = { ...defaultHandlers };
}

export function mockInvoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const handler = handlers[cmd];
  if (!handler) throw new Error(`mockInvoke: unknown command "${cmd}"`);
  return Promise.resolve(handler(args));
}

// ---------------------------------------------------------------------------
// Mock event listener
// ---------------------------------------------------------------------------

type ListenHandler = (event: { payload: unknown }) => void;

const listenerMap = new Map<string, ListenHandler[]>();

/** Simulate emitting a Tauri event to all registered listeners. */
export function emitMockEvent(eventName: string, payload: unknown): void {
  const listeners = listenerMap.get(eventName) ?? [];
  for (const fn of listeners) {
    fn({ payload });
  }
}

export function mockListen(eventName: string, handler: ListenHandler): Promise<() => void> {
  const list = listenerMap.get(eventName) ?? [];
  list.push(handler);
  listenerMap.set(eventName, list);
  return Promise.resolve(() => {
    const updated = listenerMap.get(eventName) ?? [];
    const idx = updated.indexOf(handler);
    if (idx !== -1) updated.splice(idx, 1);
  });
}

/** Clear all registered mock event listeners. */
export function clearMockListeners(): void {
  listenerMap.clear();
}
