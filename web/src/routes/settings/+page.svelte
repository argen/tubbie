<script lang="ts">
  import { goto } from '$app/navigation';
  import { configError } from '$lib/stores/config.js';
  import StationSearch from '$lib/components/StationSearch.svelte';
  import ApiKeySection from '$lib/components/ApiKeySection.svelte';
  import DisplayModeSection from '$lib/components/DisplayModeSection.svelte';
  import DisplayPrefsSection from '$lib/components/DisplayPrefsSection.svelte';
  import ThemeSection from '$lib/components/ThemeSection.svelte';
  import PollIntervalSection from '$lib/components/PollIntervalSection.svelte';
  import DirectionsSection from '$lib/components/DirectionsSection.svelte';
  import { board } from '$lib/stores/board.js';
  import { favorites, initFavorites, addFavorite, removeFavorite } from '$lib/stores/favorites.js';
  import {
    settingsForm,
    saveState,
    persist,
    persistDebounced,
    flushPending,
    cancelSaveStateTimer,
    resyncFormFromConfig,
    updateForm,
  } from '$lib/stores/settingsForm.js';
  import { shortStationName } from '$lib/utils/format.js';
  import type { Favorite, Station } from '$lib/ipc/types.js';
  import { onDestroy, onMount } from 'svelte';

  // Form state lives in `$lib/stores/settingsForm` so the section components
  // (Api / DisplayMode / DisplayPrefs already extracted; Station / Favorites
  // / Lines / Directions / Poll / Theme to follow) can read + write without
  // prop drilling. This page reads via `$settingsForm.x` and mutates via
  // `updateForm({ x })` then `persist()` or `persistDebounced()`.

  // Master roster of selectable line chips. Tube + DLR + Elizabeth +
  // the six named Overground lines (Mildmay/Lioness/Suffragette/Windrush/
  // Weaver/Liberty — TfL split the Overground in November 2024). The
  // visible/disabled subset for any station is intersected with that
  // station's `Station.lines` field in `handleStationSelect`.
  //
  // Elizabeth uses the line-form id `'elizabeth'` (matches
  // `Station.lines[].id` and the wire format after
  // `tfl_domain::canonicalize_line_id` runs at deserialization). The
  // mode-form `'elizabeth-line'` is migrated on config load so any
  // historical config keeps working.
  const KNOWN_LINES: { id: string; label: string }[] = [
    { id: 'bakerloo', label: 'Bakerloo' },
    { id: 'central', label: 'Central' },
    { id: 'circle', label: 'Circle' },
    { id: 'district', label: 'District' },
    { id: 'elizabeth', label: 'Elizabeth' },
    { id: 'hammersmith-city', label: 'Hammersmith & City' },
    { id: 'jubilee', label: 'Jubilee' },
    { id: 'metropolitan', label: 'Metropolitan' },
    { id: 'northern', label: 'Northern' },
    { id: 'piccadilly', label: 'Piccadilly' },
    { id: 'victoria', label: 'Victoria' },
    { id: 'waterloo-city', label: 'Waterloo & City' },
    { id: 'dlr', label: 'DLR' },
    { id: 'liberty', label: 'Liberty' },
    { id: 'lioness', label: 'Lioness' },
    { id: 'mildmay', label: 'Mildmay' },
    { id: 'suffragette', label: 'Suffragette' },
    { id: 'weaver', label: 'Weaver' },
    { id: 'windrush', label: 'Windrush' },
  ];

  onMount(() => {
    // Re-sync form to the latest $config — otherwise stale form state
    // survives across SPA navigations (the module-scoped store outlives
    // the page component).
    resyncFormFromConfig();
    // Load favorites once on mount. Errors surface via $favoritesError.
    void initFavorites();
  });

  function handleStationSelect(station: Station): void {
    // Prune line_ids to those the new station actually serves so we never
    // persist a filter the station can't honour.
    const prunedLineIds =
      station.lines.length > 0
        ? $settingsForm.lineIds.filter((id) => new Set(station.lines.map((l) => l.id)).has(id))
        : $settingsForm.lineIds;
    updateForm({
      stationId: station.id,
      stationName: station.common_name,
      stationLines: station.lines,
      lineIds: prunedLineIds,
    });
    void persist();
  }

  // ---------------------------------------------------------------------------
  // Favorites
  // ---------------------------------------------------------------------------

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
    handleStationSelect(synthetic);
  }

  async function handleRemoveFavorite(stationIdToRemove: string): Promise<void> {
    await removeFavorite(stationIdToRemove);
  }

  /**
   * Human-readable name of the station currently saved in config.
   *
   * Precedence:
   *  1. Local `stationName` — set by `handleStationSelect` so a freshly-picked
   *     station shows its name before the board stream catches up.
   *  2. `station_name` from the latest board arrival — survives settings
   *     re-entry when the user already has an active board.
   *
   * `shortStationName` strips the " Underground Station" suffix so the label
   * matches what the board header shows. Empty when neither source is
   * populated (e.g. brand-new install, no arrivals yet).
   */
  const currentStationName = $derived.by(() => {
    if ($settingsForm.stationName.length > 0) return shortStationName($settingsForm.stationName);
    const fromBoard = $board?.platforms[0]?.arrivals[0]?.station_name ?? '';
    return fromBoard.length > 0 ? shortStationName(fromBoard) : '';
  });

  /**
   * Lines the selected station actually serves, or `null` when unknown
   * (first mount, or stations whose metadata is empty). `null` fails open:
   * every chip is interactive. A non-null set disables everything outside it.
   */
  const availableLineIds = $derived<Set<string> | null>(
    $settingsForm.stationLines.length > 0
      ? new Set($settingsForm.stationLines.map((l) => l.id))
      : null,
  );

  function isLineAvailable(lineId: string): boolean {
    return availableLineIds === null || availableLineIds.has(lineId);
  }

  function toggleLine(lineId: string): void {
    if (!isLineAvailable(lineId)) return;
    const current = $settingsForm.lineIds;
    const next = current.includes(lineId)
      ? current.filter((id) => id !== lineId)
      : [...current, lineId];
    updateForm({ lineIds: next });
    // Debounce: a 12-chip toggle burst becomes one save_config carrying
    // the final state, instead of 12 disk writes + 12 cfg_tx.send round
    // trips. The flushPending hook in onDestroy / beforeunload makes
    // sure a click made just before closing Settings still saves.
    persistDebounced();
  }

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

  async function handleBack(): Promise<void> {
    await goto('/');
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
    <!-- Station search -->
    <section class="settings__section" aria-labelledby="section-station">
      <h2 id="section-station" class="settings__section-title">Station</h2>
      {#if currentStationName}
        <p
          class="settings__current-station"
          aria-live="polite"
          data-testid="settings-current-station"
        >
          <span class="settings__current-station-label">Current:</span>
          <span class="settings__current-station-name">{currentStationName}</span>
          <button
            type="button"
            class="settings__star"
            class:settings__star--active={isCurrentStationFavorited}
            onclick={() => void handleToggleFavorite()}
            aria-pressed={isCurrentStationFavorited}
            aria-label={isCurrentStationFavorited
              ? `Remove ${currentStationName} from favorites`
              : `Save ${currentStationName} as favorite`}
            data-testid="settings-star"
          >
            {isCurrentStationFavorited ? '★' : '☆'}
          </button>
        </p>
      {/if}
      <StationSearch selectedId={$settingsForm.stationId} onSelect={handleStationSelect} />
    </section>

    <!-- Favorites -->
    <section class="settings__section" aria-labelledby="section-favorites">
      <h2 id="section-favorites" class="settings__section-title">Favorites</h2>
      {#if $favorites.length === 0}
        <p class="settings__api-hint" data-testid="favorites-empty">
          Star a station to save it here.
        </p>
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

    <!-- Line filter -->
    <section class="settings__section" aria-labelledby="section-lines">
      <h2 id="section-lines" class="settings__section-title">
        Lines
        <span class="settings__section-hint">(empty = all lines)</span>
      </h2>
      <div class="settings__chips" role="group" aria-label="Select lines to filter">
        {#each KNOWN_LINES as line (line.id)}
          {@const available = isLineAvailable(line.id)}
          <button
            type="button"
            class="settings__chip"
            class:settings__chip--selected={$settingsForm.lineIds.includes(line.id)}
            class:settings__chip--unavailable={!available}
            disabled={!available}
            onclick={() => {
              toggleLine(line.id);
            }}
            aria-pressed={$settingsForm.lineIds.includes(line.id)}
            aria-disabled={!available}
            aria-label={available
              ? `Toggle ${line.label} line`
              : `${line.label} line is not served by this station`}
            title={available ? undefined : `Not served by ${currentStationName || 'this station'}`}
          >
            {line.label}
          </button>
        {/each}
      </div>
    </section>

    <DirectionsSection />

    <PollIntervalSection />

    <ThemeSection />

    <DisplayModeSection />

    <DisplayPrefsSection />

    <ApiKeySection />
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

  /* Favorites list */
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
