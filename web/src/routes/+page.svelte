<script lang="ts">
  import { onMount } from 'svelte';
  import { board, boardError, isLoading } from '$lib/stores/board.js';
  import { config, configError } from '$lib/stores/config.js';
  import Board from '$lib/components/Board.svelte';
  import FirstRunPrompt from '$lib/components/FirstRunPrompt.svelte';
  import { getAllLineStatuses, openSettingsWindow, setTrayDisruption } from '$lib/ipc/commands.js';
  import { anyDisrupted } from '$lib/utils/status.js';
  import type { Board as BoardT, LineStatus } from '$lib/ipc/types.js';

  // First-run prompt: shown once, until the user picks a station or dismisses.
  // Persisted in localStorage (a UI nicety flag, not app config) so it never
  // gates the board and survives normal launches.
  const ONBOARDED_KEY = 'tubbie:onboarded';
  let showFirstRun = $state(false);
  onMount(() => {
    try {
      showFirstRun = localStorage.getItem(ONBOARDED_KEY) !== 'true';
    } catch {
      showFirstRun = false;
    }
  });
  function completeOnboarding(): void {
    showFirstRun = false;
    try {
      localStorage.setItem(ONBOARDED_KEY, 'true');
    } catch {
      // localStorage unavailable — worst case the prompt reappears next launch.
    }
  }

  let statuses = $state<LineStatus[]>([]);
  // Network-wide fetch is all-or-nothing — no partial state from the backend.
  // The state var is kept so StatusView/StatusPanel props stay stable.
  let statusPartial = $state(false);
  // Epoch ms of the last successful fetch — the Status view's "Updated …"
  // freshness line. Stays put on failure (keep last-known data).
  let statusUpdatedAt = $state<number | null>(null);

  /** Fetch network-wide statuses (all TfL lines, worst-first). On reject,
   *  keep prior statuses — don't clear the board. */
  async function refreshStatuses(): Promise<void> {
    try {
      const result = await getAllLineStatuses();
      statuses = result;
      statusPartial = false;
      statusUpdatedAt = Date.now();
    } catch {
      // Keep prior statuses on failure; the freshness line will show staleness.
    }
  }

  // Fetch once on mount and every 60 s. Station changes no longer re-key
  // this effect — status is network-wide, not station-scoped.
  const REFRESH_MS = 60_000;

  $effect(() => {
    void refreshStatuses();
    const t = setInterval((): void => {
      void refreshStatuses();
    }, REFRESH_MS);
    return (): void => {
      clearInterval(t);
    };
  });

  // Lines serving the current board — the fallback scope for the tray alert
  // when the user has no explicit line filter set.
  function boardLineIds(b: BoardT | null): string[] {
    if (b === null) return [];
    const ids: string[] = [];
    for (const p of b.platforms) {
      for (const a of p.arrivals) {
        if (a.line_id.length > 0 && !ids.includes(a.line_id)) ids.push(a.line_id);
      }
    }
    return ids;
  }

  // Drive the menu-bar disruption icon. The ambient tray glance is scoped to
  // the lines the user is actually WATCHING — their explicit filter if set,
  // otherwise the lines serving the current station — NOT the whole network.
  // (The in-app Status panel/view are network-wide now; a network-wide tray
  // would light up for, e.g., a DLR fault while you're watching a tube-only
  // station.) Only pushes to the backend on a state CHANGE so we don't
  // re-dispatch a Cocoa icon swap every poll. No-op in window mode (no tray).
  // The hidden popover's JS keeps polling, so this stays current when closed.
  let lastDisruption: boolean | null = $state(null);
  $effect(() => {
    const scope = $config.line_ids.length > 0 ? $config.line_ids : boardLineIds($board);
    // No watched lines yet (board still loading, no filter) → nothing to alert.
    const disrupted = scope.length > 0 && anyDisrupted(statuses, scope);
    if (disrupted !== lastDisruption) {
      lastDisruption = disrupted;
      void setTrayDisruption(disrupted);
    }
  });
