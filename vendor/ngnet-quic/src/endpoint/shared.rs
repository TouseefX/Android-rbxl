//! What a handle and the driver say to each other.
//!
//! # Why the connections live in the driver and nothing else touches them
//!
//! A [`Conn`](crate::Conn) is `Send` but deliberately not `Sync`, and every method that
//! drives it takes `&mut self`. Exactly one thing may hold it, and that thing must be
//! whatever is polling the socket — so the driver owns every connection outright and the
//! handles a caller holds never see one.
//!
//! Sharing them behind a lock would buy nothing. The driver would still have to take that
//! lock for every datagram it processed, and no other task could usefully hold it, because
//! anything a caller wants to do to a connection has to be sequenced against the packets
//! arriving for it anyway.
//!
//! So this module is a mailbox rather than shared ownership. Handles push commands and read
//! results; the driver drains commands, does the work, and writes results back.
//!
//! # Why the handlers can be `'static`
//!
//! `Conn<'h, S>` borrows its handlers for `'h`, so a driver holding a collection of
//! connections needs `Conn<'static, S>` — a handler that borrowed a driver field would make
//! the driver self-referential. Every handler installed here therefore captures an
//! [`Arc<ConnectionShared>`] and nothing else, which is owned rather than borrowed and
//! satisfies the `Send` bound the core requires for a sound `Conn`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;

use crate::cid::ConnectionId;
use crate::error::{ApplicationErrorCode, CloseError};
use crate::handlers::StreamCloseReason;
use crate::stream::StreamId;

use super::error::{Error, ErrorKind};

/// Something a connection's handlers observed.
///
/// Handlers fire inside a call into ngtcp2 with the connection mutably borrowed, so they
/// cannot act — they can only record. The driver drains these after the call returns, which
/// is the same shape `ngnet-h3` uses and for the same reason.
#[derive(Debug)]
#[non_exhaustive]
pub enum Observed {
    /// Bytes arrived on a stream, and whether they end it.
    Data(StreamId, Vec<u8>, bool),
    /// The peer opened a stream.
    Opened(StreamId),
    /// This endpoint opened a stream, in response to a request from a handle.
    ///
    /// Distinct from [`Observed::Opened`] because the two go to different places: a peer's
    /// stream belongs to whoever is awaiting `accept_stream`, and this one belongs to the
    /// `open_*` future that asked for it. Sharing a variant let an `open_bidi()` resolve
    /// with a stream the peer had opened, and sent the caller's next write to the wrong
    /// stream.
    LocallyOpened(StreamId),
    /// A stream finished.
    Closed(StreamId, StreamCloseReason),
    /// The peer reset a stream it was sending on.
    Reset(StreamId, ApplicationErrorCode),
    /// The peer asked this endpoint to stop sending on a stream.
    StopSending(StreamId, ApplicationErrorCode),
    /// The peer acknowledged stream data.
    Acked(StreamId, u64),
    /// The handshake completed.
    HandshakeCompleted,
    /// The peer raised how many streams this endpoint may open, to the given total.
    ///
    /// The only signal that a refused open may now succeed. Without it a caller that waits
    /// for room waits forever, because opening past the limit is reported as a temporary
    /// block and nothing else announces that the block has lifted.
    StreamsExtended(u64),
}

/// State a connection's handle and the driver share.
///
/// One mutex rather than several. The critical sections are short — pushing an observation,
/// taking a queue — and one lock held briefly is both faster and easier to reason about
/// than four that must be taken in a fixed order.
pub(crate) struct ConnectionShared {
    /// The endpoint this connection belongs to, so queued work can wake its driver.
    ///
    /// Without this a handle can queue a write and then wait for a driver that is asleep on
    /// the socket and the timer -- neither of which a command touches. The connection would
    /// make no progress until something unrelated woke the driver, and on a quiescent
    /// connection the next such thing is the idle timeout, which closes it.
    endpoint: Arc<EndpointShared>,
    inner: Mutex<ConnectionInner>,
    /// Whether the connection is finished, readable without taking the lock.
    ///
    /// Hot: every operation on a handle checks it, and most find it false.
    closed: AtomicBool,
    /// Set once the handshake completes.
    established: AtomicBool,
    /// Bytes of stream data the peer has yet to acknowledge, for tests and diagnostics.
    retained: AtomicU64,
    /// Whether this connection is to be handed to its caller once established.
    detach: AtomicBool,
    /// Whether it is to be handed to whoever is accepting, rather than to one named waiter.
    detach_to_acceptor: AtomicBool,
}

