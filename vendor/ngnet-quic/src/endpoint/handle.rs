//! The endpoint: one socket, many connections, and the one future that drives them.
//!
//! # The shape, and why it is not one driver per connection
//!
//! `ngnet-h3`'s equivalent layer hands back a driver per connection, because a caller gives
//! it a connection that is already established. That cannot work here. Several drivers
//! cannot each own one UDP socket, and a driver produced by the first `connect` would have
//! no way to own connections created after it.
//!
//! So an endpoint is built once and yields two things: an [`Endpoint`] handle, which is
//! cheap to clone and does nothing on its own, and an [`EndpointDriver`], which owns the
//! socket and every connection on it. Connecting and accepting are requests the handle
//! makes of that one driver.

use core::future::Future;
use core::net::SocketAddr;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::collections::HashMap;
use std::sync::Arc;

use crate::rand::EntropySource;
use crate::tls::{Backend as TlsBackend, Session};

use super::clock::Clock;
use super::config::Config;
use super::connection::Connection;
use super::driver::{EntropyFactory, Inner, MAX_DATAGRAM, SocketError};
use super::error::{Error, ErrorKind, Result};
use super::shared::{ConnectionShared, Dial, EndpointShared};
use super::socket::AsyncUdpSocket;

/// Builds an endpoint.
///
/// Three things have no default and must be supplied: the socket, the clock, and a way to
/// make randomness. The first two are the runtime; the third is because this crate owns no
/// random number generator, and QUIC's connection identifiers and stateless reset tokens
/// must be unpredictable — choosing a generator here would be choosing it on the caller's
/// behalf, and choosing a weak one would be a security defect the API would not reveal.
pub struct EndpointBuilder<Sock, Clk, B> {
    socket: Sock,
    clock: Clk,
    backend: B,
    config: Config,
    entropy: Option<EntropyFactory>,
    accepts: bool,
    #[cfg(feature = "tls-ossl")]
    validation: Option<crate::token::TokenSecret>,
    #[cfg(feature = "tls-ossl")]
    token_lifetime: Option<crate::time::Duration>,
    #[cfg(feature = "tls-ossl")]
    reset_burst: Option<u32>,
}

