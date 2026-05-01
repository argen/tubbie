<script lang="ts">
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import type { Arrival } from '$lib/ipc/types.js';
  import {
    formatTimeToStation,
    isDue,
    lineColorVar,
    platformBadge,
    revealDuration,
    shortStationName,
  } from '$lib/utils/format.js';
  import { reducedMotion } from '$lib/stores/reducedMotion.js';
  import { now } from '$lib/stores/clock.js';

  interface Props {
    arrival: Arrival;
    rank: number; // 1-based position in the list
  }

  const { arrival, rank }: Props = $props();

  // Real dot-matrix boards show "Morden", not "Morden Underground Station".
  // Kept in a local const so reveal, marquee copies, and aria-labels stay
  // in sync — otherwise the screen-reader string drifts from the visible one.
  const destination = $derived(shortStationName(arrival.destination_name));

  const lineColor = $derived(lineColorVar(arrival.line_id));

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

  // Keep in lockstep with the in:fade duration below so the reveal starts
  // exactly as the row finishes fading in.
  const FADE_IN_MS = 600;

  onMount(() => {
    if ($reducedMotion) {
      startReveal(destination, true);
      return;
    }
    // Guard against unmount during the fade-in (e.g. a very brief arrival
    // that drops off before the delay fires) — without the cleanup the
    // timeout would still run and mutate state on a torn-down component.
    const id = setTimeout(() => {
      startReveal(destination, false);
    }, FADE_IN_MS);
    return () => {
      clearTimeout(id);
    };
  });

  // ---------------------------------------------------------------------------
  // Marquee: once reveal finishes, if the destination text is wider than its
  // column, render two copies side by side and animate the track by the width
  // of one copy — so it scrolls off the left and the second copy is already in
  // place behind it, giving a seamless left-flowing loop.
  // ---------------------------------------------------------------------------

  let destEl: HTMLSpanElement | undefined = $state();
  let textWidth = $state(0); // natural width of one copy of the destination name
  let overflowing = $state(false);

  // Pixels per second for the scroll. ~30px/s reads comfortably for a name
  // like "Edgware Underground Station" without ever feeling rushed.
  const MARQUEE_PX_PER_SEC = 30;

  // Gap between the two copies of the text as it loops, in pixels.
  // Must match `--marquee-gap` in CSS.
  const MARQUEE_GAP_PX = 48;

  function measureOverflow(): void {
    if (!destEl) return;
    // scrollWidth of the container reflects the natural text width while the
    // inner track is not yet duplicated (i.e. while `overflowing` is false).
    // Once we've flipped to marquee mode we stop re-measuring from this
    // element because duplication distorts the number.
    if (!overflowing) {
      textWidth = destEl.scrollWidth;
    }
    overflowing = destEl.scrollWidth > destEl.clientWidth + 1;
  }

  $effect(() => {
    if (!revealComplete || $reducedMotion) {
      overflowing = false;
      return;
    }
    const raf = requestAnimationFrame(measureOverflow);
    const onResize = (): void => {
      overflowing = false; // re-measure from scratch on resize
      requestAnimationFrame(measureOverflow);
    };
    window.addEventListener('resize', onResize);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener('resize', onResize);
    };
  });

  const marqueeDuration = $derived(
    overflowing ? Math.max((textWidth + MARQUEE_GAP_PX) / MARQUEE_PX_PER_SEC, 6) : 0,
  );

  // ---------------------------------------------------------------------------
  // Time formatting + due state
  // ---------------------------------------------------------------------------

  // Live seconds-to-station derived from `expected_arrival` (a wall-clock
  // anchor) and the shared 1 Hz `$now` store. `time_to_station` from the
  // wire is frozen between polls — trusting it would make a "5 min" train
  // still read "5 min" 60 s later. The Rust serde layer always emits a
  // valid ISO-8601 string for `DateTime<Utc>`; the `Number.isFinite`
  // guard is purely defensive so a malformed value degrades to the wire
  // count instead of crashing the render.
  const liveSeconds = $derived.by(() => {
    const expectedMs = Date.parse(arrival.expected_arrival);
    if (!Number.isFinite(expectedMs)) return arrival.time_to_station;
    return Math.round((expectedMs - $now) / 1000);
  });

  const formattedTime = $derived(formatTimeToStation(liveSeconds));
  const due = $derived(isDue(liveSeconds));

  // Per-row platform badge (e.g. "Platform 4") — null when redundant
  // with the column header so single-platform stops stay clean.
  const platformLabel = $derived(platformBadge(arrival.platform_name, arrival.direction));
