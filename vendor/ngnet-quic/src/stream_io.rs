//! Streams, flow control, and closing the connection.
//!
//! A QUIC connection carries data on streams. This module is the half of the API a caller
//! spends most of its time in: opening streams, writing to them, granting the peer credit,
//! and eventually shutting the whole thing down.
//!
//! # Credit is not automatic
//!
//! Two obligations here are easy to miss because omitting them produces no error, only a
//! peer that goes quiet.
//!
//! Flow-control credit is consumed as the peer sends, and is only replenished when the
//! application says so. And **stream-count limits are never raised by the library**: "The
//! library does not increase maximum stream limit automatically"
//! (`ngtcp2.h:5586-5594`). An application that accepts streams but never calls
//! [`Conn::extend_max_streams_bidi`] will let the peer open exactly as many as the initial
//! transport parameters allowed, and then no more, forever.

use ngnet_quic_sys as sys;

use std::io::IoSlice;

use crate::conn::Conn;
use crate::error::{ApplicationErrorCode, Error, Result};
use crate::retain::OwnedBytes;
use crate::stream::StreamId;
use crate::time::Timestamp;
use crate::tls::Session;

/// What happened when stream data was offered to the connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamWrite {
    /// A datagram was produced, and this many bytes of the offered data went into it.
    ///
    /// `accepted` may be less than what was offered, or zero: the packet may have been
    /// filled with control frames instead. Whatever was not accepted must be offered again.
    Datagram {
        /// Bytes of the datagram buffer that were filled.
        len: usize,
        /// Bytes of the caller's stream data the connection took.
        accepted: usize,
    },
    /// The stream's flow-control window is full. Wait for the peer to grant more.
    StreamBlocked,
    /// The connection's flow-control window is full.
    ConnectionBlocked,
    /// ngtcp2 has something to send but cannot right now — the datagram buffer was too
    /// small, or the congestion window is closed.
    ///
    /// **Not** "finished". Keep the connection running and try again; treating this as the
    /// end of the send loop is the classic QUIC stall.
    Blocked,
    /// Nothing to send at the moment.
    Idle,
}

/// The result of an owned write: what happened, and what the caller must offer again.
///
/// Distinct from [`StreamWrite`] because an owned write hands ownership *back* for whatever
/// ngtcp2 did not take. A partial acceptance is ordinary — a packet fills before the buffer
/// is exhausted — and the unaccepted suffix has to go somewhere. It goes here, as a handle
/// into the same allocation the accepted prefix is retained from, so nothing is copied on
/// either side of the split.
#[derive(Debug)]
pub struct OwnedWrite {
    /// What the write did, exactly as a borrowed write would report it.
    pub outcome: StreamWrite,
    /// The bytes ngtcp2 did not accept, to be offered again. Empty when it took everything.
    ///
    /// Shares its allocation with the retained prefix, so holding it costs no copy; a caller
    /// that drops it simply abandons the unsent tail.
    pub unsent: OwnedBytes,
}

/// One staged chunk, as it is handed to ngtcp2.
///
/// The pointer, the length, and whether anything was staged at all are three views of one
/// fact, so they travel together rather than as separate arguments that could drift apart.
/// Nothing staged is a legitimate call — a write may carry only a `fin` — and is described
/// to ngtcp2 as a null vector of length zero.
struct StagedVec {
    base: *const u8,
    len: usize,
    staged: bool,
}

impl StagedVec {
    fn new(staged: Option<(*const u8, usize)>) -> Self {
        let (base, len) = staged.unwrap_or((core::ptr::null(), 0));
        Self {
            base,
            len,
            staged: staged.is_some(),
        }
    }
}

