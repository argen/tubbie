<script lang="ts">
  import { goto } from '$app/navigation';
  import { config, updateConfig, applyTheme, type ThemeId } from '$lib/stores/config.js';
  import StationSearch from '$lib/components/StationSearch.svelte';
  import ThemePicker from '$lib/components/ThemePicker.svelte';
  import { loadAppKey, saveAppKey } from '$lib/ipc/commands.js';
  import type { Direction, Station } from '$lib/ipc/types.js';
  import { onMount } from 'svelte';

  // ---------------------------------------------------------------------------
  // Local form state (mirrors config; saved explicitly on submit)
  // ---------------------------------------------------------------------------

  let stationId = $state($config.station_id);
  let stationName = $state('');
  let lineIds = $state<string[]>([...$config.line_ids]);
  let selectedDirections = $state<Direction[]>([...$config.directions]);
  let pollSeconds = $state($config.poll_seconds);
  let theme = $state<string>($config.theme);
  let appKey = $state('');
  let appKeyVisible = $state(false);
  let appKeyStatus = $state<string | null>(null);
  let appKeySaving = $state(false);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let saveSuccess = $state(false);

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
      const key = await loadAppKey();
      if (key) {
        appKey = key;
        appKeyStatus = 'Using your TfL API key';
      } else {
        appKeyStatus = 'Using anonymous access (50 requests/min)';
      }
    } catch {
      appKeyStatus = 'Could not load API key status';
    }
  });

  function handleStationSelect(station: Station): void {
    stationId = station.id;
    stationName = station.common_name;
  }

  function toggleLine(lineId: string): void {
    if (lineIds.includes(lineId)) {
      lineIds = lineIds.filter((id) => id !== lineId);
    } else {
      lineIds = [...lineIds, lineId];
    }
  }

  function toggleDirection(dir: Direction): void {
    if (selectedDirections.includes(dir)) {
      selectedDirections = selectedDirections.filter((d) => d !== dir);
    } else {
      selectedDirections = [...selectedDirections, dir];
    }
  }

  function handleThemeSelect(newTheme: ThemeId): void {
    theme = newTheme;
    // Live preview — apply to DOM immediately
    applyTheme(newTheme);
  }

  async function handleSave(): Promise<void> {
    saving = true;
    saveError = null;
    saveSuccess = false;
    try {
      await updateConfig({
        station_id: stationId,
        line_ids: lineIds,
        directions: selectedDirections,
        poll_seconds: Math.min(300, Math.max(5, pollSeconds)),
        theme,
      });
      saveSuccess = true;
      setTimeout(() => {
        saveSuccess = false;
      }, 2000);
    } catch (err: unknown) {
      saveError = err instanceof Error ? err.message : String(err);
    } finally {
      saving = false;
    }
  }

  async function handleSaveAppKey(): Promise<void> {
    appKeySaving = true;
    try {
      const keyToSave = appKey.trim().length > 0 ? appKey.trim() : null;
      const msg = await saveAppKey(keyToSave);
      appKeyStatus = keyToSave ? `Using your TfL API key — ${msg}` : `Cleared. ${msg}`;
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
          <button
            type="button"
            class="settings__chip"
            class:settings__chip--selected={lineIds.includes(line.id)}
            onclick={() => {
              toggleLine(line.id);
            }}
            aria-pressed={lineIds.includes(line.id)}
            aria-label="Toggle {line.label} line"
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
          placeholder="Paste your TfL app key…"
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
      <button
        type="button"
        class="settings__btn settings__btn--secondary"
        onclick={handleSaveAppKey}
        disabled={appKeySaving}
        aria-label="Save API key (requires restart)"
      >
        {appKeySaving ? 'Saving…' : 'Save Key'}
      </button>
      <p id="api-key-hint" class="settings__api-hint settings__api-hint--small">
        Key is stored securely in the system app-data folder. Restart required to apply.
      </p>
    </section>

    <!-- Save button -->
    <div class="settings__actions">
      {#if saveError}
        <p class="settings__save-error" role="alert">{saveError}</p>
      {/if}
      {#if saveSuccess}
        <p class="settings__save-success" role="status" aria-live="polite">Saved!</p>
      {/if}
      <button
        type="button"
        class="settings__btn settings__btn--primary"
        onclick={handleSave}
        disabled={saving}
        aria-label="Save settings and return to board"
      >
        {saving ? 'Saving…' : 'Save Settings'}
      </button>
    </div>
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

  .settings__header {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.75rem 1.5rem;
    border-bottom: 1px solid var(--row-divider);
    flex-shrink: 0;
  }

  .settings__back-btn {
    font-family: 'VT323', monospace;
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
    font-family: 'DSEG14Classic', 'VT323', monospace;
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
    font-family: 'VT323', monospace;
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
    font-family: 'VT323', monospace;
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
    font-family: 'VT323', monospace;
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

  /* Range slider */
  .settings__range-wrap {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .settings__range-label {
    font-family: 'DSEG14Classic', 'VT323', monospace;
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
    font-family: 'VT323', monospace;
    font-size: 0.75rem;
    color: var(--platform-label);
    opacity: 0.5;
  }

  /* API key */
  .settings__api-status {
    font-family: 'VT323', monospace;
    font-size: 0.95rem;
    color: var(--accent);
    margin: 0;
    opacity: 0.9;
  }

  .settings__api-hint {
    font-family: 'VT323', monospace;
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
    font-family: 'VT323', monospace;
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
    font-family: 'VT323', monospace;
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

  /* Buttons */
  .settings__btn {
    font-family: 'VT323', monospace;
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

  .settings__btn--primary {
    background: var(--button-bg);
    color: var(--button-fg);
  }

  .settings__btn--primary:hover:not(:disabled),
  .settings__btn--primary:focus:not(:disabled) {
    filter: brightness(1.1);
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

  /* Actions */
  .settings__actions {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding-top: 1rem;
    border-top: 1px solid var(--row-divider);
    padding-bottom: 2rem;
  }

  .settings__save-error {
    font-family: 'VT323', monospace;
    font-size: 0.95rem;
    color: var(--stale-accent);
    margin: 0;
  }

  .settings__save-success {
    font-family: 'VT323', monospace;
    font-size: 0.95rem;
    color: var(--accent);
    margin: 0;
  }
</style>
