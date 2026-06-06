<script lang="ts">
  import type { LineStatus } from '$lib/ipc/types.js';
  import {
    disruptedLinesWorstFirst,
    allGoodService,
    worstBucket,
    lineStatusLabel,
  } from '$lib/utils/status.js';
  import { prettyLineName } from '$lib/utils/format.js';

  interface Props {
    statuses: LineStatus[];
    /** True when one or more lines' status could not be fetched this cycle. */
    partial?: boolean;
  }

  const { statuses, partial = false }: Props = $props();

  // Worst-first, driven by the canonical SeverityBucket (invariant #25) — no
  // marquee, no motion: a calm, scannable summary. Color severity dots are
  // fine HERE (the board UI); the monochrome constraint is the menu bar only.
  const disrupted = $derived(disruptedLinesWorstFirst(statuses));
  const allGood = $derived(allGoodService(statuses));
</script>

<section
  class="status"
  class:status--good={allGood}
  aria-label="Service status"
  role="status"
  aria-live="polite"
>
  {#if allGood}
    <p class="status__good">
      <span class="status__good-dot" aria-hidden="true"></span>
      Good service on all your lines
    </p>
  {:else}
    <ul class="status__list">
      {#each disrupted as line (line.line_id)}
        <li class="status__row" data-bucket={worstBucket(line)}>
          <span class="status__dot" data-bucket={worstBucket(line)} aria-hidden="true"></span>
          <span class="status__line">{prettyLineName(line.line_id)}</span>
          <span class="status__detail">{lineStatusLabel(line)}</span>
        </li>
      {/each}
    </ul>
  {/if}

  {#if partial}
    <p class="status__partial">Some lines couldn’t be checked — arrivals are still live.</p>
  {/if}
</section>

<style>
  .status {
    flex-shrink: 0;
    background: var(--ticker-bg);
    border-top: 1px solid var(--row-divider);
    padding: 0.4rem 0.6rem;
    max-height: 30vh;
    overflow-y: auto;
    font-family: var(--font-board);
  }

  .status--good {
    /* calm confirmation: a single quiet line, not a pane of emptiness */
    padding: 0.5rem 0.6rem;
  }

  .status__good {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.9rem;
    color: var(--platform-label);
    letter-spacing: 0.04em;
  }

  .status__good-dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--good-service, #2e9e5b);
    flex-shrink: 0;
  }

  .status__list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .status__row {
    display: grid;
    grid-template-columns: auto auto 1fr;
    align-items: baseline;
    gap: 0.5rem;
    font-size: 0.9rem;
    color: var(--ticker-fg);
  }

  .status__dot {
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
    align-self: center;
    flex-shrink: 0;
    background: var(--platform-label);
  }
  /* Severity tiers — worst tiers warmer/redder. Falls back to --stale-accent. */
  .status__dot[data-bucket='Closed'],
  .status__dot[data-bucket='PartClosure'] {
    background: var(--severity-closed, #d23b3b);
  }
  .status__dot[data-bucket='SevereDelays'] {
    background: var(--severity-severe, #e2632a);
  }
  .status__dot[data-bucket='ReducedService'],
  .status__dot[data-bucket='MinorDelays'] {
    background: var(--severity-minor, #e0a800);
  }
  .status__dot[data-bucket='Information'],
  .status__dot[data-bucket='Other'] {
    background: var(--platform-label);
  }

  .status__line {
    font-weight: 600;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }

  .status__detail {
    color: var(--platform-label);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status__partial {
    margin: 0.4rem 0 0;
    font-size: 0.75rem;
    color: var(--platform-label);
    opacity: 0.7;
  }
</style>
