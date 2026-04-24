import { describe, expect, it } from 'vitest';
import {
  formatTimeToStation,
  isDue,
  revealDuration,
  shortPlatformName,
  shortStationName,
  truncate,
} from '$lib/utils/format.js';

describe('formatTimeToStation', () => {
  it('returns "Due" for < 30 seconds', () => {
    expect(formatTimeToStation(0)).toBe('Due');
    expect(formatTimeToStation(29)).toBe('Due');
    expect(formatTimeToStation(5)).toBe('Due');
  });

  it('returns "1 min" for 30-89 seconds', () => {
    expect(formatTimeToStation(30)).toBe('1 min');
    expect(formatTimeToStation(89)).toBe('1 min');
    expect(formatTimeToStation(60)).toBe('1 min');
  });

  it('returns "N mins" for 90+ seconds', () => {
    expect(formatTimeToStation(90)).toBe('1 mins');
    expect(formatTimeToStation(120)).toBe('2 mins');
    expect(formatTimeToStation(300)).toBe('5 mins');
    expect(formatTimeToStation(599)).toBe('9 mins');
    expect(formatTimeToStation(600)).toBe('10 mins');
  });
});

describe('isDue', () => {
  it('returns true for < 30 seconds', () => {
    expect(isDue(0)).toBe(true);
    expect(isDue(29)).toBe(true);
  });

  it('returns false for >= 30 seconds', () => {
    expect(isDue(30)).toBe(false);
    expect(isDue(90)).toBe(false);
  });
});

describe('revealDuration', () => {
  it('returns 60ms per character', () => {
    expect(revealDuration('Hi')).toBe(120);
    expect(revealDuration('Hello')).toBe(300);
  });

  it('caps at 1500ms for long strings', () => {
    const longText = 'A'.repeat(100);
    expect(revealDuration(longText)).toBe(1500);
  });

  it('returns 0 for empty string', () => {
    expect(revealDuration('')).toBe(0);
  });

  it('caps exactly at 25 chars = 1500ms', () => {
    expect(revealDuration('A'.repeat(25))).toBe(1500);
  });
});

describe('shortPlatformName', () => {
  it('extracts direction before " - "', () => {
    expect(shortPlatformName('Northbound - Platform 1')).toBe('Northbound');
    expect(shortPlatformName('Southbound - Platform 2')).toBe('Southbound');
  });

  it('returns full name when no " - " separator', () => {
    expect(shortPlatformName('Platform 1')).toBe('Platform 1');
  });

  it('handles empty string', () => {
    expect(shortPlatformName('')).toBe('');
  });
});

describe('shortStationName', () => {
  it('strips the " Underground Station" suffix', () => {
    expect(shortStationName('Morden Underground Station')).toBe('Morden');
    expect(shortStationName('Belsize Park Underground Station')).toBe('Belsize Park');
  });

  it('is case-insensitive on the suffix', () => {
    expect(shortStationName('Morden underground station')).toBe('Morden');
    expect(shortStationName('Morden UNDERGROUND STATION')).toBe('Morden');
  });

  it('also strips " DLR Station" and " Rail Station"', () => {
    expect(shortStationName('Canary Wharf DLR Station')).toBe('Canary Wharf');
    expect(shortStationName('Stratford Rail Station')).toBe('Stratford');
  });

  it('does not strip when the suffix is in the middle of the name', () => {
    // Regression guard: "Battersea Power Station Underground Station" should
    // still drop the trailing suffix once, leaving "Battersea Power Station".
    expect(shortStationName('Battersea Power Station Underground Station')).toBe(
      'Battersea Power Station',
    );
  });

  it('returns the input unchanged when no suffix matches', () => {
    expect(shortStationName('Morden')).toBe('Morden');
    expect(shortStationName('Edgware')).toBe('Edgware');
    expect(shortStationName('')).toBe('');
  });

  it('returns the input unchanged when trimming would leave an empty name', () => {
    // Pathological: the whole string IS the suffix.
    expect(shortStationName('Underground Station')).toBe('Underground Station');
  });
});

describe('truncate', () => {
  it('returns unchanged string when within maxLen', () => {
    expect(truncate('Hello', 10)).toBe('Hello');
    expect(truncate('Hello', 5)).toBe('Hello');
  });

  it('truncates with ellipsis when over maxLen', () => {
    expect(truncate('Hello World', 8)).toBe('Hello W…');
    expect(truncate('Abcdefghij', 5)).toBe('Abcd…');
  });
});
