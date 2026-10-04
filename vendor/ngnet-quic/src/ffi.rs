//! Hand-written equivalents of ngtcp2's function-like macros.
//!
//! Almost every name in ngtcp2's documentation — `ngtcp2_conn_read_pkt`,
//! `ngtcp2_conn_client_new`, `ngtcp2_settings_default` and fifteen others — is not a
//! function. Each is a macro that injects a struct-version constant and forwards to a
//! `_versioned` symbol, so that a caller compiled against an older header keeps working
//! when a struct grows a field (`ngtcp2.h:7295-7476`).
//!
//! `bindgen` does not emit function-like macros. The generated bindings therefore contain
//! `ngtcp2_conn_read_pkt_versioned` and no `ngtcp2_conn_read_pkt` at all, which means this
//! crate cannot call the API as documented without supplying the macros itself.
//!
//! That is what this module is. Every version constant used anywhere in the crate appears
//! here and nowhere else, because the failure mode is unusually bad: passing the wrong
//! constant does not fail to compile and does not return an error. ngtcp2 uses it to decide
//! how to interpret the memory behind a pointer, so a wrong value is silent
//! misinterpretation of a struct. Keeping them in one file makes them reviewable, and
//! `tests/versioned_ffi.rs` pins every one against the value in the bindings.
//!
//! These are `pub(crate)` and take the same arguments in the same order as the C macros.
//! They are `unsafe` for exactly the reasons the underlying functions are; the safety
//! documentation lives at the call sites, which know what the pointers point to.

// Most of this module is unused until the phase that first calls each function. Silencing
// the warning wholesale here, rather than scattering `#[allow(dead_code)]` over individual
// items, keeps the module readable and means the attribute disappears in one edit once the
// surface is fully wired. `tests/versioned_ffi.rs` exercises the version constants
// regardless, so "unused" never means "unproven".
#![allow(dead_code)]

use ngnet_quic_sys as sys;

/// The struct-version constants, gathered so the test can enumerate them.
///
/// Duplicating them as a named table is deliberate: it gives the test something to iterate,
/// and it puts the mapping from "which struct" to "which constant" in one readable place.
pub(crate) mod version {
    use ngnet_quic_sys as sys;

    /// Version of `ngtcp2_pkt_info`.
    pub(crate) const PKT_INFO: i32 = sys::NGTCP2_PKT_INFO_VERSION as i32;
    /// Version of `ngtcp2_settings`.
    pub(crate) const SETTINGS: i32 = sys::NGTCP2_SETTINGS_VERSION as i32;
    /// Version of `ngtcp2_transport_params`.
    pub(crate) const TRANSPORT_PARAMS: i32 = sys::NGTCP2_TRANSPORT_PARAMS_VERSION as i32;
    /// Version of `ngtcp2_callbacks`.
    pub(crate) const CALLBACKS: i32 = sys::NGTCP2_CALLBACKS_VERSION as i32;
    /// Version of `ngtcp2_conn_info`.
    pub(crate) const CONN_INFO: i32 = sys::NGTCP2_CONN_INFO_VERSION as i32;
}

/// `ngtcp2_is_reserved_version`.
///
/// Safe because it is a pure predicate over an integer with no preconditions. Wrapped here
/// rather than called from `validate` so that module can stay free of `unsafe`.
pub(crate) fn is_reserved_version(version: u32) -> bool {
    // SAFETY: a pure predicate over an integer, with no preconditions.
    unsafe { sys::ngtcp2_is_reserved_version(version) != 0 }
}

/// `ngtcp2_conn_read_pkt`.
///
/// # Safety
///
/// `conn` and `path` must be valid, and `pkt` must be readable for `pktlen` bytes.
pub(crate) unsafe fn conn_read_pkt(
    conn: *mut sys::ngtcp2_conn,
    path: *const sys::ngtcp2_path,
    pi: *const sys::ngtcp2_pkt_info,
    pkt: *const u8,
    pktlen: usize,
    ts: sys::ngtcp2_tstamp,
) -> i32 {
    // SAFETY: forwarding the caller's own contract, with the version constant the macro
    // would have supplied.
    unsafe {
        sys::ngtcp2_conn_read_pkt_versioned(conn, path, version::PKT_INFO, pi, pkt, pktlen, ts)
    }
}

