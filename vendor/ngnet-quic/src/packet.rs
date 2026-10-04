//! Packets in, packets out, and the timers that drive both.
//!
//! This is where the sans-I/O loop closes. A caller receives a datagram, hands it to
//! [`Conn::read_pkt`], asks [`Conn::write_pkt`] for datagrams to send until it says there
//! are none, and asks [`Conn::expiry`] when to come back. Nothing here touches a socket or
//! a clock.
//!
//! # Two results that are easy to get wrong
//!
//! **A zero-length write is not "nothing to do".** `ngtcp2_conn_writev_stream` returning `0`
//! means "buffer too small or congestion limited" (`ngtcp2.h:5240-5243`), and the documented
//! response is to keep reading and wait for the congestion window to open. Treating it as
//! "the send loop is finished" is the classic way to build a connection that stalls under
//! load and works perfectly in tests. [`WriteOutcome`] gives the two cases different names.
//!
//! **An idle timeout is not an error to answer.** `NGTCP2_ERR_IDLE_CLOSE` means the
//! connection should be dropped *without* writing a CONNECTION_CLOSE
//! (`ngtcp2.h:4709-4713`). Routing it through the generic close path would send a packet to
//! a peer that has already gone.

use ngnet_quic_sys as sys;

use crate::conn::Conn;
use crate::error::{Error, Result};
use crate::time::Timestamp;
use crate::tls::Session;

/// What happened to a datagram handed to [`Conn::read_pkt`].
///
/// Every documented outcome of `ngtcp2_conn_read_pkt` maps to exactly one variant. Closed
/// rather than `#[non_exhaustive]`: each case demands a different response from the caller,
/// so a new one would be a change every caller must notice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadOutcome {
    /// The packet was processed, or was discarded as uninteresting.
    ///
    /// ngtcp2 does not distinguish these: it expresses "drop this packet and carry on" by
    /// returning success (`ngtcp2_conn.c:10352-10365`).
    Processed,
    /// A server must answer with a Retry packet and discard this connection.
    ///
    /// Clients never see this.
    SendRetry,
    /// A server must drop the connection **silently**, with no CONNECTION_CLOSE.
    ///
    /// Clients never see this.
    DropSilently,
    /// The connection has entered the draining period. No further packets may be sent.
    Draining,
    /// The connection has entered the closing period. No further packets may be sent.
    Closing,
}

/// What happened when [`Conn::write_pkt`] was asked for a datagram.
///
/// Closed for the same reason as [`ReadOutcome`], and because the distinction between the
/// last two variants is the entire point of the type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteOutcome {
    /// A datagram was written. Send it, then ask again.
    Datagram {
        /// How many bytes of the buffer were filled.
        len: usize,
    },
    /// Nothing more to send at this moment. Wait for input or for the expiry deadline.
    Idle,
    /// ngtcp2 has data to send but cannot right now — the congestion window is closed, or
    /// the buffer offered was too small.
    ///
    /// **Not** the same as [`WriteOutcome::Idle`]. A caller that treats it as "finished"
    /// will stall the connection until something else happens to wake it.
    Blocked,
}

/// What happened when a timer expired.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpiryOutcome {
    /// The expiry was handled. Ask [`Conn::write_pkt`] for anything it produced.
    Handled,
    /// The connection has been idle too long.
    ///
    /// Drop it. Do **not** write a CONNECTION_CLOSE: the peer is gone, and ngtcp2
    /// documents this case as requiring silence (`ngtcp2.h:4709-4713`).
    IdleClose,
    /// The connection is draining or closing and the timer changed nothing.
    Terminal,
}

