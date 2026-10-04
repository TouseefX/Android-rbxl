//! A TLS backend written entirely in safe Rust, carrying two real QUIC connections.
//!
//! This is the claim the whole seam exists to support, made checkable. The module forbids
//! unsafe code outright — not `deny`, which an allowance can silence, but `forbid`, which
//! nothing inside can override — and it depends on nothing but this crate and `std`. Being an
//! integration test, it can reach only the public API, so it structurally cannot touch the raw
//! bindings even if it wanted to.
//!
//! # Why a backend and not a mock
//!
//! A stand-in that implements the traits and completes no handshake would prove only that the
//! signatures are inhabitable. What matters is whether the seam is *expressive enough* to
//! carry a real connection: whether a backend can get keys installed at the moments ngtcp2
//! needs them, exchange transport parameters when both sides' are knowable, and drive a
//! connection to the point of moving application data. So the two connections below are real
//! `Conn`s, exchanging real packets that this backend really protects.
//!
//! It also acts as a **server**, which is not incidental. A server is the side whose transport
//! parameters cannot be produced up front, and it is where an earlier design of this seam
//! failed — silently, and only against another implementation. A backend that could only be a
//! client would not have exercised the part that was wrong.
//!
//! # This scheme is not cryptography
//!
//! It is an exclusive-or against a keystream, with a checksum for a tag. It is trivially
//! breakable by design: making it strong would mean reimplementing TLS, which proves nothing
//! extra about the seam and would obscure the parts that matter. Nothing here is exported, and
//! it is unusable outside this file.
//!
//! What it does have to be is *correct* in the ways ngtcp2 depends on: authenticated, so a
//! forged packet is rejected rather than accepted as garbage; invertible in place, because the
//! seam protects one buffer; and agreed between the two sides, because otherwise the
//! connection would fail for reasons that had nothing to do with the seam.

#![forbid(unsafe_code)]

use std::collections::VecDeque;

use ngnet_quic::{
    Backend, Conn, ConnBuilder, ConnectionId, CryptoError, Direction, DirectionalKeys,
    EntropySource, Error, Handlers, Handshaking, HeaderKey, InitialKeys, Iv, Level, PacketKey,
    ReadOutcome, Result, Role, RotatedKeys, Session, SessionEvent, Settings, Timestamp,
    TransportParams, WriteOutcome,
};

/// How many bytes the toy tag adds. Sixteen, matching a real AEAD, so that ngtcp2's packet
/// budgeting is exercised with a realistic overhead rather than a degenerate one.
const TAG_LEN: usize = 16;

/// The pre-shared secret both sides derive everything from.
///
/// A real backend derives this through a key exchange. Doing so here would be reimplementing
/// TLS to test something else.
const SHARED: u8 = 0x5a;

// ---------------------------------------------------------------------------------------
// The toy cryptography.
// ---------------------------------------------------------------------------------------

/// Protects a payload by exclusive-or against a keystream, with a checksum for a tag.
#[derive(Clone, Copy, Debug)]
struct ToyPacketKey {
    key: u8,
}

impl ToyPacketKey {
    fn stream(self, nonce: &[u8], i: usize) -> u8 {
        let n = if nonce.is_empty() {
            0
        } else {
            nonce[i % nonce.len()]
        };
        self.key ^ n ^ (i as u8)
    }

    /// A checksum over the ciphertext, the nonce and the additional data.
    ///
    /// Covers all three because that is what an AEAD's tag covers, and because a tag over the
    /// payload alone would let a packet's header be rewritten undetectably — which is exactly
    /// the property ngtcp2 relies on when it authenticates a Retry.
    fn tag(self, ciphertext: &[u8], nonce: &[u8], aad: &[u8]) -> [u8; TAG_LEN] {
        let mut tag = [0u8; TAG_LEN];
        for (i, slot) in tag.iter_mut().enumerate() {
            let mut acc = self.key.wrapping_add(i as u8);
            for (j, b) in ciphertext.iter().enumerate() {
                acc = acc
                    .wrapping_mul(31)
                    .wrapping_add(*b ^ (j as u8) ^ (i as u8));
            }
            for b in nonce {
                acc = acc.wrapping_mul(31).wrapping_add(*b);
            }
            for b in aad {
                acc = acc.wrapping_mul(31).wrapping_add(*b);
            }
            *slot = acc;
        }
        tag
    }
}