/// The part of a connection's shared state that needs the lock.
#[derive(Default)]
pub(crate) struct ConnectionInner {
    /// What handlers observed, drained by whoever drives the connection.
    pub(crate) observed: Vec<Observed>,
    /// Why the connection ended, once it has.
    pub(crate) failure: Option<Error>,
    /// Woken when the connection becomes established, closes, or has work.
    pub(crate) wakers: Vec<Waker>,
    /// Work the driver should do on this connection.
    pub(crate) commands: VecDeque<Command>,
    /// Datagrams the endpoint routed here, waiting for whoever drives the connection.
    ///
    /// Only used when the connection is detached; a managed connection is fed directly,
    /// because the driver holding it has no reason to queue.
    pub(crate) inbound: VecDeque<Vec<u8>>,
    /// Datagrams the connection produced, waiting for the endpoint to send them.
    pub(crate) outbound: VecDeque<Vec<u8>>,
    /// Identifiers minted and retired, for the endpoint's routing table.
    ///
    /// Separate from `observed` because the two have different readers: observations belong
    /// to whoever drives the connection, and these belong to the endpoint whichever that
    /// is. A detached connection that kept them to itself would still be routable under the
    /// identifier it started with and unreachable under every later one -- a connection that
    /// works and then stops, with nothing logged.
    pub(crate) routes: VecDeque<RouteUpdate>,
    /// How many inbound datagrams were dropped because the queue was full.
    pub(crate) dropped: u64,
    /// Set by whoever drives the connection when it is finished with it.
    pub(crate) terminal: bool,
}

/// A change to where a connection can be reached.
#[derive(Clone, Debug)]
pub(crate) enum RouteUpdate {
    /// The connection now answers to this identifier as well.
    Minted(ConnectionId),
    /// The connection no longer answers to this identifier.
    Retired(ConnectionId),
}

/// How many datagrams may wait in either direction for one connection.
///
/// Bounded in both directions, for different reasons.
///
/// Inbound, the endpoint reads from a single socket shared by every connection, so it
/// cannot wait for one slow consumer without starving the rest. Past the bound it drops,
/// which is what QUIC's loss recovery is for and is the same thing a full socket buffer
/// would have done a layer lower.
///
/// Outbound, the bound is a signal rather than a place to discard: a datagram already
/// produced cannot be un-produced, because the connection has already advanced its state
/// to account for it. So the producer checks for room *before* producing, and this bound is
/// what it checks against.
pub(crate) const DATAGRAM_QUEUE: usize = 64;

/// Something a handle asked the driver to do to a connection.
#[derive(Debug)]
pub(crate) enum Command {
    /// Open a stream, resolving into the slot.
    OpenStream {
        /// Whether the stream is bidirectional.
        bidi: bool,
    },
    /// Write bytes to a stream, with an end-of-stream flag.
    Write {
        /// The stream to write to.
        stream: StreamId,
        /// The bytes.
        data: Vec<u8>,
        /// Whether these are the last bytes.
        fin: bool,
    },
    /// Reset a stream.
    Reset(StreamId, ApplicationErrorCode),
    /// Ask the peer to stop sending on a stream.
    StopSending(StreamId, ApplicationErrorCode),
    /// Close the connection.
    Close(ApplicationErrorCode, Vec<u8>),
    /// Give the peer back credit for bytes the application has now consumed.
    ExtendCredit {
        /// The stream the bytes came from.
        stream: StreamId,
        /// How many bytes were consumed.
        bytes: u64,
    },
}

