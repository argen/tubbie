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
import { onBoardUpdated, onBoardError, getBoard } from '$lib/ipc/commands.js';
import type { Board, BoardConfig, BoardErrorPayload } from '$lib/ipc/types.js';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { useTsTfl } from '$lib/tfl/flag.js';
import { tflRuntime } from '$lib/tfl/runtime.js';
import { BoardStream } from '$lib/tfl/board/stream.js';

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
let unlistenError: UnlistenFn | null = null;

/**
 * The live TS `BoardStream` when `USE_TS_TFL` is on; `null` on the Rust path or
 * once the subscription is torn down. Held at module scope so config changes can
 * reach it via {@link setStreamConfig} (the TS analogue of the Rust config
 * watch-channel publish).
 */
let tsStream: BoardStream | null = null;

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
    //
    // KNOWN LIMITATION (both paths): a stale re-emit after a fetch failure
    // carries the SAME generated_at as the last-ok board (the Rust stream and
    // the TS BoardStream both set only `stale_since`, never bump generated_at),
    // so the `>=` guard drops it and the STALE badge never lights via re-emit.
    // Left as-is in Phase 5 to keep the TS and Rust paths identical; surfacing
    // staleness is a separate cross-path fix (would need a stale_since-aware
    // override here) tracked for a later phase.
    return;
  }
  board.set(b);
  boardError.set(null);
  isLoading.set(false);
  lastUpdateTs.set(Date.now());
}

/**
 * Surface a stream-side fetch failure to the user — but only when there is no
 * board on screen yet. Once a board has rendered, the existing UI gates error
 * display on `$board === null` (see `+page.svelte`), so writing the error
 * here would be invisible at best and confusing at worst when the next
 * successful tick arrives. The Rust side only emits this on a fresh error
 * streak with no last-ok fallback, so it represents a real "we have nothing
 * to show" condition.
 */
function applyBoardError(payload: BoardErrorPayload): void {
  if (get(board) !== null) {
    return;
  }
  boardError.set(payload.message);
  isLoading.set(false);
}

export async function startBoardSubscription(initialConfig?: BoardConfig): Promise<() => void> {
  // TS path (USE_TS_TFL): drive the board from the in-webview BoardStream instead
  // of the Rust `board://` event stream. The stream's immediate first emit is the
  // seed (no separate get_board one-shot), and every tick flows through the same
  // `applyBoard` so the generated_at latest-wins guard (#7) is unchanged. Needs
  // the loaded config to seed; the layout passes `$config` after initConfig().
  if (useTsTfl() && initialConfig !== undefined) {
    const { service } = await tflRuntime();
    // No get_board one-shot on this path: BoardStream.start() emits an immediate
    // board, which IS the seed (hence `getBoard` above is Rust-path-only).
    tsStream = new BoardStream(service, initialConfig, {
      onBoard: applyBoard,
      onError: applyBoardError,
    });
    tsStream.start();
    return () => {
      tsStream?.stop();
      tsStream = null;
    };
  }

  try {
    // 1. Register both event listeners FIRST so we don't miss early emissions.
    unlisten = await onBoardUpdated(applyBoard);
    unlistenError = await onBoardError(applyBoardError);
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
    if (unlistenError) {
      unlistenError();
      unlistenError = null;
    }
  };
}

/**
 * Push a new config to the live TS `BoardStream` — the analogue of the Rust
 * `save_config` → config watch-channel publish. A `station_id` change refreshes
 * immediately (#2); filter / theme / poll changes ride the diff logic inside the
 * stream. A no-op on the Rust path (`tsStream === null`), so `config.ts` can call
 * it unconditionally after every persist.
 */
export function setStreamConfig(cfg: BoardConfig): void {
  tsStream?.setConfig(cfg);
}