/// `ngtcp2_conn_write_pkt`.
///
/// # Safety
///
/// `conn` must be valid and `dest` writable for `destlen` bytes.
pub(crate) unsafe fn conn_write_pkt(
    conn: *mut sys::ngtcp2_conn,
    path: *mut sys::ngtcp2_path,
    pi: *mut sys::ngtcp2_pkt_info,
    dest: *mut u8,
    destlen: usize,
    ts: sys::ngtcp2_tstamp,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_write_pkt_versioned(conn, path, version::PKT_INFO, pi, dest, destlen, ts)
    }
}

/// `ngtcp2_conn_write_stream`.
///
/// # Safety
///
/// `conn` must be valid, `dest` writable for `destlen` bytes, and `data` readable for
/// `datalen` bytes.
#[allow(clippy::too_many_arguments)] // The C macro's own arity; diverging would obscure it.
pub(crate) unsafe fn conn_write_stream(
    conn: *mut sys::ngtcp2_conn,
    path: *mut sys::ngtcp2_path,
    pi: *mut sys::ngtcp2_pkt_info,
    dest: *mut u8,
    destlen: usize,
    pdatalen: *mut sys::ngtcp2_ssize,
    flags: u32,
    stream_id: i64,
    data: *const u8,
    datalen: usize,
    ts: sys::ngtcp2_tstamp,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_write_stream_versioned(
            conn,
            path,
            version::PKT_INFO,
            pi,
            dest,
            destlen,
            pdatalen,
            flags,
            stream_id,
            data,
            datalen,
            ts,
        )
    }
}

/// `ngtcp2_conn_writev_stream`.
///
/// # Safety
///
/// `conn` must be valid, `dest` writable for `destlen` bytes, and `datav` must point to
/// `datavcnt` valid vectors.
#[allow(clippy::too_many_arguments)] // The C macro's own arity.
pub(crate) unsafe fn conn_writev_stream(
    conn: *mut sys::ngtcp2_conn,
    path: *mut sys::ngtcp2_path,
    pi: *mut sys::ngtcp2_pkt_info,
    dest: *mut u8,
    destlen: usize,
    pdatalen: *mut sys::ngtcp2_ssize,
    flags: u32,
    stream_id: i64,
    datav: *const sys::ngtcp2_vec,
    datavcnt: usize,
    ts: sys::ngtcp2_tstamp,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_writev_stream_versioned(
            conn,
            path,
            version::PKT_INFO,
            pi,
            dest,
            destlen,
            pdatalen,
            flags,
            stream_id,
            datav,
            datavcnt,
            ts,
        )
    }
}

/// `ngtcp2_conn_write_datagram`.
///
/// # Safety
///
/// `conn` must be valid, `dest` writable for `destlen` bytes, `data` readable for `datalen`.
#[allow(clippy::too_many_arguments)] // The C macro's own arity.
pub(crate) unsafe fn conn_write_datagram(
    conn: *mut sys::ngtcp2_conn,
    path: *mut sys::ngtcp2_path,
    pi: *mut sys::ngtcp2_pkt_info,
    dest: *mut u8,
    destlen: usize,
    paccepted: *mut i32,
    flags: u32,
    dgram_id: u64,
    data: *const u8,
    datalen: usize,
    ts: sys::ngtcp2_tstamp,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_write_datagram_versioned(
            conn,
            path,
            version::PKT_INFO,
            pi,
            dest,
            destlen,
            paccepted,
            flags,
            dgram_id,
            data,
            datalen,
            ts,
        )
    }
}

/// `ngtcp2_conn_writev_datagram`.
///
/// # Safety
///
/// `conn` must be valid, `dest` writable for `destlen` bytes, and `datav` must point to
/// `datavcnt` valid vectors.
#[allow(clippy::too_many_arguments)] // The C macro's own arity.
pub(crate) unsafe fn conn_writev_datagram(
    conn: *mut sys::ngtcp2_conn,
    path: *mut sys::ngtcp2_path,
    pi: *mut sys::ngtcp2_pkt_info,
    dest: *mut u8,
    destlen: usize,
    paccepted: *mut i32,
    flags: u32,
    dgram_id: u64,
    datav: *const sys::ngtcp2_vec,
    datavcnt: usize,
    ts: sys::ngtcp2_tstamp,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_writev_datagram_versioned(
            conn,
            path,
            version::PKT_INFO,
            pi,
            dest,
            destlen,
            paccepted,
            flags,
            dgram_id,
            datav,
            datavcnt,
            ts,
        )
    }
}