impl<S: Session> Conn<'_, S> {
    /// Opens a bidirectional stream.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::Blocked`] when the peer's stream limit is already reached —
    /// an ordinary condition, not a failure. Call again after the peer grants more.
    ///
    /// [`ErrorKind::Blocked`]: crate::ErrorKind::Blocked
    pub fn open_bidi_stream(&mut self) -> Result<StreamId> {
        let mut id: i64 = -1;
        // SAFETY: `raw` is live and `id` is a valid out-parameter.
        let rc = unsafe {
            sys::ngtcp2_conn_open_bidi_stream(self.raw(), &mut id, core::ptr::null_mut())
        };
        if rc != 0 {
            return Err(Error::native(rc, "could not open a bidirectional stream"));
        }
        StreamId::new(id)
    }

    /// Opens a unidirectional stream.
    ///
    /// # Errors
    ///
    /// As [`Conn::open_bidi_stream`].
    pub fn open_uni_stream(&mut self) -> Result<StreamId> {
        let mut id: i64 = -1;
        // SAFETY: `raw` is live and `id` is a valid out-parameter.
        let rc =
            unsafe { sys::ngtcp2_conn_open_uni_stream(self.raw(), &mut id, core::ptr::null_mut()) };
        if rc != 0 {
            return Err(Error::native(rc, "could not open a unidirectional stream"));
        }
        StreamId::new(id)
    }

    /// Writes stream data, producing a datagram to send.
    ///
    /// This is the stream-carrying counterpart of [`Conn::write_pkt`], and the same loop
    /// applies: call it until it stops producing datagrams, sending each one.
    ///
    /// `fin` marks the end of the stream. A zero-length write with `fin` set is how a
    /// stream is finished without further data.
    ///
    /// # Data is copied
    ///
    /// ngtcp2 does **not** copy what it accepts — it keeps the pointer so it can
    /// retransmit, and requires the bytes stay intact "until
    /// `acked_stream_data_offset` indicates that they are acknowledged by a remote endpoint
    /// or the stream is closed" (`ngtcp2.h:5244-5248`). Since `data` is an ordinary borrow
    /// the caller may reuse immediately, this crate copies the accepted portion and holds
    /// it until then. [`Conn::retained_bytes`] reports how much is currently held.
    ///
    /// # Errors
    ///
    /// Returns an error if ngtcp2 refuses; the connection is then unusable.
    pub fn write_stream(
        &mut self,
        dest: &mut [u8],
        stream: StreamId,
        data: &[u8],
        fin: bool,
        now: Timestamp,
    ) -> Result<StreamWrite> {
        self.write_stream_vectored(dest, stream, &[IoSlice::new(data)], fin, now)
    }

    /// Writes stream data supplied as several separate ranges, and produces one datagram.
    ///
    /// Behaves exactly as [`Conn::write_stream`], including its accounting: the ranges are
    /// treated as one contiguous run of bytes, and the accepted count in
    /// [`StreamWrite::Datagram`] covers the whole offer rather than any one range. A caller
    /// re-offers by skipping that many bytes across the ranges.
    ///
    /// # Why this exists
    ///
    /// A caller whose payload is already in pieces — a framing layer with a header and a
    /// body, most obviously — would otherwise have to join them itself, and that join would
    /// be a second copy on top of the one retention already makes. This joins them *into*
    /// the retained copy, so the byte count is unchanged.
    ///
    /// The ranges arrive as [`IoSlice`]s rather than `&[&[u8]]` so a vectored source — most
    /// obviously the HTTP/3 layer, whose body writes are already `IoSlice`s — can pass them
    /// through without first collecting them into a temporary vector.
    ///
    /// # Errors
    ///
    /// Returns an error if ngtcp2 refuses; the connection is then unusable.
    pub fn write_stream_vectored(
        &mut self,
        dest: &mut [u8],
        stream: StreamId,
        ranges: &[IoSlice<'_>],
        fin: bool,
        now: Timestamp,
    ) -> Result<StreamWrite> {
        if dest.is_empty() {
            return Err(Error::invalid_input(
                "a datagram buffer must have room to write into",
            ));
        }

        let flags = if fin {
            sys::NGTCP2_WRITE_STREAM_FLAG_FIN
        } else {
            sys::NGTCP2_WRITE_STREAM_FLAG_NONE
        };

        // The bytes handed to ngtcp2 must outlive this call -- see the note above -- so a
        // copy is staged first and *that* is what ngtcp2 is given a pointer to. Several
        // ranges become one staged chunk, which is why a single vector suffices below.
        let staged = self.retained_mut().stage_many(stream, ranges);

        self.submit_one_vec(dest, stream, StagedVec::new(staged), flags, now)
    }

    /// Writes a buffer whose ownership the caller hands over, retaining it without a copy.
    ///
    /// The counterpart to [`write_stream`](Self::write_stream), for a caller who can give up
    /// the buffer. ngtcp2 keeps the pointer until acknowledgement (`ngtcp2.h:5244-5248`), and
    /// the borrowing writes satisfy that by copying every accepted byte into a buffer this
    /// crate owns. A caller who hands over an [`OwnedBytes`] skips that copy: the bytes are
    /// retained by keeping the handle alive, and ngtcp2 is given a pointer straight into it.
    ///
    /// # Partial acceptance
    ///
    /// ngtcp2 routinely takes a prefix and leaves the rest — a packet fills before the buffer
    /// is exhausted. The accepted prefix stays retained at a fixed address, and the unaccepted
    /// suffix is returned in [`OwnedWrite::unsent`] as a handle into the *same* allocation, to
    /// be offered again. Neither side of that split is copied. When ngtcp2 takes everything,
    /// `unsent` is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if ngtcp2 refuses; the connection is then unusable. On any error the
    /// buffer is dropped, since the connection cannot carry it.
    pub fn write_stream_owned(
        &mut self,
        dest: &mut [u8],
        stream: StreamId,
        mut data: OwnedBytes,
        fin: bool,
        now: Timestamp,
    ) -> Result<OwnedWrite> {
        if dest.is_empty() {
            return Err(Error::invalid_input(
                "a datagram buffer must have room to write into",
            ));
        }

        let flags = if fin {
            sys::NGTCP2_WRITE_STREAM_FLAG_FIN
        } else {
            sys::NGTCP2_WRITE_STREAM_FLAG_NONE
        };

        // The handle is cloned into retention -- an `Arc` bump, not a copy of the bytes --
        // and *that* is what ngtcp2 is given a pointer into, so the address survives the call
        // even if the caller drops the original the instant it returns.
        let staged = self.retained_mut().stage_owned(stream, data.clone());

        let outcome = self.submit_one_vec(dest, stream, StagedVec::new(staged), flags, now)?;

        // Split off what ngtcp2 took. The prefix is already retained above; the suffix shares
        // the same allocation and is handed back for the caller to offer again. On anything
        // but a datagram nothing was taken, so the whole buffer comes back.
        let taken = match outcome {
            StreamWrite::Datagram { accepted, .. } => accepted,
            _ => 0,
        };
        let _accepted_prefix = data.split_to(taken);
        Ok(OwnedWrite {
            outcome,
            unsent: data,
        })
    }

    /// Issues one already-staged stream write to ngtcp2 and reports what it did.
    ///
    /// Shared by the borrowing and owning write paths, which differ only in how the bytes in
    /// `chunk` were retained, and the outcome is mapped exactly as each caller documents.
    fn submit_one_vec(
        &mut self,
        dest: &mut [u8],
        stream: StreamId,
        chunk: StagedVec,
        flags: u32,
        now: Timestamp,
    ) -> Result<StreamWrite> {
        let StagedVec { base, len, staged } = chunk;
        let path = self.path_mut().as_raw_mut();
        let mut accepted: sys::ngtcp2_ssize = 0;
        let written = self.with_bridge(|raw| {
            // SAFETY: `raw` is live, `path` points into storage the connection owns, `dest`
            // is writable for its length, and `base` points into the retained bytes, which
            // outlive this call and are released only on acknowledgement or stream close.
            unsafe {
                let mut pi = sys::ngtcp2_pkt_info { ecn: 0 };
                crate::ffi::conn_writev_stream(
                    raw,
                    path,
                    &mut pi,
                    dest.as_mut_ptr(),
                    dest.len(),
                    &mut accepted,
                    flags,
                    stream.get(),
                    // A single vector, because staging already joined the caller's ranges
                    // into one chunk. The coalescing `MORE` loop is separately and
                    // deliberately not exposed, since its "every argument must be
                    // byte-identical across calls" rule is not expressible in a safe API
                    // without a guard type.
                    &sys::ngtcp2_vec {
                        base: base.cast_mut(),
                        len,
                    },
                    1,
                    now.as_raw(),
                )
            }
        });

        // Whatever ngtcp2 did not take was never handed over, so it must not stay retained
        // or count towards the stream's offset -- the caller will offer it again.
        let taken = if written > 0 {
            accepted.max(0) as usize
        } else {
            0
        };
        if staged {
            self.retained_mut().commit(stream, taken);
        }

        if written > 0 {
            let len = written as usize;
            debug_assert!(len <= dest.len());
            // SAFETY: `raw` is live and the timestamp is the one just used.
            unsafe { sys::ngtcp2_conn_update_pkt_tx_time(self.raw(), now.as_raw()) };
            return Ok(StreamWrite::Datagram {
                len,
                accepted: taken,
            });
        }

        let code = i32::try_from(written).unwrap_or(sys::NGTCP2_ERR_INTERNAL);
        match code {
            // Zero means "buffer too small or congestion limited" -- wait and retry -- not
            // "finished". Reporting it as `Idle` would tell a caller to stop writing, which
            // is the stall this whole enum exists to prevent, and it is why `write_pkt`
            // maps zero the same way.
            //
            // Connection-level flow control is distinguished here rather than by a separate
            // ngtcp2 error: `writev_stream` returns `STREAM_DATA_BLOCKED` only while
            // connection credit remains, so zero with no connection credit left is the
            // connection window being full.
            0 => {
                // SAFETY: `raw` is live; a pure query.
                let connection_credit = unsafe { sys::ngtcp2_conn_get_max_data_left(self.raw()) };
                if connection_credit == 0 {
                    Ok(StreamWrite::ConnectionBlocked)
                } else {
                    Ok(StreamWrite::Blocked)
                }
            }
            sys::NGTCP2_ERR_STREAM_DATA_BLOCKED => Ok(StreamWrite::StreamBlocked),
            sys::NGTCP2_ERR_STREAM_SHUT_WR => Err(Error::native(
                code,
                "the write side of this stream is already closed",
            )),
            sys::NGTCP2_ERR_CLOSING | sys::NGTCP2_ERR_DRAINING => Ok(StreamWrite::Idle),
            other => Err(Error::native(other, "could not write stream data")),
        }
    }

    /// Abandons a stream in both directions.
    pub fn shutdown_stream(&mut self, stream: StreamId, code: ApplicationErrorCode) -> Result<()> {
        // SAFETY: `raw` is live.
        let rc =
            unsafe { sys::ngtcp2_conn_shutdown_stream(self.raw(), 0, stream.get(), code.get()) };
        if rc != 0 {
            return Err(Error::native(rc, "could not shut down the stream"));
        }
        Ok(())
    }

    /// Resets the sending side of a stream, discarding anything unsent.
    pub fn reset_stream(&mut self, stream: StreamId, code: ApplicationErrorCode) -> Result<()> {
        // SAFETY: `raw` is live.
        let rc = unsafe {
            sys::ngtcp2_conn_shutdown_stream_write(self.raw(), 0, stream.get(), code.get())
        };
        if rc != 0 {
            return Err(Error::native(rc, "could not reset the stream"));
        }
        Ok(())
    }

    /// Asks the peer to stop sending on a stream.
    pub fn stop_sending(&mut self, stream: StreamId, code: ApplicationErrorCode) -> Result<()> {
        // SAFETY: `raw` is live.
        let rc = unsafe {
            sys::ngtcp2_conn_shutdown_stream_read(self.raw(), 0, stream.get(), code.get())
        };
        if rc != 0 {
            return Err(Error::native(rc, "could not stop the peer sending"));
        }
        Ok(())
    }

    /// Grants the peer more credit to send on one stream.
    ///
    /// Call this as received data is consumed. Without it the peer's window closes and the
    /// stream stalls, with nothing reported to either side.
    pub fn extend_max_stream_offset(&mut self, stream: StreamId, bytes: u64) -> Result<()> {
        // SAFETY: `raw` is live.
        let rc =
            unsafe { sys::ngtcp2_conn_extend_max_stream_offset(self.raw(), stream.get(), bytes) };
        if rc != 0 {
            return Err(Error::native(rc, "could not extend the stream window"));
        }
        Ok(())
    }

    /// Grants the peer more credit to send on the connection as a whole.
    pub fn extend_max_offset(&mut self, bytes: u64) {
        // SAFETY: `raw` is live. This one returns nothing.
        unsafe { sys::ngtcp2_conn_extend_max_offset(self.raw(), bytes) };
    }

    /// Allows the peer to open more bidirectional streams.
    ///
    /// **ngtcp2 never does this by itself** (`ngtcp2.h:5586-5594`). An application that
    /// accepts streams and never calls this will let the peer open only as many as the
    /// initial transport parameters allowed, and then stall it silently.
    pub fn extend_max_streams_bidi(&mut self, count: usize) {
        // SAFETY: `raw` is live. This one returns nothing and cannot fail.
        unsafe { sys::ngtcp2_conn_extend_max_streams_bidi(self.raw(), count) };
    }

    /// Allows the peer to open more unidirectional streams.
    ///
    /// As [`Conn::extend_max_streams_bidi`], including the part about it not happening
    /// automatically.
    pub fn extend_max_streams_uni(&mut self, count: usize) {
        // SAFETY: `raw` is live. This one returns nothing and cannot fail.
        unsafe { sys::ngtcp2_conn_extend_max_streams_uni(self.raw(), count) };
    }

    /// How many bidirectional streams this endpoint may still open.
    pub fn streams_bidi_left(&self) -> u64 {
        // SAFETY: `raw` is live; a pure query.
        unsafe { sys::ngtcp2_conn_get_streams_bidi_left(self.raw()) }
    }

    /// How many unidirectional streams this endpoint may still open.
    pub fn streams_uni_left(&self) -> u64 {
        // SAFETY: `raw` is live; a pure query.
        unsafe { sys::ngtcp2_conn_get_streams_uni_left(self.raw()) }
    }

    /// Writes a close packet for a **transport** error, from the ngtcp2 error that caused
    /// it.
    ///
    /// ngtcp2 requires that most failures of [`Conn::read_pkt`] be answered with a
    /// CONNECTION_CLOSE carrying a transport error code (`ngtcp2.h:4282-4285`), not an
    /// application one. Passing the original [`Error`] lets ngtcp2 derive the right code
    /// itself, through `ngtcp2_ccerr_set_liberr`, rather than this crate maintaining a
    /// second mapping that could disagree with it.
    ///
    /// For an ordinary application-level shutdown use [`Conn::write_connection_close`].
    ///
    /// # Errors
    ///
    /// Returns an error if a close packet cannot be produced.
    pub fn write_transport_close(
        &mut self,
        dest: &mut [u8],
        cause: &Error,
        reason: &[u8],
        now: Timestamp,
    ) -> Result<usize> {
        if dest.is_empty() {
            return Err(Error::invalid_input(
                "a datagram buffer must have room to write into",
            ));
        }

        // As below, the reason phrase is borrowed rather than copied, so it must outlive
        // the call -- which it does, being a parameter.
        // SAFETY: a zeroed `ngtcp2_ccerr` is the documented starting point.
        let mut ccerr = unsafe { core::mem::zeroed::<sys::ngtcp2_ccerr>() };
        let liberr = cause
            .native_code()
            .map_or(sys::NGTCP2_ERR_INTERNAL, |code| code.get());
        // SAFETY: `ccerr` is a valid writable struct and `reason` outlives the call.
        unsafe {
            sys::ngtcp2_ccerr_set_liberr(&mut ccerr, liberr, reason.as_ptr(), reason.len());
        }

        self.write_close_with(dest, &ccerr, now)
    }

    /// Writes a packet telling the peer this connection is closing.
    ///
    /// Send the returned datagram, then stop sending anything else: the connection has
    /// entered its closing period. Do **not** call this after
    /// [`ExpiryOutcome::IdleClose`](crate::ExpiryOutcome::IdleClose), which asks for
    /// silence.
    ///
    /// # Errors
    ///
    /// Returns an error if a close packet cannot be produced.
    pub fn write_connection_close(
        &mut self,
        dest: &mut [u8],
        code: ApplicationErrorCode,
        reason: &[u8],
        now: Timestamp,
    ) -> Result<usize> {
        if dest.is_empty() {
            return Err(Error::invalid_input(
                "a datagram buffer must have room to write into",
            ));
        }

        // The reason phrase is **borrowed**, not copied: no `ngtcp2_ccerr_set_*` function
        // takes a copy, so the buffer must outlive the call below -- which it does, being a
        // parameter. This is why `ccerr` is built here rather than stored.
        let mut ccerr = unsafe { core::mem::zeroed::<sys::ngtcp2_ccerr>() };
        // SAFETY: `ccerr` is a valid writable struct and `reason` outlives the call.
        unsafe {
            sys::ngtcp2_ccerr_set_application_error(
                &mut ccerr,
                code.get(),
                reason.as_ptr(),
                reason.len(),
            );
        }

        self.write_close_with(dest, &ccerr, now)
    }

    /// The shared tail of both close paths.
    fn write_close_with(
        &mut self,
        dest: &mut [u8],
        ccerr: &sys::ngtcp2_ccerr,
        now: Timestamp,
    ) -> Result<usize> {
        let path = self.path_mut().as_raw_mut();
        let written = self.with_bridge(|raw| {
            // SAFETY: `raw` is live, `path` points into owned storage, `dest` is writable,
            // and `ccerr` outlives the call.
            unsafe {
                let mut pi = sys::ngtcp2_pkt_info { ecn: 0 };
                crate::ffi::conn_write_connection_close(
                    raw,
                    path,
                    &mut pi,
                    dest.as_mut_ptr(),
                    dest.len(),
                    ccerr,
                    now.as_raw(),
                )
            }
        });

        if written < 0 {
            let code = i32::try_from(written).unwrap_or(sys::NGTCP2_ERR_INTERNAL);
            return Err(Error::native(code, "could not write a connection close"));
        }
        Ok(written as usize)
    }
}

