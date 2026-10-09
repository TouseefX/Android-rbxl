//! The QUIC connection.
//!
//! One `Conn` wraps one `ngtcp2_conn`, plus everything ngtcp2 retains a pointer to and
//! everything the callbacks need to reach. It is the type that turns a pile of C objects
//! with interlocking lifetime rules into something that can be moved, used and dropped like
//! any other Rust value.
//!
//! # What must outlive the connection, and what need not
//!
//! ngtcp2 **copies** its callbacks struct, its settings, its transport parameters, the path
//! addresses and the connection IDs. Those may be temporaries.
//!
//! It **retains pointers** to exactly four things: the `ngtcp2_mem` allocator
//! (`ngtcp2_conn.h:645`, dereferenced during `ngtcp2_conn_del` at `ngtcp2_conn.c:1827`), the
//! `user_data` pointer, `settings.rand_ctx.native_handle`, and `path.user_data` (unused
//! here). Those live in boxes this type owns.
//!
//! Once the TLS handle is installed there is a fifth, and it points at the session this type
//! owns. ngtcp2 treats it as opaque — it stores the pointer and hands it back — so the crypto
//! callbacks recover the session from it and nothing points the other way. That used to be a
//! cycle, with the TLS library holding a reference back to the connection; removing it is most
//! of what the safe TLS seam bought.

// The read/write entry points that use `with_bridge`, `raw` and `path_mut` arrive with the
// packet paths. The connection is built and dropped by the tests below regardless.
#![allow(dead_code)]

use core::ffi::c_void;
use core::net::SocketAddr;

use ngnet_quic_sys as sys;

use crate::accept;
use crate::alloc::Allocator;
use crate::callbacks::{self, Bridge, BridgeGuard, BridgeSlot, RandCtx, RandGuard};
use crate::cid::ConnectionId;
use crate::error::{CloseError, Error, Result};
use crate::handlers::Handlers;
use crate::params::TransportParams;
use crate::path::PathStorage;
use crate::rand::EntropySource;
use crate::retain::Retained;
use crate::settings::Settings;
use crate::tls::{Role, Session};
use crate::validate;

/// Builder for a [`Conn`].
pub struct ConnBuilder<S> {
    role: Role,
    settings: Settings,
    params: TransportParams,
    entropy: Box<dyn EntropySource + Send>,
    tls: S,
    local: SocketAddr,
    remote: SocketAddr,
    dcid: Option<ConnectionId>,
    scid: Option<ConnectionId>,
    version: u32,
    cid_len: usize,
}

impl<S: Session> ConnBuilder<S> {
    /// Starts building a connection.
    pub fn new(
        role: Role,
        settings: Settings,
        params: TransportParams,
        entropy: Box<dyn EntropySource + Send>,
        tls: S,
        local: SocketAddr,
        remote: SocketAddr,
    ) -> Self {
        Self {
            role,
            settings,
            params,
            entropy,
            tls,
            local,
            remote,
            dcid: None,
            scid: None,
            version: accept::VERSION_V1,
            cid_len: crate::cid::DEFAULT_LEN,
        }
    }

    /// Sets the destination connection ID.
    ///
    /// A client generates one at random; a server takes the client's source identifier.
    pub fn dcid(mut self, dcid: ConnectionId) -> Self {
        self.dcid = Some(dcid);
        self
    }

    /// Sets this endpoint's own source connection ID.
    pub fn scid(mut self, scid: ConnectionId) -> Self {
        self.scid = Some(scid);
        self
    }

    /// Sets the QUIC version. Defaults to version 1.
    pub fn version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Sets the length of identifiers this endpoint generates when one is not supplied.
    pub fn cid_len(mut self, len: usize) -> Self {
        self.cid_len = len;
        self
    }

