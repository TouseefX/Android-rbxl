//! The shape of the public API, pinned.
//!
//! Nothing here asserts behaviour. The technique is that **compiling is the assertion**:
//! every public item is named and used in a way that fixes its signature, so a change to
//! the surface breaks this file rather than breaking a downstream crate silently.
//!
//! Adding public API means extending this file, or the test fails to be a complete
//! description of what was promised.
//!
//! Whether an enumeration is matched exhaustively or with a wildcard is itself part of the
//! promise. A closed enum is one where a new variant is a change every caller must notice;
//! an open one is where adding a variant must not break anyone. Which is which is a
//! deliberate decision, recorded here by how it is matched.

use std::io::IoSlice;

use ngnet_quic::{
    ApplicationErrorCode, Backend, ConnBuilder, ConnectionId, CryptoError, Direction,
    DirectionalKeys, Directionality, Duration, EntropySource, Error, ErrorKind, ExpiryOutcome,
    HP_MASK_LEN, HP_SAMPLE_LEN, Handlers, Handshaking, HeaderKey, InitialKeys, Initiator,
    Inspection, Iv, Level, NativeCode, OwnedBytes, OwnedWrite, PacketKey, ReadOutcome, Result,
    Role, RotatedKeys, Session, SessionEvent, Settings, StreamCloseReason, StreamId, StreamWrite,
    Timestamp, TransportErrorCode, TransportParams, WriteOutcome,
};