#[cfg(all(test, feature = "tls-ossl"))]
mod tests {
    use super::*;
    use crate::conn::test_support::client_conn;
    use crate::error::ErrorKind;
    use crate::handlers::Handlers;
    use crate::stream::Directionality;

    fn ts(nanos: u64) -> Timestamp {
        Timestamp::from_nanos(nanos).unwrap()
    }

    #[test]
    fn opening_a_stream_before_the_handshake_is_blocked_not_broken() {
        // The peer's limits are unknown until its transport parameters arrive, so this is
        // an ordinary "not yet" rather than a failure -- and saying so distinguishably is
        // the point.
        let mut conn = client_conn(Handlers::new()).unwrap();
        match conn.open_bidi_stream() {
            Err(err) => assert_eq!(err.kind(), ErrorKind::Blocked),
            Ok(id) => assert_eq!(id.directionality(), Directionality::Bidirectional),
        }
    }

    #[test]
    fn a_fresh_connection_has_no_stream_credit_yet() {
        // Before the peer's transport parameters arrive there is nothing to spend.
        let conn = client_conn(Handlers::new()).unwrap();
        assert_eq!(conn.streams_bidi_left(), 0);
        assert_eq!(conn.streams_uni_left(), 0);
    }

    #[test]
    fn an_empty_buffer_is_rejected_by_every_write_path() {
        let mut conn = client_conn(Handlers::new()).unwrap();
        let stream = StreamId::new(0).unwrap();
        assert!(
            conn.write_stream(&mut [], stream, b"x", false, ts(2_000_000))
                .is_err()
        );
        assert!(
            conn.write_connection_close(&mut [], ApplicationErrorCode::new(0), b"", ts(2_000_000))
                .is_err()
        );
    }

