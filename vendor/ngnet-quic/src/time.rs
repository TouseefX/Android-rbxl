//! Time, as this crate understands it.
//!
//! ngtcp2 measures time in nanoseconds and reserves `UINT64_MAX` to mean "no time"
//! (`ngtcp2.h:1070-1077`). It names no epoch and no clock: the bundled examples use
//! `CLOCK_MONOTONIC`, but that is a convention of the examples rather than a contract of
//! the library.
//!
//! So this crate does not choose one either. A [`Timestamp`] is an opaque count of
//! nanoseconds in whatever monotonic timescale the caller keeps, and the only promises made
//! about it are that it advances and that the same timescale is used throughout a
//! connection's life. Reading a clock here would pick one on the caller's behalf and would
//! make every test depend on wall time.

// The raw conversions are used by the modules that call into ngtcp2, which arrive with the
// connection itself. They are written here because this is where the sentinel rule lives.
#![allow(dead_code)]

use core::fmt;

/// A point in the caller's own monotonic timescale, in nanoseconds.
///
/// The zero point is whatever the caller's clock uses; this crate never interprets it, only
/// passes it through and compares it. What matters is that every timestamp handed to one
/// connection comes from the same clock.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Wraps a nanosecond count from the caller's clock.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] for `u64::MAX`, which ngtcp2 reserves to mean
    /// "no timestamp". Letting it through would make a real time indistinguishable from
    /// the absence of one.
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    pub const fn from_nanos(nanos: u64) -> crate::Result<Self> {
        if nanos == u64::MAX {
            return Err(crate::Error::invalid_input(
                "u64::MAX is reserved by ngtcp2 to mean the absence of a timestamp",
            ));
        }
        Ok(Self(nanos))
    }

    /// The wrapped nanosecond count.
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// The raw value ngtcp2 expects.
    pub(crate) const fn as_raw(self) -> u64 {
        self.0
    }

    /// Interprets a raw ngtcp2 timestamp, mapping the reserved sentinel to `None`.
    ///
    /// ngtcp2 uses `UINT64_MAX` for "no timer is armed", which is a different thing from a
    /// timer armed at a very large time. Converting it to `None` here is what stops the
    /// two from being confused at every call site that asks about expiry.
    pub(crate) const fn from_raw(raw: u64) -> Option<Self> {
        if raw == u64::MAX {
            None
        } else {
            Some(Self(raw))
        }
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp({}ns)", self.0)
    }
}

/// A span of time in nanoseconds.
///
/// ngtcp2's own `ngtcp2_duration`. Kept distinct from [`Timestamp`] so that a point in time
/// and a length of time cannot be passed for one another.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration(u64);

impl Duration {
    /// Wraps a nanosecond count.
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Builds a duration from whole milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] if the value overflows a nanosecond count.
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    pub const fn from_millis(millis: u64) -> crate::Result<Self> {
        match millis.checked_mul(1_000_000) {
            Some(nanos) => Ok(Self(nanos)),
            None => Err(crate::Error::invalid_input(
                "duration in milliseconds overflows a nanosecond count",
            )),
        }
    }

    /// The wrapped nanosecond count.
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// The raw value ngtcp2 expects.
    pub(crate) const fn as_raw(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Duration({}ns)", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reserved_sentinel_is_not_a_timestamp() {
        assert!(Timestamp::from_nanos(u64::MAX).is_err());
        assert!(Timestamp::from_nanos(u64::MAX - 1).is_ok());
    }

    #[test]
    fn the_reserved_sentinel_reads_back_as_no_time() {
        assert_eq!(Timestamp::from_raw(u64::MAX), None);
        assert_eq!(Timestamp::from_raw(0).map(Timestamp::as_nanos), Some(0));
    }

    #[test]
    fn milliseconds_that_overflow_are_rejected() {
        assert!(Duration::from_millis(u64::MAX).is_err());
        assert_eq!(Duration::from_millis(5).unwrap().as_nanos(), 5_000_000);
    }
}