#[test]
fn the_public_surface_still_has_the_shape_it_promised() {
    // --- Time -----------------------------------------------------------------------
    let ts: Result<Timestamp> = Timestamp::from_nanos(1);
    let ts = ts.expect("a valid timestamp");
    let _: u64 = ts.as_nanos();
    let d = Duration::from_nanos(1);
    let _: u64 = d.as_nanos();
    let _: Result<Duration> = Duration::from_millis(1);

    // --- Errors ---------------------------------------------------------------------
    let native = NativeCode::new(-1);
    let _: i32 = native.get();
    let _: bool = native.is_fatal();
    let _: &'static str = native.describe();

    let transport = TransportErrorCode::new(0);
    let _: u64 = transport.get();
    let _: TransportErrorCode = TransportErrorCode::infer(native);
    let _: TransportErrorCode = TransportErrorCode::NO_ERROR;

    let app = ApplicationErrorCode::new(0);
    let _: u64 = app.get();

    fn takes_error(e: &Error) {
        let _: ErrorKind = e.kind();
        let _: Option<NativeCode> = e.native_code();
        let _: bool = e.is_fatal();
        let _: Option<TransportErrorCode> = e.transport_error_code();
        // `Error` is an ordinary std error.
        let _: &dyn std::error::Error = e;
    }
    let _ = takes_error;

    // `ErrorKind` is **open**: ngtcp2 may grow conditions this crate has to classify, and
    // adding a variant must not break a caller. Matched with a wildcard, deliberately.
    fn classify(kind: ErrorKind) -> &'static str {
        match kind {
            ErrorKind::Protocol => "protocol",
            ErrorKind::Exhausted => "exhausted",
            ErrorKind::InvalidInput => "invalid",
            ErrorKind::Crypto => "crypto",
            ErrorKind::ConnectionUnusable => "unusable",
            ErrorKind::Closing => "closing",
            ErrorKind::Blocked => "blocked",
            ErrorKind::StreamClosed => "stream-closed",
            ErrorKind::Internal => "internal",
            _ => "unknown",
        }
    }
    let _ = classify;

    // --- Connection identifiers -----------------------------------------------------
    let cid: Result<ConnectionId> = ConnectionId::new(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let cid = cid.expect("a valid identifier");
    let _: &[u8] = cid.as_bytes();
    let _: usize = ngnet_quic::MAX_CID_LEN;
    let _: usize = ngnet_quic::MIN_CID_LEN;
    let _: usize = ngnet_quic::DEFAULT_CID_LEN;

    // --- Streams --------------------------------------------------------------------
    let stream: Result<StreamId> = StreamId::new(0);
    let stream = stream.expect("a valid identifier");
    let _: i64 = stream.get();
    let _: i64 = StreamId::MAX;
    let _: bool = stream.is_writable_by(true);

    // Both **closed**: QUIC defines exactly two initiators and two directionalities, and a
    // third of either would be a new protocol.
    match stream.initiator() {
        Initiator::Client | Initiator::Server => {}
    }
    match stream.directionality() {
        Directionality::Bidirectional | Directionality::Unidirectional => {}
    }

    // --- Packet outcomes ------------------------------------------------------------
    // **Closed.** Each variant demands a different response, so a new one is a change every
    // caller must be forced to notice.
    fn on_read(outcome: ReadOutcome) {
        match outcome {
            ReadOutcome::Processed
            | ReadOutcome::SendRetry
            | ReadOutcome::DropSilently
            | ReadOutcome::Draining
            | ReadOutcome::Closing => {}
        }
    }
    let _ = on_read;

    // **Closed**, and the most important one: conflating `Idle` with `Blocked` is the
    // classic stall bug, so a caller must handle each explicitly.
    fn on_write(outcome: WriteOutcome) {
        match outcome {
            WriteOutcome::Datagram { len: _ } | WriteOutcome::Idle | WriteOutcome::Blocked => {}
        }
    }
    let _ = on_write;

    // **Closed**, for the same reason: `IdleClose` must not be answered with a packet.
    fn on_expiry(outcome: ExpiryOutcome) {
        match outcome {
            ExpiryOutcome::Handled | ExpiryOutcome::IdleClose | ExpiryOutcome::Terminal => {}
        }
    }
    let _ = on_expiry;

    // **Closed.**
    fn on_stream_write(outcome: StreamWrite) {
        match outcome {
            StreamWrite::Datagram {
                len: _,
                accepted: _,
            }
            | StreamWrite::StreamBlocked
            | StreamWrite::ConnectionBlocked
            | StreamWrite::Blocked
            | StreamWrite::Idle => {}
        }
    }
    let _ = on_stream_write;

    // **A stream that will take no more.** Distinct from invalid input: one is a normal end
    // to recognise, the other is a bug to fix.
    let _: ErrorKind = ErrorKind::StreamClosed;

    // **Two directions, independently.** QUIC shuts a stream's halves separately, so this
    // carries a code for each and either may be absent. A struct rather than an enum
    // because the two are genuinely independent: any combination of present and absent is
    // meaningful, which an enum would have to enumerate.
    fn on_limits(handlers: Handlers<'static>) -> Handlers<'static> {
        handlers
            .on_extend_max_local_streams_bidi(|_max: u64| {})
            .on_extend_max_local_streams_uni(|_max: u64| {})
    }
    let _ = on_limits;

    fn on_close(reason: StreamCloseReason) {
        let _: Option<ApplicationErrorCode> = reason.receiving();
        let _: Option<ApplicationErrorCode> = reason.sending();
        let _: bool = reason.is_clean();
        let _ = StreamCloseReason::new(None, Some(ApplicationErrorCode::new(0x10c)));
    }
    let _ = on_close;

    // --- Pre-connection inspection --------------------------------------------------
    let _: u32 = ngnet_quic::VERSION_V1;
    let _: &'static [u32] = ngnet_quic::supported_versions();

    // Named through function pointers rather than called. This file pins *shapes*; calling
    // these with placeholder arguments would be exercising behaviour, and a degenerate
    // input can trip an assertion inside ngtcp2 -- which is a job for the behavioural tests,
    // not for a signature check.
    let _: fn(&[u8]) -> bool = ngnet_quic::is_acceptable_initial;
    let _: fn(&[u8], usize) -> Result<Inspection> = ngnet_quic::inspect;
    let _: fn(&[u8]) -> Result<Option<ngnet_quic::InitialPacket>> = ngnet_quic::inspect_initial;
    // Named through an alias, which keeps the signature readable and keeps clippy from
    // objecting to a five-argument function pointer written inline.
    type WriteVersionNegotiation =
        fn(&mut [u8], u8, &ConnectionId, &ConnectionId, &[u32]) -> Result<usize>;
    let _: WriteVersionNegotiation = ngnet_quic::write_version_negotiation;

    // **Closed.** Each case leads somewhere different: build a connection, answer with a
    // Version Negotiation packet, or route to an existing connection.
    fn on_inspection(inspection: Inspection) {
        match inspection {
            Inspection::Supported {
                version: _,
                dcid: _,
                scid: _,
            }
            | Inspection::UnsupportedVersion {
                version: _,
                dcid: _,
                scid: _,
            }
            | Inspection::ShortHeader { dcid: _ } => {}
        }
    }
    let _ = on_inspection;

    // **Open.** A future QUIC version could define another token kind, and a caller that
    // matched exhaustively would stop compiling for a change that concerns only servers
    // doing address validation.
    fn on_initial_token(token: &ngnet_quic::InitialToken) -> &[u8] {
        match token {
            ngnet_quic::InitialToken::Absent => &[],
            ngnet_quic::InitialToken::Retry(bytes)
            | ngnet_quic::InitialToken::Regular(bytes)
            | ngnet_quic::InitialToken::Unknown(bytes) => bytes,
            _ => &[],
        }
    }
    let _ = on_initial_token;

    // **Open**, for the same reason: `InitialPacket` gains fields as more of a first packet
    // becomes useful to a server, and each is additive.
    fn on_initial_packet(packet: &ngnet_quic::InitialPacket) {
        let _: u32 = packet.version;
        let _: &ConnectionId = &packet.dcid;
        let _: &ConnectionId = &packet.scid;
        let _: &ngnet_quic::InitialToken = &packet.token;
    }
    let _ = on_initial_packet;

    // **Open.** A close reason is a classification of something the peer did, and QUIC can
    // grow new ones; a caller matching exhaustively would break on a variant it has no
    // opinion about.
    fn on_close_error(err: &ngnet_quic::CloseError) {
        let _: u64 = err.frame_type();
        let _: &[u8] = err.phrase();
        match err.reason() {
            ngnet_quic::CloseReason::Transport(_) => {}
            ngnet_quic::CloseReason::Application(_) => {}
            ngnet_quic::CloseReason::VersionNegotiation => {}
            ngnet_quic::CloseReason::IdleTimeout => {}
            ngnet_quic::CloseReason::Dropped => {}
            ngnet_quic::CloseReason::Retry => {}
            _ => {}
        }
    }
    let _ = on_close_error;

    // **Open.** ngtcp2 already names three token types and could name a fourth.
    fn on_token_kind(kind: ngnet_quic::TokenKind) {
        match kind {
            ngnet_quic::TokenKind::Retry => {}
            ngnet_quic::TokenKind::NewToken => {}
            _ => {}
        }
    }
    let _ = on_token_kind;

    // --- Configuration --------------------------------------------------------------
    let _settings = Settings::new(ts)
        .initial_rtt(d)
        .max_tx_udp_payload_size(1200)
        .max_window(1)
        .max_stream_window(1)
        .handshake_timeout(d)
        .initial_pkt_num(0)
        .no_pmtud(false)
        .validated_token(b"token", ngnet_quic::TokenKind::Retry);

    let _params = TransportParams::new()
        .initial_max_stream_data_bidi_local(1)
        .initial_max_stream_data_bidi_remote(1)
        .initial_max_stream_data_uni(1)
        .initial_max_data(1)
        .initial_max_streams_bidi(1)
        .initial_max_streams_uni(1)
        .max_idle_timeout(d)
        .active_connection_id_limit(2)
        .max_ack_delay(d)
        .original_dcid(&cid);

    let _: u64 = ngnet_quic::DEFAULT_STREAM_DATA;
    let _: u64 = ngnet_quic::DEFAULT_CONNECTION_DATA;
    let _: u64 = ngnet_quic::DEFAULT_MAX_STREAMS;
    let _: Duration = ngnet_quic::DEFAULT_IDLE_TIMEOUT;

    // --- Handlers -------------------------------------------------------------------
    let _handlers = Handlers::new()
        .on_stream_data(|_id, _bytes, _fin| {})
        .on_stream_open(|_id| {})
        .on_stream_close(|_id, _reason| {})
        .on_stream_reset(|_id, _code| {})
        .on_stop_sending(|_id, _code| {})
        .on_acked_stream_data(|_id, _len| {})
        .on_handshake_completed(|| {})
        .on_new_connection_id(|_cid| {})
        .on_remove_connection_id(|_cid| {});

    // --- Entropy --------------------------------------------------------------------
    struct Source;
    impl EntropySource for Source {
        fn fill(&mut self, _dest: &mut [u8]) -> Result<()> {
            Ok(())
        }
    }
    let _: Box<dyn EntropySource + Send> = Box::new(Source);

    // --- TLS seam -------------------------------------------------------------------
    // **Closed.** There are two roles in a handshake and there will not be a third.
    match Role::Client {
        Role::Client | Role::Server => {}
    }
    let _: bool = Role::Server.is_server();

    // --- The safe TLS seam ----------------------------------------------------------
    // Named by bound rather than by implementation, because the property being pinned is
    // what an implementor has to supply. Note that nothing here mentions a pointer or a
    // `sys::` type: that absence is the whole point of the seam, and Phase 7 turns it into
    // a check that reads the source rather than relying on this file's good behaviour.
    match Level::Initial {
        Level::Initial | Level::ZeroRtt | Level::Handshake | Level::OneRtt => {}
    }
    match Direction::Read {
        Direction::Read | Direction::Write => {}
    }
    // **Closed**, and deliberately two-valued. A third variant would blur the one
    // distinction the type exists to make.
    match CryptoError::Decrypt {
        CryptoError::Decrypt | CryptoError::Fatal => {}
    }
    let _: usize = HP_MASK_LEN;
    let _: usize = HP_SAMPLE_LEN;

    fn takes_packet_key<K: PacketKey>(k: &K, buf: &mut [u8]) {
        let _: core::result::Result<(), CryptoError> = k.seal(buf, 0, &[], &[]);
        let _: core::result::Result<usize, CryptoError> = k.open(buf, &[], &[], &[]);
        let _: usize = k.tag_len();
        let _: u64 = k.confidentiality_limit();
        let _: u64 = k.integrity_limit();
    }

    fn takes_header_key<K: HeaderKey>(k: &K) {
        let _: core::result::Result<[u8; HP_MASK_LEN], CryptoError> = k.mask(&[]);
    }

    fn takes_safe_session<S: Session>(
        s: &mut S,
        conn: &mut dyn Handshaking<S::PacketKey, S::HeaderKey>,
    ) {
        let _: Result<InitialKeys<S::PacketKey, S::HeaderKey>> = s.initial_keys(1, &[]);
        let _: Result<S::PacketKey> = s.retry_key(1);
        let _: Result<()> = s.set_local_transport_params(&[]);
        let _: Result<()> = s.start_handshake(conn);
        let _: Result<()> = s.read_handshake(Level::Initial, &[], conn);
        let _: Option<SessionEvent> = s.poll_event();
        let _: Result<RotatedKeys<S::PacketKey>> = s.rotate_keys(&[], &[]);
        let _: Option<Vec<u8>> = s.negotiated_alpn();
        let _: Option<String> = s.failure_reason();
    }

    fn takes_safe_backend<B: Backend>(b: &B) -> Result<B::Session> {
        b.new_session(Role::Client, Some("example.com"))
    }

    // A session's keys must be usable from wherever ngtcp2 hands them back, which is why
    // the seam bounds them `Send + 'static` rather than leaving it to each backend.
    fn keys_are_send_and_static<S: Session>() {
        fn require<T: Send + 'static>() {}
        require::<S::PacketKey>();
        require::<S::HeaderKey>();
    }

    /// The queue carries exactly two things, and neither of them is key material.
    ///
    /// Pinned because the boundary between "reported afterwards" and "performed immediately"
    /// is the load-bearing distinction in this seam: anything that drifts back onto the queue
    /// is something that will be applied too late to matter.
    fn events_are_only_what_can_wait(event: SessionEvent) {
        match event {
            SessionEvent::HandshakeComplete => {}
            SessionEvent::Alert(code) => {
                let _: u8 = code;
            }
        }
    }

    /// The capability, and every operation on it.
    fn connection_offers_exactly_four_operations<P: PacketKey, H: HeaderKey>(
        conn: &mut dyn Handshaking<P, H>,
        keys: DirectionalKeys<P, H>,
    ) {
        let _: Result<()> = conn.set_peer_transport_params(&[]);
        let _: Result<Vec<u8>> = conn.local_transport_params();
        let _: Result<()> = conn.install_keys(Level::Initial, Direction::Read, keys, &[]);
        let _: Result<()> = conn.submit_handshake(Level::Initial, &[]);
    }

    /// A level's initialisation vector is a fixed-capacity value, not a `Vec<u8>`: a length
    /// ngtcp2 cannot handle has no representation, and construction is fallible for the rest.
    /// Read through a `Deref` to the bytes, so the array behind it stays private.
    fn iv_is_a_fixed_capacity_value<P: PacketKey, H: HeaderKey>(
        keys: DirectionalKeys<P, H>,
        rotated: RotatedKeys<P>,
    ) {
        let _: Result<Iv> = Iv::new(&[]);
        let iv: Iv = keys.iv;
        let _: &[u8] = &iv;
        let _: &[u8] = iv.as_slice();
        let _: Iv = rotated.rx_iv;
        let _: Iv = rotated.tx_iv;
    }
    let _ = events_are_only_what_can_wait;

    let _ = takes_packet_key::<DummyPacketKey>;
    let _ = takes_header_key::<DummyHeaderKey>;
    let _ = takes_safe_session::<DummySafeSession>;
    let _ = connection_offers_exactly_four_operations::<DummyPacketKey, DummyHeaderKey>;
    let _ = iv_is_a_fixed_capacity_value::<DummyPacketKey, DummyHeaderKey>;
    let _ = takes_safe_backend::<DummySafeBackend>;
    let _ = keys_are_send_and_static::<DummySafeSession>;
    let _ = events_are_only_what_can_wait;
}

/// The initialisation vector is stored inline, not behind a heap pointer.
///
/// The whole point of the type is that a length ngtcp2 cannot handle has nowhere to live, so
/// the storage is asserted rather than trusted. A `Vec<u8>` is three machine words wide
/// whatever it holds; the inline value is at least as wide as the bytes it can hold, which is
/// `validate::MAX_IV_LEN` (64). This assertion holds only for the inline form and would fail
/// the moment the field went back to a `Vec`.
#[test]
fn the_initialisation_vector_is_stored_inline() {
    assert!(core::mem::size_of::<Iv>() >= 64);
}

/// The owned-send handle offers exactly the operations promised.
///
/// `OwnedBytes` is the buffer a caller hands to [`Conn::write_stream_owned`] to be retained
/// without a copy. Its surface is pinned here: a wrapping constructor over anything shaped
/// like bytes, an erasing one over an owner the crate cannot see inside, read access, and the
/// split that hands back an unaccepted suffix as a view into the same allocation.
#[test]
fn the_owned_send_handle_still_has_the_shape_it_promised() {
    let mut bytes: OwnedBytes = OwnedBytes::new(vec![1u8, 2, 3, 4]);
    let _: usize = bytes.len();
    let _: bool = bytes.is_empty();
    let _: &[u8] = bytes.as_slice();
    let suffix: OwnedBytes = bytes.split_to(2);
    let _: OwnedBytes = suffix;

    struct Owned(Vec<u8>);
    impl AsRef<[u8]> for Owned {
        fn as_ref(&self) -> &[u8] {
            &self.0
        }
    }
    // `from_owner` is `unsafe`: the handle it yields backs a raw pointer ngtcp2 keeps, so the
    // owner must lend a slice of stable address and length. `Owned` wraps an immutable `Vec`
    // and satisfies that. The surface being pinned here is that it is an `unsafe fn` taking
    // `impl AsRef<[u8]> + Send + Sync + 'static` and returning `OwnedBytes`.
    // SAFETY: `Owned` never mutates its buffer, so `as_ref` is address- and length-stable.
    let _: OwnedBytes = unsafe { OwnedBytes::from_owner(Owned(vec![0u8])) };

    let _: OwnedBytes = bytes.clone();
}

/// A backend that exists only so the generic bounds above have something to name.
#[test]
fn the_types_a_connection_owns_are_send_because_the_connection_is() {
    fn assert_send<T: Send>() {}

    // The two things a `Conn` actually keeps. `Settings` and `TransportParams` are not
    // asserted because ngtcp2 copies them at construction and the connection does not hold
    // them; they wrap C structs containing raw pointers and are deliberately consumed by
    // value into `build`.
    assert_send::<Handlers<'static>>();
    assert_send::<Box<dyn EntropySource + Send>>();
    assert_send::<ConnectionId>();

    // And the connection itself, for a `Send` session.
    fn conn_is_send<S: Session + Send>() {
        assert_send::<ngnet_quic::Conn<'static, S>>();
    }
    let _ = conn_is_send::<DummySafeSession>;
}

#[test]
fn the_connection_surface_still_has_the_shape_it_promised() {
    // Named through a generic function rather than instantiated, so this pins the signatures
    // without needing a TLS backend or a live connection.
    fn uses_conn<S: Session>(conn: &mut ngnet_quic::Conn<'_, S>, cause: &Error) -> Result<()> {
        let now = Timestamp::from_nanos(1)?;
        let mut buf = [0u8; 1500];

        let _: Role = conn.role();
        let _: &ConnectionId = conn.scid();
        let _: core::net::SocketAddr = conn.local_addr();
        let _: core::net::SocketAddr = conn.remote_addr();
        let _: bool = conn.is_handshake_completed();
        let _: Option<Vec<u8>> = conn.negotiated_alpn();
        let _: &S = conn.tls();

        let _: ReadOutcome = conn.read_pkt(&[0], now)?;
        let _: WriteOutcome = conn.write_pkt(&mut buf, now)?;
        let _: Option<Timestamp> = conn.expiry();
        let _: ExpiryOutcome = conn.handle_expiry(now)?;
        let _: bool = conn.in_closing_period();
        let _: bool = conn.in_draining_period();

        let stream: StreamId = conn.open_bidi_stream()?;
        let _: StreamId = conn.open_uni_stream()?;
        let _: StreamWrite = conn.write_stream(&mut buf, stream, &[0], true, now)?;
        let _: StreamWrite = conn.write_stream_vectored(
            &mut buf,
            stream,
            &[IoSlice::new(&[0]), IoSlice::new(&[1])],
            true,
            now,
        )?;
        let owned = OwnedBytes::new(vec![0u8]);
        let handed: OwnedWrite = conn.write_stream_owned(&mut buf, stream, owned, true, now)?;
        let _: StreamWrite = handed.outcome;
        let _: OwnedBytes = handed.unsent;
        conn.shutdown_stream(stream, ApplicationErrorCode::new(0))?;
        conn.reset_stream(stream, ApplicationErrorCode::new(0))?;
        conn.stop_sending(stream, ApplicationErrorCode::new(0))?;
        conn.extend_max_stream_offset(stream, 1)?;
        conn.extend_max_offset(1);
        conn.extend_max_streams_bidi(1);
        conn.extend_max_streams_uni(1);
        let _: u64 = conn.streams_bidi_left();
        let _: u64 = conn.streams_uni_left();
        let _: usize =
            conn.write_connection_close(&mut buf, ApplicationErrorCode::new(0), b"", now)?;
        let _: usize = conn.write_transport_close(&mut buf, cause, b"", now)?;
        let _: usize = conn.retained_bytes();
        let _: Vec<ConnectionId> = conn.scids();
        let _: usize = conn.max_tx_udp_payload_size();
        let _: ngnet_quic::CloseError = conn.close_error();
        Ok(())
    }
    let _ = uses_conn::<DummySafeSession>;

    fn builds<S: Session>(
        settings: Settings,
        params: TransportParams,
        entropy: Box<dyn EntropySource + Send>,
        tls: S,
        local: core::net::SocketAddr,
        remote: core::net::SocketAddr,
        cid: ConnectionId,
    ) -> Result<ngnet_quic::Conn<'static, S>> {
        ConnBuilder::new(Role::Client, settings, params, entropy, tls, local, remote)
            .dcid(cid)
            .scid(cid)
            .version(ngnet_quic::VERSION_V1)
            .cid_len(8)
            .build(Handlers::new())
    }
    let _ = builds::<DummySafeSession>;
}

