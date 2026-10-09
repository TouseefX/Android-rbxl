//! A live connection, from the caller's side.
//!
//! Everything here is a request to the driver rather than an operation on a connection: the
//! [`Conn`](crate::Conn) itself lives in the driver and nothing else may touch it. What
//! makes that bearable is that the operations a caller wants — open a stream, write, read,
//! close — are all things that have to be sequenced against arriving packets anyway.
//!
//! # Why reads are per-stream but writes go through the driver
//!
//! Received bytes are already demultiplexed by the time they arrive: the core reports them
//! with a stream identifier, so a per-stream reader is just a queue. Writes are not
//! symmetrical, because ngtcp2 fills a *packet* and pulls stream data as it goes — the
//! decision about which stream gets space in the next packet is the driver's, not the
//! caller's. A write therefore queues bytes and waits, rather than producing a packet.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::ApplicationErrorCode;
use crate::stream::{Directionality, StreamId};

use super::error::{Error, ErrorKind, Result};
use super::shared::{Command, ConnectionShared, Observed};

/// Bytes and end-of-stream state a caller has not yet read.
#[derive(Default)]
struct Incoming {
    bytes: Vec<u8>,
    finished: bool,
    /// Set when the peer reset the stream, with the code it sent.
    ///
    /// A reset is not an ordinary end-of-stream: the bytes already delivered are still
    /// valid, but no more are coming and the peer said why. A reader that reported it as a
    /// clean finish would hide an error the application chose to send.
    reset: Option<ApplicationErrorCode>,
}

/// A live connection.
///
/// Dropping it closes the connection: there is nothing else it could reasonably mean, and
/// leaving a connection running with nothing able to reach it would leak an entry in the
/// endpoint until its idle timeout.
pub struct Connection {
    shared: Arc<ConnectionShared>,
    /// Bytes delivered but not yet read, per stream.
    ///
    /// Held on the handle rather than in the shared state because only the handle reads
    /// them, and moving them here keeps the lock's critical sections short.
    incoming: HashMap<i64, Incoming>,
    /// Streams the peer opened that have not been accepted.
    pending_streams: Vec<StreamId>,
    /// Streams the peer asked this endpoint to stop sending on, with the code it sent.
    stopped: HashMap<i64, ApplicationErrorCode>,
    /// Bytes the peer has acknowledged, per stream.
    acked: HashMap<i64, u64>,
}

impl Connection {
    /// Wraps shared state in a handle.
    pub(crate) fn new(shared: Arc<ConnectionShared>) -> Self {
        Self {
            shared,
            incoming: HashMap::new(),
            pending_streams: Vec::new(),
            stopped: HashMap::new(),
            acked: HashMap::new(),
        }
    }

    /// Whether the handshake has completed.
    pub fn is_established(&self) -> bool {
        self.shared.is_established()
    }

    /// Whether the connection has finished.
    pub fn is_closed(&self) -> bool {
        self.shared.is_closed()
    }

    /// Bytes of sent stream data the peer has not yet acknowledged.
    ///
    /// The transport does not copy what it is given, so this crate does, and holds the copy
    /// until it is acknowledged. A peer that stops acknowledging makes this grow, which is
    /// the honest signal that memory is being held on its behalf.
    pub fn retained_bytes(&self) -> u64 {
        self.shared.retained_bytes()
    }

    /// Tells the driver the application has consumed bytes, so credit can be returned.
    ///
    /// Deliberately *not* done when the bytes arrive. Returning credit on delivery makes the
    /// flow-control window advisory rather than real: a peer could keep sending past a
    /// reader that never reads, and the bytes would accumulate in this process until it ran
    /// out of memory. Tied to consumption, the window is what bounds the buffer.
    fn consumed(&self, stream: StreamId, bytes: usize) {
        if bytes == 0 {
            return;
        }
        self.shared.push(Command::ExtendCredit {
            stream,
            bytes: bytes as u64,
        });
    }

    /// Moves everything the driver has recorded into this handle.
    fn absorb(&mut self) {
        let observed = self.shared.take_observed();
        for event in observed {
            match event {
                Observed::Data(stream, bytes, fin) => {
                    let slot = self.incoming.entry(stream.get()).or_default();
                    slot.bytes.extend_from_slice(&bytes);
                    slot.finished |= fin;
                }
                Observed::Opened(stream) => self.pending_streams.push(stream),
                Observed::Closed(stream, reason) => {
                    let slot = self.incoming.entry(stream.get()).or_default();
                    slot.finished = true;
                    // The receiving direction's code is the one that concerns a reader: it
                    // says why the bytes stopped. The sending direction's belongs to writes
                    // and is reported through the stop-sending path instead.
                    if let Some(code) = reason.receiving() {
                        slot.reset.get_or_insert(code);
                    }
                }
                Observed::Reset(stream, code) => {
                    let slot = self.incoming.entry(stream.get()).or_default();
                    slot.finished = true;
                    slot.reset.get_or_insert(code);
                }
                Observed::LocallyOpened(_) => {}
                Observed::StopSending(stream, code) => {
                    self.stopped.insert(stream.get(), code);
                }
                Observed::Acked(stream, len) => {
                    *self.acked.entry(stream.get()).or_default() += len;
                }
                _ => {}
            }
        }
    }

