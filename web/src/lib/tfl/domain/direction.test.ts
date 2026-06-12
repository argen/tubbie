import { describe, expect, it } from 'vitest';
import type { Direction } from '$lib/ipc/types.js';
import { inferDirection, lineCompassAxis } from '$lib/tfl/domain/direction.js';

// Ported from crates/tfl-domain/tests/compass_from_towards.rs (invariant #23)
// plus the priority-tier and off-axis cases from direction.rs.

/** Mirror of the Rust test helper `dir` (empty destination). */
function dir(platform: string, rawDirection: string, lineId: string, towards: string): Direction {
  return inferDirection(platform, rawDirection, lineId, towards, '')[0];
}

/** Mirror of `dir_with_dest` — exercises the destinationName fallback. */
function dirWithDest(
  platform: string,
  rawDirection: string,
  lineId: string,
  towards: string,
  destination: string,
): Direction {
  return inferDirection(platform, rawDirection, lineId, towards, destination)[0];
}

describe('destinationName fallback (towards empty)', () => {
  it('elizabeth eastbound from destination when towards empty', () => {
    expect(dirWithDest('B', 'inbound', 'elizabeth', '', 'Gidea Park Rail Station')).toBe(
      'Eastbound',
    );
  });
  it('elizabeth westbound from destination when towards empty', () => {
    expect(dirWithDest('B', '', 'elizabeth', '', 'Heathrow Terminal 4 Rail Station')).toBe(
      'Westbound',
    );
  });
  it('elizabeth eastbound to Shenfield via destination', () => {
    expect(dirWithDest('B', 'outbound', 'elizabeth', '', 'Shenfield Rail Station')).toBe(
      'Eastbound',
    );
  });
  it('non-empty towards is not overridden by a misleading destination', () => {
    expect(
      dirWithDest('B', '', 'elizabeth', 'Heathrow Terminal 4', 'Abbey Wood Rail Station'),
    ).toBe('Westbound');
  });
});

describe('Elizabeth — east/west', () => {
  it.each([
    ['Abbey Wood', 'Eastbound'],
    ['Shenfield', 'Eastbound'],
    ['Stratford', 'Eastbound'],
  ] as [string, Direction][])('eastbound to %s', (towards, expected) => {
    expect(dir('Platform 5', 'outbound', 'elizabeth', towards)).toBe(expected);
  });

  it.each([
    ['Paddington', 'Westbound'],
    ['Heathrow Terminal 5', 'Westbound'],
    ['Reading', 'Westbound'],
    ['Hayes & Harlington', 'Westbound'],
  ] as [string, Direction][])('westbound to %s', (towards, expected) => {
    expect(dir('Platform 4', 'inbound', 'elizabeth', towards)).toBe(expected);
  });

  it('accepts the elizabeth-line mode form', () => {
    expect(dir('Platform 5', 'outbound', 'elizabeth-line', 'Abbey Wood')).toBe('Eastbound');
    expect(dir('Platform 4', 'inbound', 'elizabeth-line', 'Heathrow Terminal 4')).toBe('Westbound');
  });

  it('falls back to raw direction for an unknown terminus', () => {
    expect(dir('Platform 3', 'inbound', 'elizabeth', 'Network Rail Sidings')).toBe('Inbound');
  });
});

