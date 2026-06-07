<script lang="ts">
  import { configError } from '$lib/stores/config.js';
  import { closeSettings } from '$lib/stores/settingsView.js';
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
  import { onDestroy, onMount, tick } from 'svelte';

  // Settings is an in-frame panel rendered over the board (PR2) — no longer a
  // separate webview window. This component owns:
  //   - the configError banner
  //   - the header (Back button + saving/saved chip)
  //   - the form re-sync on open (the module-scoped settingsForm store outlives
  //     this component, so re-opening Settings without resyncing would show
  //     stale field values)
  //   - flushing the pending debounced persist when the panel closes/unmounts

  let backBtn = $state<HTMLButtonElement>();

  onMount(() => {
    resyncFormFromConfig();
    // Move focus into the panel on open (W3C APG dialog pattern). The Back
    // button is the natural landing spot — the primary "get out" control, first
    // in reading order. The board behind is marked `inert` in +layout.svelte, so
    // this plus the inert background gives real modality (focus can't Tab out
    // behind the overlay). Focus is returned to the gear on close by Board.svelte.
    void tick().then(() => backBtn?.focus());
  });

  // Esc closes the panel from anywhere (window-scoped, like the board's search
  // overlay). Bound here rather than on the container so it fires regardless of
  // where focus currently sits inside the panel.
  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeAndFlush();
    }
  }

  // beforeunload fires on a hard window/tab close — the only signal before the
  // renderer tears down in that path. onDestroy covers the normal close (panel
  // toggled shut), this covers the app quitting with the panel open.
  if (typeof window !== 'undefined') {
    window.addEventListener('beforeunload', flushPending);
    window.addEventListener('keydown', handleKeydown);
  }

  onDestroy(() => {
    flushPending();
    cancelSaveStateTimer();
    if (typeof window !== 'undefined') {
      window.removeEventListener('beforeunload', flushPending);
      window.removeEventListener('keydown', handleKeydown);
    }
  });

  // "Back" closes the in-frame panel (and flushes any pending debounced save so
  // a fast close-after-edit never drops a write). The board is still mounted
  // underneath, so this is an instant return with no re-fetch.
  function closeAndFlush(): void {
    flushPending();
    closeSettings();
  }
</script>

<div class="settings" aria-label="Settings panel" role="dialog" aria-modal="true">
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
      bind:this={backBtn}
      onclick={closeAndFlush}
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
    height: 100%;
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

  /* External links (About, API-key portal). `:global` so AboutSection.svelte
     and ApiKeySection.svelte share one on-brand treatment — amber, underlined —
     instead of falling back to the browser's default blue. The click itself is
     routed through the opener plugin (`openExternal`); these are styling only. */
  :global(.settings__link) {
    color: var(--fg);
    text-decoration: underline;
    text-underline-offset: 2px;
    opacity: 0.85;
    cursor: pointer;
  }

  :global(.settings__link:hover),
  :global(.settings__link:focus-visible) {
    opacity: 1;
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
