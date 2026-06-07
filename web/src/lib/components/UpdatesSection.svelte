<script lang="ts">
  /**
   * Settings — Updates section (M8 PR-E + v0.1.2 restart hotfix).
   *
   * Mirrors the ApiKeySection.svelte structure: status line + hint +
   * action button(s) in a `settings__section`. The state machine is
   * pinned by `settings-updates.dom.test.ts`.
   *
   * Restart contract (v0.1.2): on macOS, `tauri-plugin-updater`'s
   * `download_and_install` stages the new bundle and returns; it does
   * NOT restart the process — the Rust `install_update` command
   * emits `updater://restart-imminent` and then calls `app.restart()`.
   * This component:
   *   - subscribes to the event in `onMount` and flips
   *     `installing` → `restarting` when it fires;
   *   - sets a 5 s safety timer on install start and flips
   *     `installing` → `restart-failed` if neither the event nor a
   *     process death has arrived by then.
   * Without the safety timer, a future regression in `app.restart()`
   * would freeze the UI in `installing` forever (the v0.1.1 bug).
   *
   * Deliberately deferred to v0.2.0:
   *   - Tray-icon amber dot (only relevant in menubar mode; needs
   *     visual verification on hardware).
   *   - 30 s post-setup() background check.
   *   - "Install on next launch" deferred-install.
   *
   * Defaults:
   *   - `auto_check: true` (opt-OUT — stale binaries with old
   *     WKWebView CVEs is the wrong default).
   *   - `aria-live="polite"` on the status (does not interrupt
   *     screen-reader users mid-board-read).
   */
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { listen } from '@tauri-apps/api/event';
  import {
    checkForUpdates,
    installUpdate,
    loadUpdatePrefs,
    saveUpdatePrefs,
  } from '$lib/ipc/commands.js';
  import type { UpdateInfo } from '$lib/ipc/types.js';

  type UpdateState =
    | { kind: 'never-checked' }
    | { kind: 'checking' }
    | { kind: 'up-to-date'; lastCheckedAt: number }
    | { kind: 'available'; info: UpdateInfo }
    | { kind: 'installing' }
    | { kind: 'restarting' }
    | { kind: 'restart-failed' }
    | { kind: 'network-error'; message: string }
    | { kind: 'signature-error'; message: string };

  // 5 s window from "install resolved" to "restart should have happened".
  // Matched in `settings-updates.dom.test.ts`. Tuned long enough to
  // tolerate `download_and_install`'s tail latency and Tauri's
  // `app.restart()` `exec`, short enough that a stuck UI surfaces
  // recovery copy before the user gives up.
  const RESTART_TIMEOUT_MS = 5_000;

  let phase = $state<UpdateState>({ kind: 'never-checked' });
  let autoCheck = $state(true);
  let currentVersion = $state('—');
  let restartTimer: ReturnType<typeof setTimeout> | null = null;

  function clearRestartTimer(): void {
    if (restartTimer !== null) {
      clearTimeout(restartTimer);
      restartTimer = null;
    }
  }

  onMount(() => {
    let unlistenRestart: (() => void) | null = null;
    void (async () => {
      try {
        const prefs = await loadUpdatePrefs();
        autoCheck = prefs.auto_check;
      } catch {
        // Default already true; nothing to do.
      }
      try {
        currentVersion = await getVersion();
      } catch {
        currentVersion = '—';
      }
      try {
        unlistenRestart = await listen('updater://restart-imminent', () => {
          // Either we're mid-install (most common) or the event raced
          // ahead of `await installUpdate()` resolving — in both cases
          // the right UI is `restarting`. The 5 s safety timer is moot
          // once the event arrives.
          clearRestartTimer();
          phase = { kind: 'restarting' };
        });
      } catch {
        // Event subscription failure is non-fatal; the 5 s timeout in
        // `handleInstall` still surfaces a recovery state if restart
        // never happens.
      }
    })();
    return () => {
      clearRestartTimer();
      unlistenRestart?.();
    };
  });

  function classifyError(err: unknown): UpdateState {
    const message = err instanceof Error ? err.message : String(err);
    // Signature failure is a SECURITY event: routed to distinct copy
    // that does NOT invite auto-retry. The Rust command formats its
    // errors with the substring "signature" when the verifier rejects.
    if (/signature/i.test(message)) {
      return { kind: 'signature-error', message };
    }
    return { kind: 'network-error', message };
  }

  async function handleCheck(): Promise<void> {
    phase = { kind: 'checking' };
    try {
      const info = await checkForUpdates();
      if (info === null) {
        phase = { kind: 'up-to-date', lastCheckedAt: Date.now() };
      } else {
        phase = { kind: 'available', info };
      }
    } catch (err: unknown) {
      phase = classifyError(err);
    }
  }

  async function handleInstall(): Promise<void> {
    phase = { kind: 'installing' };
    // Arm the safety timer BEFORE the IPC: if neither the
    // `updater://restart-imminent` event nor a process death arrives
    // within RESTART_TIMEOUT_MS we surface the recovery copy. The
    // listener in `onMount` cancels the timer on event arrival; a
    // successful restart kills the process before the timer fires.
    clearRestartTimer();
    restartTimer = setTimeout(() => {
      // Only flip if we're still in installing — the listener may have
      // moved us to `restarting` between the timer firing and this
      // callback running.
      if (phase.kind === 'installing') {
        phase = { kind: 'restart-failed' };
      }
      restartTimer = null;
    }, RESTART_TIMEOUT_MS);
    try {
      await installUpdate();
      // On macOS this resolves once the new bundle is staged. The Rust
      // command then emits `updater://restart-imminent` and calls
      // `app.restart()`. We stay in `installing` (or `restarting` if
      // the listener already fired) until the process dies — or until
      // the safety timer above lands us in `restart-failed`.
    } catch (err: unknown) {
      clearRestartTimer();
      phase = classifyError(err);
    }
  }

  async function handleAutoCheckToggle(): Promise<void> {
    const next = !autoCheck;
    autoCheck = next;
    try {
      await saveUpdatePrefs({ auto_check: next });
    } catch {
      // Revert optimistic toggle.
      autoCheck = !next;
    }
  }

  // Human-readable copy keyed off `phase.kind`. Plain text — no HTML —
  // because `aria-live="polite"` reads textContent.
  let statusLine = $derived.by(() => {
    switch (phase.kind) {
      case 'never-checked':
        return `Tubbie ${currentVersion}`;
      case 'checking':
        return 'Checking for updates…';
      case 'up-to-date':
        return `You're up to date · Tubbie ${currentVersion}`;
      case 'available':
        return `Update available — Tubbie ${phase.info.version}`;
      case 'installing':
        return 'Installing — please don’t close Tubbie';
      case 'restarting':
        return 'Restarting Tubbie…';
      case 'restart-failed':
        return 'Update installed, but Tubbie couldn’t restart automatically. Quit Tubbie and open it again to finish.';
      case 'network-error':
        return 'Couldn’t reach update server. Try again in a moment.';
      case 'signature-error':
        return 'Update verification failed. Your installed version is safe; if this keeps happening, re-download from github.com/argen/tubbie/releases.';
    }
  });

  let checkButtonLabel = $derived.by(() => {
    if (phase.kind === 'checking') return 'Checking…';
    if (phase.kind === 'available' || phase.kind === 'installing' || phase.kind === 'restarting')
      return 'Check for updates';
    if (phase.kind === 'network-error' || phase.kind === 'signature-error') return 'Try again';
    return 'Check for updates';
  });

  let checkButtonDisabled = $derived.by(
    () => phase.kind === 'checking' || phase.kind === 'installing' || phase.kind === 'restarting',
  );

  let showInstallButton = $derived(
    phase.kind === 'available' || phase.kind === 'installing' || phase.kind === 'restarting',
  );

  let installButtonDisabled = $derived(phase.kind === 'installing' || phase.kind === 'restarting');
