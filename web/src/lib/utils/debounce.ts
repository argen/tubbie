/**
 * Debounce utility with "latest wins" cancellation semantics.
 *
 * Returns a debounced version of `fn` that:
 *   - waits `delayMs` before calling `fn`
 *   - cancels any pending in-flight invocation when called again
 *
 * Usage:
 *   const debouncedSearch = debounce(searchStations, 200);
 *   debouncedSearch(query);    // starts a 200ms timer
 *   debouncedSearch(query2);   // cancels previous, restarts timer
 */
export function debounce<T extends unknown[]>(
  fn: (...args: T) => Promise<void> | void,
  delayMs: number,
): (...args: T) => void {
  let timer: ReturnType<typeof setTimeout> | null = null;

  return (...args: T): void => {
    if (timer !== null) {
      clearTimeout(timer);
    }
    timer = setTimeout(() => {
      timer = null;
      void fn(...args);
    }, delayMs);
  };
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
