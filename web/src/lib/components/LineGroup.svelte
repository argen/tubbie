<script lang="ts">
  import type { Arrival } from '$lib/ipc/types.js';
  import { displayMode } from '$lib/stores/displayMode.js';
  import { lineColorVar, prettyLineName } from '$lib/utils/format.js';
  import PlatformColumn from './PlatformColumn.svelte';

  /**
   * One direction within a line — e.g. Bakerloo > Northbound. Created
   * by `Board.svelte::linesGrouped` from per-arrival grouping; the
   * `arrivals` here are already filtered to a single `line_id`.
   */
  interface DirectionBucket {
    key: string;
    label: string;
    arrivals: Arrival[];
  }

  interface Props {
    /** TfL line id of the group. */
    lineId: string;
    /** Display name for the group header. Falls back to prettyLineName(lineId). */
    lineName?: string;
    /** One bucket per direction this line serves at the station. */
    directions: DirectionBucket[];
    /** Max arrival rows to show per direction. Tied to (mode, lineCount) by Board. */
    maxRows: number;
    /**
     * When true, each `PlatformColumn` collapses arrivals sharing a
     * `(destination_name, towards)` key into one row with a comma-
     * separated minutes sequence. Sourced from the desktop-only
     * `displayPrefs.group_destinations` flag — Board.svelte threads it
     * through.
     */
    groupDestinations?: boolean;
  }

  const { lineId, lineName, directions, maxRows, groupDestinations = false }: Props = $props();

  // The template appends " Line", so strip any trailing standalone "line"
  // the source name already carries. TfL's arrivals feed names the Elizabeth
  // line "Elizabeth line" — without this it renders "Elizabeth line Line"
  // ("ELIZABETH LINE LINE" once uppercased).
  const headerLabel = $derived(
    (lineName !== undefined && lineName.length > 0 ? lineName : prettyLineName(lineId)).replace(
      /\s+line$/i,
      '',
    ),
  );
  const accent = $derived(lineColorVar(lineId));
</script>

<section
  class="line-group"
  class:line-group--stacked={$displayMode === 'menubar'}
  data-line-id={lineId}
  style:--line-accent={accent}
  aria-label="{headerLabel} line"
>
  <header class="line-group__header" aria-hidden="true">
    <span class="line-group__swatch"></span>
    <span class="line-group__name">{headerLabel} Line</span>
  </header>
  <div class="line-group__platforms">
    {#each directions as dir (dir.key)}
      <PlatformColumn
        platform={{ name: dir.label, arrivals: dir.arrivals }}
        {maxRows}
        {groupDestinations}
      />
    {/each}
  </div>
</section>

<style>
  .line-group {
    display: flex;
    flex-direction: column;
    background: var(--row-divider);
  }

  .line-group__header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0.6rem;
    background: color-mix(in srgb, var(--line-accent, transparent) 18%, var(--bg));
    border-bottom: 1px solid var(--row-divider);
    /* The line accent reads as a thin coloured stripe along the left edge,
       echoing the per-row stripe in ArrivalRow so the visual hierarchy
       (Line → Platform → Train) is consistent. */
    border-left: 3px solid var(--line-accent, transparent);
  }

  .line-group__swatch {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--line-accent, transparent);
    box-shadow: 0 0 6px var(--line-accent, transparent);
    flex-shrink: 0;
  }

  .line-group__name {
    font-family: var(--font-board);
    font-size: 0.8rem;
    color: var(--platform-label);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    text-shadow:
      0 0 4px var(--platform-label),
      0 0 8px color-mix(in srgb, var(--platform-label) 25%, transparent);
  }

  /* Window mode: responsive grid — at 700px wide the two directions of a
     single-line station sit side by side; at 1200px three lines worth of
     directions pack into wider columns. The 280px floor is wide enough
     that a typical destination ("Edgware via CX", "Heathrow Terminal 4")
     fits without ever needing the marquee. */
  .line-group__platforms {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 1px;
    background: var(--row-divider);
  }

  /* Menubar mode: stack platforms vertically. The popover is only 380px
     wide, so packing two directions into ~190px columns each squashes
     destinations to "Hig…" / "Edg…". A single full-width column gives
     ~340px of usable space per direction, which fits any TfL station
     name comfortably without ever needing the marquee. */
  .line-group--stacked .line-group__platforms {
    grid-template-columns: 1fr;
  }
</style>
