<script lang="ts">
  import { board, boardError, isLoading } from '$lib/stores/board.js';
  import Board from '$lib/components/Board.svelte';
  import type { LineStatus } from '$lib/ipc/types.js';

  // Line statuses are fetched lazily and cached here.
  // For M6 scope they start empty; the ticker shows "Good service on all lines".
  // A future milestone can wire getLineStatus calls per board.platforms line_ids.
  let statuses = $state<LineStatus[]>([]);
</script>

<svelte:head>
  <title>tubbie — TfL Arrivals</title>
</svelte:head>

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
    <a href="/settings" class="error__settings-link">Open Settings</a>
  </div>
{:else if $board !== null}
  <Board board={$board} {statuses} stationName={$board.station_id} />
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
    font-family: 'VT323', monospace;
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
    font-family: 'VT323', monospace;
    font-size: 1.2rem;
    color: var(--stale-accent);
    margin: 0;
  }

  .error__hint {
    font-family: 'VT323', monospace;
    font-size: 1rem;
    color: var(--platform-label);
    margin: 0;
    opacity: 0.7;
  }

  .error__settings-link {
    font-family: 'VT323', monospace;
    font-size: 1rem;
    color: var(--fg);
    border: 1px solid var(--fg);
    padding: 0.4rem 1rem;
    text-decoration: none;
    letter-spacing: 0.1em;
  }

  .error__settings-link:hover,
  .error__settings-link:focus {
    background: var(--fg);
    color: var(--bg);
  }
</style>
