/**
 * Pool-key bridge to the Rust shell for the TypeScript data path (`USE_TS_TFL`).
 *
 * The (public) TfL pool keys live at `POOL_KEYS_URL`, but that endpoint sends no
 * `Access-Control-Allow-Origin`, so the webview can't read it cross-origin. The
 * Rust shell's `reqwest` is immune to webview CORS, so it fetches the list and
 * exposes it via the `get_pool_keys` command. The runtime calls this first and
 * falls back to a direct fetch (anonymous) on `[]` — hence the fail-open.
 *
 * Kept in its own module (not `commands.ts`) so `tfl/runtime.ts` can import it
 * without forming a cycle with `commands.ts` (which imports the runtime).
 */
import { invoke } from '@tauri-apps/api/core';

/**
 * The public pool keys from the Rust shell, or `[]` outside Tauri / on any
 * error. Coerces the IPC `unknown` to `string[]`, dropping non-string entries.
 */
export async function getPoolKeysFromShell(): Promise<string[]> {
  try {
    const raw = await invoke<unknown>('get_pool_keys');
    if (!Array.isArray(raw)) return [];
    return raw.filter((k): k is string => typeof k === 'string');
  } catch {
    return [];
  }
}