describe('named Overground lines', () => {
  it('mildmay east/west', () => {
    expect(dir('Platform 1', 'outbound', 'mildmay', 'Stratford')).toBe('Eastbound');
    expect(dir('Platform 2', 'inbound', 'mildmay', 'Richmond')).toBe('Westbound');
    expect(dir('Platform 2', 'inbound', 'mildmay', 'Clapham Junction')).toBe('Westbound');
  });
  it('lioness north/south', () => {
    expect(dir('Platform 9', 'outbound', 'lioness', 'Watford Junction')).toBe('Northbound');
    expect(dir('Platform 8', 'inbound', 'lioness', 'Euston')).toBe('Southbound');
  });
  it('suffragette east/west', () => {
    expect(dir('Platform 1', 'outbound', 'suffragette', 'Barking Riverside')).toBe('Eastbound');
    expect(dir('Platform 2', 'inbound', 'suffragette', 'Gospel Oak')).toBe('Westbound');
  });
  it('weaver north/south', () => {
    expect(dir('Platform 1', 'outbound', 'weaver', 'Chingford')).toBe('Northbound');
    expect(dir('Platform 1', 'outbound', 'weaver', 'Enfield Town')).toBe('Northbound');
    expect(dir('Platform 2', 'inbound', 'weaver', 'Liverpool Street')).toBe('Southbound');
  });
  it('windrush north/south (Clapham Junction disambiguated from Mildmay)', () => {
    expect(dir('Platform 1', 'outbound', 'windrush', 'Highbury & Islington')).toBe('Northbound');
    expect(dir('Platform 1', 'outbound', 'windrush', 'Dalston Junction')).toBe('Northbound');
    expect(dir('Platform 2', 'inbound', 'windrush', 'New Cross')).toBe('Southbound');
    expect(dir('Platform 2', 'inbound', 'windrush', 'Crystal Palace')).toBe('Southbound');
    expect(dir('Platform 2', 'inbound', 'windrush', 'Clapham Junction')).toBe('Southbound');
  });
  it('liberty east/west', () => {
    expect(dir('Platform 1', 'outbound', 'liberty', 'Upminster')).toBe('Eastbound');
    expect(dir('Platform 2', 'inbound', 'liberty', 'Romford')).toBe('Westbound');
  });
});

describe('tube + edge cases', () => {
  it('tube platform prefix wins over towards', () => {
    expect(dir('Eastbound - Platform 6', 'outbound', 'central', 'Stratford')).toBe('Eastbound');
  });
  it('DLR is unmapped — raw direction drives the result', () => {
    expect(dir('Platform 3', 'inbound', 'dlr', 'Bank')).toBe('Inbound');
  });
  it('empty towards falls back to raw direction, not Unknown', () => {
    expect(dir('Platform 3', 'inbound', 'elizabeth', '')).toBe('Inbound');
  });
  it('matches termini case-insensitively', () => {
    expect(dir('Platform 5', 'outbound', 'elizabeth', 'ABBEY WOOD')).toBe('Eastbound');
    expect(dir('Platform 5', 'outbound', 'elizabeth', 'abbey wood')).toBe('Eastbound');
  });
});

describe('per-line compass-axis gate on the platform prefix', () => {
  it('H&C rejects a Northbound prefix (east-west line) and falls through', () => {
    expect(
      dir('Northbound - Platform 4', 'outbound', 'hammersmith-city', 'Check Front of Train'),
    ).toBe('Outbound');
  });
  it('H&C keeps a valid Westbound prefix', () => {
    expect(dir('Westbound - Platform 6', 'inbound', 'hammersmith-city', 'Hammersmith')).toBe(
      'Westbound',
    );
  });
  it('H&C towards-mapping resolves without a prefix', () => {
    expect(dir('Platform 6', 'inbound', 'hammersmith-city', 'Hammersmith')).toBe('Westbound');
    expect(dir('Platform 5', 'outbound', 'hammersmith-city', 'Barking')).toBe('Eastbound');
  });
  it('Circle rejects a Southbound prefix', () => {
    expect(dir('Southbound - Platform 1', 'inbound', 'circle', 'Check Front of Train')).toBe(
      'Inbound',
    );
  });
  it('W&C rejects a wrong-axis prefix, recovers via towards (Bank → Eastbound)', () => {
    expect(dir('Northbound - Platform 1', 'outbound', 'waterloo-city', 'Bank')).toBe('Eastbound');
  });
  it('Bakerloo rejects an Eastbound prefix, recovers via towards', () => {
    expect(dir('Eastbound - Platform 1', 'inbound', 'bakerloo', 'Harrow & Wealdstone')).toBe(
      'Northbound',
    );
  });
  it('Bakerloo keeps a valid Northbound prefix', () => {
    expect(dir('Northbound - Platform 4', 'inbound', 'bakerloo', 'Harrow & Wealdstone')).toBe(
      'Northbound',
    );
  });
  it('Metropolitan keeps its Northbound prefix (multi-axis line, not gated)', () => {
    expect(dir('Northbound - Platform 4', 'inbound', 'metropolitan', 'Amersham')).toBe(
      'Northbound',
    );
  });
  it('Jubilee keeps its Eastbound prefix (multi-axis line, not gated)', () => {
    expect(dir('Eastbound - Platform 14', 'inbound', 'jubilee', 'Stratford')).toBe('Eastbound');
  });
});

