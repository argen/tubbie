<script lang="ts">
  import { onDestroy } from 'svelte';
  import { displayMode } from '$lib/stores/displayMode.js';
  import { saveDisplayMode, type DisplayMode } from '$lib/ipc/commands.js';

  let displayModeStatus = $state<string | null>(null);
  let displayModeStatusTimer: ReturnType<typeof setTimeout> | null = null;

  onDestroy(() => {
    if (displayModeStatusTimer !== null) {
      clearTimeout(displayModeStatusTimer);
      displayModeStatusTimer = null;
    }
  });

  async function handleDisplayModeChange(next: DisplayMode): Promise<void> {
    if (next === $displayMode) return;
    const previous = $displayMode;
    // Optimistic update: flip the store before the IPC round-trip so the
    // radio + downstream UI (popover chrome, board density) react instantly.
    // We roll back on error.
    displayMode.set(next);
    try {
      await saveDisplayMode(next);
      showDisplayModeStatus(`Switched to ${prettyDisplayMode(next)}.`);
    } catch (err: unknown) {
      displayMode.set(previous);
      showDisplayModeStatus(`Error: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  function prettyDisplayMode(mode: DisplayMode): string {
    return mode === 'menubar' ? 'Menu bar popover' : 'Floating window';
  }

  function showDisplayModeStatus(text: string): void {
    displayModeStatus = text;
    if (displayModeStatusTimer !== null) {
      clearTimeout(displayModeStatusTimer);
    }
    displayModeStatusTimer = setTimeout(() => {
      displayModeStatus = null;
      displayModeStatusTimer = null;
    }, 2400);
  }
</script>

<section class="settings__section" aria-labelledby="section-display-mode">
  <h2 id="section-display-mode" class="settings__section-title">Display mode</h2>
  <p class="settings__api-hint">Choose how Tubbie shows its board. Changes apply immediately.</p>
  <div class="settings__display-mode" role="radiogroup" aria-label="Display mode">
    <label class="settings__display-mode-option">
      <input
        type="radio"
        name="display-mode"
        value="window"
        checked={$displayMode === 'window'}
        onchange={() => void handleDisplayModeChange('window')}
      />
      <span class="settings__display-mode-label">Floating window</span>
      <span class="settings__display-mode-hint">
        Resizable desktop window with full board layout.
      </span>
    </label>
    <label class="settings__display-mode-option">
      <input
        type="radio"
        name="display-mode"
        value="menubar"
        checked={$displayMode === 'menubar'}
        onchange={() => void handleDisplayModeChange('menubar')}
      />
      <span class="settings__display-mode-label">Menu bar popover</span>
      <span class="settings__display-mode-hint">
        Compact popover anchored to a menu bar icon.
      </span>
    </label>
  </div>
  {#if displayModeStatus}
    <p class="settings__api-status" aria-live="polite">{displayModeStatus}</p>
  {/if}
</section>

<style>
  /* Display-mode-only rules. Shared rules (.settings__section,
     .settings__section-title, .settings__api-hint, .settings__api-status)
     live as :global in routes/settings/+page.svelte. */

  .settings__display-mode {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .settings__display-mode-option {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: 0.6rem;
    row-gap: 0.15rem;
    align-items: baseline;
    padding: 0.5rem 0.75rem;
    background: var(--chip-bg);
    border: 1px solid var(--input-border);
    border-radius: 2px;
    cursor: pointer;
  }

  .settings__display-mode-option:hover,
  .settings__display-mode-option:focus-within {
    border-color: var(--platform-label);
  }

  .settings__display-mode-option input[type='radio'] {
    grid-row: 1 / span 2;
    accent-color: var(--fg);
    margin: 0;
  }

  .settings__display-mode-label {
    font-family: var(--font-ui);
    font-size: 1rem;
    color: var(--fg);
  }

  .settings__display-mode-hint {
    font-family: var(--font-ui);
    font-size: 0.8rem;
    color: var(--platform-label);
    opacity: 0.75;
  }
</style>
