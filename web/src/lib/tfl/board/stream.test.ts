/**
 * `BoardStream` timing, ported from the `BoardService::stream` tests in
 * `service.rs`. Uses `vi.useFakeTimers()` + `advanceTimersByTimeAsync` (the
 * analogue of Rust `start_paused` + `tokio::time::advance`) to assert the
 * observable emit sequence — never microtask yield-and-hope.
 *
 * Covers: immediate first emit, periodic tick, station change → immediate
 * refresh (#2), filter change → no refetch / rides next tick (#3), poll_seconds
 * reschedule, never-stop-on-error with stale fallback (#4), lifecycle
 * pause/resume, and Skip (no backlog while a slow refresh is in flight).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Board, BoardConfig } from '$lib/ipc/types.js';
import { FakeClock } from '../transport/clock.js';
import type { BoardService } from './boardService.js';
import { BoardStream, type Lifecycle } from './stream.js';

const EPOCH = new Date('2026-01-01T00:00:00Z');

function makeBoard(stationId: string): Board {
  return {
    station_id: stationId,
    platforms: [],
    generated_at: EPOCH.toISOString(),
    stale_since: null,
  };
}

function cfg(stationId: string, over: Partial<BoardConfig> = {}): BoardConfig {
  return {
    station_id: stationId,
    line_ids: [],
    directions: [],
    poll_seconds: 20,
    theme: 'classic-amber',
    ...over,
  };
}

class FakeLifecycle implements Lifecycle {
  private isHidden = false;
  private listeners: (() => void)[] = [];
  hidden(): boolean {
    return this.isHidden;
  }
  onChange(listener: () => void): () => void {
    this.listeners.push(listener);
    return () => undefined;
  }
  set(hidden: boolean): void {
    this.isHidden = hidden;
    for (const l of this.listeners) l();
  }
}

/** Flush pending microtasks + zero-delay timers under fake timers. */
async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
}

// A controllable refresh: each test sets `refreshImpl`.
let refreshImpl: (cfg: BoardConfig) => Promise<Board>;
const service = {
  refresh: (c: BoardConfig) => refreshImpl(c),
} as unknown as BoardService;

let onBoard: ReturnType<typeof vi.fn>;
let onError: ReturnType<typeof vi.fn>;
let lifecycle: FakeLifecycle;

function newStream(initial: BoardConfig = cfg('A')): BoardStream {
  return new BoardStream(service, initial, {
    clock: FakeClock.at(EPOCH),
    lifecycle,
    onBoard: onBoard as unknown as (b: Board) => void,
    onError: onError as unknown as (p: { message: string }) => void,
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  refreshImpl = (c) => Promise.resolve(makeBoard(c.station_id));
  onBoard = vi.fn();
  onError = vi.fn();
  lifecycle = new FakeLifecycle();
});
afterEach(() => {
  vi.useRealTimers();
});

describe('BoardStream', () => {
  it('emits an immediate board on start', async () => {
    const stream = newStream();
    stream.start();
    await settle();
    expect(onBoard).toHaveBeenCalledTimes(1);
    stream.stop();
  });

  it('emits again on each periodic tick', async () => {
    const stream = newStream();
    stream.start();
    await settle(); // emit 1
    await vi.advanceTimersByTimeAsync(20_000);
    expect(onBoard).toHaveBeenCalledTimes(2);
    stream.stop();
  });

  it('refreshes immediately on a station change, not after the full period (#2)', async () => {
    const stream = newStream(cfg('A'));
    stream.start();
    await settle();
    onBoard.mockClear();

    stream.setConfig(cfg('B'));
    await settle();

    expect(onBoard).toHaveBeenCalledTimes(1);
    expect((onBoard.mock.calls[0]?.[0] as Board).station_id).toBe('B');
    stream.stop();
  });

  it('does not refetch on a filter change; rides the next tick (#3)', async () => {
    const stream = newStream(cfg('A'));
    stream.start();
    await settle();
    onBoard.mockClear();

    stream.setConfig(cfg('A', { directions: ['Northbound'] }));
    await settle();
    expect(onBoard).toHaveBeenCalledTimes(0); // no immediate refresh

    await vi.advanceTimersByTimeAsync(20_000);
    expect(onBoard).toHaveBeenCalledTimes(1); // next tick refreshes
    stream.stop();
  });

  it('reschedules the timer when poll_seconds changes', async () => {
    const stream = newStream(cfg('A'));
    stream.start();
    await settle();
    onBoard.mockClear();

    stream.setConfig(cfg('A', { poll_seconds: 5 }));
    await vi.advanceTimersByTimeAsync(5_000);
    expect(onBoard).toHaveBeenCalledTimes(1); // fired at the new cadence
    stream.stop();
  });

  it('re-emits the last board as stale on a failure, never stopping (#4)', async () => {
    const stream = newStream(cfg('A'));
    stream.start();
    await settle(); // ok board, last_ok set
    onBoard.mockClear();

    refreshImpl = () => Promise.reject(new Error('boom'));
    await vi.advanceTimersByTimeAsync(20_000); // tick → error with last_ok

    expect(onError).not.toHaveBeenCalled();
    expect(onBoard).toHaveBeenCalledTimes(1);
    expect((onBoard.mock.calls[0]?.[0] as Board).stale_since).not.toBeNull();
    stream.stop();
  });

  it('emits an error (and keeps polling) when a failure has no last-ok board (#4)', async () => {
    refreshImpl = () => Promise.reject(new Error('boom'));
    const stream = newStream(cfg('A'));
    stream.start();
    await settle(); // first refresh fails, no last_ok

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onBoard).toHaveBeenCalledTimes(0);

    await vi.advanceTimersByTimeAsync(20_000); // still polling
    expect(onError).toHaveBeenCalledTimes(2);
    stream.stop();
  });

  it('pauses while hidden and resumes with a fresh board when visible (#8)', async () => {
    const stream = newStream(cfg('A'));
    stream.start();
    await settle();
    onBoard.mockClear();

    lifecycle.set(true); // hidden → pause
    await vi.advanceTimersByTimeAsync(60_000);
    expect(onBoard).toHaveBeenCalledTimes(0); // no ticks while hidden

    lifecycle.set(false); // visible → resume + immediate refresh
    await settle();
    expect(onBoard).toHaveBeenCalledTimes(1);
    stream.stop();
  });

  it('does not backlog while a slow refresh is in flight (Skip semantics)', async () => {
    let release: () => void = () => undefined;
    let inFlight = 0;
    let maxInFlight = 0;
    refreshImpl = async () => {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise<void>((r) => {
        release = r;
      });
      inFlight -= 1;
      return makeBoard('A');
    };

    const stream = newStream(cfg('A'));
    stream.start();
    await Promise.resolve(); // first refresh enters and suspends; no timer armed yet
    await vi.advanceTimersByTimeAsync(100_000); // no armed timer → no concurrent refresh

    expect(maxInFlight).toBe(1);

    release();
    await settle();
    stream.stop();
  });
});
