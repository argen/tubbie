<script lang="ts">
  import { onMount } from 'svelte';
  import { displayPrefs, initDisplayPrefs, updateDisplayPrefs } from '$lib/stores/displayPrefs.js';

  onMount(() => {
    // Hydrate display prefs from disk (defaults to all-false on first run).
    void initDisplayPrefs();
  });

  function handleToggleGroupDestinations(): void {
    void updateDisplayPrefs({
      ...$displayPrefs,
      group_destinations: !$displayPrefs.group_destinations,
    });
  }
</script>

<section class="settings__section" aria-labelledby="section-display-prefs">
  <h2 id="section-display-prefs" class="settings__section-title">Display preferences</h2>
  <label class="settings__toggle">
    <input
      type="checkbox"
      checked={$displayPrefs.group_destinations}
      onchange={handleToggleGroupDestinations}
      data-testid="settings-group-destinations"
    />
    <span class="settings__toggle-label">Group same destination</span>
    <span class="settings__toggle-hint">
      Combine repeat trains to the same place into one row (e.g. "Edgware · 2, 5, 9 min").
    </span>
  </label>
</section>

<style>
  /* Generic toggle row. Currently only used here (display-prefs); if a
     future renderer-only flag wants the same treatment, add it as a
     sibling section in this component or factor the toggle out into
     its own primitive at that point. */
  .settings__toggle {
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

  .settings__toggle:hover,
  .settings__toggle:focus-within {
    border-color: var(--platform-label);
  }

  .settings__toggle input[type='checkbox'] {
    grid-row: 1 / span 2;
    accent-color: var(--fg);
    margin: 0;
  }

  .settings__toggle-label {
    font-family: var(--font-ui);
    font-size: 1rem;
    color: var(--fg);
  }

  .settings__toggle-hint {
    font-family: var(--font-ui);
    font-size: 0.8rem;
    color: var(--platform-label);
    opacity: 0.75;
  }
</style>