impl ConnectionShared {
    /// A fresh shared state belonging to `endpoint`.
    pub(crate) fn new(endpoint: Arc<EndpointShared>) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
            inner: Mutex::default(),
            closed: AtomicBool::new(false),
            established: AtomicBool::new(false),
            retained: AtomicU64::new(0),
            detach: AtomicBool::new(false),
            detach_to_acceptor: AtomicBool::new(false),
        })
    }

    /// Takes the lock, recovering from a poisoned one.
    ///
    /// A panic in a handler aborts the process rather than unwinding, so a poisoned lock
    /// here means a panic somewhere in the driver — at which point the connection is
    /// already failing and refusing to look at its state helps nobody.
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, ConnectionInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Records something a handler observed.
    pub(crate) fn observe(&self, event: Observed) {
        self.lock().observed.push(event);
    }

    /// Asks that this connection be handed to its caller once the handshake completes.
    pub(crate) fn request_detach(&self) {
        self.detach.store(true, Ordering::Release);
    }

    /// Asks that this connection go to whoever is accepting, rather than to one waiter.
    pub(crate) fn request_detach_to_acceptor(&self) {
        self.detach.store(true, Ordering::Release);
        self.detach_to_acceptor.store(true, Ordering::Release);
    }

    /// Whether this connection is to be handed over rather than driven by the endpoint.
    pub(crate) fn wants_detach(&self) -> bool {
        self.detach.load(Ordering::Acquire)
    }

    /// Whether it goes to an acceptor rather than to the caller who dialled it.
    pub(crate) fn detaches_to_acceptor(&self) -> bool {
        self.detach_to_acceptor.load(Ordering::Acquire)
    }

    /// Records an identifier change for the endpoint's routing table.
    ///
    /// Recorded rather than applied, because handlers run while ngtcp2 holds the connection
    /// and may only take notes. The endpoint applies these before it sends anything the
    /// connection produced in the same pass, so a new identifier is routable before the
    /// packet announcing it goes out.
    pub(crate) fn observe_route(&self, update: RouteUpdate) {
        self.lock().routes.push_back(update);
        self.endpoint.wake_driver();
    }

    /// Takes every pending routing change.
    pub(crate) fn take_routes(&self) -> VecDeque<RouteUpdate> {
        core::mem::take(&mut self.lock().routes)
    }

    /// Hands a datagram to whoever drives this connection.
    ///
    /// Returns `false` if the queue was full and the datagram was dropped. Dropping is
    /// deliberate: the endpoint reads from one socket on behalf of every connection, and
    /// blocking on a consumer that is not keeping up would stall all of them. A dropped
    /// datagram is a lost packet, which QUIC already knows how to recover from; a stalled
    /// endpoint is not something it can recover from.
    pub(crate) fn deliver_inbound(&self, datagram: Vec<u8>) -> bool {
        let mut inner = self.lock();
        if inner.inbound.len() >= DATAGRAM_QUEUE {
            inner.dropped += 1;
            return false;
        }
        inner.inbound.push_back(datagram);
        let wakers = core::mem::take(&mut inner.wakers);
        drop(inner);
        for waker in wakers {
            waker.wake();
        }
        true
    }

    /// Takes the next datagram the endpoint routed here.
    pub(crate) fn take_inbound(&self) -> Option<Vec<u8>> {
        self.lock().inbound.pop_front()
    }

    /// How many inbound datagrams have been dropped for want of room.
    pub(crate) fn dropped_inbound(&self) -> u64 {
        self.lock().dropped
    }

    /// Whether there is room to produce another outgoing datagram.
    ///
    /// Asked *before* producing one. A datagram that has been produced cannot be withdrawn:
    /// the connection has already accounted for the stream bytes it carries, so offering
    /// them again would send them twice, and discarding it loses them until a retransmission
    /// timer notices. Neither is acceptable, so the only safe place to apply back pressure
    /// is before the connection is asked to write.
    pub(crate) fn outbound_has_room(&self) -> bool {
        self.lock().outbound.len() < DATAGRAM_QUEUE
    }

    /// Queues a datagram for the endpoint to send.
    pub(crate) fn queue_outbound(&self, datagram: Vec<u8>) {
        self.lock().outbound.push_back(datagram);
        self.endpoint.wake_driver();
    }

    /// Whether anything is still waiting to be sent for this connection.
    pub(crate) fn has_outbound(&self) -> bool {
        !self.lock().outbound.is_empty()
    }

    /// How many datagrams are waiting to be sent for this connection.
    pub(crate) fn outbound_len(&self) -> usize {
        self.lock().outbound.len()
    }

    /// Takes the next datagram this connection wants sent.
    pub(crate) fn take_outbound(&self) -> Option<Vec<u8>> {
        self.lock().outbound.pop_front()
    }

    /// Whether this connection's owner has finished with it.
    pub(crate) fn is_terminal(&self) -> bool {
        self.lock().terminal
    }

    /// Says this connection's owner has finished with it, so the endpoint may release it.
    ///
    /// A detached connection's endpoint cannot ask it whether it is done -- it does not hold
    /// the connection -- so without this its routing entry would live as long as the
    /// endpoint does.
    pub(crate) fn mark_terminal(&self) {
        self.lock().terminal = true;
        self.endpoint.wake_driver();
    }

    /// Whether the connection has finished.
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Whether the handshake has completed.
    pub(crate) fn is_established(&self) -> bool {
        self.established.load(Ordering::Acquire)
    }

    /// Marks the handshake complete and wakes anything waiting for it.
    pub(crate) fn mark_established(&self) {
        self.established.store(true, Ordering::Release);
        self.wake_all();
    }

    /// Bytes still held awaiting acknowledgement.
    pub(crate) fn retained_bytes(&self) -> u64 {
        self.retained.load(Ordering::Relaxed)
    }

    /// Records how much is currently retained.
    pub(crate) fn set_retained(&self, bytes: u64) {
        self.retained.store(bytes, Ordering::Relaxed);
    }

    /// Ends the connection, recording why, and wakes everything waiting on it.
    ///
    /// The first reason wins. A connection that fails for one reason and is then torn down
    /// for another should report the first, which is the one that explains the others.
    pub(crate) fn fail(&self, error: Error) {
        {
            let mut inner = self.lock();
            if inner.failure.is_none() {
                inner.failure = Some(error);
            }
        }
        self.closed.store(true, Ordering::Release);
        self.wake_all();
    }

    /// Ends the connection because the peer closed it, carrying what the peer said.
    pub(crate) fn fail_with_close(&self, close: CloseError) {
        let kind = match close.reason() {
            crate::error::CloseReason::IdleTimeout => ErrorKind::IdleTimeout,
            _ => ErrorKind::PeerClosed,
        };
        self.fail(Error::new(kind, "the connection was closed").with_close(close));
    }

    /// Builds an error describing why the connection ended.
    pub(crate) fn failure(&self) -> Error {
        let inner = self.lock();
        match inner.failure.as_ref() {
            Some(err) => {
                let mut rebuilt = Error::new(err.kind(), "the connection ended");
                if let Some(close) = err.close_error() {
                    rebuilt = rebuilt.with_close(close.clone());
                }
                rebuilt
            }
            None => Error::new(ErrorKind::LocallyClosed, "the connection ended"),
        }
    }

    /// Registers a waker to be woken when something changes.
    pub(crate) fn register(&self, waker: &Waker) {
        let mut inner = self.lock();
        if !inner.wakers.iter().any(|w| w.will_wake(waker)) {
            inner.wakers.push(waker.clone());
        }
    }

    /// Wakes everything waiting on this connection.
    pub(crate) fn wake_all(&self) {
        let wakers = core::mem::take(&mut self.lock().wakers);
        for waker in wakers {
            waker.wake();
        }
    }

    /// Queues work for the driver, and wakes it.
    ///
    /// The wake is not optional. A driver with nothing to read and no timer due is parked,
    /// and a command reaches neither of those -- so queuing without waking means the work
    /// waits for an unrelated event, which on a quiescent connection is the idle timeout.
    pub(crate) fn push(&self, command: Command) {
        self.lock().commands.push_back(command);
        self.endpoint.wake_driver();
    }

    /// Takes everything queued.
    pub(crate) fn take_commands(&self) -> VecDeque<Command> {
        core::mem::take(&mut self.lock().commands)
    }

    /// Takes everything the handlers observed.
    pub(crate) fn take_observed(&self) -> Vec<Observed> {
        core::mem::take(&mut self.lock().observed)
    }
}

