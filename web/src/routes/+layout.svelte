<script lang="ts">
  import '../app.css';
  import { onDestroy, onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import { startBoardSubscription } from '$lib/stores/board.js';
  import { initConfig, config, applyTheme } from '$lib/stores/config.js';
  import Attribution from '$lib/components/Attribution.svelte';
  import { goto } from '$app/navigation';

  interface Props {
    children: Snippet;
  }

  const { children }: Props = $props();

  let cleanupSubscription: (() => void) | null = null;
  let cleanupTrayMenu: (() => void) | null = null;

  onMount(async () => {
    // Load config first (provides theme + station settings)
    await initConfig();

    // Apply persisted theme immediately
    applyTheme($config.theme);

    // Start listening to board://updated events from Rust stream
    cleanupSubscription = await startBoardSubscription();

    // Tray right-click menu "Settings…" → navigate the popover to /settings.
    // Tauri event listener is only available in the Tauri runtime, so we
    // feature-detect by importing dynamically.
    try {
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen('tray://open-settings', () => {
        void goto('/settings');
      });
      cleanupTrayMenu = unlisten;
    } catch {
      // Not running under Tauri (e.g. vitest / plain `vite dev`) — skip.
    }
  });

  onDestroy(() => {
    cleanupSubscription?.();
    cleanupTrayMenu?.();
  });

  // Reactively apply theme changes (from settings page)
  $effect(() => {
    applyTheme($config.theme);
  });
</script>

<div class="popover-root">
  <div class="popover-content">
    {@render children()}
  </div>
  <Attribution />
</div>

<style>
  .popover-content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 24px; /* reserve space for the absolute Attribution footer */
  }
</style>
