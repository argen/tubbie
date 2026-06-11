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

    // Three independent config reads, each a single Tauri IPC with no
    // dependency on the others. Run them concurrently instead of three
    // serial round-trips — this shortens the time before the board seed
    // fires below. Each store still updates the instant its own IPC
    // resolves, so the ordering guarantees that mattered are preserved:
    //   • display mode → popover-root chrome class set before first paint
    //     settles (avoids a flash of popover styling in a floating window);
    //   • display prefs → first board render sees saved render flags
    //     (group_destinations, …) rather than defaults that then jolt;
    //   • config → theme + station settings.
    // The two genuine *sequencing* constraints are kept explicit after the
    // barrier: applyTheme runs once config has resolved, and the board
    // subscription starts after config (see below).
    await Promise.all([initDisplayMode(), initDisplayPrefs(), initConfig()]);

    // Apply persisted theme immediately (config has resolved above)
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
  <!-- `inert` while Settings is open: the board stays MOUNTED underneath
       (invariant #7 — no re-fetch/re-warm) but is removed from the tab order and
       the a11y tree, so the overlay's role="dialog" aria-modal is actually
       enforced (focus can't Tab out behind it). -->
  <div class="popover-content" inert={$settingsOpen}>
    {@render children()}
  </div>
  <div class="attribution-host" inert={$settingsOpen}>
    <Attribution />
  </div>
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

  /* `display: contents` so the inert wrapper adds no box — Attribution keeps
     its absolute positioning relative to .popover-root. */
  .attribution-host {
    display: contents;
  }

  .settings-overlay {
    position: absolute;
    inset: 0;
    z-index: 60;
    background: var(--bg);
  }
</style>
