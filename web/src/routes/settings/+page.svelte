<script lang="ts">
  import { configError } from '$lib/stores/config.js';
  import StationSection from '$lib/components/StationSection.svelte';
  import FavoritesSection from '$lib/components/FavoritesSection.svelte';
  import LinesSection from '$lib/components/LinesSection.svelte';
  import DirectionsSection from '$lib/components/DirectionsSection.svelte';
  import PollIntervalSection from '$lib/components/PollIntervalSection.svelte';
  import ThemeSection from '$lib/components/ThemeSection.svelte';
  import DisplayModeSection from '$lib/components/DisplayModeSection.svelte';
  import DisplayPrefsSection from '$lib/components/DisplayPrefsSection.svelte';
  import ApiKeySection from '$lib/components/ApiKeySection.svelte';
  import UpdatesSection from '$lib/components/UpdatesSection.svelte';
  import AboutSection from '$lib/components/AboutSection.svelte';
  import {
    saveState,
    flushPending,
    cancelSaveStateTimer,
    resyncFormFromConfig,
  } from '$lib/stores/settingsForm.js';
  import { onDestroy, onMount } from 'svelte';

  // The page is now a thin shell: the nine section components own their
  // own state, handlers, and styles, all wired up through
  // `$lib/stores/settingsForm`. This shell handles only:
  //   - the configError banner
  //   - the header (back button + saving/saved chip)
  //   - the form re-sync on mount (the module-scoped store outlives the
  //     page, so navigation back to /settings without resyncing would
  //     show stale field values)
  //   - flushing the pending debounced persist on onDestroy + beforeunload

  onMount(() => {
    resyncFormFromConfig();
  });

  // beforeunload fires when the window/tab closes or the user navigates
  // away via the browser. In Tauri windowed mode this is the only signal
  // we get before the renderer tears down — `onDestroy` covers SPA route
  // changes back to "/", but a hard close goes through here.
  if (typeof window !== 'undefined') {
    window.addEventListener('beforeunload', flushPending);
  }

  onDestroy(() => {
    flushPending();
    cancelSaveStateTimer();
    if (typeof window !== 'undefined') {
      window.removeEventListener('beforeunload', flushPending);
    }
  });

  // Settings now runs in its own webview window. "Back" closes this window
  // rather than SPA-navigating — the main board window stays open independently.
  async function handleBack(): Promise<void> {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().close();
    } catch {
      // Fallback for non-Tauri contexts (vitest, plain vite dev).
      // In a real browser `window.close()` only works if the window was
      // opened by script; in Tauri it always works.
      window.close();
    }
  }
</script>

<svelte:head>
  <title>tubbie — Settings</title>
</svelte:head>

