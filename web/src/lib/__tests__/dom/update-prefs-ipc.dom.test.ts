// @vitest-environment happy-dom
/**
 * Updater IPC contract — wrappers over the four M8 PR-D commands.
 *
 * The Settings UI (PR-E) consumes these wrappers; the eight-state
 * coverage lives in `settings-updates.dom.test.ts` once the UI lands.
 * This file pins the wire-format contract between TS and the Rust
 * command handlers in `src-tauri/src/commands.rs`.
 *
 * RED-first contract:
 *   - Defaults: `auto_check: true` (opt-OUT for a live-data app).
 *   - `checkForUpdates()` returns `null` for the up-to-date path and a
 *     well-formed `UpdateInfo` for the available path.
 *   - `installUpdate()` is a no-op on the IPC boundary (success = no
 *     thrown error; the actual install runs Rust-side).
 *   - `saveUpdatePrefs(prefs)` round-trips through `loadUpdatePrefs()`.
 *   - A malformed Rust response throws `TypeError` rather than silently
 *     handing junk to the UI.
 */
import { describe, expect, it, beforeEach, vi } from 'vitest';
import {
  checkForUpdates,
  installUpdate,
  loadUpdatePrefs,
  saveUpdatePrefs,
} from '$lib/ipc/commands.js';
import {
  mockInvoke,
  setMockHandler,
  resetMockHandlers,
  sampleUpdateInfo,
  sampleUpdatePrefsDefault,
} from '$lib/ipc/mock.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));

describe('updater IPC wrappers', () => {
  beforeEach(() => {
    resetMockHandlers();
  });

  it('loadUpdatePrefs defaults to auto_check: true', async () => {
    const prefs = await loadUpdatePrefs();
    expect(prefs.auto_check).toBe(true);
    expect(prefs).toEqual(sampleUpdatePrefsDefault);
  });

  it('saveUpdatePrefs round-trips through loadUpdatePrefs', async () => {
    let stored = { ...sampleUpdatePrefsDefault };
    setMockHandler('save_update_prefs', (args) => {
      stored = args.prefs as typeof stored;
      return null;
    });
    setMockHandler('load_update_prefs', () => stored);

    await saveUpdatePrefs({ auto_check: false });
    const loaded = await loadUpdatePrefs();
    expect(loaded).toEqual({ auto_check: false });
  });

  it('checkForUpdates returns null when no update available', async () => {
    setMockHandler('check_for_updates', () => null);
    const info = await checkForUpdates();
    expect(info).toBeNull();
  });

  it('checkForUpdates returns UpdateInfo when one is available', async () => {
    setMockHandler('check_for_updates', () => sampleUpdateInfo);
    const info = await checkForUpdates();
    expect(info).not.toBeNull();
    // The wrapper validates the shape — corrupt responses throw, not
    // silently propagate.
    expect(info?.version).toBe('0.1.1');
    expect(info?.current_version).toBe('0.1.0');
    expect(typeof info?.body).toBe('string');
  });

  it('checkForUpdates throws TypeError on malformed response', async () => {
    // Defends against a Rust-side regression where the DTO grows or
    // renames a field — without this, the renderer would happily render
    // `undefined` strings in the available-update banner.
    setMockHandler('check_for_updates', () => ({ wrong: 'shape' }));
    await expect(checkForUpdates()).rejects.toThrow(TypeError);
  });

  it('installUpdate resolves on success', async () => {
    setMockHandler('install_update', () => null);
    await expect(installUpdate()).resolves.toBeUndefined();
  });

  it('installUpdate rejects when the Rust side errors', async () => {
    // Signature-failure copy in Settings is routed by the substring
    // "signature" in the error message; this test pins that the
    // wrapper propagates the error verbatim rather than swallowing it.
    setMockHandler('install_update', () => {
      throw new Error('install_update: signature mismatch');
    });
    await expect(installUpdate()).rejects.toThrow(/signature/);
  });
});