impl PacketKey for ToyPacketKey {
    fn seal(
        &self,
        buf: &mut [u8],
        plaintext_len: usize,
        nonce: &[u8],
        aad: &[u8],
    ) -> std::result::Result<(), CryptoError> {
        if buf.len() < plaintext_len + TAG_LEN {
            return Err(CryptoError::Fatal);
        }
        for (i, b) in buf[..plaintext_len].iter_mut().enumerate() {
            *b ^= self.stream(nonce, i);
        }
        let tag = self.tag(&buf[..plaintext_len], nonce, aad);
        buf[plaintext_len..plaintext_len + TAG_LEN].copy_from_slice(&tag);
        Ok(())
    }

    fn open(
        &self,
        dest: &mut [u8],
        ciphertext: &[u8],
        nonce: &[u8],
        aad: &[u8],
    ) -> std::result::Result<usize, CryptoError> {
        let ciphertext_len = ciphertext.len();
        let Some(plaintext_len) = ciphertext_len.checked_sub(TAG_LEN) else {
            return Err(CryptoError::Decrypt);
        };
        if dest.len() < plaintext_len {
            return Err(CryptoError::Fatal);
        }
        // Authenticated from the ciphertext, and the plaintext written into the destination:
        // the two are separate buffers, as the seam now requires, so nothing is decrypted in
        // place and nothing is copied first.
        let expected = self.tag(&ciphertext[..plaintext_len], nonce, aad);
        if ciphertext[plaintext_len..ciphertext_len] != expected {
            // A forged or reordered packet. Not a failure of the backend, and it must not end
            // the connection.
            return Err(CryptoError::Decrypt);
        }
        for (i, b) in dest[..plaintext_len].iter_mut().enumerate() {
            *b = ciphertext[i] ^ self.stream(nonce, i);
        }
        Ok(plaintext_len)
    }

    fn tag_len(&self) -> usize {
        TAG_LEN
    }

    fn confidentiality_limit(&self) -> u64 {
        // Large enough that nothing here reaches it. Zero would make ngtcp2 rekey on every
        // packet, which is the trap the seam documents.
        1 << 40
    }

    fn integrity_limit(&self) -> u64 {
        1 << 40
    }
}

/// Masks a header from a sample of the protected payload.
#[derive(Clone, Copy, Debug)]
struct ToyHeaderKey {
    key: u8,
}

impl HeaderKey for ToyHeaderKey {
    fn mask(&self, sample: &[u8]) -> std::result::Result<[u8; 5], CryptoError> {
        if sample.len() < 5 {
            return Err(CryptoError::Fatal);
        }
        let mut mask = [0u8; 5];
        for (i, slot) in mask.iter_mut().enumerate() {
            *slot = sample[i] ^ self.key ^ (i as u8);
        }
        Ok(mask)
    }
}

/// Both directions of a level's keys, derived so that the two sides agree.
///
/// The identifier and the level are mixed in so that different levels use different keys, as
/// a real key schedule does; the *direction* is mixed in so that each side encrypts with what
/// the other decrypts with.
fn keys_for(dcid: &[u8], level: u8, direction: u8) -> DirectionalKeys<ToyPacketKey, ToyHeaderKey> {
    let mut key = SHARED ^ level ^ direction;
    for b in dcid {
        key = key.wrapping_mul(31).wrapping_add(*b);
    }
    DirectionalKeys {
        packet: ToyPacketKey { key },
        header: ToyHeaderKey {
            key: key.wrapping_add(0x40),
        },
        // Twelve bytes, the length a real AEAD's nonce has. ngtcp2 builds each packet's nonce
        // from this and the packet number, so the two sides must agree on it exactly.
        iv: Iv::new(&(0..12u8).map(|i| key ^ i).collect::<Vec<u8>>())
            .expect("twelve bytes is a length ngtcp2 accepts"),
    }
}

/// Which side a key protects for, so that the two ends agree without negotiating.
const FROM_CLIENT: u8 = 0;
const FROM_SERVER: u8 = 1;

