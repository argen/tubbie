<script lang="ts">
  import type { LineStatus } from '$lib/ipc/types.js';
  import {
    disruptedLinesWorstFirst,
    allGoodService,
    bucketLabel,
    worstBucket,
  } from '$lib/utils/status.js';
  import { prettyLineName, lineColorVar } from '$lib/utils/format.js';
  import { reducedMotion } from '$lib/stores/reducedMotion.js';

  interface Props {
    statuses: LineStatus[];
    /** True when one or more lines' status could not be fetched this cycle. */
    partial?: boolean;
  }

  const { statuses, partial = false }: Props = $props();

  const disrupted = $derived(disruptedLinesWorstFirst(statuses));
  const allGood = $derived(allGoodService(statuses));

  // Marquee pace: a CONSTANT px/second, so the scroll feels identical whether
  // one line or the whole network is disrupted — and stays stable even if the
  // item text changes length. The animation translates exactly one copy-width
  // per cycle, so duration = copyWidth / pxPerSecond. `copyWidth` is measured
  // live (bind:clientWidth); it's 0 before layout / in happy-dom, where the
  // floor keeps the value sane.
  const MARQUEE_PX_PER_SEC = 32;
  const MIN_MARQUEE_SECONDS = 14;
  let copyWidth = $state(0);
  const marqueeDurationS = $derived(
    Math.max(MIN_MARQUEE_SECONDS, Math.round(copyWidth / MARQUEE_PX_PER_SEC)),
  );

  // The reduced-motion store drives whether we animate or render a static list.
  let rm = $state(false);
  const unsubscribe = reducedMotion.subscribe((v) => {
    rm = v;
  });
  $effect(() => {
    return () => {
      unsubscribe();
    };
  });
</script>

<section
  class="status"
  class:status--good={allGood}
  aria-label="Service status"
  role="status"
  aria-live="polite"
>
  {#if allGood}
    <!-- Calm, static: no disruptions. -->
    <p class="status__good">
      <span class="status__good-dot" aria-hidden="true"></span>
      Good service across the network
    </p>
  {:else if rm}
    <!-- Reduced motion: static worst-first list, no animation. -->
    <ul class="status__static-list" data-testid="status-static-list">
      {#each disrupted as line (line.line_id)}
        <li class="status__static-row" data-testid="marquee-line">
          <span
            class="status__stripe"
            style:background={lineColorVar(line.line_id)}
            aria-hidden="true"
          ></span>
          <span class="status__line">{prettyLineName(line.line_id)}</span>
          <span class="status__detail">{bucketLabel(worstBucket(line))}</span>
        </li>
      {/each}
      <li class="status__static-good">Good service on all other lines</li>
    </ul>
  {:else}
    <!-- Full marquee: horizontally-scrolling, loops seamlessly. -->
    <div class="status__marquee-wrap" aria-hidden="true">
      <!-- Two copies so the second one is always ready to loop in -->
      {#each [0, 1] as _copy (_copy)}
        <span
          class="status__marquee"
          style:animation-duration={`${String(marqueeDurationS)}s`}
          bind:clientWidth={copyWidth}
          data-testid={_copy === 0 ? 'status-marquee' : undefined}
          aria-hidden="true"
        >
          {#each disrupted as line (line.line_id + String(_copy))}
            <span class="status__marquee-item" data-testid="marquee-line">
              <span
                class="status__marquee-stripe"
                style:background={lineColorVar(line.line_id)}
                aria-hidden="true"
              ></span>
              <span class="status__marquee-name">{prettyLineName(line.line_id)}</span>
              <span class="status__marquee-label" data-bucket={worstBucket(line)}>
                {bucketLabel(worstBucket(line))}
              </span>
              <span class="status__marquee-sep" aria-hidden="true">·</span>
            </span>
          {/each}
          <span class="status__marquee-good">Good service on all other lines</span>
          <span class="status__marquee-sep" aria-hidden="true">·</span>
        </span>
      {/each}
    </div>
    <!-- Screen-reader accessible text, hidden visually. -->
    <p class="status__sr-only">
      {disrupted
        .map((l) => `${prettyLineName(l.line_id)}: ${bucketLabel(worstBucket(l))}`)
        .join('. ')}. Good service on all other lines.
    </p>
  {/if}

  {#if partial}
    <p class="status__partial">Some lines couldn't be checked — arrivals are still live.</p>
  {/if}
</section>

<style>
  .status {
    flex-shrink: 0;
    background: var(--ticker-bg);
    border-top: 1px solid var(--row-divider);
    padding: 0.4rem 0.6rem;
    font-family: var(--font-board);
    overflow: hidden;
  }

  .status--good {
    padding: 0.5rem 0.6rem;
  }

  /* ── All-good static state ─────────────────────────────────────────────── */

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

  /* ── Reduced-motion static list ────────────────────────────────────────── */

  .status__static-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .status__static-row {
    display: grid;
    grid-template-columns: auto auto 1fr;
    align-items: baseline;
    gap: 0.5rem;
    font-size: 0.9rem;
    color: var(--ticker-fg);
  }

  .status__stripe {
    width: 0.3rem;
    height: 1.1em;
    border-radius: 2px;
    align-self: center;
    flex-shrink: 0;
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

  .status__static-good {
    font-size: 0.85rem;
    color: var(--platform-label);
    padding-left: 0.8rem;
  }

  /* ── Marquee ────────────────────────────────────────────────────────────── */

  .status__marquee-wrap {
    display: flex;
    overflow: hidden;
    white-space: nowrap;
    width: 100%;
  }

  .status__marquee {
    display: inline-flex;
    align-items: center;
    gap: 0;
    /* Duration is set inline (scaled to content); this is just a fallback. */
    animation: marquee-scroll 36s linear infinite;
    flex-shrink: 0;
  }

  @keyframes marquee-scroll {
    0% {
      transform: translateX(0);
    }
    100% {
      transform: translateX(-100%);
    }
  }

  /* Disable animation when the user prefers reduced motion. */
  @media (prefers-reduced-motion: reduce) {
    .status__marquee {
      animation: none;
    }
  }

  .status__marquee-item {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.9rem;
    color: var(--ticker-fg);
  }

  .status__marquee-stripe {
    width: 0.3rem;
    height: 1em;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .status__marquee-name {
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .status__marquee-label {
    color: var(--platform-label);
  }

  .status__marquee-sep {
    margin: 0 0.6rem;
    color: var(--platform-label);
    opacity: 0.5;
  }

  .status__marquee-good {
    font-size: 0.85rem;
    color: var(--platform-label);
    font-style: italic;
  }

  /* Screen-reader only (for the marquee path). */
  .status__sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  /* ── Partial note ───────────────────────────────────────────────────────── */

  .status__partial {
    margin: 0.4rem 0 0;
    font-size: 0.75rem;
    color: var(--platform-label);
    opacity: 0.7;
  }
</style>
