import { describe, expect, it } from 'vitest';
import { parseStation } from '$lib/tfl/domain/station.js';
import { loadFixture } from '$lib/tfl/fixtures.js';

function tubeStation(id: string): unknown {
  const data = loadFixture('stop-points', 'tube') as { stopPoints: { id: string }[] };
  return data.stopPoints.find((s) => s.id === id);
}

describe('parseStation — fixture parity', () => {
  it('projects lineModeGroups into supported lines (modeName absent)', () => {
    // Belsize Park: lineModeGroups = [{ lineIdentifier: ["northern"] }].
    const station = parseStation(tubeStation('9400ZZBPSUST'));
    expect(station.lines).toEqual([{ id: 'northern', name: 'Northern' }]);
  });

  it('carries hub_naptan_code and yields empty lines for a trimmed station', () => {
    // Amersham: hubNaptanCode HUBAMR, lineModeGroups = [].
    const station = parseStation(tubeStation('0400ZZLUAMS0'));
    expect(station.hub_naptan_code).toBe('HUBAMR');
    expect(station.lines).toEqual([]);
    expect(station.modes.length).toBeGreaterThan(0);
  });
});

describe('parseStation — whitelist + units', () => {
  it('drops bus routes and national-rail operators from a mixed hub group', () => {
    const station = parseStation({
      id: '940GZZLUVIC',
      commonName: 'Victoria',
      modes: ['tube'],
      lineModeGroups: [
        { lineIdentifier: ['victoria', '52', '390', 'gatwick-express', 'district'] },
      ],
    });
    expect(station.lines.map((l) => l.id)).toEqual(['victoria', 'district']);
  });

  it('drops entire groups for unsupported modes', () => {
    const station = parseStation({
      id: 'X',
      commonName: 'X',
      lineModeGroups: [
        { modeName: 'national-rail', lineIdentifier: ['thameslink'] },
        { modeName: 'tube', lineIdentifier: ['central'] },
      ],
    });
    expect(station.lines.map((l) => l.id)).toEqual(['central']);
  });

  it('uses a pre-built lines array verbatim, still whitelisted', () => {
    const station = parseStation({
      id: 'X',
      commonName: 'X',
      lines: [
        { id: 'central', name: 'Central' },
        { id: '52', name: '52' },
      ],
    });
    expect(station.lines).toEqual([{ id: 'central', name: 'Central' }]);
  });

  it('a present-but-all-unsupported lines array yields empty (no fall-through)', () => {
    // Matches Rust `if !raw.lines.is_empty()`: the branch is chosen on raw
    // presence, so lineModeGroups is NOT consulted even though it has a
    // supported line. Filtering the unsupported entries leaves [].
    const station = parseStation({
      id: 'X',
      commonName: 'X',
      lines: [{ id: '52', name: '52' }],
      lineModeGroups: [{ lineIdentifier: ['central'] }],
    });
    expect(station.lines).toEqual([]);
  });

  it('omits hub_naptan_code when empty', () => {
    const station = parseStation({ id: 'X', commonName: 'X', hubNaptanCode: '' });
    expect(station.hub_naptan_code).toBeUndefined();
  });
});
