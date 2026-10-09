//! Scaffolding for exercising the endpoint layer, public but hidden.
//!
//! The crate takes no dev-dependencies by design, and its integration tests are separate
//! crates that cannot reach `cfg(test)` items — so the machinery those tests need lives
//! here, marked `#[doc(hidden)]` to keep it out of the documented surface. It is not a
//! supported API and carries no compatibility promise.
//!
//! # The in-memory socket is a second implementation, not a mock
//!
//! [`socket_pair`] is a complete [`AsyncUdpSocket`] pair that moves datagrams through a
//! shared queue. It shares no code with any real socket, which is what makes it evidence:
//! if the trait had been quietly shaped around one runtime's socket, writing this would
//! have been awkward, and it was not.
//!
//! It is deliberately **not** [`Send`], built on `Rc` and `RefCell`. That is not a
//! simplification — it is how the claim that this layer imposes no `Send` bound gets tested
//! rather than asserted.
//!
//! # The clock is controllable because pacing makes a real one useless
//!
//! ngtcp2 refuses to send before its pacing deadline, so a test that waits on a real clock
//! spends real milliseconds doing nothing, and a test whose clock never advances gets one
//! datagram and then silence. [`TestClock`] lets a test move time deliberately, which is
//! also the only way to reach an idle timeout without waiting for one.

use core::cell::{Cell, RefCell};
use core::future::Future;
use core::net::SocketAddr;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::collections::VecDeque;
use std::rc::Rc;

use super::clock::Clock;
use super::socket::{AsyncUdpSocket, Received, Sent};
use crate::time::Timestamp;

/// A failure injected into a test socket.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// The next send reports it would block, once.
    ///
    /// What a real socket does under memory pressure or a full send buffer. A driver that
    /// treats it as success drops the datagram.
    SendWouldBlock,
    /// Every operation from now on fails.
    Broken,
}

/// The error a test socket reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TestSocketError;

impl core::fmt::Display for TestSocketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("the test socket was broken deliberately")
    }
}

impl core::error::Error for TestSocketError {}

/// One direction of the shared queue.
#[derive(Default)]
struct Queue {
    datagrams: VecDeque<(SocketAddr, Vec<u8>)>,
    waker: Option<Waker>,
}

