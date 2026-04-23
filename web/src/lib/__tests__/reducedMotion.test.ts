import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

// We test the reducedMotion store using a matchMedia mock.
// The store module is loaded once per test file; we need to mock matchMedia
// before importing the store.

describe('reducedMotion store', () => {
  let mqListeners: Map<string, ((e: Partial<MediaQueryListEvent>) => void)[]>;

  function createMockMq(matches: boolean): MediaQueryList {
    return {
      matches,
      media: '(prefers-reduced-motion: reduce)',
      onchange: null,
      addEventListener: vi.fn(
        (event: string, handler: (e: Partial<MediaQueryListEvent>) => void) => {
          const list = mqListeners.get(event) ?? [];
          list.push(handler);
          mqListeners.set(event, list);
        },
      ),
      removeEventListener: vi.fn(
        (event: string, handler: (e: Partial<MediaQueryListEvent>) => void) => {
          const list = mqListeners.get(event) ?? [];
          const idx = list.indexOf(handler);
          if (idx !== -1) list.splice(idx, 1);
        },
      ),
      dispatchEvent: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
    } as unknown as MediaQueryList;
  }

  beforeEach(() => {
    mqListeners = new Map();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    // Clear module cache so each test gets a fresh store
    vi.resetModules();
  });

  it('returns false when prefers-reduced-motion does not match', async () => {
    // Mock window.matchMedia to return matches: false
    vi.stubGlobal('window', {
      matchMedia: (_query: string) => createMockMq(false),
    });

    const { reducedMotion } = await import('$lib/stores/reducedMotion.js');
    expect(get(reducedMotion)).toBe(false);
  });

  it('returns true when prefers-reduced-motion matches', async () => {
    vi.stubGlobal('window', {
      matchMedia: (_query: string) => createMockMq(true),
    });

    const { reducedMotion } = await import('$lib/stores/reducedMotion.js');
    expect(get(reducedMotion)).toBe(true);
  });

  it('updates reactively when OS setting changes', async () => {
    vi.stubGlobal('window', {
      matchMedia: (_query: string) => createMockMq(false),
    });

    const { reducedMotion } = await import('$lib/stores/reducedMotion.js');

    // Keep an active subscription so the store's addEventListener stays registered
    const values: boolean[] = [];
    const unsubscribe = reducedMotion.subscribe((v) => values.push(v));

    // Initially false
    expect(values[0]).toBe(false);

    // Simulate OS preference change by calling all registered 'change' handlers
    const handlers = mqListeners.get('change') ?? [];
    expect(handlers.length).toBeGreaterThan(0);
    for (const h of handlers) {
      h({ matches: true });
    }

    // Store should have updated to true
    expect(values[values.length - 1]).toBe(true);

    unsubscribe();
  });

  it('returns false in non-browser environment (no matchMedia)', async () => {
    // Remove window.matchMedia
    vi.stubGlobal('window', { matchMedia: undefined });

    const { reducedMotion } = await import('$lib/stores/reducedMotion.js');
    expect(get(reducedMotion)).toBe(false);
  });

  it('returns false when window is undefined', async () => {
    vi.stubGlobal('window', undefined);

    const { reducedMotion } = await import('$lib/stores/reducedMotion.js');
    expect(get(reducedMotion)).toBe(false);
  });
});
