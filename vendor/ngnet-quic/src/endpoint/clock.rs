//! Time, which this crate still refuses to read for itself.
//!
//! # One clock, not two
//!
//! The sans-I/O core takes a [`Timestamp`] on almost every call, documented as an opaque
//! count of nanoseconds in whatever monotonic timescale the caller chose. This layer needs
//! to *wait* until a deadline as well as read one, and the temptation is to treat those as
//! two facilities — a clock for the core and a timer for the runtime — which would then
//! need reconciling every time they disagreed.
//!
//! They are one facility. [`Clock::now`] returns a `Timestamp` in the same timescale the
//! core is given, and a wait is computed as `deadline - now()`, saturating at zero. There
//! is no second time source and so nothing to reconcile.
//!
//! # There is only one timer, and ngtcp2 already folded pacing into it
//!
//! It is easy to believe a QUIC driver needs two deadlines: one for loss recovery and the
//! idle timeout, and another for pacing, since ngtcp2 refuses to send before the pacing
//! deadline passes. It does not. `ngtcp2_conn_get_expiry2` finishes with
//! `ngtcp2_min(res, conn->tx.pacing.next_ts)` (`crates/ngnet-quic-sys/vendor/ngtcp2/lib/ngtcp2_conn.c:11387`), so
//! the value [`crate::Conn::expiry`] reports is already the earlier of the two.
//!
//! The practical consequence is the whole reason this is written down: a driver that stops
//! rearming its timer after a sending pass will send one datagram and then go quiet, and
//! the connection will look broken rather than slow. Rearming from `expiry()` after every
//! pass is what prevents that, and no separate pacing bookkeeping is needed — the core
//! already calls `ngtcp2_conn_update_pkt_tx_time` inside its write paths, and calling it
//! again from a driver would corrupt the pacing it is trying to respect.

use core::future::Future;

use crate::time::Timestamp;

/// A monotonic clock and a way to wait on it.
///
/// # Contract
///
/// [`Clock::now`] must be monotonic: it may return the same value twice but must never go
/// backwards. A clock that goes backwards makes the core compute a negative elapsed time,
/// which it reads as an enormous positive one.
///
/// [`Clock::sleep_until`] must resolve at or after the deadline, and must resolve
/// *immediately* for a deadline already in the past — a driver reaching that case has work
/// waiting, and a clock that instead waited for the next tick would add latency to exactly
/// the path that is already late.
pub trait Clock {
    /// The future [`Clock::sleep_until`] returns.
    ///
    /// [`Unpin`] because the driver stores one and polls it in place. Every implementation
    /// satisfies it by boxing, which costs one allocation per rearm and buys the driver
    /// freedom from `unsafe` — a trade worth making in a subtree that contains none at all.
    type Sleep: Future<Output = ()> + Unpin;

    /// The current time, in the caller's own monotonic timescale.
    fn now(&self) -> Timestamp;

    /// Waits until `deadline`, resolving immediately if it has passed.
    fn sleep_until(&self, deadline: Timestamp) -> Self::Sleep;
}
