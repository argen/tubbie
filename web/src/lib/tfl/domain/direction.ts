/**
 * Direction + Northern-branch inference — ported verbatim-in-logic from
 * `crates/tfl-domain/src/direction.rs`.
 *
 * `Direction` and `NorthernBranch` are the canonical string unions from
 * `ipc/types.ts` (they serialise as bare strings on the Rust side, so the TS
 * unions are a faithful mirror).
 */
import type { Direction, NorthernBranch } from '$lib/ipc/types.js';

/**
 * Compass axis a line is constrained to network-wide, or `null` when the line
 * legitimately labels platforms differently at different stations (Met is N/S
 * at Baker Street, E/W at Watford; Jubilee, Piccadilly, District, DLR are
 * multi-axis). Shared source of truth for the platform-prefix gate in
 * `inferDirection` and the off-axis filter (Phase 4).
 */
export type CompassAxis = 'EastWest' | 'NorthSouth';

/** Strict compass axis for a line, or `null` when it spans multiple axes. */
export function lineCompassAxis(lineId: string): CompassAxis | null {
  switch (lineId) {
    case 'hammersmith-city':
    case 'circle':
    case 'waterloo-city':
    case 'central':
    case 'elizabeth':
    case 'elizabeth-line':
      return 'EastWest';
    case 'bakerloo':
    case 'victoria':
      return 'NorthSouth';
    default:
      return null;
  }
}

/**
 * Whether a line legitimately runs north-south somewhere on the network (so a
 * `"Northbound"`/`"Southbound"` platform prefix can be trusted). Lines that
 * are east-west everywhere return `false` — an N/S prefix on one of them is a
 * TfL data quirk (a starter train on a different line's platform).
 */
function lineAllowsNorthSouth(lineId: string): boolean {
  switch (lineId) {
    case 'hammersmith-city':
    case 'circle':
    case 'waterloo-city':
    case 'central':
    case 'elizabeth':
    case 'elizabeth-line':
      return false;
    default:
      return true;
  }
}

/** Mirror of `lineAllowsNorthSouth` for the east-west axis. */
function lineAllowsEastWest(lineId: string): boolean {
  return lineId !== 'bakerloo' && lineId !== 'victoria';
}

/** Infer the Northern-line branch from the `towards` `"via …"` suffix. */
function inferNorthernBranch(towards: string): NorthernBranch | null {
  const lower = towards.toLowerCase();
  if (lower.includes('via bank')) return 'Bank';
  if (lower.includes('via cx') || lower.includes('via charing cross')) return 'CharingCross';
  return null;
}

/**
 * Infer a compass `Direction` from `(lineId, towards)` for the lines TfL labels
 * with bare `"Platform N"` (Elizabeth + the six named Overground lines) plus
 * the uniformly-oriented tube lines. Ambiguous termini return `null` and fall
 * back to the raw inbound/outbound — better than guessing wrong. (Invariant
 * #23.) DLR is intentionally absent (multi-branch topology).
 */