#[cfg(feature = "tls-ossl")]
#[test]
fn the_openssl_backend_surface_still_has_the_shape_it_promised() {
    use ngnet_quic::{OsslBackend, OsslBackendBuilder, OsslSession, Verify};

    let _: OsslBackendBuilder = OsslBackend::builder(Role::Client)
        .alpn("h3")
        .alpn(b"hq".to_vec())
        .certificate_chain_pem("")
        .private_key_pem("")
        .trust_anchor_pem("")
        .use_system_trust_store(false)
        .verify(Verify::Peer);

    // **Open.** A future backend may grow another verification mode, and adding one must
    // not break a caller that names the ones it knows.
    fn on_verify(verify: Verify) {
        match verify {
            Verify::Peer => {}
            Verify::RequireClientCertificate => {}
            Verify::DangerouslyAcceptAnyCertificate => {}
            _ => {}
        }
    }
    let _ = on_verify;

    fn takes_ossl_session(s: &OsslSession) -> Option<Vec<u8>> {
        // Named through the trait: the type now implements both seams, and the safe one is
        // what a caller should be reaching for.
        Session::negotiated_alpn(s)
    }
    let _ = takes_ossl_session;
}

/// Stand-ins for the seam.
///
/// Worth noticing what is absent: these implement the **entire** seam — a backend, a session,
/// both key kinds — and contain no `unsafe`, no raw pointer and no reference to the raw
/// bindings. The stand-ins they replaced needed three `unsafe` blocks and an exemption in
/// `invariants.rs` to be allowed to write `unsafe` at all. That difference is the whole of
/// this work, expressed in the smallest possible implementation.
struct DummyPacketKey;

