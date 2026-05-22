// @vitest-environment happy-dom
/**
 * Settings — Updates section state machine (M8 PR-E + v0.1.2 hotfix).
 *
 * Pins the contract for the UI states. Originally seven (M8 plan);
 * v0.1.2 added two more for the macOS restart-after-install path:
 *
 *   1. never-checked        — initial render
 *   2. checking              — between user click and IPC resolve
 *   3. up-to-date            — IPC returned null
 *   4. available             — IPC returned UpdateInfo
 *   5. installing            — install_update in flight (download + stage)
 *   6. restarting            — emitted updater://restart-imminent received
 *   7. restart-failed        — install resolved but process never died
 *   8. network-error         — install/check threw a non-signature error
 *   9. signature-error       — install/check threw "signature ..." error
 *
 * RED-first: every state's status copy + button-set + ARIA wiring
 * lives in this file. A regression that, e.g., conflates the two
 * error states or auto-retries signature-errors lights this up.
 *
 * The `restart-failed` test is the regression coverage for the v0.1.1
 * stuck-install bug: `update.download_and_install()` on macOS Tauri
 * 2.10.1 returns Ok without restarting the process, leaving the UI
 * frozen in `'installing'` forever. v0.1.2 calls `app.restart()` after
 * a successful install; if that ever stops working, this test goes red.
 */
import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import UpdatesSection from '$lib/components/UpdatesSection.svelte';
import { mockInvoke, setMockHandler, resetMockHandlers, sampleUpdateInfo } from '$lib/ipc/mock.js';

// Event mock — registered listeners are kept in this map and the test
// triggers them via `emitMockEvent`. Allows asserting that the Svelte
// component reacts to `updater://restart-imminent` from Rust.
type EventCb = (e: { event: string; payload: unknown }) => void;
const eventListeners = new Map<string, Set<EventCb>>();

function emitMockEvent(event: string, payload: unknown = null): void {
  const set = eventListeners.get(event);
  if (set === undefined) return;
  for (const cb of set) {
    cb({ event, payload });
  }
}

function resetMockEvents(): void {
  eventListeners.clear();
}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: (event: string, cb: EventCb) => {
    let set = eventListeners.get(event);
    if (set === undefined) {
      set = new Set();
      eventListeners.set(event, set);
    }
    set.add(cb);
    return Promise.resolve(() => {
      set?.delete(cb);
    });
  },
}));
vi.mock('@tauri-apps/api/app', () => ({
  getVersion: () => Promise.resolve('0.1.0'),
}));

function statusText(): string {
  const el = document.querySelector<HTMLElement>('[data-testid="updates-status"]');
  if (el === null) throw new Error('updates-status not in DOM');
  return el.textContent?.trim() ?? '';
}

function checkBtn(): HTMLButtonElement {
  const el = document.querySelector<HTMLButtonElement>('[data-testid="updates-check-btn"]');
  if (el === null) throw new Error('updates-check-btn not in DOM');
  return el;
}

function installBtn(): HTMLButtonElement | null {
  return document.querySelector<HTMLButtonElement>('[data-testid="updates-install-btn"]');
}

function autoCheckToggle(): HTMLInputElement {
  const el = document.querySelector<HTMLInputElement>('[data-testid="updates-auto-check"]');
  if (el === null) throw new Error('updates-auto-check not in DOM');
  return el;
}

