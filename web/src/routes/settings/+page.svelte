<script lang="ts">
  import { goto } from '$app/navigation';
  import {
    config,
    configError,
    updateConfig,
    applyTheme,
    type ThemeId,
  } from '$lib/stores/config.js';
  import StationSearch from '$lib/components/StationSearch.svelte';
  import ThemePicker from '$lib/components/ThemePicker.svelte';
  import ApiKeySection from '$lib/components/ApiKeySection.svelte';
  import { saveDisplayMode, type DisplayMode } from '$lib/ipc/commands.js';
  import { displayMode } from '$lib/stores/displayMode.js';
  import { displayPrefs, initDisplayPrefs, updateDisplayPrefs } from '$lib/stores/displayPrefs.js';
  import { board } from '$lib/stores/board.js';
  import { favorites, initFavorites, addFavorite, removeFavorite } from '$lib/stores/favorites.js';
  import { debounce } from '$lib/utils/debounce.js';
  import { shortStationName } from '$lib/utils/format.js';
  import type { Direction, Favorite, LineRef, Station } from '$lib/ipc/types.js';
  import { onDestroy, onMount } from 'svelte';

  // ---------------------------------------------------------------------------
  // Local form state (mirrors config; auto-persisted on change)
  // ---------------------------------------------------------------------------

  let stationId = $state($config.station_id);
  let stationName = $state('');
  /**
   * Lines served by the currently-selected station, populated from
   * `StationSearch.onSelect`. Empty on first mount (we don't know which
   * lines the saved station serves without refetching) — the UI then falls
   * back to the global KNOWN_LINES list.
   */
  let stationLines = $state<LineRef[]>([]);
  let lineIds = $state<string[]>([...$config.line_ids]);
  let selectedDirections = $state<Direction[]>([...$config.directions]);
  let pollSeconds = $state($config.poll_seconds);
  let theme = $state<string>($config.theme);

  // Display-mode picker state. The Rust side now applies the swap live,
  // so we no longer mirror into a separate `pendingDisplayMode` — the
  // `$displayMode` store updates as soon as `save_display_mode` returns.
  let displayModeStatus = $state<string | null>(null);
  let displayModeStatusTimer: ReturnType<typeof setTimeout> | null = null;
  /** Transient "saved X seconds ago" chip next to the header. */
  let saveState = $state<'idle' | 'saving' | 'saved'>('idle');
  let saveStateTimer: ReturnType<typeof setTimeout> | null = null;

  const DIRECTIONS: { id: Direction; label: string }[] = [
    { id: 'Northbound', label: 'Northbound' },
    { id: 'Southbound', label: 'Southbound' },
    { id: 'Eastbound', label: 'Eastbound' },
    { id: 'Westbound', label: 'Westbound' },
    { id: 'Inbound', label: 'Inbound' },
    { id: 'Outbound', label: 'Outbound' },
  ];

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
    // Load favorites once on mount. Errors surface via $favoritesError.
    void initFavorites();
    // Hydrate display prefs from disk (defaults to all-false on first run).
    void initDisplayPrefs();
  });

  function handleToggleGroupDestinations(): void {
    void updateDisplayPrefs({
      ...$displayPrefs,
      group_destinations: !$displayPrefs.group_destinations,
    });
  }

  /**
   * Persist the current form state. `updateConfig` catches its own errors
   * and drives `$configError`, so callers never need to try/catch.
   *
   * The backend's `save_config` publishes the new config to a watch channel
   * the running stream observes; the stream applies the change on its next
   * tick (or immediately for `poll_seconds`/`station_id`) without
   * restarting. The board page subscribes to the same `$config` store and
   * updates in place — no explicit navigation needed.
   */
  async function persist(): Promise<void> {
    if (saveStateTimer !== null) {
      clearTimeout(saveStateTimer);
      saveStateTimer = null;
    }
    saveState = 'saving';
    await updateConfig({
      station_id: stationId,
      line_ids: lineIds,
      directions: selectedDirections,
      poll_seconds: Math.min(300, Math.max(10, pollSeconds)),
      theme,
    });
    if ($configError !== null) {
      saveState = 'idle';
      return;
    }
    saveState = 'saved';
    saveStateTimer = setTimeout(() => {
      saveState = 'idle';
      saveStateTimer = null;
    }, 1500);
  }

  // Slider events fire on every tick of the drag, and chip / direction /
  // theme toggles can come in bursts. Debounce to the trailing edge so a
  // burst becomes one disk write and one watch-channel publish.
  const persistDebounced = debounce(persist, 400);

  function handleStationSelect(station: Station): void {
    stationId = station.id;
    stationName = station.common_name;
    stationLines = station.lines;
    // Prune line_ids to those the new station actually serves so we never
    // persist a filter the station can't honour.
    if (station.lines.length > 0) {
      const allowed = new Set(station.lines.map((l) => l.id));
      lineIds = lineIds.filter((id) => allowed.has(id));
    }
    void persist();
  }

  // ---------------------------------------------------------------------------
  // Favorites
  // ---------------------------------------------------------------------------

  /** True iff the currently-selected station is in the favorites list. */
  const isCurrentStationFavorited = $derived($favorites.some((f) => f.station_id === stationId));

  async function handleToggleFavorite(): Promise<void> {
    if (isCurrentStationFavorited) {
      await removeFavorite(stationId);
    } else {
      // Use whatever name + lines we know about right now. `currentStationName`
      // already falls back to the latest board's station_name when local
      // state is empty (e.g. user opens Settings on a fresh launch).
      const name =
        stationName.length > 0
          ? stationName
          : ($board?.platforms[0]?.arrivals[0]?.station_name ?? stationId);
      await addFavorite(stationId, name, stationLines);
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
    if (stationName.length > 0) return shortStationName(stationName);
    const fromBoard = $board?.platforms[0]?.arrivals[0]?.station_name ?? '';
    return fromBoard.length > 0 ? shortStationName(fromBoard) : '';
  });

  /**
   * Lines the selected station actually serves, or `null` when unknown
   * (first mount, or stations whose metadata is empty). `null` fails open:
   * every chip is interactive. A non-null set disables everything outside it.
   */
  const availableLineIds = $derived<Set<string> | null>(
    stationLines.length > 0 ? new Set(stationLines.map((l) => l.id)) : null,
  );

  function isLineAvailable(lineId: string): boolean {
    return availableLineIds === null || availableLineIds.has(lineId);
  }

  function toggleLine(lineId: string): void {
    if (!isLineAvailable(lineId)) return;
    if (lineIds.includes(lineId)) {
      lineIds = lineIds.filter((id) => id !== lineId);
    } else {
      lineIds = [...lineIds, lineId];
    }
    // Debounce: a 12-chip toggle burst becomes one save_config carrying
    // the final state, instead of 12 disk writes + 12 cfg_tx.send round
    // trips. The flushPending hook in onDestroy / beforeunload makes
    // sure a click made just before closing Settings still saves.
    persistDebounced();
  }

  function toggleDirection(dir: Direction): void {
    if (selectedDirections.includes(dir)) {
      selectedDirections = selectedDirections.filter((d) => d !== dir);
    } else {
      selectedDirections = [...selectedDirections, dir];
    }
    persistDebounced();
  }

  function handleThemeSelect(newTheme: ThemeId): void {
    theme = newTheme;
    // Live preview — apply to DOM immediately, then debounce the persist.
    // The user sees the theme change instantly; the disk write coalesces
    // if they tap through several themes in quick succession.
    applyTheme(newTheme);
    persistDebounced();
  }

  function handlePollInput(): void {
    // Fires on every slider tick; the debounced persist coalesces the drag
    // so we don't slam save_config → stream-restart 295 times on a full sweep.
    persistDebounced();
  }

  /**
   * Run any pending debounced persist immediately. Called on
   * `onDestroy` (settings unmount) and `beforeunload` (window close)
   * so a click made just inside the debounce window isn't lost.
   */
  function flushPending(): void {
    persistDebounced.flush();
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
    if (typeof window !== 'undefined') {
      window.removeEventListener('beforeunload', flushPending);
    }
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
      class:settings__save-state--saving={saveState === 'saving'}
      class:settings__save-state--saved={saveState === 'saved'}
      role="status"
      aria-live="polite"
      data-testid="settings-save-state"
    >
      {#if saveState === 'saving'}
        Saving…
      {:else if saveState === 'saved'}
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
      <StationSearch selectedId={stationId} onSelect={handleStationSelect} />
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
            class:settings__chip--selected={lineIds.includes(line.id)}
            class:settings__chip--unavailable={!available}
            disabled={!available}
            onclick={() => {
              toggleLine(line.id);
            }}
            aria-pressed={lineIds.includes(line.id)}
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

    <!-- Direction filter -->
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
            class:settings__chip--selected={selectedDirections.includes(dir.id)}
            onclick={() => {
              toggleDirection(dir.id);
            }}
            aria-pressed={selectedDirections.includes(dir.id)}
            aria-label="Toggle {dir.label} direction"
          >
            {dir.label}
          </button>
        {/each}
      </div>
    </section>

    <!-- Poll interval -->
    <section class="settings__section" aria-labelledby="section-poll">
      <h2 id="section-poll" class="settings__section-title">Poll Interval</h2>
      <div class="settings__range-wrap">
        <label for="poll-slider" class="settings__range-label">
          Every {pollSeconds}s
        </label>
        <input
          id="poll-slider"
          type="range"
          min="10"
          max="300"
          step="5"
          bind:value={pollSeconds}
          oninput={handlePollInput}
          class="settings__range"
          aria-label="Poll interval in seconds: {pollSeconds}"
          aria-valuemin={10}
          aria-valuemax={300}
          aria-valuenow={pollSeconds}
        />
        <div class="settings__range-bounds" aria-hidden="true">
          <span>10s</span>
          <span>300s</span>
        </div>
      </div>
    </section>

    <!-- Theme -->
    <section class="settings__section" aria-labelledby="section-theme">
      <h2 id="section-theme" class="settings__section-title">Theme</h2>
      <ThemePicker selected={theme} onSelect={handleThemeSelect} />
    </section>

    <!-- Display mode -->
    <section class="settings__section" aria-labelledby="section-display-mode">
      <h2 id="section-display-mode" class="settings__section-title">Display mode</h2>
      <p class="settings__api-hint">
        Choose how Tubbie shows its board. Changes apply immediately.
      </p>
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

    <!-- Display preferences (frontend-only render flags) -->
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

  .settings__section-hint {
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

  /* Chips */
  .settings__chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .settings__chip {
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

  .settings__chip:hover {
    opacity: 1;
    border-color: var(--platform-label);
  }

  .settings__chip--selected {
    background: var(--chip-selected-bg);
    color: var(--chip-selected-fg);
    border-color: var(--chip-selected-bg);
    opacity: 1;
  }

  .settings__chip--unavailable {
    opacity: 0.3;
    cursor: not-allowed;
    border-style: dashed;
  }

  .settings__chip--unavailable:hover {
    opacity: 0.3;
    border-color: var(--input-border);
  }

  /* Range slider */
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

  /* Generic toggle row used by display-prefs (and any future renderer-only flags). */
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
