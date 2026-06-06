<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import type { Arrival, Board, Direction, LineStatus } from '$lib/ipc/types.js';
  import {
    formatTime,
    formatUpdatedAgo,
    prettyLineName,
    shortPlatformName,
    shortStationName,
  } from '$lib/utils/format.js';
  import { disruptedLinesWorstFirst } from '$lib/utils/status.js';
  import { lastUpdateTs } from '$lib/stores/board.js';
  import { reducedMotion } from '$lib/stores/reducedMotion.js';
  import { displayMode } from '$lib/stores/displayMode.js';
  import { displayPrefs } from '$lib/stores/displayPrefs.js';
  import { applyBoardSize, openSettingsWindow } from '$lib/ipc/commands.js';
  import { config } from '$lib/stores/config.js';
  import { selectStation } from '$lib/stores/settingsForm.js';
  import type { Station } from '$lib/ipc/types.js';
  import LineGroup from './LineGroup.svelte';
  import StatusPanel from './StatusPanel.svelte';
  import StatusView from './StatusView.svelte';
  import StationSearch from './StationSearch.svelte';

  interface Props {
    board: Board;
    statuses?: LineStatus[];
    stationName?: string;
    /** Active line filter — when non-empty, the Board shows a "filtering" badge. */
    lineIds?: string[];
    /** True when one or more lines' status could not be fetched this cycle. */
    statusPartial?: boolean;
    /** Epoch ms of the last successful status fetch (for the freshness line). */
    statusUpdatedAt?: number | null;
    /** Manual status refresh (Status view "Refresh" button). May be async. */
    onStatusRefresh?: () => void | Promise<void>;
  }

  const {
    board,
    statuses = [],
    stationName = '',
    lineIds = [],
    statusPartial = false,
    statusUpdatedAt = null,
    onStatusRefresh,
  }: Props = $props();

  // Header view toggle: the board body shows either arrivals or the full
  // Service-status view (the desktop equivalent of the iOS Status tab).
  let view = $state<'arrivals' | 'status'>('arrivals');

  // Station-search overlay — toggled by the magnifier button in the header.
  let searchOpen = $state(false);
  let searchToggleEl = $state<HTMLButtonElement>();
  let searchOverlayEl = $state<HTMLDivElement>();

  function closeSearch(): void {
    searchOpen = false;
    // Return focus to the toggle so keyboard / screen-reader users aren't
    // dropped at the top of the document when the overlay unmounts.
    searchToggleEl?.focus();
  }

  function handleStationSelect(station: Station): void {
    selectStation(station);
    closeSearch();
  }

  // When the overlay opens: move focus into the search input (so keyboard /
  // screen-reader users land in the field) and bind a WINDOW-level Escape
  // handler so Esc closes it regardless of where focus currently sits — not
  // only when focus is already inside the overlay div.
  $effect(() => {
    if (!searchOpen) return;
    void tick().then(() => {
      searchOverlayEl?.querySelector<HTMLInputElement>('input')?.focus();
    });
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') closeSearch();
    };
    window.addEventListener('keydown', onKey);
    return (): void => {
      window.removeEventListener('keydown', onKey);
    };
  });
  // Disruption count drives the toggle's badge. Network-wide — matches the
  // StatusView it opens (invariant #22: lineIds is a display-mask on arrivals
  // only; the status badge reflects the full network picture).
  const disruptionCount = $derived(disruptedLinesWorstFirst(statuses).length);

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

  // Freshness label for the Status view — reuses the ticking `now` above, so
  // no extra timer. "" until the first status fetch lands.
  const statusUpdatedLabel = $derived(formatUpdatedAgo(statusUpdatedAt, now.getTime()));

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

  // ---------------------------------------------------------------------------
  // Group arrivals by line, then by direction
  //
  // The Rust backend (`crates/tfl-board::build_board`) buckets arrivals by
  // **Direction**, not by line — `Board.platforms[]` is at most seven
  // entries (Northbound, Southbound, Eastbound, Westbound, Inbound,
  // Outbound, Unknown), and a single direction bucket explicitly merges
  // arrivals from multiple lines (e.g. King's Cross "Westbound" carries
  // both hammersmith-city and metropolitan trains; Baker Street southbound
  // has Bakerloo + Jubilee on shared platforms).
  //
  // For the line-grouped UI we have to invert the structure: walk every
  // arrival, bucket by `line_id`, then by `direction` inside each line.
  // This is the only correct grouping for multi-line interchanges —
  // grouping by `Platform.arrivals[0].line_id` (the previous attempt)
  // labels the entire direction column by whichever line was scheduled
  // first and silently mis-colours every minority-line train.
  //
  // The synthetic `Platform` we hand to `PlatformColumn` carries
  // `name = direction.label` and the line+direction-filtered arrivals.
  // PlatformColumn's existing dedupe key
  // `${line_id}|${platform_name}|${expected_arrival}` stays unique even
  // when arrivals were merged from multiple physical platforms by the
  // backend (their `platform_name` differs).
  // ---------------------------------------------------------------------------

  interface DirectionBucket {
    key: string;
    label: string;
    arrivals: Arrival[];
  }

  interface LineBucket {
    lineId: string;
    lineName: string;
    directions: DirectionBucket[];
  }

  // Canonical compass order. The backend guarantees Platform order in the
  // same sequence (`crates/tfl-board::build_board`), but we apply our own
  // sort because we are re-grouping per arrival and a line may be missing
  // some directions (e.g. Bakerloo only has Northbound/Southbound at
  // Baker Street, no Inbound/Outbound).
  const DIRECTION_ORDER: Direction[] = [
    'Northbound',
    'Southbound',
    'Eastbound',
    'Westbound',
    'Inbound',
    'Outbound',
    'Unknown',
  ];

  function directionKeyAndLabel(arrival: Arrival): { key: string; label: string } {
    if (arrival.direction !== 'Unknown') {
      return { key: arrival.direction, label: arrival.direction };
    }
    // Defensive fallback for the rare case where an arrival's direction
    // didn't infer cleanly. Use the prefix of `platform_name` (TfL's raw
    // string, e.g. "Inner Rail"), which the backend already used to
    // populate Platform.name. Keys are namespaced so they can never
    // collide with the canonical Direction enum keys.
    const fallback = shortPlatformName(arrival.platform_name);
    const safe = fallback.length > 0 ? fallback : 'Unknown';
    return { key: `name:${safe}`, label: safe };
  }

  function compareDirections(a: DirectionBucket, b: DirectionBucket): number {
    const aIdx = DIRECTION_ORDER.indexOf(a.key as Direction);
    const bIdx = DIRECTION_ORDER.indexOf(b.key as Direction);
    if (aIdx === -1 && bIdx === -1) return a.label.localeCompare(b.label);
    if (aIdx === -1) return 1;
    if (bIdx === -1) return -1;
    return aIdx - bIdx;
  }

  // Two parallel arrays rather than a `Map` — the lint forbids mutable
  // Maps even when scoped to a single derivation pass. With only ~20
  // arrivals and ~5 lines on a busy station, O(n²) lookup is irrelevant.
  //
  // The user's `lineIds` chip filter is applied HERE at the display
  // layer, NOT in the Rust `apply_filters`. CLAUDE.md invariant #22:
  // toggling a chip in Settings re-derives `linesGrouped` instantly
  // off the locally-stored `$board`, so the visible board updates in
  // a frame — no waiting for the next ~30 s periodic stream tick to
  // re-emit a backend-filtered payload. Non-empty `lineIds` masks
  // arrivals whose `line_id` is not in the set; empty = show all.
  const linesGrouped = $derived.by(() => {
    // Insertion order in `lineBuckets` is the first-seen order across
    // arrivals. `find` keeps TS happy without an out-of-bounds-aware
    // index, and at ≤6 lines per station the linear scan is irrelevant.
    const lineBuckets: LineBucket[] = [];
    const lineFilterActive = lineIds.length > 0;

    for (const platform of board.platforms) {
      for (const arrival of platform.arrivals) {
        if (lineFilterActive && !lineIds.includes(arrival.line_id)) {
          continue;
        }
        const { line_id: lineId, line_name: lineName } = arrival;
        let bucket = lineBuckets.find((b) => b.lineId === lineId);
        if (bucket === undefined) {
          bucket = { lineId, lineName, directions: [] };
          lineBuckets.push(bucket);
        } else if (bucket.lineName.length === 0 && lineName.length > 0) {
          bucket.lineName = lineName;
        }

        const { key, label } = directionKeyAndLabel(arrival);
        const existingDir = bucket.directions.find((d) => d.key === key);
        if (existingDir === undefined) {
          bucket.directions.push({ key, label, arrivals: [arrival] });
        } else {
          existingDir.arrivals.push(arrival);
        }
      }
    }

    for (const line of lineBuckets) {
      line.directions.sort(compareDirections);
    }
    return lineBuckets;
  });

  // Rows-per-direction, tuned for the new (line × direction) grouping.
  // The denominator is `lineCount`, not platform count — a 4-line
  // interchange like Baker Street has up to 5 lines × 2 directions = 10
  // direction columns, and pushing 5 rows into each would force a lot
  // of vertical scroll. Drop rows aggressively for busier stations to
  // reduce scroll, and pad rows for sparse stations so the dot-matrix
  // panel doesn't look anaemic.
  const rowsPerPlatform = $derived.by(() => {
    const lineCount = linesGrouped.length;
    if ($displayMode === 'menubar') {
      if (lineCount <= 1) return 5;
      if (lineCount <= 2) return 4;
      if (lineCount <= 3) return 3;
      return 2;
    }
    if (lineCount <= 1) return 6;
    if (lineCount <= 2) return 6;
    if (lineCount <= 3) return 5;
    return 4;
  });

  // ---------------------------------------------------------------------------
  // Adaptive window resize
  //
  // Picks a (width, height) tier from the line / platform count and the
  // current display mode, then pushes it through `applyBoardSize` only when
  // the tier changes. The renderer-side dedupe is what keeps this off the
  // main-thread Cocoa dispatch on every poll tick (every 30 s the board
  // updates but the line count rarely changes).
  // ---------------------------------------------------------------------------

  // Adaptive size tiers by line count. Width is fixed in menubar (the
  // popover is anchored under the tray and width changes would re-trigger
  // horizontal repositioning); width grows in window mode so multi-line
  // interchanges have room for side-by-side direction columns. Heights
  // are deliberately a bit larger than the worst-case "no scroll" need —
  // the user explicitly preferred a taller popover over internal scroll
  // on busy stations like Baker Street (5 lines).
  function pickBoardSize(
    mode: 'window' | 'menubar',
    lineCount: number,
  ): { width: number; height: number } {
    if (mode === 'menubar') {
      if (lineCount <= 1) return { width: 380, height: 520 };
      if (lineCount <= 2) return { width: 380, height: 620 };
      if (lineCount <= 3) return { width: 380, height: 720 };
      return { width: 380, height: 800 };
    }
    if (lineCount <= 1) return { width: 700, height: 560 };
    if (lineCount <= 2) return { width: 980, height: 680 };
    if (lineCount <= 3) return { width: 1200, height: 760 };
    return { width: 1200, height: 880 };
  }

  let lastSizeKey = $state('');

  $effect(() => {
    const lineCount = linesGrouped.length;
    if (lineCount === 0) return;
    const { width, height } = pickBoardSize($displayMode, lineCount);
    const key = `${$displayMode}|${String(width)}x${String(height)}`;
    if (key === lastSizeKey) return;
    lastSizeKey = key;
    void applyBoardSize(width, height).catch((err: unknown) => {
      // Resize failures are non-fatal (renderer keeps working at the
      // previous size); log so the dev console surfaces a regression
      // without blocking the board render.
      console.warn('[board] applyBoardSize failed', err);
    });
  });

  // ---------------------------------------------------------------------------
  // Window controls (window mode only)
  //
  // The Tauri window is borderless + transparent, so we draw our own
  // close / minimise / fullscreen buttons in the title-bar strip. Each
  // delegates to the Tauri window API. We feature-detect at call time so
  // unit tests (vitest, plain `vite dev`) do not crash trying to import
  // @tauri-apps/api outside the Tauri runtime.
  // ---------------------------------------------------------------------------

  async function withWindow<T>(fn: (win: import('@tauri-apps/api/window').Window) => Promise<T>) {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      return await fn(getCurrentWindow());
    } catch {
      // Not running under Tauri — no-op.
    }
  }

  const handleClose = () => withWindow((w) => w.close());
  const handleMinimize = () => withWindow((w) => w.minimize());
  const handleFullscreen = () =>
    withWindow(async (w) => {
      const isFs = await w.isFullscreen();
      await w.setFullscreen(!isFs);
    });
