// @vitest-environment happy-dom
/**
 * Phase 5 — board store driven by the TS `BoardStream` when `USE_TS_TFL` is on.
 *
 * Flag on, `startBoardSubscription(cfg)` builds a `BoardStream` from the shared
 * runtime service and pumps its emits through the existing `applyBoard`
 * (generated_at latest-wins, #7) — the Rust `invoke`/`board://` path is not
 * touched. `setStreamConfig(cfg)` is the TS analogue of the Rust config
 * watch-channel publish: a station change refreshes immediately (#2). After
 * cleanup, or with the flag off, it is an inert no-op.
 *
 * Uses the real `BoardStream` over a stub service (deterministic, increasing
 * `generated_at`) so this is a genuine wiring test, not a re-mock of the stream.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { Board, BoardConfig } from '$lib/ipc/types.js';

// --- Boundary mocks (hoisted) ----------------------------------------------

let flagOn = true;
vi.mock('$lib/tfl/flag.js', () => ({ useTsTfl: () => flagOn }));

let seq = 0;
function boardFor(stationId: string): Board {
  seq += 1;
  return {
    station_id: stationId,
    platforms: [],
    generated_at: `2026-01-01T00:00:${String(seq).padStart(2, '0')}Z`,
    stale_since: null,
  };
}

const refreshSpy = vi.fn(
  (c: BoardConfig): Promise<Board> => Promise.resolve(boardFor(c.station_id)),
);
vi.mock('$lib/tfl/runtime.js', () => ({
  tflRuntime: () => Promise.resolve({ client: {}, service: { refresh: refreshSpy } }),
}));

const invokeSpy = vi.fn((_cmd: string, _args?: Record<string, unknown>) => Promise.resolve(null));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeSpy(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

import {
  board,
  boardError,
  isLoading,
  startBoardSubscription,
  setStreamConfig,
} from '$lib/stores/board.js';
import { updateConfig } from '$lib/stores/config.js';

function cfg(stationId: string, over: Partial<BoardConfig> = {}): BoardConfig {
  return {
    station_id: stationId,
    line_ids: [],
    directions: [],
    poll_seconds: 30,
    theme: 'classic-amber',
    ...over,
  };
}

function resetStores(): void {
  board.set(null);
  boardError.set(null);
  isLoading.set(true);
}

// Track the active subscription's cleanup so a failed assertion never leaks a
// live stream into the next test (which would see spurious refresh calls).
let activeCleanup: (() => void) | null = null;
async function start(initial: BoardConfig): Promise<void> {
  activeCleanup = await startBoardSubscription(initial);
}

beforeEach(() => {
  flagOn = true;
  seq = 0;
  refreshSpy.mockClear();
  invokeSpy.mockClear();
  resetStores();
});
afterEach(() => {
  activeCleanup?.();
  activeCleanup = null;
  vi.clearAllMocks();
});

describe('board store — TS stream path (flag ON)', () => {
  it('emits an immediate board from the stream, never touching invoke', async () => {
    await start(cfg('A'));

    await vi.waitFor(() => {
      expect(get(board)?.station_id).toBe('A');
    });
    expect(get(isLoading)).toBe(false);
    expect(refreshSpy).toHaveBeenCalled();
    expect(invokeSpy).not.toHaveBeenCalled();
  });

  it('setStreamConfig with a new station refreshes the board immediately (#2)', async () => {
    await start(cfg('A'));
    await vi.waitFor(() => {
      expect(get(board)?.station_id).toBe('A');
    });

    setStreamConfig(cfg('B'));

    await vi.waitFor(() => {
      expect(get(board)?.station_id).toBe('B');
    });
  });

  it('cleanup stops the stream: a later setStreamConfig is a no-op', async () => {
    const cleanup = await startBoardSubscription(cfg('A'));
    await vi.waitFor(() => {
      expect(get(board)?.station_id).toBe('A');
    });
    cleanup();
    refreshSpy.mockClear();

    setStreamConfig(cfg('B'));
    await Promise.resolve();
    await Promise.resolve();

    expect(refreshSpy).not.toHaveBeenCalled();
    expect(get(board)?.station_id).toBe('A');
  });
});

describe('board store — Rust path (flag OFF) is untouched', () => {
  it('seeds via get_board and never constructs the TS stream', async () => {
    flagOn = false;

    await start(cfg('A'));

    // Rust seed path engaged (the one-shot get_board)...
    expect(invokeSpy).toHaveBeenCalledWith('get_board', undefined);
    // ...and the TS BoardStream service was never touched.
    expect(refreshSpy).not.toHaveBeenCalled();
  });

  it('setStreamConfig stays a no-op even after a flag-OFF subscription', async () => {
    flagOn = false;
    await start(cfg('A'));
    refreshSpy.mockClear();

    setStreamConfig(cfg('B'));
    await Promise.resolve();

    expect(refreshSpy).not.toHaveBeenCalled();
  });
});

describe('config.updateConfig → stream (flag ON, item 5.4)', () => {
  it('persisting a station change pushes it to the stream and refreshes (#2)', async () => {
    await start(cfg('A'));
    await vi.waitFor(() => {
      expect(get(board)?.station_id).toBe('A');
    });

    // updateConfig persists via the (mocked) save_config invoke, then publishes
    // to the stream — the TS analogue of the Rust watch-channel publish.
    await updateConfig({ station_id: 'C' });

    await vi.waitFor(() => {
      expect(get(board)?.station_id).toBe('C');
    });
  });
});

describe('setStreamConfig — no active stream', () => {
  it('is a safe no-op when the flag is off / nothing started', () => {
    flagOn = false;
    expect(() => {
      setStreamConfig(cfg('Z'));
    }).not.toThrow();
    expect(refreshSpy).not.toHaveBeenCalled();
  });
});