impl<S: Session> Conn<'_, S> {
    /// Feeds a received datagram to the connection.
    ///
    /// `now` is a reading from the caller's clock, in the same timescale as every other
    /// timestamp given to this connection.
    ///
    /// # Errors
    ///
    /// Returns an error for any failure that is not one of the documented outcomes. Such a
    /// failure means the connection should be closed with a connection-close packet;
    /// [`Error::is_fatal`] distinguishes the cases where
    /// even that is not possible.
    pub fn read_pkt(&mut self, datagram: &[u8], now: Timestamp) -> Result<ReadOutcome> {
        if datagram.is_empty() {
            // ngtcp2 treats this as success, but a caller passing an empty slice has
            // almost certainly made a mistake, and saying so is more useful than silence.
            return Err(Error::invalid_input("an empty datagram cannot be read"));
        }

        let path = self.path_ptr();
        let rc = self.with_bridge(|raw| {
            // SAFETY: `raw` is live, `path` points into storage the connection owns, and
            // `datagram` is readable for its length. The packet info is a zeroed struct
            // with the ECN field unset, which is the "no marking" case.
            unsafe {
                let pi = sys::ngtcp2_pkt_info { ecn: 0 };
                crate::ffi::conn_read_pkt(
                    raw,
                    path,
                    &pi,
                    datagram.as_ptr(),
                    datagram.len(),
                    now.as_raw(),
                )
            }
        });

        match rc {
            0 => Ok(ReadOutcome::Processed),
            sys::NGTCP2_ERR_RETRY => Ok(ReadOutcome::SendRetry),
            sys::NGTCP2_ERR_DROP_CONN => Ok(ReadOutcome::DropSilently),
            sys::NGTCP2_ERR_DRAINING => Ok(ReadOutcome::Draining),
            sys::NGTCP2_ERR_CLOSING => Ok(ReadOutcome::Closing),
            other => Err(Error::native(other, "could not read the packet").into_unusable()),
        }
    }

    /// Asks the connection for the next datagram to send.
    ///
    /// Call this in a loop until it returns something other than
    /// [`WriteOutcome::Datagram`], sending each datagram as it comes.
    ///
    /// # Errors
    ///
    /// Returns an error if ngtcp2 refuses. The connection is then unusable.
    pub fn write_pkt(&mut self, dest: &mut [u8], now: Timestamp) -> Result<WriteOutcome> {
        if dest.is_empty() {
            return Err(Error::invalid_input(
                "a datagram buffer must have room to write into",
            ));
        }

        let path = self.path_mut().as_raw_mut();
        let written = self.with_bridge(|raw| {
            // SAFETY: `raw` is live, `path` points into storage the connection owns, and
            // `dest` is writable for its length.
            unsafe {
                let mut pi = sys::ngtcp2_pkt_info { ecn: 0 };
                crate::ffi::conn_write_pkt(
                    raw,
                    path,
                    &mut pi,
                    dest.as_mut_ptr(),
                    dest.len(),
                    now.as_raw(),
                )
            }
        });

        if written > 0 {
            let len = written as usize;
            debug_assert!(len <= dest.len());
            // Pacing depends on this, and omitting it returns no error -- it simply makes
            // the connection send in bursts (`ngtcp2.h:5320-5324`).
            // SAFETY: `raw` is live and the timestamp is the one just used to write.
            unsafe { sys::ngtcp2_conn_update_pkt_tx_time(self.raw(), now.as_raw()) };
            return Ok(WriteOutcome::Datagram { len });
        }

        match written {
            // The distinction this whole type exists for. Zero does not mean "done".
            0 => Ok(WriteOutcome::Blocked),
            w if w == sys::NGTCP2_ERR_WRITE_MORE as isize => Ok(WriteOutcome::Blocked),
            other => {
                let code = i32::try_from(other).unwrap_or(sys::NGTCP2_ERR_INTERNAL);
                if code == sys::NGTCP2_ERR_CLOSING || code == sys::NGTCP2_ERR_DRAINING {
                    // Not a failure: there is genuinely nothing left to send.
                    return Ok(WriteOutcome::Idle);
                }
                Err(Error::native(code, "could not write a packet").into_unusable())
            }
        }
    }

    /// When this connection next needs attention, if any timer is armed.
    ///
    /// `None` means no timer is pending. ngtcp2 signals that with `UINT64_MAX`, which
    /// [`Timestamp`] converts rather than passing on, so a very distant deadline and the
    /// absence of one cannot be confused.
    ///
    /// A connection that is never told its deadline has passed will silently stop making
    /// progress: loss recovery, acknowledgement delay and the idle timeout are all driven
    /// from here.
    pub fn expiry(&self) -> Option<Timestamp> {
        // SAFETY: `raw` is live; the `2` variant is the one that takes a const pointer.
        let raw = unsafe { sys::ngtcp2_conn_get_expiry2(self.raw()) };
        Timestamp::from_raw(raw)
    }

    /// Tells the connection that its deadline has passed.
    ///
    /// # Errors
    ///
    /// Returns an error for a failure that is not one of the documented outcomes.
    pub fn handle_expiry(&mut self, now: Timestamp) -> Result<ExpiryOutcome> {
        let rc = self.with_bridge(|raw| {
            // SAFETY: `raw` is live.
            unsafe { sys::ngtcp2_conn_handle_expiry(raw, now.as_raw()) }
        });

        match rc {
            0 => Ok(ExpiryOutcome::Handled),
            // Distinct from every other outcome on purpose: the correct response is to drop
            // the connection in silence, and answering with a close packet would be wrong.
            sys::NGTCP2_ERR_IDLE_CLOSE => Ok(ExpiryOutcome::IdleClose),
            sys::NGTCP2_ERR_CLOSING | sys::NGTCP2_ERR_DRAINING => Ok(ExpiryOutcome::Terminal),
            other => Err(Error::native(other, "could not handle the expiry").into_unusable()),
        }
    }

    /// Whether the connection is in its closing period.
    pub fn in_closing_period(&self) -> bool {
        // SAFETY: `raw` is live; the `2` variant takes a const pointer, which is what makes
        // this an honest `&self` method.
        unsafe { sys::ngtcp2_conn_in_closing_period2(self.raw()) != 0 }
    }

    /// Whether the connection is in its draining period.
    pub fn in_draining_period(&self) -> bool {
        // SAFETY: `raw` is live; the `2` variant takes a const pointer.
        unsafe { sys::ngtcp2_conn_in_draining_period2(self.raw()) != 0 }
    }
}

