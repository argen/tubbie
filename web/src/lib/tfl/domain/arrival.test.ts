import { describe, expect, it } from 'vitest';
import { parseArrival } from '$lib/tfl/domain/arrival.js';
import { loadFixture } from '$lib/tfl/fixtures.js';

const VALID_DIRECTIONS = new Set([
  'Northbound',
  'Southbound',
  'Eastbound',
  'Westbound',
  'Inbound',
  'Outbound',
  'Unknown',
]);

describe('parseArrival — fixture parity', () => {
  it('parses every Bank prediction without throwing and enriches direction', () => {
    const raw = loadFixture('arrivals', '940GZZLUBNK') as unknown[];
    expect(raw.length).toBeGreaterThan(0);
    const parsed = raw.map(parseArrival);
    for (const a of parsed) {
      expect(VALID_DIRECTIONS.has(a.direction)).toBe(true);
      // Canonicalisation invariant: no mode-form id survives ingest.
      expect(a.line_id).not.toBe('elizabeth-line');
    }
    // The first Bank entry is a Westbound Central train (platform-prefix wins).
    const central = parsed.find(
      (a) => a.line_id === 'central' && a.platform_name.toLowerCase().startsWith('westbound'),
    );
    expect(central?.direction).toBe('Westbound');
    expect(central?.northern_branch).toBeNull();
  });

  it("derives the Northern 'via Bank' branch at King's Cross", () => {
    const raw = loadFixture('arrivals', '940GZZLUKSX') as unknown[];
    const parsed = raw.map(parseArrival);
    const viaBank = parsed.find(
      (a) => a.line_id === 'northern' && a.towards.toLowerCase().includes('via bank'),
    );
    expect(viaBank?.northern_branch).toBe('Bank');
    // Non-Northern arrivals never carry a branch.
    for (const a of parsed) {
      if (a.line_id !== 'northern') expect(a.northern_branch).toBeNull();
    }
  });
});

describe('parseArrival — units', () => {
  it('canonicalises the elizabeth-line id and maps the compass direction', () => {
    const a = parseArrival({
      id: '-1',
      stationName: 'Liverpool Street',
      platformName: 'Platform 5',
      lineId: 'elizabeth-line',
      lineName: 'Elizabeth line',
      direction: 'outbound',
      destinationName: 'Abbey Wood Rail Station',
      towards: 'Abbey Wood',
      timeToStation: 120,
      expectedArrival: '2026-06-12T10:00:00Z',
      naptanId: '940GZZLULVT',
    });
    expect(a.line_id).toBe('elizabeth');
    expect(a.direction).toBe('Eastbound');
    expect(a.time_to_station).toBe(120);
    expect(a.naptan_id).toBe('940GZZLULVT');
  });

  it('defaults missing fields safely instead of throwing', () => {
    const a = parseArrival({});
    expect(a.id).toBe('');
    expect(a.direction).toBe('Unknown');
    expect(a.time_to_station).toBe(0);
    expect(a.northern_branch).toBeNull();
  });
});