/// State the endpoint handle and the driver share.
#[derive(Default)]
pub(crate) struct EndpointShared {
    inner: Mutex<EndpointInner>,
    /// Whether the driver has gone, readable without the lock.
    gone: AtomicBool,
    /// Whether connections this endpoint accepts are to be handed to their caller.
    detached_accepts: AtomicBool,
}

/// The part of the endpoint's shared state that needs the lock.
#[derive(Default)]
pub(crate) struct EndpointInner {
    /// Requests to open a connection.
    pub(crate) dials: VecDeque<Dial>,
    /// Connections the peer opened, waiting to be accepted.
    pub(crate) accepted: VecDeque<Arc<ConnectionShared>>,
    /// Woken when a connection is accepted or the driver goes away.
    pub(crate) wakers: Vec<Waker>,
    /// Woken when there is work for the driver.
    pub(crate) driver_waker: Option<Waker>,
}

/// A request to open a connection.
pub(crate) struct Dial {
    /// Where to connect.
    pub(crate) remote: core::net::SocketAddr,
    /// The name to present and verify.
    pub(crate) server_name: Option<String>,
    /// The state the resulting connection will share with its handle.
    pub(crate) shared: Arc<ConnectionShared>,
}

impl core::fmt::Debug for Dial {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dial")
            .field("remote", &self.remote)
            .field("server_name", &self.server_name)
            .finish_non_exhaustive()
    }
}

impl EndpointShared {
    /// A fresh shared state.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Takes the lock, recovering from a poisoned one.
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, EndpointInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Asks that accepted connections be handed to their caller rather than driven here.
    pub(crate) fn request_detached_accepts(&self) {
        self.detached_accepts.store(true, Ordering::Release);
    }