impl PacketKey for DummyPacketKey {
    fn seal(
        &self,
        _buf: &mut [u8],
        _plaintext_len: usize,
        _nonce: &[u8],
        _aad: &[u8],
    ) -> core::result::Result<(), CryptoError> {
        Err(CryptoError::Fatal)
    }

    fn open(
        &self,
        _dest: &mut [u8],
        _ciphertext: &[u8],
        _nonce: &[u8],
        _aad: &[u8],
    ) -> core::result::Result<usize, CryptoError> {
        Err(CryptoError::Decrypt)
    }

    fn tag_len(&self) -> usize {
        16
    }

    fn confidentiality_limit(&self) -> u64 {
        1
    }

    fn integrity_limit(&self) -> u64 {
        1
    }
}

struct DummyHeaderKey;

impl HeaderKey for DummyHeaderKey {
    fn mask(&self, _sample: &[u8]) -> core::result::Result<[u8; HP_MASK_LEN], CryptoError> {
        Ok([0; HP_MASK_LEN])
    }
}

struct DummySafeSession;

impl Session for DummySafeSession {
    type PacketKey = DummyPacketKey;
    type HeaderKey = DummyHeaderKey;

    fn initial_keys(
        &mut self,
        _version: u32,
        _dcid: &[u8],
    ) -> Result<InitialKeys<Self::PacketKey, Self::HeaderKey>> {
        Err(Error::backend("stand-in"))
    }