impl<Sock, Clk, B> EndpointBuilder<Sock, Clk, B>
where
    Sock: AsyncUdpSocket,
    Clk: Clock,
    B: TlsBackend,
{
    /// Starts from a socket, a clock and a TLS backend.
    pub fn new(socket: Sock, clock: Clk, backend: B) -> Self {
        Self {
            socket,
            clock,
            backend,
            config: Config::new(),
            entropy: None,
            accepts: false,
            #[cfg(feature = "tls-ossl")]
            validation: None,
            #[cfg(feature = "tls-ossl")]
            token_lifetime: None,
            #[cfg(feature = "tls-ossl")]
            reset_burst: None,
        }
    }

    /// Applies a configuration.
    #[must_use]
    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    /// Supplies the source of randomness each connection will use.
    ///
    /// Called once per connection. It must be cryptographically secure: connection
    /// identifiers and stateless reset tokens are derived from it, and a predictable one
    /// lets an observer link or forge connections.
    #[must_use]
    pub fn entropy<F, E>(mut self, make: F) -> Self
    where
        F: Fn() -> E + Send + 'static,
        E: EntropySource + Send + 'static,
    {
        self.entropy = Some(Box::new(move || Box::new(make())));
        self
    }

    /// Accepts connections this endpoint did not initiate.
    ///
    /// Off by default, so a client endpoint does not answer strangers.
    #[must_use]
    pub fn accepts(mut self, accepts: bool) -> Self {
        self.accepts = accepts;
        self
    }

    /// Validates client addresses before committing any connection state.
    ///
    /// A server without this completes a handshake in response to a first packet, and the
    /// handshake is several times larger than that packet — so a spoofed source address
    /// turns the server into an amplifier aimed at whoever the attacker names. With it, an
    /// unvalidated first packet draws a small Retry carrying a token instead, and only a
    /// client that genuinely holds the address it claimed can come back with that token.
    ///
    /// The same secret also derives the stateless reset tokens this endpoint sends to peers
    /// whose connections it no longer has.
    ///
    /// **Strongly recommended for anything reachable from a network.** It is not the
    /// default only because it requires a secret this crate cannot invent for you.
    ///
    /// Available only with the bundled TLS backend: writing a Retry packet needs the packet
    /// protection that backend supplies, so without it there is nothing to turn on.
    #[cfg(feature = "tls-ossl")]
    #[must_use]
    pub fn validate_addresses(mut self, secret: crate::token::TokenSecret) -> Self {
        self.validation = Some(secret);
        self
    }

    /// How long a Retry token stays valid.
    ///
    /// Shorter is safer and costs a dawdling client one extra round trip; longer widens the
    /// window in which a captured token is useful. Defaults to
    /// [`DEFAULT_TOKEN_LIFETIME`](super::DEFAULT_TOKEN_LIFETIME). Ignored unless
    /// [`EndpointBuilder::validate_addresses`] was called.
    #[cfg(feature = "tls-ossl")]
    #[must_use]
    pub fn token_lifetime(mut self, lifetime: crate::time::Duration) -> Self {
        self.token_lifetime = Some(lifetime);
        self
    }

    /// How many stateless resets this endpoint may send in a burst.
    ///
    /// Answering unmatched datagrams tells a peer that has lost state to stop
    /// retransmitting, but doing it without limit turns a flood of spoofed datagrams into a
    /// flood aimed at whoever they name. The budget refills once a second. Defaults to
    /// [`DEFAULT_RESET_BURST`](super::DEFAULT_RESET_BURST).
    #[cfg(feature = "tls-ossl")]
    #[must_use]
    pub fn stateless_reset_burst(mut self, burst: u32) -> Self {
        self.reset_burst = Some(burst);
        self
    }

    /// Builds the handle and the driver.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] if no entropy source was supplied.
    /// Builds the endpoint and the one driver that serves it.
    ///
    /// Connections from this endpoint are driven by that driver. To take one over and drive
    /// it yourself — which the HTTP/3 layer requires — use
    /// [`EndpointBuilder::build_detachable`] instead.
    ///
    /// # Errors
    ///
    /// Fails if no source of randomness was supplied.
    pub fn build(self) -> Result<Built<Sock, Clk, B>> {
        self.assemble(None, None)
    }

    /// Builds an endpoint whose connections may be handed to callers who drive them.
    ///
    /// # Why this is separate from [`EndpointBuilder::build`]
    ///
    /// A connection handed over must read the *same* timescale the endpoint drove its
    /// handshake against; two clocks with different origins make every later timestamp
    /// incomparable with the ones already recorded, which ngtcp2 catches with an assertion
    /// in debug builds and mis-times silently in release ones. So the clock is captured here
    /// and travels with every detached connection, which means it must be cloneable and
    /// shareable.
    ///
    /// That bound is deliberately *not* on [`Clock`] itself, and deliberately not on
    /// `build`. The seams in this module impose no `Send` requirement precisely so a
    /// thread-per-core runtime can build them on non-shared types — a property the test
    /// clock exists to keep honest by being non-`Send` on purpose. Putting the bound here
    /// asks for it only from callers who need what it buys.
    ///
    /// # Errors
    ///
    /// Fails if no source of randomness was supplied.
    pub fn build_detachable(self) -> Result<Built<Sock, Clk, B>>
    where
        Clk: Clone + Send + Sync + 'static,
        Clk::Sleep: Send + 'static,
    {
        let timescale = {
            let clock = self.clock.clone();
            Arc::new(move || clock.now()) as Arc<dyn Fn() -> crate::Timestamp + Send + Sync>
        };
        let sleeper = {
            let clock = self.clock.clone();
            Arc::new(move |deadline| Box::pin(clock.sleep_until(deadline)) as Sleep)
                as Arc<dyn Fn(crate::Timestamp) -> Sleep + Send + Sync>
        };
        self.assemble(Some(timescale), Some(sleeper))
    }

    fn assemble(
        self,
        timescale: Option<Arc<dyn Fn() -> crate::Timestamp + Send + Sync>>,
        sleeper: Option<Arc<dyn Fn(crate::Timestamp) -> Sleep + Send + Sync>>,
    ) -> Result<Built<Sock, Clk, B>> {
        let entropy = self.entropy.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidInput,
                "an endpoint needs a source of randomness; see EndpointBuilder::entropy",
            )
        })?;

        let shared = EndpointShared::new();
        let detached: Arc<DetachQueue<B::Session>> = Arc::default();
        let inner = Inner {
            detached: Arc::clone(&detached),
            timescale,
            sleeper,
            socket: self.socket,
            clock: self.clock,
            backend: self.backend,
            config: self.config,
            entropy,
            connections: HashMap::new(),
            routes: HashMap::new(),
            next_index: 0,
            buffer: vec![0u8; MAX_DATAGRAM],
            send_buffer: vec![0u8; MAX_DATAGRAM],
            index_scratch: Vec::new(),
            accepts: self.accepts,
            outbox: std::collections::VecDeque::new(),
            #[cfg(feature = "tls-ossl")]
            validation: self.validation.map(|secret| {
                let mut policy = super::validate::Validation::new(secret);
                if let Some(lifetime) = self.token_lifetime {
                    policy.lifetime(lifetime);
                }
                if let Some(burst) = self.reset_burst {
                    policy.reset_burst(burst);
                }
                policy
            }),
            sleeping: None,
            sleeping_until: None,
        };

        Ok((
            Endpoint {
                shared: Arc::clone(&shared),
                detached,
            },
            EndpointDriver {
                inner,
                shared,
                stopped: false,
            },
        ))
    }
}

/// What [`EndpointBuilder::build`] produces: a handle and the one driver that serves it.
pub type Built<Sock, Clk, B> =
    (Endpoint<<B as TlsBackend>::Session>, EndpointDriver<Sock, Clk, B>);

/// Connections handed over to callers who drive them themselves.
///
/// Keyed by the address of the shared state each waiter holds, so a caller gets back the
/// connection it asked for rather than whichever finished first. Zero means "any", which is
/// what an acceptor wants.
pub(crate) struct DetachQueue<S: Session> {
    ready: std::sync::Mutex<Vec<(usize, DetachedConnection<S>)>>,
    wakers: std::sync::Mutex<Vec<core::task::Waker>>,
}