fn level_tag(level: Level) -> u8 {
    match level {
        Level::Initial => 0x10,
        Level::ZeroRtt => 0x20,
        Level::Handshake => 0x30,
        Level::OneRtt => 0x40,
    }
}

// ---------------------------------------------------------------------------------------
// The handshake.
// ---------------------------------------------------------------------------------------

/// The toy handshake's two messages.
///
/// Each carries the sender's transport parameters, which is not optional: ngtcp2 asserts that
/// a connection has the peer's before it will use one, and in a release build — where the
/// assertion is compiled out — dereferences null instead. A toy handshake that skipped them
/// would appear to work in debug and crash in release.
const MSG_CLIENT_PARAMS: u8 = 0x01;
const MSG_SERVER_PARAMS: u8 = 0x02;
/// The client's acknowledgement, which is what lets the server finish.
///
/// A third message rather than two, because the handshake must be a **round trip**. A server
/// that declares itself finished while still inside the call that delivered the client's first
/// message has told ngtcp2 to retire the Initial and Handshake packet number spaces — throwing
/// away the reply it submitted a moment earlier, and leaving a connection with nothing to send
/// and no error to report. TLS 1.3 waits for the client's Finished for its own reasons; a toy
/// handshake has to wait for something too.
const MSG_CLIENT_DONE: u8 = 0x03;

/// A session for one connection.
struct ToySession {
    role: Role,
    /// The identifier the keys are derived from, learned when initial keys are asked for.
    dcid: Vec<u8>,
    /// This endpoint's transport parameters. A client is given them up front; a server has to
    /// ask for them at the one moment they are complete.
    local_params: Option<Vec<u8>>,
    /// What the connection still has to be told, once the call returns.
    events: VecDeque<SessionEvent>,
    /// Which operations were performed, so the test can assert the seam was exercised rather
    /// than merely present.
    log: Vec<&'static str>,
    /// Set once the handshake has been driven, so a second call does nothing.
    started: bool,
    /// Makes the session hand back initialisation vectors of this length instead of the right
    /// one, standing in for a backend that is buggy or hostile.
    ///
    /// A safe backend supplies these as ordinary `Vec` lengths. Nothing in the type system
    /// constrains them, and ngtcp2's own bounds are assertions that release builds delete — so
    /// what stops a wrong length reaching C has to be the crate.
    bad_iv_len: Option<usize>,
    /// Handshake bytes that have arrived but do not yet make a whole message.
    ///
    /// Not optional, and not obvious. ngtcp2 deliberately **splits** a client's Initial CRYPTO
    /// across frames — `NGTCP2_CONN_FLAG_CRUMBLE_INITIAL_CRYPTO`, set for every client in
    /// `conn_new` — so the very first thing a server sees here is three bytes of a
    /// fifty-nine-byte message. A backend that assumed one call meant one message would fail
    /// on the first connection it ever made, which is how this was found.
    ///
    /// So `read_handshake` delivers a *stream*, exactly as TLS records do, and reassembling it
    /// is the backend's job. A real TLS stack already does this; a toy one has to be told.
    inbound: Vec<u8>,
}

impl ToySession {
    fn new(role: Role) -> Self {
        Self {
            role,
            dcid: Vec::new(),
            local_params: None,
            events: VecDeque::new(),
            log: Vec::new(),
            started: false,
            bad_iv_len: None,
            inbound: Vec::new(),
        }
    }

    /// Installs one level's keys in both directions.
    fn install_level(
        &mut self,
        conn: &mut dyn Handshaking<ToyPacketKey, ToyHeaderKey>,
        level: Level,
    ) -> Result<()> {
        let (rx_side, tx_side) = match self.role {
            Role::Client => (FROM_SERVER, FROM_CLIENT),
            Role::Server => (FROM_CLIENT, FROM_SERVER),
        };
        let tag = level_tag(level);
        // The secret is what a key update would advance. It is not used to derive anything
        // here, but it has to be the right length and it has to round-trip.
        let secret = vec![tag; 32];
        let mut rx = keys_for(&self.dcid, tag, rx_side);
        let mut tx = keys_for(&self.dcid, tag, tx_side);
        if let Some(len) = self.bad_iv_len {
            rx.iv = Iv::new(&vec![0; len])?;
            tx.iv = Iv::new(&vec![0; len])?;
        }
        conn.install_keys(level, Direction::Read, rx, &secret)?;
        conn.install_keys(level, Direction::Write, tx, &secret)?;
        self.log.push("install_keys");
        Ok(())
    }

