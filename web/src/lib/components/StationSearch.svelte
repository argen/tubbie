<script lang="ts">
  import type { Station } from '$lib/ipc/types.js';
  import { searchStations } from '$lib/ipc/commands.js';
  import { debounceAsync } from '$lib/utils/debounce.js';

  interface Props {
    selectedId: string;
    onSelect: (station: Station) => void;
  }

  const { selectedId, onSelect }: Props = $props();

  let query = $state('');
  let results = $state<Station[]>([]);
  let searching = $state(false);
  let searchError = $state<string | null>(null);
  let listboxOpen = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();
  let activeIdx = $state(-1);
  // Flipped true after the first completed search for the current query; lets
  // us tell "still typing / still loading" apart from "search ran, no results".
  let searched = $state(false);

  /** Safety net so a stuck IPC call never leaves the user staring at a spinner. */
  const SEARCH_TIMEOUT_MS = 12_000;

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

  // Debounced search — 200 ms, latest-wins. `handleInput` bumps the debounce
  // generation on every keystroke (including clears), so stale resolutions
  // from the previous query are discarded by `debounceAsync`.
  const debouncedSearch = debounceAsync(
    async (q: string): Promise<Station[]> => {
      // Skip the backend round-trip for an empty query; `onResult` still
      // fires so we can re-settle local UI state.
      if (q.trim().length === 0) return [];
      return searchWithTimeout(q);
    },
    200,
    (res: Station[]) => {
      results = res;
      searching = false;
      searchError = null;
      activeIdx = -1;
      // If the user cleared the input, treat this as "not searched"; the
      // listbox stays closed and the empty-state hint stays hidden.
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
    if (query.trim().length === 0) {
      results = [];
      listboxOpen = false;
      searching = false;
      searched = false;
      // Bump the debounce generation so any in-flight call discards its result.
      debouncedSearch('');
      return;
    }
    // Reset "no results" state until the new search resolves, and turn the
    // spinner on immediately so the user sees the 200 ms debounce window as
    // "still searching" rather than "broken".
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
    // Without this the empty-state branch would fire immediately: `searched`
    // would stay true from the preceding search, `query` would still be the
    // selected station name, `results` would have just been cleared — so the
    // UI would render "No tube stations match 'Victoria'" right under a
    // station the user just picked.
    searched = false;
    onSelect(station);
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (!listboxOpen) return;

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
    // Close listbox slightly delayed to allow click to register
    setTimeout(() => {
      listboxOpen = false;
    }, 150);
  }
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
    {#if searching}
      <span class="station-search__spinner" aria-hidden="true">⠿</span>
    {/if}
  </div>

  {#if searchError}
    <p class="station-search__error" role="alert">{searchError}</p>
  {/if}

  {#if listboxOpen && results.length > 0}
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
    padding-right: 2rem;
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

  /* Remove default search input clear button */
  .station-search__input::-webkit-search-cancel-button {
    -webkit-appearance: none;
  }

  .station-search__spinner {
    position: absolute;
    right: 0.5rem;
    color: var(--platform-label);
    animation: spin 1s linear infinite;
    opacity: 0.7;
    font-size: 1rem;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
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
</style>