    /// Builds the connection.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] if the configuration is inconsistent — which
    /// includes every check ngtcp2 makes with an assertion, because those vanish in release
    /// builds — or a native error if ngtcp2 refuses.
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    pub fn build<'h>(mut self, handlers: Handlers<'h>) -> Result<Conn<'h, S>> {
        let server = self.role.is_server();
        validate::server_version(server, self.version)?;

        // Identifiers first, because they may need entropy and a failure here should not
        // leave a half-built connection behind.
        //
        // A server must be told the client's source identifier: generating one would name a
        // connection the client has never heard of, and every packet sent would be ignored.
        // ngtcp2 does not catch that -- the connection builds and then silently stalls --
        // so it is refused here.
        let dcid = match self.dcid {
            Some(dcid) => dcid,
            None if server => {
                return Err(Error::invalid_input(
                    "a server must be given the client's source connection ID as its \
                     destination; take it from `Inspection::Supported`",
                ));
            }
            None => ConnectionId::generate(&mut *self.entropy, self.cid_len)?,
        };
        let scid = match self.scid {
            Some(scid) => scid,
            None => ConnectionId::generate(&mut *self.entropy, self.cid_len)?,
        };

        // The token buffer must outlive the constructor call, which is the only place
        // ngtcp2 copies it from. Binding it here rather than dropping it with the builder
        // is what keeps `settings.token` from dangling.
        let (mut settings, _token_storage) = self.settings.build()?;
        let params = self.params.build(server)?;

        // Everything ngtcp2 keeps a pointer to is boxed here, before the constructor runs,
        // so the addresses it records stay valid for the connection's whole life.
        let allocator = Allocator::new();
        let slot = BridgeSlot::new();
        let mut rand_ctx = Box::new(RandCtx {
            source: self.entropy,
            failed: core::cell::Cell::new(false),
        });
        let path = PathStorage::new(self.local, self.remote);

        let rand_handle: *mut RandCtx = &mut *rand_ctx;
        settings.rand_ctx.native_handle = rand_handle.cast::<c_void>();
        // The same source, reachable a second way. `rand` receives only `rand_ctx`;
        // `get_path_challenge_data` receives only `user_data`. Two routes to one source rather
        // than two sources, which is the whole point.
        // SAFETY: both the slot and the context are boxed and owned by the connection, and the
        // connection is destroyed before either.
        unsafe { slot.set_rand(rand_handle) };

        let cbs = Self::callbacks(self.role);

        let mut raw: *mut sys::ngtcp2_conn = core::ptr::null_mut();
        // The `rand` callback fires inside this call, so the entropy source has to be
        // reachable before it rather than after.
        // SAFETY: the context outlives the guard, which is dropped at the end of the block.
        let rc = {
            let _guard = unsafe { RandGuard::install(rand_handle) };
            // SAFETY: every pointer is valid for the call; the four the library retains
            // point into boxes this function is about to hand to the returned `Conn`.
            unsafe {
                if server {
                    crate::ffi::conn_server_new(
                        &mut raw,
                        dcid.as_raw(),
                        scid.as_raw(),
                        path.as_raw(),
                        self.version,
                        &cbs,
                        &settings,
                        &params,
                        allocator.as_mem_ptr(),
                        slot.as_ptr(),
                    )
                } else {
                    crate::ffi::conn_client_new(
                        &mut raw,
                        dcid.as_raw(),
                        scid.as_raw(),
                        path.as_raw(),
                        self.version,
                        &cbs,
                        &settings,
                        &params,
                        allocator.as_mem_ptr(),
                        slot.as_ptr(),
                    )
                }
            }
        };
        if rc != 0 {
            return Err(Error::native(rc, "could not create the connection"));
        }
        debug_assert!(!raw.is_null());

        // The `rand` callback fires inside the constructor and returns `void`, so a failing
        // entropy source cannot be reported where it happens -- and ngtcp2 uses the buffer
        // regardless, seeding a hash map and a PRNG from it. The failure is latched instead,
        // and checked here, so a connection whose randomness was not what was asked for is
        // never handed back.
        if rand_ctx.failed() {
            // SAFETY: `raw` came from the constructor above and has not been freed.
            unsafe { sys::ngtcp2_conn_del(raw) };
            return Err(Error::with_kind(
                crate::ErrorKind::Internal,
                "the entropy source failed while the connection was being created",
            ));
        }

        let mut conn = Conn {
            raw,
            tls: Box::new(crate::tls_bridge::SessionSlot {
                session: self.tls,
                exchange: crate::tls_bridge::Exchange::default(),
            }),
            handlers,
            _allocator: allocator,
            slot,
            _rand_ctx: rand_ctx,
            path,
            role: self.role,
            scid,
            retained: Retained::default(),
        };

        // The TLS handle is installed after construction, which is also when the cycle
        // between the connection and the TLS session is formed.
        conn.bind_tls()?;
        Ok(conn)
    }

    /// Builds the callback table: the crypto half from the TLS backend, the transport half
    /// here.
    fn callbacks(role: Role) -> sys::ngtcp2_callbacks {
        // SAFETY: a zeroed callbacks struct is the documented starting point; every
        // mandatory entry is filled below or by the backend.
        let mut cbs = unsafe { core::mem::zeroed::<sys::ngtcp2_callbacks>() };

        // The crypto half, written once and generically rather than once per backend. Nothing
        // in it belongs to OpenSSL or to any other stack: it is the same translation for every
        // implementation of `crate::tls::Session`.
        crate::tls_bridge::install::<S>(&mut cbs);

        // The transport half. The mandatory set is taken from the runtime assert block at
        // `ngtcp2_conn.c:1272-1286`, not from the header prose, whose "added since
        // NGTCP2_CALLBACKS_V*" comments are off by one against the length table in
        // `ngtcp2_callbacks.c`.
        cbs.rand = Some(callbacks::rand_cb);
        // Not the TLS backend's, deliberately. A connection has one source of randomness,
        // because two could diverge and only one of them would be the configured one.
        cbs.get_path_challenge_data2 = Some(callbacks::get_path_challenge_data2_cb);
        cbs.get_new_connection_id = Some(callbacks::get_new_connection_id_cb);
        cbs.remove_connection_id = Some(callbacks::remove_connection_id_cb);

        // Optional, but these are the events an application acts on.
        cbs.recv_stream_data = Some(callbacks::recv_stream_data_cb);
        cbs.stream_open = Some(callbacks::stream_open_cb);
        cbs.stream_close2 = Some(callbacks::stream_close2_cb);
        cbs.stream_reset = Some(callbacks::stream_reset_cb);
        cbs.recv_stop_sending = Some(callbacks::recv_stop_sending_cb);
        cbs.acked_stream_data_offset = Some(callbacks::acked_stream_data_offset_cb);
        cbs.handshake_completed = Some(callbacks::handshake_completed_cb);
        cbs.recv_datagram = Some(callbacks::recv_datagram_cb);
        // The peer raising the number of streams this endpoint may open. Without these an
        // application that has hit the limit has nothing to wait on: opening fails as
        // blocked, and the event that lifts the block is never delivered, so a caller that
        // parks waiting for it parks forever.
        cbs.extend_max_local_streams_bidi = Some(callbacks::extend_max_local_streams_bidi_cb);
        cbs.extend_max_local_streams_uni = Some(callbacks::extend_max_local_streams_uni_cb);

        let _ = role;
        cbs
    }
}

