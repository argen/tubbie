<script lang="ts">
  import '../app.css';
  import { onDestroy, onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import { startBoardSubscription } from '$lib/stores/board.js';
  import { initConfig, config, applyTheme } from '$lib/stores/config.js';
  import { initDisplayMode, displayMode } from '$lib/stores/displayMode.js';
  import { initDisplayPrefs } from '$lib/stores/displayPrefs.js';
  import Attribution from '$lib/components/Attribution.svelte';
  import { openSettingsWindow } from '$lib/ipc/commands.js';

  interface Props {
    children: Snippet;
  }

  const { children }: Props = $props();

  let cleanupSubscription: (() => void) | null = null;
  let cleanupTrayMenu: (() => void) | null = null;

  onMount(async () => {
    // Detect which webview window this layout is mounted in.
    // When the settings window loads `/settings` via SPA routing the same
    // +layout.svelte wraps it. We must skip all board-window bootstrap
    // (subscription, config init, display-mode init, tray listener) in that
    // context to avoid:
    //   • a double board://updated subscription that fights the main window
    //   • initConfig() / initDisplayMode() running from the settings renderer
    //   • a recursive tray://open-settings → openSettingsWindow() loop
    // The settings page owns its own onMount and manages its own lifecycle.
    let windowLabel = 'main';
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      windowLabel = getCurrentWindow().label;
    } catch {
      // Not running under Tauri (vitest / plain `vite dev`) — assume main.
    }

    if (windowLabel === 'settings') {
      // Settings window: skip all main-window bootstrap. The settings page
      // component handles its own initialization.
      return;
    }

    // --- Main window bootstrap only below this point ---

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

    // Tray right-click menu "Settings…" → open the dedicated Settings window.
    // The main window cannot navigate to /settings in-place because
    // `load_app_key` is gated to the "settings" webview window only
    // (MEDIUM-2 / M7 TODO fix). The Rust side now calls
    // `open_settings_window_impl` directly from the tray event handler,
    // so this frontend listener is kept only as a fallback for any
    // in-page triggers that still emit `tray://open-settings`.
    try {
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen('tray://open-settings', () => {
        void openSettingsWindow();
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

<div class="popover-root mode-{$displayMode}">
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
