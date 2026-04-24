// @vitest-environment happy-dom
/**
 * Integration tests for board store seeding — Fix 4.
 *
 * Verifies that startBoardSubscription():
 *   1. Seeds the store from get_board when the event stream hasn't fired yet.
 *   2. Applies "latest wins" when the stream emits a newer board after the seed.
 *   3. Does NOT regress to an older board if the seed is older than a live event.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { get } from 'svelte/store';
import { emitMockEvent, mockListen, sampleBoard } from '$lib/ipc/mock.js';
import type { Board } from '$lib/ipc/types.js';

// ---------------------------------------------------------------------------
// Tauri API mocks — must be hoisted, so no top-level variable references.
// ---------------------------------------------------------------------------

// We control invoke behaviour via this module-level spy.
// Using a plain function reference so mockImplementation typing is flexible.
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

// ---------------------------------------------------------------------------
// Board type helpers
// ---------------------------------------------------------------------------

const olderBoard: Board = {
  ...sampleBoard,
  generated_at: '2025-01-15T09:59:00Z', // older than sampleBoard's 10:00:00Z
};

const newerBoard: Board = {
  ...sampleBoard,
  generated_at: '2025-01-15T10:01:00Z', // newer
};

// Import store AFTER mocks are set up (static import is fine — mocks are hoisted).
import { board, boardError, isLoading, startBoardSubscription } from '$lib/stores/board.js';

function resetStores(): void {
  board.set(null);
  boardError.set(null);
  isLoading.set(true);
}

describe('board store — Fix 4: seed via getBoard()', () => {
  beforeEach(() => {
    resetStores();
    invokeSpy.mockReset();
    // Default: get_board returns sampleBoard; all other commands return null.
    invokeSpy.mockImplementation((cmd: string) => {
      if (cmd === 'get_board') return Promise.resolve(sampleBoard);
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('seeds the store from get_board when no event has fired yet', async () => {
    const cleanup = await startBoardSubscription();

    expect(get(board)).not.toBeNull();
    expect(get(board)?.station_id).toBe('940GZZLUBZP');

    cleanup();
    resetStores();
  });

  it('isLoading becomes false after seed resolves', async () => {
    const cleanup = await startBoardSubscription();

    expect(get(isLoading)).toBe(false);

    cleanup();
    resetStores();
  });

  it('later stream event wins over older seed (latest-wins by generated_at)', async () => {
    invokeSpy.mockImplementation((cmd: string) => {
      if (cmd === 'get_board') return Promise.resolve(olderBoard);
      return Promise.resolve(null);
    });

    const cleanup = await startBoardSubscription();

    // After seed, store has older board.
    expect(get(board)?.generated_at).toBe(olderBoard.generated_at);

    // Stream emits a newer board.
    emitMockEvent('board://updated', newerBoard);

    expect(get(board)?.generated_at).toBe(newerBoard.generated_at);

    cleanup();
    resetStores();
  });

  it('older seed does NOT overwrite a newer stream event already in store', async () => {
    // Pre-populate with a newer board (simulating stream having fired first).
    board.set(newerBoard);
    boardError.set(null);
    isLoading.set(false);

    // Seed resolves with an older board.
    invokeSpy.mockImplementation((cmd: string) => {
      if (cmd === 'get_board') return Promise.resolve(olderBoard);
      return Promise.resolve(null);
    });

    const cleanup = await startBoardSubscription();

    // The store should still hold the newer board.
    expect(get(board)?.generated_at).toBe(newerBoard.generated_at);

    cleanup();
    resetStores();
  });

  it('boardError is null after a successful seed', async () => {
    const cleanup = await startBoardSubscription();

    expect(get(boardError)).toBeNull();

    cleanup();
    resetStores();
  });
});