/// A QUIC connection.
///
/// Sans-I/O: it never touches a socket and never reads a clock. Datagrams and timestamps
/// come from the caller, and datagrams to send go back to the caller.
pub struct Conn<'h, S: Session> {
    raw: *mut sys::ngtcp2_conn,
    /// The TLS session, and the crate's record of the transport-parameter exchange, boxed
    /// because ngtcp2 is given this address.
    ///
    /// `ngtcp2_conn_set_tls_native_handle` stores the pointer and every crypto callback
    /// recovers the session from it. Held inline it would move when this `Conn` is returned
    /// from its builder, leaving ngtcp2 with the address of a dead frame — the same reason
    /// `BridgeSlot` is boxed.
    tls: Box<crate::tls_bridge::SessionSlot<S>>,
    handlers: Handlers<'h>,
    /// Retained by ngtcp2 as `mem`, and dereferenced during `ngtcp2_conn_del`.
    _allocator: Box<Allocator>,
    /// Retained by ngtcp2 as `user_data`; the indirection every callback recovers state
    /// through.
    slot: Box<BridgeSlot>,
    /// Retained through `settings.rand_ctx.native_handle`.
    _rand_ctx: Box<RandCtx>,
    /// Copied by ngtcp2, but kept so the connection can report its own path.
    path: Box<PathStorage>,
    role: Role,
    scid: ConnectionId,
    /// Copies of stream data ngtcp2 has accepted but the peer has not acknowledged.
    ///
    /// ngtcp2 keeps the caller's pointer rather than copying, so this is what stops a
    /// retransmission reading a buffer the caller already freed. See `crate::retain`.
    retained: Retained,
}