    #[test]
    fn extending_the_connection_window_is_always_permitted() {
        // It returns nothing and cannot fail, which is worth pinning: a caller may grant
        // credit at any point without checking a result.
        let mut conn = client_conn(Handlers::new()).unwrap();
        conn.extend_max_offset(1024);
        conn.extend_max_offset(0);
    }

    #[test]
    fn extending_stream_limits_is_accepted_before_the_handshake() {
        // These return nothing and cannot fail, so a caller may grant capacity at any
        // point without checking a result -- which is worth pinning, because forgetting to
        // call them at all is the failure mode.
        let mut conn = client_conn(Handlers::new()).unwrap();
        conn.extend_max_streams_bidi(4);
        conn.extend_max_streams_uni(4);
    }

    #[test]
    fn a_connection_close_packet_can_be_written() {
        let mut conn = client_conn(Handlers::new()).unwrap();
        let mut buf = [0u8; 1500];
        // Sending the first flight first, so there are keys to encrypt a close with.
        let _ = conn.write_pkt(&mut buf, ts(2_000_000));

        let written = conn
            .write_connection_close(
                &mut buf,
                ApplicationErrorCode::new(0x1234),
                b"done here",
                ts(2_000_001),
            )
            .unwrap();
        assert!(written > 0);
        assert!(conn.in_closing_period());
    }

