<script lang="ts">
  import { settingsForm, updateForm, persistDebounced } from '$lib/stores/settingsForm.js';
  import type { Direction } from '$lib/ipc/types.js';

  const DIRECTIONS: { id: Direction; label: string }[] = [
    { id: 'Northbound', label: 'Northbound' },
    { id: 'Southbound', label: 'Southbound' },
    { id: 'Eastbound', label: 'Eastbound' },
    { id: 'Westbound', label: 'Westbound' },
    { id: 'Inbound', label: 'Inbound' },
    { id: 'Outbound', label: 'Outbound' },
  ];

  function toggleDirection(dir: Direction): void {
    const current = $settingsForm.selectedDirections;
    const next = current.includes(dir) ? current.filter((d) => d !== dir) : [...current, dir];
    updateForm({ selectedDirections: next });
    persistDebounced();
  }
</script>

<section class="settings__section" aria-labelledby="section-directions">
  <h2 id="section-directions" class="settings__section-title">
    Directions
    <span class="settings__section-hint">(empty = all directions)</span>
  </h2>
  <div class="settings__chips" role="group" aria-label="Select directions to filter">
    {#each DIRECTIONS as dir (dir.id)}
      <button
        type="button"
        class="settings__chip"
        class:settings__chip--selected={$settingsForm.selectedDirections.includes(dir.id)}
        onclick={() => {
          toggleDirection(dir.id);
        }}
        aria-pressed={$settingsForm.selectedDirections.includes(dir.id)}
        aria-label="Toggle {dir.label} direction"
      >
        {dir.label}
      </button>
    {/each}
  </div>
</section>