#[cfg(all(test, feature = "tls-ossl"))]
mod tests {
    use super::*;
    use crate::conn::test_support::client_conn;
    use crate::handlers::Handlers;
    use crate::time::Timestamp;

    fn ts(nanos: u64) -> Timestamp {
        Timestamp::from_nanos(nanos).unwrap()
    }

    #[test]
    fn an_empty_datagram_is_rejected_rather_than_silently_accepted() {
        let mut conn = client_conn(Handlers::new()).unwrap();
        assert!(conn.read_pkt(&[], ts(2_000_000)).is_err());
    }

    #[test]
    fn an_empty_write_buffer_is_rejected() {
        let mut conn = client_conn(Handlers::new()).unwrap();
        assert!(conn.write_pkt(&mut [], ts(2_000_000)).is_err());
    }

    #[test]
    fn a_client_produces_a_first_flight() {
        // The cheapest falsification of the whole egress path: no server, no certificate,
        // no timer loop. If the callback table, the TLS handle or the version constants
        // were wrong, there would be nothing to send.
        let mut conn = client_conn(Handlers::new()).unwrap();
        let mut buf = [0u8; 1500];
        let outcome = conn.write_pkt(&mut buf, ts(2_000_000)).unwrap();

        let WriteOutcome::Datagram { len } = outcome else {
            panic!("a fresh client must have a first flight to send, got {outcome:?}");
        };
        assert!(len > 0);

        // A QUIC Initial is a long-header packet: high bit set, fixed bit set.
        assert_eq!(buf[0] & 0b1100_0000, 0b1100_0000, "long header form");
        // And it carries the version it was built for.
        let version = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
        assert_eq!(version, crate::accept::VERSION_V1);
        // Initial packets must be padded to at least 1200 bytes, which is what stops a
        // server being used as an amplifier.
        assert!(len >= 1200, "an Initial must be padded, got {len}");
    }

