/**
 * Formatting utilities for the arrivals board display.
 * All functions are pure — no side-effects, no imports from Tauri.
 */

/**
 * Format `time_to_station` (seconds) as the TfL dot-matrix board does:
 *   < 30s → "Due"
 *   < 90s → "1 min"
 *   else  → "N mins"
 */
export function formatTimeToStation(seconds: number): string {
  if (seconds < 30) return 'Due';
  if (seconds < 90) return '1 min';
  return `${String(Math.floor(seconds / 60))} mins`;
}

/**
 * Format a UTC ISO string as "HH:MM" in local time.
 */
export function formatTime(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
}

/**
 * Truncate a string to `maxLen` chars, appending "…" if truncated.
 */
export function truncate(str: string, maxLen: number): string {
  if (str.length <= maxLen) return str;
  return str.slice(0, maxLen - 1) + '…';
}

/**
 * Extract a short platform label from a full platform name.
 *
 * TfL platform names are like "Northbound - Platform 1".
 * We want "Northbound" or "Platform 1" depending on context.
 */
export function shortPlatformName(fullName: string): string {
  const dashIdx = fullName.indexOf(' - ');
  if (dashIdx !== -1) {
    return fullName.slice(0, dashIdx);
  }
  return fullName;
}

/**
 * Trim the TfL " Underground Station" / " DLR Station" / " Rail Station"
 * suffix so station/destination names match the real dot-matrix board,
 * where "Morden Underground Station" is rendered as just "Morden".
 *
 * Case-insensitive on the suffix; the preceding name keeps its original
 * casing so callers can decide whether to uppercase downstream.
 * Returns the input unchanged when no known suffix is present (or when
 * the input is empty / would become empty after trimming).
 */
export function shortStationName(fullName: string): string {
  const suffixes = [' Underground Station', ' DLR Station', ' Rail Station'];
  const lower = fullName.toLowerCase();
  for (const suf of suffixes) {
    const sufLower = suf.toLowerCase();
    if (lower.endsWith(sufLower)) {
      const trimmed = fullName.slice(0, fullName.length - suf.length).trim();
      if (trimmed.length > 0) return trimmed;
    }
  }
  return fullName;
}

/**
 * Return true if the given time_to_station should show as "Due".
 */
export function isDue(seconds: number): boolean {
  return seconds < 30;
}

/**
 * Compute the char-by-char reveal duration for a string.
 *
 * 60ms per character, capped at 1500ms total.
 */
export function revealDuration(text: string): number {
  return Math.min(text.length * 60, 1500);
}
