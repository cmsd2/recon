//! Virtual monotonic time.
//!
//! Deliberately not `std::time::Instant` or any runtime's instant type: neither can be
//! constructed at an arbitrary value, which makes a seeded, replayable run impossible.
//! `std::time::Duration` is used as-is — it is a pure value type with no clock behind it.

use core::ops::{Add, Sub};
use core::time::Duration;

/// A point in time, measured as nanoseconds since the start of a run.
///
/// Monotonic by construction: the simulator only ever advances it, and a real driver
/// derives it from a base instant captured at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Time(u64);

impl Time {
    /// The start of a run.
    pub const ZERO: Time = Time(0);

    /// The latest representable time, useful as a sentinel for "no deadline".
    pub const MAX: Time = Time(u64::MAX);

    /// Construct a time at `nanos` nanoseconds after the start of the run.
    pub const fn from_nanos(nanos: u64) -> Self {
        Time(nanos)
    }

    /// Construct a time at `millis` milliseconds after the start of the run.
    pub const fn from_millis(millis: u64) -> Self {
        Time(millis.saturating_mul(1_000_000))
    }

    /// Nanoseconds since the start of the run.
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// Milliseconds since the start of the run, truncated.
    pub const fn as_millis(self) -> u64 {
        self.0 / 1_000_000
    }

    /// Time elapsed from `earlier` to `self`, saturating at zero if `earlier` is later.
    pub fn saturating_since(self, earlier: Time) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }

    /// `self` advanced by `d`, saturating at [`Time::MAX`].
    pub fn saturating_add(self, d: Duration) -> Time {
        let nanos = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        Time(self.0.saturating_add(nanos))
    }
}

impl Add<Duration> for Time {
    type Output = Time;
    fn add(self, d: Duration) -> Time {
        self.saturating_add(d)
    }
}

impl Sub<Time> for Time {
    type Output = Duration;
    fn sub(self, earlier: Time) -> Duration {
        self.saturating_since(earlier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructed_at_an_arbitrary_value() {
        // The property std::time::Instant cannot offer, and the reason this type exists.
        assert_eq!(Time::from_nanos(42).as_nanos(), 42);
        assert_eq!(Time::from_millis(3).as_nanos(), 3_000_000);
        assert_eq!(Time::from_nanos(5_500_000).as_millis(), 5);
    }

    #[test]
    fn orders_by_nanos() {
        assert!(Time::ZERO < Time::from_nanos(1));
        assert!(Time::from_millis(2) > Time::from_millis(1));
        assert_eq!(Time::ZERO, Time::from_nanos(0));
        assert!(Time::MAX > Time::from_millis(u32::MAX as u64));

        let mut v = [Time::from_nanos(3), Time::from_nanos(1), Time::from_nanos(2)];
        v.sort();
        assert_eq!(v, [Time::from_nanos(1), Time::from_nanos(2), Time::from_nanos(3)]);
    }

    #[test]
    fn adds_a_duration() {
        assert_eq!(Time::ZERO + Duration::from_millis(5), Time::from_millis(5));
        assert_eq!(
            Time::from_nanos(10) + Duration::from_nanos(7),
            Time::from_nanos(17)
        );
    }

    #[test]
    fn subtracts_to_a_duration() {
        assert_eq!(Time::from_millis(9) - Time::from_millis(4), Duration::from_millis(5));
    }

    #[test]
    fn saturates_rather_than_wrapping() {
        // Monotonicity must not be breakable by arithmetic.
        assert_eq!(Time::MAX + Duration::from_secs(1), Time::MAX);
        assert_eq!(Time::from_millis(1) - Time::from_millis(9), Duration::ZERO);
        assert_eq!(Time::ZERO.saturating_add(Duration::MAX), Time::MAX);
    }
}
