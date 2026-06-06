<script lang="ts">
  /**
   * Full service-status view (the desktop equivalent of the iOS Status tab),
   * shown in the board body when the header Status toggle is active. Richer
   * than the bottom StatusPanel summary: a headline count, disrupted lines
   * worst-first, an "Other lines — good service" section listing the healthy
   * lines, a freshness line, and a manual refresh.
   */
  import type { LineStatus } from '$lib/ipc/types.js';
  import {
    disruptedLinesWorstFirst,
    isDisrupted,
    lineStatusLabel,
    worstBucket,
  } from '$lib/utils/status.js';
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
  const healthy = $derived(
    statuses
      .filter((s) => !isDisrupted(s))
      .slice()
      .sort((a, b) => prettyLineName(a.line_id).localeCompare(prettyLineName(b.line_id))),
  );
  const countLabel = $derived(
    disrupted.length === 0
      ? 'All lines good'
      : `${String(disrupted.length)} disruption${disrupted.length === 1 ? '' : 's'}`,
  );
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
        <li class="statusview__row" data-bucket={worstBucket(line)}>
          <span
            class="statusview__chip"
            style:background={lineColorVar(line.line_id)}
            aria-hidden="true"
          ></span>
          <span class="statusview__line">{prettyLineName(line.line_id)}</span>
          <span class="statusview__detail">{lineStatusLabel(line)}</span>
        </li>
      {/each}
    </ul>
  {/if}

  {#if healthy.length > 0}
    <div class="statusview__healthy">
      <p class="statusview__healthy-label">
        {disrupted.length > 0 ? 'Other lines — good service' : 'Good service'}
      </p>
      <ul class="statusview__chips">
        {#each healthy as line (line.line_id)}
          <li class="statusview__chip-row">
            <span
              class="statusview__chip"
              style:background={lineColorVar(line.line_id)}
              aria-hidden="true"
            ></span>
            <span class="statusview__chip-name">{prettyLineName(line.line_id)}</span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if statuses.length === 0}
    <p class="statusview__empty">No lines at this station yet.</p>
  {/if}

  <footer class="statusview__foot">
    {#if partial}
      <span class="statusview__partial">Some lines couldn’t be checked.</span>
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

  .statusview__list {
    list-style: none;
    margin: 0 0 0.8rem;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }
  .statusview__row {
    display: grid;
    grid-template-columns: auto auto 1fr;
    align-items: baseline;
    gap: 0.6rem;
  }
  .statusview__chip {
    width: 0.7rem;
    height: 0.7rem;
    border-radius: 2px;
    align-self: center;
    flex-shrink: 0;
  }
  .statusview__line {
    font-weight: 600;
    white-space: nowrap;
  }
  .statusview__detail {
    color: var(--platform-label);
    font-size: 0.9rem;
  }

  .statusview__healthy-label {
    margin: 0 0 0.4rem;
    font-size: 0.75rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--platform-label);
    opacity: 0.8;
  }
  .statusview__chips {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 0.8rem;
  }
  .statusview__chip-row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.85rem;
    color: var(--fg);
  }

  .statusview__empty {
    color: var(--platform-label);
    font-size: 0.9rem;
  }

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
