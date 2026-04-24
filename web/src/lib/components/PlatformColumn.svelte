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
  const arrivals = $derived(platform.arrivals.slice(0, maxRows));
</script>

<section class="platform-col" aria-label="Platform: {displayName}">
  <header class="platform-col__header">
    <span class="platform-col__label" aria-hidden="true">
      {displayName}
    </span>
    <span class="platform-col__dest-header" aria-hidden="true">Destination</span>
    <span class="platform-col__time-header" aria-hidden="true">Time</span>
  </header>

  {#if arrivals.length === 0}
    <div class="platform-col__empty" role="status" aria-live="polite">No arrivals</div>
  {:else}
    <ol class="platform-col__list" aria-label="Arrivals for {displayName}">
      {#each arrivals as arrival (arrival.id)}
        <ArrivalRow {arrival} rank={arrivals.indexOf(arrival) + 1} />
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
    min-width: 260px;
    flex: 1;
  }

  .platform-col__header {
    display: grid;
    grid-template-columns: 1fr auto auto;
    column-gap: 0.5rem;
    padding: 0.4rem 0.5rem 0.3rem;
    border-bottom: 1px solid var(--row-divider);
    background: var(--settings-bg);
  }

  .platform-col__label {
    font-family: 'DSEG14Classic', 'VT323', monospace;
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
    align-self: end;
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