<div class="settings" aria-label="Settings page">
  {#if $configError}
    <div class="settings__config-error" role="alert" aria-live="assertive">
      <span class="settings__config-error-text">{$configError}</span>
      <button
        type="button"
        class="settings__config-error-dismiss"
        onclick={() => {
          configError.set(null);
        }}
        aria-label="Dismiss error"
      >
        ✕
      </button>
    </div>
  {/if}

  <header class="settings__header">
    <button
      type="button"
      class="settings__back-btn"
      onclick={handleBack}
      aria-label="Back to arrivals board"
    >
      ← Back
    </button>
    <h1 class="settings__title">Settings</h1>
    <span
      class="settings__save-state"
      class:settings__save-state--saving={$saveState === 'saving'}
      class:settings__save-state--saved={$saveState === 'saved'}
      role="status"
      aria-live="polite"
      data-testid="settings-save-state"
    >
      {#if $saveState === 'saving'}
        Saving…
      {:else if $saveState === 'saved'}
        Saved
      {/if}
    </span>
  </header>

  <div class="settings__body">
    <StationSection />

    <FavoritesSection />

    <LinesSection />

    <DirectionsSection />

    <PollIntervalSection />

    <ThemeSection />

    <DisplayModeSection />

    <DisplayPrefsSection />

    <ApiKeySection />

    <UpdatesSection />

    <AboutSection />
  </div>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    min-height: calc(100vh - 24px);
    background: var(--bg);
    color: var(--fg);
  }

  .settings__config-error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.6rem 1.5rem;
    background: color-mix(in srgb, var(--stale-accent) 15%, var(--bg));
    border: 1px solid var(--stale-accent);
    border-radius: 2px;
    margin: 0.75rem 1.5rem 0;
    font-family: var(--font-ui);
    font-size: 0.95rem;
    color: var(--stale-accent);
  }

  .settings__config-error-text {
    flex: 1;
  }

  .settings__config-error-dismiss {
    background: transparent;
    border: none;
    color: var(--stale-accent);
    cursor: pointer;
    font-size: 1rem;
    padding: 0 0.2rem;
    opacity: 0.8;
    flex-shrink: 0;
  }

  .settings__config-error-dismiss:hover {
    opacity: 1;
  }

  .settings__header {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.75rem 1.5rem;
    border-bottom: 1px solid var(--row-divider);
    flex-shrink: 0;
  }

  .settings__back-btn {
    font-family: var(--font-ui);
    font-size: 1rem;
    color: var(--fg);
    background: transparent;
    border: 1px solid var(--row-divider);
    padding: 0.3rem 0.75rem;
    cursor: pointer;
    letter-spacing: 0.05em;
    border-radius: 2px;
  }

  .settings__back-btn:hover,
  .settings__back-btn:focus {
    border-color: var(--fg);
    background: var(--chip-bg);
  }

  .settings__title {
    font-family: var(--font-board);
    font-size: 1.3rem;
    color: var(--accent);
    margin: 0;
    letter-spacing: 0.1em;
    font-weight: 400;
  }

  .settings__body {
    flex: 1;
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 2rem;
    max-width: 640px;
    width: 100%;
    margin: 0 auto;
    overflow-y: auto;
  }

  /* `:global` so child components (ApiKeySection.svelte) can use the same
     classes without duplicating the rules. */
  :global(.settings__section) {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  :global(.settings__section-title) {
    font-family: var(--font-board);
    font-size: 1.1rem;
    color: var(--platform-label);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    margin: 0;
    font-weight: 400;
    border-bottom: 1px solid var(--row-divider);
    padding-bottom: 0.3rem;
  }

  /* `:global` so the Lines / Directions section components can use it. */
  :global(.settings__section-hint) {
    font-size: 0.7rem;
    opacity: 0.5;
    text-transform: lowercase;
    letter-spacing: 0.03em;
    margin-left: 0.5rem;
  }

  /* Chips. `:global` so the Lines / Directions / Favorites sections
     share these classes without duplicating the rules. */
  :global(.settings__chips) {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  :global(.settings__chip) {
    font-family: var(--font-ui);
    font-size: 0.9rem;
    background: var(--chip-bg);
    color: var(--fg);
    border: 1px solid var(--input-border);
    padding: 0.25rem 0.6rem;
    cursor: pointer;
    letter-spacing: 0.05em;
    border-radius: 2px;
    opacity: 0.8;
  }

  :global(.settings__chip:hover) {
    opacity: 1;
    border-color: var(--platform-label);
  }

  :global(.settings__chip--selected) {
    background: var(--chip-selected-bg);
    color: var(--chip-selected-fg);
    border-color: var(--chip-selected-bg);
    opacity: 1;
  }

  :global(.settings__chip--unavailable) {
    opacity: 0.3;
    cursor: not-allowed;
    border-style: dashed;
  }

  :global(.settings__chip--unavailable:hover) {
    opacity: 0.3;
    border-color: var(--input-border);
  }

  /* Shared status / hint typography. `:global` so ApiKeySection.svelte
     can use the same classes without duplicating the rules. */
  :global(.settings__api-status) {
    font-family: var(--font-ui);
    font-size: 0.95rem;
    color: var(--accent);
    margin: 0;
    opacity: 0.9;
  }

  :global(.settings__api-hint) {
    font-family: var(--font-ui);
    font-size: 0.85rem;
    color: var(--platform-label);
    margin: 0;
    opacity: 0.7;
    line-height: 1.4;
  }

  /* Buttons. `:global` so ApiKeySection.svelte can use the same classes
     without duplicating the rules. */
  :global(.settings__btn) {
    font-family: var(--font-ui);
    font-size: 1.1rem;
    padding: 0.5rem 1.5rem;
    border: none;
    border-radius: 2px;
    cursor: pointer;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    width: fit-content;
  }

  :global(.settings__btn:disabled) {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(.settings__btn--secondary) {
    background: transparent;
    color: var(--fg);
    border: 1px solid var(--input-border);
  }

  :global(.settings__btn--secondary:hover:not(:disabled)),
  :global(.settings__btn--secondary:focus:not(:disabled)) {
    border-color: var(--fg);
  }

  .settings__save-state {
    font-family: var(--font-ui);
    font-size: 0.8rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    opacity: 0;
    transition: opacity 200ms ease-out;
    color: var(--platform-label);
    min-width: 4.5rem;
    text-align: right;
    margin-left: auto;
  }

  .settings__save-state--saving {
    opacity: 0.9;
    color: var(--platform-label);
  }

  .settings__save-state--saved {
    opacity: 0.9;
    color: var(--accent);
  }
</style>