describe('Settings — Updates section', () => {
  beforeEach(() => {
    resetMockHandlers();
    resetMockEvents();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders the never-checked state on mount and pins the polite live region', async () => {
    render(UpdatesSection);
    // Wait for onMount's async work (loadUpdatePrefs + getVersion) to settle.
    await waitFor(() => {
      expect(statusText()).toMatch(/Tubbie 0\.1\.0/);
    });
    expect(statusText()).not.toMatch(/up to date/i);
    expect(installBtn()).toBeNull();
    // aria-live="polite" — must NOT be aria-live="assertive" / role=alert.
    const status = document.querySelector<HTMLElement>('[data-testid="updates-status"]');
    expect(status?.getAttribute('aria-live')).toBe('polite');
    expect(status?.getAttribute('role')).not.toBe('alert');
  });

  it('clicking Check for updates flips the check button into a disabled "Checking…" state mid-flight', async () => {
    // Stall the IPC so the in-flight `checking` state is observable.
    // Without the stall, the resolution would land in the same micro-
    // task batch and the intermediate state would never be visible.
    let resolveCheck!: (v: null) => void;
    setMockHandler(
      'check_for_updates',
      () =>
        new Promise<null>((resolve) => {
          resolveCheck = resolve;
        }),
    );
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(statusText()).toMatch(/checking/i);
    });
    expect(checkBtn().disabled).toBe(true);
    // Resolve so the stalled handler can finish.
    resolveCheck(null);
    await waitFor(() => {
      expect(statusText()).toMatch(/up to date/i);
    });
  });

  it('transitions to up-to-date when the IPC returns null', async () => {
    setMockHandler('check_for_updates', () => null);
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(statusText()).toMatch(/up to date/i);
    });
    expect(statusText()).toMatch(/Tubbie 0\.1\.0/);
    expect(installBtn()).toBeNull();
  });

  it('transitions to available and reveals the install button when IPC returns UpdateInfo', async () => {
    setMockHandler('check_for_updates', () => sampleUpdateInfo);
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(statusText()).toMatch(/update available/i);
    });
    expect(statusText()).toMatch(/0\.1\.1/);
    const inst = installBtn();
    expect(inst).not.toBeNull();
    expect(inst?.disabled).toBe(false);
    expect(inst?.textContent?.trim().toLowerCase()).toContain('install and restart');
  });

  it('clicking Install transitions to installing and disables both buttons', async () => {
    setMockHandler('check_for_updates', () => sampleUpdateInfo);
    let resolveInstall!: () => void;
    setMockHandler(
      'install_update',
      () =>
        new Promise<null>((resolve) => {
          resolveInstall = () => {
            resolve(null);
          };
        }),
    );
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(installBtn()).not.toBeNull();
    });
    await fireEvent.click(installBtn()!);
    await waitFor(() => {
      expect(statusText()).toMatch(/installing/i);
    });
    expect(checkBtn().disabled).toBe(true);
    expect(installBtn()?.disabled).toBe(true);
    resolveInstall();
  });

  it('transitions installing → restarting when Rust emits updater://restart-imminent', async () => {
    // Production path for v0.1.2+: Rust emits the event right before
    // calling `app.restart()`. The Svelte component must move into a
    // distinct `restarting` UI phase so the user sees "Restarting Tubbie…"
    // for the brief window before the process actually dies.
    //
    // Regression-test for the v0.1.1 stuck-install bug.
    setMockHandler('check_for_updates', () => sampleUpdateInfo);
    // Stall `install_update` — in real life the IPC reply races the
    // restart-imminent event and the process exit. We hold the install
    // promise open so the `restarting` transition is unambiguously
    // event-driven, not install-resolve-driven.
    setMockHandler(
      'install_update',
      () =>
        new Promise<null>(() => {
          /* never resolves */
        }),
    );
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(installBtn()).not.toBeNull();
    });
    await fireEvent.click(installBtn()!);
    await waitFor(() => {
      expect(statusText()).toMatch(/installing/i);
    });
    emitMockEvent('updater://restart-imminent');
    await waitFor(() => {
      expect(statusText()).toMatch(/restarting/i);
    });
    // Both buttons must stay disabled — restart is in flight, no
    // user-actionable affordance until the new process boots.
    expect(checkBtn().disabled).toBe(true);
    expect(installBtn()?.disabled).toBe(true);
  });

  it('transitions installing → restart-failed when install resolves but no restart-imminent arrives within 5s', async () => {
    // Failure path: `download_and_install` returned Ok (bundle staged
    // on disk) but `app.restart()` never fired the event AND the process
    // wasn't killed. v0.1.1 zombie scenario; v0.1.2 surfaces a recovery
    // copy after a 5 s grace window instead of freezing on "Installing…".
    //
    // Fake timers so the 5 s wait is deterministic.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    setMockHandler('check_for_updates', () => sampleUpdateInfo);
    // install_update resolves immediately — simulates the broken path
    // where the Rust side returns Ok without ever killing the process.
    setMockHandler('install_update', () => null);
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(installBtn()).not.toBeNull();
    });
    await fireEvent.click(installBtn()!);
    await waitFor(() => {
      expect(statusText()).toMatch(/installing/i);
    });
    // Advance past the 5 s grace window. No event was emitted, the
    // process is still alive (we're in a test), so the timeout fires.
    await vi.advanceTimersByTimeAsync(5_500);
    await waitFor(() => {
      expect(statusText()).toMatch(/couldn.t restart/i);
    });
    expect(statusText()).toMatch(/quit tubbie and open it again/i);
    // Recovery copy must NOT invite an auto-retry of install (the
    // bundle is already staged; another install_update would no-op or
    // re-download for nothing). Install button is gone.
    expect(installBtn()).toBeNull();
    // Check button stays usable so the user can verify state once
    // they've manually restarted.
    expect(checkBtn().disabled).toBe(false);
  });

  it('restart-imminent event suppresses the 5s restart-failed timeout', async () => {
    // Belt-and-braces: even if `install_update` resolves cleanly AND
    // the event arrives, the 5 s timeout must NOT fire (otherwise we'd
    // flip to `restart-failed` after the process should already be
    // restarting). Pins that the event clears the timer.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    setMockHandler('check_for_updates', () => sampleUpdateInfo);
    setMockHandler('install_update', () => null);
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(installBtn()).not.toBeNull();
    });
    await fireEvent.click(installBtn()!);
    await waitFor(() => {
      expect(statusText()).toMatch(/installing/i);
    });
    emitMockEvent('updater://restart-imminent');
    await waitFor(() => {
      expect(statusText()).toMatch(/restarting/i);
    });
    // Advance past the would-be timeout window.
    await vi.advanceTimersByTimeAsync(10_000);
    // Still `restarting`, not `restart-failed`.
    expect(statusText()).toMatch(/restarting/i);
    expect(statusText()).not.toMatch(/couldn.t restart/i);
  });

  it('shows a polite network-error message when check fails with a non-signature error', async () => {
    setMockHandler('check_for_updates', () => {
      throw new Error('check_for_updates: dns lookup failed');
    });
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(statusText()).toMatch(/couldn.t reach/i);
    });
    expect(checkBtn().textContent?.toLowerCase()).toMatch(/try again/);
    // Network-error must NOT auto-retry — the button stays clickable but the
    // command isn't re-invoked until the user clicks.
    expect(checkBtn().disabled).toBe(false);
  });

  it('shows the security copy when check fails with a "signature" error', async () => {
    // This is the security-event path: a signature mismatch means the
    // download is NOT trusted. Copy MUST distinguish from network errors.
    setMockHandler('check_for_updates', () => {
      throw new Error('check_for_updates: signature mismatch');
    });
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(statusText()).toMatch(/verification failed/i);
    });
    expect(statusText()).toMatch(/installed version is safe/i);
    expect(statusText()).toMatch(/github\.com\/argen\/tubbie\/releases/i);
    // No auto-install path from this state.
    expect(installBtn()).toBeNull();
  });

  it('signature error from install_update is also routed to the security copy', async () => {
    setMockHandler('check_for_updates', () => sampleUpdateInfo);
    setMockHandler('install_update', () => {
      throw new Error('install_update: signature mismatch');
    });
    render(UpdatesSection);
    await waitFor(() => statusText());
    await fireEvent.click(checkBtn());
    await waitFor(() => {
      expect(installBtn()).not.toBeNull();
    });
    await fireEvent.click(installBtn()!);
    await waitFor(() => {
      expect(statusText()).toMatch(/verification failed/i);
    });
  });

  it('auto-check toggle defaults ON and persists via saveUpdatePrefs', async () => {
    const saved: { auto_check?: boolean } = {};
    setMockHandler('save_update_prefs', (args) => {
      saved.auto_check = (args.prefs as { auto_check: boolean }).auto_check;
      return null;
    });
    render(UpdatesSection);
    await waitFor(() => {
      expect(autoCheckToggle().checked).toBe(true);
    });
    // User opts out.
    await fireEvent.click(autoCheckToggle());
    await waitFor(() => {
      expect(saved.auto_check).toBe(false);
    });
    expect(autoCheckToggle().checked).toBe(false);
  });

  it('auto-check toggle reverts on save failure (optimistic UI honesty)', async () => {
    setMockHandler('save_update_prefs', () => {
      throw new Error('save_update_prefs: store locked');
    });
    render(UpdatesSection);
    await waitFor(() => {
      expect(autoCheckToggle().checked).toBe(true);
    });
    await fireEvent.click(autoCheckToggle());
    // The optimistic flip happens immediately, then the save throws and
    // we revert. Wait for the revert.
    await waitFor(() => {
      expect(autoCheckToggle().checked).toBe(true);
    });
  });
});
