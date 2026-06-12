/**
 * Pool-key onboarding — port of `src-tauri/src/pool_key.rs`.
 *
 * Fetches a pool of public TfL API keys from the tubbie key-server and selects
 * one via round-robin, so the app avoids the anonymous shared-IP quota
 * (50 req/min) without any user action. The keys are already public; the
 * personal `app_key` (when present) always overrides the pool and stays a
 * Rust-only secret over IPC — it is never fetched here.
 *
 * ## Fail-open contract
 * Every failure mode (network, non-2xx, timeout, malformed JSON, wrong schema,
 * empty/all-invalid pool) resolves to `null`. The caller falls back to
 * anonymous access; the board is never blocked or delayed by this module.
 *
 * ## Endpoint & schema
 * `https://tubbie.brunobelcastro.com/pool-keys.json`, shared with iOS:
 * `{ "schema_version": 1, "keys": ["<32 hex digits>", ...] }`. Only
 * `schema_version === 1` is accepted; invalid keys are silently dropped.
 */

import { isRecord, rArray, rNumber } from '$lib/tfl/domain/raw.js';

/** Canonical pool-keys endpoint (shared with iOS). */
export const POOL_KEYS_URL = 'https://tubbie.brunobelcastro.com/pool-keys.json';

/** Hard per-request network timeout (matches the Rust / iOS `fetchTimeout`). */
export const POOL_KEYS_FETCH_TIMEOUT_MS = 3000;

/** Only this schema version is accepted. */
const ACCEPTED_SCHEMA_VERSION = 1;

/**
 * True if `key` is exactly 32 ASCII hex digits. Mirrors the Rust predicate
 * `key.len() == 32 && key.chars().all(is_ascii_hexdigit)` and the iOS
 * `k.count == 32 && k.allSatisfy(\.isHexDigit)` — both cases accepted.
 */
export function isValidPoolKey(key: string): boolean {
  return /^[0-9a-fA-F]{32}$/.test(key);
}

/**
 * A validated, immutable set of pool keys with a round-robin cursor. Build via
 * {@link KeyPool.create}; the cursor is the only mutable state.
 */
export class KeyPool {
  private readonly keys: readonly string[];
  private cursor = 0;

  private constructor(keys: readonly string[]) {
    this.keys = keys;
  }

  /**
   * Build from a raw list, silently dropping entries that fail
   * {@link isValidPoolKey}. Returns `null` when no valid keys remain (mirrors
   * `KeyPool::new` → `Option<Self>`).
   */
  static create(raw: readonly string[]): KeyPool | null {
    const keys = raw.filter(isValidPoolKey);
    return keys.length === 0 ? null : new KeyPool(keys);
  }

  /**
   * Advance the round-robin cursor and return the selected `{ slot, key }`.
   * Mirrors Rust `pick()`: `slot = oldCursor % len`, then increment. The first
   * pick is slot 0; the cursor wraps back to 0 after the last key.
   */
  pick(): { slot: number; key: string } {
    const slot = this.cursor % this.keys.length;
    this.cursor += 1;
    const key = this.keys[slot];
    if (key === undefined) {
      // Unreachable: slot < keys.length and keys is non-empty by construction.
      throw new Error('pool key cursor out of range');
    }
    return { slot, key };
  }

  /** Number of valid keys in the pool. */
  get length(): number {
    return this.keys.length;
  }
}

/**
 * Fetch and validate the pool-keys payload from `url`, returning a
 * {@link KeyPool} or `null` on any error (fail-open). `fetchImpl` is injectable
 * for tests; it defaults to the global `fetch`.
 *
 * This is the testable core. The production caller wraps it with
 * {@link POOL_KEYS_URL}.
 */
export async function fetchPoolKeys(
  url: string,
  fetchImpl: typeof fetch = (input, init) => fetch(input, init),
): Promise<KeyPool | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => {
    controller.abort();
  }, POOL_KEYS_FETCH_TIMEOUT_MS);

  let response: Response;
  try {
    response = await fetchImpl(url, { signal: controller.signal });
  } catch (e) {
    // Log only the error's name, never the raw error object — a thrown `fetch`
    // error's `message` can echo the request URL, and a future caller could
    // pass a credential-bearing URL even though today's pool-keys URL has none.
    console.warn(`[tubbie:pool-key] network error (${errName(e)}): fail-open`);
    return null;
  } finally {
    clearTimeout(timer);
  }

  if (!response.ok) {
    console.warn(`[tubbie:pool-key] server returned ${String(response.status)}: fail-open`);
    return null;
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch (e) {
    console.warn(`[tubbie:pool-key] JSON parse error (${errName(e)}): fail-open`);
    return null;
  }

  if (!isRecord(payload)) {
    console.warn('[tubbie:pool-key] payload is not an object: fail-open');
    return null;
  }

  if (rNumber(payload, 'schema_version') !== ACCEPTED_SCHEMA_VERSION) {
    console.warn('[tubbie:pool-key] unsupported schema_version: fail-open');
    return null;
  }

  const rawKeys = rArray(payload, 'keys').filter((k): k is string => typeof k === 'string');
  const pool = KeyPool.create(rawKeys);
  if (pool === null) {
    console.warn('[tubbie:pool-key] zero valid keys after filter: fail-open');
  }
  return pool;
}

/** A URL-free description of a thrown error for safe logging. */
function errName(e: unknown): string {
  return e instanceof Error ? e.name : 'unknown';
}
