<script lang="ts">
  import { onMount } from 'svelte';
  import { hasAppKey, saveAppKey, openExternal } from '$lib/ipc/commands.js';

  const API_PORTAL_URL = 'https://api-portal.tfl.gov.uk';

  let appKey = $state('');
  let hasStoredAppKey = $state(false);
  let appKeyVisible = $state(false);
  let appKeyStatus = $state<string | null>(null);
  let appKeySaving = $state(false);

  onMount(async () => {
    try {
      // SECURITY: only fetch presence, not the actual key value.
      // The key must never be loaded into the renderer heap unless the user
      // explicitly triggers a "reveal" action (post-MVP).
      hasStoredAppKey = await hasAppKey();
      // Phase 1 pool-key: the happy path is invisible. When the user has no
      // personal key, tubbie connects via a built-in key automatically — say
      // nothing about keys or quotas (a pool key may be active, and quota math
      // only alarms). Status is shown only once the user sets their own key.
      appKeyStatus = hasStoredAppKey ? 'Using your TfL API key' : null;
    } catch {
      appKeyStatus = 'Could not load API key status';
    }
  });

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
</script>

<section class="settings__section" aria-labelledby="section-apikey">
  <h2 id="section-apikey" class="settings__section-title">TfL API Key</h2>
  {#if appKeyStatus}
    <p class="settings__api-status" aria-live="polite">{appKeyStatus}</p>
  {/if}
  <p class="settings__api-hint">
    Optional — tubbie works out of the box. To use your own TfL key, register at
    <a
      href={API_PORTAL_URL}
      onclick={(e) => {
        e.preventDefault();
        void openExternal(API_PORTAL_URL);
      }}
      class="settings__link">api-portal.tfl.gov.uk</a
    >.
  </p>
  <div class="settings__api-input-row">
    <input
      type={appKeyVisible ? 'text' : 'password'}
      id="api-key-input"
      class="settings__api-input"
      bind:value={appKey}
      placeholder={hasStoredAppKey ? '(stored — type new to replace)' : '(optional TfL API key)'}
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

<style>
  /* API-key-only rules. Shared rules (.settings__section, .settings__section-title,
     .settings__api-status, .settings__api-hint, .settings__btn,
     .settings__btn--secondary, .settings__link) live in
     routes/settings/+page.svelte under :global(...) so this component doesn't
     have to copy them. */

  .settings__api-hint--small {
    font-size: 0.75rem;
    opacity: 0.5;
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
    font-family: var(--font-ui);
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
    font-family: var(--font-ui);
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
</style>