/// `ngtcp2_conn_write_connection_close`.
///
/// # Safety
///
/// `conn` must be valid, `dest` writable for `destlen` bytes, and `ccerr` must outlive the
/// call — ngtcp2 does not copy its reason phrase.
pub(crate) unsafe fn conn_write_connection_close(
    conn: *mut sys::ngtcp2_conn,
    path: *mut sys::ngtcp2_path,
    pi: *mut sys::ngtcp2_pkt_info,
    dest: *mut u8,
    destlen: usize,
    ccerr: *const sys::ngtcp2_ccerr,
    ts: sys::ngtcp2_tstamp,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_write_connection_close_versioned(
            conn,
            path,
            version::PKT_INFO,
            pi,
            dest,
            destlen,
            ccerr,
            ts,
        )
    }
}

/// `ngtcp2_transport_params_encode`.
///
/// # Safety
///
/// `dest` must be writable for `destlen` bytes and `params` must be valid.
pub(crate) unsafe fn transport_params_encode(
    dest: *mut u8,
    destlen: usize,
    params: *const sys::ngtcp2_transport_params,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_transport_params_encode_versioned(
            dest,
            destlen,
            version::TRANSPORT_PARAMS,
            params,
        )
    }
}

/// `ngtcp2_transport_params_decode`.
///
/// # Safety
///
/// `params` must be writable and `data` readable for `datalen` bytes. Note that the decoded
/// `version_info.available_versions` borrows into `data` rather than owning a copy.
pub(crate) unsafe fn transport_params_decode(
    params: *mut sys::ngtcp2_transport_params,
    data: *const u8,
    datalen: usize,
) -> i32 {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_transport_params_decode_versioned(
            version::TRANSPORT_PARAMS,
            params,
            data,
            datalen,
        )
    }
}

/// `ngtcp2_conn_client_new`.
///
/// # Safety
///
/// Every pointer must be valid for the call. `mem` and `user_data` are **retained** by the
/// connection and must outlive it; the rest are copied.
#[allow(clippy::too_many_arguments)] // The C macro's own arity.
pub(crate) unsafe fn conn_client_new(
    pconn: *mut *mut sys::ngtcp2_conn,
    dcid: *const sys::ngtcp2_cid,
    scid: *const sys::ngtcp2_cid,
    path: *const sys::ngtcp2_path,
    client_chosen_version: u32,
    callbacks: *const sys::ngtcp2_callbacks,
    settings: *const sys::ngtcp2_settings,
    params: *const sys::ngtcp2_transport_params,
    mem: *const sys::ngtcp2_mem,
    user_data: *mut core::ffi::c_void,
) -> i32 {
    // SAFETY: forwarding the caller's own contract, with the three version constants the
    // macro would have supplied.
    unsafe {
        sys::ngtcp2_conn_client_new_versioned(
            pconn,
            dcid,
            scid,
            path,
            client_chosen_version,
            version::CALLBACKS,
            callbacks,
            version::SETTINGS,
            settings,
            version::TRANSPORT_PARAMS,
            params,
            mem,
            user_data,
        )
    }
}

/// `ngtcp2_conn_server_new`.
///
/// # Safety
///
/// As [`conn_client_new`]. `params` must have `original_dcid_present` set, which for a
/// server means the client's Initial packet must already have been decoded.
#[allow(clippy::too_many_arguments)] // The C macro's own arity.
pub(crate) unsafe fn conn_server_new(
    pconn: *mut *mut sys::ngtcp2_conn,
    dcid: *const sys::ngtcp2_cid,
    scid: *const sys::ngtcp2_cid,
    path: *const sys::ngtcp2_path,
    client_chosen_version: u32,
    callbacks: *const sys::ngtcp2_callbacks,
    settings: *const sys::ngtcp2_settings,
    params: *const sys::ngtcp2_transport_params,
    mem: *const sys::ngtcp2_mem,
    user_data: *mut core::ffi::c_void,
) -> i32 {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_server_new_versioned(
            pconn,
            dcid,
            scid,
            path,
            client_chosen_version,
            version::CALLBACKS,
            callbacks,
            version::SETTINGS,
            settings,
            version::TRANSPORT_PARAMS,
            params,
            mem,
            user_data,
        )
    }
}

