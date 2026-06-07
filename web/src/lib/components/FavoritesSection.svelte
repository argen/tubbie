<script lang="ts">
  import { onMount } from 'svelte';
  import { favorites, initFavorites, removeFavorite } from '$lib/stores/favorites.js';
  import { selectStation } from '$lib/stores/settingsForm.js';
  import { shortStationName } from '$lib/utils/format.js';
  import type { Favorite, Station } from '$lib/ipc/types.js';

  onMount(() => {
    // Load favorites once on mount. Errors surface via $favoritesError.
    void initFavorites();
  });

  /**
   * Selecting a favorite re-uses the existing station-select path so the
   * watch-channel publishes the new station_id and the stream refreshes
   * immediately (invariant #2). We construct a synthetic `Station` from the
   * favorite snapshot — the lines field gives us cold-cache-safe chips.
   */
  function handleSelectFavorite(fav: Favorite): void {
    const synthetic: Station = {
      id: fav.station_id,
      common_name: fav.common_name,
      modes: [],
      lat: 0,
      lon: 0,
      lines: fav.lines,
    };
    selectStation(synthetic);
  }

  async function handleRemoveFavorite(stationIdToRemove: string): Promise<void> {
    await removeFavorite(stationIdToRemove);
  }
</script>

<section class="settings__section" aria-labelledby="section-favorites">
  <h2 id="section-favorites" class="settings__section-title">Favorites</h2>
  {#if $favorites.length === 0}
    <p class="settings__api-hint" data-testid="favorites-empty">Star a station to save it here.</p>
  {:else}
    <ul class="favorites__list" data-testid="favorites-list">
      {#each $favorites as fav (fav.station_id)}
        <li class="favorites__row">
          <button
            type="button"
            class="favorites__row-body"
            onclick={() => {
              handleSelectFavorite(fav);
            }}
            aria-label={`Select ${fav.common_name}`}
            data-testid="favorite-row"
            data-station-id={fav.station_id}
          >
            <span class="favorites__row-name">{shortStationName(fav.common_name)}</span>
            <span class="favorites__row-chips" aria-hidden="true">
              {#each fav.lines as line (line.id)}
                <span class="settings__chip favorites__row-chip">{line.name}</span>
              {/each}
            </span>
          </button>
          <button
            type="button"
            class="favorites__trash"
            onclick={() => void handleRemoveFavorite(fav.station_id)}
            aria-label={`Remove ${fav.common_name} from favorites`}
            data-testid="favorite-trash"
            data-station-id={fav.station_id}
          >
            ✕
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  /* Favorites list — only used here. Shared rules (.settings__section,
     .settings__section-title, .settings__api-hint, .settings__chip) live
     as :global in SettingsView.svelte. */

  .favorites__list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .favorites__row {
    display: flex;
    align-items: stretch;
    gap: 0.4rem;
    border: 1px solid var(--input-border);
    border-radius: 2px;
    background: var(--chip-bg);
  }

  .favorites__row:hover,
  .favorites__row:focus-within {
    border-color: var(--platform-label);
  }

  .favorites__row-body {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--fg);
    text-align: left;
    cursor: pointer;
    padding: 0.5rem 0.6rem;
    font-family: var(--font-ui);
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    letter-spacing: 0.04em;
  }

  .favorites__row-name {
    font-size: 0.95rem;
    color: var(--fg);
  }

  .favorites__row-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }

  .favorites__row-chip {
    font-size: 0.75rem;
    padding: 0.1rem 0.4rem;
    opacity: 0.7;
    cursor: default;
  }

  .favorites__trash {
    background: transparent;
    border: none;
    border-left: 1px solid var(--input-border);
    color: var(--platform-label);
    font-size: 0.9rem;
    padding: 0 0.65rem;
    cursor: pointer;
    border-radius: 0 2px 2px 0;
  }

  .favorites__trash:hover,
  .favorites__trash:focus {
    color: var(--stale-accent);
  }
</style>
