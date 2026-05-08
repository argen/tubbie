<script lang="ts">
  import StationSearch from '$lib/components/StationSearch.svelte';
  import { board } from '$lib/stores/board.js';
  import { favorites, addFavorite, removeFavorite } from '$lib/stores/favorites.js';
  import { settingsForm, currentStationName, selectStation } from '$lib/stores/settingsForm.js';
  import type { Station } from '$lib/ipc/types.js';

  /** True iff the currently-selected station is in the favorites list. */
  const isCurrentStationFavorited = $derived(
    $favorites.some((f) => f.station_id === $settingsForm.stationId),
  );

  async function handleToggleFavorite(): Promise<void> {
    if (isCurrentStationFavorited) {
      await removeFavorite($settingsForm.stationId);
    } else {
      // Use whatever name + lines we know about right now. `currentStationName`
      // already falls back to the latest board's station_name when local
      // state is empty (e.g. user opens Settings on a fresh launch).
      const name =
        $settingsForm.stationName.length > 0
          ? $settingsForm.stationName
          : ($board?.platforms[0]?.arrivals[0]?.station_name ?? $settingsForm.stationId);
      await addFavorite($settingsForm.stationId, name, $settingsForm.stationLines);
    }
  }

  function handleStationSelect(station: Station): void {
    selectStation(station);
  }
</script>

<section class="settings__section" aria-labelledby="section-station">
  <h2 id="section-station" class="settings__section-title">Station</h2>
  {#if $currentStationName}
    <p class="settings__current-station" aria-live="polite" data-testid="settings-current-station">
      <span class="settings__current-station-label">Current:</span>
      <span class="settings__current-station-name">{$currentStationName}</span>
      <button
        type="button"
        class="settings__star"
        class:settings__star--active={isCurrentStationFavorited}
        onclick={() => void handleToggleFavorite()}
        aria-pressed={isCurrentStationFavorited}
        aria-label={isCurrentStationFavorited
          ? `Remove ${$currentStationName} from favorites`
          : `Save ${$currentStationName} as favorite`}
        data-testid="settings-star"
      >
        {isCurrentStationFavorited ? '★' : '☆'}
      </button>
    </p>
  {/if}
  <StationSearch selectedId={$settingsForm.stationId} onSelect={handleStationSelect} />
</section>

<style>
  /* Station-only rules — current-station header + favorite-toggle star.
     Shared rules (.settings__section, .settings__section-title) live as
     :global in routes/settings/+page.svelte. */

  .settings__current-station {
    font-family: var(--font-ui);
    font-size: 0.95rem;
    margin: 0 0 0.1rem;
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    letter-spacing: 0.04em;
  }

  .settings__current-station-label {
    color: var(--platform-label);
    opacity: 0.6;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }

  .settings__current-station-name {
    color: var(--accent);
    text-shadow:
      0 0 4px var(--accent),
      0 0 8px color-mix(in srgb, var(--accent) 40%, transparent);
  }

  /* Star toggle inline with the current-station label. */
  .settings__star {
    background: transparent;
    border: 1px solid var(--input-border);
    color: var(--platform-label);
    font-size: 1rem;
    line-height: 1;
    padding: 0.15rem 0.4rem;
    border-radius: 2px;
    cursor: pointer;
    margin-left: 0.4rem;
    letter-spacing: 0;
  }

  .settings__star:hover,
  .settings__star:focus {
    border-color: var(--accent);
    color: var(--accent);
  }

  .settings__star--active {
    color: var(--accent);
    border-color: var(--accent);
    text-shadow: 0 0 4px var(--accent);
  }
</style>