impl<S: Session> Default for DetachQueue<S> {
    fn default() -> Self {
        Self {
            ready: std::sync::Mutex::new(Vec::new()),
            wakers: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl<S: Session> DetachQueue<S> {
    pub(crate) fn deliver(&self, key: usize, connection: DetachedConnection<S>) {
        self.ready
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((key, connection));
        let wakers = core::mem::take(&mut *self.wakers.lock().unwrap_or_else(|e| e.into_inner()));
        for waker in wakers {
            waker.wake();
        }
    }

    /// Takes the connection a waiter asked for.
    ///
    /// Key zero means "one this endpoint accepted", which is what an acceptor wants; any
    /// other key names one particular outbound attempt. The two must not be confused: an
    /// acceptor that matched anything could take a connection this endpoint *dialled*, hand
    /// it to a server, and leave the caller who dialled it waiting forever. Which of the two
    /// completes first is not this endpoint's decision — the peer influences it.
    fn take(&self, key: usize) -> Option<DetachedConnection<S>> {
        let mut ready = self.ready.lock().unwrap_or_else(|e| e.into_inner());
        let at = ready.iter().position(|(k, _)| *k == key)?;
        Some(ready.remove(at).1)
    }

    /// Gives up a connection nobody collected, so the endpoint can reclaim it.
    fn abandon(&self, key: usize) {
        let mut ready = self.ready.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = ready.iter().position(|(k, _)| *k == key) {
            let (_, connection) = ready.remove(at);
            connection.release();
        }
    }

    fn register(&self, waker: &core::task::Waker) {
        let mut wakers = self.wakers.lock().unwrap_or_else(|e| e.into_inner());
        if !wakers.iter().any(|w| w.will_wake(waker)) {
            wakers.push(waker.clone());
        }
    }
}

/// An established connection handed over to its caller, with the endpoint still routing for
/// it.
///
/// The caller owns the protocol state and is responsible for reading the datagrams the
/// endpoint routes here, producing the ones it wants sent, and firing its own timer. The
/// endpoint keeps the socket, the routing table, address validation and stateless reset —
/// everything that is shared between connections rather than particular to one.
///
/// This exists because a consumer that has to reach the connection *synchronously* while it
/// composes a packet cannot be served across a queue. The HTTP/3 layer is such a consumer:
/// it fills a packet by asking its transport for bytes and expects an answer before the call
/// returns.
///
/// Dropping one without calling [`DetachedConnection::release`] leaves the endpoint routing
/// to a connection nobody is driving until its identifiers are cleaned up.
#[must_use = "a detached connection makes no progress unless something drives it"]
pub struct DetachedConnection<S: Session> {
    /// The protocol state, now this caller's to drive.
    pub conn: crate::Conn<'static, S>,
    /// The queues this connection exchanges datagrams and identifier changes over.
    shared: Arc<ConnectionShared>,
    /// Where this connection's peer is.
    pub remote: SocketAddr,
    /// The endpoint's clock, so both sides of the hand-over read the same timescale.
    ///
    /// Not a clock of the caller's own. The endpoint drove this connection's handshake
    /// against *its* clock, and a second clock with a different origin makes every
    /// timestamp afterwards incomparable with the ones already recorded — which ngtcp2
    /// catches with an assertion in debug builds and silently mis-times in release ones.
    /// Erased into a closure rather than carried as another type parameter, because a caller
    /// has no reason to name the endpoint's clock type in order to hold a connection.
    clock: Arc<dyn Fn() -> crate::Timestamp + Send + Sync>,
    /// Sleeps until a deadline, on that same clock.
    ///
    /// Carried for the same reason and with more force than reading the time: a deadline
    /// produced by this connection is expressed on the endpoint's timescale, so a sleep
    /// arranged on any other clock resolves at the wrong moment. Nothing reports that; the
    /// connection simply retransmits late, or wakes early and spins.
    sleeper: Arc<dyn Fn(crate::Timestamp) -> Sleep + Send + Sync>,
}

/// A sleep that finishes at a deadline.
pub type Sleep = Pin<Box<dyn Future<Output = ()> + Send>>;

impl<S: Session> DetachedConnection<S> {
    pub(crate) fn new(
        conn: crate::Conn<'static, S>,
        shared: Arc<ConnectionShared>,
        remote: SocketAddr,
        clock: Arc<dyn Fn() -> crate::Timestamp + Send + Sync>,
        sleeper: Arc<dyn Fn(crate::Timestamp) -> Sleep + Send + Sync>,
    ) -> Self {
        Self {
            conn,
            shared,
            remote,
            clock,
            sleeper,
        }
    }

    /// Sleeps until a deadline, on the endpoint's clock.
    ///
    /// Always this rather than a clock of the caller's own. A deadline this connection
    /// produced is on the endpoint's timescale, and another clock's origin differs, so the
    /// sleep would resolve at the wrong time — late, which delays loss recovery, or
    /// immediately, which spins.
    pub fn sleep_until(&self, deadline: crate::Timestamp) -> Sleep {
        (self.sleeper)(deadline)
    }

    /// The current time, on the endpoint's clock.
    ///
    /// Always this rather than a clock of the caller's own: see the field's documentation.
    pub fn now(&self) -> crate::Timestamp {
        (self.clock)()
    }

    /// Takes the next datagram the endpoint routed to this connection.
    pub fn next_inbound(&self) -> Option<Vec<u8>> {
        self.shared.take_inbound()
    }

    /// Takes everything this connection's handlers have recorded since the last call.
    ///
    /// Handlers may only take notes: ngtcp2 calls them while it holds the connection, so
    /// nothing they see can be acted on until the call that triggered them has returned.
    /// This is where those notes come out.
    pub fn take_observed(&self) -> Vec<super::shared::Observed> {
        self.shared.take_observed()
    }

    /// Whether there is room to produce another outgoing datagram.
    ///
    /// Checked *before* writing, never after. A datagram that has been produced cannot be
    /// withdrawn: the connection has already accounted for the stream bytes in it, so
    /// offering them again would send them twice and dropping it loses them until a
    /// retransmission timer notices.
    pub fn outbound_has_room(&self) -> bool {
        self.shared.outbound_has_room()
    }

    /// Queues a datagram for the endpoint to send, and wakes it.
    pub fn send(&self, datagram: Vec<u8>) {
        self.shared.queue_outbound(datagram);
    }

    /// How many datagrams this connection has queued for the endpoint to send.
    ///
    /// Not a supported API: an allocation-counting test uses the difference across a send
    /// pass to learn how many datagrams that pass produced, which is what lets it assert one
    /// allocation per datagram and nothing besides.
    #[doc(hidden)]
    pub fn outbound_len_for_test(&self) -> usize {
        self.shared.outbound_len()
    }

    /// Registers a waker to be woken when a datagram arrives for this connection.
    pub fn register(&self, waker: &core::task::Waker) {
        self.shared.register(waker);
    }

    /// How many inbound datagrams were dropped because this connection was not keeping up.
    ///
    /// Non-zero means the endpoint discarded packets rather than stalling every other
    /// connection on the socket behind this one. QUIC recovers from that, but it is worth
    /// knowing it happened.
    pub fn dropped_inbound(&self) -> u64 {
        self.shared.dropped_inbound()
    }

    /// Tells the endpoint this connection is finished, so it can release its routes.
    ///
    /// The endpoint cannot work this out for itself: it does not hold the connection and
    /// cannot ask whether it is draining. Without this the routing entries live as long as
    /// the endpoint does.
    pub fn release(&self) {
        self.shared.mark_terminal();
    }
}

impl<S: Session> core::fmt::Debug for DetachedConnection<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DetachedConnection")
            .field("remote", &self.remote)
            .finish_non_exhaustive()
    }
}

/// A connection being established and handed over.
#[must_use = "nothing is detached until this is awaited"]
pub struct Detaching<S: Session> {
    shared: Option<Arc<ConnectionShared>>,
    queue: Arc<DetachQueue<S>>,
    endpoint: Arc<EndpointShared>,
    /// Whether this wait handed its connection over, or ended without one.
    settled: bool,
}

impl<S: Session> Drop for Detaching<S> {
    fn drop(&mut self) {
        // A caller that gives up before collecting must not leave a connection stranded in
        // the queue: the endpoint would keep routing to it, and its entry and identifiers
        // would live as long as the endpoint does. Only a keyed wait can clean up after
        // itself; an abandoned accept is covered by the endpoint's own accept bookkeeping.
        if self.settled {
            return;
        }
        if let Some(shared) = self.shared.as_ref() {
            let key = Arc::as_ptr(shared) as *const u8 as usize;
            self.queue.abandon(key);
            shared.mark_terminal();
        }
    }
}

impl<S: Session> Future for Detaching<S> {
    type Output = Result<DetachedConnection<S>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let key = this
            .shared
            .as_ref()
            .map_or(0, |s| Arc::as_ptr(s) as *const u8 as usize);