</script>

<svelte:head>
  <title>tubbie — TfL Arrivals</title>
</svelte:head>

{#if showFirstRun}
  <FirstRunPrompt onDone={completeOnboarding} />
{/if}

{#if $configError && $board === null}
  <div class="config-error" role="alert">
    <p class="config-error__message">{$configError}</p>
    <p class="config-error__hint">
      <button type="button" class="config-error__link" onclick={() => void openSettingsWindow()}
        >Open Settings</button
      > to fix the configuration.
    </p>
  </div>
{/if}

{#if $isLoading && $board === null}
  <div class="loading" role="status" aria-live="polite">
    <span class="loading__text">Loading arrivals…</span>
  </div>
{:else if $boardError && $board === null}
  <div class="error" role="alert">
    <p class="error__message">{$boardError}</p>
    <p class="error__hint">
      Check your connection and open Settings to verify the station configuration.
    </p>
    <button type="button" class="error__settings-link" onclick={() => void openSettingsWindow()}
      >Open Settings</button
    >
  </div>
{:else if $board !== null}
  <Board
    board={$board}
    {statuses}
    {statusPartial}
    {statusUpdatedAt}
    onStatusRefresh={refreshStatuses}
    stationName={$board.platforms[0]?.arrivals[0]?.station_name ?? $board.station_id}
    lineIds={$config.line_ids}
  />
{:else}
  <!-- No board yet, show waiting state -->
  <div class="loading" role="status" aria-live="polite">
    <span class="loading__text">Waiting for arrival data…</span>
  </div>
{/if}

<style>
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    height: calc(100vh - 24px);
    background: var(--bg);
  }

  .loading__text {
    font-family: var(--font-board);
    font-size: 1.5rem;
    color: var(--fg);
    opacity: 0.6;
    letter-spacing: 0.1em;
    animation: loading-blink 1.4s step-end infinite;
  }

  @keyframes loading-blink {
    0%,
    100% {
      opacity: 0.6;
    }
    50% {
      opacity: 0.2;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .loading__text {
      animation: none;
      opacity: 0.6;
    }
  }

  .error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: calc(100vh - 24px);
    background: var(--bg);
    gap: 1rem;
    padding: 2rem;
    text-align: center;
  }

  .error__message {
    font-family: var(--font-board);
    font-size: 1.2rem;
    color: var(--stale-accent);
    margin: 0;
  }

  .error__hint {
    font-family: var(--font-board);
    font-size: 1rem;
    color: var(--platform-label);
    margin: 0;
    opacity: 0.7;
  }

  .error__settings-link {
    font-family: var(--font-board);
    font-size: 1rem;
    color: var(--fg);
    background: transparent;
    border: 1px solid var(--fg);
    padding: 0.4rem 1rem;
    cursor: pointer;
    letter-spacing: 0.1em;
  }

  .error__settings-link:hover,
  .error__settings-link:focus {
    background: var(--fg);
    color: var(--bg);
  }

  /* Inline config-error banner — shown even when board is visible */
  .config-error {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    z-index: 100;
    background: color-mix(in srgb, var(--stale-accent) 15%, var(--bg));
    border-bottom: 1px solid var(--stale-accent);
    padding: 0.5rem 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .config-error__message {
    font-family: var(--font-board);
    font-size: 1rem;
    color: var(--stale-accent);
    margin: 0;
  }

  .config-error__hint {
    font-family: var(--font-board);
    font-size: 0.85rem;
    color: var(--platform-label);
    margin: 0;
    opacity: 0.8;
  }

  .config-error__link {
    background: none;
    border: none;
    padding: 0;
    color: var(--fg);
    cursor: pointer;
    font-family: inherit;
    font-size: inherit;
    text-decoration: underline;
  }
</style>