    /// Opens a bidirectional stream.
    ///
    /// # Errors
    ///
    /// Fails if the connection has ended.
    pub fn open_bidi(&self) -> OpenStream<'_> {
        self.open(true)
    }

    /// Opens a unidirectional stream.
    ///
    /// # Errors
    ///
    /// Fails if the connection has ended.
    pub fn open_uni(&self) -> OpenStream<'_> {
        self.open(false)
    }

    fn open(&self, bidi: bool) -> OpenStream<'_> {
        self.shared.push(Command::OpenStream { bidi });
        OpenStream {
            shared: Arc::clone(&self.shared),
            bidi,
            _borrow: core::marker::PhantomData,
        }
    }

    /// Writes bytes to a stream, optionally ending it.
    ///
    /// Resolves once the transport has accepted the bytes, which is before the peer has
    /// acknowledged them — the caller's buffer may be reused as soon as this returns, and
    /// the copy the transport needs is made internally.
    ///
    /// # Errors
    ///
    /// Fails if the connection has ended.
    pub fn write(&mut self, stream: StreamId, data: &[u8], fin: bool) -> Result<()> {
        if self.shared.is_closed() {
            return Err(self.shared.failure());
        }
        self.absorb();
        // Writing to a stream the peer has asked us to stop sending on wastes the
        // connection's flow-control window on bytes nothing will read, so it is refused
        // with the code the peer sent rather than silently accepted.
        if let Some(code) = self.stopped.get(&stream.get()) {
            return Err(Error::new(
                ErrorKind::StreamStopped,
                "the peer asked this endpoint to stop sending on this stream",
            )
            .with_stream_code(*code));
        }
        self.shared.push(Command::Write {
            stream,
            data: data.to_vec(),
            fin,
        });
        Ok(())
    }

    /// Reads whatever has arrived on a stream, waiting until something has.
    pub fn read(&mut self, stream: StreamId) -> ReadStream<'_> {
        ReadStream {
            connection: self,
            stream,
        }
    }

    /// Resets a stream, discarding anything not yet delivered.
    ///
    /// # Errors
    ///
    /// Fails if the connection has ended.
    pub fn reset(&self, stream: StreamId, code: ApplicationErrorCode) -> Result<()> {
        if self.shared.is_closed() {
            return Err(self.shared.failure());
        }
        self.shared.push(Command::Reset(stream, code));
        Ok(())
    }

    /// Asks the peer to stop sending on a stream.
    ///
    /// # Errors
    ///
    /// Fails if the connection has ended.
    pub fn stop_sending(&self, stream: StreamId, code: ApplicationErrorCode) -> Result<()> {
        if self.shared.is_closed() {
            return Err(self.shared.failure());
        }
        self.shared.push(Command::StopSending(stream, code));
        Ok(())
    }

    /// Closes the connection, telling the peer why.
    pub fn close(&self, code: ApplicationErrorCode, reason: &[u8]) {
        self.shared
            .push(Command::Close(code, reason.to_vec()));
    }

    /// Waits for the next stream the peer opens.
    pub fn accept_stream(&mut self) -> AcceptStream<'_> {
        AcceptStream { connection: self }
    }

    /// How many bytes the peer has acknowledged on a stream.
    ///
    /// Acknowledgement is what releases the copy the transport holds for retransmission, so
    /// this is the number that explains [`Connection::retained_bytes`] going down.
    pub fn acked_bytes(&mut self, stream: StreamId) -> u64 {
        self.absorb();
        self.acked.get(&stream.get()).copied().unwrap_or(0)
    }

    /// The code the peer sent if it asked this endpoint to stop sending on a stream.
    pub fn stop_sending_code(&mut self, stream: StreamId) -> Option<ApplicationErrorCode> {
        self.absorb();
        self.stopped.get(&stream.get()).copied()
    }

    /// Why the connection ended, if it has.
    pub fn failure(&self) -> Option<Error> {
        self.shared.is_closed().then(|| self.shared.failure())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if !self.shared.is_closed() {
            self.shared
                .push(Command::Close(ApplicationErrorCode::new(0), Vec::new()));
        }
    }
}

