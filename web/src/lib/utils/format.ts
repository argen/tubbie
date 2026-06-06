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
 * Compact platform identifier for the per-row "PLAT" column.
 *
 * TfL returns either `"Northbound - Platform 1"` (tube prefix-style) or a
 * bare `"Platform 4"` (Elizabeth, Overground, …) or, for a few single-
 * platform stops, just the direction string itself (`"Northbound"`).
 *
 * Rules:
 *   - strip the `"<direction> - "` prefix when present;
 *   - strip the literal word `"Platform "` so the column shows just the
 *     identifier (e.g. `"1"`, `"7"`, `"A"`) — saves significant horizontal
 *     real estate in the 380 px menubar popover. The column header
 *     "PLAT" tells the user what the number means once;
 *   - return `null` when nothing meaningful remains, when the result
 *     equals the direction label, or when it's just the bare word
 *     `"Platform"` with no identifier.
 */
export function platformBadge(platformName: string, direction: string): string | null {
  const dashIdx = platformName.indexOf(' - ');
  const tail = dashIdx === -1 ? platformName : platformName.slice(dashIdx + 3);
  const stripped = tail.replace(/^\s*platform\s*/i, '').trim();
  if (stripped.length === 0) return null;
  if (stripped.toLowerCase() === direction.trim().toLowerCase()) return null;
  // Defensive: if `tail` was literally "Platform" (no number), the regex
  // strips everything and we end up empty — caught by the length check.
  return stripped;
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

/** Human-readable labels for every TfL line id we surface in the UI —
 *  tube + DLR + London Overground (legacy + the six named lines TfL
 *  introduced in Nov 2024) + Elizabeth.
 */
const LINE_LABELS: Record<string, string> = {
  bakerloo: 'Bakerloo',
  central: 'Central',
  circle: 'Circle',
  district: 'District',
  'elizabeth-line': 'Elizabeth',
  elizabeth: 'Elizabeth',
  'hammersmith-city': 'Hammersmith & City',
  jubilee: 'Jubilee',
  metropolitan: 'Metropolitan',
  northern: 'Northern',
  piccadilly: 'Piccadilly',
  victoria: 'Victoria',
  'waterloo-city': 'Waterloo & City',
  dlr: 'DLR',
  'london-overground': 'Overground',
  overground: 'Overground',
  liberty: 'Liberty',
  lioness: 'Lioness',
  mildmay: 'Mildmay',
  suffragette: 'Suffragette',
  weaver: 'Weaver',
  windrush: 'Windrush',
};

/**
 * Map a TfL line id (e.g. `"northern"`, `"elizabeth-line"`) to its display
 * name. Unknown ids are returned unchanged so the UI always renders something.
 */
export function prettyLineName(lineId: string): string {
  return LINE_LABELS[lineId] ?? lineId;
}

/**
 * TfL line id → CSS custom property name defined in app.css. A small map
 * (rather than string-templating `--line-${id}`) lets us alias TfL's id
 * variants (e.g. "elizabeth-line") and fall back silently for unknown ids
 * instead of emitting a reference to a non-existent variable.
 */
const LINE_COLOR_VAR: Record<string, string> = {
  bakerloo: '--line-bakerloo',
  central: '--line-central',
  circle: '--line-circle',
  district: '--line-district',
  'elizabeth-line': '--line-elizabeth',
  elizabeth: '--line-elizabeth',
  'hammersmith-city': '--line-hammersmith-city',
  jubilee: '--line-jubilee',
  metropolitan: '--line-metropolitan',
  northern: '--line-northern',
  piccadilly: '--line-piccadilly',
  victoria: '--line-victoria',
  'waterloo-city': '--line-waterloo-city',
  dlr: '--line-dlr',
  'london-overground': '--line-overground',
  overground: '--line-overground',
  liberty: '--line-liberty',
  lioness: '--line-lioness',
  mildmay: '--line-mildmay',
  suffragette: '--line-suffragette',
  weaver: '--line-weaver',
  windrush: '--line-windrush',
};

/**
 * Resolve a line id to a CSS `var(--line-...)` token, or `"transparent"` for
 * unknown ids. Single source of truth shared by `ArrivalRow` (left stripe on
 * each row) and `LineGroup` (line header accent).
 */
export function lineColorVar(lineId: string): string {
  const name = LINE_COLOR_VAR[lineId];
  return name === undefined ? 'transparent' : `var(${name})`;
}

/**
 * Compute the char-by-char reveal duration for a string.
 *
 * 60ms per character, capped at 1500ms total.
 */
export function revealDuration(text: string): number {
  return Math.min(text.length * 60, 1500);
}

/**
 * Format a great-circle distance (metres) as a TfL dot-matrix-style
 * label. The chosen unit follows the locale: `en-GB` and `en-US` get
 * miles ("0.3MI"); every other locale gets metric ("220M" / "1.4KM").
 *
 * Why a 1.3× walking-distance fudge: haversine measures the chord
 * across London's grid. Real walking paths are longer (rivers,
 * stations on opposite sides of a hub, no zebra crossings). 1.3× is
 * the rule-of-thumb scaling pedestrian routing engines apply across
 * dense urban centres — close enough for a "nearest 3 stations"
 * picker, and free.
 *
 * Output is always uppercase, no internal space, two significant
 * figures so the listbox column stays consistent regardless of
 * whether the value is "0.3MI" (4 chars) or "1.4KM" (5 chars).
 */
export function formatDistance(meters: number, locale: string): string {
  // Defensive: NaN / negative would surface as "NANMI" — cleaner to
  // collapse to an empty label so the row simply lacks a distance
  // chip rather than render garbage.
  if (!Number.isFinite(meters) || meters < 0) return '';

  const adjusted = meters * 1.3;
  const lc = locale.toLowerCase();
  const useMiles =
    lc === 'en-gb' || lc === 'en-us' || lc.startsWith('en-gb-') || lc.startsWith('en-us-');

  if (useMiles) {
    const miles = adjusted / 1609.344;
    if (miles < 0.1) {
      return `${miles.toFixed(2)}MI`;
    }
    return `${miles.toFixed(1)}MI`;
  }

  if (adjusted < 1000) {
    // Round to nearest 10 m so a 213 m haversine becomes "210M"
    // — sub-10 m precision is meaningless to a pedestrian.
    const rounded = Math.round(adjusted / 10) * 10;
    return `${String(rounded)}M`;
  }
  const km = adjusted / 1000;
  return `${km.toFixed(1)}KM`;
}

/**
 * Coarse "time since" label for the status freshness line — e.g. "just now",
 * "3 min ago", "2 hr ago". `updatedAt` / `now` are epoch milliseconds. Returns
 * "" when `updatedAt` is null/undefined (nothing fetched yet) so the caller can
 * omit the line. This is a status timestamp, not a train time, so a relative
 * label is fine (the never-show-HH:MM rule is for arrivals only).
 */
export function formatUpdatedAgo(updatedAt: number | null | undefined, now: number): string {
  if (updatedAt === null || updatedAt === undefined) return '';
  const secs = Math.max(0, Math.round((now - updatedAt) / 1000));
  if (secs < 45) return 'just now';
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${String(mins)} min ago`;
  const hrs = Math.round(mins / 60);
  return `${String(hrs)} hr ago`;
}