        if let Some(connection) = this.queue.take(key) {
            this.settled = true;
            return Poll::Ready(Ok(connection));
        }
        if let Some(shared) = this.shared.as_ref()
            && shared.is_closed()
        {
            this.settled = true;
            return Poll::Ready(Err(shared.failure()));
        }
        if this.endpoint.is_gone() {
            this.settled = true;
            return Poll::Ready(Err(Error::new(
                ErrorKind::DriverGone,
                "the endpoint driver is not running",
            )));
        }
        this.queue.register(cx.waker());
        if let Some(shared) = this.shared.as_ref() {
            shared.register(cx.waker());
        }
        this.endpoint.register(cx.waker());
        Poll::Pending
    }
}

/// A handle to an endpoint.
///
/// Cheap to clone and inert on its own: everything it does is a request to the driver, and
/// nothing happens until that driver is polled.
///
/// Generic over the TLS session type but not over the socket or clock. Those two are only
/// ever reached through a mailbox, so naming them here would put them in every signature
/// that mentions an endpoint for no benefit. The session type earns its place because
/// [`Endpoint::connect_detached`] and [`Endpoint::accept_detached`] hand back a connection,
/// and a connection cannot be named without it.
pub struct Endpoint<S: Session> {
    shared: Arc<EndpointShared>,
    detached: Arc<DetachQueue<S>>,
}

impl<S: Session> Clone for Endpoint<S> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            detached: Arc::clone(&self.detached),
        }
    }
}

