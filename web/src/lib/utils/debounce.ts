/**
 * A debounced function with extra control surface for unmount/unload paths.
 *
 * `flush()` runs the pending invocation immediately (if any) and clears the
 * timer; `cancel()` discards a pending invocation without running it. Both
 * are no-ops when no call is queued.
 */
export interface Debounced<T extends unknown[]> {
  (...args: T): void;
  /** Run any pending invocation now. No-op if nothing is queued. */
  flush(): void;
  /** Discard any pending invocation without running it. */
  cancel(): void;
}

/**
 * Debounce utility with "latest wins" cancellation semantics.
 *
 * Returns a debounced version of `fn` that:
 *   - waits `delayMs` before calling `fn`
 *   - cancels any pending in-flight invocation when called again
 *
 * The returned function exposes `flush()` and `cancel()` for explicit
 * control — Settings uses `flush()` on `onDestroy` and `beforeunload` so a
 * chip click made just before navigating away still saves.
 *
 * Usage:
 *   const debouncedSearch = debounce(searchStations, 200);
 *   debouncedSearch(query);    // starts a 200ms timer
 *   debouncedSearch(query2);   // cancels previous, restarts timer
 *   debouncedSearch.flush();   // runs query2 immediately
 */
export function debounce<T extends unknown[]>(
  fn: (...args: T) => Promise<void> | void,
  delayMs: number,
): Debounced<T> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  // The latest args are captured here so `flush()` can replay them.
  let pendingArgs: T | null = null;

  const debounced = ((...args: T): void => {
    if (timer !== null) {
      clearTimeout(timer);
    }
    pendingArgs = args;
    timer = setTimeout(() => {
      timer = null;
      const a = pendingArgs;
      pendingArgs = null;
      if (a !== null) {
        void fn(...a);
      }
    }, delayMs);
  }) as Debounced<T>;

  debounced.flush = (): void => {
    if (timer === null) return;
    clearTimeout(timer);
    timer = null;
    const a = pendingArgs;
    pendingArgs = null;
    if (a !== null) {
      void fn(...a);
    }
  };

  debounced.cancel = (): void => {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    pendingArgs = null;
  };

  return debounced;
}

/**
 * Async debounce with "latest token wins" semantics.
 *
 * When `fn` is async, only the last-issued invocation's result is used.
 * Earlier in-flight calls are silently discarded via a generation counter.
 *
 * Returns a function that:
 *   - debounces `fn` by `delayMs`
 *   - calls `onResult` only if no newer call superseded this one
 *   - calls `onError` if the call throws (and isn't superseded)
 */
export function debounceAsync<TArgs extends unknown[], TResult>(
  fn: (...args: TArgs) => Promise<TResult>,
  delayMs: number,
  onResult: (result: TResult) => void,
  onError?: (err: unknown) => void,
): (...args: TArgs) => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let generation = 0;

  return (...args: TArgs): void => {
    if (timer !== null) clearTimeout(timer);
    const thisGen = ++generation;

    timer = setTimeout(() => {
      timer = null;
      fn(...args)
        .then((result) => {
          if (thisGen === generation) onResult(result);
        })
        .catch((err: unknown) => {
          if (thisGen === generation) onError?.(err);
        });
    }, delayMs);
  };
}
