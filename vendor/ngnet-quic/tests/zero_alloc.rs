//! Proof that driving the send loop allocates nothing in the wrapper.
//!
//! The design reason it *can* be true is that the caller supplies the datagram buffer, so
//! nothing has to be allocated per packet. But that is an argument, not a guarantee, and it
//! is exactly the kind of property that decays silently: one `to_vec()` added inside the
//! wrapper for convenience would never fail a functional test.
//!
//! So a counting global allocator is installed and armed around the calls that matter.
//! Following the technique in `crates/ngnet-h3/tests/zero_alloc.rs`.

#![cfg(feature = "tls-ossl")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use ngnet_quic::{
    Backend as TlsBackend, ConnBuilder, EntropySource, Handlers, Inspection, OsslBackend,
    OsslSession, OwnedBytes, ReadOutcome, Result, Role, Settings, StreamWrite, Timestamp,
    TransportParams, Verify, WriteOutcome, inspect,
};

thread_local! {
    /// Allocations observed while armed.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    /// Whether this thread is currently counting.
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    /// Allocations larger than `LARGE_THRESHOLD` observed while armed.
    static LARGE: Cell<usize> = const { Cell::new(0) };
    /// Only allocations strictly larger than this are tallied in `LARGE`. A test measuring a
    /// send path sets this to the stream payload size so that the core's own copy of the
    /// accepted bytes -- which is exactly the payload -- is excluded and only a whole-datagram
    /// copy, which wraps the payload in QUIC framing and so is strictly larger, is counted.
    static LARGE_THRESHOLD: Cell<usize> = const { Cell::new(usize::MAX) };
}

struct Counting;

// SAFETY: every method forwards to the system allocator unchanged; the counters are
// thread-local and never affect the pointers returned.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: forwarding the caller's own contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarding the caller's own contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        // SAFETY: forwarding the caller's own contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: forwarding the caller's own contract.
        unsafe { System.alloc_zeroed(layout) }
    }
}

/// Records an allocation of `size` bytes, if counting is armed.
fn note(size: usize) {
    COUNTING.with(|counting| {
        if counting.get() {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
            if size > LARGE_THRESHOLD.with(Cell::get) {
                LARGE.with(|count| count.set(count.get() + 1));
            }
        }
    });
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Runs `f` with allocation counting armed, and reports how many were seen.
fn count_allocations<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let (value, total, _) = count_allocations_larger_than(usize::MAX, f);
    (value, total)
}

/// Runs `f` with allocation counting armed and reports both the total number of allocations
/// and how many were strictly larger than `threshold`. The large count lets a send-path test
/// distinguish a whole-datagram copy from the smaller allocations the core makes regardless.
fn count_allocations_larger_than<T>(threshold: usize, f: impl FnOnce() -> T) -> (T, usize, usize) {
    ALLOCATIONS.with(|count| count.set(0));
    LARGE.with(|count| count.set(0));
    LARGE_THRESHOLD.with(|t| t.set(threshold));
    COUNTING.with(|counting| counting.set(true));
    let value = f();
    COUNTING.with(|counting| counting.set(false));
    LARGE_THRESHOLD.with(|t| t.set(usize::MAX));
    let total = ALLOCATIONS.with(Cell::get);
    let large = LARGE.with(Cell::get);
    (value, total, large)
}

/// A counter, adequate because these tests do not depend on unpredictability.
struct StubEntropy(u8);

impl EntropySource for StubEntropy {
    fn fill(&mut self, dest: &mut [u8]) -> Result<()> {
        for slot in dest.iter_mut() {
            self.0 = self.0.wrapping_add(1);
            *slot = self.0;
        }
        Ok(())
    }
}

#[test]
fn writing_packets_allocates_nothing_in_the_wrapper() {
    let backend = OsslBackend::builder(Role::Client)
        .alpn("h3")
        .verify(Verify::DangerouslyAcceptAnyCertificate)
        .build()
        .expect("building a backend");
    let session = backend
        .new_session(Role::Client, None)
        .expect("creating a session");

    let start = Timestamp::from_nanos(1_000_000).unwrap();
    let mut conn = ConnBuilder::new(
        Role::Client,
        Settings::new(start),
        TransportParams::new(),
        Box::new(StubEntropy(0)),
        session,
        "127.0.0.1:1000".parse().unwrap(),
        "127.0.0.1:2000".parse().unwrap(),
    )
    .build(Handlers::new())
    .expect("building the connection");

    // The buffer is the caller's, allocated once and reused -- which is the whole reason
    // the send loop can be allocation-free.
    let mut buf = vec![0u8; 1500];

    // One write outside the count, so any lazily-initialised state inside OpenSSL or ngtcp2
    // is already warm. Counting that would measure the first call, not the loop.
    let _ = conn.write_pkt(&mut buf, start);

    let mut when = 2_000_000u64;
    // A count of zero proves nothing if the loop produced no datagram, so the region also
    // records how many bytes it wrote and asserts a datagram genuinely came out. The
    // warm-up above consumes the first Initial; the client still has a second one to send,
    // so at least one pass here writes a real packet.
    let ((datagrams, bytes), allocations) = count_allocations(|| {
        let mut datagrams = 0usize;
        let mut bytes = 0usize;
        for _ in 0..8 {
            let now = Timestamp::from_nanos(when).unwrap();
            when += 2_000_000;
            match conn.write_pkt(&mut buf, now) {
                Ok(WriteOutcome::Datagram { len }) => {
                    datagrams += 1;
                    bytes += len;
                }
                Ok(WriteOutcome::Idle | WriteOutcome::Blocked) => {}
                Err(_) => break,
            }
        }
        (datagrams, bytes)
    });

    assert!(
        datagrams > 0 && bytes > 0,
        "the send loop produced no datagram ({datagrams} datagrams, {bytes} bytes), so a \
         zero allocation count would prove nothing"
    );
    assert_eq!(
        allocations, 0,
        "the send loop allocated {allocations} times; the wrapper is supposed to write \
         into the caller's buffer and nothing else"
    );
}

#[test]
fn asking_for_the_expiry_allocates_nothing() {
    // Called on every pass of a caller's event loop, so it is worth knowing it is free.
    let backend = OsslBackend::builder(Role::Client)
        .alpn("h3")
        .verify(Verify::DangerouslyAcceptAnyCertificate)
        .build()
        .unwrap();
    let session = backend.new_session(Role::Client, None).unwrap();
    let start = Timestamp::from_nanos(1_000_000).unwrap();
    let conn = ConnBuilder::new(
        Role::Client,
        Settings::new(start),
        TransportParams::new(),
        Box::new(StubEntropy(0)),
        session,
        "127.0.0.1:1000".parse().unwrap(),
        "127.0.0.1:2000".parse().unwrap(),
    )
    .build(Handlers::new())
    .unwrap();

    // A count of zero is vacuous if the queries never ran, so the region reports the last
    // expiry it read and how many times it read one. The assertions below require both a
    // concrete deadline and that the loop actually executed -- a region that queried nothing
    // would satisfy neither.
    let ((ran, deadline), allocations) = count_allocations(|| {
        let mut ran = 0usize;
        let mut deadline = None;
        for _ in 0..64 {
            deadline = conn.expiry();
            let _ = conn.in_closing_period();
            let _ = conn.in_draining_period();
            let _ = conn.is_handshake_completed();
            ran += 1;
        }
        (ran, deadline)
    });

    assert!(
        ran > 0,
        "the query loop never ran, so a zero count proves nothing"
    );
    assert!(
        deadline.is_some(),
        "the connection reported no expiry, so the region queried nothing concrete"
    );
    assert_eq!(allocations, 0, "querying a connection should be free");
}

#[test]
fn the_counter_would_notice_a_real_allocation() {
    // A counting allocator that had stopped counting would make both tests above assert
    // nothing at all, and would do so silently.
    let (_, allocations) = count_allocations(|| {
        let v: Vec<u8> = Vec::with_capacity(1024);
        core::hint::black_box(&v);
    });
    assert!(
        allocations > 0,
        "the allocation counter is not counting, so the tests above prove nothing"
    );
}

#[test]
fn the_counter_is_disarmed_outside_a_measured_region() {
    let before = ALLOCATIONS.with(Cell::get);
    let v: Vec<u8> = Vec::with_capacity(4096);
    core::hint::black_box(&v);
    let after = ALLOCATIONS.with(Cell::get);
    assert_eq!(
        before, after,
        "allocations outside a measured region must not be counted"
    );
}

// The read and handshake regions below need two connections that have actually completed a
// handshake, driven entirely in memory. The wrapper is sans-I/O, so a datagram is moved from
// one side to the other by hand; there is no socket and no runtime. The certificate is the
// committed test one -- generating it would need a dev-dependency the crate forbids.

/// The committed self-signed certificate for `localhost`.
const TEST_CERT_PEM: &str = include_str!("data/test-cert.pem");
/// Its private key. Public and worthless; see `tests/data/README.md`.
const TEST_KEY_PEM: &str = include_str!("data/test-key.pem");

/// A clock the harness advances by hand.
///
/// ngtcp2 paces its sending, so a clock that never moves yields one datagram and then
/// silence. Advancing time between passes is what lets a handshake finish.
struct HandClock {
    now: Cell<u64>,
}

