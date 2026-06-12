import { describe, expect, it } from 'vitest';
import type { SeverityBucket } from '$lib/ipc/types.js';
import { severityBucket, severityBucketSortRank } from '$lib/tfl/domain/status.js';

// Ported from crates/tfl-domain/tests/severity_bucket.rs (invariant #25).
describe('severityBucket', () => {
  const cases: [number, SeverityBucket][] = [
    [0, 'Other'],
    [1, 'Closed'],
    [2, 'Closed'],
    [3, 'PartClosure'],
    [4, 'PartClosure'],
    [5, 'PartClosure'],
    [6, 'SevereDelays'],
    [7, 'ReducedService'],
    [8, 'ReducedService'],
    [9, 'MinorDelays'],
    [10, 'GoodService'],
    [11, 'PartClosure'],
    [12, 'Other'],
    [13, 'Other'],
    [14, 'MinorDelays'],
    [15, 'ReducedService'],
    [16, 'Closed'],
    [17, 'Information'],
    [18, 'GoodService'],
    [19, 'Information'],
    [20, 'Closed'],
  ];

  it.each(cases)('maps code %i to %s', (code, bucket) => {
    expect(severityBucket(code)).toBe(bucket);
  });

  it('defaults out-of-range codes to Other', () => {
    expect(severityBucket(-1)).toBe('Other');
    expect(severityBucket(99)).toBe('Other');
  });
});

describe('severityBucketSortRank', () => {
  it('orders worst-first, Information then Other then GoodService last', () => {
    const order: SeverityBucket[] = [
      'Closed',
      'PartClosure',
      'SevereDelays',
      'ReducedService',
      'MinorDelays',
      'Information',
      'Other',
      'GoodService',
    ];
    for (let i = 0; i < order.length - 1; i++) {
      expect(severityBucketSortRank(order[i]!)).toBeLessThan(severityBucketSortRank(order[i + 1]!));
    }
  });

  it('ranks GoodService strictly last', () => {
    const good = severityBucketSortRank('GoodService');
    for (const b of [
      'Closed',
      'PartClosure',
      'SevereDelays',
      'ReducedService',
      'MinorDelays',
      'Information',
      'Other',
    ] as SeverityBucket[]) {
      expect(severityBucketSortRank(b)).toBeLessThan(good);
    }
  });
});