</script>

<main class="board" aria-label="Arrivals board for {displayName}">
  {#if $displayMode === 'window'}
    <!--
      Window-mode title bar. The Tauri window is borderless + transparent
      so we draw our own close/minimise/fullscreen buttons here and the
      rest of the strip is a drag region. Hidden in menubar mode where
      the popover already has its own rounded chrome.
    -->
    <div class="board__titlebar" data-tauri-drag-region>
      <div class="board__traffic-lights" aria-label="Window controls">
        <button
          type="button"
          class="board__traffic-light board__traffic-light--close"
          aria-label="Close window"
          title="Close"
          onclick={handleClose}
        >
          <svg viewBox="0 0 12 12" aria-hidden="true">
            <path d="M3.5 3.5l5 5M8.5 3.5l-5 5" stroke-width="1.2" stroke-linecap="round" />
          </svg>
        </button>
        <button
          type="button"
          class="board__traffic-light board__traffic-light--min"
          aria-label="Minimise window"
          title="Minimise"
          onclick={handleMinimize}
        >
          <svg viewBox="0 0 12 12" aria-hidden="true">
            <path d="M3 6h6" stroke-width="1.2" stroke-linecap="round" />
          </svg>
        </button>
        <button
          type="button"
          class="board__traffic-light board__traffic-light--full"
          aria-label="Toggle fullscreen"
          title="Fullscreen"
          onclick={handleFullscreen}
        >
          <svg viewBox="0 0 12 12" aria-hidden="true">
            <path d="M3.5 3.5h3v3zM8.5 8.5h-3v-3z" stroke-width="0" fill="currentColor" />
          </svg>
        </button>
      </div>
      <span class="board__titlebar-wordmark led-accent" data-tauri-drag-region>TUBBIE</span>
    </div>
  {/if}

  <!-- Header -->
  <header
    class="board__header"
    class:refresh-pulse={pulsing}
    class:board__header--stale={isStale}
    aria-label="Station header"
    data-tauri-drag-region
  >
    <div class="board__station-block" data-tauri-drag-region>
      <h1 class="board__station-name led-accent" data-tauri-drag-region>
        {displayName}
      </h1>
      {#if filterLabels.length > 0}
        <p
          class="board__line-filter"
          data-testid="board-line-filter"
          role="status"
          aria-label="Filtering by lines: {filterLabels.join(', ')}"
          data-tauri-drag-region
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

      <button
        type="button"
        bind:this={searchToggleEl}
        class="board__settings-btn board__search-btn"
        class:board__search-btn--active={searchOpen}
        onclick={() => {
          searchOpen = !searchOpen;
        }}
        aria-pressed={searchOpen}
        aria-label={searchOpen ? 'Close station search' : 'Change station'}
        title="Change station"
      >
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
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
      </button>

      <button
        type="button"
        class="board__status-btn"
        class:board__status-btn--active={view === 'status'}
        onclick={() => (view = view === 'status' ? 'arrivals' : 'status')}
        aria-pressed={view === 'status'}
        aria-label={view === 'status' ? 'Hide service status' : 'Show service status'}
        title="Service status"
      >
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
          <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" />
        </svg>
        {#if disruptionCount > 0}
          <span class="board__status-badge" aria-hidden="true">{disruptionCount}</span>
        {/if}
      </button>

      <button
        type="button"
        class="board__settings-btn"
        onclick={() => void openSettingsWindow()}
        aria-label="Open settings"
        title="Settings"
      >
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
      </button>
    </div>
  </header>

  {#if searchOpen}
    <!-- Station-search overlay — appears directly under the header. A
         window-level Escape handler (see the $effect above) closes it from
         anywhere, and the input is auto-focused on open. -->
    <div
      class="board__search-overlay"
      bind:this={searchOverlayEl}
      data-testid="board-search-overlay"
    >
      <StationSearch selectedId={$config.station_id} onSelect={handleStationSelect} />
    </div>
  {/if}

  <!-- Arrivals area: stacked by line. Each line group renders one
       direction column per direction, with the per-direction arrivals
       filtered to that line — so a Bakerloo + Jubilee shared platform
       at Baker Street shows up as separate Bakerloo and Jubilee groups
       and the line-coloured stripe always matches the train.

       The arrivals tree stays MOUNTED in Status view and is hidden, not
       unmounted — same "never re-mount the board" rule the rerender-count
       test pins (no cache re-warm, instant toggle-back) — and `aria-hidden`
       + `display:none` keep it out of the accessibility tree while hidden. -->
  <div
    class="board__platforms"
    class:board__platforms--hidden={view === 'status'}
    aria-hidden={view === 'status' ? 'true' : undefined}
    aria-label="Arrivals by line"
    role="region"
  >
    {#each linesGrouped as group (group.lineId)}
      <LineGroup
        lineId={group.lineId}
        lineName={group.lineName}
        directions={group.directions}
        maxRows={rowsPerPlatform}
        groupDestinations={$displayPrefs.group_destinations}
      />
    {/each}

    {#if linesGrouped.length === 0}
      <div class="board__no-platforms" role="status">No arrivals to display</div>
    {/if}
  </div>

  {#if view === 'status'}
    <!-- Full Service-status view (desktop equivalent of the iOS Status tab). -->
    <StatusView
      {statuses}
      partial={statusPartial}
      updatedLabel={statusUpdatedLabel}
      onRefresh={onStatusRefresh}
    />
  {:else}
    <!-- Service status summary — worst-first, calm states (replaces the marquee). -->
    <StatusPanel {statuses} partial={statusPartial} />
  {/if}
</main>

<style>
  .board {
    display: flex;
    flex-direction: column;
    height: calc(100vh - 24px); /* 24px for Attribution footer */
    background: var(--bg);
    overflow: hidden;
    position: relative;
  }

  /* Title bar (window mode only).
     Borderless transparent window — we draw our own chrome here so the
     rounded corners + shadow from .popover-root.mode-window give a clean
     LED-themed window with no native macOS title bar above. The whole
     strip is a drag region except for the traffic-light buttons. */
  .board__titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 32px;
    padding: 0 1rem;
    background: var(--bg);
    border-bottom: 1px solid var(--row-divider);
    flex-shrink: 0;
    -webkit-user-select: none;
    user-select: none;
  }

  .board__titlebar-wordmark {
    font-family: var(--font-board);
    font-size: 0.7rem;
    letter-spacing: 0.25em;
    color: var(--accent);
    opacity: 0.55;
  }

  /* Traffic-light style buttons — match the macOS look (red/yellow/green
     12px circles, glyph appears on group hover) but rendered in HTML so
     they sit inside our LED-dark title bar instead of floating in a
     separate native chrome strip. */
  .board__traffic-lights {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .board__traffic-light {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 0.5px solid rgba(0, 0, 0, 0.25);
    padding: 0;
    margin: 0;
    cursor: default;
    display: flex;
    align-items: center;
    justify-content: center;
    color: rgba(0, 0, 0, 0.55);
  }

  .board__traffic-light svg {
    width: 100%;
    height: 100%;
    stroke: currentColor;
    fill: none;
    opacity: 0;
    transition: opacity 0.1s ease;
  }

  .board__traffic-lights:hover .board__traffic-light svg,
  .board__traffic-light:focus-visible svg {
    opacity: 1;
  }

  .board__traffic-light:focus {
    outline: none;
  }

  .board__traffic-light--close {
    background: #ff5f57;
  }

  .board__traffic-light--min {
    background: #febc2e;
  }

  .board__traffic-light--full {
    background: #28c840;
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
    background: transparent;
    cursor: pointer;
    width: 32px;
    height: 32px;
    border: 1px solid var(--row-divider);
    border-radius: 2px;
    transition: color 0.15s ease;
    opacity: 0.7;
    padding: 0;
  }

  .board__settings-btn:hover,
  .board__settings-btn:focus {
    color: var(--fg);
    opacity: 1;
    border-color: var(--platform-label);
  }

  /* Station-search toggle — same chrome as the cog/status buttons. */
  .board__search-btn--active {
    color: var(--fg);
    opacity: 1;
    border-color: var(--fg);
    background: color-mix(in srgb, var(--fg) 10%, transparent);
  }

  /* Search overlay — absolutely positioned directly under the header, above
     the board content. Uses dot-matrix CSS vars for visual consistency. */
  .board__search-overlay {
    position: absolute;
    top: 52px; /* header min-height */
    left: 0;
    right: 0;
    z-index: 50;
    background: var(--settings-bg, var(--bg));
    border-bottom: 1px solid var(--input-border);
    padding: 0.75rem 1rem;
  }

  /* Service-status toggle — same chrome as the cog so the pair reads as a
     control cluster. `--active` gives the pressed state a filled look. */
  .board__status-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--platform-label);
    background: transparent;
    cursor: pointer;
    width: 32px;
    height: 32px;
    border: 1px solid var(--row-divider);
    border-radius: 2px;
    transition: color 0.15s ease;
    opacity: 0.7;
    padding: 0;
  }

  .board__status-btn:hover,
  .board__status-btn:focus {
    color: var(--fg);
    opacity: 1;
    border-color: var(--platform-label);
  }

  .board__status-btn--active {
    color: var(--fg);
    opacity: 1;
    border-color: var(--fg);
    background: color-mix(in srgb, var(--fg) 10%, transparent);
  }

  .board__status-badge {
    position: absolute;
    top: -5px;
    right: -5px;
    min-width: 15px;
    height: 15px;
    padding: 0 3px;
    border-radius: 999px;
    background: var(--stale-accent);
    color: var(--bg);
    font-family: var(--font-ui);
    font-size: 0.65rem;
    font-weight: 700;
    line-height: 15px;
    text-align: center;
  }

  /* Platforms area: line groups stack vertically. Each LineGroup owns its
     own responsive grid of platform columns (auto-fit, minmax(180px,
     1fr)), so the only direction we ever need to scroll here is vertical
     when a station has more groups than the window can show at once. */
  .board__platforms {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
    overflow-y: auto;
    background: var(--row-divider);
    padding: 1px;
  }

  /* Status view: hide the (still-mounted) arrivals tree. `display: none`
     overrides the flex above and removes it from the a11y tree. */
  .board__platforms--hidden {
    display: none;
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
</style>