impl<S: Session> Endpoint<S> {
    /// Opens a connection to `remote`, resolving once its handshake completes.
    ///
    /// `server_name` is presented for SNI and checked against the peer's certificate.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::HandshakeRejected`] if the peer refused, [`ErrorKind::HandshakeTimeout`]
    /// if it never answered, [`ErrorKind::Socket`] if the socket failed, and
    /// [`ErrorKind::DriverGone`] if the driver is not running.
    pub fn connect(&self, remote: SocketAddr, server_name: Option<&str>) -> Connecting {
        let shared = ConnectionShared::new(Arc::clone(&self.shared));
        if self.shared.is_gone() {
            shared.fail(Error::new(
                ErrorKind::DriverGone,
                "the endpoint driver is not running",
            ));
            return Connecting { shared };
        }
        self.shared.lock().dials.push_back(Dial {
            remote,
            server_name: server_name.map(str::to_string),
            shared: Arc::clone(&shared),
        });
        self.shared.wake_driver();
        Connecting { shared }
    }

    /// Waits for the next connection a peer opened.
    ///
    /// Only ever resolves on an endpoint built with
    /// [`EndpointBuilder::accepts`](EndpointBuilder::accepts).
    pub fn accept(&self) -> Accepting {
        Accepting {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Opens a connection and hands it over once established, for the caller to drive.
    ///
    /// The endpoint completes the handshake as it would for [`Endpoint::connect`], then
    /// gives up the connection rather than the handle. Handing it over earlier would mean
    /// giving a caller a connection that cannot yet carry anything, and would put the
    /// handshake — the part most worth having written once — in every consumer.
    ///
    /// The endpoint goes on routing datagrams to it, validating addresses, and answering
    /// datagrams that match no connection. What it stops doing is reading and writing this
    /// connection's protocol state, because that admits exactly one owner.
    pub fn connect_detached(&self, remote: SocketAddr, server_name: Option<&str>) -> Detaching<S> {
        let shared = ConnectionShared::new(Arc::clone(&self.shared));
        shared.request_detach();
        if self.shared.is_gone() {
            shared.fail(Error::new(
                ErrorKind::DriverGone,
                "the endpoint driver is not running",
            ));
        } else {
            self.shared.lock().dials.push_back(Dial {
                remote,
                server_name: server_name.map(str::to_string),
                shared: Arc::clone(&shared),
            });
            self.shared.wake_driver();
        }
        Detaching {
            shared: Some(shared),
            queue: Arc::clone(&self.detached),
            endpoint: Arc::clone(&self.shared),
            settled: false,
        }
    }

    /// Waits for the next connection a peer opened and hands it over for the caller to
    /// drive. See [`Endpoint::connect_detached`].
    pub fn accept_detached(&self) -> Detaching<S> {
        self.shared.request_detached_accepts();
        self.shared.wake_driver();
        Detaching {
            shared: None,
            queue: Arc::clone(&self.detached),
            endpoint: Arc::clone(&self.shared),
            settled: false,
        }
    }
}

impl<S: Session> core::fmt::Debug for Endpoint<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Endpoint").finish_non_exhaustive()
    }
}

/// A connection being established.
#[must_use = "a connection is not opened until this is awaited"]
pub struct Connecting {
    shared: Arc<ConnectionShared>,
}

impl Future for Connecting {
    type Output = Result<Connection>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.shared.is_established() {
            return Poll::Ready(Ok(Connection::new(Arc::clone(&self.shared))));
        }
        if self.shared.is_closed() {
            return Poll::Ready(Err(self.shared.failure()));
        }
        self.shared.register(cx.waker());
        // Re-check, because the driver may have finished between the two checks above and
        // the registration -- in which case the wake has already happened and waiting for
        // another would wait forever.
        if self.shared.is_established() {
            return Poll::Ready(Ok(Connection::new(Arc::clone(&self.shared))));
        }
        if self.shared.is_closed() {
            return Poll::Ready(Err(self.shared.failure()));
        }
        Poll::Pending
    }
}

/// A connection being accepted.
#[must_use = "nothing is accepted until this is awaited"]
pub struct Accepting {
    shared: Arc<EndpointShared>,
}

impl Accepting {
    /// Takes the first accepted connection that has finished its handshake.
    ///
    /// A connection is only worth handing over once it is established: before that its
    /// streams cannot be used and it may still fail, so yielding it early would hand the
    /// caller something whose only useful operation is to wait. Connections still
    /// handshaking stay in the queue, in arrival order, and ones that failed are discarded
    /// -- a server does not want to be told about every client that could not complete a
    /// handshake.
    fn take_ready(&self) -> Option<Arc<ConnectionShared>> {
        let mut inner = self.shared.lock();
        let mut examined = 0;
        let total = inner.accepted.len();
        while examined < total {
            let Some(candidate) = inner.accepted.pop_front() else {
                break;
            };
            examined += 1;
            if candidate.is_established() {
                return Some(candidate);
            }
            if candidate.is_closed() {
                continue;
            }
            inner.accepted.push_back(candidate);
        }
        None
    }
}

impl Future for Accepting {
    type Output = Result<Connection>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(connection) = self.take_ready() {
            return Poll::Ready(Ok(Connection::new(connection)));
        }
        if self.shared.is_gone() {
            return Poll::Ready(Err(Error::new(
                ErrorKind::DriverGone,
                "the endpoint driver is not running",
            )));
        }
        self.shared.register(cx.waker());
        if let Some(connection) = self.take_ready() {
            return Poll::Ready(Ok(Connection::new(connection)));
        }
        Poll::Pending
    }
}