impl<'h, S: Session> Conn<'h, S> {
    /// Installs the TLS handle and the reference the crypto helper reads back.
    fn bind_tls(&mut self) -> Result<()> {
        // The session itself, not a helper context. ngtcp2 treats this as opaque — it stores
        // the pointer and hands it back, and does nothing else with it
        // (`ngtcp2_conn.c:14149-14159`, verified) — so the crypto callbacks in
        // `crate::tls_bridge` recover the session from it and need no reference back to the
        // connection. That absence is what makes the seam above them safe: there is nothing
        // for a backend to be given.
        let handle: *mut crate::tls_bridge::SessionSlot<S> = &raw mut *self.tls;
        // SAFETY: `raw` is live, and the slot is boxed, so its address outlives the connection
        // — `Drop` below frees the connection first.
        unsafe { sys::ngtcp2_conn_set_tls_native_handle(self.raw, handle.cast::<c_void>()) };

        // A **client's** transport parameters, now, because they travel in its very first
        // message and are settled from the start. A server's are not, and it obtains them
        // mid-handshake through `crate::tls::Handshaking` — see the trait, which exists
        // entirely because of that asymmetry.
        if self.role == Role::Client {
            // SAFETY: `raw` is live and the session is the one just installed on it.
            let rv = unsafe {
                crate::tls_bridge::set_client_local_params(self.raw, &mut self.tls.session)
            };
            if rv != 0 {
                return Err(Error::native(
                    rv,
                    "the TLS backend would not take the local transport parameters",
                ));
            }
        }
        Ok(())
    }

    /// The role this endpoint plays.
    pub fn role(&self) -> Role {
        self.role
    }

    /// This endpoint's source connection ID.
    pub fn scid(&self) -> &ConnectionId {
        &self.scid
    }

    /// The local address this connection was built with.
    pub fn local_addr(&self) -> SocketAddr {
        self.path.local()
    }

    /// The peer's address.
    pub fn remote_addr(&self) -> SocketAddr {
        self.path.remote()
    }

    /// Whether the TLS handshake has completed.
    pub fn is_handshake_completed(&self) -> bool {
        // SAFETY: `raw` is live; this is a pure query.
        unsafe { sys::ngtcp2_conn_get_handshake_completed(self.raw) != 0 }
    }

    /// The application protocol the handshake negotiated, if it has completed.
    pub fn negotiated_alpn(&self) -> Option<Vec<u8>> {
        Session::negotiated_alpn(&self.tls.session)
    }

    /// The TLS session driving this connection.
    pub fn tls(&self) -> &S {
        &self.tls.session
    }

    /// Every source connection ID this endpoint is currently reachable by.
    ///
    /// A connection answers to several identifiers at once, and [`Conn::scid`] reports only
    /// the one it was built with. Anything routing datagrams to connections needs the whole
    /// set at creation, and then needs to follow
    /// [`Handlers::on_new_connection_id`](crate::Handlers::on_new_connection_id) and
    /// [`Handlers::on_remove_connection_id`](crate::Handlers::on_remove_connection_id) to
    /// keep it accurate — this is a snapshot, not a subscription.
    pub fn scids(&self) -> Vec<ConnectionId> {
        // Asking for the count first is ngtcp2's documented protocol for this call, and is
        // what its own server does (`examples/server.cc:1856-1865`).
        // SAFETY: `raw` is live; passing null asks for the count without writing.
        let count = unsafe { sys::ngtcp2_conn_get_scid2(self.raw, core::ptr::null_mut()) };
        if count == 0 {
            return Vec::new();
        }
        // SAFETY: a zeroed `ngtcp2_cid` is a valid empty identifier, and the buffer is
        // exactly the length ngtcp2 just asked for.
        let mut raw = vec![unsafe { core::mem::zeroed::<sys::ngtcp2_cid>() }; count];
        // SAFETY: `raw` has room for `count` identifiers, which is what ngtcp2 reported.
        let written = unsafe { sys::ngtcp2_conn_get_scid2(self.raw, raw.as_mut_ptr()) };
        raw.truncate(written);
        raw.iter().map(ConnectionId::from_raw).collect()
    }