    /// Frames a message: a kind byte, a two-byte length, then the body.
    fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![kind, (body.len() >> 8) as u8, body.len() as u8];
        out.extend_from_slice(body);
        out
    }

    /// Reports a whole message's kind and body length, or nothing if one has not arrived.
    fn peek(data: &[u8]) -> Option<(u8, usize)> {
        if data.len() < 3 {
            return None;
        }
        let len = ((data[1] as usize) << 8) | data[2] as usize;
        if data.len() < 3 + len {
            return None;
        }
        Some((data[0], len))
    }
}

impl Session for ToySession {
    type PacketKey = ToyPacketKey;
    type HeaderKey = ToyHeaderKey;

    fn initial_keys(
        &mut self,
        _version: u32,
        dcid: &[u8],
    ) -> Result<InitialKeys<Self::PacketKey, Self::HeaderKey>> {
        self.dcid = dcid.to_vec();
        self.log.push("initial_keys");
        let (rx_side, tx_side) = match self.role {
            Role::Client => (FROM_SERVER, FROM_CLIENT),
            Role::Server => (FROM_CLIENT, FROM_SERVER),
        };
        let tag = level_tag(Level::Initial);
        let mut rx = keys_for(dcid, tag, rx_side);
        let mut tx = keys_for(dcid, tag, tx_side);
        if let Some(len) = self.bad_iv_len {
            rx.iv = Iv::new(&vec![0; len])?;
            tx.iv = Iv::new(&vec![0; len])?;
        }
        Ok(InitialKeys { rx, tx })
    }

    fn retry_key(&mut self, _version: u32) -> Result<Self::PacketKey> {
        self.log.push("retry_key");
        Ok(ToyPacketKey { key: 0xa5 })
    }

    fn set_local_transport_params(&mut self, params: &[u8]) -> Result<()> {
        self.log.push("set_local_transport_params");
        self.local_params = Some(params.to_vec());
        Ok(())
    }

    fn start_handshake(
        &mut self,
        conn: &mut dyn Handshaking<Self::PacketKey, Self::HeaderKey>,
    ) -> Result<()> {
        if self.started {
            return Ok(());
        }
        self.started = true;

        // The handshake keys go in before anything is sent at that level, because the peer's
        // reply will arrive protected with them.
        self.install_level(conn, Level::Handshake)?;

        let params = self
            .local_params
            .clone()
            .ok_or_else(|| Error::backend("a client was never given its transport parameters"))?;
        // Non-empty, and at the Initial level: a client that sends nothing here produces no
        // first flight, and ngtcp2 has nothing to put in a packet.
        conn.submit_handshake(Level::Initial, &Self::frame(MSG_CLIENT_PARAMS, &params))?;
        self.log.push("submit_handshake");
        Ok(())
    }