</script>

<section class="settings__section" aria-labelledby="section-updates">
  <h2 id="section-updates" class="settings__section-title">Updates</h2>

  <p class="settings__api-status" aria-live="polite" data-testid="updates-status">
    {statusLine}
  </p>

  <p class="settings__api-hint">
    Tubbie checks the GitHub Releases feed for signed updates. The download is verified against a
    public key bundled with this app — a third party can’t push a replacement.
  </p>

  <div class="settings__api-actions">
    <button
      type="button"
      class="settings__btn settings__btn--secondary"
      onclick={handleCheck}
      disabled={checkButtonDisabled}
      data-testid="updates-check-btn"
      aria-label="Check for updates now"
    >
      {checkButtonLabel}
    </button>
    {#if showInstallButton}
      <button
        type="button"
        class="settings__btn"
        onclick={handleInstall}
        disabled={installButtonDisabled}
        data-testid="updates-install-btn"
        aria-label="Install update and restart Tubbie"
      >
        {#if phase.kind === 'installing'}
          Installing…
        {:else if phase.kind === 'restarting'}
          Restarting…
        {:else}
          Install and restart
        {/if}
      </button>
    {/if}
  </div>

  <label class="settings__toggle-row">
    <input
      type="checkbox"
      checked={autoCheck}
      onchange={handleAutoCheckToggle}
      data-testid="updates-auto-check"
      aria-describedby="updates-auto-check-hint"
    />
    <span>Check for updates automatically</span>
  </label>
  <p id="updates-auto-check-hint" class="settings__api-hint settings__api-hint--small">
    Defaults to on so security fixes reach you without you having to remember.
  </p>
</section>

<style>
  /* Shared rules (`.settings__section`, `.settings__section-title`,
     `.settings__api-status`, `.settings__api-hint`, `.settings__btn*`)
     live in `SettingsView.svelte` under `:global(...)`. This
     component only adds rules specific to the toggle row. */

  .settings__toggle-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin-top: 0.6rem;
    font-family: var(--font-ui);
    color: var(--fg);
    cursor: pointer;
  }

  .settings__toggle-row input[type='checkbox'] {
    width: 1rem;
    height: 1rem;
    accent-color: var(--fg);
    cursor: pointer;
  }
</style>
