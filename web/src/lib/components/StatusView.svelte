<script lang="ts">
  /**
   * Full service-status view (the desktop equivalent of the iOS Status tab),
   * shown in the board body when the header Status toggle is active.
   *
   * Layout mirrors the TfL website Status page:
   *   - Left vertical colour stripe per line
   *   - Bold line name
   *   - Per StatusEntry: severity sub-headline + "A ↔ B" segment rows
   *     (or "Entire line" when no segments)
   *   - Disclosure chevron → expands disruption_text prose
   *   - Single "Good service on all other lines" footer bar
   *   - Empty state "Service status unavailable."
   */
  import type { LineStatus } from '$lib/ipc/types.js';
  import { disruptedLinesWorstFirst, isDisrupted, segmentsFor } from '$lib/utils/status.js';
  import { prettyLineName, lineColorVar } from '$lib/utils/format.js';

  interface Props {
    statuses: LineStatus[];
    partial?: boolean;
    /** Pre-formatted freshness label (e.g. "3 min ago"); "" hides the line. */
    updatedLabel?: string;
    /** Manual refresh; omitted → no refresh button. */
    onRefresh?: (() => void) | undefined;
  }

  const { statuses, partial = false, updatedLabel = '', onRefresh }: Props = $props();

  const disrupted = $derived(disruptedLinesWorstFirst(statuses));
  const hasHealthy = $derived(statuses.some((s) => !isDisrupted(s)));
  const countLabel = $derived(
    disrupted.length === 0
      ? 'All lines good'
      : `${String(disrupted.length)} disruption${disrupted.length === 1 ? '' : 's'}`,
  );

  // Per-line expanded state for disclosure chevrons.
  let expanded = $state<Record<string, boolean>>({});

  function toggleLine(lineId: string): void {
    expanded = { ...expanded, [lineId]: !expanded[lineId] };
  }

  function detailsId(lineId: string): string {
    return `statusview-details-${lineId}`;
  }
</script>

