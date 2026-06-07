<script lang="ts">
  /**
   * First-run prompt (Phase 4). NOT a multi-step wizard and NOT a gate — a
   * single, skippable banner over the (already-live) board nudging the user to
   * pick their station. Dismiss with the × or Esc; picking a station applies
   * immediately and closes it. The "menu bar vs window" choice is a quiet
   * secondary link to Settings, not a step.
   */
  import StationSearch from '$lib/components/StationSearch.svelte';
  import { settingsForm, selectStation } from '$lib/stores/settingsForm.js';
  import { openSettings } from '$lib/stores/settingsView.js';
  import type { Station } from '$lib/ipc/types.js';

  // Opening Settings from the first-run prompt dismisses the prompt first, so
  // it doesn't linger behind the (overlaid) Settings panel.
  function goToSettings(): void {
    onDone();
    openSettings();
  }

  interface Props {
    /** Called when the user picks a station or dismisses — persists "onboarded". */
    onDone: () => void;
  }

  const { onDone }: Props = $props();

  function handleSelect(station: Station): void {
    selectStation(station);
    onDone();
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') onDone();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="firstrun" role="region" aria-label="Welcome to Tubbie">
  <button type="button" class="firstrun__skip" onclick={onDone} aria-label="Dismiss welcome">
    ×
  </button>
  <p class="firstrun__title">Welcome to Tubbie</p>
  <p class="firstrun__hint">Pick your station to get started — you can change it any time.</p>
  <StationSearch selectedId={$settingsForm.stationId} onSelect={handleSelect} />
  <p class="firstrun__secondary">
    Prefer it in the menu bar? Choose a display mode in
    <button type="button" class="firstrun__link" onclick={goToSettings}> Settings</button>.
  </p>
</div>

<style>
  .firstrun {
    position: relative;
    background: color-mix(in srgb, var(--fg) 8%, var(--bg));
    border-bottom: 1px solid var(--row-divider);
    padding: 0.75rem 1rem 0.9rem;
    font-family: var(--font-ui);
  }

  .firstrun__skip {
    position: absolute;
    top: 0.4rem;
    right: 0.5rem;
    background: none;
    border: none;
    color: var(--fg);
    opacity: 0.6;
    font-size: 1.2rem;
    line-height: 1;
    cursor: pointer;
    padding: 0.2rem 0.4rem;
  }
  .firstrun__skip:hover,
  .firstrun__skip:focus-visible {
    opacity: 1;
  }

  .firstrun__title {
    margin: 0 0 0.15rem;
    font-size: 1rem;
    font-weight: 600;
    color: var(--fg);
    letter-spacing: 0.02em;
  }

  .firstrun__hint {
    margin: 0 0 0.5rem;
    font-size: 0.85rem;
    color: var(--platform-label);
  }

  .firstrun__secondary {
    margin: 0.5rem 0 0;
    font-size: 0.75rem;
    color: var(--platform-label);
  }

  .firstrun__link {
    background: none;
    border: none;
    padding: 0;
    color: var(--fg);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }
</style>
