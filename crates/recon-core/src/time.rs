//! Virtual monotonic time.
//!
//! Deliberately not `std::time::Instant` or any runtime's instant type: neither can be
//! constructed at an arbitrary value, which makes a seeded, replayable run impossible.
//!
//! `std::time::Duration` has no such problem — it is a pure value type with no clock behind it —
//! so it is what [`Time`] is built from. The newtype is still worth having: a `Time` is a
//! *point* and a `Duration` is a *span*, and keeping them distinct is what stops one being
//! passed where the other belongs.

use core::ops::{Add, AddAssign, Sub};
use core::time::Duration;

/// A point in time, measured as an offset from the start of a run.
///
/// Monotonic by construction: the simulator only ever advances it, and a real driver derives it
/// from a base instant captured at startup. Arithmetic saturates rather than wrapping, so
/// monotonicity cannot be broken by overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Time(Duration);

impl Time {
    /// The start of a run.
    pub const ZERO: Time = Time(Duration::ZERO);

    /// The latest representable time — around 584 billion years in, so useful as a
    /// "no deadline" sentinel and unreachable in practice.
    pub const MAX: Time = Time(Duration::MAX);

    /// A time `offset` after the start of the run.
    pub const fn from_offset(offset: Duration) -> Self {
        Time(offset)
    }

    pub const fn from_nanos(nanos: u64) -> Self {
        Time(Duration::from_nanos(nanos))
    }

    pub const fn from_micros(micros: u64) -> Self {
        Time(Duration::from_micros(micros))
    }

    pub const fn from_millis(millis: u64) -> Self {
        Time(Duration::from_millis(millis))
    }

    pub const fn from_secs(secs: u64) -> Self {
        Time(Duration::from_secs(secs))
    }

    /// The offset from the start of the run.
    pub const fn as_offset(self) -> Duration {
        self.0
    }

    /// Nanoseconds since the start of the run. `u128`, because the range exceeds `u64`.
    pub const fn as_nanos(self) -> u128 {
        self.0.as_nanos()
    }

    /// Milliseconds since the start of the run, truncated.
    pub const fn as_millis(self) -> u128 {
        self.0.as_millis()
    }

    /// Time elapsed from `earlier` to `self`, saturating at zero if `earlier` is later.
    pub fn saturating_since(self, earlier: Time) -> Duration {
        self.0.saturating_sub(earlier.0)
    }

    /// `self` advanced by `d`, saturating at [`Time::MAX`].
    pub fn saturating_add(self, d: Duration) -> Time {
        Time(self.0.saturating_add(d))
    }
}

impl Add<Duration> for Time {
    type Output = Time;
    fn add(self, d: Duration) -> Time {
        self.saturating_add(d)
    }
}

impl AddAssign<Duration> for Time {
    fn add_assign(&mut self, d: Duration) {
        *self = self.saturating_add(d);
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
        assert_eq!(Time::from_secs(2), Time::from_millis(2_000));
        assert_eq!(Time::from_offset(Duration::from_micros(7)).as_nanos(), 7_000);
    }

    #[test]
    fn orders_by_offset() {
        assert!(Time::ZERO < Time::from_nanos(1));
        assert!(Time::from_millis(2) > Time::from_millis(1));
        assert_eq!(Time::ZERO, Time::from_nanos(0));
        assert!(Time::MAX > Time::from_secs(u32::MAX as u64));

        let mut v = [Time::from_nanos(3), Time::from_nanos(1), Time::from_nanos(2)];
        v.sort();
        assert_eq!(v, [Time::from_nanos(1), Time::from_nanos(2), Time::from_nanos(3)]);
    }

    #[test]
    fn adds_a_duration() {
        assert_eq!(Time::ZERO + Duration::from_millis(5), Time::from_millis(5));
        assert_eq!(Time::from_nanos(10) + Duration::from_nanos(7), Time::from_nanos(17));

        let mut t = Time::ZERO;
        t += Duration::from_millis(4);
        t += Duration::from_millis(6);
        assert_eq!(t, Time::from_millis(10));
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

    #[test]
    fn the_range_exceeds_a_u64_of_nanoseconds() {
        // The ceiling a u64-of-nanos representation would have imposed was ~584 years.
        let beyond_u64_nanos = Time::from_secs(600 * 365 * 24 * 60 * 60);
        assert!(beyond_u64_nanos.as_nanos() > u64::MAX as u128);
        assert!(beyond_u64_nanos < Time::MAX);
    }
}
