//! The bridge from ngtcp2's C callbacks back to Rust state.
//!
//! ngtcp2 accepts a `user_data` pointer at construction and hands it to every callback. It
//! offers no way to change it afterwards. But the state a callback needs — the
//! application's handlers, the connection's own bookkeeping — is borrowed only for the
//! duration of one call into the library, and lives at an address that may move between
//! calls. Registering it directly is therefore not possible.
//!
//! The indirection is the same one `ngnet-h3` uses. What ngtcp2 receives is a stable,
//! boxed [`BridgeSlot`] that lives as long as the connection. Before each call that can
//! fire callbacks, a [`Bridge`] holding the live borrows is installed into the slot by a
//! [`BridgeGuard`], and the guard clears it on drop — **including while unwinding**, so a
//! stray later callback cannot follow a pointer into a dead frame.
//!
//! # What this does not have to handle
//!
//! Nested calls. The slot holds exactly one pointer, so an installed bridge would be
//! overwritten by a second. That is sound here because ngtcp2 forbids re-entering it:
//! `read_pkt`, `writev_stream` and `write_connection_close` all state they "must not be
//! called from inside the callback functions" (`ngtcp2.h:4256`, `:5318`, `:6665`).
//!
//! Note the scope of that rule, because it is narrower than it first reads: it covers the
//! **packet-processing** entry points, not the whole API. The crypto callbacks in
//! [`crate::tls_bridge`] are *required* to call back into the connection — installing keys
//! and submitting handshake data from inside `client_initial` is how a handshake starts at
//! all (`ngtcp2.h:2641-2648`). Those callbacks deliberately do not use this slot; they reach
//! their state through the connection's TLS handle instead, so no second bridge is ever
//! installed and the one-pointer argument above still holds.
//!
//! # The one callback this cannot serve
//!
//! `rand` receives neither the connection nor `user_data`, and fires during construction
//! before `user_data` is stored. It reaches its entropy source through
//! `settings.rand_ctx.native_handle` instead — see [`crate::rand`].
//!
//! # Why the slot carries the entropy source as well
//!
//! `get_path_challenge_data` *does* receive `user_data`, but what it needs is unpredictable
//! bytes rather than the application's handlers. It reads them from the slot, which lives as
//! long as the connection, so path validation cannot depend on whether a bridge happened to be
//! installed. A connection has deliberately **one** source of randomness: two could diverge,
//! and only one of them would be the one an application configured.

use core::cell::Cell;
use core::ffi::c_void;

use ngnet_quic_sys as sys;

use crate::error::ApplicationErrorCode;
use crate::handlers::{Handlers, StreamCloseReason};
use crate::rand::EntropySource;
use crate::stream::StreamId;

/// The stable indirection ngtcp2 is given at construction.
///
/// Interior mutability is required because the slot is written through a shared reference
/// while ngtcp2 holds a raw pointer to it.
pub(crate) struct BridgeSlot {
    current: Cell<*mut c_void>,
    /// The connection's entropy source, for the one callback that needs randomness and does
    /// receive `user_data`.
    ///
    /// Here rather than in the [`Bridge`] because it is *always* available, whereas a bridge
    /// exists only for the duration of one call. `get_path_challenge_data` does fire from the
    /// write path, where a bridge happens to be installed today — but depending on that would
    /// make the connection's randomness contingent on an unrelated detail of when bridges are
    /// armed.
    rand: Cell<*mut RandCtx>,
}

impl BridgeSlot {
    /// Allocates an empty slot.
    pub(crate) fn new() -> Box<Self> {
        Box::new(Self {
            current: Cell::new(core::ptr::null_mut()),
            rand: Cell::new(core::ptr::null_mut()),
        })
    }

    /// Points the slot at the connection's entropy source.
    ///
    /// # Safety
    ///
    /// `rand` must outlive every callback the connection can make, which means outliving
    /// `ngtcp2_conn_del`.
    pub(crate) unsafe fn set_rand(&self, rand: *mut RandCtx) {
        self.rand.set(rand);
    }

