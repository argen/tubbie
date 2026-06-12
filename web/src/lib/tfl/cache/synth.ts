/**
 * Synthetic TfL JSON builders for cache tests — the hermetic, in-memory analogue
 * of the Rust completeness harness's temp-file fixtures. Shared by the cache
 * test suites so the wire shapes live in one place. Test support only.
 */

import { SUPPORTED_MODES } from './tflClient.js';
import type { RecordHttp } from './recordHttp.js';

export interface SynthStationOpts {
  modes: readonly string[];
  lines: readonly string[];
  hub?: string;
  name?: string;
  lat?: number;
  lon?: number;
}

/** A stop-point JSON entry as `parseStation` reads it (`lineModeGroups` form). */
export function synthStation(id: string, opts: SynthStationOpts): unknown {
  return {
    id,
    commonName: opts.name ?? `${id} synthetic`,
    modes: opts.modes,
    ...(opts.hub !== undefined ? { hubNaptanCode: opts.hub } : {}),
    lat: opts.lat ?? 51.5,
    lon: opts.lon ?? -0.1,
    lineModeGroups: opts.modes.map((m) => ({ modeName: m, lineIdentifier: opts.lines })),
  };
}

/** Wrap stop-points in TfL's `{ total, stopPoints }` envelope. */
export function modeBody(stations: unknown[]): unknown {
  return { total: stations.length, stopPoints: stations };
}

/** A `/StopPoint/{HUB}` hub-detail JSON with the given children. */
export function synthHubDetail(
  children: readonly { id: string; modes: readonly string[]; lines: readonly string[] }[],
): unknown {
  return {
    children: children.map((c) => ({
      id: c.id,
      modes: c.modes,
      lineModeGroups: [{ modeName: c.modes[0] ?? '', lineIdentifier: c.lines }],
    })),
  };
}

/**
 * Register every surfaced mode's `stop-points` response, defaulting any mode not
 * in `perMode` to an empty list — an unregistered mode would otherwise resolve
 * to `NotFound` and be counted as a failed (partial) warm, which most tests
 * don't intend.
 */
export function seedModes(http: RecordHttp, perMode: Record<string, unknown[]>): void {
  for (const mode of SUPPORTED_MODES) {
    http.put('stop-points', mode, modeBody(perMode[mode] ?? []));
  }
}
