<script lang="ts">
  import { reducedMotion } from '$lib/stores/reducedMotion.js';
  import type { LineStatus } from '$lib/ipc/types.js';
  import { prettyLineName } from '$lib/utils/format.js';

  interface Props {
    statuses: LineStatus[];
  }

  const { statuses }: Props = $props();

  /**
   * Build the ticker text as one segment per line in scope. Lines with a
   * disruption contribute `"{Line}: {disruption}"`; good-service lines
   * contribute `"{Line}: Good service"`, so the user can see at a glance
   * which of their lines are healthy alongside any that aren't.
   */
  const tickerText = $derived((): string => {
    if (statuses.length === 0) {
      return 'Good service';
    }
    return statuses
      .map((s) => {
        const name = prettyLineName(s.line_id);
        const disruption = s.disruption_text ?? '';
        return disruption.length > 0 ? `${name}: ${disruption}` : `${name}: Good service`;
      })
      .join('  •  ');
  });

  const hasDisruptions = $derived(
    statuses.some((s) => s.disruption_text !== null && s.disruption_text.length > 0),
  );

  let paused = $state(false);
  let tickerEl: HTMLDivElement | undefined = $state();
  let innerEl: HTMLSpanElement | undefined = $state();

  // Approx 60px/s — calculate animation duration from inner width
  let animDuration = $state(20);

  function updateDuration(): void {
    if (innerEl) {
      const textWidth = innerEl.scrollWidth;
      animDuration = Math.max(textWidth / 60, 5);
    }
  }

  $effect(() => {
    // Re-calculate when ticker text changes (access the derived to track dependency)
    tickerText();
    // Small delay to allow DOM measurement after reactive update
    setTimeout(updateDuration, 50);
  });
</script>

<div
  class="ticker"
  class:ticker--disrupted={hasDisruptions}
  aria-label="Service status"
  role="status"
  aria-live="polite"
>
  <span class="ticker__label" aria-hidden="true">
    {hasDisruptions ? 'DISRUPTIONS' : 'SERVICE'}
  </span>

  <div
    class="ticker__track"
    role="presentation"
    bind:this={tickerEl}
    onmouseenter={() => {
      paused = true;
    }}
    onmouseleave={() => {
      paused = false;
    }}
  >
    {#if hasDisruptions && !$reducedMotion}
      <!-- Scrolling marquee for disruptions -->
      <span
        class="ticker__inner ticker__inner--scroll"
        bind:this={innerEl}
        style:animation-duration="{animDuration}s"
        style:animation-play-state={paused ? 'paused' : 'running'}
        aria-hidden="true"
      >
        {tickerText()}
        &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
        {tickerText()}
      </span>
      <!-- Accessible version for screen readers -->
      <span class="sr-only">{tickerText()}</span>
    {:else}
      <!-- Static text: good service OR reduced-motion fallback -->
      <span class="ticker__inner" bind:this={innerEl}>
        {tickerText()}
      </span>
    {/if}
  </div>
</div>

<style>
  .ticker {
    display: flex;
    align-items: center;
    height: 32px;
    background: var(--ticker-bg);
    border-top: 1px solid var(--row-divider);
    overflow: hidden;
    flex-shrink: 0;
  }

  .ticker--disrupted {
    border-top-color: var(--stale-accent);
  }

  .ticker__label {
    font-family: var(--font-board);
    font-size: 0.7rem;
    color: var(--platform-label);
    letter-spacing: 0.1em;
    padding: 0 0.6rem;
    white-space: nowrap;
    border-right: 1px solid var(--row-divider);
    height: 100%;
    display: flex;
    align-items: center;
    flex-shrink: 0;
    opacity: 0.7;
  }

  .ticker__track {
    flex: 1;
    overflow: hidden;
    position: relative;
    height: 100%;
    display: flex;
    align-items: center;
  }

  .ticker__inner {
    font-family: var(--font-board);
    font-size: 1rem;
    color: var(--ticker-fg);
    white-space: nowrap;
    padding: 0 0.8rem;
    letter-spacing: 0.04em;
  }

  .ticker__inner--scroll {
    display: inline-block;
    padding: 0;
    animation: ticker-scroll linear infinite;
    will-change: transform;
  }

  @keyframes ticker-scroll {
    0% {
      transform: translateX(0);
    }
    100% {
      transform: translateX(-50%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .ticker__inner--scroll {
      animation: none;
    }
  }
</style>