/// The future that moves bytes.
///
/// Nothing happens until this is polled. Where it is polled is entirely the caller's
/// business — spawn it, join it, or poll it alongside whatever else there is; this crate
/// takes no executor, spawner or timer.
///
/// Dropping it is defined rather than undefined: every connection on the endpoint fails
/// with [`ErrorKind::DriverGone`] immediately, rather than hanging.
#[must_use = "an endpoint does nothing until its driver is polled: connections opened on \
              its handle will wait forever"]
pub struct EndpointDriver<Sock, Clk, B>
where
    Sock: AsyncUdpSocket,
    Clk: Clock,
    B: TlsBackend,
{
    inner: Inner<Sock, Clk, B>,
    shared: Arc<EndpointShared>,
    stopped: bool,
}

impl<Sock, Clk, B> EndpointDriver<Sock, Clk, B>
where
    Sock: AsyncUdpSocket,
    Clk: Clock,
    B: TlsBackend,
{
    /// Takes everything the handles have asked for.
    fn drain_dials(&mut self) {
        let dials: Vec<Dial> = self.shared.lock().dials.drain(..).collect();
        for dial in dials {
            let shared = Arc::clone(&dial.shared);
            if let Err(err) = self
                .inner
                .dial(dial.remote, dial.server_name.as_deref(), shared)
            {
                dial.shared.fail(err);
            }
        }
    }

    /// Reads up to the configured bound from the socket.
    fn read_datagrams(&mut self, cx: &mut Context<'_>) -> core::result::Result<bool, Error> {
        let mut progressed = false;
        for _ in 0..self.inner.config.datagrams_per_pass {
            let mut buffer = core::mem::take(&mut self.inner.buffer);
            let outcome = self.inner.socket.poll_recv(cx, &mut buffer);
            match outcome {
                Poll::Ready(Ok(received)) => {
                    progressed = true;
                    // The buffer is a local, not a field, for the duration of the dispatch:
                    // that is what lets a borrow of it be handed to `&mut self` methods
                    // without copying first. No dispatch destination retains the borrow --
                    // `read_pkt` passes it to ngtcp2 only for the call, `accept` stores
                    // owned state, the detached path copies it into its own queue, and the
                    // Retry and stateless-reset paths read it only while composing separate
                    // outgoing buffers -- so the compiler is free to end the borrow here,
                    // before the buffer goes back to `self.inner`.
                    self.dispatch(&buffer[..received.len], received.source);
                    self.inner.buffer = buffer;
                }
                Poll::Ready(Err(err)) => {
                    self.inner.buffer = buffer;
                    return Err(Error::new(ErrorKind::Socket, "the socket failed")
                        .with_source(SocketError(err.to_string())));
                }
                Poll::Pending => {
                    self.inner.buffer = buffer;
                    break;
                }
            }
        }
        Ok(progressed)
    }

    /// Runs one receive pass in isolation, for the allocation-counting tests.
    ///
    /// A test that needs to count only the receive half cannot go through `poll`, which
    /// reads, services and sends in one call. This exposes that half and nothing else.
    /// Hidden and unsupported, like everything in `endpoint::testing`.
    #[doc(hidden)]
    pub fn read_datagrams_for_test(
        &mut self,
        cx: &mut Context<'_>,
    ) -> core::result::Result<bool, Error> {
        self.read_datagrams(cx)
    }

    /// Borrows the socket, for the allocation-counting tests that inject and re-inject
    /// datagrams. Hidden and unsupported, like everything in `endpoint::testing`.
    #[doc(hidden)]
    pub fn socket_for_test(&self) -> &Sock {
        &self.inner.socket
    }

    /// Whether any connection is holding a datagram the socket has not yet taken, for the
    /// allocation-counting tests. A stream datagram the socket accepts is sent straight from
    /// the reusable buffer and leaves nothing here; only one the socket refuses is copied
    /// into `pending` and shows up. Hidden and unsupported.
    #[doc(hidden)]
    pub fn has_pending_for_test(&self) -> bool {
        self.inner.has_pending()
    }

    /// Clones every datagram a connection is currently holding in its `pending` slot, for
    /// the SC-012 tests. A close datagram waits here as an owned buffer of its own while the
    /// driver reuses its send buffer for other connections' work, so capturing these bytes
    /// before and after that work is how a test observes the held datagram keeps its own
    /// bytes without decrypting it. Hidden and unsupported.
    #[doc(hidden)]
    pub fn held_datagrams_for_test(&self) -> Vec<Vec<u8>> {
        self.inner
            .connections
            .values()
            .filter_map(|t| t.pending.clone())
            .collect()
    }

    /// Runs the command and timer half of a pass, without the send half, for the
    /// allocation-counting tests. Command production offers each stream datagram it produces
    /// to the socket, so this half completes or refuses a stream send; it does not flush, so a
    /// datagram a test wants held as `pending` for a later counted flush stays held. It takes
    /// the task context the send needs. Hidden and unsupported.
    #[doc(hidden)]
    pub fn service_commands_for_test(&mut self, cx: &mut Context<'_>) {
        let indices: Vec<u64> = self.inner.connections.keys().copied().collect();
        for index in &indices {
            self.inner.apply_routes(*index);
            self.inner.apply_commands(*index, cx);
            self.inner.handle_expiry(*index);
        }
    }

    /// Runs the whole command-and-send pass -- exactly what a `poll` does apart from reading
    /// the socket and re-arming the timer -- for the allocation-counting tests. This is the
    /// pass that walks the connection list twice, once to service commands and once to flush,
    /// using the driver's reusable index scratch rather than the two `Vec<u64>`s Phase 2
    /// removed. Excluding the re-arm keeps the boxed-timer allocation, which has nothing to do
    /// with walking the list, out of a test that counts the walk. Hidden and unsupported.
    #[doc(hidden)]
    pub fn service_for_test(&mut self, cx: &mut Context<'_>) -> core::result::Result<(), Error> {
        self.service(cx)
    }

    /// Runs one send pass in isolation, for the allocation-counting tests. This is the half
    /// Phase 5 made allocate nothing when its datagrams complete. Hidden and unsupported.
    #[doc(hidden)]
    pub fn flush_for_test(&mut self, cx: &mut Context<'_>) -> core::result::Result<(), Error> {
        self.inner.flush(cx)
    }

    /// Delivers one datagram to whatever should have it.
    fn dispatch(&mut self, datagram: &[u8], source: SocketAddr) {
        if let Some(index) = self.inner.route(datagram) {
            self.inner.deliver(index, datagram);
            return;
        }
        if !self.inner.accepts {
            // A client endpoint answers strangers with silence.
            return;
        }

        // Too short to be a QUIC packet at all. Answering would mean answering noise, and
        // any answer would be larger than what provoked it.
        if datagram.len() < 21 {
            return;
        }

        match crate::accept::inspect_initial(datagram) {
            Ok(Some(packet)) => self.begin(datagram, &packet, source),
            // Not an acceptable Initial. Either it names a version this build cannot speak,
            // or it belongs to a connection this endpoint no longer has.
            Ok(None) | Err(_) => self.answer_unmatched(datagram, source),
        }
    }

    /// Decides what a first packet has earned, and acts on it.
    fn begin(&mut self, datagram: &[u8], packet: &crate::accept::InitialPacket, source: SocketAddr) {
        #[cfg(feature = "tls-ossl")]
        let (original, retried) = {
            use super::validate::Decision;

            if let Some(policy) = self.inner.validation.take() {
                let now = self.inner.clock.now();
                let mut entropy = (self.inner.entropy)();
                let decision = policy.decide(packet, source, entropy.as_mut(), now);
                self.inner.validation = Some(policy);

                match decision {
                    Decision::Accept(original) => (original, true),
                    Decision::Retry { scid, token } => {
                        // No per-connection state is created here, which is the whole point:
                        // a Retry is computed from the packet and the secret and then
                        // forgotten, so answering a flood costs nothing to remember.
                        let mut buffer = vec![0u8; MAX_DATAGRAM];
                        if let Ok(len) = crate::token::write_retry(
                            &mut buffer,
                            packet.version,
                            &packet.scid,
                            &scid,
                            &packet.dcid,
                            &token,
                        ) {
                            buffer.truncate(len);
                            self.inner.outbox.push_back((source, buffer));
                        }
                        return;
                    }
                    Decision::Ignore => return,
                }
            } else {
                (packet.dcid, false)
            }
        };
        #[cfg(not(feature = "tls-ossl"))]
        let (original, retried) = (packet.dcid, false);

        let shared = ConnectionShared::new(Arc::clone(&self.shared));
        // An endpoint whose caller asked for detached accepts hands over every connection it
        // accepts, once each is established. The request is made on the endpoint rather than
        // per connection because a server does not know what is coming before it arrives.
        if self.shared.detached_accepts() {
            shared.request_detach_to_acceptor();
        }
        match self
            .inner
            .accept(source, packet, &original, retried, Arc::clone(&shared))
        {
            Ok(index) => {
                self.inner.deliver(index, datagram);
                // A connection destined for a detached acceptor does not also belong on the
                // managed accept queue: nothing would ever take it from there, and every
                // entry left behind keeps its shared state alive for the endpoint's life.
                if !shared.detaches_to_acceptor() {
                    self.shared.lock().accepted.push_back(shared);
                    self.shared.wake_acceptors();
                }
            }
            Err(_) => {
                // Refusing to build a connection for a first packet is not something the
                // peer needs to be told about; it will retransmit and give up.
            }
        }
    }

    /// Answers a datagram that belongs to no connection here.
    ///
    /// A peer that has lost this endpoint's state -- or whose connection this endpoint has
    /// evicted -- would otherwise retransmit until its idle timeout. A stateless reset tells
    /// it to stop.
    fn answer_unmatched(&mut self, datagram: &[u8], source: SocketAddr) {
        #[cfg(feature = "tls-ossl")]
        {
            let now = self.inner.clock.now();
            let Some(mut policy) = self.inner.validation.take() else {
                return;
            };
            let permitted = policy.take_reset_budget(now);
            let secret = policy.secret().clone();
            self.inner.validation = Some(policy);
            if !permitted {
                return;
            }

            // The identifier the datagram was addressed to is what the reset must be
            // derived from: the peer recognises the token only for the connection it thought
            // it was talking to.
            let Ok(inspection) = crate::accept::inspect(datagram, crate::cid::DEFAULT_LEN) else {
                return;
            };
            let dcid = match inspection {
                crate::accept::Inspection::Supported { dcid, .. }
                | crate::accept::Inspection::UnsupportedVersion { dcid, .. }
                | crate::accept::Inspection::ShortHeader { dcid } => dcid,
            };

            let Ok(token) = crate::token::reset_token(&secret, &dcid) else {
                return;
            };

            let mut random = vec![0u8; datagram.len()];
            let mut entropy = (self.inner.entropy)();
            if entropy.fill(&mut random).is_err() {
                return;
            }

            let mut buffer = vec![0u8; MAX_DATAGRAM];
            // Strictly smaller than what provoked it, or not sent at all -- otherwise the
            // way this endpoint says "I have lost your connection" becomes an amplifier.
            if let Ok(Some(len)) = crate::token::write_stateless_reset_smaller_than(
                &mut buffer,
                &token,
                &random,
                datagram.len(),
            ) {
                buffer.truncate(len);
                self.inner.outbox.push_back((source, buffer));
            }
        }
        #[cfg(not(feature = "tls-ossl"))]
        {
            // Deriving a reset token needs the crypto helpers, which are absent without a
            // TLS backend. Silence is the only honest answer.
            let _ = (datagram, source);
        }
    }

    /// Runs commands, services timers and writes.
    fn service(&mut self, cx: &mut Context<'_>) -> core::result::Result<(), Error> {
        let mut indices = core::mem::take(&mut self.inner.index_scratch);
        indices.clear();
        indices.extend(self.inner.connections.keys().copied());
        for index in &indices {
            // Routing first: an identifier a connection has just minted must be installed
            // before any datagram announcing it goes out, or the peer may use it before the
            // endpoint knows about it.
            self.inner.apply_routes(*index);
            self.inner.apply_commands(*index, cx);
            self.inner.handle_expiry(*index);
        }
        // Restored before `flush`, which takes the same scratch. This loop has no early
        // return, so returning it here is the only path out.
        indices.clear();
        self.inner.index_scratch = indices;
        self.inner.flush(cx)?;
        self.inner.evict();
        Ok(())
    }
}

