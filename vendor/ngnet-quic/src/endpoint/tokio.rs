//! Ready-made seams for tokio.
//!
//! Behind the off-by-default `tokio` feature, following `ngnet-h2`'s transport of the same
//! name and for the same reason: a caller on another runtime should not pay for this one,
//! and describing a socket to this crate is a small job for anyone who has to.
//!
//! # The clock mapping is the part worth reading
//!
//! [`Timestamp`] is an opaque nanosecond count in *the caller's* monotonic timescale, and
//! tokio measures time as an [`Instant`] with no public numeric value. Nothing converts
//! between them, so this picks an origin — the instant [`TokioClock`] was created — and
//! reports nanoseconds since then.
//!
//! Two consequences follow, and both are deliberate. Timestamps from two different
//! `TokioClock`s are not comparable, so an endpoint must be given one clock rather than a
//! fresh one per call. And the origin is *not* the process start or the epoch, so these
//! values mean nothing outside this crate and should not be logged as if they were wall
//! time.

use core::net::SocketAddr;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io;
use std::sync::Arc;
use std::time::Instant;

use tokio::net::UdpSocket;
use tokio::time::{Sleep, sleep_until};

use super::clock::Clock;
use super::socket::{AsyncUdpSocket, Received, Sent};
use crate::time::Timestamp;

/// An [`AsyncUdpSocket`] over tokio's.
///
/// Holds the socket in an [`Arc`] because tokio's `UdpSocket` is happy to be shared and the
/// endpoint wants `&mut self`; sharing costs nothing here and lets a caller keep a handle
/// to the socket they bound.
#[derive(Debug, Clone)]
pub struct TokioSocket {
    inner: Arc<UdpSocket>,
}

impl TokioSocket {
    /// Wraps an already-bound socket.
    pub fn new(socket: UdpSocket) -> Self {
        Self {
            inner: Arc::new(socket),
        }
    }

    /// Wraps a socket already behind an [`Arc`].
    pub fn from_arc(socket: Arc<UdpSocket>) -> Self {
        Self { inner: socket }
    }

    /// Binds a socket to `address`.
    ///
    /// # Errors
    ///
    /// Whatever binding a UDP socket can fail with.
    pub async fn bind(address: SocketAddr) -> io::Result<Self> {
        Ok(Self::new(UdpSocket::bind(address).await?))
    }

    /// The socket underneath, for a caller who needs to set an option on it.
    pub fn inner(&self) -> &Arc<UdpSocket> {
        &self.inner
    }
}

impl AsyncUdpSocket for TokioSocket {
    type Error = io::Error;

    fn poll_recv(
        &mut self,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<Result<Received, Self::Error>> {
        let mut read = tokio::io::ReadBuf::new(buffer);
        match self.inner.poll_recv_from(cx, &mut read) {
            Poll::Ready(Ok(source)) => Poll::Ready(Ok(Received {
                len: read.filled().len(),
                source,
            })),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_send(
        &mut self,
        cx: &mut Context<'_>,
        destination: SocketAddr,
        datagram: &[u8],
    ) -> Poll<Result<Sent, Self::Error>> {
        match self.inner.poll_send_to(cx, datagram, destination) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(Sent::Complete)),
            // A UDP send is not partial: it either takes the whole datagram or none of it,
            // so there is no short-write case to handle here.
            Poll::Ready(Err(err)) if err.kind() == io::ErrorKind::WouldBlock => {
                Poll::Ready(Ok(Sent::WouldBlock))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            // tokio registers the waker itself, which is what the trait's contract asks
            // for: the driver will be woken when the socket is writable again.
            Poll::Pending => Poll::Ready(Ok(Sent::WouldBlock)),
        }
    }

    fn local_addr(&self) -> Result<SocketAddr, Self::Error> {
        self.inner.local_addr()
    }
}

/// A [`Clock`] over tokio's timer.
///
/// Cheap to clone, and clones share an origin — which matters, because timestamps from two
/// clocks with different origins are not comparable and the core would read the difference
/// as an enormous elapsed time.
#[derive(Debug, Clone)]
pub struct TokioClock {
    origin: Instant,
}

impl Default for TokioClock {
    fn default() -> Self {
        Self::new()
    }
}

impl TokioClock {
    /// A clock whose origin is now.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// Converts one of this clock's timestamps back into a tokio instant.
    fn instant_for(&self, timestamp: Timestamp) -> tokio::time::Instant {
        tokio::time::Instant::from_std(self.origin + core::time::Duration::from_nanos(timestamp.as_nanos()))
    }
}

impl Clock for TokioClock {
    type Sleep = Pin<Box<Sleep>>;

    fn now(&self) -> Timestamp {
        let elapsed = self.origin.elapsed().as_nanos();
        // `u64::MAX` is the one value `Timestamp` rejects, and 584 years of nanoseconds is
        // beyond any process lifetime -- but saturating is still cheaper than the panic it
        // would otherwise be, and keeps this total.
        let nanos = u64::try_from(elapsed).unwrap_or(u64::MAX - 1);
        Timestamp::from_nanos(nanos.min(u64::MAX - 1)).expect("clamped below the rejected value")
    }

    fn sleep_until(&self, deadline: Timestamp) -> Self::Sleep {
        // `sleep_until` with a deadline already past resolves immediately, which is what
        // the `Clock` contract requires and what a driver already behind schedule needs.
        Box::pin(sleep_until(self.instant_for(deadline)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clocks_first_reading_is_near_its_origin() {
        let clock = TokioClock::new();
        // Not zero -- some time passes between construction and the call -- but the
        // distance from the origin is what is being pinned, since the whole mapping rests
        // on it.
        assert!(clock.now().as_nanos() < 1_000_000_000);
    }

    #[test]
    fn a_clocks_readings_do_not_go_backwards() {
        let clock = TokioClock::new();
        let first = clock.now().as_nanos();
        let second = clock.now().as_nanos();
        assert!(
            second >= first,
            "the clock went backwards, which the core reads as an enormous elapsed time"
        );
    }

    #[test]
    fn clones_share_an_origin() {
        // Load-bearing: an endpoint given a fresh clock per call would produce timestamps
        // from unrelated timescales, and the core compares them.
        let clock = TokioClock::new();
        let clone = clock.clone();
        let a = clock.now().as_nanos();
        let b = clone.now().as_nanos();
        assert!(b.abs_diff(a) < 1_000_000_000);
    }
}
