import { describe, expect, it } from 'vitest';
import { isRecord, rArray, rNumber, rString } from '$lib/tfl/domain/raw.js';

// These readers reproduce serde's `#[serde(default)]` "missing/wrong-typed →
// safe default" contract. The parsers (parseArrival/parseStation) lean on them
// to fail-soft on malformed TfL JSON, so the negative space matters.

describe('isRecord', () => {
  it('accepts plain objects only', () => {
    expect(isRecord({})).toBe(true);
    expect(isRecord({ a: 1 })).toBe(true);
  });
  it('rejects null, arrays, and primitives', () => {
    expect(isRecord(null)).toBe(false);
    expect(isRecord([])).toBe(false);
    expect(isRecord([1, 2])).toBe(false);
    expect(isRecord('x')).toBe(false);
    expect(isRecord(3)).toBe(false);
    expect(isRecord(undefined)).toBe(false);
  });
});

describe('rString', () => {
  it('returns the string when present', () => {
    expect(rString({ k: 'hi' }, 'k')).toBe('hi');
  });
  it('defaults to "" for missing or wrong-typed values', () => {
    expect(rString({}, 'k')).toBe('');
    expect(rString({ k: 42 }, 'k')).toBe('');
    expect(rString({ k: null }, 'k')).toBe('');
    expect(rString({ k: ['x'] }, 'k')).toBe('');
  });
});

describe('rNumber', () => {
  it('returns the number when present', () => {
    expect(rNumber({ k: 120 }, 'k')).toBe(120);
    expect(rNumber({ k: 0 }, 'k')).toBe(0);
  });
  it('defaults to 0 for missing or wrong-typed values', () => {
    expect(rNumber({}, 'k')).toBe(0);
    expect(rNumber({ k: '120' }, 'k')).toBe(0);
    expect(rNumber({ k: null }, 'k')).toBe(0);
  });
});

describe('rArray', () => {
  it('returns the array when present', () => {
    expect(rArray({ k: [1, 2] }, 'k')).toEqual([1, 2]);
    expect(rArray({ k: [] }, 'k')).toEqual([]);
  });
  it('defaults to [] for missing or wrong-typed values', () => {
    expect(rArray({}, 'k')).toEqual([]);
    expect(rArray({ k: null }, 'k')).toEqual([]);
    expect(rArray({ k: { a: 1 } }, 'k')).toEqual([]);
    expect(rArray({ k: 'nope' }, 'k')).toEqual([]);
  });
});
