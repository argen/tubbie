/**
 * DOM test setup file.
 *
 * - Installs @testing-library/svelte auto-cleanup (unmounts + clears DOM after each test).
 * - Polyfills Element.prototype.animate so Svelte fly/fade transitions don't throw
 *   in happy-dom (which doesn't implement the Web Animations API).
 */

// Auto-cleanup: unmounts components and resets DOM after each test.
import '@testing-library/svelte/vitest';

// Web Animations API polyfill — happy-dom does not implement element.animate().
// We replace it with a no-op that returns a minimal Animation-like object so
// Svelte's transition system can call .cancel() / .finished without throwing.
Element.prototype.animate = function (): Animation {
  const noop: Partial<Animation> = {
    cancel: () => undefined,
    finish: () => undefined,
    pause: () => undefined,
    play: () => undefined,
    reverse: () => undefined,
    commitStyles: () => undefined,
    persist: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => false,
    finished: Promise.resolve({} as Animation),
    ready: Promise.resolve({} as Animation),
    currentTime: 0,
    startTime: null,
    playState: 'finished' as AnimationPlayState,
    effect: null,
    id: '',
    pending: false,
    playbackRate: 1,
    replaceState: 'active' as AnimationReplaceState,
    timeline: null,
    oncancel: null,
    onfinish: null,
    onremove: null,
  };
  return noop as Animation;
};
