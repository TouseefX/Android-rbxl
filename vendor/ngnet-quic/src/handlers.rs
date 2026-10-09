//! Events a connection reports to its owner.
//!
//! ngtcp2 delivers everything through C callbacks. Rather than exposing forty-seven of
//! them, this crate groups the ones an application actually acts on into a handler struct
//! built through [`Handlers`], following the shape `ngnet-h3` uses.
//!
//! Handlers run **inside** a call into ngtcp2, with the connection mutably borrowed. They
//! therefore cannot call back into the connection — and three ngtcp2 entry points
//! (`read_pkt`, `writev_stream`, `write_connection_close`) explicitly forbid it anyway
//! (`ngtcp2.h:4256`, `:5318`, `:6665`). Anything a handler wants to *do* should be recorded
//! and acted on after the call returns.
//!
//! That restriction is about *these* handlers rather than about callbacks in general. The
//! crypto callbacks in [`crate::tls_bridge`] run in the same position and must do the
//! opposite: ngtcp2 requires them to install keys and submit handshake data on the very
//! connection that invoked them. The difference is which entry points are involved, not
//! whether a callback is running.
//!
//! A handler must not panic: unwinding into a C stack frame aborts the process.

use crate::cid::ConnectionId;
use crate::error::ApplicationErrorCode;
use crate::stream::StreamId;

/// Why a stream ended.
///
/// QUIC closes the two directions of a stream independently, so there are two error codes
/// and either may be absent. A direction with no code closed cleanly.
///
/// ngtcp2's own example: a client receives a `STOP_SENDING` frame and answers with
/// `RESET_STREAM` carrying the same code, which is reported as the *sending* side's code;
/// meanwhile the response body arrived intact, so the *receiving* side has no code at all
/// (`ngtcp2.h:3683-3712`).
///
/// This comes from `stream_close2` rather than the older `stream_close`, which collapses
/// both directions into one code and cannot express the case above.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub struct StreamCloseReason {
    receiving: Option<ApplicationErrorCode>,
    sending: Option<ApplicationErrorCode>,
}

impl StreamCloseReason {
    /// A reason with the given per-direction codes.
    pub fn new(
        receiving: Option<ApplicationErrorCode>,
        sending: Option<ApplicationErrorCode>,
    ) -> Self {
        Self { receiving, sending }
    }

    /// The code that shut down the receiving side, if it did not end cleanly.
    pub fn receiving(&self) -> Option<ApplicationErrorCode> {
        self.receiving
    }

    /// The code that shut down the sending side, if it did not end cleanly.
    pub fn sending(&self) -> Option<ApplicationErrorCode> {
        self.sending
    }

    /// Whether both directions ended cleanly.
    pub fn is_clean(&self) -> bool {
        self.receiving.is_none() && self.sending.is_none()
    }
}

/// A handler for stream data: the identifier, the bytes, and whether they end the stream.
///
/// Named rather than written inline because the borrowed slice makes the closure type long
/// enough to obscure the signature it appears in.
///
/// The `Send` bound is load-bearing. A [`crate::Conn`] is `Send`, and it owns its handlers;
/// without this bound a handler capturing an `Rc` or a `RefCell` could be moved to another
/// thread while a clone stayed behind, which is a data race on a non-atomic refcount
/// reachable from entirely safe code. The entropy source carries the same bound for the
/// same reason.
type StreamDataHandler<'a> = Box<dyn FnMut(StreamId, &[u8], bool) + Send + 'a>;

/// A handler taking a stream and an application error code.
type StreamErrorHandler<'a> = Box<dyn FnMut(StreamId, ApplicationErrorCode) + Send + 'a>;

/// A handler taking one unframed QUIC DATAGRAM payload.
type DatagramHandler<'a> = Box<dyn FnMut(&[u8]) + Send + 'a>;

/// A handler taking a connection identifier.
type ConnectionIdHandler<'a> = Box<dyn FnMut(&ConnectionId) + Send + 'a>;

/// A source-CID generator. `false` aborts the native callback rather than emitting a CID.
type ConnectionIdGenerator<'a> = Box<dyn FnMut(&mut [u8]) -> bool + Send + 'a>;