    /// The pointer to hand to a connection constructor.
    pub(crate) fn as_ptr(&self) -> *mut c_void {
        (self as *const Self).cast_mut().cast::<c_void>()
    }

    /// Whether a bridge is currently installed.
    #[cfg(test)]
    pub(crate) fn is_armed(&self) -> bool {
        !self.current.get().is_null()
    }
}

// SAFETY: the slot holds a raw pointer that is only ever written and read while the
// connection is mutably borrowed, so no two threads can observe it at once. The connection
// is `Send` and not `Sync`, which is what enforces that.
unsafe impl Send for BridgeSlot {}

/// Everything a callback may reach, for the duration of one call.
pub(crate) struct Bridge<'a, 'h> {
    pub(crate) handlers: &'a mut Handlers<'h>,
    /// The retained copies of sent stream data, so acknowledgements can release them.
    pub(crate) retained: &'a mut crate::retain::Retained,
}

/// Installs a bridge into the slot for as long as it is alive.
///
/// Clearing on drop rather than after the call is what makes a panic safe: the slot is
/// emptied while unwinding, so a later callback cannot follow a pointer to a dead frame.
pub(crate) struct BridgeGuard<'a> {
    slot: &'a BridgeSlot,
}

impl<'a> BridgeGuard<'a> {
    /// # Safety
    ///
    /// `bridge` must remain alive and unmoved for the lifetime of the returned guard.
    pub(crate) unsafe fn install(slot: &'a BridgeSlot, bridge: &mut Bridge<'_, '_>) -> Self {
        let raw = (bridge as *mut Bridge<'_, '_>).cast::<c_void>();
        slot.current.set(raw);
        Self { slot }
    }
}

impl Drop for BridgeGuard<'_> {
    fn drop(&mut self) {
        self.slot.current.set(core::ptr::null_mut());
    }
}

/// Recovers the bridge a callback was handed.
///
/// Returns `None` when no call is in progress, which is not a bug: ngtcp2 invokes several
/// callbacks from inside its own constructor, before any bridge has been installed.
///
/// # Safety
///
/// `user_data` must be null or the pointer given to a connection constructor by
/// [`BridgeSlot::as_ptr`].
unsafe fn bridge<'a, 'h>(user_data: *mut c_void) -> Option<&'a mut Bridge<'a, 'h>> {
    if user_data.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees this is the slot pointer, which lives in a box the
    // connection owns and outlives every callback.
    let slot = unsafe { &*(user_data.cast::<BridgeSlot>()) };
    let current = slot.current.get();
    if current.is_null() {
        return None;
    }
    // SAFETY: the slot holds a pointer to a live `Bridge` installed by `BridgeGuard`, which
    // clears it before the bridge goes out of scope, including while unwinding.
    Some(unsafe { &mut *(current.cast::<Bridge<'a, 'h>>()) })
}

/// The entropy source, reached through `settings.rand_ctx.native_handle`.
///
/// Boxed and owned by the connection so its address is stable.
pub(crate) struct RandCtx {
    pub(crate) source: Box<dyn EntropySource + Send>,
    /// Set if the source ever failed.
    ///
    /// The `rand` callback returns `void`, so a failure cannot be reported where it
    /// happens. It is latched here instead, and the connection builder refuses to hand back
    /// a connection whose randomness was not what it asked for.
    pub(crate) failed: Cell<bool>,
}

impl RandCtx {
    /// Whether the entropy source failed at any point.
    pub(crate) fn failed(&self) -> bool {
        self.failed.get()
    }
}