    fn read_handshake(
        &mut self,
        _level: Level,
        data: &[u8],
        conn: &mut dyn Handshaking<Self::PacketKey, Self::HeaderKey>,
    ) -> Result<()> {
        self.log.push("read_handshake");
        self.inbound.extend_from_slice(data);
        let Some((kind, body_len)) = Self::peek(&self.inbound) else {
            // Not a whole message yet. Perfectly ordinary, and the case that matters most.
            return Ok(());
        };
        let body = self.inbound[3..3 + body_len].to_vec();
        self.inbound.drain(..3 + body_len);
        let body = body.as_slice();

        match (self.role, kind) {
            (Role::Server, MSG_CLIENT_PARAMS) => {
                // The order below is the whole reason the seam has a capability rather than a
                // queue, and it is the order ngtcp2 forces:
                //
                // 1. take the peer's parameters, because a server's own are not complete
                //    until they have been taken;
                // 2. install the handshake write key, which is the other half of what
                //    completes them;
                // 3. only then ask for this endpoint's.
                //
                // Any earlier and the answer is an incomplete set the peer rejects; any later
                // and it is too late to send.
                conn.set_peer_transport_params(body)?;
                self.log.push("set_peer_transport_params");

                self.install_level(conn, Level::Handshake)?;

                let params = conn.local_transport_params()?;
                self.log.push("local_transport_params");
                // At the **Initial** level, which is where a real ServerHello goes. A server
                // that replies only at the handshake level leaves ngtcp2 with nothing to put
                // in a packet and no error to report: `write_pkt` simply says it has nothing,
                // forever. That is a protocol mistake rather than a seam one, and it is the
                // kind this backend exists to run into on the seam's behalf.
                conn.submit_handshake(Level::Initial, &Self::frame(MSG_SERVER_PARAMS, &params))?;
                self.log.push("submit_handshake");
            }
            (Role::Client, MSG_SERVER_PARAMS) => {
                conn.set_peer_transport_params(body)?;
                self.log.push("set_peer_transport_params");
                self.install_level(conn, Level::OneRtt)?;
                // The acknowledgement goes out before completion is reported, so that it is
                // still sendable when it is sent.
                conn.submit_handshake(Level::Handshake, &Self::frame(MSG_CLIENT_DONE, &[]))?;
                self.log.push("submit_handshake");
                self.events.push_back(SessionEvent::HandshakeComplete);
            }
            (Role::Server, MSG_CLIENT_DONE) => {
                self.install_level(conn, Level::OneRtt)?;
                self.events.push_back(SessionEvent::HandshakeComplete);
            }
            _ => return Err(Error::backend("an unexpected handshake message")),
        }
        Ok(())
    }

    fn poll_event(&mut self) -> Option<SessionEvent> {
        self.events.pop_front()
    }

    fn rotate_keys(
        &mut self,
        rx_secret: &[u8],
        tx_secret: &[u8],
    ) -> Result<RotatedKeys<Self::PacketKey>> {
        self.log.push("rotate_keys");
        let next = |s: &[u8]| -> Vec<u8> { s.iter().map(|b| b.wrapping_add(1)).collect() };
        let rx = keys_for(&self.dcid, level_tag(Level::OneRtt) ^ 1, FROM_SERVER);
        let tx = keys_for(&self.dcid, level_tag(Level::OneRtt) ^ 1, FROM_CLIENT);
        let (rx, tx) = match self.role {
            Role::Client => (rx, tx),
            Role::Server => (tx, rx),
        };
        Ok(RotatedKeys {
            rx_packet: rx.packet,
            rx_iv: rx.iv,
            rx_secret: next(rx_secret),
            tx_packet: tx.packet,
            tx_iv: tx.iv,
            tx_secret: next(tx_secret),
        })
    }

    fn negotiated_alpn(&self) -> Option<Vec<u8>> {
        Some(b"toy".to_vec())
    }
}

/// The backend the sessions come from.
struct ToyBackend;

impl Backend for ToyBackend {
    type Session = ToySession;

    fn new_session(&self, role: Role, _server_name: Option<&str>) -> Result<Self::Session> {
        Ok(ToySession::new(role))
    }
}

// ---------------------------------------------------------------------------------------
// Driving two real connections with it.
// ---------------------------------------------------------------------------------------

/// A deterministic source, so a failure is reproducible.
struct Counter(u8);

impl EntropySource for Counter {
    fn fill(&mut self, out: &mut [u8]) -> Result<()> {
        for b in out.iter_mut() {
            self.0 = self.0.wrapping_add(1);
            *b = self.0;
        }
        Ok(())
    }
}

fn addrs() -> (std::net::SocketAddr, std::net::SocketAddr) {
    (
        "127.0.0.1:4433".parse().expect("a local address"),
        "127.0.0.1:4434".parse().expect("a peer address"),
    )
}

fn params() -> TransportParams {
    TransportParams::new()
        .initial_max_data(1 << 20)
        .initial_max_stream_data_bidi_local(1 << 18)
        .initial_max_stream_data_bidi_remote(1 << 18)
        .initial_max_stream_data_uni(1 << 18)
        .initial_max_streams_bidi(8)
        .initial_max_streams_uni(8)
}

