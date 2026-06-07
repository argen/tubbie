import { describe, it, expect } from 'vitest';
import type { LineStatus, SeverityBucket } from '$lib/ipc/types.js';
import {
  worstBucket,
  isDisrupted,
  sortLinesWorstFirst,
  allGoodService,
  disruptedLinesWorstFirst,
  anyDisrupted,
  bucketRank,
} from '$lib/utils/status.js';

function line(line_id: string, buckets: SeverityBucket[]): LineStatus {
  return {
    line_id,
    disruption_text: buckets.some((b) => b !== 'GoodService') ? 'something' : null,
    status: buckets.map((bucket, i) => ({ severity: i, description: bucket, bucket })),
  };
}

describe('worstBucket', () => {
  it('returns GoodService for a line with no status entries', () => {
    expect(worstBucket(line('victoria', []))).toBe('GoodService');
  });

  it('picks the most severe bucket among entries', () => {
    expect(worstBucket(line('northern', ['MinorDelays', 'SevereDelays', 'GoodService']))).toBe(
      'SevereDelays',
    );
  });

  it('falls back to Other when an entry has no bucket', () => {
    const l: LineStatus = {
      line_id: 'x',
      disruption_text: 'legacy',
      status: [{ severity: 9, description: 'Minor Delays' }], // no bucket (legacy payload)
    };
    expect(worstBucket(l)).toBe('Other');
  });
});

describe('isDisrupted', () => {
  it('is false for good service', () => {
    expect(isDisrupted(line('victoria', ['GoodService']))).toBe(false);
  });
  it('is true for any non-good bucket', () => {
    expect(isDisrupted(line('central', ['MinorDelays']))).toBe(true);
  });
});

describe('sortLinesWorstFirst', () => {
  it('orders most-severe first, then alphabetical for ties', () => {
    const input = [
      line('victoria', ['GoodService']),
      line('central', ['MinorDelays']),
      line('bakerloo', ['Closed']),
      line('northern', ['MinorDelays']),
    ];
    const out = sortLinesWorstFirst(input).map((l) => l.line_id);
    // Closed (worst) → the two MinorDelays (central before northern) → GoodService.
    expect(out).toEqual(['bakerloo', 'central', 'northern', 'victoria']);
  });

  it('does not mutate the input array', () => {
    const input = [line('b', ['Closed']), line('a', ['GoodService'])];
    const before = input.map((l) => l.line_id);
    sortLinesWorstFirst(input);
    expect(input.map((l) => l.line_id)).toEqual(before);
  });
});

describe('allGoodService', () => {
  it('true when every line is good (the calm empty state)', () => {
    expect(allGoodService([line('a', ['GoodService']), line('b', [])])).toBe(true);
  });
  it('false when any line is disrupted', () => {
    expect(allGoodService([line('a', ['GoodService']), line('b', ['SevereDelays'])])).toBe(false);
  });
});

describe('disruptedLinesWorstFirst', () => {
  it('keeps only disrupted lines, worst-first', () => {
    const input = [
      line('victoria', ['GoodService']),
      line('central', ['SevereDelays']),
      line('jubilee', ['MinorDelays']),
    ];
    expect(disruptedLinesWorstFirst(input).map((l) => l.line_id)).toEqual(['central', 'jubilee']);
  });
});

describe('anyDisrupted', () => {
  const lines = [line('victoria', ['GoodService']), line('bakerloo', ['SevereDelays'])];

  it('empty selection = all lines: true when any line is disrupted', () => {
    expect(anyDisrupted(lines, [])).toBe(true);
  });

  it('false when the disruption is on a line NOT in the selection', () => {
    // Only Victoria selected; Bakerloo (disrupted) is filtered out.
    expect(anyDisrupted(lines, ['victoria'])).toBe(false);
  });

  it('true when a disrupted line IS in the selection', () => {
    expect(anyDisrupted(lines, ['bakerloo'])).toBe(true);
  });

  it('false when everything in scope is good', () => {
    expect(anyDisrupted([line('victoria', ['GoodService'])], [])).toBe(false);
  });
});

describe('bucketRank', () => {
  it('orders worst < best', () => {
    expect(bucketRank('Closed')).toBeLessThan(bucketRank('MinorDelays'));
    expect(bucketRank('MinorDelays')).toBeLessThan(bucketRank('GoodService'));
  });
  it('treats undefined as Other', () => {
    expect(bucketRank(undefined)).toBe(bucketRank('Other'));
  });
});