<section class="statusview" aria-label="Service status">
  <header class="statusview__head">
    <h2 class="statusview__title">Service status</h2>
    <span class="statusview__count" class:statusview__count--bad={disrupted.length > 0}>
      {countLabel}
    </span>
    {#if onRefresh}
      <button type="button" class="statusview__refresh" onclick={onRefresh}>Refresh</button>
    {/if}
  </header>

  {#if disrupted.length > 0}
    <ul class="statusview__list">
      {#each disrupted as line (line.line_id)}
        {@const isExpanded = expanded[line.line_id] ?? false}
        <li class="statusview__row">
          <!-- Left colour stripe -->
          <span
            class="statusview__stripe"
            style:background={lineColorVar(line.line_id)}
            aria-hidden="true"
          ></span>

          <div class="statusview__body">
            <!-- Line name + disclosure toggle -->
            <div class="statusview__namerow">
              <span class="statusview__line">{prettyLineName(line.line_id)}</span>
              {#if line.disruption_text}
                <button
                  type="button"
                  class="statusview__toggle"
                  data-testid="details-toggle"
                  aria-expanded={isExpanded}
                  aria-controls={detailsId(line.line_id)}
                  onclick={() => {
                    toggleLine(line.line_id);
                  }}
                >
                  <span
                    class="statusview__chevron"
                    class:statusview__chevron--open={isExpanded}
                    aria-hidden="true">›</span
                  >
                </button>
              {/if}
            </div>

            <!-- Per-entry severity sub-headlines + segments -->
            {#each line.status as entry (`${entry.description}-${String(entry.severity)}`)}
              {@const segs = segmentsFor(entry)}
              <div class="statusview__entry">
                <p class="statusview__entry-headline">{entry.description}</p>
                {#if segs.length > 0}
                  <ul class="statusview__segments">
                    {#each segs as seg (`${seg.from}→${seg.to}`)}
                      <li class="statusview__segment" data-testid="route-segment">
                        <span class="statusview__segment-from">{seg.from}</span>
                        <span class="statusview__segment-arrow" aria-hidden="true"> ↔ </span>
                        <span class="statusview__segment-to">{seg.to}</span>
                      </li>
                    {/each}
                  </ul>
                {:else}
                  <p class="statusview__segment-entire">Entire line</p>
                {/if}
              </div>
            {/each}

            <!-- Disclosure panel (disruption prose) — only mounted when open -->
            {#if line.disruption_text && isExpanded}
              <div
                id={detailsId(line.line_id)}
                class="statusview__details statusview__details--open"
              >
                <p class="statusview__disruption">{line.disruption_text}</p>
              </div>
            {:else if line.disruption_text}
              <!-- Placeholder to keep the aria-controls id in the DOM. -->
              <div id={detailsId(line.line_id)} class="statusview__details" hidden></div>
            {/if}
          </div>
        </li>
      {/each}
    </ul>

    <!-- Footer bar replaces chip enumeration of healthy lines -->
    {#if hasHealthy}
      <p class="statusview__good-footer">Good service on all other lines</p>
    {/if}
  {/if}

  {#if statuses.length === 0}
    <p class="statusview__empty">Service status unavailable.</p>
  {/if}

  <footer class="statusview__foot">
    {#if partial}
      <span class="statusview__partial">Some lines couldn't be checked.</span>
    {/if}
    {#if updatedLabel}
      <span class="statusview__updated" aria-live="polite">Updated {updatedLabel}</span>
    {/if}
  </footer>
</section>

<style>
  .statusview {
    flex: 1;
    overflow-y: auto;
    padding: 0.8rem 1rem 1rem;
    background: var(--bg);
    font-family: var(--font-board);
    color: var(--fg);
  }

  .statusview__head {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    margin-bottom: 0.6rem;
  }
  .statusview__title {
    margin: 0;
    font-size: 0.9rem;
    letter-spacing: 0.1em;
    color: var(--platform-label);
    text-transform: uppercase;
  }
  .statusview__count {
    font-size: 0.85rem;
    color: var(--platform-label);
  }
  .statusview__count--bad {
    color: var(--stale-accent);
    font-weight: 600;
  }
  .statusview__refresh {
    margin-left: auto;
    background: none;
    border: 1px solid var(--input-border);
    color: var(--fg);
    border-radius: 2px;
    padding: 0.15rem 0.6rem;
    font-family: var(--font-ui);
    font-size: 0.8rem;
    cursor: pointer;
  }
  .statusview__refresh:hover,
  .statusview__refresh:focus-visible {
    border-color: var(--fg);
  }

  /* ── Disrupted rows ─────────────────────────────────────────────────────── */

  .statusview__list {
    list-style: none;
    margin: 0 0 0.8rem;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .statusview__row {
    display: flex;
    align-items: stretch;
    border-bottom: 1px solid var(--row-divider);
  }
  .statusview__row:first-child {
    border-top: 1px solid var(--row-divider);
  }

  /* Left vertical colour stripe */
  .statusview__stripe {
    width: 0.35rem;
    flex-shrink: 0;
    border-radius: 2px 0 0 2px;
  }

  .statusview__body {
    flex: 1;
    padding: 0.6rem 0.6rem 0.6rem 0.7rem;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .statusview__namerow {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .statusview__line {
    font-weight: 700;
    font-size: 1rem;
    letter-spacing: 0.02em;
    flex: 1;
  }

  /* ── Disclosure toggle ──────────────────────────────────────────────────── */

  .statusview__toggle {
    background: none;
    border: none;
    padding: 0.1rem 0.3rem;
    cursor: pointer;
    color: var(--platform-label);
    font-size: 1rem;
    display: flex;
    align-items: center;
  }
  .statusview__toggle:hover,
  .statusview__toggle:focus-visible {
    color: var(--fg);
  }

  .statusview__chevron {
    display: inline-block;
    transition: transform 0.2s ease;
    transform: rotate(0deg);
    line-height: 1;
  }
  .statusview__chevron--open {
    transform: rotate(90deg);
  }

  @media (prefers-reduced-motion: reduce) {
    .statusview__chevron {
      transition: none;
    }
  }

  /* ── Per-entry severity block ───────────────────────────────────────────── */

  .statusview__entry {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding-top: 0.1rem;
  }

  .statusview__entry-headline {
    margin: 0;
    font-weight: 600;
    font-size: 0.9rem;
    color: var(--fg);
  }

  /* ── Segments ───────────────────────────────────────────────────────────── */

  .statusview__segments {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .statusview__segment {
    font-size: 0.82rem;
    color: var(--platform-label);
    display: flex;
    align-items: baseline;
    gap: 0.15rem;
  }

  .statusview__segment-arrow {
    opacity: 0.6;
  }

  .statusview__segment-entire {
    margin: 0;
    font-size: 0.82rem;
    color: var(--platform-label);
    font-style: italic;
  }

  /* ── Disclosure details panel ───────────────────────────────────────────── */

  .statusview__details {
    overflow: hidden;
  }
  .statusview__details--open {
    display: block;
  }

  .statusview__disruption {
    margin: 0.2rem 0 0;
    font-size: 0.82rem;
    color: var(--platform-label);
    line-height: 1.4;
  }

  /* ── Footer bar ─────────────────────────────────────────────────────────── */

  .statusview__good-footer {
    margin: 0.6rem 0 0;
    padding: 0.5rem 0.8rem;
    font-weight: 700;
    font-size: 0.9rem;
    background: var(--good-service-bg, rgba(46, 158, 91, 0.1));
    color: var(--good-service, #2e9e5b);
    border-radius: 4px;
  }

  /* ── Empty state ────────────────────────────────────────────────────────── */

  .statusview__empty {
    color: var(--platform-label);
    font-size: 0.9rem;
  }

  /* ── Footer ─────────────────────────────────────────────────────────────── */

  .statusview__foot {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    margin-top: 0.8rem;
    font-size: 0.75rem;
    color: var(--platform-label);
    opacity: 0.75;
  }
</style>