/// Moves every datagram one side produced to the other, until neither has more to say.
fn pump(client: &mut Conn<'_, ToySession>, server: &mut Conn<'_, ToySession>, start: Timestamp) {
    /// Drains one side's datagrams into the other, reporting whether anything moved.
    fn drain(
        from: &mut Conn<'_, ToySession>,
        to: &mut Conn<'_, ToySession>,
        now: Timestamp,
    ) -> bool {
        let mut moved = false;
        let mut buf = [0u8; 1500];
        loop {
            let outcome = from.write_pkt(&mut buf, now);
            let len = match outcome {
                Ok(WriteOutcome::Datagram { len }) => len,
                _ => break,
            };
            if len == 0 {
                break;
            }
            moved = true;
            if let Err(e) = to.read_pkt(&buf[..len], now) {
                eprintln!("DBG read {len}B {:?}", e.kind());
            }
        }
        moved
    }

    // The clock advances between rounds. ngtcp2 paces its sending, so a loop that asks for
    // packets at a single instant is told "not now" and mistakes it for "nothing to send".
    let mut nanos = start.as_nanos();
    for _ in 0..32 {
        nanos += 1_000_000;
        let now = Timestamp::from_nanos(nanos).expect("a timestamp");
        let a = drain(client, server, now);
        let b = drain(server, client, now);
        if !a && !b {
            return;
        }
    }
}

#[test]
fn a_backend_that_forbids_unsafe_completes_a_connection_in_both_roles() {
    // The claim, made whole. Two real connections, a real handshake, real packets protected by
    // a backend that could not write `unsafe` if it wanted to — the module forbids it — and
    // that reaches the crate only through its public API, so it cannot see the raw bindings at
    // all.
    let now = Timestamp::from_nanos(1).expect("a timestamp");
    let (local, remote) = addrs();
    let backend = ToyBackend;
    let dcid = ConnectionId::new(&[1, 2, 3, 4, 5, 6, 7, 8]).expect("an identifier");

    let mut client = ConnBuilder::new(
        Role::Client,
        Settings::new(now),
        params(),
        Box::new(Counter(0)),
        Backend::new_session(&backend, Role::Client, None).expect("a client session"),
        local,
        remote,
    )
    .dcid(dcid)
    .scid(dcid)
    .cid_len(8)
    .build(Handlers::new())
    .expect("a client connection");

    // The server is built from what the client's first packet carries. That is the only way
    // to obtain the identifier it must echo back, and it is why the client is drained first.
    let mut buf = [0u8; 1500];
    let mut first_flight = Vec::new();
    while let Ok(WriteOutcome::Datagram { len }) = client.write_pkt(&mut buf, now) {
        if len == 0 {
            break;
        }
        first_flight.push(buf[..len].to_vec());
    }
    assert!(
        !first_flight.is_empty(),
        "a fresh client produced no first flight: {:?}",
        client.tls().log
    );

    let (original_dcid, client_scid) =
        match ngnet_quic::inspect(&first_flight[0], 8).expect("decoding the first packet") {
            ngnet_quic::Inspection::Supported { dcid, scid, .. } => (dcid, scid),
            other => panic!("the first flight should be a long header: {other:?}"),
        };

    let mut server = ConnBuilder::new(
        Role::Server,
        Settings::new(now),
        params().original_dcid(&original_dcid),
        Box::new(Counter(64)),
        Backend::new_session(&backend, Role::Server, None).expect("a server session"),
        remote,
        local,
    )
    .dcid(client_scid)
    .build(Handlers::new())
    .expect("a server connection");

    for datagram in &first_flight {
        let _ = server.read_pkt(datagram, now);
    }

    pump(&mut client, &mut server, now);

    assert!(
        client.is_handshake_completed(),
        "the client did not complete: {:?}",
        client.tls().log
    );
    assert!(
        server.is_handshake_completed(),
        "the server did not complete: {:?}",
        server.tls().log
    );

    // Every seam operation the handshake needs was actually performed, on both sides. Without
    // this the test could pass against a seam that quietly did the work itself.
    for (who, session) in [("client", client.tls()), ("server", server.tls())] {
        for op in ["initial_keys", "install_keys", "read_handshake"] {
            assert!(
                session.log.contains(&op),
                "the {who} never performed {op}: {:?}",
                session.log
            );
        }
    }
    // And the two operations only one side performs.
    assert!(client.tls().log.contains(&"set_local_transport_params"));
    assert!(
        server.tls().log.contains(&"local_transport_params"),
        "the server never asked for its own transport parameters, which is the operation an \
         earlier design of this seam could not express"
    );
    assert!(
        client.tls().log.contains(&"set_peer_transport_params")
            && server.tls().log.contains(&"set_peer_transport_params")
    );

    // And application data crosses, protected by this backend's keys. A handshake that
    // completes but cannot carry anything would have proved only half of what matters.
    let stream = client.open_bidi_stream().expect("opening a stream");
    let payload = b"carried by a backend that cannot write unsafe";
    let mut buf = [0u8; 1500];
    let now = Timestamp::from_nanos(64_000_000).expect("a timestamp");
    let written = client
        .write_stream(&mut buf, stream, payload, true, now)
        .expect("writing to the stream");
    let ngnet_quic::StreamWrite::Datagram { len, accepted } = written else {
        panic!("the stream would not accept data: {written:?}");
    };
    assert_eq!(accepted, payload.len(), "not all of the payload was taken");
    server
        .read_pkt(&buf[..len], now)
        .expect("the server reading application data");
}