/// `ngtcp2_rand`: fill a buffer with unpredictable bytes.
///
/// The odd one out. It receives only `rand_ctx`, never the connection or `user_data`, and
/// fires during `ngtcp2_conn_client_new` before `user_data` has been stored — so the bridge
/// above cannot serve it.
pub(crate) unsafe extern "C" fn rand_cb(
    dest: *mut u8,
    destlen: usize,
    rand_ctx: *const sys::ngtcp2_rand_ctx,
) {
    if dest.is_null() || destlen == 0 {
        return;
    }
    // SAFETY: ngtcp2 guarantees `dest` is writable for `destlen` bytes.
    let out = unsafe { core::slice::from_raw_parts_mut(dest, destlen) };

    let handle = if rand_ctx.is_null() {
        core::ptr::null_mut()
    } else {
        // SAFETY: `rand_ctx` is the struct inside the settings the connection built, whose
        // `native_handle` is the boxed `RandCtx` the connection owns.
        unsafe { (*rand_ctx).native_handle }
    };

    if !handle.is_null() {
        // SAFETY: the handle is the boxed `RandCtx`, alive for as long as the connection.
        let ctx = unsafe { &mut *handle.cast::<RandCtx>() };
        if ctx.source.fill(out).is_ok() {
            return;
        }
        ctx.failed.set(true);
    }

    // Reaching here means no source, or a source that failed. The callback returns `void`,
    // so there is no way to tell ngtcp2 -- and at three of its four call sites `dest` is an
    // *uninitialised stack local* it will use regardless (`ngtcp2_conn.c:1234`, `:1240`,
    // `:1149`). Leaving it untouched would therefore be an uninitialised read in C, seeding
    // a hash map and a PRNG from stack residue.
    //
    // So the buffer is zeroed, which is defined rather than undefined, and the failure is
    // latched above. `ConnBuilder::build` refuses to return a connection whose randomness
    // came from here, so these deterministic bytes never reach the wire.
    out.fill(0);
}

/// `ngtcp2_get_new_connection_id`: mint an identifier and its stateless reset token.
pub(crate) unsafe extern "C" fn get_new_connection_id_cb(
    _conn: *mut sys::ngtcp2_conn,
    cid: *mut sys::ngtcp2_cid,
    token: *mut u8,
    cidlen: usize,
    user_data: *mut c_void,
) -> core::ffi::c_int {
    if cid.is_null() || token.is_null() {
        return sys::NGTCP2_ERR_CALLBACK_FAILURE;
    }
    // The entropy comes from the same `RandCtx` the `rand` callback uses, reached through a
    // thread-local rather than through `user_data`, so that a connection has one source of
    // randomness rather than two that could diverge.
    //
    // SAFETY: ngtcp2 guarantees `cid` is writable and `cidlen` is within its capacity.
    unsafe {
        (*cid).datalen = cidlen;
        let bytes = core::slice::from_raw_parts_mut((*cid).data.as_mut_ptr(), cidlen);
        let tokens =
            core::slice::from_raw_parts_mut(token, sys::NGTCP2_STATELESS_RESET_TOKENLEN as usize);
        let custom = if let Some(bridge) = unsafe { bridge(user_data) } {
            bridge
                .handlers
                .connection_id_generator
                .as_mut()
                .map(|generator| generator(bytes))
        } else {
            None
        };
        match custom {
            Some(true) => {}
            Some(false) => return sys::NGTCP2_ERR_CALLBACK_FAILURE,
            None if fill_from_conn_rand(bytes).is_ok() => {}
            None => return sys::NGTCP2_ERR_CALLBACK_FAILURE,
        }
        if fill_from_conn_rand(tokens).is_err() {
            return sys::NGTCP2_ERR_CALLBACK_FAILURE;
        }
    }

    // Reporting it *is* routed through `user_data`, because unlike the entropy the handler
    // is per-connection state the bridge already carries. The bridge is absent only when no
    // call is in progress, which for this callback cannot happen: ngtcp2 reaches it solely
    // from frame enqueue (`ngtcp2_conn.c:3336`), never from its constructor.
    //
    // SAFETY: `user_data` is the slot pointer given at construction.
    if let Some(bridge) = (unsafe { bridge(user_data) })
        && let Some(handler) = bridge.handlers.on_new_connection_id.as_mut()
    {
        // SAFETY: `cid` was filled above and remains valid for this call.
        let minted = crate::cid::ConnectionId::from_raw(unsafe { &*cid });
        handler(&minted);
    }
    0
}

