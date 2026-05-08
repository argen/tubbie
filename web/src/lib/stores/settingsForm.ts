/**
 * Shared state for the /settings page — form values, the debounced
 * persist, and the "Saving / Saved" chip status.
 *
 * Lifted out of `routes/settings/+page.svelte` so the section components
 * (ApiKeySection / DisplayModeSection / DisplayPrefsSection are already
 * extracted; Station, Favorites, Lines, Directions, Poll, and Theme are
 * next) can read and write without prop drilling.
 *
 * Why a writable store rather than a `$state` rune in a `.svelte.ts`
 * module: the rest of this codebase's shared state (config, displayMode,
 * displayPrefs, favorites) is already a `writable<T>` store, so this
 * matches existing patterns and the existing test ergonomics
 * (`get(store)` in vitest specs, `store.set(...)` in test setup).
 *
 * Persistence model: every field mutation calls `persistDebounced()`,
 * which trails by 400 ms. A burst (12-chip toggle, slider drag) becomes
 * one `save_config` IPC. `flushPending()` runs the pending save
 * immediately — wired into the page's `onDestroy` and `beforeunload`
 * so a click made inside the debounce window survives navigation.
 */

import { writable, get } from 'svelte/store';
import { config, configError, updateConfig } from './config.js';
import { debounce, type Debounced } from '$lib/utils/debounce.js';
import type { Direction, LineRef } from '$lib/ipc/types.js';

export interface SettingsFormState {
  stationId: string;
  /** Friendly name of the picked station; empty until the user touches the picker. */
  stationName: string;
  /**
   * Lines served by the currently-selected station, populated from
   * `StationSearch.onSelect`. Empty on first mount (we don't know which
   * lines the saved station serves without refetching) — the UI then falls
   * back to the global KNOWN_LINES list.
   */
  stationLines: LineRef[];
  lineIds: string[];
  selectedDirections: Direction[];
  pollSeconds: number;
  /**
   * `BoardConfig.theme` is typed as `string` (not the narrower `ThemeId`
   * union) because the wire format may carry historical values that
   * `applyTheme` doesn't know about. The narrowing happens at the
   * `handleThemeSelect` boundary in the page, where the picker only
   * yields valid `ThemeId`s.
   */
  theme: string;
}

export type SaveState = 'idle' | 'saving' | 'saved';

function initialFormState(): SettingsFormState {
  const cfg = get(config);
  return {
    stationId: cfg.station_id,
    stationName: '',
    stationLines: [],
    lineIds: [...cfg.line_ids],
    selectedDirections: [...cfg.directions],
    pollSeconds: cfg.poll_seconds,
    theme: cfg.theme,
  };
}

export const settingsForm = writable<SettingsFormState>(initialFormState());
export const saveState = writable<SaveState>('idle');

let saveStateTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Patch the form. Use this from section handlers instead of a direct
 * `settingsForm.update`-with-spread so the call site stays readable.
 */
export function updateForm(patch: Partial<SettingsFormState>): void {
  settingsForm.update((s) => ({ ...s, ...patch }));
}

/**
 * Reset form back to current `$config` values. Call from `onMount` of
 * /settings so each visit reflects the latest config — otherwise stale
 * form state survives across SPA navigations.
 */
export function resyncFormFromConfig(): void {
  settingsForm.set(initialFormState());
}

/**
 * Persist the current form state. `updateConfig` catches its own errors
 * and drives `$configError`, so callers never need to try/catch.
 *
 * The backend's `save_config` publishes the new config to a watch channel
 * the running stream observes; the stream applies the change on its next
 * tick (or immediately for `poll_seconds`/`station_id`) without
 * restarting. The board page subscribes to the same `$config` store and
 * updates in place — no explicit navigation needed.
 */
export async function persist(): Promise<void> {
  if (saveStateTimer !== null) {
    clearTimeout(saveStateTimer);
    saveStateTimer = null;
  }
  saveState.set('saving');
  const form = get(settingsForm);
  await updateConfig({
    station_id: form.stationId,
    line_ids: form.lineIds,
    directions: form.selectedDirections,
    poll_seconds: Math.min(300, Math.max(10, form.pollSeconds)),
    theme: form.theme,
  });
  if (get(configError) !== null) {
    saveState.set('idle');
    return;
  }
  saveState.set('saved');
  saveStateTimer = setTimeout(() => {
    saveState.set('idle');
    saveStateTimer = null;
  }, 1500);
}

// Slider events fire on every tick of the drag, and chip / direction /
// theme toggles can come in bursts. Debounce to the trailing edge so a
// burst becomes one disk write and one watch-channel publish.
export const persistDebounced: Debounced<[]> = debounce(persist, 400);

/** Flush any pending debounced persist. Call from onDestroy + beforeunload. */
export function flushPending(): void {
  persistDebounced.flush();
}

/** Cancel the "Saved" auto-clear timeout. Call from onDestroy. */
export function cancelSaveStateTimer(): void {
  if (saveStateTimer !== null) {
    clearTimeout(saveStateTimer);
    saveStateTimer = null;
  }
}