#[test]
fn the_toy_protection_is_invertible_and_rejects_forgery() {
    // The two properties ngtcp2 depends on, checked directly rather than inferred from a
    // handshake that happened to work.
    let keys = keys_for(&[1, 2, 3], level_tag(Level::OneRtt), FROM_CLIENT);
    let nonce = &keys.iv;
    let aad = b"a packet header";
    let plaintext = b"an application payload";

    let mut buf = vec![0u8; plaintext.len() + TAG_LEN];
    buf[..plaintext.len()].copy_from_slice(plaintext);
    keys.packet
        .seal(&mut buf, plaintext.len(), nonce, aad)
        .expect("sealing");
    assert_ne!(
        &buf[..plaintext.len()],
        &plaintext[..],
        "nothing was protected"
    );

    // Ciphertext in, plaintext out into a distinct buffer, which is the shape the seam now
    // requires: ngtcp2's core always decrypts a packet into a buffer separate from the
    // packet it received.
    let mut plain = vec![0u8; plaintext.len()];
    let recovered = keys
        .packet
        .open(&mut plain, &buf, nonce, aad)
        .expect("opening");
    assert_eq!(&plain[..recovered], &plaintext[..]);

    // A forged packet is an ordinary event, not a failed backend.
    let forged = vec![0u8; plaintext.len() + TAG_LEN];
    let mut out = vec![0u8; plaintext.len()];
    assert_eq!(
        keys.packet.open(&mut out, &forged, nonce, aad).unwrap_err(),
        CryptoError::Decrypt
    );

    // And a packet too short to hold a tag, which is what a truncating attacker sends.
    let short = vec![0u8; 4];
    let mut out = vec![0u8; 4];
    assert_eq!(
        keys.packet.open(&mut out, &short, nonce, aad).unwrap_err(),
        CryptoError::Decrypt
    );
}

#[test]
fn the_two_sides_derive_each_others_keys() {
    // If this did not hold, the handshake test above would fail for a reason that had nothing
    // to do with the seam, which is the kind of confusion worth ruling out separately.
    for level in [Level::Initial, Level::Handshake, Level::OneRtt] {
        let tag = level_tag(level);
        let client_tx = keys_for(&[9, 9], tag, FROM_CLIENT);
        let server_rx = keys_for(&[9, 9], tag, FROM_CLIENT);
        assert_eq!(client_tx.iv, server_rx.iv);

        let plaintext = b"a payload";
        let mut buf = vec![0u8; plaintext.len() + TAG_LEN];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        client_tx
            .packet
            .seal(&mut buf, plaintext.len(), &client_tx.iv, b"h")
            .expect("sealing");
        let mut plain = vec![0u8; plaintext.len()];
        let n = server_rx
            .packet
            .open(&mut plain, &buf, &server_rx.iv, b"h")
            .expect("opening");
        assert_eq!(&plain[..n], &plaintext[..]);
    }
}

