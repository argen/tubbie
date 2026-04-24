/**
 * Board subscription store.
 *
 * Listens to the `board://updated` Tauri event emitted by the Rust
 * stream-wiring in src-tauri/src/lib.rs. The store is populated once on
 * init via `load_config` → `get_board` (one-shot), and then updated on
 * every subsequent event emission.
 *
 * Usage:
 *   import { board, boardError, isLoading } from '$lib/stores/board.js';
 *   $board    // current Board | null
 *   $isLoading
 *   $boardError
 */

import { writable, get } from 'svelte/store';
import { onBoardUpdated, getBoard } from '$lib/ipc/commands.js';
import type { Board } from '$lib/ipc/types.js';
import type { UnlistenFn } from '@tauri-apps/api/event';

// ---------------------------------------------------------------------------
// Exported stores
// ---------------------------------------------------------------------------

export const board = writable<Board | null>(null);
export const boardError = writable<string | null>(null);
export const isLoading = writable<boolean>(true);

/** Timestamp (Date.now()) of the last successful board update, for pulse. */
export const lastUpdateTs = writable<number>(0);

// ---------------------------------------------------------------------------
// Subscription lifecycle
// ---------------------------------------------------------------------------

let unlisten: UnlistenFn | null = null;

/**
 * Start listening to `board://updated` events from the Rust backend.
 *
 * Call this once on app startup (from `+layout.svelte`).
 * Returns a cleanup function — call it when the layout is destroyed.
 */
/**
 * Handler shared between the listen callback and the seed fetch.
 * "Latest wins" by generated_at timestamp: a newer emission always replaces
 * the current store value, regardless of whether it came from the event stream
 * or the seed call.
 */
function applyBoard(b: Board): void {
  const current = get(board);
  if (current !== null && current.generated_at >= b.generated_at) {
    // Current board is newer or equal — do not regress.
    return;
  }
  board.set(b);
  boardError.set(null);
  isLoading.set(false);
  lastUpdateTs.set(Date.now());
}

export async function startBoardSubscription(): Promise<() => void> {
  try {
    // 1. Register the event listener FIRST so we don't miss early emissions.
    unlisten = await onBoardUpdated(applyBoard);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    boardError.set(`Failed to subscribe to board updates: ${msg}`);
    isLoading.set(false);
  }

  // 2. One-shot seed: fetch the current board and apply it only if the stream
  //    hasn't already populated the store (race guard: applyBoard checks timestamps).
  try {
    const seedBoard = await getBoard();
    applyBoard(seedBoard);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    // Only surface if we still have no board — don't overwrite a real board
    // if (extremely unlikely) the stream beat us to it.
    if (get(board) === null) {
      boardError.set(`Couldn't load arrivals: ${msg}. Retrying…`);
      isLoading.set(false);
    }
  }

  return () => {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
  };
}
