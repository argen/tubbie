// @vitest-environment happy-dom
/**
 * `getPoolKeysFromShell` — the renderer's bridge to the Rust `get_pool_keys`
 * command (Phase 5 pool-key auth). The webview can't read POOL_KEYS_URL
 * directly (no ACAO), so the shell proxies the public keys. Must coerce the IPC
 * response and fail-open to `[]` (so the runtime falls back to a direct fetch /
 * anonymous) on any error, non-array, or non-Tauri context.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invokeSpy = vi.fn((_cmd: string): Promise<unknown> => Promise.resolve([]));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string) => invokeSpy(cmd),
}));

import { getPoolKeysFromShell } from '$lib/ipc/poolKeys.js';

beforeEach(() => {
  invokeSpy.mockReset();
});
afterEach(() => {
  vi.clearAllMocks();
});

describe('getPoolKeysFromShell', () => {
  it('invokes get_pool_keys and returns the string list', async () => {
    invokeSpy.mockResolvedValue([
      'aaaabbbbccccddddaaaabbbbccccdddd',
      '1111222233334444111122223333444b',
    ]);
    const keys = await getPoolKeysFromShell();
    expect(invokeSpy).toHaveBeenCalledWith('get_pool_keys');
    expect(keys).toEqual(['aaaabbbbccccddddaaaabbbbccccdddd', '1111222233334444111122223333444b']);
  });

  it('drops non-string entries defensively', async () => {
    invokeSpy.mockResolvedValue(['aaaabbbbccccddddaaaabbbbccccdddd', 42, null, 'x']);
    expect(await getPoolKeysFromShell()).toEqual(['aaaabbbbccccddddaaaabbbbccccdddd', 'x']);
  });

  it('fails open to [] when the response is not an array', async () => {
    invokeSpy.mockResolvedValue({ keys: ['x'] });
    expect(await getPoolKeysFromShell()).toEqual([]);
  });

  it('fails open to [] when invoke throws (non-Tauri / command missing)', async () => {
    invokeSpy.mockRejectedValue(new Error('not running under Tauri'));
    expect(await getPoolKeysFromShell()).toEqual([]);
  });
});