/// The callbacks an application supplies.
///
/// Every field is optional; an absent handler means the event is ignored. The defaults are
/// chosen so that a connection with no handlers at all is still correct, merely silent.
#[derive(Default)]
pub struct Handlers<'a> {
    pub(crate) on_stream_data: Option<StreamDataHandler<'a>>,
    pub(crate) on_stream_open: Option<Box<dyn FnMut(StreamId) + Send + 'a>>,
    pub(crate) on_stream_close: Option<Box<dyn FnMut(StreamId, StreamCloseReason) + Send + 'a>>,
    pub(crate) on_stream_reset: Option<StreamErrorHandler<'a>>,
    pub(crate) on_stop_sending: Option<StreamErrorHandler<'a>>,
    pub(crate) on_acked_stream_data: Option<Box<dyn FnMut(StreamId, u64) + Send + 'a>>,
    pub(crate) on_handshake_completed: Option<Box<dyn FnMut() + Send + 'a>>,
    pub(crate) on_datagram: Option<DatagramHandler<'a>>,
    pub(crate) on_new_connection_id: Option<ConnectionIdHandler<'a>>,
    pub(crate) on_remove_connection_id: Option<ConnectionIdHandler<'a>>,
    pub(crate) connection_id_generator: Option<ConnectionIdGenerator<'a>>,
    pub(crate) on_extend_max_local_streams_bidi: Option<Box<dyn FnMut(u64) + Send + 'a>>,
    pub(crate) on_extend_max_local_streams_uni: Option<Box<dyn FnMut(u64) + Send + 'a>>,
}

impl<'a> Handlers<'a> {
    /// An empty handler set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Called with data received on a stream, and whether that data ends it.
    ///
    /// The slice borrows ngtcp2's own buffer and is valid only for the call.
    pub fn on_stream_data(mut self, f: impl FnMut(StreamId, &[u8], bool) + Send + 'a) -> Self {
        self.on_stream_data = Some(Box::new(f));
        self
    }

    /// Called when the peer opens a stream.
    pub fn on_stream_open(mut self, f: impl FnMut(StreamId) + Send + 'a) -> Self {
        self.on_stream_open = Some(Box::new(f));
        self
    }

    /// Called when a stream is fully closed.
    pub fn on_stream_close(
        mut self,
        f: impl FnMut(StreamId, StreamCloseReason) + Send + 'a,
    ) -> Self {
        self.on_stream_close = Some(Box::new(f));
        self
    }

    /// Called when the peer resets a stream it was sending on.
    pub fn on_stream_reset(
        mut self,
        f: impl FnMut(StreamId, ApplicationErrorCode) + Send + 'a,
    ) -> Self {
        self.on_stream_reset = Some(Box::new(f));
        self
    }

    /// Called when the peer asks this endpoint to stop sending on a stream.
    pub fn on_stop_sending(
        mut self,
        f: impl FnMut(StreamId, ApplicationErrorCode) + Send + 'a,
    ) -> Self {
        self.on_stop_sending = Some(Box::new(f));
        self
    }

    /// Called with the number of bytes the peer has acknowledged on a stream.
    ///
    /// This is what releases buffers retained for retransmission. An application that sends
    /// large bodies and ignores this will hold every byte it ever sent.
    pub fn on_acked_stream_data(mut self, f: impl FnMut(StreamId, u64) + Send + 'a) -> Self {
        self.on_acked_stream_data = Some(Box::new(f));
        self
    }

    /// Called once, when the TLS handshake completes.
    pub fn on_handshake_completed(mut self, f: impl FnMut() + Send + 'a) -> Self {
        self.on_handshake_completed = Some(Box::new(f));
        self
    }

    /// Called with the payload of each received QUIC DATAGRAM frame.
    ///
    /// The slice borrows ngtcp2's receive buffer and is valid only for this call. Copy it in
    /// the handler if it must outlive the callback.
    pub fn on_datagram(mut self, f: impl FnMut(&[u8]) + Send + 'a) -> Self {
        self.on_datagram = Some(Box::new(f));
        self
    }

