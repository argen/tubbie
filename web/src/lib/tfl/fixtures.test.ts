import { describe, expect, it } from 'vitest';
import { loadFixture } from '$lib/tfl/fixtures.js';

describe('loadFixture', () => {
  it('reads and parses a repo-root stop-points fixture', () => {
    // fixtures/stop-points/tube.json is TfL's /StopPoint/Mode/tube response:
    // { $type, stopPoints: [...], total }. Proves the Node-fs path resolution
    // reaches the repo-root fixtures shared with the Rust crate tests.
    const data = loadFixture('stop-points', 'tube');
    expect(data).toBeTypeOf('object');
    const stopPoints = (data as { stopPoints?: unknown }).stopPoints;
    expect(Array.isArray(stopPoints)).toBe(true);
    expect((stopPoints as unknown[]).length).toBeGreaterThan(0);
  });

  it('reads an arrivals fixture by stop-point id', () => {
    const data = loadFixture('arrivals', '940GZZLUBNK');
    expect(Array.isArray(data)).toBe(true);
  });

  it('throws for a missing fixture rather than returning empty', () => {
    expect(() => loadFixture('arrivals', 'does-not-exist')).toThrow();
  });
});
