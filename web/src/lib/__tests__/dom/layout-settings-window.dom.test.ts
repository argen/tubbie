// @vitest-environment happy-dom
/**
 * Root-layout bootstrap contract after Settings moved in-frame (PR2).
 *
 * Settings is no longer a separate "settings" webview window, so the layout no
 * longer branches on `getCurrentWindow().label` to skip board bootstrap. There
 * is exactly one window now, and it ALWAYS bootstraps. The tray "Settings…"
 * item emits an `open-settings` event; the layout listens for it and flips the
 * `settingsOpen` store, which mounts the in-frame Settings overlay.
 *
 * Contract:
 *   1. startBoardSubscription IS called on mount (unconditionally).
 *   2. The layout registers a listener for the `open-settings` event.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import { resetMockHandlers, mockInvoke } from '$lib/ipc/mock.js';

// Board module mock — inline spy so no hoisting issues.
vi.mock('$lib/stores/board.js', () => {
  const spy = vi.fn(() => Promise.resolve(() => undefined));
  return {
    startBoardSubscription: spy,
    __spy: spy,
    board: {
      subscribe: (fn: (v: null) => void) => {
        fn(null);
        return () => undefined;
      },
    },
    boardError: {
      subscribe: (fn: (v: null) => void) => {
        fn(null);
        return () => undefined;
      },
    },
    isLoading: {
      subscribe: (fn: (v: boolean) => void) => {
        fn(false);
        return () => undefined;
      },
    },
    lastUpdateTs: {
      subscribe: (fn: (v: number) => void) => {
        fn(0);
        return () => undefined;
      },
    },
  };
});

vi.mock('$lib/stores/config.js', () => ({
  initConfig: vi.fn(() => Promise.resolve()),
  // Full BoardConfig shape — the layout now statically imports SettingsView,
  // whose sections init `settingsForm` from `get(config)` (reads line_ids,
  // directions, poll_seconds), so a partial stub would throw at module load.
  config: {
    subscribe: (
      fn: (v: {
        theme: string;
        station_id: string;
        line_ids: string[];
        directions: string[];
        poll_seconds: number;
      }) => void,
    ) => {
      fn({
        theme: 'classic-amber',
        station_id: '940GZZLUOXC',
        line_ids: [],
        directions: [],
        poll_seconds: 30,
      });
      return () => undefined;
    },
  },
  applyTheme: vi.fn(),
  configError: {
    subscribe: (fn: (v: null) => void) => {
      fn(null);
      return () => undefined;
    },
  },
}));

vi.mock('$lib/stores/displayMode.js', () => ({
  initDisplayMode: vi.fn(() => Promise.resolve()),
  displayMode: {
    subscribe: (fn: (v: string) => void) => {
      fn('window');
      return () => undefined;
    },
  },
}));

vi.mock('$lib/stores/displayPrefs.js', () => ({
  initDisplayPrefs: vi.fn(() => Promise.resolve()),
  displayPrefs: {
    subscribe: (fn: (v: { group_destinations: boolean }) => void) => {
      fn({ group_destinations: false });
      return () => undefined;
    },
  },
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));

// Record event listener registrations so we can assert the layout wires up
// the `open-settings` tray event.
const listenSpy = vi.fn((_eventName: string, _handler: (e: { payload: unknown }) => void) =>
  Promise.resolve(() => undefined),
);
vi.mock('@tauri-apps/api/event', () => ({
  listen: (eventName: string, handler: (e: { payload: unknown }) => void) =>
    listenSpy(eventName, handler),
}));

// Import after all vi.mock() calls so mocks are registered first.
import Layout from '../../../routes/+layout.svelte';

const childStub = (() => undefined) as unknown as import('svelte').Snippet;

describe('+layout.svelte — single-window bootstrap (Settings is in-frame)', () => {
  beforeEach(() => {
    resetMockHandlers();
    listenSpy.mockClear();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('always calls startBoardSubscription on mount (no window-label skip)', async () => {
    const { __spy } = (await import('$lib/stores/board.js')) as unknown as {
      __spy: ReturnType<typeof vi.fn>;
    };
    __spy.mockClear();

    render(Layout, { props: { children: childStub } });
    await new Promise((r) => setTimeout(r, 50));

    expect(__spy).toHaveBeenCalledOnce();
  });

  it('registers an `open-settings` event listener for the tray menu', async () => {
    render(Layout, { props: { children: childStub } });
    await new Promise((r) => setTimeout(r, 50));

    expect(listenSpy).toHaveBeenCalledWith('open-settings', expect.any(Function));
  });
});