impl core::fmt::Debug for Connection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Connection")
            .field("established", &self.is_established())
            .field("closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

/// A stream being opened.
#[must_use = "no stream is opened until this is awaited"]
pub struct OpenStream<'a> {
    shared: Arc<ConnectionShared>,
    bidi: bool,
    _borrow: core::marker::PhantomData<&'a Connection>,
}

impl OpenStream<'_> {
    /// Takes the first stream this endpoint opened with the directionality asked for.
    ///
    /// Matching on directionality matters when a bidirectional and a unidirectional open
    /// are outstanding at once: without it the two futures could resolve with each other's
    /// stream, and a caller would write to a stream the peer cannot read.
    fn take(&self) -> Option<StreamId> {
        let mut inner = self.shared.lock();
        let wanted = if self.bidi {
            Directionality::Bidirectional
        } else {
            Directionality::Unidirectional
        };
        let position = inner.observed.iter().position(|event| {
            matches!(event, Observed::LocallyOpened(id) if id.directionality() == wanted)
        });
        position.and_then(|at| match inner.observed.remove(at) {
            Observed::LocallyOpened(id) => Some(id),
            _ => None,
        })
    }
}

impl Future for OpenStream<'_> {
    type Output = Result<StreamId>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(stream) = self.take() {
            return Poll::Ready(Ok(stream));
        }
        if self.shared.is_closed() {
            return Poll::Ready(Err(self.shared.failure()));
        }

        self.shared.register(cx.waker());
        // Re-check after registering. The driver may have opened the stream between the
        // scan above and the registration, in which case the wake has already happened and
        // waiting for another would wait forever.
        if let Some(stream) = self.take() {
            return Poll::Ready(Ok(stream));
        }
        if self.shared.is_closed() {
            return Poll::Ready(Err(self.shared.failure()));
        }
        Poll::Pending
    }
}

/// Bytes read from a stream.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Chunk {
    /// The bytes, which may be empty when only end-of-stream arrived.
    pub bytes: Vec<u8>,
    /// Whether the peer will send nothing more on this stream.
    pub fin: bool,
}

/// A pending read.
#[must_use = "nothing is read until this is awaited"]
pub struct ReadStream<'a> {
    connection: &'a mut Connection,
    stream: StreamId,
}

impl Future for ReadStream<'_> {
    type Output = Result<Chunk>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.connection.absorb();

        if let Some(slot) = this.connection.incoming.get_mut(&this.stream.get())
            && (!slot.bytes.is_empty() || slot.finished)
        {
            // Bytes delivered before a reset are still valid and are handed over first; the
            // reset is reported once they have been drained, so a reader never loses data
            // it had already been told about.
            if slot.bytes.is_empty()
                && let Some(code) = slot.reset
            {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::StreamReset,
                    "the peer reset this stream",
                )
                .with_stream_code(code)));
            }
            // A zero-length chunk with `fin` set is legal and must be delivered: it is how
            // a stream ends without a final byte, and a reader that treated it as "nothing
            // yet" would wait forever for bytes that are not coming.
            let chunk = Chunk {
                bytes: core::mem::take(&mut slot.bytes),
                fin: slot.finished,
            };
            this.connection.consumed(this.stream, chunk.bytes.len());
            return Poll::Ready(Ok(chunk));
        }

        if this.connection.shared.is_closed() {
            return Poll::Ready(Err(this.connection.shared.failure()));
        }

        this.connection.shared.register(cx.waker());
        this.connection.absorb();
        if let Some(slot) = this.connection.incoming.get_mut(&this.stream.get())
            && (!slot.bytes.is_empty() || slot.finished)
        {
            let chunk = Chunk {
                bytes: core::mem::take(&mut slot.bytes),
                fin: slot.finished,
            };
            this.connection.consumed(this.stream, chunk.bytes.len());
            return Poll::Ready(Ok(chunk));
        }
        Poll::Pending
    }
}

/// A pending accept of a peer-opened stream.
#[must_use = "nothing is accepted until this is awaited"]
pub struct AcceptStream<'a> {
    connection: &'a mut Connection,
}

impl Future for AcceptStream<'_> {
    type Output = Result<StreamId>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.connection.absorb();
        if !this.connection.pending_streams.is_empty() {
            return Poll::Ready(Ok(this.connection.pending_streams.remove(0)));
        }
        if this.connection.shared.is_closed() {
            return Poll::Ready(Err(Error::new(
                ErrorKind::LocallyClosed,
                "the connection ended before a stream was accepted",
            )));
        }
        this.connection.shared.register(cx.waker());
        this.connection.absorb();
        if !this.connection.pending_streams.is_empty() {
            return Poll::Ready(Ok(this.connection.pending_streams.remove(0)));
        }
        Poll::Pending
    }
}