/// `ngtcp2_remove_connection_id`: an identifier this endpoint issued has been retired.
///
/// The counterpart to [`get_new_connection_id_cb`]. Without it an owner routing by
/// identifier would keep a table that only ever grows, and would keep delivering to
/// identifiers the peer has been told to stop using.
pub(crate) unsafe extern "C" fn remove_connection_id_cb(
    _conn: *mut sys::ngtcp2_conn,
    cid: *const sys::ngtcp2_cid,
    user_data: *mut c_void,
) -> core::ffi::c_int {
    if cid.is_null() {
        return 0;
    }
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    if let Some(handler) = bridge.handlers.on_remove_connection_id.as_mut() {
        // SAFETY: ngtcp2 passes a live identifier valid for the duration of the call.
        let retired = crate::cid::ConnectionId::from_raw(unsafe { &*cid });
        handler(&retired);
    }
    0
}

// Thread-local handle to the entropy source of the connection currently being called into.
//
// `get_new_connection_id` has no `rand_ctx` parameter, so the source has to be reachable
// some other way. It is set for the duration of each call that can fire the callback,
// alongside the bridge.
thread_local! {
    static CURRENT_RAND: Cell<*mut RandCtx> = const { Cell::new(core::ptr::null_mut()) };
}

/// Installs the current entropy source for the duration of a call.
pub(crate) struct RandGuard;

impl RandGuard {
    /// # Safety
    ///
    /// `ctx` must remain alive for the lifetime of the returned guard.
    pub(crate) unsafe fn install(ctx: *mut RandCtx) -> Self {
        CURRENT_RAND.with(|slot| slot.set(ctx));
        Self
    }
}

impl Drop for RandGuard {
    fn drop(&mut self) {
        CURRENT_RAND.with(|slot| slot.set(core::ptr::null_mut()));
    }
}

/// Fills a buffer from the connection currently being called into.
fn fill_from_conn_rand(dest: &mut [u8]) -> crate::Result<()> {
    let ptr = CURRENT_RAND.with(|slot| slot.get());
    if ptr.is_null() {
        return Err(crate::Error::with_kind(
            crate::ErrorKind::Internal,
            "no entropy source is installed for this call",
        ));
    }
    // SAFETY: the guard that set this keeps the `RandCtx` alive for the whole call.
    let ctx = unsafe { &mut *ptr };
    ctx.source.fill(dest)
}

/// `ngtcp2_recv_stream_data`: bytes arrived on a stream.
pub(crate) unsafe extern "C" fn recv_stream_data_cb(
    _conn: *mut sys::ngtcp2_conn,
    flags: u32,
    stream_id: i64,
    _offset: u64,
    data: *const u8,
    datalen: usize,
    user_data: *mut c_void,
    _stream_user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    let Ok(id) = StreamId::new(stream_id) else {
        return 0;
    };
    let fin = flags & sys::NGTCP2_STREAM_DATA_FLAG_FIN != 0;
    let bytes = if data.is_null() || datalen == 0 {
        &[][..]
    } else {
        // SAFETY: ngtcp2 guarantees the buffer is readable for `datalen` bytes and valid
        // for the duration of this call.
        unsafe { core::slice::from_raw_parts(data, datalen) }
    };
    if let Some(handler) = bridge.handlers.on_stream_data.as_mut() {
        handler(id, bytes, fin);
    }
    0
}

/// `ngtcp2_recv_datagram`: an unreliable QUIC DATAGRAM frame arrived.
pub(crate) unsafe extern "C" fn recv_datagram_cb(
    _conn: *mut sys::ngtcp2_conn,
    _flags: u32,
    data: *const u8,
    datalen: usize,
    user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    let Some(handler) = bridge.handlers.on_datagram.as_mut() else {
        return 0;
    };
    if datalen == 0 {
        handler(&[]);
        return 0;
    }
    if data.is_null() {
        return sys::NGTCP2_ERR_CALLBACK_FAILURE;
    }
    // SAFETY: ngtcp2 guarantees `data` is readable for `datalen` bytes during the callback.
    handler(unsafe { core::slice::from_raw_parts(data, datalen) });
    0
}