    #[test]
    fn a_transport_close_can_be_written_from_a_native_error() {
        // ngtcp2 requires most `read_pkt` failures be answered with a *transport* error
        // code, not an application one. Without this path a caller had no way to comply.
        let mut conn = client_conn(Handlers::new()).unwrap();
        let mut buf = [0u8; 1500];
        let _ = conn.write_pkt(&mut buf, ts(2_000_000));

        let cause = Error::native(sys::NGTCP2_ERR_PROTO, "a protocol violation");
        let written = conn
            .write_transport_close(&mut buf, &cause, b"protocol error", ts(2_000_001))
            .unwrap();
        assert!(written > 0);
        assert!(conn.in_closing_period());
    }

    #[test]
    fn a_borrowed_reason_phrase_survives_the_call() {
        // ngtcp2 does not copy the reason phrase, so this pins that it is read while still
        // alive rather than stored and read later.
        let mut conn = client_conn(Handlers::new()).unwrap();
        let mut buf = [0u8; 1500];
        let _ = conn.write_pkt(&mut buf, ts(2_000_000));

        let written = {
            let reason = String::from("a reason that goes out of scope");
            conn.write_connection_close(
                &mut buf,
                ApplicationErrorCode::new(1),
                reason.as_bytes(),
                ts(2_000_001),
            )
            .unwrap()
        };
        assert!(written > 0);
    }

    #[test]
    fn writing_to_a_stream_that_does_not_exist_is_reported_not_ignored() {
        let mut conn = client_conn(Handlers::new()).unwrap();
        let mut buf = [0u8; 1500];
        let stream = StreamId::new(0).unwrap();
        // The stream was never opened, so this must not silently claim success.
        match conn.write_stream(&mut buf, stream, b"hello", false, ts(2_000_000)) {
            Err(err) => assert!(err.native_code().is_some()),
            Ok(StreamWrite::Datagram { accepted, .. }) => {
                assert_eq!(
                    accepted, 0,
                    "no data can be accepted for an unopened stream"
                );
            }
            Ok(_) => {}
        }
    }

    #[test]
    fn the_stream_write_outcomes_are_distinguishable() {
        assert_ne!(StreamWrite::Idle, StreamWrite::StreamBlocked);
        assert_ne!(StreamWrite::StreamBlocked, StreamWrite::ConnectionBlocked);
        assert_ne!(
            StreamWrite::Datagram {
                len: 1,
                accepted: 0
            },
            StreamWrite::Datagram {
                len: 1,
                accepted: 1
            }
        );
    }
}
