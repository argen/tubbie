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

import { writable } from 'svelte/store';
import { onBoardUpdated } from '$lib/ipc/commands.js';
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
export async function startBoardSubscription(): Promise<() => void> {
  try {
    unlisten = await onBoardUpdated((b) => {
      board.set(b);
      boardError.set(null);
      isLoading.set(false);
      lastUpdateTs.set(Date.now());
    });
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    boardError.set(`Failed to subscribe to board updates: ${msg}`);
    isLoading.set(false);
  }

  return () => {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
  };
}