/// `ngtcp2_stream_open`: the peer opened a stream.
pub(crate) unsafe extern "C" fn stream_open_cb(
    _conn: *mut sys::ngtcp2_conn,
    stream_id: i64,
    user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    if let (Ok(id), Some(handler)) = (
        StreamId::new(stream_id),
        bridge.handlers.on_stream_open.as_mut(),
    ) {
        handler(id);
    }
    0
}

/// `ngtcp2_stream_close2`: a stream ended.
///
/// The `2` form rather than the original: QUIC shuts the two directions of a stream
/// independently and this callback reports a code for each, where the older `stream_close`
/// collapses them into one and loses which direction it belonged to. ngtcp2 prefers whichever
/// is registered, checking `stream_close2` first (`ngtcp2_conn.c:198`), so only this one is
/// installed.
pub(crate) unsafe extern "C" fn stream_close2_cb(
    _conn: *mut sys::ngtcp2_conn,
    flags: u32,
    stream_id: i64,
    rx_app_error_code: u64,
    tx_app_error_code: u64,
    user_data: *mut c_void,
    _stream_user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    let Ok(id) = StreamId::new(stream_id) else {
        return 0;
    };
    // A closed stream will never be retransmitted, so anything still held for it is dead
    // weight -- and ngtcp2's own contract says retention ends at close as well as at
    // acknowledgement.
    bridge.retained.forget(id);

    // Each direction reports a code only when the flag says one was set. A direction without
    // one closed cleanly, which is not the same as closing with code zero.
    let receiving = (flags & sys::NGTCP2_STREAM_CLOSE2_FLAG_RX_APP_ERROR_CODE_SET != 0)
        .then(|| ApplicationErrorCode::new(rx_app_error_code));
    let sending = (flags & sys::NGTCP2_STREAM_CLOSE2_FLAG_TX_APP_ERROR_CODE_SET != 0)
        .then(|| ApplicationErrorCode::new(tx_app_error_code));

    if let Some(handler) = bridge.handlers.on_stream_close.as_mut() {
        handler(id, StreamCloseReason::new(receiving, sending));
    }
    0
}

/// `ngtcp2_stream_reset`: the peer reset a stream it was sending on.
pub(crate) unsafe extern "C" fn stream_reset_cb(
    _conn: *mut sys::ngtcp2_conn,
    stream_id: i64,
    _final_size: u64,
    app_error_code: u64,
    user_data: *mut c_void,
    _stream_user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    if let (Ok(id), Some(handler)) = (
        StreamId::new(stream_id),
        bridge.handlers.on_stream_reset.as_mut(),
    ) {
        handler(id, ApplicationErrorCode::new(app_error_code));
    }
    0
}

/// `ngtcp2_recv_stop_sending`: the peer wants this endpoint to stop sending.
pub(crate) unsafe extern "C" fn recv_stop_sending_cb(
    _conn: *mut sys::ngtcp2_conn,
    stream_id: i64,
    app_error_code: u64,
    user_data: *mut c_void,
    _stream_user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    if let (Ok(id), Some(handler)) = (
        StreamId::new(stream_id),
        bridge.handlers.on_stop_sending.as_mut(),
    ) {
        handler(id, ApplicationErrorCode::new(app_error_code));
    }
    0
}

/// `ngtcp2_acked_stream_data_offset`: the peer acknowledged stream bytes.
pub(crate) unsafe extern "C" fn acked_stream_data_offset_cb(
    _conn: *mut sys::ngtcp2_conn,
    stream_id: i64,
    offset: u64,
    datalen: u64,
    user_data: *mut c_void,
    _stream_user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    let Ok(id) = StreamId::new(stream_id) else {
        return 0;
    };

    // Releasing the retained copy is the point of this callback, and it happens whether or
    // not the application registered a handler: the memory is held on its behalf either way.
    bridge.retained.acknowledge(id, offset, datalen);

    if let Some(handler) = bridge.handlers.on_acked_stream_data.as_mut() {
        handler(id, datalen);
    }
    0
}