    /// The largest UDP payload this connection may currently put on the wire, in bytes.
    ///
    /// Note what this is *not*. It is not the send quantum, which is a pacing burst budget
    /// spanning several packets and would be far too large to size one datagram with. And
    /// it is not the size of the buffer to hand a write call: that must be at least the
    /// configured maximum transmit payload size, because ngtcp2 writes into it before it
    /// knows how much of it will be used. This value is *permission* — the ceiling on what
    /// may be emitted — while the buffer is *capacity*.
    ///
    /// It tracks path MTU discovery, so it changes over a connection's life.
    pub fn max_tx_udp_payload_size(&self) -> usize {
        // SAFETY: `raw` is live; this is a pure query.
        unsafe { sys::ngtcp2_conn_get_path_max_tx_udp_payload_size2(self.raw) }
    }

    /// Why the connection closed, once something has closed it.
    ///
    /// [`ReadOutcome::Draining`](crate::ReadOutcome) says only *that* the peer closed; this
    /// says what it said — an application code and a reason phrase the protocol above QUIC
    /// chose, or an idle timeout, or a transport error. That difference is what turns "the
    /// connection ended" into something an application can report or act on.
    ///
    /// Meaningful only after a close has been observed. Before then it reports the
    /// connection's initial unset error, which is byte-for-byte a graceful `NO_ERROR`
    /// close; see [`CloseError`].
    pub fn close_error(&self) -> CloseError {
        // SAFETY: `raw` is live; ngtcp2 returns a pointer to storage inside the connection,
        // which lives at least as long as this borrow.
        let raw = unsafe { sys::ngtcp2_conn_get_ccerr2(self.raw) };
        debug_assert!(!raw.is_null(), "ngtcp2 always carries a connection error");
        // SAFETY: non-null per the above, and ngtcp2 keeps `reason`/`reasonlen` consistent.
        unsafe { CloseError::from_raw(&*raw) }
    }

    /// Runs a closure with the bridge installed, so callbacks can reach the handlers.
    ///
    /// Every entry point that can fire a callback goes through here. It is deliberately not
    /// a universal wrapper: calls that cannot fire one skip it, so the cost and the borrow
    /// are only paid where they are needed.
    pub(crate) fn with_bridge<T>(&mut self, f: impl FnOnce(*mut sys::ngtcp2_conn) -> T) -> T {
        let rand_handle: *mut RandCtx = &mut *self._rand_ctx;
        let mut bridge = Bridge {
            handlers: &mut self.handlers,
            retained: &mut self.retained,
        };
        // SAFETY: both `bridge` and the rand context outlive the guards, which are dropped
        // at the end of this function -- including while unwinding.
        let _rand_guard = unsafe { RandGuard::install(rand_handle) };
        // SAFETY: `bridge` lives until the end of this function and is not moved.
        let _guard = unsafe { BridgeGuard::install(&self.slot, &mut bridge) };
        f(self.raw)
    }

    /// The raw connection, for the modules that call into ngtcp2.
    pub(crate) fn raw(&self) -> *mut sys::ngtcp2_conn {
        self.raw
    }

    /// The path, for the write paths that need it mutable.
    pub(crate) fn path_mut(&mut self) -> &mut PathStorage {
        &mut self.path
    }

    /// The path as a const pointer, for the read path.
    pub(crate) fn path_ptr(&self) -> *const sys::ngtcp2_path {
        self.path.as_raw()
    }

    /// Bytes of sent stream data still held awaiting acknowledgement.
    ///
    /// ngtcp2 does not copy what it accepts, so this crate does; the copy is released when
    /// the peer acknowledges it. A connection whose peer stops acknowledging will see this
    /// grow, which is the honest signal that memory is being held on its behalf.
    pub fn retained_bytes(&self) -> usize {
        self.retained.bytes_held()
    }

    /// The retention map, for the write path and the acknowledgement callback.
    pub(crate) fn retained_mut(&mut self) -> &mut Retained {
        &mut self.retained
    }
}

