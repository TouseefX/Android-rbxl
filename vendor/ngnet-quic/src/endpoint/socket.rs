//! The UDP socket an endpoint runs over, described rather than chosen.
//!
//! # Why a trait at all
//!
//! This crate cannot pick an async runtime for its caller. A QUIC endpoint needs to send a
//! datagram, receive one, and know its own address, and every runtime spells those three
//! things differently. Naming one would make the crate unusable from the others, and
//! naming all of them would make it depend on all of them.
//!
//! So the endpoint takes a description of a socket and the caller supplies it. A ready-made
//! description for one widely used runtime ships behind an optional feature, and
//! [`crate::endpoint::testing`] contains a second implementation that moves datagrams in
//! memory — which is what makes "this is not shaped around one runtime" evidence rather
//! than an assertion.
//!
//! # Why poll-shaped rather than `async fn`
//!
//! `async fn` in a trait would read better at the call site and is the wrong tool here, for
//! two reasons that are about the driver rather than about style.
//!
//! The driver does several things in one wakeup: it drains the socket, it services expired
//! timers, and it writes. It has to ask "is there a datagram *right now*" and carry on if
//! the answer is no, which is precisely what [`Poll`] expresses and what awaiting a future
//! does not — an awaited receive would park the whole driver until a datagram arrived, and
//! the timers would not run.
//!
//! It also stores the socket behind a pointer, since an endpoint is generic over it and
//! held by a driver the caller owns. `async fn` in a trait is not dyn-compatible without
//! boxing every call, which would mean an allocation per datagram.
//!
//! # No `Send` bound
//!
//! There is none, here or anywhere in this subtree. Thread-per-core runtimes build their
//! I/O on `Rc`, and requiring `Send` would exclude them for the benefit of nobody. Auto
//! traits propagate instead: an endpoint over a `Send` socket is `Send` without anything
//! saying so.
//!
//! Note the asymmetry with the sans-I/O core, which *does* require `Send` on its handlers.
//! That is not an inconsistency: `Conn` is `Send`, so a non-`Send` handler inside it would
//! be unsound. Nothing here is `Send` by declaration, so nothing here needs the bound.

use core::net::SocketAddr;
use core::task::{Context, Poll};

/// A datagram that arrived, and where it came from.
///
/// The bytes are borrowed from the caller's own buffer: [`AsyncUdpSocket::poll_recv`] is
/// handed the buffer to fill, so a receive costs no allocation and no copy beyond the one
/// the kernel already did.
#[derive(Debug)]
pub struct Received {
    /// How many bytes of the buffer were filled.
    pub len: usize,
    /// The address the datagram came from.
    pub source: SocketAddr,
}

/// What happened to a send.
///
/// A send is not a future. UDP sends either take the datagram or say they cannot right now,
/// and modelling that as three outcomes lets the driver decide — retry later, drop, or fail
/// the connection — rather than having that decided for it by an await.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sent {
    /// The datagram was handed to the operating system.
    Complete,
    /// The socket could not take it now and the waker will fire when it can.
    ///
    /// The datagram has **not** been sent and must be offered again. A driver that treats
    /// this as success silently drops packets, which QUIC will eventually recover from and
    /// which will look like an unaccountably slow connection.
    WouldBlock,
}

/// An asynchronous UDP socket.
///
/// # Contract
///
/// Implementations must register the [`Context`]'s waker whenever they return
/// [`Poll::Pending`] or [`Sent::WouldBlock`], and must wake it when the condition clears. A
/// socket that returns `WouldBlock` and never wakes stalls every connection on it with no
/// error and no timeout — the endpoint has no way to detect it, because "nothing to do" and
/// "waiting forever" look the same from here.
///
/// A receive that fails for a reason specific to one datagram — an ICMP error attributed to
/// a previous send, for instance — should be reported as an error and the endpoint will
/// carry on. An error is treated as fatal to the whole endpoint, so an implementation that
/// can distinguish transient from permanent should absorb the transient ones itself.
pub trait AsyncUdpSocket {
    /// The failure type. Anything that can describe itself.
    type Error: core::fmt::Debug + core::fmt::Display;

    /// Attempts to receive one datagram into `buffer`.
    ///
    /// Returns [`Poll::Pending`] with the waker registered when nothing has arrived.
    ///
    /// # Errors
    ///
    /// A returned error is treated as fatal to the endpoint: every connection on this
    /// socket is failed with it.
    fn poll_recv(
        &mut self,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<Result<Received, Self::Error>>;

    /// Attempts to send one datagram.
    ///
    /// Returning [`Sent::WouldBlock`] means the datagram was not sent and must be offered
    /// again once the waker fires.
    ///
    /// # Errors
    ///
    /// A returned error is treated as fatal to the endpoint.
    fn poll_send(
        &mut self,
        cx: &mut Context<'_>,
        destination: SocketAddr,
        datagram: &[u8],
    ) -> Poll<Result<Sent, Self::Error>>;

    /// The address this socket is bound to.
    ///
    /// Every connection needs it: ngtcp2 is told the local and remote address of each
    /// datagram, and gets it wrong if this reports something other than what the peer sees.
    /// An implementation bound to a wildcard address should report the wildcard rather than
    /// guessing an interface.
    ///
    /// # Errors
    ///
    /// Returns an error if the socket has no address, which for a bound socket should not
    /// happen.
    fn local_addr(&self) -> Result<SocketAddr, Self::Error>;
}
