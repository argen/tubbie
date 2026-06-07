<script lang="ts">
  import '../app.css';
  import { onDestroy, onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import { startBoardSubscription } from '$lib/stores/board.js';
  import { initConfig, config, applyTheme } from '$lib/stores/config.js';
  import { initDisplayMode, displayMode } from '$lib/stores/displayMode.js';
  import { initDisplayPrefs } from '$lib/stores/displayPrefs.js';
  import { settingsOpen } from '$lib/stores/settingsView.js';
  import Attribution from '$lib/components/Attribution.svelte';
  import SettingsView from '$lib/components/SettingsView.svelte';

  interface Props {
    children: Snippet;
  }

  const { children }: Props = $props();

  let cleanupSubscription: (() => void) | null = null;
  let cleanupTrayMenu: (() => void) | null = null;

  onMount(async () => {
    // There is now exactly one window ("main"). Settings used to run in its own
    // webview window — it's now an in-frame overlay (see the SettingsView mount
    // below), so the per-window-label bootstrap skip is gone.

    // Load the active display mode first so the popover-root has the
    // correct chrome class on first paint. This avoids a flash of
    // popover styling inside a regular floating window.
    await initDisplayMode();

    // Hydrate desktop display preferences (group_destinations, …) so the
    // first board render sees the user's saved render flags rather than
    // the default and then jolting after the IPC settles.
    await initDisplayPrefs();

    // Load config (provides theme + station settings)
    await initConfig();

    // Apply persisted theme immediately
    applyTheme($config.theme);

    // Start listening to board://updated events from the Rust stream. Called
    // after initConfig() so the seed fetch and the first stream tick can race
    // safely: board.ts's generated_at latest-wins discipline (invariant #7)
    // ensures whichever payload arrives second is the one the UI keeps.
    cleanupSubscription = await startBoardSubscription();

    // Tray right-click "Settings…" → open the in-frame Settings panel. The
    // Rust tray handler shows + focuses the main window and emits
    // `open-settings`; we flip the store so the overlay mounts over the board.
    try {
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen('open-settings', () => {
        settingsOpen.set(true);
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

  // Reactively apply theme changes (from the Settings panel)
  $effect(() => {
    applyTheme($config.theme);
  });
</script>

<div class="popover-root mode-{$displayMode}">
  <div class="popover-content">
    {@render children()}
  </div>
  <Attribution />
  {#if $settingsOpen}
    <!-- In-frame Settings panel. Absolutely positioned inside popover-root so
         it inherits the window's rounded clip (overflow:hidden) and covers the
         board + footer. The board stays mounted underneath (invariant #7). -->
    <div class="settings-overlay">
      <SettingsView />
    </div>
  {/if}
</div>

<style>
  .popover-content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 24px; /* reserve space for the absolute Attribution footer */
  }

  .settings-overlay {
    position: absolute;
    inset: 0;
    z-index: 60;
    background: var(--bg);
  }
</style>