impl<S: Session> Drop for Conn<'_, S> {
    fn drop(&mut self) {
        // Order matters, though for a narrower reason than it once did. The connection is
        // destroyed first, while the TLS session is still alive, because `ngtcp2_conn_del`
        // releases the key material the session produced -- and it does that by calling the
        // delete callbacks, which reconstruct each key as the session's own key type. Freeing
        // the session first would leave those callbacks reconstructing a type whose backend
        // has gone.
        //
        // It is no longer because the TLS library holds a reference back to the connection. It
        // does not; the safe seam removed that.
        //
        // The session's own `Drop` then runs, followed by the boxes ngtcp2 was holding
        // pointers into, which nothing can reach any more.
        if !self.raw.is_null() {
            // SAFETY: the pointer came from a connection constructor and is freed exactly
            // once. The allocator it dereferences here is still alive, since it is dropped
            // after this method returns.
            unsafe { sys::ngtcp2_conn_del(self.raw) };
            self.raw = core::ptr::null_mut();
        }
    }
}

// SAFETY: a `Conn` owns its native connection, its TLS session and every box ngtcp2 points
// into, exclusively. It is deliberately not `Sync`: the bridge slot is written and read
// without synchronisation, which is sound only because a `&mut Conn` is required to reach
// it.
unsafe impl<S: Session + Send> Send for Conn<'_, S> {}

// The connection tests need a TLS session to build a connection at all, and the only
// implementation this crate ships is the OpenSSL one. With that feature off the seam is
// still compiled and still type-checked; there is simply nothing behind it to instantiate.
/// Fixtures shared by the connection tests and the packet-path tests.
///
/// A self-signed certificate is committed rather than generated because the crate may have
/// no dev-dependencies; see `tests/data/README.md`.
#[cfg(all(test, feature = "tls-ossl"))]
pub(crate) mod test_support {
    use super::*;
    use crate::rand::test_support::CountingEntropy;
    use crate::time::Timestamp;
    use crate::tls::Backend;
    use crate::tls_ossl::{OsslBackend, OsslSession, Verify};

    pub(crate) const CERT: &str = include_str!("../tests/data/test-cert.pem");
    pub(crate) const KEY: &str = include_str!("../tests/data/test-key.pem");

    pub(crate) fn addrs() -> (SocketAddr, SocketAddr) {
        (
            "127.0.0.1:1000".parse().unwrap(),
            "127.0.0.1:2000".parse().unwrap(),
        )
    }

    pub(crate) fn ts() -> Timestamp {
        Timestamp::from_nanos(1_000_000).unwrap()
    }

    pub(crate) fn client_backend() -> OsslBackend {
        OsslBackend::builder(Role::Client)
            .alpn("h3")
            .verify(Verify::DangerouslyAcceptAnyCertificate)
            .build()
            .unwrap()
    }

    pub(crate) fn server_backend() -> OsslBackend {
        OsslBackend::builder(Role::Server)
            .alpn("h3")
            .certificate_chain_pem(CERT)
            .private_key_pem(KEY)
            .build()
            .unwrap()
    }

    pub(crate) fn client_conn<'h>(handlers: Handlers<'h>) -> Result<Conn<'h, OsslSession>> {
        let backend = client_backend();
        let session = Backend::new_session(&backend, Role::Client, None)?;
        let (local, remote) = addrs();
        ConnBuilder::new(
            Role::Client,
            Settings::new(ts()),
            TransportParams::new(),
            Box::new(CountingEntropy::default()),
            session,
            local,
            remote,
        )
        .build(handlers)
    }
}

