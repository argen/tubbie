<script lang="ts">
  import '../app.css';
  import { onDestroy, onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import { startBoardSubscription } from '$lib/stores/board.js';
  import { initConfig, config, applyTheme } from '$lib/stores/config.js';
  import Attribution from '$lib/components/Attribution.svelte';

  interface Props {
    children: Snippet;
  }

  const { children }: Props = $props();

  let cleanupSubscription: (() => void) | null = null;

  onMount(async () => {
    // Load config first (provides theme + station settings)
    await initConfig();

    // Apply persisted theme immediately
    applyTheme($config.theme);

    // Start listening to board://updated events from Rust stream
    cleanupSubscription = await startBoardSubscription();
  });

  onDestroy(() => {
    cleanupSubscription?.();
  });

  // Reactively apply theme changes (from settings page)
  $effect(() => {
    applyTheme($config.theme);
  });
</script>

{@render children()}

<Attribution />