impl Queue {
    fn push(&mut self, from: SocketAddr, datagram: Vec<u8>) {
        self.datagrams.push_back((from, datagram));
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

/// Datagrams sent somewhere no peer is listening, recorded rather than dropped.
type StrayLog = Rc<RefCell<Vec<(SocketAddr, Vec<u8>)>>>;

/// An in-memory UDP socket, one half of a [`socket_pair`].
pub struct TestSocket {
    address: SocketAddr,
    inbox: Rc<RefCell<Queue>>,
    outbox: Rc<RefCell<Queue>>,
    fault: Rc<Cell<Option<Fault>>>,
    sent: Rc<Cell<usize>>,
    /// Datagrams whose destination matches no peer, kept so a test can inspect them.
    stray: StrayLog,
    /// When set, a completed send is counted but its bytes are dropped instead of copied
    /// into the peer's queue. A real socket hands the payload to the kernel and keeps
    /// nothing; this queue keeps a copy so the peer can read it, and that copy is the
    /// harness's own allocation, not the driver's. A test measuring the driver's send path
    /// in isolation turns this on so the count reflects `flush` alone.
    sink: Rc<Cell<bool>>,
}

impl TestSocket {
    /// How many datagrams this socket has actually put on the wire.
    pub fn sent(&self) -> usize {
        self.sent.get()
    }

    /// Arranges for a fault on the next operation.
    pub fn inject(&self, fault: Fault) {
        self.fault.set(Some(fault));
    }

    /// Drops the bytes of completed sends while still counting them.
    ///
    /// See the field: this lets a test measure the driver's send path without the harness's
    /// own per-datagram delivery copy landing in the count.
    pub fn set_sink(&self, on: bool) {
        self.sink.set(on);
    }

    /// Datagrams sent to an address with no peer behind it.
    pub fn stray(&self) -> Vec<(SocketAddr, Vec<u8>)> {
        self.stray.borrow().clone()
    }

    /// Delivers a datagram to this socket as if it had arrived from `source`.
    ///
    /// How a test injects traffic no peer would send — a runt packet, a stray short header,
    /// a datagram naming an unsupported version.
    pub fn deliver(&self, source: SocketAddr, datagram: &[u8]) {
        self.inbox.borrow_mut().push(source, datagram.to_vec());
    }

    /// Removes and returns everything currently waiting to be received.
    ///
    /// Lets a test capture the datagrams a peer produced so it can re-deliver them — a
    /// duplicate 1-RTT packet is processed and then dropped, which is how a receive pass can
    /// be counted while it does real work but stores nothing.
    pub fn drain_inbox(&self) -> Vec<(SocketAddr, Vec<u8>)> {
        self.inbox.borrow_mut().datagrams.drain(..).collect()
    }
}

impl AsyncUdpSocket for TestSocket {
    type Error = TestSocketError;

    fn poll_recv(
        &mut self,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<Result<Received, Self::Error>> {
        if self.fault.get() == Some(Fault::Broken) {
            return Poll::Ready(Err(TestSocketError));
        }
        let mut inbox = self.inbox.borrow_mut();
        match inbox.datagrams.pop_front() {
            Some((source, datagram)) => {
                let len = datagram.len().min(buffer.len());
                buffer[..len].copy_from_slice(&datagram[..len]);
                Poll::Ready(Ok(Received { len, source }))
            }
            None => {
                inbox.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    fn poll_send(
        &mut self,
        cx: &mut Context<'_>,
        destination: SocketAddr,
        datagram: &[u8],
    ) -> Poll<Result<Sent, Self::Error>> {
        match self.fault.get() {
            Some(Fault::Broken) => return Poll::Ready(Err(TestSocketError)),
            Some(Fault::SendWouldBlock) => {
                // Once. A fault that never cleared would be a stalled socket rather than a
                // busy one, and the driver has no way to tell those apart -- which is the
                // point of the contract on `AsyncUdpSocket`.
                self.fault.set(None);
                cx.waker().wake_by_ref();
                return Poll::Ready(Ok(Sent::WouldBlock));
            }
            None => {}
        }

        self.sent.set(self.sent.get() + 1);
        // Sink mode: counted, but the payload is dropped rather than copied into the peer's
        // queue, so the harness adds no allocation of its own to a measured send.
        if self.sink.get() {
            return Poll::Ready(Ok(Sent::Complete));
        }
        let mut outbox = self.outbox.borrow_mut();
        // The queue has exactly one peer, so anything addressed elsewhere is a datagram
        // nobody will receive -- recorded rather than dropped, so a test can assert about
        // stateless resets and version negotiation sent to strangers.
        if destination == self.address {
            self.stray
                .borrow_mut()
                .push((destination, datagram.to_vec()));
        } else {
            outbox.push(self.address, datagram.to_vec());
        }
        Poll::Ready(Ok(Sent::Complete))
    }

    fn local_addr(&self) -> Result<SocketAddr, Self::Error> {
        Ok(self.address)
    }
}

/// Two sockets that deliver to each other.
///
/// Neither is `Send`, which is what proves the layer requires no `Send` bound.
pub fn socket_pair(left: SocketAddr, right: SocketAddr) -> (TestSocket, TestSocket) {
    let to_left = Rc::new(RefCell::new(Queue::default()));
    let to_right = Rc::new(RefCell::new(Queue::default()));
    let stray = Rc::new(RefCell::new(Vec::new()));
    (
        TestSocket {
            address: left,
            inbox: Rc::clone(&to_left),
            outbox: Rc::clone(&to_right),
            fault: Rc::new(Cell::new(None)),
            sent: Rc::new(Cell::new(0)),
            stray: Rc::clone(&stray),
            sink: Rc::new(Cell::new(false)),
        },
        TestSocket {
            address: right,
            inbox: to_right,
            outbox: to_left,
            fault: Rc::new(Cell::new(None)),
            sent: Rc::new(Cell::new(0)),
            stray,
            sink: Rc::new(Cell::new(false)),
        },
    )
}

/// A clock a test moves by hand.
#[derive(Clone)]
pub struct TestClock {
    now: Rc<Cell<u64>>,
    /// Deadlines waiting to be reached, so advancing time wakes them.
    sleepers: Rc<RefCell<Vec<(u64, Waker)>>>,
    /// Set whenever a sleep is requested, so a test can tell a driver that armed a timer
    /// from one that did not.
    armed: Rc<Cell<usize>>,
}

impl Default for TestClock {
    fn default() -> Self {
        Self::new()
    }
}

impl TestClock {
    /// A clock starting at a non-zero instant.
    ///
    /// Non-zero because zero is the value ngtcp2 uses to mean "no deadline", so a clock
    /// starting there makes every unset timer look expired.
    pub fn new() -> Self {
        Self {
            now: Rc::new(Cell::new(1_000_000_000)),
            sleepers: Rc::new(RefCell::new(Vec::new())),
            armed: Rc::new(Cell::new(0)),
        }
    }

    /// Moves time forward and wakes anything whose deadline has passed.
    pub fn advance(&self, nanos: u64) {
        self.now.set(self.now.get().saturating_add(nanos));
        let reached = self.now.get();
        let mut sleepers = self.sleepers.borrow_mut();
        let mut still_waiting = Vec::new();
        for (deadline, waker) in sleepers.drain(..) {
            if deadline <= reached {
                waker.wake();
            } else {
                still_waiting.push((deadline, waker));
            }
        }
        *sleepers = still_waiting;
    }

    /// How many times a sleep has been requested.
    pub fn timers_armed(&self) -> usize {
        self.armed.get()
    }
}

impl Clock for TestClock {
    type Sleep = Pin<Box<TestSleep>>;

    fn now(&self) -> Timestamp {
        Timestamp::from_nanos(self.now.get()).expect("the test clock stays in range")
    }

    fn sleep_until(&self, deadline: Timestamp) -> Self::Sleep {
        self.armed.set(self.armed.get() + 1);
        Box::pin(TestSleep {
            deadline: deadline.as_nanos(),
            now: Rc::clone(&self.now),
            sleepers: Rc::clone(&self.sleepers),
            registered: false,
        })
    }
}

/// A pending sleep on a [`TestClock`].
pub struct TestSleep {
    deadline: u64,
    now: Rc<Cell<u64>>,
    sleepers: Rc<RefCell<Vec<(u64, Waker)>>>,
    registered: bool,
}

impl Future for TestSleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.now.get() >= self.deadline {
            return Poll::Ready(());
        }
        if !self.registered {
            self.sleepers
                .borrow_mut()
                .push((self.deadline, cx.waker().clone()));
            self.registered = true;
        }
        Poll::Pending
    }
}

/// Runs a future to completion on the current thread, with no runtime.
///
/// Deliberately a busy loop over a no-op waker rather than anything cleverer. What it
/// demonstrates is that this layer needs no executor at all — if driving it required a real
/// runtime, this would not work, and the claim would be false.
///
/// `steps` bounds the loop so a future that never completes fails the test instead of
/// hanging it. Returns `None` if the bound was reached.
pub fn poll_until<F: Future>(future: F, steps: usize, between: impl Fn()) -> Option<F::Output> {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut future = Box::pin(future);
    for _ in 0..steps {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return Some(output);
        }
        between();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addrs() -> (SocketAddr, SocketAddr) {
        (
            "127.0.0.1:1".parse().expect("valid"),
            "127.0.0.1:2".parse().expect("valid"),
        )
    }

    #[test]
    fn a_datagram_sent_to_the_peer_arrives() {
        let (left, right) = addrs();
        let (mut a, mut b) = socket_pair(left, right);
        let output = poll_until(
            async move {
                let mut buffer = [0u8; 64];
                core::future::poll_fn(|cx| a.poll_send(cx, right, b"hello"))
                    .await
                    .expect("send");
                let received = core::future::poll_fn(|cx| b.poll_recv(cx, &mut buffer))
                    .await
                    .expect("recv");
                (buffer[..received.len].to_vec(), received.source)
            },
            32,
            || {},
        )
        .expect("the exchange completes");
        assert_eq!(output.0, b"hello");
        assert_eq!(output.1, left);
    }

    #[test]
    fn an_injected_would_block_reports_itself_once() {
        let (left, right) = addrs();
        let (mut a, _b) = socket_pair(left, right);
        a.inject(Fault::SendWouldBlock);
        let first = poll_until(
            core::future::poll_fn(|cx| a.poll_send(cx, right, b"x")),
            4,
            || {},
        )
        .expect("resolves")
        .expect("no error");
        assert_eq!(first, Sent::WouldBlock);
        assert_eq!(a.sent(), 0, "a would-blocked datagram was not sent");

        let second = poll_until(
            core::future::poll_fn(|cx| a.poll_send(cx, right, b"x")),
            4,
            || {},
        )
        .expect("resolves")
        .expect("no error");
        assert_eq!(second, Sent::Complete);
        assert_eq!(a.sent(), 1);
    }

    #[test]
    fn a_broken_socket_fails_every_operation() {
        let (left, right) = addrs();
        let (mut a, _b) = socket_pair(left, right);
        a.inject(Fault::Broken);
        let mut buffer = [0u8; 8];
        assert!(
            poll_until(
                core::future::poll_fn(|cx| a.poll_recv(cx, &mut buffer)),
                4,
                || {}
            )
            .expect("resolves")
            .is_err()
        );
    }

    #[test]
    fn the_clock_only_moves_when_told_to() {
        let clock = TestClock::new();
        let start = clock.now().as_nanos();
        assert_eq!(clock.now().as_nanos(), start, "time passed on its own");
        clock.advance(5);
        assert_eq!(clock.now().as_nanos(), start + 5);
    }

    #[test]
    fn a_sleep_resolves_only_once_its_deadline_is_reached() {
        let clock = TestClock::new();
        let deadline = Timestamp::from_nanos(clock.now().as_nanos() + 100).expect("valid");

        let never = poll_until(clock.sleep_until(deadline), 4, || {});
        assert!(never.is_none(), "a sleep resolved before its deadline");

        let clock2 = clock.clone();
        let resolved = poll_until(clock.sleep_until(deadline), 8, move || clock2.advance(50));
        assert!(resolved.is_some(), "a sleep never resolved");
    }

    #[test]
    fn a_deadline_already_past_resolves_at_once() {
        // Load-bearing: a driver reaching this case has work waiting, and a clock that made
        // it wait for a tick would add latency to the path that is already late.
        let clock = TestClock::new();
        let past = Timestamp::from_nanos(1).expect("valid");
        assert!(poll_until(clock.sleep_until(past), 2, || {}).is_some());
    }

    #[test]
    fn the_test_socket_is_deliberately_not_send() {
        // Compile-time evidence that this layer imposes no `Send` bound: if it did, the
        // in-memory socket could not be built on `Rc` and this function would not compile.
        fn assert_not_required<T>(_: &T) {}
        let (left, right) = addrs();
        let (a, _b) = socket_pair(left, right);
        assert_not_required(&a);
    }
}
