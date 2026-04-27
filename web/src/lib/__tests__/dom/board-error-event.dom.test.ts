// @vitest-environment happy-dom
/**
 * Stream-error propagation: the Rust side now emits `board://error` when a
 * stream tick fails and there is no last-ok board to fall back to. Without
 * this, the renderer used to sit on "Loading arrivals…" forever — the seed
 * IPC could resolve OK while the stream was the source of failure, and the
 * stream's errors only ever reached the dev console (eprintln!).
 *
 * Contract:
 * - `board://error` payload is `{ message: string }`.
 * - When the event fires, `boardError` becomes the message and `isLoading`
 *   becomes `false` so the existing error UI in +page.svelte takes over.
 * - The existing `applyBoard` path must still clear `boardError` on the next
 *   successful emit (covered indirectly by the existing seed test suite).
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { get } from 'svelte/store';
import { emitMockEvent, mockListen, sampleBoard } from '$lib/ipc/mock.js';

const invokeSpy = vi.fn(
  (_cmd: string, _args?: Record<string, unknown>): Promise<unknown> => Promise.resolve(null),
);

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invokeSpy(cmd, args),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: (eventName: string, handler: (e: { payload: unknown }) => void) =>
    mockListen(eventName, handler),
}));

import { board, boardError, isLoading, startBoardSubscription } from '$lib/stores/board.js';

function resetStores(): void {
  board.set(null);
  boardError.set(null);
  isLoading.set(true);
}

describe('board store — board://error stream-error propagation', () => {
  beforeEach(() => {
    resetStores();
    invokeSpy.mockReset();
    // Make get_board hang so it does not race the error event in this test.
    invokeSpy.mockImplementation((cmd: string) => {
      if (cmd === 'get_board') return new Promise(() => undefined);
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  /**
   * `startBoardSubscription` registers two listeners then awaits a (here,
   * never-resolving) `getBoard`. Two microtask flushes are enough to settle
   * both `await onBoardUpdated(...)` and `await onBoardError(...)` before we
   * emit anything.
   */
  async function flushListenerRegistration(): Promise<void> {
    await Promise.resolve();
    await Promise.resolve();
  }

  it('surfaces a board://error event as boardError + clears isLoading when no board yet', async () => {
    void startBoardSubscription();
    await flushListenerRegistration();

    emitMockEvent('board://error', { message: 'rate limited by TfL API' });

    expect(get(boardError)).toBe('rate limited by TfL API');
    expect(get(isLoading)).toBe(false);
    expect(get(board)).toBeNull();
  });

  it('does NOT replace an existing board when an error event arrives', async () => {
    // User already has a board on screen.
    board.set(sampleBoard);
    boardError.set(null);
    isLoading.set(false);

    void startBoardSubscription();
    await flushListenerRegistration();

    emitMockEvent('board://error', { message: 'transient network failure' });

    // Board stays — error is silenced because the existing UI shows the board.
    expect(get(board)).not.toBeNull();
    expect(get(board)?.station_id).toBe(sampleBoard.station_id);
    // boardError stays null too: surfacing it would be a no-op (the UI gates
    // error display on `$board === null`), and a stale error string would
    // mis-render if the user navigated to a route that DOES read it.
    expect(get(boardError)).toBeNull();
  });

  it('ignores events with the wrong shape (not just a string message)', async () => {
    void startBoardSubscription();
    await flushListenerRegistration();

    emitMockEvent('board://error', { not: 'the right shape' });

    expect(get(boardError)).toBeNull();
    expect(get(isLoading)).toBe(true);
  });
});
