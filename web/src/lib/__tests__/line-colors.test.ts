import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/**
 * Cross-platform line-colour contract (Phase 5).
 *
 * Mac (app.css `--line-*`) and iOS (SharedLineColors.swift) hand-maintain the
 * SAME TfL Colour Standard. This test pins the Mac CSS block to the canonical
 * set so a stray edit on the Mac side can't silently drift away from iOS. The
 * map below is the canonical TfL hex per line id — keep it byte-identical to
 * `tubbie-ios .../TubbieShared/SharedLineColors.swift` (`allCanonical`). The
 * source of both is https://content.tfl.gov.uk/tfl-colour-standard.pdf.
 *
 * Keyed by the CSS var SUFFIX (`--line-<suffix>`); Overground uses the legacy
 * `overground` suffix on the Mac side (iOS canonical id is "london-overground"
 * with the same value).
 */
const CANONICAL: Record<string, string> = {
  bakerloo: '#b36305',
  central: '#e32017',
  circle: '#ffd300',
  district: '#00782a',
  elizabeth: '#6950a1',
  'hammersmith-city': '#f3a9bb',
  jubilee: '#a0a5a9',
  metropolitan: '#9b0056',
  northern: '#ffffff',
  piccadilly: '#003688',
  victoria: '#0098d4',
  'waterloo-city': '#95cdba',
  dlr: '#00a4a7',
  overground: '#ee7c0e',
  liberty: '#5d6062',
  lioness: '#fbc91b',
  mildmay: '#006fe6',
  suffragette: '#5bbd72',
  weaver: '#9b0058',
  windrush: '#dc241f',
};

function parseLineVars(): Record<string, string> {
  const cssPath = fileURLToPath(new URL('../../app.css', import.meta.url));
  const css = readFileSync(cssPath, 'utf8');
  const out: Record<string, string> = {};
  const re = /--line-([a-z-]+):\s*(#[0-9a-fA-F]{6})/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(css)) !== null) {
    const id = m[1];
    const hex = m[2];
    if (id !== undefined && hex !== undefined) {
      out[id] = hex.toLowerCase();
    }
  }
  return out;
}

describe('line colours — cross-platform contract', () => {
  const parsed = parseLineVars();

  it('app.css defines exactly the canonical set of line colours', () => {
    expect(Object.keys(parsed).sort()).toEqual(Object.keys(CANONICAL).sort());
  });

  for (const [id, hex] of Object.entries(CANONICAL)) {
    it(`--line-${id} is ${hex} (matches iOS SharedLineColors)`, () => {
      expect(parsed[id]).toBe(hex);
    });
  }
});
