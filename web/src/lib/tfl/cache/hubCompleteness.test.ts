/**
 * Multi-mode hub completeness — the TS arm of the permanent regression harness.
 *
 * Drives the 10 scenarios in the shared `tests/fixtures/hub-vectors.json` (the
 * same single source of truth the Rust `multi_mode_hub_completeness_tests` and
 * the iOS `HubVectorTests` consume): build hermetic synthetic fixtures, warm the
 * cache, and assert `allowedLineIdsFor(station_id)` is a superset of the expected
 * lines (positive) or exactly equal (the Belsize-Park no-over-merge pin). Also
 * pins that the 9 positive scenarios agree with `CANONICAL_MULTI_MODE_HUBS`.
 *
 * If a hub-merge fix is reverted, a mode filter breaks, or a line id is
 * mis-canonicalised, the matching scenario goes red — exactly as on the Rust side.
 */

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeClock } from '../transport/clock.js';
import { RecordHttp } from './recordHttp.js';
import { CANONICAL_MULTI_MODE_HUBS, TflClient } from './tflClient.js';
import { seedModes, synthHubDetail, synthStation } from './synth.js';

interface Scenario {
  id: string;
  station_id: string;
  hub_id: string | null;
  expected_lines: string[];
  negative: boolean;
}

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../..');
const vectors = JSON.parse(
  readFileSync(resolve(REPO_ROOT, 'tests/fixtures/hub-vectors.json'), 'utf-8'),
) as { version: number; scenarios: Scenario[] };

const OVERGROUND_NAMED = new Set([
  'liberty',
  'lioness',
  'mildmay',
  'suffragette',
  'weaver',
  'windrush',
  'london-overground',
]);

/** The TfL mode that carries a given line (for building synthetic hub children). */
function modeForLine(line: string): string {
  if (line === 'dlr') return 'dlr';
  if (line === 'elizabeth' || line === 'elizabeth-line') return 'elizabeth-line';
  return OVERGROUND_NAMED.has(line) ? 'overground' : 'tube';
}

/** One synthetic hub child per mode, carrying that mode's expected lines. */
function childrenForLines(lines: readonly string[]): {
  id: string;
  modes: string[];
  lines: string[];
}[] {
  const byMode = new Map<string, string[]>();
  for (const line of lines) {
    const list = byMode.get(modeForLine(line)) ?? [];
    list.push(line);
    byMode.set(modeForLine(line), list);
  }
  return [...byMode].map(([mode, ls]) => ({ id: `child-${mode}`, modes: [mode], lines: ls }));
}

const noop = (): Promise<void> => Promise.resolve();

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe('hub-vectors.json ↔ CANONICAL_MULTI_MODE_HUBS consistency', () => {
  it('the positive scenarios agree with the const (station id + expected lines)', () => {
    const positives = vectors.scenarios.filter((s) => !s.negative);
    expect(positives).toHaveLength(CANONICAL_MULTI_MODE_HUBS.length);

    const constById = new Map(
      CANONICAL_MULTI_MODE_HUBS.map((h) => [h.stationId, [...h.expectedLines].sort()]),
    );
    for (const scenario of positives) {
      expect(constById.get(scenario.station_id)).toEqual([...scenario.expected_lines].sort());
    }
  });
});

describe('multi-mode hub completeness', () => {
  for (const scenario of vectors.scenarios) {
    it(scenario.id, async () => {
      const http = new RecordHttp();

      if (scenario.negative || scenario.hub_id === null) {
        // Hubless station carries exactly its own lines; a decoy hub in the same
        // warm carries unrelated DLR/Elizabeth lines it must NOT inherit.
        seedModes(http, {
          tube: [
            synthStation(scenario.station_id, { modes: ['tube'], lines: scenario.expected_lines }),
            synthStation('940GZZLUDECOY', { modes: ['tube'], lines: ['central'], hub: 'HUBDECOY' }),
          ],
        });
        http.put(
          'stop-point',
          'HUBDECOY',
          synthHubDetail([{ id: 'decoy-child', modes: ['dlr'], lines: ['dlr', 'elizabeth'] }]),
        );

        const c = new TflClient(http, {
          clock: FakeClock.at(new Date('2026-01-01T00:00:00Z')),
          sleep: noop,
        });
        await c.warmStopPointsCache();

        expect([...c.allowedLineIdsFor(scenario.station_id)].sort()).toEqual(
          [...scenario.expected_lines].sort(),
        );
        return;
      }

      // Positive: tube parent carries its tube lines directly; the hub detail
      // supplies the full set (the non-tube siblings, plus the tube ones — the
      // merge dedupes). Exercises both the parent-lines and hub-merge paths.
      const tubeLines = scenario.expected_lines.filter((l) => modeForLine(l) === 'tube');
      seedModes(http, {
        tube: [
          synthStation(scenario.station_id, {
            modes: ['tube'],
            lines: tubeLines,
            hub: scenario.hub_id,
          }),
        ],
      });
      http.put(
        'stop-point',
        scenario.hub_id,
        synthHubDetail(childrenForLines(scenario.expected_lines)),
      );

      const c = new TflClient(http, {
        clock: FakeClock.at(new Date('2026-01-01T00:00:00Z')),
        sleep: noop,
      });
      await c.warmStopPointsCache();

      const got = c.allowedLineIdsFor(scenario.station_id);
      for (const line of scenario.expected_lines) {
        expect(got.has(line), `${scenario.station_id} should serve ${line}`).toBe(true);
      }
    });
  }
});
