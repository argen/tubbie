/**
 * Injectable wall-clock — port of `tfl_client::clock::Clock`.
 *
 * `SystemClock` returns the real time; `FakeClock` returns a pinned instant so
 * elapsed-time logic (cooldown gates, `Retry-After` HTTP-date arithmetic,
 * `timeToStation` formatting) is deterministic in tests. The Rust trait returns
 * `DateTime<Utc>`; here we return a `Date` (always an absolute instant — `Date`
 * has no timezone, so there is no `Utc` distinction to preserve).
 */

export interface Clock {
  now(): Date;
}

/** Production clock backed by the system wall clock. */
export class SystemClock implements Clock {
  now(): Date {
    return new Date();
  }
}

/**
 * Deterministic clock for tests. Pin it to an instant with {@link FakeClock.at}
 * or {@link FakeClock.fromRfc3339}, then move it forward with
 * {@link FakeClock.advance}.
 */
export class FakeClock implements Clock {
  private current: Date;

  private constructor(at: Date) {
    this.current = at;
  }

  /** Pin the clock to `at` (defensively copied so the caller can't mutate it). */
  static at(at: Date): FakeClock {
    return new FakeClock(new Date(at.getTime()));
  }

  /**
   * Parse an RFC3339 / ISO-8601 string and pin the clock to that instant.
   * Throws if the string is not a valid timestamp (mirrors the Rust
   * `from_rfc3339` `Result`).
   */
  static fromRfc3339(s: string): FakeClock {
    const t = new Date(s);
    if (Number.isNaN(t.getTime())) {
      throw new Error(`invalid RFC3339 timestamp: ${s}`);
    }
    return new FakeClock(t);
  }

  /** Advance the clock by `ms` milliseconds. */
  advance(ms: number): void {
    this.current = new Date(this.current.getTime() + ms);
  }

  now(): Date {
    return new Date(this.current.getTime());
  }
}
