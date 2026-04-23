/**
 * Reactive store for `prefers-reduced-motion: reduce`.
 *
 * Use this in animation logic to decide whether to skip transitions.
 * CSS animations should ALSO use `@media (prefers-reduced-motion: reduce)`.
 *
 * This module is safe to import in a Node (non-browser) test environment:
 * when `window` / `matchMedia` are absent, `reducedMotion` is `false`.
 */

import { readable } from 'svelte/store';

function getReducedMotionQuery(): MediaQueryList | null {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return null;
  }
  return window.matchMedia('(prefers-reduced-motion: reduce)');
}

/**
 * A Svelte readable store that is `true` when the user prefers reduced motion.
 *
 * Updates reactively if the user changes the OS accessibility setting while
 * the app is running.
 */
export const reducedMotion = readable<boolean>(false, (set) => {
  const mq = getReducedMotionQuery();
  if (!mq) {
    // Non-browser or matchMedia unavailable — default to no reduction.
    set(false);
    return;
  }

  set(mq.matches);

  const handler = (e: MediaQueryListEvent): void => {
    set(e.matches);
  };

  mq.addEventListener('change', handler);
  return () => {
    mq.removeEventListener('change', handler);
  };
});
