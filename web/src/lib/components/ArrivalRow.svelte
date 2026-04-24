<script lang="ts">
  import { onMount } from 'svelte';
  import { fly } from 'svelte/transition';
  import type { Arrival } from '$lib/ipc/types.js';
  import { formatTimeToStation, isDue, revealDuration } from '$lib/utils/format.js';
  import { reducedMotion } from '$lib/stores/reducedMotion.js';

  interface Props {
    arrival: Arrival;
    rank: number; // 1-based position in the list
  }

  const { arrival, rank }: Props = $props();

  // ---------------------------------------------------------------------------
  // Char-by-char reveal
  // ---------------------------------------------------------------------------

  let revealedDest = $state('');
  let revealComplete = $state(false);

  function startReveal(text: string, skipAnimation: boolean): void {
    if (skipAnimation || text.length === 0) {
      revealedDest = text;
      revealComplete = true;
      return;
    }

    const totalMs = revealDuration(text);
    const msPerChar = totalMs / text.length;
    let i = 0;

    const tick = (): void => {
      i += 1;
      revealedDest = text.slice(0, i);
      if (i < text.length) {
        setTimeout(tick, msPerChar);
      } else {
        revealComplete = true;
      }
    };

    setTimeout(tick, msPerChar);
  }

  onMount(() => {
    startReveal(arrival.destination_name, $reducedMotion);
  });

  // ---------------------------------------------------------------------------
  // Marquee: if the destination text overflows its column after reveal,
  // scroll it horizontally. Track overflow in px and duration so slow
  // overflows don't race past the eye.
  // ---------------------------------------------------------------------------

  let destEl: HTMLSpanElement | undefined = $state();
  let overflowPx = $state(0);

  // Characters per second for the marquee scroll. ~12 c/s matches a comfortable
  // read speed on the TfL boards.
  const MARQUEE_CHARS_PER_SEC = 12;

  function measureOverflow(): void {
    if (!destEl) return;
    const overflow = destEl.scrollWidth - destEl.clientWidth;
    overflowPx = overflow > 1 ? overflow : 0;
  }

  $effect(() => {
    if (!revealComplete || $reducedMotion) {
      overflowPx = 0;
      return;
    }
    // Let layout settle before measuring.
    const raf = requestAnimationFrame(measureOverflow);
    const onResize = (): void => {
      measureOverflow();
    };
    window.addEventListener('resize', onResize);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener('resize', onResize);
    };
  });

  const marqueeDuration = $derived(
    overflowPx > 0 ? Math.max(arrival.destination_name.length / MARQUEE_CHARS_PER_SEC, 4) : 0,
  );

  // ---------------------------------------------------------------------------
  // Time formatting + due state
  // ---------------------------------------------------------------------------

  const formattedTime = $derived(formatTimeToStation(arrival.time_to_station));
  const due = $derived(isDue(arrival.time_to_station));
</script>

<li
  class="arrival-row"
  aria-label="Train {rank}: {arrival.destination_name}, {formattedTime}"
  in:fly|global={{ y: -20, duration: $reducedMotion ? 0 : 250 }}
  out:fly|global={{ y: 20, duration: $reducedMotion ? 0 : 200 }}
>
  <span class="arrival-row__rank" aria-hidden="true">{rank}</span>

  <span
    class="arrival-row__dest led-text"
    class:arrival-row__dest--marquee={overflowPx > 0}
    aria-label="Destination: {arrival.destination_name}"
    bind:this={destEl}
  >
    <span
      class="arrival-row__dest-track"
      style:--marquee-shift="-{overflowPx}px"
      style:--marquee-duration="{marqueeDuration}s"
    >
      {revealComplete ? arrival.destination_name : revealedDest}
      {#if !revealComplete}<span class="arrival-row__cursor" aria-hidden="true">_</span>{/if}
    </span>
  </span>

  <span class="arrival-row__via" aria-label="Towards: {arrival.towards}">
    {arrival.towards}
  </span>

  <span
    class="arrival-row__time"
    class:due-pulse={due}
    class:led-accent={due}
    aria-label="Time: {formattedTime}"
  >
    {formattedTime}
  </span>
</li>

<style>
  .arrival-row {
    display: grid;
    grid-template-columns: 1.2rem 1fr auto auto;
    column-gap: 0.5rem;
    align-items: center;
    padding: 0.3rem 0.5rem;
    border-bottom: 1px solid var(--row-divider);
    list-style: none;
    font-family: var(--font-board);
    font-size: 1.1rem;
    line-height: 1.3;
    min-height: 2rem;
  }

  .arrival-row:last-child {
    border-bottom: none;
  }

  .arrival-row__rank {
    color: var(--platform-label);
    font-size: 0.85rem;
    opacity: 0.6;
    user-select: none;
  }

  .arrival-row__dest {
    color: var(--fg);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: 0.03em;
    min-width: 0; /* allow the 1fr grid column to actually shrink below content */
  }

  /* When the marquee kicks in we don't want the ellipsis to appear alongside
     the scrolling text. */
  .arrival-row__dest--marquee {
    text-overflow: clip;
  }

  .arrival-row__dest-track {
    display: inline-block;
    white-space: nowrap;
    /* No transition on non-marquee rows — keep reveal crisp. */
  }

  /* When destination overflows the column, scroll horizontally. The overflow
     distance and duration are set as CSS custom properties from the script. */
  .arrival-row__dest--marquee .arrival-row__dest-track {
    animation: dest-marquee var(--marquee-duration, 8s) ease-in-out infinite alternate;
    will-change: transform;
  }

  @keyframes dest-marquee {
    0%,
    15% {
      transform: translateX(0);
    }
    85%,
    100% {
      transform: translateX(var(--marquee-shift, 0));
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .arrival-row__dest--marquee .arrival-row__dest-track {
      animation: none;
    }
  }

  .arrival-row__via {
    color: var(--platform-label);
    font-size: 0.85rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0 0.4rem;
    opacity: 0.8;
  }

  .arrival-row__time {
    color: var(--fg);
    font-size: 1rem;
    font-weight: normal;
    white-space: nowrap;
    letter-spacing: 0.05em;
    min-width: 4.5rem;
    text-align: right;
  }

  .arrival-row__cursor {
    display: inline-block;
    animation: blink-cursor 0.7s step-end infinite;
    color: var(--accent);
  }

  @keyframes blink-cursor {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .arrival-row__cursor {
      animation: none;
    }
  }
</style>
