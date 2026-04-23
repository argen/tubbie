import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { debounce, debounceAsync } from '$lib/utils/debounce.js';

describe('debounce', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('does not call fn immediately', () => {
    const fn = vi.fn();
    const debounced = debounce(fn, 200);
    debounced('a');
    expect(fn).not.toHaveBeenCalled();
  });

  it('calls fn after delay', () => {
    const fn = vi.fn();
    const debounced = debounce(fn, 200);
    debounced('a');
    vi.advanceTimersByTime(200);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith('a');
  });

  it('cancels previous call when invoked again within delay', () => {
    const fn = vi.fn();
    const debounced = debounce(fn, 200);
    debounced('a');
    vi.advanceTimersByTime(100);
    debounced('b'); // should reset timer
    vi.advanceTimersByTime(100);
    expect(fn).not.toHaveBeenCalled(); // 100ms after 'b' — not yet
    vi.advanceTimersByTime(100);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith('b'); // only 'b', not 'a'
  });

  it('calls fn once per burst', () => {
    const fn = vi.fn();
    const debounced = debounce(fn, 200);
    debounced('a');
    debounced('b');
    debounced('c');
    vi.advanceTimersByTime(200);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith('c');
  });

  it('calls fn for each separate burst', () => {
    const fn = vi.fn();
    const debounced = debounce(fn, 200);
    debounced('a');
    vi.advanceTimersByTime(250);
    debounced('b');
    vi.advanceTimersByTime(250);
    expect(fn).toHaveBeenCalledTimes(2);
  });
});

describe('debounceAsync — latest wins', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('only calls onResult for the latest invocation', async () => {
    let callCount = 0;
    const fn = async (q: string): Promise<string> => {
      callCount++;
      return q.toUpperCase();
    };
    const results: string[] = [];
    const debounced = debounceAsync(fn, 200, (r) => {
      results.push(r);
    });

    debounced('a');
    debounced('b');
    debounced('c');

    // Advance past the debounce delay
    vi.advanceTimersByTime(200);
    // Allow microtasks to flush
    await vi.runAllTimersAsync();

    // Only one fn call — only the last 'c' should have been queued
    expect(callCount).toBe(1);
    expect(results).toEqual(['C']);
  });

  it('calls onError when fn throws', async () => {
    const fn = async (_q: string): Promise<string> => {
      throw new Error('network error');
    };
    const errors: unknown[] = [];
    const debounced = debounceAsync(
      fn,
      200,
      () => undefined,
      (e) => {
        errors.push(e);
      },
    );

    debounced('x');
    vi.advanceTimersByTime(200);
    await vi.runAllTimersAsync();

    expect(errors.length).toBe(1);
    expect(errors[0]).toBeInstanceOf(Error);
  });

  it('does not call onResult for superseded calls', async () => {
    const resolvers: ((v: string) => void)[] = [];
    const fn = async (q: string): Promise<string> => {
      return new Promise((resolve) => {
        resolvers.push((v) => {
          resolve(v.length > 0 ? v : q);
        });
      });
    };
    const results: string[] = [];
    const debounced = debounceAsync(fn, 200, (r) => {
      results.push(r);
    });

    debounced('first');
    vi.advanceTimersByTime(250); // fires first
    debounced('second');
    vi.advanceTimersByTime(250); // fires second (supersedes first)

    // Resolve both promises
    for (const resolve of resolvers) resolve('');

    await vi.runAllTimersAsync();

    // Only 'second' result should be visible
    expect(results.length).toBe(1);
  });
});
