/**
 * `parseArrival` — TfL `Prediction` JSON (camelCase) → domain `Arrival`,
 * ported from the hand-written `Deserialize` impl in
 * `crates/tfl-domain/src/types.rs`. The id is canonicalised
 * (`elizabeth-line`→`elizabeth`) and `direction`/`northern_branch` are enriched
 * via `inferDirection` at ingest, so every downstream consumer sees one stable
 * id and a compass direction.
 */
import type { Arrival } from '$lib/ipc/types.js';
import { inferDirection } from './direction.js';
import { canonicalizeLineId } from './lines.js';
import { isRecord, rNumber, rString } from './raw.js';

/** Parse one raw TfL prediction. Missing/wrong-typed fields default safely. */
export function parseArrival(raw: unknown): Arrival {
  const rec = isRecord(raw) ? raw : {};

  const lineId = canonicalizeLineId(rString(rec, 'lineId'));
  const platformName = rString(rec, 'platformName');
  const towards = rString(rec, 'towards');
  const destinationName = rString(rec, 'destinationName');
  const rawDirection = rString(rec, 'direction');

  const [direction, northernBranch] = inferDirection(
    platformName,
    rawDirection,
    lineId,
    towards,
    destinationName,
  );

  return {
    id: rString(rec, 'id'),
    station_name: rString(rec, 'stationName'),
    platform_name: platformName,
    line_id: lineId,
    line_name: rString(rec, 'lineName'),
    direction,
    northern_branch: northernBranch,
    destination_name: destinationName,
    towards,
    current_location: rString(rec, 'currentLocation'),
    time_to_station: rNumber(rec, 'timeToStation'),
    expected_arrival: rString(rec, 'expectedArrival'),
    naptan_id: rString(rec, 'naptanId'),
  };
}