    fn retry_key(&mut self, _version: u32) -> Result<Self::PacketKey> {
        Err(Error::backend("stand-in"))
    }

    fn set_local_transport_params(&mut self, _params: &[u8]) -> Result<()> {
        Ok(())
    }

    fn start_handshake(
        &mut self,
        _conn: &mut dyn Handshaking<Self::PacketKey, Self::HeaderKey>,
    ) -> Result<()> {
        Ok(())
    }

    fn read_handshake(
        &mut self,
        _level: Level,
        _data: &[u8],
        _conn: &mut dyn Handshaking<Self::PacketKey, Self::HeaderKey>,
    ) -> Result<()> {
        Ok(())
    }

    fn poll_event(&mut self) -> Option<SessionEvent> {
        None
    }

    fn rotate_keys(
        &mut self,
        _rx_secret: &[u8],
        _tx_secret: &[u8],
    ) -> Result<RotatedKeys<Self::PacketKey>> {
        Err(Error::backend("stand-in"))
    }

    fn negotiated_alpn(&self) -> Option<Vec<u8>> {
        None
    }
}

struct DummySafeBackend;

impl Backend for DummySafeBackend {
    type Session = DummySafeSession;

    fn new_session(&self, _role: Role, _server_name: Option<&str>) -> Result<Self::Session> {
        Ok(DummySafeSession)
    }
}
