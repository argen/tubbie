<script lang="ts">
  import type { Arrival, Platform } from '$lib/ipc/types.js';
  import {
    formatTimeToStation,
    isDue,
    lineColorVar,
    shortPlatformName,
    shortStationName,
  } from '$lib/utils/format.js';
  import { now } from '$lib/stores/clock.js';
  import ArrivalRow from './ArrivalRow.svelte';

  interface Props {
    platform: Platform;
    /** Max arrivals to show. Default 6. */
    maxRows?: number;
    /**
     * Opt-in destination grouping (Phase 3 of arrival-feedback plan).
     * When true, arrivals sharing a `(destination_name, towards)` key
     * collapse into a single row with a comma-separated minutes
     * sequence ("Edgware · 2, 5, 9 min"). Frontend-only: backend
     * keeps shipping the full per-train set.
     */
    groupDestinations?: boolean;
  }

  const { platform, maxRows = 6, groupDestinations = false }: Props = $props();

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

  // ---------------------------------------------------------------------------
  // Destination-grouped view (opt-in, Phase 3)
  //
  // Each group keys on `(destination_name, towards)` so distinct via-paths
  // (e.g. "Edgware via CX" vs "Edgware via Bank") stay split. Within a
  // group the times are derived live from `expected_arrival` against the
  // shared 1 Hz clock — same anchor as `ArrivalRow.svelte`, so a group
  // counts down second-by-second between polls without any extra work.
  // ---------------------------------------------------------------------------

  interface DestGroup {
    key: string;
    destination: string;
    towards: string;
    line_id: string;
    seconds: number[]; // ascending; the first entry drives the due-pulse
  }

  // Keep group cardinality bounded — a 4-line interchange with eight
  // destinations would otherwise stack a tall column.
  const MAX_GROUPS = 6;
  // Cap visible times per group so the marquee-free row stays compact.
  const MAX_TIMES_PER_GROUP = 4;

  function groupKey(a: Arrival): string {
    return `${a.destination_name}|${a.towards}`;
  }

  function liveSecondsFor(a: Arrival, currentMs: number): number {
    const expectedMs = Date.parse(a.expected_arrival);
    if (!Number.isFinite(expectedMs)) return a.time_to_station;
    return Math.round((expectedMs - currentMs) / 1000);
  }

  const grouped = $derived.by<DestGroup[]>(() => {
    const currentMs = $now;
    // `Map` lookup is O(1) and we never expose this collection past the
    // $derived run — it's a transient grouping aid, not reactive state.
    // The `svelte/prefer-svelte-reactivity` lint can't see that, so we
    // ESLint-disable rather than reach for `SvelteMap` (which would add
    // a Svelte fine-grained subscription to a thing we throw away each
    // tick anyway).
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const byKey = new Map<string, DestGroup>();
    for (const a of platform.arrivals) {
      const key = groupKey(a);
      const secs = liveSecondsFor(a, currentMs);
      const existing = byKey.get(key);
      if (existing) {
        existing.seconds.push(secs);
      } else {
        byKey.set(key, {
          key,
          destination: shortStationName(a.destination_name),
          towards: a.towards,
          line_id: a.line_id,
          seconds: [secs],
        });
      }
      if (byKey.size >= MAX_GROUPS) break;
    }
    for (const g of byKey.values()) g.seconds.sort((x, y) => x - y);
    return Array.from(byKey.values());
  });

  /**
   * Render a sorted seconds list as a single condensed string, mirroring
   * `formatTimeToStation`: "Due, 2, 5 min" / "2, 5, 9 mins". A trailing
   * "+N more" suffix appears when the group exceeds `MAX_TIMES_PER_GROUP`
   * so the row stays one line wide.
   */
  function formatGroupTimes(seconds: number[]): string {
    const visible = seconds.slice(0, MAX_TIMES_PER_GROUP);
    const more = Math.max(0, seconds.length - visible.length);
    const parts = visible.map((s) => {
      // Strip the "min"/"mins" suffix from individual entries; we append
      // one suffix at the end so "2, 5, 9 min" reads naturally.
      const t = formatTimeToStation(s);
      if (t === 'Due') return 'Due';
      return t.replace(/\s*mins?$/, '');
    });
    // Pick the suffix from the LAST visible entry so a "1 min" in the list
    // still reads as "min" not "mins" — matches platform-board grammar.
    const last = visible[visible.length - 1] ?? 0;
    const lastFormatted = formatTimeToStation(last);
    const suffix = lastFormatted === 'Due' ? '' : / mins?$/.test(lastFormatted)
      ? lastFormatted.replace(/^\d+\s*/, '')
      : 'min';
    const head = parts.join(', ');
    const body = suffix.length > 0 ? `${head} ${suffix}`.trim() : head;
    return more > 0 ? `${body} · +${String(more)} more` : body;
  }
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
      <span class="platform-col__plat-header">Plat</span>
      <span class="platform-col__time-header">Time</span>
    </div>
  </header>

  {#if arrivals.length === 0}
    <div class="platform-col__empty" role="status" aria-live="polite">No arrivals</div>
  {:else if groupDestinations}
    <ol class="platform-col__list" aria-label="Arrivals for {displayName}">
      {#each grouped as group, idx (group.key)}
        {@const summary = formatGroupTimes(group.seconds)}
        {@const groupDue = isDue(group.seconds[0] ?? 999)}
        <li
          class="arrival-row arrival-row--grouped"
          data-line-id={group.line_id}
          style:--line-color={lineColorVar(group.line_id)}
          aria-label="Group {idx + 1}: {group.destination} {group.towards}, {summary}"
        >
          <span class="arrival-row__rank" aria-hidden="true">{idx + 1}</span>
          <span class="arrival-row__dest led-text">{group.destination}</span>
          <span class="arrival-row__via">{group.towards}</span>
          <!-- Grouped rows span platforms by definition; leave the PLAT
               cell empty rather than guess. The grid alignment with
               non-grouped rows still works because the column slot exists. -->
          <span class="arrival-row__platform"></span>
          <span
            class="arrival-row__time"
            class:due-pulse={groupDue}
            class:led-accent={groupDue}>{summary}</span
          >
        </li>
      {/each}
    </ol>
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
     "Plat", and "Time" below all line up at the same x-positions as the
     row content. Five columns now: rank | dest | towards | plat | time. */
  .platform-col__row {
    display: grid;
    grid-template-columns: 1.2rem 1fr auto auto auto;
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
  .platform-col__plat-header,
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

  .platform-col__plat-header {
    grid-column: 4;
    text-align: right;
    min-width: 0.9rem;
  }

  .platform-col__time-header {
    grid-column: 5;
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
