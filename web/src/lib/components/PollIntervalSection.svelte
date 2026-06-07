<script lang="ts">
  import { settingsForm, updateForm, persistDebounced } from '$lib/stores/settingsForm.js';

  function handlePollSlider(event: Event): void {
    const value = +(event.currentTarget as HTMLInputElement).value;
    updateForm({ pollSeconds: value });
    // Fires on every slider tick; the debounced persist coalesces the drag
    // so we don't slam save_config → stream-restart 295 times on a full sweep.
    persistDebounced();
  }
</script>

<section class="settings__section" aria-labelledby="section-poll">
  <h2 id="section-poll" class="settings__section-title">Poll Interval</h2>
  <div class="settings__range-wrap">
    <label for="poll-slider" class="settings__range-label">
      Every {$settingsForm.pollSeconds}s
    </label>
    <input
      id="poll-slider"
      type="range"
      min="10"
      max="300"
      step="5"
      value={$settingsForm.pollSeconds}
      oninput={handlePollSlider}
      class="settings__range"
      aria-label="Poll interval in seconds: {$settingsForm.pollSeconds}"
      aria-valuemin={10}
      aria-valuemax={300}
      aria-valuenow={$settingsForm.pollSeconds}
    />
    <div class="settings__range-bounds" aria-hidden="true">
      <span>10s</span>
      <span>300s</span>
    </div>
  </div>
</section>

<style>
  /* Range slider — only used in the poll-interval section. Shared rules
     (.settings__section, .settings__section-title) live as :global in
     SettingsView.svelte. */

  .settings__range-wrap {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .settings__range-label {
    font-family: var(--font-ui);
    font-size: 1rem;
    color: var(--fg);
    letter-spacing: 0.05em;
  }

  .settings__range {
    width: 100%;
    accent-color: var(--fg);
    cursor: pointer;
  }

  .settings__range-bounds {
    display: flex;
    justify-content: space-between;
    font-family: var(--font-ui);
    font-size: 0.75rem;
    color: var(--platform-label);
    opacity: 0.5;
  }
</style>
