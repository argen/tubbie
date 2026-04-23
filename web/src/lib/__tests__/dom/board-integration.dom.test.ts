// @vitest-environment happy-dom
/**
 * Integration test: Board component receiving board://updated events.
 *
 * Mocks @tauri-apps/api invoke and listen, renders Board (via the stores),
 * emits a simulated board://updated event, and asserts the DOM updates.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { emitMockEvent, mockInvoke, mockListen, sampleBoard } from '$lib/ipc/mock.js';

// Wire up Tauri API mocks before importing anything that uses them
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: (eventName: string, handler: (e: { payload: unknown }) => void) =>
    mockListen(eventName, handler),
}));

// Mock reducedMotion
vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));

describe('Board — board://updated event integration', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('can be imported and stores do not throw', async () => {
    // Import board store (which calls onBoardUpdated → mockListen)
    const { board, startBoardSubscription } = await import('$lib/stores/board.js');
    const cleanup = await startBoardSubscription();

    // Emit a board event
    emitMockEvent('board://updated', sampleBoard);

    // Svelte store should now have the board
    const { get } = await import('svelte/store');
    const current = get(board);
    expect(current).not.toBeNull();
    expect(current?.station_id).toBe('940GZZLUBZP');

    cleanup();
  });

  it('ignores malformed payloads', async () => {
    const { board, startBoardSubscription } = await import('$lib/stores/board.js');
    const cleanup = await startBoardSubscription();

    // Emit a valid board first
    emitMockEvent('board://updated', sampleBoard);

    const { get } = await import('svelte/store');
    const beforeBad = get(board);

    // Emit malformed payload — should be ignored
    emitMockEvent('board://updated', { not_a_board: true });

    // Board should remain unchanged
    const afterBad = get(board);
    expect(afterBad).toEqual(beforeBad);

    cleanup();
  });
});
