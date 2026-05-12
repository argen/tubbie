// @vitest-environment happy-dom
/**
 * Tests that the root layout short-circuits its main-window bootstrap when
 * running inside the "settings" webview window.
 *
 * Before this fix the layout unconditionally ran startBoardSubscription(),
 * initConfig(), initDisplayMode() and subscribed to tray://open-settings.
 * That was wrong when window.label === "settings" — it double-subscribed the
 * board stream, ran config init from the wrong context, and could trigger a
 * recursive openSettingsWindow() loop.
 *
 * Fix: branch on getCurrentWindow().label at the top of onMount and skip all
 * board/config/display-mode wiring when the label is "settings".
 *
 * RED → GREEN contract:
 *   1. When window label is "settings" → startBoardSubscription is NOT called.
 *   2. When window label is "main"     → startBoardSubscription IS called.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import { resetMockHandlers, mockInvoke, mockListen } from '$lib/ipc/mock.js';

// ---------------------------------------------------------------------------
// Window label — mutable so each test can set its own label.
// ---------------------------------------------------------------------------
let _windowLabel = 'main';

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ label: _windowLabel }),
}));

// ---------------------------------------------------------------------------
// Board module mock — inline spy so no hoisting issues.
// ---------------------------------------------------------------------------
vi.mock('$lib/stores/board.js', () => {
  const spy = vi.fn(() => Promise.resolve(() => undefined));
  // Expose via a named export so tests can read it.
  return {
    startBoardSubscription: spy,
    __spy: spy,
    board: { subscribe: (fn: (v: null) => void) => { fn(null); return () => undefined; } },
    boardError: { subscribe: (fn: (v: null) => void) => { fn(null); return () => undefined; } },
    isLoading: { subscribe: (fn: (v: boolean) => void) => { fn(false); return () => undefined; } },
    lastUpdateTs: { subscribe: (fn: (v: number) => void) => { fn(0); return () => undefined; } },
  };
});

vi.mock('$lib/stores/config.js', () => ({
  initConfig: vi.fn(() => Promise.resolve()),
  config: {
    subscribe: (fn: (v: { theme: string }) => void) => {
      fn({ theme: 'classic-amber' });
      return () => undefined;
    },
  },
  applyTheme: vi.fn(),
  configError: { subscribe: (fn: (v: null) => void) => { fn(null); return () => undefined; } },
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

vi.mock('$lib/ipc/commands.js', () => ({
  openSettingsWindow: vi.fn(() => Promise.resolve()),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (eventName: string, handler: (e: { payload: unknown }) => void) =>
    mockListen(eventName, handler),
}));

// Import after all vi.mock() calls so mocks are registered first.
import Layout from '../../../routes/+layout.svelte';

describe('+layout.svelte — settings window short-circuit', () => {
  beforeEach(() => {
    resetMockHandlers();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('does NOT call startBoardSubscription when window label is "settings"', async () => {
    _windowLabel = 'settings';

    const { __spy } = await import('$lib/stores/board.js') as unknown as { __spy: ReturnType<typeof vi.fn> };
    __spy.mockClear();

    render(Layout, {
      props: {
        children: (() => undefined) as unknown as import('svelte').Snippet,
      },
    });

    // Allow onMount microtasks to settle.
    await new Promise((r) => setTimeout(r, 50));

    expect(__spy).not.toHaveBeenCalled();
  });

  it('DOES call startBoardSubscription when window label is "main"', async () => {
    _windowLabel = 'main';

    const { __spy } = await import('$lib/stores/board.js') as unknown as { __spy: ReturnType<typeof vi.fn> };
    __spy.mockClear();

    render(Layout, {
      props: {
        children: (() => undefined) as unknown as import('svelte').Snippet,
      },
    });

    // Allow onMount microtasks to settle.
    await new Promise((r) => setTimeout(r, 50));

    expect(__spy).toHaveBeenCalledOnce();
  });
});