describe('Northern-line branch', () => {
  /** Branch component of inferDirection for a Northern train. */
  function branch(towards: string): 'Bank' | 'CharingCross' | null {
    return inferDirection('Platform 1', 'inbound', 'northern', towards, '')[1];
  }

  it('derives Bank from "via Bank"', () => {
    expect(branch('Morden via Bank')).toBe('Bank');
    expect(branch('High Barnet via Bank')).toBe('Bank');
  });
  it('derives Charing Cross from both the abbreviated and full forms', () => {
    expect(branch('Edgware via CX')).toBe('CharingCross');
    expect(branch('Battersea via CX')).toBe('CharingCross');
    // Full spelling must map too — a regression here would silently null the branch.
    expect(branch('Edgware via Charing Cross')).toBe('CharingCross');
  });
  it('is null for a Northern destination with no "via" suffix', () => {
    expect(branch('High Barnet')).toBeNull();
    expect(branch('Kennington')).toBeNull();
  });
  it('is null for non-Northern lines even with a "via" string', () => {
    expect(inferDirection('Platform 5', 'outbound', 'elizabeth', 'Abbey Wood', '')[1]).toBeNull();
    expect(
      inferDirection('Platform 1', 'inbound', 'victoria', 'Brixton via Bank', '')[1],
    ).toBeNull();
  });
  it('resolves direction from the platform prefix even when raw direction conflicts', () => {
    // "Northbound - Platform 4" with raw direction "inbound": prefix wins
    // (Northern allows N/S), branch still derived from towards.
    const [dir, br] = inferDirection(
      'Northbound - Platform 4',
      'inbound',
      'northern',
      'High Barnet via Bank',
      '',
    );
    expect(dir).toBe('Northbound');
    expect(br).toBe('Bank');
  });
});

// Per-needle coverage so a future typo in a towards-terminus string fails a TS
// test (as it would the Rust suite), not just silently mis-buckets live data.
describe('inferCompassFromTowards — per-terminus needles', () => {
  const cases: [string, string, Direction][] = [
    // Elizabeth westbound short-workings / termini
    ['elizabeth', 'Maidenhead', 'Westbound'],
    ['elizabeth', 'West Drayton', 'Westbound'],
    ['elizabeth', 'West Ealing', 'Westbound'],
    ['elizabeth', 'Ealing Broadway', 'Westbound'],
    ['elizabeth', 'Gidea Park', 'Eastbound'],
    ['elizabeth', 'Romford', 'Eastbound'],
    // Circle
    ['circle', 'Aldgate', 'Eastbound'],
    ['circle', 'Tower Hill', 'Eastbound'],
    ['circle', 'Edgware Road', 'Westbound'],
    ['circle', 'Paddington', 'Westbound'],
    // Hammersmith & City eastbound short-workings
    ['hammersmith-city', 'Plaistow', 'Eastbound'],
    ['hammersmith-city', 'East Ham', 'Eastbound'],
    ['hammersmith-city', 'Whitechapel', 'Eastbound'],
    ['hammersmith-city', 'Edgware Road', 'Westbound'],
    // Bakerloo northbound short-workings
    ['bakerloo', 'Stonebridge Park', 'Northbound'],
    ['bakerloo', 'Willesden Junction', 'Northbound'],
    ['bakerloo', "Queen's Park", 'Northbound'],
    ['bakerloo', 'Harrow and Wealdstone', 'Northbound'],
    ['bakerloo', 'Elephant & Castle', 'Southbound'],
    // Waterloo & City
    ['waterloo-city', 'Bank', 'Eastbound'],
    ['waterloo-city', 'Waterloo', 'Westbound'],
  ];
  it.each(cases)('%s towards %s → %s', (lineId, towards, expected) => {
    // Bare "Platform N" so the prefix branch never fires — isolates the towards table.
    expect(dir('Platform 7', 'inbound', lineId, towards)).toBe(expected);
  });
});

describe('lineCompassAxis', () => {
  it('pins uniformly-oriented lines and returns null for multi-axis lines', () => {
    expect(lineCompassAxis('central')).toBe('EastWest');
    expect(lineCompassAxis('elizabeth-line')).toBe('EastWest');
    expect(lineCompassAxis('victoria')).toBe('NorthSouth');
    expect(lineCompassAxis('bakerloo')).toBe('NorthSouth');
    expect(lineCompassAxis('metropolitan')).toBeNull();
    expect(lineCompassAxis('dlr')).toBeNull();
  });
});
