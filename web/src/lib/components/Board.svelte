<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import type { Board, LineStatus } from '$lib/ipc/types.js';
  import { formatTime, prettyLineName, shortStationName } from '$lib/utils/format.js';
  import { lastUpdateTs } from '$lib/stores/board.js';
  import { reducedMotion } from '$lib/stores/reducedMotion.js';
  import PlatformColumn from './PlatformColumn.svelte';
  import LineStatusTicker from './LineStatusTicker.svelte';

  interface Props {
    board: Board;
    statuses?: LineStatus[];
    stationName?: string;
    /** Active line filter — when non-empty, the Board shows a "filtering" badge. */
    lineIds?: string[];
  }

  const { board, statuses = [], stationName = '', lineIds = [] }: Props = $props();

  // Pretty-print a line id for the filter badge, preferring a matching
  // arrival's line_name (already in the board data) and falling back to the
  // shared prettyLineName map for lines not present on the current board.
  const filterLabels = $derived(
    lineIds.map((id) => {
      const fromBoard = board.platforms
        .flatMap((p) => p.arrivals)
        .find((a) => a.line_id === id)?.line_name;
      return fromBoard ?? prettyLineName(id);
    }),
  );

  // ---------------------------------------------------------------------------
  // Clock
  // ---------------------------------------------------------------------------

  let now = $state(new Date());
  let clockTimer: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    clockTimer = setInterval(() => {
      now = new Date();
    }, 1000);
  });

  onDestroy(() => {
    if (clockTimer !== null) clearInterval(clockTimer);
  });

  const clockStr = $derived(
    now.toLocaleTimeString('en-GB', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    }),
  );

  // ---------------------------------------------------------------------------
  // Refresh pulse
  // ---------------------------------------------------------------------------

  let pulsing = $state(false);
  let pulseTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    // Trigger pulse whenever lastUpdateTs changes
    if ($lastUpdateTs > 0 && !$reducedMotion) {
      pulsing = false;
      if (pulseTimer !== null) clearTimeout(pulseTimer);
      // Use a microtask break so toggling false→true causes a re-render
      void Promise.resolve().then(() => {
        pulsing = true;
        pulseTimer = setTimeout(() => {
          pulsing = false;
        }, 400);
      });
    }
  });

  // ---------------------------------------------------------------------------
  // Stale indicator
  // ---------------------------------------------------------------------------

  const isStale = $derived(board.stale_since !== null);
  const staleSinceStr = $derived(board.stale_since ? formatTime(board.stale_since) : '');

  // ---------------------------------------------------------------------------
  // Display name
  // ---------------------------------------------------------------------------

  const displayName = $derived(
    stationName.length > 0 ? shortStationName(stationName).toUpperCase() : board.station_id,
  );
</script>

<main class="board" aria-label="Arrivals board for {displayName}">
  <!-- Header -->
  <header
    class="board__header"
    class:refresh-pulse={pulsing}
    class:board__header--stale={isStale}
    aria-label="Station header"
  >
    <div class="board__station-block">
      <h1 class="board__station-name led-accent">
        {displayName}
      </h1>
      {#if filterLabels.length > 0}
        <p
          class="board__line-filter"
          data-testid="board-line-filter"
          role="status"
          aria-label="Filtering by lines: {filterLabels.join(', ')}"
        >
          <span class="board__line-filter-label">Filtering:</span>
          {filterLabels.join(' · ')}
        </p>
      {/if}
    </div>

    <div class="board__header-right">
      {#if isStale}
        <span class="board__stale-badge" role="alert" aria-live="assertive">
          STALE {staleSinceStr}
        </span>
      {/if}

      <time class="board__clock" aria-label="Current time {clockStr}" datetime={now.toISOString()}>
        {clockStr}
      </time>

      <a href="/settings" class="board__settings-btn" aria-label="Open settings" title="Settings">
        <svg
          aria-hidden="true"
          focusable="false"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <circle cx="12" cy="12" r="3" />
          <path
            d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
          />
        </svg>
      </a>
    </div>
  </header>

  <!-- Platforms grid -->
  <div class="board__platforms" aria-label="Platform arrivals" role="region">
    {#each board.platforms as platform (platform.name)}
      <PlatformColumn {platform} maxRows={6} />
    {/each}

    {#if board.platforms.length === 0}
      <div class="board__no-platforms" role="status">No platforms to display</div>
    {/if}
  </div>

  <!-- Line status ticker -->
  <LineStatusTicker {statuses} />
</main>

<style>
  .board {
    display: flex;
    flex-direction: column;
    height: calc(100vh - 24px); /* 24px for Attribution footer */
    background: var(--bg);
    overflow: hidden;
  }

  /* Header */
  .board__header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--header-border);
    flex-shrink: 0;
    min-height: 52px;
    transition: border-bottom-color 0.4s ease;
  }

  .board__header--stale {
    border-bottom-color: var(--stale-accent);
  }

  .board__station-block {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    max-width: 70%;
    min-width: 0;
  }

  .board__station-name {
    font-family: var(--font-board);
    font-size: 1.4rem;
    margin: 0;
    letter-spacing: 0.1em;
    color: var(--accent);
    text-shadow:
      0 0 6px var(--accent),
      0 0 12px color-mix(in srgb, var(--accent) 50%, transparent);
    font-weight: 400;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .board__line-filter {
    font-family: var(--font-board);
    font-size: 0.75rem;
    margin: 0;
    color: var(--platform-label);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    opacity: 0.75;
  }

  .board__line-filter-label {
    color: var(--platform-label);
    opacity: 0.6;
    margin-right: 0.3rem;
  }

  .board__header-right {
    display: flex;
    align-items: center;
    gap: 1rem;
    flex-shrink: 0;
  }

  .board__stale-badge {
    font-family: var(--font-board);
    font-size: 0.85rem;
    color: var(--stale-accent);
    letter-spacing: 0.1em;
    border: 1px solid var(--stale-accent);
    padding: 0.1rem 0.4rem;
    animation: blink-stale 2s step-end infinite;
  }

  @keyframes blink-stale {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.4;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .board__stale-badge {
      animation: none;
      opacity: 1;
    }
  }

  .board__clock {
    font-family: var(--font-board);
    font-size: 1.2rem;
    color: var(--fg);
    letter-spacing: 0.05em;
    text-shadow:
      0 0 4px var(--fg),
      0 0 8px color-mix(in srgb, var(--fg) 30%, transparent);
  }

  .board__settings-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--platform-label);
    text-decoration: none;
    width: 32px;
    height: 32px;
    border: 1px solid var(--row-divider);
    border-radius: 2px;
    transition: color 0.15s ease;
    opacity: 0.7;
  }

  .board__settings-btn:hover,
  .board__settings-btn:focus {
    color: var(--fg);
    opacity: 1;
    border-color: var(--platform-label);
  }

  /* Platforms grid */
  .board__platforms {
    display: flex;
    flex-direction: row;
    flex-wrap: wrap;
    gap: 1px;
    flex: 1;
    overflow-y: auto;
    background: var(--row-divider);
    padding: 1px;
  }

  .board__no-platforms {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--platform-label);
    font-family: var(--font-board);
    font-size: 1.2rem;
    opacity: 0.5;
  }

  /* Narrow screens: stack platforms vertically */
  @media (max-width: 800px) {
    .board__platforms {
      flex-direction: column;
    }
  }
</style>
