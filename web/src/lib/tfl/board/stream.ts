/**
 * `BoardStream` — the polling driver around {@link BoardService.refresh}, ported
 * from the `BoardService::stream` unfold in `crates/tfl-board/src/service.rs`.
 *
 * The Rust version is a `stream::unfold` selecting over a `watch` config channel,
 * a lifecycle phase channel, and a `tokio::time::Interval`. The idiomatic JS
 * equivalent is this event-driven class:
 *
 * - **`setConfig(cfg)`** is the analogue of `cfg_tx.send`: it diffs against the
 *   current config and applies the same side-effects.
 *   - `station_id` changed → drop `last_ok`, cancel the in-flight refresh,
 *     refresh **now**, reset the timer (#2).
 *   - `poll_seconds` changed → reschedule the timer; no refetch.
 *   - filter / theme / directions / `line_ids` changed → nothing; the next tick
 *     refreshes against the new config (coalesces a chip-toggle burst, #3).
 * - **The timer is a `setTimeout` re-armed on completion** (never `setInterval`),
 *   which is exactly `MissedTickBehavior::Skip` — a slow refresh never backlogs.
 * - **The loop never stops on error (#4).** On failure with a `last_ok` board it
 *   re-emits that board with `stale_since` set (forward-only); without one it
 *   emits an error. Polling continues either way.
 * - **Lifecycle** (`document.visibilitychange`) replaces the Rust phase channel:
 *   hidden → pause + cancel; visible → refresh + reset. The source is injected so
 *   the core stays framework- and `document`-free (and node-testable).
 * - **In-flight cancellation** uses a generation counter rather than aborting a
 *   promise: a refresh whose generation is stale when it resolves is discarded.
 *
 * Emits flow out through the `onBoard` / `onError` callbacks. The frontend
 * adapter (Phase 5) feeds `onBoard` into the existing `applyBoard`, whose
 * `generated_at` latest-wins guard (#7) is unchanged. NOTE: a stale re-emit
 * keeps the original `generated_at` (faithful to Rust); the Phase-5 adapter must
 * surface `stale_since` even when `generated_at` is unchanged, or #7 would drop
 * the staleness update.
 */

import type { Board, BoardConfig, BoardErrorPayload } from '$lib/ipc/types.js';
import type { Clock } from '../transport/clock.js';
import { SystemClock } from '../transport/clock.js';
import type { BoardService } from './boardService.js';

/** Visibility/lifecycle source — abstracts `document` so the core is portable. */
export interface Lifecycle {
  /** Is the app currently hidden/backgrounded? */
  hidden(): boolean;
  /** Subscribe to visibility changes; returns an unsubscribe function. */
  onChange(listener: () => void): () => void;
}

/** Default lifecycle backed by `document.visibilitychange`; inert under SSR/node. */
export function documentLifecycle(): Lifecycle {
  if (typeof document === 'undefined') {
    return { hidden: () => false, onChange: () => () => undefined };
  }
  return {
    hidden: () => document.hidden,
    onChange: (listener) => {
      document.addEventListener('visibilitychange', listener);
      return () => {
        document.removeEventListener('visibilitychange', listener);
      };
    },
  };
}

export interface BoardStreamOptions {
  clock?: Clock;
  lifecycle?: Lifecycle;
  /** Called with each fresh or stale-re-emitted board. */
  onBoard: (board: Board) => void;
  /** Called on a refresh failure with no last-ok board to fall back to. */
  onError: (payload: BoardErrorPayload) => void;
}

export class BoardStream {
  private readonly service: BoardService;
  private readonly clock: Clock;
  private readonly lifecycle: Lifecycle;
  private readonly onBoard: (board: Board) => void;
  private readonly onError: (payload: BoardErrorPayload) => void;

  private cfg: BoardConfig;
  private lastOk: Board | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private unsubscribe: (() => void) | null = null;
  private running = false;
  private paused = false;
  /** Bumped to discard an in-flight refresh (station change, pause, stop). */
  private generation = 0;

  constructor(service: BoardService, cfg: BoardConfig, opts: BoardStreamOptions) {
    this.service = service;
    this.cfg = cfg;
    this.clock = opts.clock ?? new SystemClock();
    this.lifecycle = opts.lifecycle ?? documentLifecycle();
    this.onBoard = opts.onBoard;
    this.onError = opts.onError;
  }