#[cfg(all(test, feature = "tls-ossl"))]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::rand::test_support::CountingEntropy;
    use crate::tls::Backend;
    use crate::tls_ossl::OsslBackend;

    #[test]
    fn a_client_connection_can_be_built_and_dropped() {
        // The first real proof that the callback table satisfies ngtcp2's mandatory set:
        // a missing entry trips an assertion inside the constructor in a debug build.
        let conn = client_conn(Handlers::new()).unwrap();
        assert_eq!(conn.role(), Role::Client);
        assert!(!conn.is_handshake_completed());
        drop(conn);
    }

    #[test]
    fn a_connection_reports_the_addresses_it_was_built_with() {
        let conn = client_conn(Handlers::new()).unwrap();
        let (local, remote) = addrs();
        assert_eq!(conn.local_addr(), local);
        assert_eq!(conn.remote_addr(), remote);
    }

    #[test]
    fn a_connection_can_be_dropped_without_any_method_being_called() {
        for _ in 0..8 {
            drop(client_conn(Handlers::new()).unwrap());
        }
    }

    #[test]
    fn the_bridge_is_not_armed_outside_a_call() {
        // A callback firing between calls must find nothing rather than a stale pointer.
        let mut conn = client_conn(Handlers::new()).unwrap();
        assert!(!conn.slot.is_armed());
        conn.with_bridge(|_| {});
        assert!(!conn.slot.is_armed());
    }

    #[test]
    fn the_bridge_is_armed_during_a_call() {
        let mut conn = client_conn(Handlers::new()).unwrap();
        let armed = conn.with_bridge(|_| true);
        assert!(armed);
    }

    #[test]
    fn construction_draws_on_the_entropy_source() {
        // The `rand` callback fires inside the constructor, before `user_data` exists, so
        // this is the proof that the `rand_ctx` route works where the bridge cannot.
        let conn = client_conn(Handlers::new()).unwrap();
        assert_eq!(conn.scid().as_bytes().len(), 8);
    }

    #[test]
    fn a_server_without_an_original_dcid_is_refused_in_both_profiles() {
        // ngtcp2 asserts this, and the assertion is compiled out of release builds, so the
        // check has to be ours. Building a server with default parameters must fail.
        let backend = OsslBackend::builder(Role::Server)
            .alpn("h3")
            .certificate_chain_pem(CERT)
            .private_key_pem(KEY)
            .build()
            .unwrap();
        let session = Backend::new_session(&backend, Role::Server, None).unwrap();
        let (local, remote) = addrs();
        let result = ConnBuilder::new(
            Role::Server,
            Settings::new(ts()),
            TransportParams::new(),
            Box::new(CountingEntropy::default()),
            session,
            local,
            remote,
        )
        .build(Handlers::new());
        assert!(result.is_err());
    }

    #[test]
    fn a_server_connection_can_be_built_from_a_decoded_initial() {
        let backend = OsslBackend::builder(Role::Server)
            .alpn("h3")
            .certificate_chain_pem(CERT)
            .private_key_pem(KEY)
            .build()
            .unwrap();
        let session = Backend::new_session(&backend, Role::Server, None).unwrap();
        let (local, remote) = addrs();

        // What a server would have taken from the client's first packet.
        let original_dcid = ConnectionId::new(&[0xaa; 8]).unwrap();
        let client_scid = ConnectionId::new(&[0xbb; 8]).unwrap();

        let conn = ConnBuilder::new(
            Role::Server,
            Settings::new(ts()),
            TransportParams::new().original_dcid(&original_dcid),
            Box::new(CountingEntropy::default()),
            session,
            local,
            remote,
        )
        .dcid(client_scid)
        .scid(ConnectionId::new(&[0xcc; 8]).unwrap())
        .build(Handlers::new())
        .unwrap();

        assert_eq!(conn.role(), Role::Server);
        assert_eq!(conn.scid().as_bytes(), &[0xcc; 8]);
    }

    #[test]
    fn a_reserved_version_is_refused_for_a_server() {
        let backend = OsslBackend::builder(Role::Server)
            .alpn("h3")
            .certificate_chain_pem(CERT)
            .private_key_pem(KEY)
            .build()
            .unwrap();
        let session = Backend::new_session(&backend, Role::Server, None).unwrap();
        let (local, remote) = addrs();
        let dcid = ConnectionId::new(&[0xaa; 8]).unwrap();
        let result = ConnBuilder::new(
            Role::Server,
            Settings::new(ts()),
            TransportParams::new().original_dcid(&dcid),
            Box::new(CountingEntropy::default()),
            session,
            local,
            remote,
        )
        .version(0x0a0a_0a0a)
        .build(Handlers::new());
        assert!(result.is_err());
    }
}
