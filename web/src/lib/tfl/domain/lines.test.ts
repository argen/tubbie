import { describe, expect, it } from 'vitest';
import {
  canonicalizeLineId,
  isSupportedLineId,
  lineFamilyKey,
  prettyLineName,
} from '$lib/tfl/domain/lines.js';

// Ported from crates/tfl-domain/tests/line_family_key.rs (invariant #10).
describe('lineFamilyKey', () => {
  it('groups the named Overground lines with the legacy id', () => {
    for (const id of [
      'london-overground',
      'liberty',
      'lioness',
      'mildmay',
      'suffragette',
      'weaver',
      'windrush',
    ]) {
      expect(lineFamilyKey(id)).toBe('london-overground');
    }
  });

  it('leaves non-Overground ids unchanged', () => {
    for (const id of [
      'northern',
      'victoria',
      'central',
      'bakerloo',
      'dlr',
      'elizabeth',
      'jubilee',
    ]) {
      expect(lineFamilyKey(id)).toBe(id);
    }
  });
});

describe('canonicalizeLineId', () => {
  it('folds the elizabeth-line mode form to the line form', () => {
    expect(canonicalizeLineId('elizabeth-line')).toBe('elizabeth');
  });

  it('leaves every other id unchanged', () => {
    for (const id of ['elizabeth', 'northern', 'dlr', 'windrush', 'london-overground']) {
      expect(canonicalizeLineId(id)).toBe(id);
    }
  });
});

describe('isSupportedLineId', () => {
  it('accepts tube, Elizabeth (both forms), DLR, and Overground (legacy + named)', () => {
    for (const id of [
      'bakerloo',
      'central',
      'circle',
      'district',
      'hammersmith-city',
      'jubilee',
      'metropolitan',
      'northern',
      'piccadilly',
      'victoria',
      'waterloo-city',
      'elizabeth',
      'elizabeth-line',
      'dlr',
      'london-overground',
      'liberty',
      'lioness',
      'mildmay',
      'suffragette',
      'weaver',
      'windrush',
    ]) {
      expect(isSupportedLineId(id)).toBe(true);
    }
  });

  it('rejects bus routes and national-rail operators', () => {
    for (const id of ['52', '390', 'gatwick-express', 'thameslink', 'southern', '']) {
      expect(isSupportedLineId(id)).toBe(false);
    }
  });
});

describe('prettyLineName (re-exported single source)', () => {
  it('maps known ids and falls back to the id', () => {
    expect(prettyLineName('hammersmith-city')).toBe('Hammersmith & City');
    expect(prettyLineName('elizabeth-line')).toBe('Elizabeth');
    expect(prettyLineName('windrush')).toBe('Windrush');
    expect(prettyLineName('unknown-id')).toBe('unknown-id');
  });
});