/// `ngtcp2_conn_set_local_transport_params`.
///
/// # Safety
///
/// `conn` and `params` must be valid. Server use only.
pub(crate) unsafe fn conn_set_local_transport_params(
    conn: *mut sys::ngtcp2_conn,
    params: *const sys::ngtcp2_transport_params,
) -> i32 {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_set_local_transport_params_versioned(
            conn,
            version::TRANSPORT_PARAMS,
            params,
        )
    }
}

/// `ngtcp2_transport_params_default`.
///
/// # Safety
///
/// `params` must be writable.
///
/// Note that this leaves every `initial_max_*` field and `max_idle_timeout` at zero — a
/// connection with no flow-control credit and no idle timeout — so it is a starting point
/// rather than a usable configuration.
pub(crate) unsafe fn transport_params_default(params: *mut sys::ngtcp2_transport_params) {
    // SAFETY: forwarding the caller's own contract.
    unsafe { sys::ngtcp2_transport_params_default_versioned(version::TRANSPORT_PARAMS, params) }
}

/// `ngtcp2_settings_default`.
///
/// # Safety
///
/// `settings` must be writable.
///
/// Note that this does **not** set `initial_ts`, so a caller that forgets it gets a
/// connection whose entire clock base is zero.
pub(crate) unsafe fn settings_default(settings: *mut sys::ngtcp2_settings) {
    // SAFETY: forwarding the caller's own contract.
    unsafe { sys::ngtcp2_settings_default_versioned(version::SETTINGS, settings) }
}

/// `ngtcp2_conn_get_conn_info`.
///
/// # Safety
///
/// `conn` and `cinfo` must be valid.
pub(crate) unsafe fn conn_get_conn_info(
    conn: *mut sys::ngtcp2_conn,
    cinfo: *mut sys::ngtcp2_conn_info,
) {
    // SAFETY: forwarding the caller's own contract.
    unsafe { sys::ngtcp2_conn_get_conn_info_versioned(conn, version::CONN_INFO, cinfo) }
}

/// `ngtcp2_conn_get_conn_info2`.
///
/// The `const`-correct variant, added in ngtcp2 1.23.0.
///
/// # Safety
///
/// `conn` and `cinfo` must be valid.
pub(crate) unsafe fn conn_get_conn_info2(
    conn: *const sys::ngtcp2_conn,
    cinfo: *mut sys::ngtcp2_conn_info,
) {
    // SAFETY: forwarding the caller's own contract.
    unsafe { sys::ngtcp2_conn_get_conn_info2_versioned(conn, version::CONN_INFO, cinfo) }
}

/// `ngtcp2_conn_write_aggregate_pkt`.
///
/// # Safety
///
/// `conn` must be valid and `buf` writable for `buflen` bytes.
#[allow(clippy::too_many_arguments)] // The C macro's own arity.
pub(crate) unsafe fn conn_write_aggregate_pkt(
    conn: *mut sys::ngtcp2_conn,
    path: *mut sys::ngtcp2_path,
    pi: *mut sys::ngtcp2_pkt_info,
    buf: *mut u8,
    buflen: usize,
    pgsolen: *mut usize,
    write_pkt: sys::ngtcp2_write_pkt,
    ts: sys::ngtcp2_tstamp,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_write_aggregate_pkt_versioned(
            conn,
            path,
            version::PKT_INFO,
            pi,
            buf,
            buflen,
            pgsolen,
            write_pkt,
            ts,
        )
    }
}

/// `ngtcp2_conn_write_aggregate_pkt2`.
///
/// # Safety
///
/// `conn` must be valid and `buf` writable for `buflen` bytes.
#[allow(clippy::too_many_arguments)] // The C macro's own arity.
pub(crate) unsafe fn conn_write_aggregate_pkt2(
    conn: *mut sys::ngtcp2_conn,
    path: *mut sys::ngtcp2_path,
    pi: *mut sys::ngtcp2_pkt_info,
    buf: *mut u8,
    buflen: usize,
    pgsolen: *mut usize,
    write_pkt: sys::ngtcp2_write_pkt,
    num_pkts: usize,
    ts: sys::ngtcp2_tstamp,
) -> sys::ngtcp2_ssize {
    // SAFETY: forwarding the caller's own contract.
    unsafe {
        sys::ngtcp2_conn_write_aggregate_pkt2_versioned(
            conn,
            path,
            version::PKT_INFO,
            pi,
            buf,
            buflen,
            pgsolen,
            write_pkt,
            num_pkts,
            ts,
        )
    }
}
