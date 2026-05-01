<script lang="ts">
  import { onMount } from 'svelte';
  import type { LocationError, NearbyStation, Station } from '$lib/ipc/types.js';
  import {
    findNearestStations,
    requestCurrentLocation,
    searchStations,
  } from '$lib/ipc/commands.js';
  import { debounceAsync } from '$lib/utils/debounce.js';
  import { formatDistance, shortStationName } from '$lib/utils/format.js';

  interface Props {
    selectedId: string;
    onSelect: (station: Station) => void;
  }

  const { selectedId, onSelect }: Props = $props();

  // ---------------------------------------------------------------------
  // Modes
  //
  // Text-search and near-me share one listbox so the user has one mental
  // model. The mode tag drives WHICH list is rendered; entering near-me
  // clears any text-search state and vice versa.
  // ---------------------------------------------------------------------
  type NearMeMode =
    | { kind: 'idle' }
    | { kind: 'locating' }
    | { kind: 'results'; nearby: NearbyStation[] }
    | { kind: 'error'; error: LocationError };

  let query = $state('');
  let results = $state<Station[]>([]);
  let searching = $state(false);
  let searchError = $state<string | null>(null);
  let listboxOpen = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();
  let activeIdx = $state(-1);
  let searched = $state(false);
  let nearMe = $state<NearMeMode>({ kind: 'idle' });

  /** Safety net so a stuck IPC call never leaves the user staring at a spinner. */
  const SEARCH_TIMEOUT_MS = 12_000;
  /** How many nearby stations to ask for. Eight covers central London hubs
   *  comfortably without the listbox running the full popover height. */
  const NEAREST_LIMIT = 8;

  function searchWithTimeout(q: string): Promise<Station[]> {
    return new Promise<Station[]>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(
          new Error(
            `Search timed out after ${String(SEARCH_TIMEOUT_MS / 1000)}s — check your network and TfL API access.`,
          ),
        );
      }, SEARCH_TIMEOUT_MS);
      searchStations(q)
        .then((stations) => {
          clearTimeout(timer);
          resolve(stations);
        })
        .catch((err: unknown) => {
          clearTimeout(timer);
          reject(err instanceof Error ? err : new Error(String(err)));
        });
    });
  }

  const debouncedSearch = debounceAsync(
    async (q: string): Promise<Station[]> => {
      if (q.trim().length === 0) return [];
      return searchWithTimeout(q);
    },
    200,
    (res: Station[]) => {
      results = res;
      searching = false;
      searchError = null;
      activeIdx = -1;
      searched = query.trim().length > 0;
      listboxOpen = searched && results.length > 0;
    },
    (err: unknown) => {
      searchError = err instanceof Error ? err.message : String(err);
      searching = false;
      searched = true;
    },
  );

  function handleInput(e: Event): void {
    query = (e.target as HTMLInputElement).value;
    // Typing leaves near-me mode entirely — text search and listbox can't
    // both occupy the same dropdown without confusing keyboard navigation.
    nearMe = { kind: 'idle' };
    if (query.trim().length === 0) {
      results = [];
      listboxOpen = false;
      searching = false;
      searched = false;
      debouncedSearch('');
      return;
    }
    searched = false;
    searching = true;
    debouncedSearch(query);
  }

  function selectStation(station: Station): void {
    query = station.common_name;
    listboxOpen = false;
    results = [];
    searching = false;
    searchError = null;
    searched = false;
    nearMe = { kind: 'idle' };
    onSelect(station);
  }

  // ---------------------------------------------------------------------
  // Near me
  // ---------------------------------------------------------------------

  async function startNearMe(): Promise<void> {
    // Don't fire while already locating; the global Mutex on the Rust side
    // would serialise but we'd flicker the row state pointlessly.
    if (nearMe.kind === 'locating') return;
    nearMe = { kind: 'locating' };
    listboxOpen = true;
    activeIdx = -1;
    // Clear text-search state so the listbox renders the near-me list,
    // not stale matches from the previous query.
    results = [];
    searched = false;
    searching = false;
    searchError = null;

    const fixResult = await requestCurrentLocation();
    if (!fixResult.ok) {
      nearMe = { kind: 'error', error: fixResult.error };
      return;
    }

    try {
      const nearby = await findNearestStations(
        fixResult.fix.lat,
        fixResult.fix.lon,
        NEAREST_LIMIT,
      );
      // Empty list means we're outside the 25 km radius (Paris query, …) —
      // surface that as a typed error row so the user knows to fall back
      // to text search rather than retry.
      if (nearby.length === 0) {
        nearMe = {
          kind: 'error',
          error: { kind: 'Internal', message: 'no stations within 25 km' },
        };
        return;
      }
      nearMe = { kind: 'results', nearby };
      activeIdx = 0;
    } catch (e: unknown) {
      nearMe = {
        kind: 'error',
        error: {
          kind: 'Internal',
          message: e instanceof Error ? e.message : String(e),
        },
      };
    }
  }

  /** Map a `LocationError` to the listbox row label. The Rust side decides
   *  *which* error happened; this map decides how it reads. */
  function errorRowLabel(error: LocationError): string {
    switch (error.kind) {
      case 'PermissionDenied':
        return 'LOCATION OFF — TAP TO SEARCH';
      case 'PermissionRestricted':
        return 'ENABLE IN SETTINGS';
      case 'ServicesDisabled':
        return 'LOCATION SERVICES OFF';
      case 'Timeout':
      case 'LowAccuracy':
        return 'NO SIGNAL — TRY AGAIN';
      case 'AppBackground':
        return 'TAP AGAIN ONCE FOREGROUNDED';
      case 'Internal':
        return error.message.includes('25 km')
          ? 'OUTSIDE NETWORK — TRY SEARCH'
          : 'NO SIGNAL — TRY AGAIN';
    }
  }

  /** What clicking the error row should do. PermissionDenied focuses the
   *  text input (the alternative the user has). The retry-prone errors
   *  re-fire the location request. The terminal ones (Restricted, Disabled)
   *  do nothing — the user has to take action in System Settings. */
  function errorRowAction(error: LocationError): 'focus-search' | 'retry' | 'noop' {
    switch (error.kind) {
      case 'PermissionDenied':
        return 'focus-search';
      case 'Timeout':
      case 'LowAccuracy':
      case 'AppBackground':
      case 'Internal':
        return 'retry';
      case 'PermissionRestricted':
      case 'ServicesDisabled':
        return 'noop';
    }
  }

  function handleErrorRowClick(error: LocationError): void {
    const action = errorRowAction(error);
    if (action === 'focus-search') {
      nearMe = { kind: 'idle' };
      listboxOpen = false;
      inputEl?.focus();
    } else if (action === 'retry') {
      void startNearMe();
    }
    // 'noop': leave the row in place; user will go to Settings.
  }

  // Cmd+L (macOS) / Ctrl+L (Linux/Windows) — focus the search and trigger
  // near-me. The shortcut is window-scoped and only fires when the
  // component is mounted (no leaks across pages).
  function handleGlobalKeydown(e: KeyboardEvent): void {
    if ((e.metaKey || e.ctrlKey) && (e.key === 'l' || e.key === 'L')) {
      e.preventDefault();
      inputEl?.focus();
      void startNearMe();
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleGlobalKeydown);
    return () => {
      window.removeEventListener('keydown', handleGlobalKeydown);
    };
  });

  // ---------------------------------------------------------------------
  // Keyboard nav
  // ---------------------------------------------------------------------

  function handleKeydown(e: KeyboardEvent): void {
    if (!listboxOpen) return;

    if (nearMe.kind === 'results') {
      const len = nearMe.nearby.length;
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        activeIdx = Math.min(activeIdx + 1, len - 1);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        activeIdx = Math.max(activeIdx - 1, 0);
      } else if (e.key === 'Enter' && activeIdx >= 0) {
        e.preventDefault();
        const item = nearMe.nearby[activeIdx];
        if (item) selectStation(item.station);
      } else if (e.key === 'Escape') {
        listboxOpen = false;
        activeIdx = -1;
      }
      return;
    }

    // Text-search mode keyboard nav.
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      activeIdx = Math.min(activeIdx + 1, results.length - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      activeIdx = Math.max(activeIdx - 1, 0);
    } else if (e.key === 'Enter' && activeIdx >= 0) {
      e.preventDefault();
      const station = results[activeIdx];
      if (station) selectStation(station);
    } else if (e.key === 'Escape') {
      listboxOpen = false;
      activeIdx = -1;
    }
  }

  function handleBlur(): void {
    setTimeout(() => {
      // Don't close while locating — the user might be staring at the
      // ACQUIRING FIX row and the system permission prompt blurred us.
      if (nearMe.kind === 'locating') return;
      listboxOpen = false;
    }, 150);
  }

  // Locale used to pick the distance unit. Read once at component init —
  // a runtime locale change is rare enough that we don't bother
  // subscribing to it. Falls back to "en-GB" because TfL stations are
  // in London and the dot-matrix board reads in miles.
  const distanceLocale = (() => {
    if (typeof navigator !== 'undefined' && typeof navigator.language === 'string') {
      return navigator.language;
    }
    return 'en-GB';
  })();

  // Pre-compute aria announcement string for results so screen readers
  // get a stable summary instead of N row-update messages.
  const nearbyAnnouncement = $derived.by(() => {
    if (nearMe.kind !== 'results') return '';
    const closest = nearMe.nearby[0];
    if (!closest) return '';
    const distLabel = formatDistance(closest.distance_m, distanceLocale);
    return `${String(nearMe.nearby.length)} stations found, ${shortStationName(closest.station.common_name)} nearest, ${distLabel}.`;
  });
