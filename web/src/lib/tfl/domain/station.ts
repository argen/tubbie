/**
 * `parseStation` — TfL StopPoint JSON (camelCase) → domain `Station`, ported
 * from the `Deserialize` impl in `crates/tfl-domain/src/types.rs`.
 *
 * TfL's stop-points response has no ready-made `lines` array; it emits
 * `lineModeGroups` grouped by mode. We project the supported-mode groups into
 * `lines`, filtered through `isSupportedLineId` so bus routes and national-rail
 * operators (which share the hub's list) never reach the chip UI. A pre-built
 * `lines` array, when present, is used verbatim (back-compat with inline-JSON
 * fixtures). `hub_naptan_code` is carried so the arrivals layer can query the
 * hub rather than the tube child.
 */
import type { LineRef, Station } from '$lib/ipc/types.js';
import { isSupportedLineId, prettyLineName } from './lines.js';
import { isRecord, rArray, rNumber, rString } from './raw.js';

const SUPPORTED_MODE_NAMES: ReadonlySet<string> = new Set([
  'tube',
  'dlr',
  'overground',
  'elizabeth-line',
]);

/** Parse one raw TfL stop-point. Missing/wrong-typed fields default safely. */
export function parseStation(raw: unknown): Station {
  const rec = isRecord(raw) ? raw : {};

  // Branch on whether the wire carried a `lines` array AT ALL (matching the
  // Rust `if !raw.lines.is_empty()` — the decision is made before filtering).
  // A present-but-all-unsupported `lines` array yields empty lines and does
  // NOT fall through to lineModeGroups, exactly as the Rust impl does.
  const rawLines = rArray(rec, 'lines');
  const preBuilt = rawLines
    .map((l) => (isRecord(l) ? l : {}))
    .map((l): LineRef => ({ id: rString(l, 'id'), name: rString(l, 'name') }))
    .filter((l) => isSupportedLineId(l.id));

  let lines: LineRef[];
  if (rawLines.length > 0) {
    lines = preBuilt;
  } else {
    lines = rArray(rec, 'lineModeGroups')
      .map((g) => (isRecord(g) ? g : {}))
      // Accept supported-mode groups, plus groups with an absent/empty modeName
      // (our trimmed fixtures drop the field). Bus / coach / national-rail /
      // tram groups are dropped here so their ids never reach the whitelist.
      .filter((g) => {
        const mode = rString(g, 'modeName');
        return mode.length === 0 || SUPPORTED_MODE_NAMES.has(mode);
      })
      .flatMap((g) => rArray(g, 'lineIdentifier'))
      .filter((id): id is string => typeof id === 'string')
      .filter((id) => isSupportedLineId(id))
      .map((id): LineRef => ({ id, name: prettyLineName(id) }));
  }

  const station: Station = {
    id: rString(rec, 'id'),
    common_name: rString(rec, 'commonName'),
    modes: rArray(rec, 'modes').filter((m): m is string => typeof m === 'string'),
    lat: rNumber(rec, 'lat'),
    lon: rNumber(rec, 'lon'),
    lines,
  };

  const hub = rString(rec, 'hubNaptanCode');
  if (hub.length > 0) station.hub_naptan_code = hub;

  return station;
}