/// `ngtcp2_handshake_completed`: the TLS handshake finished.
pub(crate) unsafe extern "C" fn handshake_completed_cb(
    _conn: *mut sys::ngtcp2_conn,
    user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    if let Some(handler) = bridge.handlers.on_handshake_completed.as_mut() {
        handler();
    }
    0
}

/// `ngtcp2_extend_max_local_streams_bidi`: the peer raised this endpoint's bidirectional
/// stream limit.
///
/// `max_streams` is the cumulative total this endpoint may now open, not an increment
/// (`ngtcp2.h:3074-3084`).
///
/// This is the only notification that a previously refused open may now succeed. An
/// application that waits for it without this callback registered waits forever, because
/// nothing else reports the limit moving — and the failure is a silent hang rather than an
/// error.
pub(crate) unsafe extern "C" fn extend_max_local_streams_bidi_cb(
    _conn: *mut sys::ngtcp2_conn,
    max_streams: u64,
    user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    if let Some(handler) = bridge.handlers.on_extend_max_local_streams_bidi.as_mut() {
        handler(max_streams);
    }
    0
}

/// `ngtcp2_extend_max_local_streams_uni`: the peer raised this endpoint's unidirectional
/// stream limit. See [`extend_max_local_streams_bidi_cb`].
pub(crate) unsafe extern "C" fn extend_max_local_streams_uni_cb(
    _conn: *mut sys::ngtcp2_conn,
    max_streams: u64,
    user_data: *mut c_void,
) -> core::ffi::c_int {
    // SAFETY: `user_data` is the slot pointer given at construction.
    let Some(bridge) = (unsafe { bridge(user_data) }) else {
        return 0;
    };
    if let Some(handler) = bridge.handlers.on_extend_max_local_streams_uni.as_mut() {
        handler(max_streams);
    }
    0
}