</script>

<div class="station-search" role="search" aria-label="Station search">
  <div class="station-search__input-wrap">
    <input
      type="search"
      bind:this={inputEl}
      class="station-search__input"
      placeholder="Search stations…"
      autocomplete="off"
      autocorrect="off"
      autocapitalize="off"
      spellcheck="false"
      value={query}
      oninput={handleInput}
      onkeydown={handleKeydown}
      onblur={handleBlur}
      aria-label="Search for a tube station"
      aria-autocomplete="list"
      aria-controls="station-listbox"
      aria-expanded={listboxOpen}
      aria-activedescendant={activeIdx >= 0 ? `station-option-${String(activeIdx)}` : undefined}
      role="combobox"
    />
    {#if searching || nearMe.kind === 'locating'}
      <span class="station-search__spinner" aria-hidden="true">⠿</span>
    {/if}
    <button
      type="button"
      class="station-search__crosshair"
      class:station-search__crosshair--active={nearMe.kind !== 'idle'}
      aria-label="Find nearest stations"
      title="Find nearest stations (⌘L)"
      onmousedown={(e) => {
        // mousedown rather than click so the input's onblur doesn't fire
        // first and close the listbox before we get to render results.
        e.preventDefault();
        void startNearMe();
      }}
    >
      ⊕
    </button>
  </div>

  {#if searchError}
    <p class="station-search__error" role="alert">{searchError}</p>
  {/if}

  <span class="visually-hidden" role="status" aria-live="polite">
    {#if nearMe.kind === 'locating'}
      Finding nearest stations
    {:else if nearMe.kind === 'results'}
      {nearbyAnnouncement}
    {/if}
  </span>

  {#if listboxOpen && nearMe.kind === 'locating'}
    <ul
      id="station-listbox"
      class="station-search__results"
      role="listbox"
      aria-label="Locating"
    >
      <li class="station-search__result station-search__result--status" data-testid="station-search-locating">
        ACQUIRING FIX<span class="station-search__dots" aria-hidden="true">···</span>
      </li>
    </ul>
  {:else if listboxOpen && nearMe.kind === 'error'}
    <ul
      id="station-listbox"
      class="station-search__results"
      role="listbox"
      aria-label="Location error"
    >
      <li
        class="station-search__result station-search__result--error"
        role="option"
        aria-selected="false"
        data-testid="station-search-location-error"
        data-error-kind={nearMe.error.kind}
        onmousedown={() => {
          handleErrorRowClick(nearMe.kind === 'error' ? nearMe.error : { kind: 'Internal', message: '' });
        }}
      >
        {errorRowLabel(nearMe.error)}
      </li>
    </ul>
  {:else if listboxOpen && nearMe.kind === 'results'}
    <ul
      id="station-listbox"
      class="station-search__results"
      role="listbox"
      aria-label="Nearest stations"
    >
      {#each nearMe.nearby as item, idx (item.station.id)}
        <li
          id="station-option-{idx}"
          class="station-search__result station-search__result--nearby"
          class:station-search__result--active={idx === activeIdx}
          class:station-search__result--selected={item.station.id === selectedId}
          role="option"
          aria-selected={item.station.id === selectedId}
          aria-label="{shortStationName(item.station.common_name)}, {formatDistance(item.distance_m, distanceLocale)}"
          onmousedown={() => {
            selectStation(item.station);
          }}
        >
          <span class="station-search__result-name">{shortStationName(item.station.common_name)}</span>
          <span class="station-search__result-distance" aria-hidden="true">
            {formatDistance(item.distance_m, distanceLocale)}
          </span>
        </li>
      {/each}
    </ul>
  {:else if listboxOpen && results.length > 0}
    <ul
      id="station-listbox"
      class="station-search__results"
      role="listbox"
      aria-label="Station search results"
    >
      {#each results as station, idx (station.id)}
        <li
          id="station-option-{idx}"
          class="station-search__result"
          class:station-search__result--active={idx === activeIdx}
          class:station-search__result--selected={station.id === selectedId}
          role="option"
          aria-selected={station.id === selectedId}
          onmousedown={() => {
            selectStation(station);
          }}
        >
          <span class="station-search__result-name">{station.common_name}</span>
          {#if station.lines.length > 0}
            <span
              class="station-search__result-lines"
              aria-label="Lines: {station.lines.map((l) => l.name).join(', ')}"
            >
              {station.lines.map((l) => l.id).join(', ')}
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  {:else if searched && !searching && query.trim().length > 0 && results.length === 0 && !searchError}
    <p
      class="station-search__empty"
      role="status"
      aria-live="polite"
      data-testid="station-search-empty"
    >
      No tube stations match “{query.trim()}”.
    </p>
  {/if}
</div>

<style>
  .station-search {
    position: relative;
    width: 100%;
  }

  .station-search__input-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .station-search__input {
    width: 100%;
    background: var(--input-bg);
    border: 1px solid var(--input-border);
    color: var(--fg);
    font-family: var(--font-board);
    font-size: 1.1rem;
    padding: 0.5rem 0.75rem;
    /* Make room for the spinner and the crosshair button on the right. */
    padding-right: 3.25rem;
    border-radius: 2px;
    outline: none;
    letter-spacing: 0.04em;
  }

  .station-search__input:focus {
    border-color: var(--fg);
    box-shadow: 0 0 0 2px var(--focus-ring);
  }

  .station-search__input::placeholder {
    color: var(--platform-label);
    opacity: 0.5;
  }

  .station-search__input::-webkit-search-cancel-button {
    -webkit-appearance: none;
  }

  .station-search__spinner {
    position: absolute;
    right: 2rem;
    color: var(--platform-label);
    animation: spin 1s linear infinite;
    opacity: 0.7;
    font-size: 1rem;
  }

  /* Spotlight-style in-input affordance: doesn't shift layout, doesn't
     create a second focus stop on the page, but still hit-testable. */
  .station-search__crosshair {
    position: absolute;
    right: 0.4rem;
    top: 50%;
    transform: translateY(-50%);
    background: transparent;
    border: 0;
    color: var(--platform-label);
    font-family: var(--font-board);
    font-size: 1.1rem;
    line-height: 1;
    cursor: pointer;
    padding: 0.2rem 0.35rem;
    opacity: 0.7;
    transition: opacity 0.12s ease;
  }

  .station-search__crosshair:hover,
  .station-search__crosshair:focus-visible {
    opacity: 1;
    color: var(--accent);
    outline: none;
  }

  .station-search__crosshair--active {
    color: var(--accent);
    opacity: 1;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .station-search__spinner {
      animation: none;
    }
  }

  .station-search__error {
    color: var(--stale-accent);
    font-family: var(--font-board);
    font-size: 0.9rem;
    margin: 0.3rem 0 0;
  }

  .station-search__empty {
    font-family: var(--font-board);
    font-size: 0.9rem;
    color: var(--platform-label);
    opacity: 0.75;
    margin: 0.3rem 0 0;
  }

  .station-search__results {
    position: absolute;
    top: calc(100% + 2px);
    left: 0;
    right: 0;
    background: var(--settings-bg);
    border: 1px solid var(--input-border);
    border-top: none;
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 240px;
    overflow-y: auto;
    z-index: 100;
    border-radius: 0 0 2px 2px;
  }

  .station-search__result {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.5rem 0.75rem;
    cursor: pointer;
    border-bottom: 1px solid var(--row-divider);
    font-family: var(--font-board);
  }

  .station-search__result:last-child {
    border-bottom: none;
  }

  .station-search__result:hover,
  .station-search__result--active {
    background: var(--chip-bg);
    color: var(--accent);
  }

  .station-search__result--selected {
    color: var(--accent);
  }

  .station-search__result--status,
  .station-search__result--error {
    color: var(--platform-label);
    cursor: default;
    font-size: 0.95rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .station-search__result--error {
    cursor: pointer;
  }

  .station-search__dots {
    margin-left: 0.25rem;
    animation: dots 1.2s steps(4, end) infinite;
  }

  @keyframes dots {
    0%, 20% {
      content: '';
    }
    40% {
      content: '·';
    }
    60% {
      content: '··';
    }
    80%, 100% {
      content: '···';
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .station-search__dots {
      animation: none;
    }
  }

  .station-search__result-name {
    font-size: 1.05rem;
    color: inherit;
  }

  .station-search__result-lines {
    font-size: 0.75rem;
    color: var(--platform-label);
    opacity: 0.7;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  /* Right-aligned dimmed-amber distance chip. The colour is `var(--accent)`
     at reduced opacity rather than a new variable — staying in the same hue
     keeps the dot-matrix aesthetic. */
  .station-search__result-distance {
    font-size: 0.85rem;
    color: var(--accent);
    opacity: 0.55;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-variant-numeric: tabular-nums;
  }

  .station-search__result--nearby .station-search__result-name {
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>