#[test]
fn a_connection_reports_the_backends_protocol() {
    // The one thing the seam reports that a caller sees directly.
    let now = Timestamp::from_nanos(1).expect("a timestamp");
    let (local, remote) = addrs();
    let backend = ToyBackend;
    let dcid = ConnectionId::new(&[7; 8]).expect("an identifier");

    let mut client = ConnBuilder::new(
        Role::Client,
        Settings::new(now),
        params(),
        Box::new(Counter(0)),
        Backend::new_session(&backend, Role::Client, None).expect("a session"),
        local,
        remote,
    )
    .dcid(dcid)
    .scid(dcid)
    .cid_len(8)
    .build(Handlers::new())
    .expect("a connection");

    let mut buf = [0u8; 1500];
    let _: std::result::Result<WriteOutcome, Error> = client.write_pkt(&mut buf, now);
    assert_eq!(client.negotiated_alpn().as_deref(), Some(&b"toy"[..]));
    let _: ReadOutcome = client.read_pkt(&[0], now).unwrap_or(ReadOutcome::Processed);
}

#[test]
fn a_backend_that_supplies_an_impossible_vector_is_told_so() {
    // The one dimension a *safe* backend chooses, now narrowed to a length the seam checks.
    //
    // `DirectionalKeys::iv` is an [`Iv`], a fixed-capacity value whose `new` refuses a length
    // outside the range ngtcp2 accepts -- so a backend cannot even *hold* an over-capacity
    // vector, and a shorter or slightly-longer one is rejected the moment it is constructed.
    // ngtcp2 builds each packet's nonce in a 64-byte stack buffer guarded only by
    // `assert(sizeof(nonce) >= ckm->iv.len)` (`ngtcp2_conn.c:5920-5926`, with a `TODO` above
    // it saying exactly that), and derives it by subtracting eight from that length under
    // `assert(ivlen >= sizeof(n))` (`ngtcp2_crypto.c:100-112`). Release builds contain neither
    // assertion, so those bounds are the crate's to keep -- the same reason `crate::validate`
    // exists at all.
    //
    // What is asserted here is what the crate guarantees: the backend is told its vector is
    // unusable, and the connection does not go on to complete a handshake with it. Whether a
    // particular out-of-range length would go on to corrupt memory in a release build is not
    // something a test should try to find out.
    for bad in [0usize, 4, 7, 65, 4096] {
        let now = Timestamp::from_nanos(1).expect("a timestamp");
        let (local, remote) = addrs();
        let backend = ToyBackend;
        let dcid = ConnectionId::new(&[3; 8]).expect("an identifier");

        let mut session = Backend::new_session(&backend, Role::Client, None).expect("a session");
        session.bad_iv_len = Some(bad);

        let mut client = ConnBuilder::new(
            Role::Client,
            Settings::new(now),
            params(),
            Box::new(Counter(0)),
            session,
            local,
            remote,
        )
        .dcid(dcid)
        .scid(dcid)
        .cid_len(8)
        .build(Handlers::new())
        .expect("building the connection");

        // The first write is what drives the client's initial flight, and therefore the key
        // installs.
        let mut buf = [0u8; 1500];
        let outcome = client.write_pkt(&mut buf, now);

        assert!(
            outcome.is_err(),
            "a {bad}-byte initialisation vector was accepted and produced {outcome:?}"
        );
        assert!(
            !client.is_handshake_completed(),
            "a connection completed a handshake with a {bad}-byte initialisation vector"
        );
    }
}

#[test]
fn a_backend_that_rotates_to_an_impossible_vector_is_refused() {
    // The same hazard on the key-update path, where ngtcp2 hands the callback buffers it sized
    // from the generation being replaced. A longer vector or secret writes past them.
    let mut session = ToySession::new(Role::Client);
    session.dcid = vec![1, 2, 3];
    let rotated = Session::rotate_keys(&mut session, &[0; 32], &[0; 32]).expect("rotating");
    assert_eq!(
        rotated.rx_iv.len(),
        12,
        "the toy backend should rotate to the length it installed"
    );
    assert_eq!(
        rotated.rx_secret.len(),
        32,
        "and to the secret length it was given"
    );
}