</script>

<li
  class="arrival-row"
  data-line-id={arrival.line_id}
  style:--line-color={lineColor}
  aria-label="Train {rank}: {destination}, {formattedTime}"
  in:fade|global={{ duration: $reducedMotion ? 0 : FADE_IN_MS }}
  out:fade|global={{ duration: $reducedMotion ? 0 : 200 }}
>
  <span class="arrival-row__rank" aria-hidden="true">{rank}</span>

  <span
    class="arrival-row__dest led-text"
    class:arrival-row__dest--marquee={overflowing}
    aria-label="Destination: {destination}"
    bind:this={destEl}
  >
    {#if overflowing}
      <span class="arrival-row__dest-track" style:animation-duration="{marqueeDuration}s">
        <span class="arrival-row__dest-copy">{destination}</span>
        <span class="arrival-row__dest-copy" aria-hidden="true">{destination}</span>
      </span>
    {:else}
      <span class="arrival-row__dest-track">
        {revealComplete ? destination : revealedDest}
        {#if !revealComplete}<span class="arrival-row__cursor" aria-hidden="true">_</span>{/if}
      </span>
    {/if}
  </span>

  <span class="arrival-row__via" aria-label="Towards: {arrival.towards}">
    {arrival.towards}
  </span>

  <span
    class="arrival-row__platform"
    aria-label={platformLabel ? `Platform: ${platformLabel}` : undefined}
  >
    {platformLabel ?? ''}
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
    /* rank | destination | towards | plat | time
       The platform column carries just an identifier ("4", "P3", "A");
       the column header in PlatformColumn shows "PLAT" once so the
       digit's meaning is clear. Saves significant horizontal real
       estate compared to repeating "Platform N" on every row in the
       380 px menubar popover. */
    grid-template-columns: 1.2rem 1fr auto auto auto;
    column-gap: 0.5rem;
    align-items: center;
    /* 4px left stripe carries the line colour; padding-left compensates so
       the rank digit lines up with where it was before the stripe existed. */
    padding: 0.3rem 0.5rem 0.3rem calc(0.5rem + 4px);
    border-left: 4px solid var(--line-color, transparent);
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
  }

  /* Two copies of the text sit side-by-side on the track. We translate the
     whole track by exactly -50% — which, because both copies are the same
     width, lands the second copy where the first started. Looping the same
     animation from there gives an uninterrupted right-to-left scroll. */
  .arrival-row__dest--marquee .arrival-row__dest-track {
    animation: dest-scroll linear infinite;
    will-change: transform;
  }

  .arrival-row__dest-copy {
    display: inline-block;
    padding-right: 48px; /* keep in sync with MARQUEE_GAP_PX in the script */
  }

  @keyframes dest-scroll {
    from {
      transform: translateX(0);
    }
    to {
      transform: translateX(-50%);
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

  .arrival-row__platform {
    /* Just the identifier ("4", "P3", "A"). The column header in
       PlatformColumn says "PLAT" once so the meaning is clear without
       repeating the word on every row. Lower opacity so the eye still
       locks on the destination first; platform is supporting context. */
    color: var(--platform-label);
    font-size: 0.85rem;
    opacity: 0.8;
    min-width: 0.9rem;
    text-align: right;
    letter-spacing: 0.05em;
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