impl<Sock, Clk, B> Future for EndpointDriver<Sock, Clk, B>
where
    Sock: AsyncUdpSocket + Unpin,
    Clk: Clock + Unpin,
    B: TlsBackend + Unpin,
    Inner<Sock, Clk, B>: Unpin,
{
    type Output = Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.stopped {
            return Poll::Ready(Ok(()));
        }
        this.shared.register_driver(cx.waker());

        // Bounded rather than unbounded, so a socket that is never empty cannot keep the
        // driver from ever reaching its timers.
        for _ in 0..8 {
            this.drain_dials();

            let read = match this.read_datagrams(cx) {
                Ok(read) => read,
                Err(err) => {
                    // A dead socket is fatal to every connection on it, because there is
                    // no longer a way to send or receive for any of them.
                    this.inner
                        .fail_all(|| Error::new(ErrorKind::Socket, "the socket failed"));
                    this.stopped = true;
                    this.shared.mark_gone();
                    return Poll::Ready(Err(err));
                }
            };

            // An accepted connection becomes interesting when it finishes its handshake,
            // which happens inside `service` rather than when it was queued -- so waking
            // acceptors only on arrival would leave one waiting for a connection that was
            // already ready. Waking only when something is actually takeable is what keeps
            // this from spinning.
            let ready = {
                let inner = this.shared.lock();
                inner
                    .accepted
                    .iter()
                    .any(|c| c.is_established() || c.is_closed())
            };
            if ready {
                this.shared.wake_acceptors();
            }

            if let Err(err) = this.service(cx) {
                this.inner
                    .fail_all(|| Error::new(ErrorKind::Socket, "the socket failed"));
                this.stopped = true;
                this.shared.mark_gone();
                return Poll::Ready(Err(err));
            }

            // Rearm after *every* pass, including one that only wrote. ngtcp2 folds its
            // pacing deadline into `expiry()`, so this is what lets a paced connection send
            // its second datagram -- without it, a bulk transfer stops after the first.
            let fired = this.inner.rearm(cx) == Poll::Ready(());

            if !read && !fired && !this.inner.has_pending() {
                break;
            }
        }

        Poll::Pending
    }
}

impl<Sock, Clk, B> Drop for EndpointDriver<Sock, Clk, B>
where
    Sock: AsyncUdpSocket,
    Clk: Clock,
    B: TlsBackend,
{
    fn drop(&mut self) {
        // Defined rather than undefined. A caller that drops the driver while holding
        // handles has made a mistake the compiler cannot catch, and the useful behaviour is
        // for every pending operation to fail at once rather than wait for a driver that
        // will never run.
        self.inner
            .fail_all(|| Error::new(ErrorKind::DriverGone, "the endpoint driver was dropped"));
        self.inner.wake_all();

        // Dials that were queued but never picked up are the easy case to miss: they are
        // not connections yet, so failing every connection does not reach them, and a
        // caller awaiting one would wait for a driver that no longer exists.
        let dials: Vec<Dial> = self.shared.lock().dials.drain(..).collect();
        for dial in dials {
            dial.shared.fail(Error::new(
                ErrorKind::DriverGone,
                "the endpoint driver was dropped before this connection was opened",
            ));
        }

        self.shared.mark_gone();
    }
}
