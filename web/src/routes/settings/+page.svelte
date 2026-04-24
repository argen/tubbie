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
  import { hasAppKey, saveAppKey } from '$lib/ipc/commands.js';
  import { debounce } from '$lib/utils/debounce.js';
  import type { Direction, LineRef, Station } from '$lib/ipc/types.js';
  import { onMount } from 'svelte';

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
  let appKey = $state('');
  let hasStoredAppKey = $state(false);
  let appKeyVisible = $state(false);
  let appKeyStatus = $state<string | null>(null);
  let appKeySaving = $state(false);
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

  const KNOWN_LINES: { id: string; label: string }[] = [
    { id: 'bakerloo', label: 'Bakerloo' },
    { id: 'central', label: 'Central' },
    { id: 'circle', label: 'Circle' },
    { id: 'district', label: 'District' },
    { id: 'elizabeth-line', label: 'Elizabeth' },
    { id: 'hammersmith-city', label: 'Hammersmith & City' },
    { id: 'jubilee', label: 'Jubilee' },
    { id: 'metropolitan', label: 'Metropolitan' },
    { id: 'northern', label: 'Northern' },
    { id: 'piccadilly', label: 'Piccadilly' },
    { id: 'victoria', label: 'Victoria' },
    { id: 'waterloo-city', label: 'Waterloo & City' },
  ];

  onMount(async () => {
    try {
      // SECURITY: only fetch presence, not the actual key value.
      // The key must never be loaded into the renderer heap unless the user
      // explicitly triggers a "reveal" action (post-MVP).
      hasStoredAppKey = await hasAppKey();
      appKeyStatus = hasStoredAppKey
        ? 'Using your TfL API key'
        : 'Using anonymous access (50 requests/min)';
    } catch {
      appKeyStatus = 'Could not load API key status';
    }
  });

  /**
   * Persist the current form state. `updateConfig` catches its own errors
   * and drives `$configError`, so callers never need to try/catch.
   *
   * The backend's `save_config` aborts the current stream; the watcher loop
   * in `src-tauri/src/lib.rs` restarts it with the new config. No explicit
   * navigation needed — the board page subscribes to the same `$config`
   * store and updates in place.
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
      poll_seconds: Math.min(300, Math.max(5, pollSeconds)),
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

  // Slider events fire on every tick of the drag; each persist round-trips
  // through save_config + stream-restart, so debounce to the trailing edge.
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
    void persist();
  }

  function toggleDirection(dir: Direction): void {
    if (selectedDirections.includes(dir)) {
      selectedDirections = selectedDirections.filter((d) => d !== dir);
    } else {
      selectedDirections = [...selectedDirections, dir];
    }
    void persist();
  }

  function handleThemeSelect(newTheme: ThemeId): void {
    theme = newTheme;
    // Live preview — apply to DOM immediately, then persist.
    applyTheme(newTheme);
    void persist();
  }

  function handlePollInput(): void {
    // Fires on every slider tick; the debounced persist coalesces the drag
    // so we don't slam save_config → stream-restart 295 times on a full sweep.
    persistDebounced();
  }

  async function handleSaveAppKey(): Promise<void> {
    appKeySaving = true;
    try {
      const trimmed = appKey.trim();
      const keyToSave = trimmed.length > 0 ? trimmed : null;
      const msg = await saveAppKey(keyToSave);
      // Clear from heap immediately — key must not linger in renderer state.
      appKey = '';
      hasStoredAppKey = keyToSave !== null;
      appKeyStatus = keyToSave ? `Using your TfL API key — ${msg}` : `Cleared. ${msg}`;
    } catch (err: unknown) {
      appKeyStatus = `Error: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      appKeySaving = false;
    }
  }

  async function handleClearAppKey(): Promise<void> {
    appKeySaving = true;
    try {
      await saveAppKey(null);
      appKey = '';
      hasStoredAppKey = false;
      appKeyStatus = 'Cleared. Restart to apply.';
    } catch (err: unknown) {
      appKeyStatus = `Error: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      appKeySaving = false;
    }
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
      <StationSearch selectedId={stationId} onSelect={handleStationSelect} />
      {#if stationName}
        <p class="settings__selected-station" aria-live="polite">
          Selected: {stationName}
        </p>
      {:else if stationId}
        <p class="settings__selected-station" aria-live="polite">
          Station ID: {stationId}
        </p>
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
            title={available ? undefined : `Not served by ${stationName || 'this station'}`}
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
          min="5"
          max="300"
          step="5"
          bind:value={pollSeconds}
          oninput={handlePollInput}
          class="settings__range"
          aria-label="Poll interval in seconds: {pollSeconds}"
          aria-valuemin={5}
          aria-valuemax={300}
          aria-valuenow={pollSeconds}
        />
        <div class="settings__range-bounds" aria-hidden="true">
          <span>5s</span>
          <span>300s</span>
        </div>
      </div>
    </section>

    <!-- Theme -->
    <section class="settings__section" aria-labelledby="section-theme">
      <h2 id="section-theme" class="settings__section-title">Theme</h2>
      <ThemePicker selected={theme} onSelect={handleThemeSelect} />
    </section>

    <!-- API key -->
    <section class="settings__section" aria-labelledby="section-apikey">
      <h2 id="section-apikey" class="settings__section-title">TfL API Key</h2>
      {#if appKeyStatus}
        <p class="settings__api-status" aria-live="polite">{appKeyStatus}</p>
      {/if}
      <p class="settings__api-hint">
        Optional. Register at
        <a
          href="https://api-portal.tfl.gov.uk"
          target="_blank"
          rel="noopener noreferrer"
          class="settings__link">api-portal.tfl.gov.uk</a
        >. Anonymous access allows 50 req/min.
      </p>
      <div class="settings__api-input-row">
        <input
          type={appKeyVisible ? 'text' : 'password'}
          id="api-key-input"
          class="settings__api-input"
          bind:value={appKey}
          placeholder={hasStoredAppKey
            ? '(stored — type new to replace)'
            : '(optional TfL API key)'}
          autocomplete="off"
          maxlength={64}
          aria-label="TfL API key (optional)"
          aria-describedby="api-key-hint"
        />
        <button
          type="button"
          class="settings__api-reveal-btn"
          onclick={() => {
            appKeyVisible = !appKeyVisible;
          }}
          aria-label={appKeyVisible ? 'Hide API key' : 'Show API key'}
          aria-pressed={appKeyVisible}
        >
          {appKeyVisible ? 'Hide' : 'Show'}
        </button>
      </div>
      <div class="settings__api-actions">
        <button
          type="button"
          class="settings__btn settings__btn--secondary"
          onclick={handleSaveAppKey}
          disabled={appKeySaving}
          aria-label="Save API key (requires restart)"
        >
          {appKeySaving ? 'Saving…' : 'Save Key'}
        </button>
        {#if hasStoredAppKey}
          <button
            type="button"
            class="settings__btn settings__btn--secondary"
            onclick={handleClearAppKey}
            disabled={appKeySaving}
            aria-label="Clear stored API key"
          >
            Clear Key
          </button>
        {/if}
      </div>
      <p id="api-key-hint" class="settings__api-hint settings__api-hint--small">
        Key is stored securely in the system app-data folder. Restart required to apply.
      </p>
    </section>
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
    font-family: var(--font-board);
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
    font-family: var(--font-board);
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

  .settings__section {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .settings__section-title {
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

  .settings__selected-station {
    font-family: var(--font-board);
    font-size: 0.9rem;
    color: var(--accent);
    margin: 0;
    opacity: 0.8;
  }

  /* Chips */
  .settings__chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .settings__chip {
    font-family: var(--font-board);
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
    font-family: var(--font-board);
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
    font-family: var(--font-board);
    font-size: 0.75rem;
    color: var(--platform-label);
    opacity: 0.5;
  }

  /* API key */
  .settings__api-status {
    font-family: var(--font-board);
    font-size: 0.95rem;
    color: var(--accent);
    margin: 0;
    opacity: 0.9;
  }

  .settings__api-hint {
    font-family: var(--font-board);
    font-size: 0.85rem;
    color: var(--platform-label);
    margin: 0;
    opacity: 0.7;
    line-height: 1.4;
  }

  .settings__api-hint--small {
    font-size: 0.75rem;
    opacity: 0.5;
  }

  .settings__link {
    color: var(--fg);
    opacity: 0.9;
  }

  .settings__api-input-row {
    display: flex;
    gap: 0.5rem;
  }

  .settings__api-input {
    flex: 1;
    background: var(--input-bg);
    border: 1px solid var(--input-border);
    color: var(--fg);
    font-family: var(--font-board);
    font-size: 1rem;
    padding: 0.5rem 0.75rem;
    border-radius: 2px;
    outline: none;
    letter-spacing: 0.04em;
  }

  .settings__api-input:focus {
    border-color: var(--fg);
    box-shadow: 0 0 0 2px var(--focus-ring);
  }

  .settings__api-reveal-btn {
    font-family: var(--font-board);
    font-size: 0.9rem;
    background: var(--chip-bg);
    color: var(--fg);
    border: 1px solid var(--input-border);
    padding: 0.3rem 0.75rem;
    cursor: pointer;
    border-radius: 2px;
    white-space: nowrap;
  }

  .settings__api-reveal-btn:hover,
  .settings__api-reveal-btn:focus {
    border-color: var(--platform-label);
  }

  .settings__api-actions {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  /* Buttons */
  .settings__btn {
    font-family: var(--font-board);
    font-size: 1.1rem;
    padding: 0.5rem 1.5rem;
    border: none;
    border-radius: 2px;
    cursor: pointer;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    width: fit-content;
  }

  .settings__btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .settings__btn--secondary {
    background: transparent;
    color: var(--fg);
    border: 1px solid var(--input-border);
  }

  .settings__btn--secondary:hover:not(:disabled),
  .settings__btn--secondary:focus:not(:disabled) {
    border-color: var(--fg);
  }

  .settings__save-state {
    font-family: var(--font-board);
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
