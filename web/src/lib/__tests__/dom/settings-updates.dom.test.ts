// @vitest-environment happy-dom
/**
 * Settings — Updates section state machine (M8 PR-E).
 *
 * Pins the contract for the seven UI states defined in the M8 plan,
 * minus "ready-to-restart" (collapsed into `installing` because
 * tauri-plugin-updater 2.10.1 doesn't expose download-without-install
 * on macOS — there's no observable moment between "installed" and
 * "restarting"):
 *
 *   1. never-checked        — initial render
 *   2. checking              — between user click and IPC resolve
 *   3. up-to-date            — IPC returned null
 *   4. available             — IPC returned UpdateInfo
 *   5. installing            — install_update in flight
 *   6. network-error         — install/check threw a non-signature error
 *   7. signature-error       — install/check threw "signature ..." error
 *
 * RED-first: every state's status copy + button-set + ARIA wiring
 * lives in this file. A regression that, e.g., conflates the two
 * error states or auto-retries signature-errors lights this up.
 */
import { describe, expect, it, beforeEach, vi } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import UpdatesSection from '$lib/components/UpdatesSection.svelte';
import { mockInvoke, setMockHandler, resetMockHandlers, sampleUpdateInfo } from '$lib/ipc/mock.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args: Record<string, unknown>) => mockInvoke(cmd, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
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
