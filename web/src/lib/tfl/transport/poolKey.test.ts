/**
 * Ports `src-tauri/src/pool_key.rs` tests — key validation, the round-robin
 * KeyPool, and the fail-open `fetchPoolKeys` contract (every failure → null).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { KeyPool, fetchPoolKeys, isValidPoolKey } from './poolKey.js';

const KEY_A = 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4';
const KEY_B = 'b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5';

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    headers: { get: () => null },
    json: () => Promise.resolve(body),
    text: () => Promise.resolve(typeof body === 'string' ? body : JSON.stringify(body)),
  } as unknown as Response;
}

function badJsonResponse(status: number, text: string): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    headers: { get: () => null },
    json: () => Promise.reject(new SyntaxError('Unexpected token')),
    text: () => Promise.resolve(text),
  } as unknown as Response;
}

beforeEach(() => {
  // The fail-open paths log to console.warn by design; keep test output clean.
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('isValidPoolKey', () => {
  it('accepts 32 lowercase hex digits', () => {
    expect(isValidPoolKey(KEY_A)).toBe(true);
  });

  it('accepts 32 uppercase hex digits (matches is_ascii_hexdigit)', () => {
    expect(isValidPoolKey('A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4')).toBe(true);
  });

  it('rejects too-short, too-long, and non-hex keys', () => {
    expect(isValidPoolKey('a1b2c3d4e5f6')).toBe(false);
    expect(isValidPoolKey(`${KEY_A}ff`)).toBe(false);
    expect(isValidPoolKey('z1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4')).toBe(false);
  });
});

describe('KeyPool', () => {
  it('filters invalid entries on create', () => {
    const pool = KeyPool.create([KEY_A, 'tooshort', KEY_B]);
    expect(pool?.length).toBe(2);
  });

  it('returns null when all entries are invalid', () => {
    expect(KeyPool.create(['bad', 'alsoBad'])).toBeNull();
  });

  it('returns null for an empty list', () => {
    expect(KeyPool.create([])).toBeNull();
  });

  it('round-robin wraps back to slot 0', () => {
    const pool = KeyPool.create([KEY_A, KEY_B]);
    if (pool === null) throw new Error('expected a pool');
    expect(pool.pick()).toEqual({ slot: 0, key: KEY_A });
    expect(pool.pick()).toEqual({ slot: 1, key: KEY_B });
    expect(pool.pick()).toEqual({ slot: 0, key: KEY_A });
  });
});

describe('fetchPoolKeys (fail-open)', () => {
  it('returns a pool on a valid 200 payload', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(jsonResponse(200, { schema_version: 1, keys: [KEY_A] }));
    const pool = await fetchPoolKeys(
      'http://x/pool-keys.json',
      fetchMock as unknown as typeof fetch,
    );
    expect(pool?.pick().key).toBe(KEY_A);
  });

  it('first pick is slot 0 with multiple keys', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(jsonResponse(200, { schema_version: 1, keys: [KEY_A, KEY_B] }));
    const pool = await fetchPoolKeys(
      'http://x/pool-keys.json',
      fetchMock as unknown as typeof fetch,
    );
    expect(pool?.pick()).toEqual({ slot: 0, key: KEY_A });
  });

  it('returns null on a 5xx', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(500, 'err'));
    expect(await fetchPoolKeys('http://x', fetchMock as unknown as typeof fetch)).toBeNull();
  });

  it('returns null on a 429', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(429, 'too many'));
    expect(await fetchPoolKeys('http://x', fetchMock as unknown as typeof fetch)).toBeNull();
  });

  it('returns null when the connection is refused (fetch throws)', async () => {
    const fetchMock = vi.fn().mockRejectedValue(new TypeError('fetch failed'));
    expect(await fetchPoolKeys('http://x', fetchMock as unknown as typeof fetch)).toBeNull();
  });

  it('returns null on malformed JSON', async () => {
    const fetchMock = vi.fn().mockResolvedValue(badJsonResponse(200, 'not json {{{{'));
    expect(await fetchPoolKeys('http://x', fetchMock as unknown as typeof fetch)).toBeNull();
  });

  it('returns null on an empty keys array', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { schema_version: 1, keys: [] }));
    expect(await fetchPoolKeys('http://x', fetchMock as unknown as typeof fetch)).toBeNull();
  });

  it('returns null when every key is invalid', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse(200, {
        schema_version: 1,
        keys: ['tooshort', 'notvalidhex!!!!!!!!!!!!!!!!!!'],
      }),
    );
    expect(await fetchPoolKeys('http://x', fetchMock as unknown as typeof fetch)).toBeNull();
  });

  it('returns null on an unsupported schema_version', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(jsonResponse(200, { schema_version: 2, keys: [KEY_A] }));
    expect(await fetchPoolKeys('http://x', fetchMock as unknown as typeof fetch)).toBeNull();
  });

  it('keeps only the valid keys from a mixed payload', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse(200, {
        schema_version: 1,
        keys: ['badkey', KEY_A, 'gggggggggggggggggggggggggggggggg', KEY_B],
      }),
    );
    const pool = await fetchPoolKeys('http://x', fetchMock as unknown as typeof fetch);
    expect(pool?.length).toBe(2);
  });
});
