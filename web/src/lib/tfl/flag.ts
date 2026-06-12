/**
 * Runtime feature flag for the TypeScript TfL core.
 *
 * While the Rust→TS port lands incrementally, both data paths ship in the
 * binary. This flag selects which one drives the UI, per install, at runtime —
 * a `localStorage` toggle so a tester can A/B against real TfL without a
 * rebuild. Defaults OFF until the port is proven (source plan Phase 5+).
 *
 * No callers yet — wiring lands in Phase 5.
 */

/** `localStorage` key holding the flag. Value is the string `"true"` when on. */
export const USE_TS_TFL_KEY = 'tubbie:use_ts_tfl';

/**
 * Whether the frontend should drive TfL data through the TypeScript core
 * instead of the Rust/IPC path. SSR- and no-`localStorage`-safe: any absence
 * or access error reads as `false`.
 */
export function useTsTfl(): boolean {
  try {
    if (typeof localStorage === 'undefined') return false;
    return localStorage.getItem(USE_TS_TFL_KEY) === 'true';
  } catch {
    return false;
  }
}