impl HandClock {
    fn new() -> Self {
        // Non-zero: ngtcp2 reads a zero start as a real instant an eternity ago.
        Self {
            now: Cell::new(1_000_000_000),
        }
    }

    fn now(&self) -> Timestamp {
        Timestamp::from_nanos(self.now.get()).unwrap()
    }

    fn advance(&self, nanos: u64) {
        self.now.set(self.now.get() + nanos);
    }

    fn advance_to(&self, when: Timestamp) {
        let target = when.as_nanos();
        if target > self.now.get() {
            self.now.set(target);
        }
    }
}

/// A connection under test.
type HandConn = ngnet_quic::Conn<'static, OsslSession>;

fn client_backend() -> OsslBackend {
    OsslBackend::builder(Role::Client)
        .alpn("h3")
        .verify(Verify::DangerouslyAcceptAnyCertificate)
        .build()
        .expect("a client backend")
}

fn server_backend() -> OsslBackend {
    OsslBackend::builder(Role::Server)
        .alpn("h3")
        .certificate_chain_pem(TEST_CERT_PEM)
        .private_key_pem(TEST_KEY_PEM)
        .build()
        .expect("a server backend")
}

/// Drains a connection's outbound datagrams, advancing the clock for pacing.
fn drain(conn: &mut HandConn, clock: &HandClock) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 1500];
    for _ in 0..64 {
        match conn
            .write_pkt(&mut buf, clock.now())
            .expect("writing a packet")
        {
            WriteOutcome::Datagram { len } => {
                out.push(buf[..len].to_vec());
                clock.advance(2_000_000);
            }
            WriteOutcome::Idle | WriteOutcome::Blocked => break,
        }
    }
    out
}

/// Relays datagrams between two connections until both go quiet, honouring their deadlines.
fn pump(client: &mut HandConn, server: &mut HandConn, clock: &HandClock, rounds: usize) {
    for _ in 0..rounds {
        let mut progressed = false;
        for datagram in drain(client, clock) {
            progressed = true;
            if server
                .read_pkt(&datagram, clock.now())
                .expect("server read")
                != ReadOutcome::Processed
            {
                return;
            }
        }
        for datagram in drain(server, clock) {
            progressed = true;
            if client
                .read_pkt(&datagram, clock.now())
                .expect("client read")
                != ReadOutcome::Processed
            {
                return;
            }
        }
        if client.is_handshake_completed() && server.is_handshake_completed() && !progressed {
            break;
        }
        if !progressed {
            let next = [client.expiry(), server.expiry()]
                .into_iter()
                .flatten()
                .min();
            match next {
                Some(deadline) => {
                    clock.advance_to(deadline);
                    clock.advance(1);
                    let _ = client.handle_expiry(clock.now());
                    let _ = server.handle_expiry(clock.now());
                }
                None => break,
            }
        }
    }
}

/// Builds a client and a server and drives them to a completed handshake, in memory.
///
/// The server can only be built from what the client's first packet carries, because
/// `original_dcid` is the client's initial destination identifier and ngtcp2 requires a
/// server to be told it.
fn establish(
    client_backend: &OsslBackend,
    server_backend: &OsslBackend,
    clock: &HandClock,
) -> (HandConn, HandConn) {
    let client_session = client_backend
        .new_session(Role::Client, Some("localhost"))
        .expect("a client session");
    let mut client = ConnBuilder::new(
        Role::Client,
        Settings::new(clock.now()),
        TransportParams::new(),
        Box::new(StubEntropy(0)),
        client_session,
        "127.0.0.1:1000".parse().unwrap(),
        "127.0.0.1:2000".parse().unwrap(),
    )
    .build(Handlers::new())
    .expect("building the client");

    let first_flight = drain(&mut client, clock);
    assert!(
        !first_flight.is_empty(),
        "a fresh client must have a first flight to send"
    );

    let (original_dcid, client_scid) = match inspect(&first_flight[0], 8).expect("decoding") {
        Inspection::Supported { dcid, scid, .. } => (dcid, scid),
        other => panic!("the first flight should be a supported long header, got {other:?}"),
    };

    let server_session = server_backend
        .new_session(Role::Server, None)
        .expect("a server session");
    let mut server = ConnBuilder::new(
        Role::Server,
        Settings::new(clock.now()),
        TransportParams::new().original_dcid(&original_dcid),
        Box::new(StubEntropy(64)),
        server_session,
        "127.0.0.1:2000".parse().unwrap(),
        "127.0.0.1:1000".parse().unwrap(),
    )
    .dcid(client_scid)
    .build(Handlers::new())
    .expect("building the server");

    for datagram in &first_flight {
        let _ = server.read_pkt(datagram, clock.now());
    }
    pump(&mut client, &mut server, clock, 32);

    (client, server)
}

#[test]
fn reading_a_packet_allocates_nothing_and_is_processed() {
    // SC-008. Reading a datagram on an established connection decrypts in place, so it must
    // allocate nothing -- and a zero count would be vacuous unless the packet was genuinely
    // taken in, so the region asserts the read reported `Processed`.
    let client_backend = client_backend();
    let server_backend = server_backend();
    let clock = HandClock::new();
    let (mut client, mut server) = establish(&client_backend, &server_backend, &clock);
    assert!(
        client.is_handshake_completed() && server.is_handshake_completed(),
        "the harness did not establish a connection to read on"
    );

    // A 1-RTT datagram carrying stream data, produced outside the count. This is the packet
    // the server will read inside it.
    let stream = client.open_uni_stream().expect("opening a stream");
    let mut buf = vec![0u8; 1500];
    let payload = [0x5au8; 64];
    let datagram = loop {
        match client
            .write_stream(&mut buf, stream, &payload, true, clock.now())
            .expect("writing stream data")
        {
            StreamWrite::Datagram { len, .. } if len > 0 => break buf[..len].to_vec(),
            StreamWrite::Datagram { .. } => clock.advance(2_000_000),
            other => panic!("the client produced no datagram to read: {other:?}"),
        }
    };

    // Warm the read path once, outside the count, so any lazily-initialised state is ready.
    // A duplicate 1-RTT packet is dropped rather than rejected, so this does not consume the
    // ability to observe a processed read below -- both report `Processed`.
    assert_eq!(
        server.read_pkt(&datagram, clock.now()).expect("warm read"),
        ReadOutcome::Processed
    );

    let (outcome, allocations) = count_allocations(|| {
        server
            .read_pkt(&datagram, clock.now())
            .expect("counted read")
    });

    assert_eq!(
        outcome,
        ReadOutcome::Processed,
        "the packet was not processed, so a zero allocation count would prove nothing"
    );
    assert_eq!(
        allocations, 0,
        "reading a packet on an established connection allocated {allocations} times; \
         ngtcp2 decrypts in place and the wrapper is supposed to add no copy"
    );
}

#[test]
fn establishing_a_connection_bounds_its_allocations() {
    // SC-006 (counting half) and FR-011. Establishing a connection is not allocation-free --
    // a handshake sets up TLS, transport parameters and stream state, and this in-memory
    // harness copies each datagram it relays. So this region is a BOUND, not an attribution:
    // it records the total the code produces today and fails only if a change pushes it
    // higher. Other forced allocations share this region, which is why it does not assert
    // zero. Its non-vacuity comes from asserting both roles reached a completed handshake.
    let client_backend = client_backend();
    let server_backend = server_backend();

    // Warm the backends and the allocator's own lazy state outside the count.
    {
        let clock = HandClock::new();
        let _ = establish(&client_backend, &server_backend, &clock);
    }

    let clock = HandClock::new();
    let ((client, server), allocations) =
        count_allocations(|| establish(&client_backend, &server_backend, &clock));

    assert!(
        client.is_handshake_completed() && server.is_handshake_completed(),
        "the handshake did not complete for both roles, so the recorded total is meaningless"
    );

    // The recorded bound. The observed total on this harness is 156 in both debug and
    // release; the handshake is deterministic in its allocation count even though its key
    // exchange is not. The bound sits a little above that so an OpenSSL or allocator
    // difference between environments does not fail it, while a change that allocates per
    // datagram or per pass -- the kind this audit removes -- would push it well past.
    const BOUND: usize = 220;
    assert!(
        allocations <= BOUND,
        "establishing a connection allocated {allocations} times, above the recorded bound \
         of {BOUND}; if this is an intended increase, raise the bound deliberately"
    );
    eprintln!("establishing a connection allocated {allocations} times");
}

