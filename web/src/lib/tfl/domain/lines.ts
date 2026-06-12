/**
 * Line-id identity helpers — ported from `crates/tfl-domain/src/types.rs`
 * (`is_supported_line_id`, `canonicalize_line_id`, `line_family_key`).
 *
 * `prettyLineName` already exists in `utils/format.ts` (the single display-name
 * source, used by every component); it is re-exported here so the domain layer
 * has a cohesive line API without a second copy of the table.
 */
export { prettyLineName } from '$lib/utils/format.js';

/**
 * TfL line ids we surface in the UI: the 11 tube lines + Elizabeth (both id
 * forms) + DLR + London Overground (legacy single id + the six named lines TfL
 * introduced Nov 2024). Used as a whitelist when projecting hub stop-points,
 * whose `lineModeGroups` mix bus routes and national-rail operators into the
 * same list.
 */
const SUPPORTED_LINE_IDS: ReadonlySet<string> = new Set([
  // Tube
  'bakerloo',
  'central',
  'circle',
  'district',
  'hammersmith-city',
  'jubilee',
  'metropolitan',
  'northern',
  'piccadilly',
  'victoria',
  'waterloo-city',
  // Elizabeth (line + mode form)
  'elizabeth',
  'elizabeth-line',
  // DLR
  'dlr',
  // London Overground — legacy id + the six named lines
  'london-overground',
  'liberty',
  'lioness',
  'mildmay',
  'suffragette',
  'weaver',
  'windrush',
]);

/** `true` iff `id` is a line we surface (tube / DLR / Overground / Elizabeth). */
export function isSupportedLineId(id: string): boolean {
  return SUPPORTED_LINE_IDS.has(id);
}

/**
 * Map a TfL `lineId` to the canonical line form used by station metadata and
 * the chip filter. TfL hands Elizabeth predictions back as `"elizabeth-line"`
 * (the mode form) but station metadata uses `"elizabeth"` (the line form);
 * without this, filtering by the Elizabeth chip hides every arrival.
 */
export function canonicalizeLineId(raw: string): string {
  return raw === 'elizabeth-line' ? 'elizabeth' : raw;
}

const OVERGROUND_FAMILY: ReadonlySet<string> = new Set([
  'london-overground',
  'liberty',
  'lioness',
  'mildmay',
  'suffragette',
  'weaver',
  'windrush',
]);

/**
 * Collapse the whole London Overground family — the legacy `london-overground`
 * id and the six named lines — to one key; every other id is returned
 * unchanged. The defensive `dropArrivalsForLinesNotServing` filter (Phase 4)
 * compares on this key so a Windrush train survives at a station whose
 * metadata only advertised Mildmay, while a cross-mode phantom (a `bakerloo`
 * train at an Overground station) is still a different family and still
 * dropped. (Invariant #10.)
 */
export function lineFamilyKey(id: string): string {
  return OVERGROUND_FAMILY.has(id) ? 'london-overground' : id;
}