    /// Called when this endpoint mints a connection identifier the peer may route to.
    ///
    /// A connection is reachable by several identifiers at once, and the set changes over
    /// its life: ngtcp2 issues new ones and retires old ones on its own schedule. An owner
    /// that routes datagrams by identifier — anything multiplexing connections over one
    /// socket — must track both this and [`Handlers::on_remove_connection_id`], or its
    /// table goes stale the first time an identifier rotates and the connection quietly
    /// stops receiving.
    ///
    /// The identifier is borrowed for the call; copy it if you need to keep it. Use
    /// [`crate::Conn::scids`] to learn the identifiers a connection already has, which this
    /// does not replay.
    pub fn on_new_connection_id(mut self, f: impl FnMut(&ConnectionId) + Send + 'a) -> Self {
        self.on_new_connection_id = Some(Box::new(f));
        self
    }

    /// Called when an identifier this endpoint issued is retired.
    ///
    /// The other half of [`Handlers::on_new_connection_id`]. Ignoring it leaves a routing
    /// table growing without bound and keeps delivering to identifiers the peer has been
    /// told to stop using.
    pub fn on_remove_connection_id(mut self, f: impl FnMut(&ConnectionId) + Send + 'a) -> Self {
        self.on_remove_connection_id = Some(Box::new(f));
        self
    }

    /// Supplies bytes for each source connection ID ngtcp2 creates after the initial one.
    /// The slice has the CID length requested by ngtcp2; return `false` to fail creation.
    /// The initial source CID is supplied separately through `ConnBuilder::scid`.
    pub fn connection_id_generator(
        mut self,
        f: impl FnMut(&mut [u8]) -> bool + Send + 'a,
    ) -> Self {
        self.connection_id_generator = Some(Box::new(f));
        self
    }

    /// Called when the peer raises the number of bidirectional streams this endpoint may
    /// open, with the new cumulative total.
    ///
    /// This is what makes a refused open worth retrying. Opening a stream past the peer's
    /// limit fails as blocked rather than as an error, because the condition is temporary —
    /// but nothing else announces that it has lifted, so a caller that waits without
    /// listening here waits indefinitely.
    pub fn on_extend_max_local_streams_bidi(mut self, f: impl FnMut(u64) + Send + 'a) -> Self {
        self.on_extend_max_local_streams_bidi = Some(Box::new(f));
        self
    }

    /// Called when the peer raises the number of unidirectional streams this endpoint may
    /// open, with the new cumulative total.
    ///
    /// See [`Handlers::on_extend_max_local_streams_bidi`].
    pub fn on_extend_max_local_streams_uni(mut self, f: impl FnMut(u64) + Send + 'a) -> Self {
        self.on_extend_max_local_streams_uni = Some(Box::new(f));
        self
    }
}

impl core::fmt::Debug for Handlers<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Handlers")
            .field("on_stream_data", &self.on_stream_data.is_some())
            .field("on_stream_open", &self.on_stream_open.is_some())
            .field("on_stream_close", &self.on_stream_close.is_some())
            .field("on_acked_stream_data", &self.on_acked_stream_data.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_handler_set_registers_nothing() {
        let handlers = Handlers::new();
        assert!(handlers.on_stream_data.is_none());
        assert!(handlers.on_handshake_completed.is_none());
    }

    #[test]
    fn registering_a_handler_records_it() {
        let handlers = Handlers::new()
            .on_stream_data(|_, _, _| {})
            .on_handshake_completed(|| {});
        assert!(handlers.on_stream_data.is_some());
        assert!(handlers.on_handshake_completed.is_some());
        assert!(handlers.on_stream_open.is_none());
    }

    #[test]
    fn a_handler_can_borrow_from_its_environment() {
        // The lifetime parameter exists for this: recording events into a local is the
        // ordinary way to use these, since a handler cannot call back into the connection.
        let mut seen = Vec::new();
        {
            let mut handlers = Handlers::new().on_stream_open(|id| seen.push(id));
            if let Some(f) = handlers.on_stream_open.as_mut() {
                f(StreamId::new(0).unwrap());
            }
        }
        assert_eq!(seen.len(), 1);
    }
}
