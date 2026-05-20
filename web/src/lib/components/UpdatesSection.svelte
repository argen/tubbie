<script lang="ts">
  /**
   * Settings — Updates section (M8 PR-E).
   *
   * Mirrors the ApiKeySection.svelte structure: status line + hint +
   * action button(s) in a `settings__section`. The state machine
   * covers seven UI states; the contract is pinned by the DOM test
   * in `settings-updates.dom.test.ts`.
   *
   * Deliberately deferred to v0.2.0:
   *   - Tray-icon amber dot (only relevant in menubar mode; needs
   *     visual verification on hardware).
   *   - 30 s post-setup() background check.
   *   - "Install on next launch" deferred-install (tauri-plugin-
   *     updater 2.10.1 doesn't expose download-without-install on
   *     macOS; the single "Install and restart" button is the
   *     honest UX until that lands).
   *
   * Defaults:
   *   - `auto_check: true` (opt-OUT — stale binaries with old
   *     WKWebView CVEs is the wrong default).
   *   - `aria-live="polite"` on the status (does not interrupt
   *     screen-reader users mid-board-read).
   */
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
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
    | { kind: 'network-error'; message: string }
    | { kind: 'signature-error'; message: string };

  let state = $state<UpdateState>({ kind: 'never-checked' });
  let autoCheck = $state(true);
  let currentVersion = $state('—');

  onMount(async () => {
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
    state = { kind: 'checking' };
    try {
      const info = await checkForUpdates();
      if (info === null) {
        state = { kind: 'up-to-date', lastCheckedAt: Date.now() };
      } else {
        state = { kind: 'available', info };
      }
    } catch (err: unknown) {
      state = classifyError(err);
    }
  }

  async function handleInstall(): Promise<void> {
    state = { kind: 'installing' };
    try {
      await installUpdate();
      // `installUpdate` only resolves if Tauri couldn't relaunch;
      // normally the process exits + restarts before this line.
      // Stay in `installing` to avoid flashing a stale up-to-date.
    } catch (err: unknown) {
      state = classifyError(err);
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

  // Human-readable copy keyed off `state.kind`. Plain text — no HTML —
  // because `aria-live="polite"` reads textContent.
  let statusLine = $derived.by(() => {
    switch (state.kind) {
      case 'never-checked':
        return `Tubbie ${currentVersion}`;
      case 'checking':
        return 'Checking for updates…';
      case 'up-to-date':
        return `You're up to date · Tubbie ${currentVersion}`;
      case 'available':
        return `Update available — Tubbie ${state.info.version}`;
      case 'installing':
        return 'Installing — please don’t close Tubbie';
      case 'network-error':
        return 'Couldn’t reach update server. Try again in a moment.';
      case 'signature-error':
        return 'Update verification failed. Your installed version is safe; if this keeps happening, re-download from github.com/argen/tubbie/releases.';
    }
  });

  let checkButtonLabel = $derived.by(() => {
    if (state.kind === 'checking') return 'Checking…';
    if (state.kind === 'available' || state.kind === 'installing') return 'Check for updates';
    if (state.kind === 'network-error' || state.kind === 'signature-error') return 'Try again';
    return 'Check for updates';
  });

  let checkButtonDisabled = $derived.by(
    () => state.kind === 'checking' || state.kind === 'installing',
  );

  let showInstallButton = $derived(state.kind === 'available' || state.kind === 'installing');

  let installButtonDisabled = $derived(state.kind === 'installing');
</script>

<section class="settings__section" aria-labelledby="section-updates">
  <h2 id="section-updates" class="settings__section-title">Updates</h2>

  <p class="settings__api-status" aria-live="polite" data-testid="updates-status">
    {statusLine}
  </p>

  <p class="settings__api-hint">
    Tubbie checks the GitHub Releases feed for signed updates. The download is
    verified against a public key bundled with this app — a third party can’t
    push a replacement.
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
        {state.kind === 'installing' ? 'Installing…' : 'Install and restart'}
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
     live in `routes/settings/+page.svelte` under `:global(...)`. This
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