export function inferCompassFromTowards(lineId: string, towards: string): Direction | null {
  // Rust uses `to_ascii_lowercase`; `toLowerCase()` is equivalent here because
  // TfL `towards` / `platformName` values are ASCII-only in practice (no
  // diacritics in "Edgware via CX", "Harrow & Wealdstone", …).
  const lower = towards.toLowerCase();
  if (lower.trim().length === 0) return null;
  const any = (needles: string[]): boolean => needles.some((n) => lower.includes(n));

  switch (lineId) {
    case 'elizabeth':
    case 'elizabeth-line':
      if (any(['abbey wood', 'shenfield', 'stratford', 'gidea park', 'romford'])) {
        return 'Eastbound';
      }
      if (
        any([
          'paddington',
          'heathrow',
          'reading',
          'maidenhead',
          'hayes',
          'west drayton',
          'west ealing',
          'ealing broadway',
        ])
      ) {
        return 'Westbound';
      }
      return null;

    case 'mildmay':
      if (lower.includes('stratford')) return 'Eastbound';
      if (any(['richmond', 'clapham junction'])) return 'Westbound';
      return null;

    case 'lioness':
      if (lower.includes('watford')) return 'Northbound';
      if (lower.includes('euston')) return 'Southbound';
      return null;

    case 'suffragette':
      if (lower.includes('barking')) return 'Eastbound';
      if (lower.includes('gospel oak')) return 'Westbound';
      return null;

    case 'weaver':
      if (any(['cheshunt', 'enfield town', 'chingford'])) return 'Northbound';
      if (lower.includes('liverpool street')) return 'Southbound';
      return null;

    case 'windrush':
      if (any(['highbury', 'dalston'])) return 'Northbound';
      if (any(['new cross', 'crystal palace', 'west croydon', 'clapham junction'])) {
        return 'Southbound';
      }
      return null;

    case 'liberty':
      if (lower.includes('upminster')) return 'Eastbound';
      if (lower.includes('romford')) return 'Westbound';
      return null;

    case 'hammersmith-city':
      if (any(['barking', 'plaistow', 'east ham', 'whitechapel'])) return 'Eastbound';
      if (any(['hammersmith', 'edgware road'])) return 'Westbound';
      return null;

    case 'circle':
      if (any(['aldgate', 'tower hill', 'liverpool street'])) return 'Eastbound';
      if (any(['hammersmith', 'edgware road', 'paddington'])) return 'Westbound';
      return null;

    case 'waterloo-city':
      if (lower.includes('bank')) return 'Eastbound';
      if (lower.includes('waterloo')) return 'Westbound';
      return null;

    case 'bakerloo':
      if (
        any([
          'harrow & wealdstone',
          'harrow and wealdstone',
          'stonebridge park',
          "queen's park",
          'queens park',
          'willesden junction',
        ])
      ) {
        return 'Northbound';
      }
      if (lower.includes('elephant')) return 'Southbound';
      return null;

    default:
      return null;
  }
}

/**
 * Infer `(Direction, NorthernBranch | null)` from a TfL prediction's fields.
 *
 * Priority (exact, from `direction.rs`):
 *  1. `platformName` compass prefix, gated by the line's allowed axis.
 *  2. Per-line `towards`→compass mapping, falling back to `destinationName`
 *     when `towards` is empty (TfL leaves it blank for many Elizabeth /
 *     Overground hub predictions).
 *  3. Raw `direction` field (`"inbound"` / `"outbound"`).
 *  4. `Unknown`.
 *
 * The branch is derived from the `towards` suffix (Northern line only).
 */
export function inferDirection(
  platformName: string,
  direction: string,
  lineId: string,
  towards: string,
  destinationName: string,
): [Direction, NorthernBranch | null] {
  const platformLower = platformName.toLowerCase();
  const northernBranch = lineId === 'northern' ? inferNorthernBranch(towards) : null;

  const allowNs = lineAllowsNorthSouth(lineId);
  const allowEw = lineAllowsEastWest(lineId);

  const towardsCompass =
    inferCompassFromTowards(lineId, towards) ?? inferCompassFromTowards(lineId, destinationName);

  let dir: Direction;
  if (platformLower.startsWith('northbound') && allowNs) {
    dir = 'Northbound';
  } else if (platformLower.startsWith('southbound') && allowNs) {
    dir = 'Southbound';
  } else if (platformLower.startsWith('eastbound') && allowEw) {
    dir = 'Eastbound';
  } else if (platformLower.startsWith('westbound') && allowEw) {
    dir = 'Westbound';
  } else if (towardsCompass !== null) {
    dir = towardsCompass;
  } else if (direction === 'inbound') {
    dir = 'Inbound';
  } else if (direction === 'outbound') {
    dir = 'Outbound';
  } else {
    dir = 'Unknown';
  }

  return [dir, northernBranch];
}
