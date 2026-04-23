use chrono::{DateTime, Utc};

/// Injectable wall-clock abstraction.
///
/// Real impl (`SystemClock`) returns `Utc::now()`.
/// Test impl (`FakeClock`) returns a fixed instant set to the fixture's
/// `recorded_at` timestamp, making `timeToStation` formatting deterministic.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Production clock backed by the system wall clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Deterministic clock for tests.
///
/// Constructed from a fixture's `recorded_at` timestamp so that
/// `timeToStation` values in the fixture produce stable formatted output.
///
/// Call `advance` to move the clock forward in tests that exercise elapsed-time logic.
#[derive(Debug, Clone)]
pub struct FakeClock {
    current: DateTime<Utc>,
}

impl FakeClock {
    /// Create a `FakeClock` pinned to `at`.
    pub fn at(at: DateTime<Utc>) -> Self {
        Self { current: at }
    }

    /// Parse an RFC3339 string and pin the clock to that instant.
    ///
    /// # Errors
    /// Returns `Err` if `s` is not a valid RFC3339 timestamp.
    pub fn from_rfc3339(s: &str) -> Result<Self, chrono::ParseError> {
        let dt = DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc);
        Ok(Self { current: dt })
    }

    /// Advance the clock by `duration`.
    pub fn advance(&mut self, duration: chrono::Duration) {
        self.current += duration;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn fake_clock_returns_pinned_instant() {
        let t = DateTime::parse_from_rfc3339("2025-06-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let clock = FakeClock::at(t);
        assert_eq!(clock.now(), t);
    }

    #[test]
    fn fake_clock_advance_moves_forward() {
        let t = DateTime::parse_from_rfc3339("2025-06-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut clock = FakeClock::at(t);
        clock.advance(Duration::minutes(5));
        let expected = t + Duration::minutes(5);
        assert_eq!(clock.now(), expected);
    }

    #[test]
    fn fake_clock_from_rfc3339_parses_correctly() {
        let clock = FakeClock::from_rfc3339("2025-06-01T14:30:00Z").unwrap();
        let expected = DateTime::parse_from_rfc3339("2025-06-01T14:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(clock.now(), expected);
    }

    #[test]
    fn fake_clock_from_rfc3339_rejects_invalid() {
        assert!(FakeClock::from_rfc3339("not-a-date").is_err());
    }

    #[test]
    fn system_clock_returns_a_recent_timestamp() {
        let before = Utc::now();
        let t = SystemClock.now();
        let after = Utc::now();
        assert!(t >= before);
        assert!(t <= after);
    }

    #[test]
    fn fake_clock_advance_multiple_times() {
        let t = DateTime::parse_from_rfc3339("2025-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut clock = FakeClock::at(t);
        clock.advance(Duration::seconds(30));
        clock.advance(Duration::seconds(30));
        let expected = t + Duration::minutes(1);
        assert_eq!(clock.now(), expected);
    }
}
