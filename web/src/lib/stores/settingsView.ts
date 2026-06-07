import { writable } from 'svelte/store';

/**
 * Whether the in-frame Settings panel is open over the board.
 *
 * Replaces the old separate `"settings"` webview window (PR2). Settings now
 * renders as a full-frame overlay inside the main window — the same in-app
 * pattern as the Status view and station Search — so it works identically in
 * both window and menu-bar display modes, and switching display mode no longer
 * leaves a second OS window stranded.
 *
 * The board stays mounted underneath while this is true (see `+layout.svelte`),
 * so opening Settings never re-warms the board cache (CLAUDE.md invariant #7).
 */
export const settingsOpen = writable(false);

/** Open the in-frame Settings panel. */
export function openSettings(): void {
  settingsOpen.set(true);
}

/** Close the in-frame Settings panel, returning to the board. */
export function closeSettings(): void {
  settingsOpen.set(false);
}