#[test]
fn sending_owned_data_allocates_nothing_where_a_borrowed_send_allocates() {
    // SC-007 and FR-007. Both writes retain the bytes ngtcp2 accepts until they are
    // acknowledged. The borrowing write must copy them, because the caller may reuse its
    // buffer the moment the call returns; the owning write is handed the buffer and copies
    // nothing. The proof is the *difference*: the same payload, sent the two ways, allocates
    // at least once as a borrow and not at all as an owned send.
    let client_backend = client_backend();
    let server_backend = server_backend();
    let clock = HandClock::new();
    let (mut client, mut server) = establish(&client_backend, &server_backend, &clock);
    assert!(
        client.is_handshake_completed() && server.is_handshake_completed(),
        "the harness did not establish a connection to send on"
    );

    let stream = client.open_uni_stream().expect("opening a stream");
    let mut buf = vec![0u8; 1500];
    let payload = [0x5au8; 256];

    // Warm what the first write to a stream lazily allocates -- the stream's retention queue
    // and the map node that holds it -- outside the count. The accepted chunk is left live
    // and undelivered, so the queue keeps its capacity and the map keeps the entry; a later
    // push reuses that room instead of allocating. Warming is not cheating: the property
    // under test is that *sending owned data* copies nothing, not that the first touch of a
    // brand-new stream is free.
    let warm = client
        .write_stream(&mut buf, stream, &payload, false, clock.now())
        .expect("warm-up write");
    let warm_len = match warm {
        StreamWrite::Datagram { len, accepted } if accepted > 0 => len,
        other => panic!("the warm-up write was not accepted, so nothing was primed: {other:?}"),
    };
    if warm_len > 0 {
        server
            .read_pkt(&buf[..warm_len], clock.now())
            .expect("server reads the warm-up datagram");
    }
    assert!(
        client.retained_bytes() > 0,
        "the warm-up write should be retained until acknowledged"
    );

    // The owning send of an already-allocated buffer. The handle is built outside the count,
    // so its one allocation is not charged here; inside, only an `Arc` refcount bump happens
    // as the handle is cloned into retention. The clock is advanced first so ngtcp2's pacer
    // will emit a second datagram rather than returning `Blocked` at the warm-up's timestamp;
    // the step is far below the connection's PTO, so no retransmission fires to perturb the
    // count.
    clock.advance(20_000_000);
    let owned = OwnedBytes::new(payload.to_vec());
    let (owned_write, owned_allocs) = count_allocations(|| {
        client
            .write_stream_owned(&mut buf, stream, owned, false, clock.now())
            .expect("owned write")
    });
    assert_eq!(
        owned_allocs, 0,
        "sending owned data allocated {owned_allocs} times; it copies nothing and retains by \
         keeping the handle alive"
    );
    // Partial acceptance is real, not assumed: whatever ngtcp2 left is handed back as a view
    // into the same allocation, and it is exactly the payload minus what was taken. The
    // counted call must actually have produced a datagram carrying owned bytes -- a `Blocked`
    // or `Idle` outcome would send nothing, and a zero allocation count over a call that did
    // nothing would prove nothing -- so this requires a datagram that accepted bytes.
    let (owned_len, owned_accepted) = match owned_write.outcome {
        StreamWrite::Datagram { len, accepted } => (len, accepted),
        other => panic!(
            "the owned write produced no datagram ({other:?}); a zero allocation count over a \
             send that produced nothing proves nothing"
        ),
    };
    assert!(
        owned_accepted > 0,
        "the owned write accepted no bytes, so the counted call sent no owned data"
    );
    assert_eq!(
        owned_write.unsent.len(),
        payload.len() - owned_accepted,
        "the unaccepted suffix must be exactly what ngtcp2 left"
    );
    if owned_len > 0 {
        server
            .read_pkt(&buf[..owned_len], clock.now())
            .expect("server reads the owned datagram");
    }

    // The borrowing send of the identical payload. It cannot keep the borrow, so every
    // accepted byte is copied into a buffer the crate owns -- at least one allocation. The
    // clock is advanced again so the pacer emits this datagram too, and the outcome is
    // required to be a datagram that accepted bytes so the comparison is between two sends
    // that both did the same work -- not one against a `Blocked` no-op.
    clock.advance(20_000_000);
    let (borrowed_write, borrowed_allocs) = count_allocations(|| {
        client
            .write_stream(&mut buf, stream, &payload, false, clock.now())
            .expect("borrowed write")
    });
    let (borrowed_len, borrowed_accepted) = match borrowed_write {
        StreamWrite::Datagram { len, accepted } => (len, accepted),
        other => panic!(
            "the borrowing send produced no datagram ({other:?}); the comparison needs both \
             sends to have carried the payload"
        ),
    };
    assert!(
        borrowed_accepted > 0,
        "the borrowing send accepted no bytes, so it did not carry the payload"
    );
    if borrowed_len > 0 {
        server
            .read_pkt(&buf[..borrowed_len], clock.now())
            .expect("server reads the borrowed datagram");
    }
    assert!(
        borrowed_allocs >= 1,
        "the borrowing send must allocate the retained copy, but allocated {borrowed_allocs}"
    );
    assert!(
        borrowed_allocs > owned_allocs,
        "the difference is the whole proof: borrowed allocated {borrowed_allocs}, owned \
         {owned_allocs}"
    );

    // Both sends retain until acknowledged. Something is still held now; once every datagram
    // has reached the server and its acknowledgements have come back, retention drains away.
    assert!(
        client.retained_bytes() > 0,
        "the accepted sends must stay retained until they are acknowledged"
    );
    // A tolerant relay: unlike `pump`, it does not stop when a datagram is a duplicate, since
    // the stream data was already delivered by hand above and the retransmissions this
    // provokes are expected to be dropped. It runs until acknowledgements have drained
    // retention or a generous round cap is reached.
    for _ in 0..256 {
        if client.retained_bytes() == 0 {
            break;
        }
        let mut progressed = false;
        for datagram in drain(&mut client, &clock) {
            progressed = true;
            let _ = server.read_pkt(&datagram, clock.now());
        }
        for datagram in drain(&mut server, &clock) {
            progressed = true;
            let _ = client.read_pkt(&datagram, clock.now());
        }
        if !progressed {
            let next = [client.expiry(), server.expiry()]
                .into_iter()
                .flatten()
                .min();
            match next {
                Some(deadline) => {
                    clock.advance_to(deadline);
                    clock.advance(1);
                    let _ = client.handle_expiry(clock.now());
                    let _ = server.handle_expiry(clock.now());
                }
                None => break,
            }
        }
    }
    assert_eq!(
        client.retained_bytes(),
        0,
        "once acknowledged, both the borrowed and the owned sends must be released"
    );
}

