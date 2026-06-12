import { afterEach, describe, expect, it } from 'vitest';
import { USE_TS_TFL_KEY, useTsTfl } from '$lib/tfl/flag.js';

interface GlobalWithStorage {
  localStorage?: Storage;
}

function stubLocalStorage(initial: Record<string, string> = {}): void {
  const store = new Map(Object.entries(initial));
  (globalThis as GlobalWithStorage).localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => {
      store.set(k, v);
    },
    removeItem: (k: string) => {
      store.delete(k);
    },
    clear: () => {
      store.clear();
    },
    key: () => null,
    length: 0,
  } as Storage;
}

afterEach(() => {
  delete (globalThis as GlobalWithStorage).localStorage;
});

describe('useTsTfl', () => {
  it('defaults to false when localStorage is absent (SSR / node)', () => {
    expect(useTsTfl()).toBe(false);
  });

  it('defaults to false when the key is unset', () => {
    stubLocalStorage();
    expect(useTsTfl()).toBe(false);
  });

  it('is true only when the key is exactly "true"', () => {
    stubLocalStorage({ [USE_TS_TFL_KEY]: 'true' });
    expect(useTsTfl()).toBe(true);
  });

  it('is false for any other truthy-looking value', () => {
    stubLocalStorage({ [USE_TS_TFL_KEY]: '1' });
    expect(useTsTfl()).toBe(false);
  });
});