/// Supplies the unpredictable bytes a PATH_CHALLENGE frame carries.
///
/// The bytes must be unguessable. An off-path attacker who can predict them can answer a
/// challenge it never received and convince this endpoint that a path it does not control is
/// valid — so they come from the connection's configured entropy source rather than from
/// whatever the TLS backend happens to have. That is also why the TLS seam has no random
/// number generator on it at all.
pub(crate) unsafe extern "C" fn get_path_challenge_data2_cb(
    _conn: *mut sys::ngtcp2_conn,
    data: *mut sys::ngtcp2_path_challenge_data,
    user_data: *mut c_void,
) -> core::ffi::c_int {
    if data.is_null() || user_data.is_null() {
        return sys::NGTCP2_ERR_CALLBACK_FAILURE;
    }
    // SAFETY: `user_data` is the boxed slot the connection was constructed with, alive for as
    // long as the connection.
    let slot = unsafe { &*user_data.cast::<BridgeSlot>() };
    let rand = slot.rand.get();
    if rand.is_null() {
        return sys::NGTCP2_ERR_CALLBACK_FAILURE;
    }
    // SAFETY: ngtcp2 provides a writable struct of exactly this size.
    let out = unsafe { &mut (*data).data };
    // SAFETY: the pointer is the connection's boxed entropy context, alive for as long as the
    // connection, and no other reference to it is live inside a callback.
    let ctx = unsafe { &mut *rand };
    if ctx.source.fill(out).is_err() {
        ctx.failed.set(true);
        return sys::NGTCP2_ERR_CALLBACK_FAILURE;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rand::test_support::CountingEntropy;

    #[test]
    fn a_slot_starts_empty_and_a_guard_arms_and_clears_it() {
        let slot = BridgeSlot::new();
        assert!(!slot.is_armed());

        let mut handlers = Handlers::new();
        {
            let mut retained = crate::retain::Retained::default();
            let mut bridge = Bridge {
                handlers: &mut handlers,
                retained: &mut retained,
            };
            // SAFETY: `bridge` outlives the guard.
            let _guard = unsafe { BridgeGuard::install(&slot, &mut bridge) };
            assert!(slot.is_armed());
        }
        assert!(!slot.is_armed());
    }

    #[test]
    fn the_slot_is_cleared_even_when_a_panic_unwinds_through_it() {
        // The property that makes a panicking handler merely fatal rather than a
        // use-after-free: an unwinding drop still empties the slot.
        let slot = BridgeSlot::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut handlers = Handlers::new();
            let mut retained = crate::retain::Retained::default();
            let mut bridge = Bridge {
                handlers: &mut handlers,
                retained: &mut retained,
            };
            // SAFETY: `bridge` outlives the guard.
            let _guard = unsafe { BridgeGuard::install(&slot, &mut bridge) };
            assert!(slot.is_armed());
            panic!("unwinding through the guard");
        }));
        assert!(result.is_err());
        assert!(
            !slot.is_armed(),
            "the guard must clear the slot while unwinding"
        );
    }

    #[test]
    fn recovering_from_a_null_user_data_yields_nothing() {
        // ngtcp2 fires callbacks from inside its own constructor, before any bridge exists.
        // Returning `None` rather than dereferencing is what makes that safe.
        // SAFETY: null is explicitly permitted.
        assert!(unsafe { bridge(core::ptr::null_mut()) }.is_none());
    }

    #[test]
    fn recovering_from_an_unarmed_slot_yields_nothing() {
        let slot = BridgeSlot::new();
        // SAFETY: the pointer is the slot's own.
        assert!(unsafe { bridge(slot.as_ptr()) }.is_none());
    }

    #[test]
    fn a_callback_reaches_the_handler_through_the_slot() {
        let slot = BridgeSlot::new();
        let mut seen: Vec<(i64, Vec<u8>, bool)> = Vec::new();
        {
            let mut handlers = Handlers::new()
                .on_stream_data(|id, data, fin| seen.push((id.get(), data.to_vec(), fin)));
            let mut retained = crate::retain::Retained::default();
            let mut bridge = Bridge {
                handlers: &mut handlers,
                retained: &mut retained,
            };
            // SAFETY: `bridge` outlives the guard.
            let _guard = unsafe { BridgeGuard::install(&slot, &mut bridge) };

            let payload = [1u8, 2, 3];
            // SAFETY: the slot is armed and the buffer is readable for its length.
            unsafe {
                recv_stream_data_cb(
                    core::ptr::null_mut(),
                    sys::NGTCP2_STREAM_DATA_FLAG_FIN,
                    0,
                    0,
                    payload.as_ptr(),
                    payload.len(),
                    slot.as_ptr(),
                    core::ptr::null_mut(),
                );
            }
        }
        assert_eq!(seen, vec![(0, vec![1, 2, 3], true)]);
    }

    #[test]
    fn a_callback_on_an_unarmed_slot_is_a_no_op_rather_than_a_crash() {
        let slot = BridgeSlot::new();
        // SAFETY: the slot is valid but unarmed, which the callback must tolerate.
        let rc = unsafe { stream_open_cb(core::ptr::null_mut(), 0, slot.as_ptr()) };
        assert_eq!(rc, 0);
    }

    #[test]
    fn the_rand_callback_reaches_its_source_through_rand_ctx() {
        // The callback that cannot use the bridge at all, because it fires before
        // `user_data` exists and never receives it.
        let mut ctx = Box::new(RandCtx {
            source: Box::new(CountingEntropy::default()),
            failed: Cell::new(false),
        });
        let handle: *mut RandCtx = &mut *ctx;
        let rand_ctx = sys::ngtcp2_rand_ctx {
            native_handle: handle.cast::<c_void>(),
        };

        let mut buf = [0xffu8; 4];
        // SAFETY: the buffer is writable and the context outlives the call.
        unsafe { rand_cb(buf.as_mut_ptr(), buf.len(), &rand_ctx) };
        assert_eq!(buf, [0, 1, 2, 3]);
    }

    #[test]
    fn the_rand_callback_initialises_the_buffer_even_with_no_source() {
        // ngtcp2 passes *uninitialised stack locals* to this callback at three of its four
        // call sites (`ngtcp2_conn.c:1234`, `:1240`, `:1149`) and uses them regardless of
        // what happened here. Leaving the buffer untouched would therefore be an
        // uninitialised read in C, seeding a hash map and a PRNG from stack residue.
        //
        // Zeroing is defined rather than undefined. It is only acceptable because the
        // failure is latched and `ConnBuilder::build` refuses to return the connection, so
        // these deterministic bytes never reach the wire.
        let rand_ctx = sys::ngtcp2_rand_ctx {
            native_handle: core::ptr::null_mut(),
        };
        let mut buf = [0xffu8; 2];
        // SAFETY: the buffer is writable; a null handle must be tolerated.
        unsafe { rand_cb(buf.as_mut_ptr(), buf.len(), &rand_ctx) };
        assert_eq!(buf, [0, 0]);
    }

    #[test]
    fn a_failing_entropy_source_is_latched_rather_than_ignored() {
        struct Failing;
        impl EntropySource for Failing {
            fn fill(&mut self, _dest: &mut [u8]) -> crate::Result<()> {
                Err(crate::Error::with_kind(
                    crate::ErrorKind::Internal,
                    "no entropy today",
                ))
            }
        }

        let mut ctx = Box::new(RandCtx {
            source: Box::new(Failing),
            failed: Cell::new(false),
        });
        let handle: *mut RandCtx = &mut *ctx;
        let rand_ctx = sys::ngtcp2_rand_ctx {
            native_handle: handle.cast::<c_void>(),
        };

        let mut buf = [0xffu8; 4];
        // SAFETY: the buffer is writable and the context outlives the call.
        unsafe { rand_cb(buf.as_mut_ptr(), buf.len(), &rand_ctx) };

        assert_eq!(buf, [0, 0, 0, 0], "the buffer must be left initialised");
        assert!(
            ctx.failed(),
            "the failure must be recorded, since the callback cannot report it"
        );
    }

    #[test]
    fn connection_ids_are_minted_from_the_installed_source() {
        let mut ctx = Box::new(RandCtx {
            source: Box::new(CountingEntropy::default()),
            failed: Cell::new(false),
        });
        let handle: *mut RandCtx = &mut *ctx;
        // SAFETY: the context outlives the guard.
        let _guard = unsafe { RandGuard::install(handle) };

        let mut cid = sys::ngtcp2_cid {
            datalen: 0,
            data: [0; 20],
        };
        let mut token = [0u8; sys::NGTCP2_STATELESS_RESET_TOKENLEN as usize];
        // SAFETY: both out-parameters are valid and a source is installed.
        let rc = unsafe {
            get_new_connection_id_cb(
                core::ptr::null_mut(),
                &mut cid,
                token.as_mut_ptr(),
                8,
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, 0);
        assert_eq!(cid.datalen, 8);
        assert_eq!(&cid.data[..8], &[0, 1, 2, 3, 4, 5, 6, 7]);
        // The token continues the same sequence, proving one source rather than two.
        assert_eq!(token[0], 8);
    }

    #[test]
    fn minting_a_connection_id_without_a_source_fails_rather_than_inventing_bytes() {
        let mut cid = sys::ngtcp2_cid {
            datalen: 0,
            data: [0; 20],
        };
        let mut token = [0u8; sys::NGTCP2_STATELESS_RESET_TOKENLEN as usize];
        // SAFETY: both out-parameters are valid; no source is installed.
        let rc = unsafe {
            get_new_connection_id_cb(
                core::ptr::null_mut(),
                &mut cid,
                token.as_mut_ptr(),
                8,
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, sys::NGTCP2_ERR_CALLBACK_FAILURE);
    }
}