// Phase 2's region needs a driver pass over an established connection, which is the endpoint
// layer rather than a bare `Conn`. The harness below is the in-memory, runtime-free one the
// endpoint's own integration tests use, rebuilt here from the crate's public
// `endpoint::testing` surface so this test needs no extra dependency.
#[cfg(feature = "endpoint")]
mod driver_pass {
    use super::{
        StubEntropy, TEST_CERT_PEM, TEST_KEY_PEM, count_allocations, count_allocations_larger_than,
    };
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll, Waker};
    use ngnet_quic::endpoint::testing::{TestClock, TestSocket, socket_pair};
    use ngnet_quic::endpoint::{
        Clock, Config, Connection, Endpoint, EndpointBuilder, EndpointDriver,
    };
    use ngnet_quic::{ApplicationErrorCode, OsslBackend, OsslSession, Role, Timestamp};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    type Driver = EndpointDriver<TestSocket, TestClock, OsslBackend>;

    fn build(role: Role, socket: TestSocket, clock: TestClock) -> (Endpoint<OsslSession>, Driver) {
        let backend = match role {
            Role::Client => OsslBackend::builder(Role::Client)
                .alpn("h3")
                .trust_anchor_pem(TEST_CERT_PEM)
                .use_system_trust_store(false)
                .build()
                .expect("a client backend"),
            Role::Server => OsslBackend::builder(Role::Server)
                .alpn("h3")
                .certificate_chain_pem(TEST_CERT_PEM)
                .private_key_pem(TEST_KEY_PEM)
                .build()
                .expect("a server backend"),
        };
        let seed = if role == Role::Client { 0 } else { 64 };
        let mut builder = EndpointBuilder::new(socket, clock, backend)
            .config(Config::new())
            .entropy(move || StubEntropy(seed));
        if role == Role::Server {
            builder = builder.accepts(true);
        }
        builder.build().expect("an endpoint")
    }

    fn poll_all(drivers: &mut [Pin<Box<Driver>>], cx: &mut Context<'_>) {
        for d in drivers.iter_mut() {
            let _ = d.as_mut().poll(cx);
        }
    }

    /// Builds a client and a server on an in-memory socket pair and drives them to an
    /// established connection on both sides, returning the drivers, the two connection
    /// handles, and the shared clock. The receive and send regions below all start here.
    fn establish(
        cx: &mut Context<'_>,
    ) -> (Vec<Pin<Box<Driver>>>, Connection, Connection, TestClock) {
        let (caddr, saddr) = (
            "127.0.0.1:4433".parse().unwrap(),
            "127.0.0.1:4434".parse().unwrap(),
        );
        let clock = TestClock::new();
        let (cs, ss) = socket_pair(caddr, saddr);
        let (client, cdrv) = build(Role::Client, cs, clock.clone());
        let (server, sdrv) = build(Role::Server, ss, clock.clone());
        let mut drivers: Vec<Pin<Box<Driver>>> = vec![Box::pin(cdrv), Box::pin(sdrv)];

        let mut connecting = Box::pin(client.connect(saddr, Some("localhost")));
        let mut accepting = Box::pin(server.accept());
        let mut cside: Option<Connection> = None;
        let mut sside: Option<Connection> = None;
        for _ in 0..400 {
            poll_all(&mut drivers, cx);
            if cside.is_none()
                && let Poll::Ready(r) = connecting.as_mut().poll(cx)
            {
                cside = Some(r.expect("the client handshake failed"));
            }
            if sside.is_none()
                && let Poll::Ready(r) = accepting.as_mut().poll(cx)
            {
                sside = Some(r.expect("the server accept failed"));
            }
            if cside.is_some() && sside.is_some() {
                break;
            }
            clock.advance(2_000_000);
        }
        let cside = cside.expect("a client connection");
        let sside = sside.expect("a server connection");
        assert!(
            cside.is_established() && sside.is_established(),
            "the harness did not establish a connection"
        );
        (drivers, cside, sside, clock)
    }

    #[test]
    fn a_receive_pass_to_an_attached_connection_does_not_allocate() {
        // Phase 3, SC-002. Every datagram received for an attached connection used to be
        // copied out of the reusable receive buffer with `to_vec` before dispatch, for no
        // reason -- the core reads it in place and keeps nothing. With that copy gone, a
        // receive pass that delivers to attached connections allocates nothing.
        //
        // The count is taken over `read_datagrams` alone, not a whole `poll`: the send half
        // still takes a datagram buffer eagerly until Phase 5, so a full pass could not
        // report zero yet. This isolates the receive half, which is what this phase changed.
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let (mut drivers, mut cside, mut sside, clock) = establish(&mut cx);

        // A stream the client will carry to the server. Opened and confirmed live before
        // anything is counted, so the region measures a genuine delivery.
        let sid = {
            let mut opening = cside.open_uni();
            loop {
                match Pin::new(&mut opening).poll(&mut cx) {
                    Poll::Ready(r) => break r.expect("opening a stream"),
                    Poll::Pending => {
                        poll_all(&mut drivers, &mut cx);
                        clock.advance(2_000_000);
                    }
                }
            }
        };

        // Round one warms the read path and captures what the client sent: the client
        // writes, only the client driver is polled so the datagrams pile up in the server's
        // socket, and the server's inbox is then drained so the test owns those bytes.
        let first = [0xa5u8; 256];
        cside.write(sid, &first, true).expect("stream write");
        for _ in 0..16 {
            let _ = drivers[0].as_mut().poll(&mut cx);
            clock.advance(2_000_000);
        }
        let captured = drivers[1].as_ref().socket_for_test().drain_inbox();
        assert!(
            !captured.is_empty(),
            "the client produced no datagram for the server to receive"
        );
        // Keep only the 1-RTT (short-header) packets: those route to the established
        // connection and are read in place. A long-header straggler would route nowhere and
        // reach the stateless-reset path instead, which allocates for reasons unrelated to
        // this phase.
        let onertt: Vec<(_, Vec<u8>)> = captured
            .into_iter()
            .filter(|(_, d)| !d.is_empty() && d[0] & 0x80 == 0)
            .collect();
        assert!(
            !onertt.is_empty(),
            "the client sent no 1-RTT datagram carrying the stream"
        );

        // Deliver them once, uncounted, so the core takes the stream data in and warms every
        // lazily-initialised path. This read stores bytes and so does allocate; it is not
        // what the region measures.
        for (source, datagram) in &onertt {
            drivers[1]
                .as_ref()
                .socket_for_test()
                .deliver(*source, datagram);
        }
        let _ = drivers[1]
            .as_mut()
            .get_mut()
            .read_datagrams_for_test(&mut cx)
            .expect("warm read");

        // Drive both sides and read the whole stream back on the server, outside the count.
        // This confirms the warmed delivery arrived intact, and -- crucially for the
        // measurement below -- it drains the connection's observed-event queue. A read pass
        // that left events unconsumed would reallocate that queue when it hands the events
        // back to the application, which is the connection's own plumbing rather than the
        // receive-buffer copy this phase removed.
        let mut received = Vec::new();
        let mut fin = false;
        let mut accepted = None;
        for _ in 0..200 {
            poll_all(&mut drivers, &mut cx);
            if accepted.is_none() {
                let mut a = sside.accept_stream();
                if let Poll::Ready(r) = Pin::new(&mut a).poll(&mut cx) {
                    accepted = Some(r.expect("accepting the stream"));
                }
            }
            if let Some(stream) = accepted {
                let mut reading = sside.read(stream);
                if let Poll::Ready(r) = Pin::new(&mut reading).poll(&mut cx) {
                    let chunk = r.expect("reading the stream");
                    received.extend_from_slice(&chunk.bytes);
                    fin = chunk.fin;
                }
            }
            if fin {
                break;
            }
            clock.advance(2_000_000);
        }
        assert!(fin, "the server never saw the end of the stream");
        assert_eq!(
            received, first,
            "the delivered bytes did not survive the receive pass intact"
        );

        // Now deliver the *same* datagrams again and count that pass. Their packet numbers
        // have already been seen, so the core reads each in place, recognises it as a
        // duplicate, and drops it, storing nothing and observing nothing. With the stream
        // already consumed there is no queued event to hand back either, so the only thing
        // that could allocate is a copy of the datagram itself -- exactly what this phase
        // removed.
        for (source, datagram) in &onertt {
            drivers[1]
                .as_ref()
                .socket_for_test()
                .deliver(*source, datagram);
        }
        let (progressed, allocations) = count_allocations(|| {
            drivers[1]
                .as_mut()
                .get_mut()
                .read_datagrams_for_test(&mut cx)
                .expect("counted read")
        });

        assert!(
            progressed,
            "the receive pass read no datagram, so a zero allocation count would prove nothing"
        );
        assert_eq!(
            allocations, 0,
            "a receive pass delivering to an attached connection allocated {allocations} \
             times; the core reads in place and the wrapper is supposed to add no copy"
        );
    }

    #[test]
    fn a_driver_pass_over_an_established_connection_does_not_allocate_for_iteration() {
        // Phase 2. `service` and `flush` each used to collect a `Vec<u64>` of connection
        // indices on every pass, so no pass could ever report zero however little it did.
        // With those replaced by a reusable scratch -- and, after Phase 5, with `next_datagram`
        // writing into the driver's reusable send buffer -- a pass that walks an established
        // connection and forwards a datagram it already owns allocates nothing. The region
        // below proves that by giving the counted pass one observable, zero-allocation thing to
        // do: flush a held datagram. The inversion (restoring the index vectors) pushes the
        // count above zero.
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let (caddr, saddr) = (
            "127.0.0.1:4433".parse().unwrap(),
            "127.0.0.1:4434".parse().unwrap(),
        );
        let clock = TestClock::new();
        let (cs, ss) = socket_pair(caddr, saddr);
        let (client, cdrv) = build(Role::Client, cs, clock.clone());
        let (server, sdrv) = build(Role::Server, ss, clock.clone());
        let mut drivers: Vec<Pin<Box<Driver>>> = vec![Box::pin(cdrv), Box::pin(sdrv)];

        let mut connecting = Box::pin(client.connect(saddr, Some("localhost")));
        let mut accepting = Box::pin(server.accept());
        let mut cside: Option<Connection> = None;
        let mut sside: Option<Connection> = None;
        for _ in 0..400 {
            poll_all(&mut drivers, &mut cx);
            if cside.is_none()
                && let Poll::Ready(r) = connecting.as_mut().poll(&mut cx)
            {
                cside = Some(r.expect("the client handshake failed"));
            }
            if sside.is_none()
                && let Poll::Ready(r) = accepting.as_mut().poll(&mut cx)
            {
                sside = Some(r.expect("the server accept failed"));
            }
            if cside.is_some() && sside.is_some() {
                break;
            }
            clock.advance(2_000_000);
        }
        let mut cside = cside.expect("a client connection");
        let mut sside = sside.expect("a server connection");
        assert!(
            cside.is_established() && sside.is_established(),
            "the harness did not establish a connection for the pass to service"
        );

        // The pass does real work: 128 bytes of application data cross the connection under
        // the driver's own passes, and the server accepts the stream. A region that measured
        // a pass over a dead or empty connection would prove nothing, so this establishes
        // that the driver is genuinely servicing a live connection before anything is counted.
        let sid = {
            let mut opening = cside.open_uni();
            loop {
                match Pin::new(&mut opening).poll(&mut cx) {
                    Poll::Ready(r) => break r.expect("opening a stream"),
                    Poll::Pending => {
                        poll_all(&mut drivers, &mut cx);
                        clock.advance(2_000_000);
                    }
                }
            }
        };
        cside
            .write(sid, &[0x5au8; 128], true)
            .expect("writing stream data");
        let mut accepted = None;
        for _ in 0..64 {
            poll_all(&mut drivers, &mut cx);
            let mut accepting = sside.accept_stream();
            if let Poll::Ready(r) = Pin::new(&mut accepting).poll(&mut cx) {
                accepted = Some(r.expect("accepting the stream"));
                break;
            }
            clock.advance(2_000_000);
        }
        assert!(
            accepted.is_some(),
            "the server never saw the stream, so the driver pass did no work"
        );

        // Quiesce, then stop the clock so no timer re-arms and no retransmission fires: what
        // remains is a bare pass over an established, idle connection.
        for _ in 0..64 {
            poll_all(&mut drivers, &mut cx);
            clock.advance(2_000_000);
        }
        // Warm the scratch and the armed sleep at the now-fixed clock, outside the count.
        poll_all(&mut drivers, &mut cx);

        // A pass that only walks an idle connection could report zero simply because it
        // serviced nothing, so the counted pass is given one observable thing to do that still
        // allocates nothing: flush a datagram it already owns. The held stream is written and
        // its datagram refused once, so the driver holds it as `pending`; the counted pass must
        // then walk to this connection and hand that already-owned datagram to the socket, which
        // copies nothing. The socket is a sink from here so that delivery adds no allocation of
        // the harness's own to the count.
        //
        // The counted call is `service_for_test`, the whole command-and-send pass -- the two
        // connection-list walks Phase 2 gave a reusable scratch -- minus the socket read and the
        // timer re-arm a full `poll` also does. The re-arm allocates its boxed timer future when
        // the deadline moves, which the send here does; that allocation has nothing to do with
        // walking the list, so counting the walk excludes it.
        drivers[0].as_ref().socket_for_test().set_sink(true);
        let held_stream = {
            let mut opening = cside.open_uni();
            loop {
                match Pin::new(&mut opening).poll(&mut cx) {
                    Poll::Ready(r) => break r.expect("opening the held stream"),
                    Poll::Pending => {
                        poll_all(&mut drivers, &mut cx);
                    }
                }
            }
        };
        cside
            .write(held_stream, &[0x5au8; 128], true)
            .expect("writing the held stream");
        drivers[0]
            .as_ref()
            .socket_for_test()
            .inject(ngnet_quic::endpoint::testing::Fault::SendWouldBlock);
        let sent_before_stage = drivers[0].as_ref().socket_for_test().sent();
        // The command-and-timer half only: it produces the datagram and lets the socket refuse
        // it, holding it as `pending`, and -- unlike the send half -- does not then flush it.
        drivers[0]
            .as_mut()
            .get_mut()
            .service_commands_for_test(&mut cx);
        assert_eq!(
            drivers[0].as_ref().socket_for_test().sent(),
            sent_before_stage,
            "the staged datagram was sent, not refused, so nothing is held for the pass to flush"
        );
        assert!(
            drivers[0].as_ref().has_pending_for_test(),
            "the refused datagram was not held, so the counted pass would have nothing to flush"
        );

        // The counted pass: it walks its single established connection and flushes the held
        // datagram.
        let sent_before = drivers[0].as_ref().socket_for_test().sent();
        let (result, allocations) =
            count_allocations(|| drivers[0].as_mut().get_mut().service_for_test(&mut cx));
        result.expect("the counted pass failed");
        let sent_after = drivers[0].as_ref().socket_for_test().sent();

        assert!(
            cside.is_established() && sside.is_established(),
            "the connection did not survive the pass, so the count is meaningless"
        );
        // The observable action: the counted pass sent the held datagram and no longer holds
        // it. A pass that walked no connection would have flushed nothing and left it pending,
        // so this ties the zero count below to a pass that genuinely serviced the connection.
        assert!(
            sent_after > sent_before,
            "the counted pass sent nothing, so it serviced no connection and a zero count would \
             prove nothing"
        );
        assert!(
            !drivers[0].as_ref().has_pending_for_test(),
            "the counted pass left the held datagram unsent, so it did not service the connection"
        );
        // Zero: walking the connection list uses a reusable scratch rather than a fresh
        // `Vec<u64>`, `next_datagram` writes into the driver's reusable send buffer instead of
        // taking one of its own, and the held datagram is forwarded as itself. So a pass that
        // services an established connection -- flushing a datagram it already owns -- allocates
        // nothing at all. The inversion check restores the two index vectors, each of which
        // pushes this count above zero.
        assert_eq!(
            allocations, 0,
            "a driver pass that serviced one established connection allocated {allocations} \
             times; walking the connection list and forwarding an owned datagram is supposed to \
             allocate nothing"
        );
    }

    /// A clock two endpoints can share across the `Send + Sync` bound that `build_detachable`
    /// requires. `TestClock` is built on `Rc` and so cannot cross that bound; this is the
    /// same hand-moved clock built on an atomic instead. It registers no wakers because the
    /// tests below drive with a busy poll over a no-op waker and move time by hand between
    /// polls, so a sleeper only has to resolve once its deadline has passed.
    #[derive(Clone)]
    struct SharedClock {
        now: Arc<AtomicU64>,
    }

    impl SharedClock {
        fn new() -> Self {
            Self {
                now: Arc::new(AtomicU64::new(1_000_000_000)),
            }
        }

        fn advance(&self, nanos: u64) {
            self.now.fetch_add(nanos, Ordering::Relaxed);
        }
    }

    struct SharedSleep {
        deadline: u64,
        now: Arc<AtomicU64>,
    }

    impl Future for SharedSleep {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            if self.now.load(Ordering::Relaxed) >= self.deadline {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }

    impl Clock for SharedClock {
        type Sleep = Pin<Box<SharedSleep>>;

        fn now(&self) -> Timestamp {
            Timestamp::from_nanos(self.now.load(Ordering::Relaxed))
                .expect("the shared clock stays in range")
        }

        fn sleep_until(&self, deadline: Timestamp) -> Self::Sleep {
            Box::pin(SharedSleep {
                deadline: deadline.as_nanos(),
                now: Arc::clone(&self.now),
            })
        }
    }

    type DetDriver = EndpointDriver<TestSocket, SharedClock, OsslBackend>;

    /// Builds an endpoint that can hand its connections over, on the shared clock. Same as
    /// `build`, but `build_detachable` rather than `build`, which is what makes a connection
    /// reach the detached branch this phase measures.
    fn build_detachable(
        role: Role,
        socket: TestSocket,
        clock: SharedClock,
    ) -> (Endpoint<OsslSession>, DetDriver) {
        let backend = match role {
            Role::Client => OsslBackend::builder(Role::Client)
                .alpn("h3")
                .trust_anchor_pem(TEST_CERT_PEM)
                .use_system_trust_store(false)
                .build()
                .expect("a client backend"),
            Role::Server => OsslBackend::builder(Role::Server)
                .alpn("h3")
                .certificate_chain_pem(TEST_CERT_PEM)
                .private_key_pem(TEST_KEY_PEM)
                .build()
                .expect("a server backend"),
        };
        let seed = if role == Role::Client { 0 } else { 64 };
        let mut builder = EndpointBuilder::new(socket, clock, backend)
            .config(Config::new())
            .entropy(move || StubEntropy(seed));
        if role == Role::Server {
            builder = builder.accepts(true);
        }
        builder.build_detachable().expect("a detachable endpoint")
    }

    #[test]
    fn a_receive_pass_to_a_detached_connection_allocates_one_buffer_per_datagram() {
        // Phase 4, SC-003 and SC-012. A connection whose owner has detached it is no longer
        // read by the endpoint: the endpoint routes datagrams to it but the protocol state
        // has one owner, elsewhere. So the receive pass copies each datagram into that
        // owner's queue instead of reading it. That copy is forced -- the datagram borrows
        // the endpoint's reusable receive buffer, which the next read overwrites, while the
        // owner may not collect until a later pass -- so this phase proves the copy costs
        // exactly one buffer per datagram, not that it can be removed.
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let (caddr, saddr) = (
            "127.0.0.1:4453".parse().unwrap(),
            "127.0.0.1:4454".parse().unwrap(),
        );
        let clock = SharedClock::new();
        let (cs, ss) = socket_pair(caddr, saddr);
        let (client, cdrv) = build_detachable(Role::Client, cs, clock.clone());
        let (server, sdrv) = build_detachable(Role::Server, ss, clock.clone());
        let mut drivers: Vec<Pin<Box<DetDriver>>> = vec![Box::pin(cdrv), Box::pin(sdrv)];

        // The server hands its connection over the moment the handshake completes; the client
        // keeps a managed handle so it can drive a stream from the other end.
        let mut connecting = Box::pin(client.connect(saddr, Some("localhost")));
        let mut detaching = Box::pin(server.accept_detached());
        let mut cside: Option<Connection> = None;
        let mut detached = None;
        for _ in 0..800 {
            for d in drivers.iter_mut() {
                let _ = d.as_mut().poll(&mut cx);
            }
            if cside.is_none()
                && let Poll::Ready(r) = connecting.as_mut().poll(&mut cx)
            {
                cside = Some(r.expect("the client handshake failed"));
            }
            if detached.is_none()
                && let Poll::Ready(r) = detaching.as_mut().poll(&mut cx)
            {
                detached = Some(r.expect("the server detach failed"));
            }
            if cside.is_some() && detached.is_some() {
                break;
            }
            clock.advance(2_000_000);
        }
        let mut cside = cside.expect("a client connection");
        let detached = detached.expect("a detached server connection");
        assert!(
            cside.is_established() && detached.conn.is_handshake_completed(),
            "the harness did not establish a connection to detach"
        );

        // A stream the client carries. The credit for it came from the server's transport
        // parameters during the handshake, so the client can open and write it with only its
        // own driver running -- the detached server no longer answers on the endpoint.
        let sid = {
            let mut opening = cside.open_uni();
            let mut sid = None;
            for _ in 0..200 {
                if let Poll::Ready(r) = Pin::new(&mut opening).poll(&mut cx) {
                    sid = Some(r.expect("opening a stream"));
                    break;
                }
                let _ = drivers[0].as_mut().poll(&mut cx);
                clock.advance(2_000_000);
            }
            sid.expect("a stream id")
        };
        let payload = [0xc3u8; 256];
        cside.write(sid, &payload, true).expect("stream write");
        for _ in 0..16 {
            let _ = drivers[0].as_mut().poll(&mut cx);
            clock.advance(2_000_000);
        }
        // Keep the short-header datagrams: those route to the detached connection and reach
        // the copy under test. A long-header straggler routes nowhere and takes the
        // stateless-reset path, which allocates for reasons unrelated to this phase.
        let onertt: Vec<(_, Vec<u8>)> = drivers[1]
            .as_ref()
            .socket_for_test()
            .drain_inbox()
            .into_iter()
            .filter(|(_, d)| !d.is_empty() && d[0] & 0x80 == 0)
            .collect();
        assert!(
            !onertt.is_empty(),
            "the client sent no 1-RTT datagram to route to the detached connection"
        );

        // Warm the owner's queue so it already holds the capacity the count needs. A fresh
        // `VecDeque` allocates its ring on the first push, which would show up in the count
        // as an allocation that is not the per-datagram copy. Delivering the datagrams once
        // and collecting them back grows that ring outside the count and leaves it in place.
        for (source, datagram) in &onertt {
            drivers[1]
                .as_ref()
                .socket_for_test()
                .deliver(*source, datagram);
        }
        let _ = drivers[1]
            .as_mut()
            .get_mut()
            .read_datagrams_for_test(&mut cx)
            .expect("warm read");
        let mut warmed = Vec::new();
        while let Some(bytes) = detached.next_inbound() {
            warmed.push(bytes);
        }
        assert_eq!(
            warmed.len(),
            onertt.len(),
            "the warm delivery did not queue every datagram for the owner"
        );
        for (expected, got) in onertt.iter().zip(&warmed) {
            assert_eq!(
                &expected.1, got,
                "the queued bytes did not match what was sent"
            );
        }

        // Deliver the same datagrams again and count that pass. Each reaches the detached
        // branch and is copied into the owner's queue -- one owned buffer per datagram, and
        // nothing else, because the queue already has room and the endpoint reads none of it.
        for (source, datagram) in &onertt {
            drivers[1]
                .as_ref()
                .socket_for_test()
                .deliver(*source, datagram);
        }
        let (progressed, allocations) = count_allocations(|| {
            drivers[1]
                .as_mut()
                .get_mut()
                .read_datagrams_for_test(&mut cx)
                .expect("counted read")
        });

        assert!(
            progressed,
            "the receive pass read no datagram, so a fixed allocation count would prove nothing"
        );
        assert_eq!(
            allocations,
            onertt.len(),
            "a receive pass delivering {} datagrams to a detached connection allocated \
             {allocations} buffers; the forced copy should cost exactly one each",
            onertt.len()
        );

        // SC-012. The owner collects on this later pass and the bytes are intact and its own:
        // each queued buffer is a separate owned copy, so re-delivering the same datagram
        // does not alias the earlier one and nothing the endpoint reuses reaches through.
        let mut collected = Vec::new();
        while let Some(bytes) = detached.next_inbound() {
            collected.push(bytes);
        }
        assert_eq!(
            collected.len(),
            onertt.len(),
            "the owner did not receive one datagram per delivery on the later pass"
        );
        for (expected, got) in onertt.iter().zip(&collected) {
            assert_eq!(
                &expected.1, got,
                "a datagram the owner collected on a later pass did not survive intact"
            );
        }
        // The two passes queued distinct buffers: had the copy aliased the reusable receive
        // buffer, the first collection would not still match after the second delivery
        // overwrote it. Both do, so the buffers are independent.
        assert_eq!(
            warmed, collected,
            "the two passes did not hold independent bytes"
        );
        eprintln!("detached receive pass allocated {allocations} times");
    }

    #[test]
    fn a_completing_send_pass_copies_a_datagram_only_when_the_socket_refuses_it() {
        // Phase 5, SC-001, and the correction of a test that used to prove less than it
        // claimed. `write_stream` used to copy every produced stream datagram into
        // `tracked.pending` *before* the socket was consulted, and the old test paid that
        // copy outside the counted region by running the command half first, then counted
        // only the forwarding of an already-owned datagram. It never measured a completing
        // driver send pass.
        //
        // Command production now offers each stream datagram it produces to the socket before
        // the reusable buffer is reused, exactly as `flush` does for a core-produced
        // datagram. So a complete driver send pass -- command production and send counted
        // together -- copies the datagram out of the reusable buffer into `tracked.pending`
        // only when the socket refuses it; a datagram the socket accepts is sent straight from
        // the buffer and never copied.
        //
        // The proof is two otherwise identical complete send passes -- an accepting one and a
        // refusing one, each a first write to a fresh stream -- watched two ways. First,
        // `has_pending` reports directly whether the driver kept a copy: false after the
        // accepting pass, true after the refusing one. Second, allocation counting filtered to
        // sizes larger than the stream payload isolates a whole-datagram copy from the core's
        // own copy of the accepted bytes (which is exactly the payload, not larger): the
        // accepting pass makes no over-payload allocation, the refusing pass makes exactly one.
        // The inversion is built in both ways: were `write_stream` to copy into `pending`
        // unconditionally again, the accepting pass would report a pending copy and an
        // over-payload allocation too, and both assertions on it would fail.
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let (mut drivers, mut cside, _sside, clock) = establish(&mut cx);

        let open_uni =
            |cside: &mut Connection, drivers: &mut [Pin<Box<Driver>>], cx: &mut Context<'_>| {
                let mut opening = cside.open_uni();
                loop {
                    match Pin::new(&mut opening).poll(cx) {
                        Poll::Ready(r) => break r.expect("opening a stream"),
                        Poll::Pending => {
                            poll_all(drivers, cx);
                            clock.advance(2_000_000);
                        }
                    }
                }
            };
        let anchor_stream = open_uni(&mut cside, &mut drivers, &mut cx);
        let accept_stream = open_uni(&mut cside, &mut drivers, &mut cx);
        let refuse_stream = open_uni(&mut cside, &mut drivers, &mut cx);

        // The payload each counted write carries, and the threshold that separates a
        // whole-datagram copy from the core's own copy of the accepted bytes. The core keeps
        // exactly the payload it accepted; a datagram wraps that payload in QUIC framing and so
        // is strictly larger. Counting only allocations larger than the payload therefore sees
        // the `pending` datagram copy and nothing the core does on every send.
        const PAYLOAD: usize = 256;

        // Command production now performs the send: `write_stream` offers each datagram it
        // produces to the socket before the reusable buffer is reused. So a complete driver
        // send pass for a stream command is exactly `service_commands_for_test`, which is what
        // the counted regions below measure. The socket is a sink so a completed send is
        // counted but its bytes are dropped, keeping the harness's own delivery copy out of
        // the count.
        drivers[0].as_ref().socket_for_test().set_sink(true);

        // The anchor: one small write, left unacknowledged, so the driver's send buffer, index
        // scratch, retention map, and any lazy allocator state all warm here, outside every
        // counted region.
        cside
            .write(anchor_stream, &[0x5au8; 64], false)
            .expect("anchor write");
        drivers[0]
            .as_mut()
            .get_mut()
            .service_commands_for_test(&mut cx);
        drivers[0]
            .as_mut()
            .get_mut()
            .flush_for_test(&mut cx)
            .expect("anchor flush");
        clock.advance(1_000_000);

        // The accepting pass: a complete send pass whose stream datagram the socket accepts
        // straight from the reusable buffer. No `pending` copy is made, so no over-payload
        // allocation happens and the driver holds nothing afterwards.
        cside
            .write(accept_stream, &[0x5au8; PAYLOAD], true)
            .expect("accept-stream write");
        let sent_before = drivers[0].as_ref().socket_for_test().sent();
        let (_, accept_allocs, accept_large) = count_allocations_larger_than(PAYLOAD, || {
            drivers[0]
                .as_mut()
                .get_mut()
                .service_commands_for_test(&mut cx);
        });
        let sent_after = drivers[0].as_ref().socket_for_test().sent();
        let accept_pending = drivers[0].as_ref().has_pending_for_test();
        assert!(
            sent_after > sent_before,
            "the accepting pass sent no datagram, so a zero pending copy would prove nothing"
        );
        assert!(
            accept_allocs >= 1,
            "the accepting pass allocated nothing, so it produced no datagram and measured no \
             send"
        );
        assert!(
            !accept_pending,
            "the driver kept a copy of a datagram the socket accepted; an accepted datagram is \
             sent from the reusable buffer and must not be copied into `pending`"
        );
        assert_eq!(
            accept_large, 0,
            "the accepting pass made {accept_large} allocation(s) larger than the {PAYLOAD}-byte \
             payload; a datagram the socket accepts must not be copied, so nothing datagram-sized \
             should be allocated"
        );
        clock.advance(1_000_000);

        // The refusing pass: identical work, but the socket refuses the stream datagram once.
        // `write_stream` must then copy it out of the reusable buffer into `pending` -- the one
        // over-payload allocation a refusal adds -- and the driver holds it afterwards.
        cside
            .write(refuse_stream, &[0x5au8; PAYLOAD], true)
            .expect("refuse-stream write");
        let refused_before = drivers[0].as_ref().socket_for_test().sent();
        drivers[0]
            .as_ref()
            .socket_for_test()
            .inject(ngnet_quic::endpoint::testing::Fault::SendWouldBlock);
        let (_, _refuse_allocs, refuse_large) = count_allocations_larger_than(PAYLOAD, || {
            drivers[0]
                .as_mut()
                .get_mut()
                .service_commands_for_test(&mut cx);
        });
        let refused_after = drivers[0].as_ref().socket_for_test().sent();
        let refuse_pending = drivers[0].as_ref().has_pending_for_test();
        assert_eq!(
            refused_after, refused_before,
            "the refused send still counted as sent, so the refusal path was not exercised"
        );
        assert!(
            refuse_pending,
            "the socket refused the datagram but the driver kept no copy; a refused datagram \
             must be copied into `pending` so the reused buffer does not overwrite it"
        );
        assert_eq!(
            refuse_large, 1,
            "a refused stream datagram must cost exactly one over-payload allocation -- the \
             `pending` copy -- but the refusing pass made {refuse_large}"
        );
    }

    #[test]
    fn a_core_produced_datagram_costs_nothing_to_send_and_one_to_retain() {
        // Phase 5, SC-001, the two dispositions of a datagram the core writes into the
        // reusable buffer. When the socket accepts it, nothing is allocated: the bytes are
        // left in the buffer to be overwritten by the next datagram. When the socket refuses
        // it -- `WouldBlock` or `Pending` -- it must be retained across the pass, and since
        // the next pass will overwrite the buffer it has to be copied into one of its own.
        // That copy is the single allocation the send path can owe.
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let (mut drivers, mut cside, _sside, clock) = establish(&mut cx);

        let sid = {
            let mut opening = cside.open_uni();
            loop {
                match Pin::new(&mut opening).poll(&mut cx) {
                    Poll::Ready(r) => break r.expect("opening a stream"),
                    Poll::Pending => {
                        poll_all(&mut drivers, &mut cx);
                        clock.advance(2_000_000);
                    }
                }
            }
        };

        // Make the server owe an acknowledgement: the client writes ack-eliciting stream
        // data, only the client is polled so its datagrams pile up in the server's socket,
        // the server reads them, and the clock is moved past the acknowledgement delay. The
        // server's next flush then produces the ACK by writing it into the reusable buffer --
        // a genuine core-produced datagram.
        cside.write(sid, &[0x11u8; 256], true).expect("first write");
        for _ in 0..16 {
            let _ = drivers[0].as_mut().poll(&mut cx);
            clock.advance(2_000_000);
        }
        let _ = drivers[1]
            .as_mut()
            .get_mut()
            .read_datagrams_for_test(&mut cx)
            .expect("server reads the ack-eliciting data");
        clock.advance(50_000_000);

        // Complete disposition: the socket accepts the ACK, so flush allocates nothing.
        drivers[1].as_ref().socket_for_test().set_sink(true);
        let sent_before = drivers[1].as_ref().socket_for_test().sent();
        let (_, complete_allocs) = count_allocations(|| {
            drivers[1]
                .as_mut()
                .get_mut()
                .flush_for_test(&mut cx)
                .expect("completing flush");
        });
        let sent_after = drivers[1].as_ref().socket_for_test().sent();
        assert!(
            sent_after > sent_before,
            "the server sent no ack datagram, so the completing region did no work"
        );
        assert_eq!(
            complete_allocs, 0,
            "a core-produced datagram that the socket accepts allocated {complete_allocs} \
             times; a completed send is supposed to leave its bytes in the reusable buffer"
        );

        // Retain disposition: make the server owe a fresh ACK, refuse the send once, and the
        // datagram must be copied out of the reusable buffer to survive the pass -- exactly
        // one allocation.
        cside
            .write(sid, &[0x22u8; 256], true)
            .expect("second write");
        for _ in 0..16 {
            let _ = drivers[0].as_mut().poll(&mut cx);
            clock.advance(2_000_000);
        }
        let _ = drivers[1]
            .as_mut()
            .get_mut()
            .read_datagrams_for_test(&mut cx)
            .expect("server reads the second batch");
        clock.advance(50_000_000);

        drivers[1]
            .as_ref()
            .socket_for_test()
            .inject(ngnet_quic::endpoint::testing::Fault::SendWouldBlock);
        let sent_before = drivers[1].as_ref().socket_for_test().sent();
        let (_, retain_allocs) = count_allocations(|| {
            drivers[1]
                .as_mut()
                .get_mut()
                .flush_for_test(&mut cx)
                .expect("retaining flush");
        });
        let sent_after = drivers[1].as_ref().socket_for_test().sent();
        assert_eq!(
            sent_after, sent_before,
            "the refused send still counted as sent, so the retain path was not exercised"
        );
        assert_eq!(
            retain_allocs, 1,
            "a core-produced datagram the socket refuses allocated {retain_allocs} times; it \
             must be copied out of the reusable buffer exactly once to be retained"
        );
    }

    #[test]
    fn datagrams_sharing_the_reused_buffer_keep_their_own_bytes() {
        // Phase 5, SC-012. The send path now reuses one buffer across every datagram in a
        // pass, which is the correctness risk of buffer reuse: a datagram must still hold its
        // own bytes even as the buffer is rewritten for the next. A large payload forces many
        // datagrams through that single buffer and a refused send in the middle forces the
        // retain-and-copy path. This is an end-to-end check: it proves the transfer arrives
        // byte-for-byte under heavy reuse, and it would fail outright on any *systematic*
        // corruption of the buffer. It does not isolate a single transiently corrupted
        // datagram, which QUIC would retransmit and recover -- that soundness rests instead
        // on the retain copying out of the buffer (`buffer[..len].to_vec()`) and the borrow
        // checker refusing to let a datagram keep a borrow of the buffer across a pass.
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let (mut drivers, mut cside, mut sside, clock) = establish(&mut cx);

        let sid = {
            let mut opening = cside.open_uni();
            loop {
                match Pin::new(&mut opening).poll(&mut cx) {
                    Poll::Ready(r) => break r.expect("opening a stream"),
                    Poll::Pending => {
                        poll_all(&mut drivers, &mut cx);
                        clock.advance(2_000_000);
                    }
                }
            }
        };

        // A payload several packets long, each byte a function of its index so any datagram
        // written into the wrong place would show up as a mismatch.
        let payload: Vec<u8> = (0..24_000u32).map(|i| (i % 251) as u8).collect();
        cside.write(sid, &payload, true).expect("stream write");

        // Refuse one send early to drive the retain-and-copy path while the transfer is still
        // in flight, then let it recover.
        drivers[0]
            .as_ref()
            .socket_for_test()
            .inject(ngnet_quic::endpoint::testing::Fault::SendWouldBlock);

        let mut received = Vec::new();
        let mut fin = false;
        let mut accepted = None;
        for _ in 0..2_000 {
            poll_all(&mut drivers, &mut cx);
            if accepted.is_none() {
                let mut a = sside.accept_stream();
                if let Poll::Ready(r) = Pin::new(&mut a).poll(&mut cx) {
                    accepted = Some(r.expect("accepting the stream"));
                }
            }
            if let Some(stream) = accepted {
                let mut reading = sside.read(stream);
                if let Poll::Ready(r) = Pin::new(&mut reading).poll(&mut cx) {
                    let chunk = r.expect("reading the stream");
                    received.extend_from_slice(&chunk.bytes);
                    fin = chunk.fin;
                }
            }
            if fin {
                break;
            }
            clock.advance(2_000_000);
        }

        assert!(fin, "the server never saw the end of the stream");
        assert_eq!(
            received, payload,
            "a datagram carried the wrong bytes; reusing the send buffer corrupted the stream"
        );
    }

    /// Builds an endpoint whose per-connection entropy starts at a distinct seed, so two
    /// connections on one endpoint derive different connection identifiers instead of
    /// colliding -- two connections between the same address pair are told apart by
    /// identifier, never by address.
    fn build_distinct(
        role: Role,
        socket: TestSocket,
        clock: TestClock,
        base_seed: u8,
    ) -> (Endpoint<OsslSession>, Driver) {
        let backend = match role {
            Role::Client => OsslBackend::builder(Role::Client)
                .alpn("h3")
                .trust_anchor_pem(TEST_CERT_PEM)
                .use_system_trust_store(false)
                .build()
                .expect("a client backend"),
            Role::Server => OsslBackend::builder(Role::Server)
                .alpn("h3")
                .certificate_chain_pem(TEST_CERT_PEM)
                .private_key_pem(TEST_KEY_PEM)
                .build()
                .expect("a server backend"),
        };
        let next = Arc::new(AtomicU64::new(base_seed as u64));
        let mut builder = EndpointBuilder::new(socket, clock, backend)
            .config(Config::new())
            .entropy(move || StubEntropy((next.fetch_add(37, Ordering::Relaxed) & 0xff) as u8));
        if role == Role::Server {
            builder = builder.accepts(true);
        }
        builder.build().expect("an endpoint")
    }

    /// Two established client connections to one server, over a single socket pair.
    ///
    /// The server connections are in acceptance order, which is not necessarily the order the
    /// clients were dialled in.
    struct TwoConnections {
        drivers: Vec<Pin<Box<Driver>>>,
        clients: (Connection, Connection),
        servers: (Connection, Connection),
        clock: TestClock,
    }

    /// Drives two client connections to one server to establishment over a single socket
    /// pair.
    fn establish_two(cx: &mut Context<'_>) -> TwoConnections {
        let (caddr, saddr) = (
            "127.0.0.1:4455".parse().unwrap(),
            "127.0.0.1:4456".parse().unwrap(),
        );
        let clock = TestClock::new();
        let (cs, ss) = socket_pair(caddr, saddr);
        let (client, cdrv) = build_distinct(Role::Client, cs, clock.clone(), 1);
        let (server, sdrv) = build_distinct(Role::Server, ss, clock.clone(), 128);
        let mut drivers: Vec<Pin<Box<Driver>>> = vec![Box::pin(cdrv), Box::pin(sdrv)];

        let mut connecting_a = Box::pin(client.connect(saddr, Some("localhost")));
        let mut connecting_b = Box::pin(client.connect(saddr, Some("localhost")));
        let mut accepting = Box::pin(server.accept());
        let mut ca: Option<Connection> = None;
        let mut cb: Option<Connection> = None;
        let mut servers: Vec<Connection> = Vec::new();
        for _ in 0..1200 {
            poll_all(&mut drivers, cx);
            if ca.is_none()
                && let Poll::Ready(r) = connecting_a.as_mut().poll(cx)
            {
                ca = Some(r.expect("the first client handshake failed"));
            }
            if cb.is_none()
                && let Poll::Ready(r) = connecting_b.as_mut().poll(cx)
            {
                cb = Some(r.expect("the second client handshake failed"));
            }
            if servers.len() < 2
                && let Poll::Ready(r) = accepting.as_mut().poll(cx)
            {
                servers.push(r.expect("a server accept failed"));
                accepting = Box::pin(server.accept());
            }
            if ca.is_some() && cb.is_some() && servers.len() == 2 {
                break;
            }
            clock.advance(2_000_000);
        }
        let ca = ca.expect("a first client connection");
        let cb = cb.expect("a second client connection");
        assert_eq!(
            servers.len(),
            2,
            "the server did not accept both connections"
        );
        let mut servers = servers.into_iter();
        let sa = servers.next().unwrap();
        let sb = servers.next().unwrap();
        assert!(
            ca.is_established()
                && cb.is_established()
                && sa.is_established()
                && sb.is_established(),
            "the harness did not establish both connections"
        );
        TwoConnections {
            drivers,
            clients: (ca, cb),
            servers: (sa, sb),
            clock,
        }
    }

    #[test]
    fn a_second_connection_in_the_same_pass_keeps_its_own_bytes() {
        // SC-012, the cross-connection half. One send pass now walks every connection through
        // a single reusable buffer, so a second connection's datagram is composed in the very
        // buffer the first just used. This observes the datagrams that one pass produces
        // directly: they are captured off the wire and delivered to the server exactly once,
        // and the client is never polled again so it cannot retransmit. A datagram whose bytes
        // had been overwritten by the other connection's would fail its authentication tag and
        // its stream would arrive short, and with retransmission impossible nothing could
        // paper over it -- which is why this does not rely on the transfer recovering.
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let TwoConnections {
            mut drivers,
            clients: (mut ca, mut cb),
            servers: (mut sa, mut sb),
            clock,
        } = establish_two(&mut cx);

        let open_uni =
            |c: &mut Connection, drivers: &mut [Pin<Box<Driver>>], cx: &mut Context<'_>| {
                let mut opening = c.open_uni();
                loop {
                    match Pin::new(&mut opening).poll(cx) {
                        Poll::Ready(r) => break r.expect("opening a stream"),
                        Poll::Pending => {
                            poll_all(drivers, cx);
                            clock.advance(2_000_000);
                        }
                    }
                }
            };
        let sid_a = open_uni(&mut ca, &mut drivers, &mut cx);
        let sid_b = open_uni(&mut cb, &mut drivers, &mut cx);

        const PAYLOAD: usize = 256;
        let payload_a = [0xaau8; PAYLOAD];
        let payload_b = [0xbbu8; PAYLOAD];

        // Release the pacer, then clear whatever the handshakes left in the server's socket so
        // the capture below is exactly what the one measured pass produces.
        clock.advance(20_000_000);
        let _ = drivers[1].as_ref().socket_for_test().drain_inbox();

        // Both connections are given data, then a single client send pass composes a datagram
        // for each through the one reusable buffer.
        ca.write(sid_a, &payload_a, true).expect("client A write");
        cb.write(sid_b, &payload_b, true).expect("client B write");
        let sent_before = drivers[0].as_ref().socket_for_test().sent();
        drivers[0]
            .as_mut()
            .get_mut()
            .service_for_test(&mut cx)
            .expect("the client send pass failed");
        let sent_after = drivers[0].as_ref().socket_for_test().sent();
        assert!(
            sent_after - sent_before >= 2,
            "a single pass produced fewer than two datagrams, so it never exercised two \
             connections sharing the buffer"
        );
        let captured = drivers[1].as_ref().socket_for_test().drain_inbox();

        // The client is not polled again, so it cannot retransmit: whatever the server reads
        // now comes solely from this single delivery of the captured datagrams.
        for (source, datagram) in &captured {
            drivers[1]
                .as_ref()
                .socket_for_test()
                .deliver(*source, datagram);
        }

        let read_stream = |s: &mut Connection,
                           drivers: &mut [Pin<Box<Driver>>],
                           cx: &mut Context<'_>|
         -> Vec<u8> {
            let mut stream = None;
            for _ in 0..200 {
                let mut a = s.accept_stream();
                if let Poll::Ready(r) = Pin::new(&mut a).poll(cx) {
                    stream = Some(r.expect("accepting a stream"));
                    break;
                }
                let _ = drivers[1].as_mut().poll(cx);
            }
            let stream = stream.expect("the server never saw the stream");
            let mut bytes = Vec::new();
            for _ in 0..200 {
                let mut reading = s.read(stream);
                if let Poll::Ready(r) = Pin::new(&mut reading).poll(cx) {
                    let chunk = r.expect("reading a stream");
                    bytes.extend_from_slice(&chunk.bytes);
                    if chunk.fin {
                        break;
                    }
                }
                let _ = drivers[1].as_mut().poll(cx);
            }
            bytes
        };
        let got_a = read_stream(&mut sa, &mut drivers, &mut cx);
        let got_b = read_stream(&mut sb, &mut drivers, &mut cx);

        // Pairing-agnostic: whichever server connection carried which stream, both payloads
        // must appear intact and distinct, which they cannot if a datagram took the other's
        // bytes out of the shared buffer.
        let mut seen = [got_a, got_b];
        seen.sort();
        let mut want = [payload_a.to_vec(), payload_b.to_vec()];
        want.sort();
        assert_eq!(
            seen, want,
            "a datagram from the shared pass carried the wrong connection's bytes"
        );
    }

    #[test]
    fn a_held_close_datagram_keeps_its_own_bytes_while_the_buffer_is_reused() {
        // SC-012, the held-datagram half. A connection close is written into a buffer of its
        // own and parked in the connection's `pending` slot to be sent on a later pass. While
        // it waits, the driver goes on reusing its send buffer for another connection's work.
        // This captures the parked close bytes, drives a second connection's stream through
        // the reusable buffer repeatedly, then captures the parked bytes again: they must be
        // unchanged. It observes the held buffer directly rather than trusting a delivered
        // close, which a retransmitted close could otherwise repair.
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let TwoConnections {
            mut drivers,
            clients: (ca, mut cb),
            servers: (_sa, _sb),
            clock,
        } = establish_two(&mut cx);

        // The second connection carries a stream: the work that reuses the send buffer.
        let sid_b = {
            let mut opening = cb.open_uni();
            loop {
                match Pin::new(&mut opening).poll(&mut cx) {
                    Poll::Ready(r) => break r.expect("opening a stream"),
                    Poll::Pending => {
                        poll_all(&mut drivers, &mut cx);
                        clock.advance(2_000_000);
                    }
                }
            }
        };

        // Close the first connection, then run the command half alone so the close is composed
        // and parked in `pending` without being flushed.
        ca.close(ApplicationErrorCode::new(0x4242), b"held-close");
        clock.advance(20_000_000);
        drivers[0]
            .as_mut()
            .get_mut()
            .service_commands_for_test(&mut cx);

        let held_before = drivers[0].as_ref().held_datagrams_for_test();
        assert_eq!(
            held_before.len(),
            1,
            "closing one connection should park exactly one held datagram"
        );
        assert!(
            !held_before[0].is_empty(),
            "the parked close datagram is empty"
        );

        // Further work: the second connection writes repeatedly, each write composing a
        // datagram in the driver's reusable send buffer -- the buffer that would be corrupted
        // if the parked close had kept a borrow of it rather than its own copy.
        for i in 0..8u8 {
            let chunk = [0x30 + i; 256];
            cb.write(sid_b, &chunk, false).expect("client B write");
            clock.advance(20_000_000);
            drivers[0]
                .as_mut()
                .get_mut()
                .service_commands_for_test(&mut cx);
        }

        let held_after = drivers[0].as_ref().held_datagrams_for_test();
        assert_eq!(
            held_after.len(),
            1,
            "the parked close datagram went missing while the buffer was reused"
        );
        assert_eq!(
            held_after[0], held_before[0],
            "the parked close datagram's bytes changed while the send buffer was reused for \
             another connection"
        );
    }
}