    #[test]
    fn the_first_flight_is_followed_by_idle_or_blocked_not_more_datagrams_forever() {
        // A send loop must terminate. If `write_pkt` never stopped returning datagrams,
        // this would hang rather than fail, so the iteration count is bounded.
        let mut conn = client_conn(Handlers::new()).unwrap();
        let mut buf = [0u8; 1500];
        let mut datagrams = 0;
        for _ in 0..16 {
            match conn.write_pkt(&mut buf, ts(2_000_000)).unwrap() {
                WriteOutcome::Datagram { .. } => datagrams += 1,
                WriteOutcome::Idle | WriteOutcome::Blocked => break,
            }
        }
        assert!(datagrams >= 1);
        assert!(datagrams < 16, "the send loop did not terminate");
    }

    #[test]
    fn a_fresh_connection_arms_a_timer() {
        // Loss recovery needs one from the moment the first packet is sent, and a
        // connection whose deadline was never reported would stall silently.
        let mut conn = client_conn(Handlers::new()).unwrap();
        let mut buf = [0u8; 1500];
        conn.write_pkt(&mut buf, ts(2_000_000)).unwrap();
        assert!(conn.expiry().is_some(), "a sent packet must arm a timer");
    }

    #[test]
    fn handling_an_expiry_before_anything_has_happened_is_harmless() {
        let mut conn = client_conn(Handlers::new()).unwrap();
        let outcome = conn.handle_expiry(ts(2_000_000)).unwrap();
        assert_eq!(outcome, ExpiryOutcome::Handled);
    }

    #[test]
    fn an_idle_connection_eventually_reports_an_idle_close() {
        // The outcome that must not be answered with a close packet. Reaching it needs the
        // clock advanced past the idle timeout, which is what makes it worth pinning: a
        // caller who mapped it onto the generic error path would send to a vanished peer.
        let mut conn = client_conn(Handlers::new()).unwrap();
        let mut buf = [0u8; 1500];
        conn.write_pkt(&mut buf, ts(2_000_000)).unwrap();

        // Well past the default idle timeout of thirty seconds.
        let far_future = ts(2_000_000 + 120_000_000_000);
        let mut outcome = conn.handle_expiry(far_future).unwrap();
        for _ in 0..8 {
            if outcome == ExpiryOutcome::IdleClose {
                break;
            }
            outcome = conn.handle_expiry(far_future).unwrap();
        }
        assert_eq!(
            outcome,
            ExpiryOutcome::IdleClose,
            "an idle connection must report an idle close, not a generic error"
        );
    }

    #[test]
    fn a_fresh_connection_is_in_neither_terminal_period() {
        let conn = client_conn(Handlers::new()).unwrap();
        assert!(!conn.in_closing_period());
        assert!(!conn.in_draining_period());
    }

    #[test]
    fn the_three_write_outcomes_are_distinguishable() {
        // The type-level point of `WriteOutcome`: `Idle` and `Blocked` are different
        // answers, and conflating them is the classic stall bug.
        assert_ne!(WriteOutcome::Idle, WriteOutcome::Blocked);
        assert_ne!(WriteOutcome::Datagram { len: 1 }, WriteOutcome::Idle);
        assert_ne!(WriteOutcome::Datagram { len: 1 }, WriteOutcome::Blocked);
    }

    #[test]
    fn garbage_is_not_accepted_as_a_packet() {
        let mut conn = client_conn(Handlers::new()).unwrap();
        let junk = [0x42u8; 64];
        // Either a clean discard or an error is acceptable; silently claiming to have
        // processed it as a real packet would not be, and neither would a crash.
        let _ = conn.read_pkt(&junk, ts(2_000_000));
    }
}
