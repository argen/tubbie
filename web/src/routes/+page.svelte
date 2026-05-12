<script lang="ts">
  import { board, boardError, isLoading } from '$lib/stores/board.js';
  import { config, configError } from '$lib/stores/config.js';
  import Board from '$lib/components/Board.svelte';
  import { getLineStatus, openSettingsWindow } from '$lib/ipc/commands.js';
  import type { Board as BoardT, LineStatus } from '$lib/ipc/types.js';

  let statuses = $state<LineStatus[]>([]);

  function uniqueLineIds(b: BoardT | null): string[] {
    if (b === null) return [];
    const ids: string[] = [];
    for (const p of b.platforms) {
      for (const a of p.arrivals) {
        if (a.line_id.length > 0 && !ids.includes(a.line_id)) ids.push(a.line_id);
      }
    }
    return ids.sort();
  }

  async function fetchStatuses(ids: string[]): Promise<void> {
    if (ids.length === 0) {
      statuses = [];
      return;
    }
    const results = await Promise.allSettled(ids.map((id) => getLineStatus(id)));
    statuses = results.flatMap((r) => (r.status === 'fulfilled' ? [r.value] : []));
  }

  // Refresh on line-id set change and every 60s so a live disruption lands in
  // the ticker without a full board reload. The $derived key means we don't
  // tear down the interval on every 10-second board poll.
  const REFRESH_MS = 60_000;
  const lineIdsKey = $derived(uniqueLineIds($board).join(','));

  $effect(() => {
    const ids = lineIdsKey.length > 0 ? lineIdsKey.split(',') : [];
    void fetchStatuses(ids);
    const t = setInterval((): void => {
      void fetchStatuses(ids);
    }, REFRESH_MS);
    return (): void => {
      clearInterval(t);
    };
  });
</script>

<svelte:head>
  <title>tubbie — TfL Arrivals</title>
</svelte:head>

{#if $configError && $board === null}
  <div class="config-error" role="alert">
    <p class="config-error__message">{$configError}</p>
    <p class="config-error__hint">
      <button
        type="button"
        class="config-error__link"
        onclick={() => void openSettingsWindow()}
      >Open Settings</button> to fix the configuration.
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
    <button
      type="button"
      class="error__settings-link"
      onclick={() => void openSettingsWindow()}
    >Open Settings</button>
  </div>
{:else if $board !== null}
  <Board
    board={$board}
    {statuses}
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
