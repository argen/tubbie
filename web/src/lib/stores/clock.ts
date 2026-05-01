/**
 * Shared 1 Hz wall-clock store.
 *
 * Lazily starts a single setInterval on first subscription and stops it
 * when the last subscriber unsubscribes. One ticking source feeds every
 * arrival row instead of each row spinning up its own timer — at a busy
 * 4-line interchange that is ~30 timers vs. 1, and they all render in
 * sync rather than at drifting phases.
 *
 * Components subscribe with `$now` to derive a wall-clock anchor and
 * re-render every second.
 */

import { readable, type Readable } from 'svelte/store';

const TICK_MS = 1000;

export const now: Readable<number> = readable(Date.now(), (set) => {
  set(Date.now());
  const id = setInterval(() => {
    set(Date.now());
  }, TICK_MS);
  return () => {
    clearInterval(id);
  };
});