    /// Whether accepted connections are handed over.
    pub(crate) fn detached_accepts(&self) -> bool {
        self.detached_accepts.load(Ordering::Acquire)
    }

    /// Whether the driver has stopped.
    pub(crate) fn is_gone(&self) -> bool {
        self.gone.load(Ordering::Acquire)
    }

    /// Records that the driver has stopped and wakes everything waiting.
    pub(crate) fn mark_gone(&self) {
        self.gone.store(true, Ordering::Release);
        let wakers = core::mem::take(&mut self.lock().wakers);
        for waker in wakers {
            waker.wake();
        }
    }

    /// Wakes the driver, so a command queued from a handle is noticed.
    ///
    /// Without this a handle could queue work and then wait forever: the driver is asleep
    /// on the socket and the timer, neither of which a command touches.
    pub(crate) fn wake_driver(&self) {
        let waker = self.lock().driver_waker.take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Registers the driver's waker.
    pub(crate) fn register_driver(&self, waker: &Waker) {
        self.lock().driver_waker = Some(waker.clone());
    }

    /// Registers a waker to be woken when a connection is accepted.
    pub(crate) fn register(&self, waker: &Waker) {
        let mut inner = self.lock();
        if !inner.wakers.iter().any(|w| w.will_wake(waker)) {
            inner.wakers.push(waker.clone());
        }
    }

    /// Wakes everything waiting for an accept.
    pub(crate) fn wake_acceptors(&self) {
        let wakers = core::mem::take(&mut self.lock().wakers);
        for waker in wakers {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_failure_is_the_one_reported() {
        // A connection that fails and is then torn down should report what actually went
        // wrong, not the teardown that followed from it.
        let shared = ConnectionShared::new(EndpointShared::new());
        shared.fail(Error::new(ErrorKind::HandshakeRejected, "first"));
        shared.fail(Error::new(ErrorKind::DriverGone, "second"));
        assert_eq!(shared.failure().kind(), ErrorKind::HandshakeRejected);
    }

    #[test]
    fn an_idle_close_is_reported_as_a_timeout_rather_than_a_peer_close() {
        // The distinction a caller acts on: nothing refused anything, and the peer may not
        // know the connection is over.
        let shared = ConnectionShared::new(EndpointShared::new());
        let close = crate::error::CloseError::idle_for_test();
        shared.fail_with_close(close);
        assert_eq!(shared.failure().kind(), ErrorKind::IdleTimeout);
    }

    #[test]
    fn commands_are_taken_in_order() {
        let shared = ConnectionShared::new(EndpointShared::new());
        shared.push(Command::Reset(
            StreamId::new(0).expect("valid"),
            ApplicationErrorCode::new(1),
        ));
        shared.push(Command::Reset(
            StreamId::new(4).expect("valid"),
            ApplicationErrorCode::new(2),
        ));
        let taken = shared.take_commands();
        assert_eq!(taken.len(), 2);
        assert!(shared.take_commands().is_empty(), "commands were taken twice");
    }

    #[test]
    fn a_waker_is_registered_once() {
        // A handle polled repeatedly must not grow the waker list without bound, which is
        // the classic way a busy connection turns into an out-of-memory failure.
        //
        // A real waker rather than `Waker::noop()`, because two noop wakers do not compare
        // equal under `will_wake` and the test would pass for the wrong reason.
        struct Counting(AtomicBool);
        impl std::task::Wake for Counting {
            fn wake(self: Arc<Self>) {
                self.0.store(true, Ordering::Release);
            }
        }

        let shared = ConnectionShared::new(EndpointShared::new());
        let first = Arc::new(Counting(AtomicBool::new(false)));
        let waker = Waker::from(Arc::clone(&first));
        shared.register(&waker);
        shared.register(&waker);
        assert_eq!(
            shared.lock().wakers.len(),
            1,
            "the same waker was stored twice, so a repeatedly polled handle grows without \
             bound"
        );

        // A genuinely different waker must still be kept, or a second task waiting on the
        // same connection would never be woken.
        let second = Arc::new(Counting(AtomicBool::new(false)));
        shared.register(&Waker::from(Arc::clone(&second)));
        assert_eq!(shared.lock().wakers.len(), 2);

        // And waking must reach both, which is what makes the dedup above safe rather than
        // merely tidy.
        shared.wake_all();
        assert!(first.0.load(Ordering::Acquire));
        assert!(second.0.load(Ordering::Acquire));
    }
}
