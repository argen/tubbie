/**
 * Ports `crates/tfl-client/src/clock.rs` tests — pinned instant, advance,
 * RFC3339 parsing, and invalid rejection.
 */

import { describe, expect, it } from 'vitest';
import { FakeClock, SystemClock } from './clock.js';

describe('FakeClock', () => {
  it('returns the pinned instant', () => {
    const t = new Date('2025-06-01T12:00:00Z');
    expect(FakeClock.at(t).now().getTime()).toBe(t.getTime());
  });

  it('advance moves the clock forward', () => {
    const t = new Date('2025-06-01T12:00:00Z');
    const clock = FakeClock.at(t);
    clock.advance(5 * 60 * 1000);
    expect(clock.now().getTime()).toBe(t.getTime() + 5 * 60 * 1000);
  });

  it('fromRfc3339 parses a valid timestamp', () => {
    const clock = FakeClock.fromRfc3339('2025-06-01T14:30:00Z');
    expect(clock.now().toISOString()).toBe('2025-06-01T14:30:00.000Z');
  });

  it('fromRfc3339 rejects an invalid string', () => {
    expect(() => FakeClock.fromRfc3339('not-a-date')).toThrow();
  });

  it('advance accumulates across multiple calls', () => {
    const t = new Date('2025-06-01T00:00:00Z');
    const clock = FakeClock.at(t);
    clock.advance(30_000);
    clock.advance(30_000);
    expect(clock.now().getTime()).toBe(t.getTime() + 60_000);
  });

  it('does not leak its internal Date by reference', () => {
    const clock = FakeClock.at(new Date('2025-06-01T00:00:00Z'));
    const a = clock.now();
    a.setFullYear(1999);
    expect(clock.now().getUTCFullYear()).toBe(2025);
  });
});

describe('SystemClock', () => {
  it('returns a timestamp bracketed by wall-clock reads around it', () => {
    const before = Date.now();
    const t = new SystemClock().now().getTime();
    const after = Date.now();
    expect(t).toBeGreaterThanOrEqual(before);
    expect(t).toBeLessThanOrEqual(after);
  });
});
