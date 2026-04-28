<script lang="ts">
  import type { Platform } from '$lib/ipc/types.js';
  import { shortPlatformName } from '$lib/utils/format.js';
  import ArrivalRow from './ArrivalRow.svelte';

  interface Props {
    platform: Platform;
    /** Max arrivals to show. Default 6. */
    maxRows?: number;
  }

  const { platform, maxRows = 6 }: Props = $props();

  const displayName = $derived(shortPlatformName(platform.name));

  /**
   * The keyed-each below uses `(line_id, platform_name, expected_arrival)`
   * as the row key. Within a real TfL response that triple is the
   * minimum unique slot — one physical train at one platform at one moment.
   * If TfL ever serves a payload where two predictions collide on every
   * field of the triple (we have not observed it; its `id` already
   * surprised us once), Svelte would throw `each_key_duplicate` and the
   * render would freeze on the previous frame. Dedupe defensively here so
   * a future surprise in the data shape can never re-introduce that bug.
   * First-occurrence wins; the dropped row is invisible to the user.
   */
  function rowKey(a: { line_id: string; platform_name: string; expected_arrival: string }): string {
    return `${a.line_id}|${a.platform_name}|${a.expected_arrival}`;
  }
  const arrivals = $derived.by(() => {
    // `string[]` not `Set<string>`: with maxRows ≤ 6 the O(n²) lookup is
    // trivially cheap and avoids the `svelte/prefer-svelte-reactivity` lint —
    // the local set is never reactive (it's recreated on every $derived run)
    // but the lint can't tell that.
    const seen: string[] = [];
    const out: typeof platform.arrivals = [];
    for (const a of platform.arrivals) {
      const k = rowKey(a);
      if (seen.includes(k)) continue;
      seen.push(k);
      out.push(a);
      if (out.length >= maxRows) break;
    }
    return out;
  });
</script>

<section class="platform-col" aria-label="Platform: {displayName}">
  <header class="platform-col__header" aria-hidden="true">
    <div class="platform-col__row platform-col__row--label">
      <span></span>
      <span class="platform-col__label">{displayName}</span>
    </div>
    <div class="platform-col__row platform-col__row--subheader">
      <span></span>
      <span class="platform-col__dest-header">Destination</span>
      <span></span>
      <span class="platform-col__time-header">Time</span>
    </div>
  </header>

  {#if arrivals.length === 0}
    <div class="platform-col__empty" role="status" aria-live="polite">No arrivals</div>
  {:else}
    <ol class="platform-col__list" aria-label="Arrivals for {displayName}">
      <!--
        Key on (line_id, platform_name, expected_arrival) — NOT on `arrival.id`.
        TfL's prediction `id` is not a unique identifier (observed at Chalk
        Farm: 10 distinct trains all returned with `id=1731547612`); a
        keyed-each on `arrival.id` crashes Svelte with each_key_duplicate and
        leaves the UI stuck on the previous render. The composite below is
        stable for one real train across polls so row enter/exit transitions
        still work.
      -->
      {#each arrivals as arrival, idx (`${arrival.line_id}|${arrival.platform_name}|${arrival.expected_arrival}`)}
        <ArrivalRow {arrival} rank={idx + 1} />
      {/each}
    </ol>
  {/if}
</section>

<style>
  .platform-col {
    background: var(--bg);
    border: 1px solid var(--row-divider);
    border-radius: 2px;
    display: flex;
    flex-direction: column;
    /* Width comes from the parent (LineGroup CSS grid with auto-fit /
       minmax). `min-width: 0` lets the grid track actually shrink below
       the column's natural width — without it the children's intrinsic
       size pins the column wide and overflows the container. */
    min-width: 0;
  }

  .platform-col__header {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.4rem 0.5rem 0.3rem;
    border-bottom: 1px solid var(--row-divider);
    background: var(--settings-bg);
  }

  /* Both header rows share ArrivalRow's grid so NORTHBOUND, "Destination",
     and the destination text below all line up at the same x-position. */
  .platform-col__row {
    display: grid;
    grid-template-columns: 1.2rem 1fr auto auto;
    column-gap: 0.5rem;
    align-items: center;
  }

  .platform-col__label {
    grid-column: 2 / -1;
    font-family: var(--font-board);
    font-size: 0.9rem;
    color: var(--platform-label);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    text-shadow:
      0 0 4px var(--platform-label),
      0 0 8px color-mix(in srgb, var(--platform-label) 30%, transparent);
  }

  .platform-col__dest-header,
  .platform-col__time-header {
    font-family: var(--font-board);
    font-size: 0.75rem;
    color: var(--platform-label);
    opacity: 0.5;
    text-transform: uppercase;
    letter-spacing: 0.1em;
  }

  .platform-col__dest-header {
    grid-column: 2;
  }

  .platform-col__time-header {
    grid-column: 4;
    text-align: right;
    min-width: 4.5rem;
  }

  .platform-col__list {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
  }

  .platform-col__empty {
    padding: 1rem 0.5rem;
    color: var(--platform-label);
    opacity: 0.5;
    font-family: var(--font-board);
    font-size: 1rem;
    text-align: center;
  }
</style>