  /** Begin streaming: emit an immediate board (unless hidden) and arm the timer. */
  start(): void {
    if (this.running) return;
    this.running = true;
    this.unsubscribe = this.lifecycle.onChange(() => {
      this.onVisibilityChange();
    });
    this.paused = this.lifecycle.hidden();
    if (!this.paused) void this.refreshAndArm();
  }

  /** Apply a new config, reproducing the Rust watch-channel diff semantics. */
  setConfig(cfg: BoardConfig): void {
    const prev = this.cfg;
    this.cfg = cfg;
    if (!this.running || this.paused) return;

    if (cfg.station_id !== prev.station_id) {
      // #2: immediate feedback. Drop stale data, cancel any in-flight refresh,
      // refresh now, and reset the timer.
      this.lastOk = null;
      this.generation += 1;
      void this.refreshAndArm();
    } else if (cfg.poll_seconds !== prev.poll_seconds) {
      // Reschedule to the new cadence without a refetch.
      this.armTimer();
    }
    // Otherwise (filter / theme / directions / line_ids) the next scheduled
    // tick refreshes against the new config — no immediate work (#3).
  }

  /** Stop streaming: cancel the timer, any in-flight refresh, and the listener. */
  stop(): void {
    this.running = false;
    this.clearTimer();
    this.generation += 1;
    if (this.unsubscribe !== null) {
      this.unsubscribe();
      this.unsubscribe = null;
    }
  }

  // -------------------------------------------------------------------------
  // Internals
  // -------------------------------------------------------------------------

  private onVisibilityChange(): void {
    if (!this.running) return;
    const hidden = this.lifecycle.hidden();
    if (hidden && !this.paused) {
      // → background: pause, cancel the timer and any in-flight refresh (#8).
      this.paused = true;
      this.clearTimer();
      this.generation += 1;
    } else if (!hidden && this.paused) {
      // → foreground: resume with an immediate refresh and a reset timer. If a
      // refresh from before the pause is still pending, it resolves stale
      // (generation was bumped at pause) and discards; any timer its `maybeArm`
      // arms is immediately superseded by `refreshAndArm`'s `clearTimer`, so a
      // single timer is maintained.
      this.paused = false;
      void this.refreshAndArm();
    }
  }

  /** Refresh now (cancelling any pending tick), then re-arm the timer. */
  private async refreshAndArm(): Promise<void> {
    this.clearTimer();
    await this.runRefresh();
    this.maybeArm();
  }

  /** Periodic tick: refresh and re-arm on completion (Skip semantics). */
  private async onTick(): Promise<void> {
    if (!this.running || this.paused) return;
    await this.runRefresh();
    this.maybeArm();
  }

  /**
   * Arm the next tick unless we were stopped or paused *during* the refresh.
   * A method so the check isn't narrowed away by a preceding guard — `running`
   * and `paused` can flip across the awaited refresh (via `stop` / visibility).
   */
  private maybeArm(): void {
    if (this.running && !this.paused) this.armTimer();
  }

  private async runRefresh(): Promise<void> {
    const gen = this.generation;
    let board: Board;
    try {
      board = await this.service.refresh(this.cfg);
    } catch (e) {
      if (gen !== this.generation) return; // superseded — discard
      this.handleError(e);
      return;
    }
    if (gen !== this.generation) return; // superseded — discard
    this.lastOk = board; // refresh stamps stale_since = null on success
    this.onBoard(board);
  }

  /** On failure: re-emit the stale last-ok board (#4), or emit an error. */
  private handleError(e: unknown): void {
    if (this.lastOk !== null) {
      const stale: Board = { ...this.lastOk };
      // Forward-only: set stale_since once, at the first failure of a streak.
      stale.stale_since ??= this.clock.now().toISOString();
      this.lastOk = stale;
      this.onBoard(stale);
      return;
    }
    this.onError({ message: e instanceof Error ? e.message : 'board refresh failed' });
  }

  private armTimer(): void {
    this.clearTimer();
    const ms = Math.max(1, this.cfg.poll_seconds) * 1000;
    this.timer = setTimeout(() => {
      void this.onTick();
    }, ms);
  }

  private clearTimer(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }
}
