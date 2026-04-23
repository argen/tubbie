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

  <span class="arrival-row__dest led-text" aria-label="Destination: {arrival.destination_name}">
    {revealComplete ? arrival.destination_name : revealedDest}
    {#if !revealComplete}<span class="arrival-row__cursor" aria-hidden="true">_</span>{/if}
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
    font-family: 'VT323', monospace;
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
