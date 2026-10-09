//! Validation this crate performs because ngtcp2 does not.
//!
//! ngtcp2 checks the ranges of its settings and transport parameters, and the presence of
//! its mandatory callbacks, with `assert()` — forty lines of them at the top of
//! `ngtcp2_conn_client_new` / `ngtcp2_conn_server_new`
//! (`crates/ngnet-quic-sys/vendor/ngtcp2/lib/ngtcp2_conn.c:1250-1291`).
//!
//! `assert()` compiles to nothing when `NDEBUG` is defined, and `NDEBUG` is defined in
//! every release build. The `cmake` crate maps the cargo profile onto `CMAKE_BUILD_TYPE`,
//! so a `cargo build --release` of this workspace produces a `libngtcp2.a` with
//! `-O3 -DNDEBUG` and none of those checks in it. Passing an out-of-range transport
//! parameter is then not a crash but undefined behaviour, in exactly the builds anyone
//! ships.
//!
//! A safe API cannot rest on a safety net that is absent from the configuration it is used
//! in. So the checks are restated here, in Rust, where they hold identically in debug and
//! release. The tests for this module run under both.
//!
//! The list is derived from the C assert block rather than from the header prose. Where the
//! two disagree — and they do, about `version_negotiation` — the code that actually runs is
//! the one worth matching.

// These are called by the configuration builders, which arrive with the settings and
// transport-parameter types. The checks are written here first because they are what those
// builders exist to enforce, and because they are independently testable. Every one has a
// test below, so "unused" never means "unproven".
#![allow(dead_code)]

use ngnet_quic_sys as sys;

use crate::error::{Error, Result};

/// The largest value a QUIC variable-length integer can hold.
///
/// `NGTCP2_MAX_VARINT` in C. Restated because it is one of the constants bindgen dropped
/// for containing a cast.
const MAX_VARINT: u64 = (1 << 62) - 1;

/// The largest `initial_pkt_num` ngtcp2 accepts.
const MAX_INITIAL_PKT_NUM: u64 = i32::MAX as u64;

/// The exclusive upper bound on `max_ack_delay`, in nanoseconds.
///
/// `(1 << 14) * NGTCP2_MILLISECONDS` in C.
const MAX_ACK_DELAY_LIMIT: u64 = (1 << 14) * 1_000_000;

/// The most unused destination connection IDs ngtcp2 will track.
///
/// `NGTCP2_DCIDTR_MAX_UNUSED_DCID_SIZE`, which lives in the **private** header
/// `crates/ngnet-quic-sys/vendor/ngtcp2/lib/ngtcp2_dcidtr.h:45` and so is absent from the
/// generated bindings. It is
/// restated here because the assert block bounds `active_connection_id_limit` by it, and a
/// caller who exceeds it gets undefined behaviour in a release build.
///
/// Being private, it could in principle change without notice. That is why
/// `tests/versioned_ffi.rs` reads the value back out of the vendored header rather than
/// trusting this line: if ngtcp2 ever changes it, the test fails instead of the check
/// silently becoming wrong.
const MAX_UNUSED_DCID: u64 = 8;

/// Checks a value fits in a QUIC variable-length integer.
pub(crate) const fn varint(value: u64, context: &'static str) -> Result<()> {
    if value > MAX_VARINT {
        return Err(Error::invalid_input(context));
    }
    Ok(())
}

/// The settings-related checks from the assert block.
///
/// Takes the individual values rather than the raw struct so that it can be called from a
/// builder before a struct exists, and so the tests do not need to construct one.
pub(crate) const fn settings(
    max_window: u64,
    max_stream_window: u64,
    max_tx_udp_payload_size: usize,
    initial_pkt_num: u64,
    initial_rtt: u64,
) -> Result<()> {
    if max_window > MAX_VARINT {
        return Err(Error::invalid_input(
            "settings.max_window exceeds the maximum QUIC varint",
        ));
    }
    if max_stream_window > MAX_VARINT {
        return Err(Error::invalid_input(
            "settings.max_stream_window exceeds the maximum QUIC varint",
        ));
    }
    if max_tx_udp_payload_size < sys::NGTCP2_MAX_UDP_PAYLOAD_SIZE as usize {
        return Err(Error::invalid_input(
            "settings.max_tx_udp_payload_size is below NGTCP2_MAX_UDP_PAYLOAD_SIZE",
        ));
    }
    if max_tx_udp_payload_size > sys::NGTCP2_MAX_TX_UDP_PAYLOAD_SIZE as usize {
        return Err(Error::invalid_input(
            "settings.max_tx_udp_payload_size exceeds NGTCP2_MAX_TX_UDP_PAYLOAD_SIZE",
        ));
    }
    if initial_pkt_num > MAX_INITIAL_PKT_NUM {
        return Err(Error::invalid_input(
            "settings.initial_pkt_num exceeds INT32_MAX",
        ));
    }
    // Not a range check but a zero check: ngtcp2 asserts the value is truthy, and a zero
    // initial RTT would divide by zero in loss recovery.
    if initial_rtt == 0 {
        return Err(Error::invalid_input(
            "settings.initial_rtt must not be zero",
        ));
    }
    Ok(())
}

/// A single PMTUD probe size, checked against the bounds ngtcp2 asserts per element.
pub(crate) const fn pmtud_probe(size: u16) -> Result<()> {
    if size as u32 <= sys::NGTCP2_MAX_UDP_PAYLOAD_SIZE {
        return Err(Error::invalid_input(
            "a PMTUD probe size must exceed NGTCP2_MAX_UDP_PAYLOAD_SIZE",
        ));
    }
    if size as u32 > sys::NGTCP2_MAX_TX_UDP_PAYLOAD_SIZE {
        return Err(Error::invalid_input(
            "a PMTUD probe size exceeds NGTCP2_MAX_TX_UDP_PAYLOAD_SIZE",
        ));
    }
    Ok(())
}

/// The transport-parameter checks from the assert block that apply to both roles.
pub(crate) const fn transport_params_common(
    active_connection_id_limit: u64,
    initial_max_data: u64,
    initial_max_stream_data_bidi_local: u64,
    initial_max_stream_data_bidi_remote: u64,
    initial_max_stream_data_uni: u64,
    max_idle_timeout: u64,
    max_ack_delay: u64,
) -> Result<()> {
    if active_connection_id_limit < sys::NGTCP2_DEFAULT_ACTIVE_CONNECTION_ID_LIMIT as u64 {
        return Err(Error::invalid_input(
            "params.active_connection_id_limit is below the QUIC minimum of 2",
        ));
    }
    if active_connection_id_limit > MAX_UNUSED_DCID {
        return Err(Error::invalid_input(
            "params.active_connection_id_limit exceeds what ngtcp2 can track",
        ));
    }
    if initial_max_data > MAX_VARINT {
        return Err(Error::invalid_input(
            "params.initial_max_data exceeds the maximum QUIC varint",
        ));
    }
    if initial_max_stream_data_bidi_local > MAX_VARINT {
        return Err(Error::invalid_input(
            "params.initial_max_stream_data_bidi_local exceeds the maximum QUIC varint",
        ));
    }
    if initial_max_stream_data_bidi_remote > MAX_VARINT {
        return Err(Error::invalid_input(
            "params.initial_max_stream_data_bidi_remote exceeds the maximum QUIC varint",
        ));
    }
    if initial_max_stream_data_uni > MAX_VARINT {
        return Err(Error::invalid_input(
            "params.initial_max_stream_data_uni exceeds the maximum QUIC varint",
        ));
    }
    // ngtcp2 asserts this is not `UINT64_MAX` specifically, rather than bounding it.
    if max_idle_timeout == u64::MAX {
        return Err(Error::invalid_input(
            "params.max_idle_timeout must not be u64::MAX",
        ));
    }
    if max_ack_delay >= MAX_ACK_DELAY_LIMIT {
        return Err(Error::invalid_input(
            "params.max_ack_delay must be under 2^14 milliseconds",
        ));
    }
    Ok(())
}

/// The role-dependent transport-parameter checks.
///
/// These are the ones a caller is most likely to get wrong, because the same field is
/// required for one role and forbidden for the other. In particular a server **must** carry
/// `original_dcid`, which it can only obtain by decoding the client's Initial packet — so
/// this check is what turns "you forgot to call `accept` first" from undefined behaviour in
/// a release build into an error.
pub(crate) const fn transport_params_role(
    server: bool,
    original_dcid_present: bool,
    initial_scid_present: bool,
    stateless_reset_token_present: bool,
    preferred_addr_present: bool,
    retry_scid_present: bool,
) -> Result<()> {
    if server && !original_dcid_present {
        return Err(Error::invalid_input(
            "a server must set params.original_dcid, taken from the client's Initial packet",
        ));
    }
    if !server && original_dcid_present {
        return Err(Error::invalid_input(
            "a client must not set params.original_dcid",
        ));
    }
    if initial_scid_present {
        return Err(Error::invalid_input(
            "params.initial_scid is set by ngtcp2, not by the caller",
        ));
    }
    if !server && stateless_reset_token_present {
        return Err(Error::invalid_input(
            "a client must not set params.stateless_reset_token",
        ));
    }
    if !server && preferred_addr_present {
        return Err(Error::invalid_input(
            "a client must not set params.preferred_addr",
        ));
    }
    if !server && retry_scid_present {
        return Err(Error::invalid_input(
            "a client must not set params.retry_scid",
        ));
    }
    Ok(())
}

/// Rejects a reserved QUIC version for a server connection.
///
/// Reserved versions exist to exercise version negotiation and must never be chosen as the
/// actual version by a server.
/// The shortest initialisation vector ngtcp2 will accept.
///
/// `ngtcp2_crypto_create_nonce` does `dest += ivlen - sizeof(uint64_t)`
/// (`ngtcp2_crypto.c:100-112`). With a shorter vector that subtraction wraps, and the write
/// that follows lands wherever it points. ngtcp2 guards it with `assert(ivlen >= sizeof(n))`,
/// which is not there in release.
pub(crate) const MIN_IV_LEN: usize = 8;

/// The longest initialisation vector ngtcp2 will accept.
///
/// `decrypt_pkt` builds the nonce in `uint8_t nonce[64]` — with a `/* TODO nonce is limited to
/// 64 bytes. */` above it and `assert(sizeof(nonce) >= ckm->iv.len)` below
/// (`ngtcp2_conn.c:5920-5926`). A longer vector overruns that stack buffer on every packet
/// received, in release builds only.
pub(crate) const MAX_IV_LEN: usize = 64;

/// Checks an initialisation vector a TLS backend produced.
///
/// # Why this is here rather than left to the backend
///
/// The TLS seam is safe: a backend supplies `iv` as an ordinary `Vec<u8>` and the compiler
/// has nothing to say about its length. ngtcp2's own bounds are two `assert`s, and release
/// builds delete both — so without this check, safe backend code could overrun a stack buffer
/// or wrap a pointer, which is precisely the kind of hole the seam exists to close. A seam
/// that is safe only in debug builds is not safe.
pub(crate) fn iv_len(len: usize) -> Result<()> {
    if !(MIN_IV_LEN..=MAX_IV_LEN).contains(&len) {
        return Err(Error::invalid_input(
            "a TLS backend produced an initialisation vector outside the length the QUIC \
             library can handle",
        ));
    }
    Ok(())
}

/// Checks that two initialisation vectors a backend produced are the same length.
///
/// ngtcp2 installs both directions with a single length parameter, so a backend that returns
/// vectors of different lengths would have one of them read at the other's length.
pub(crate) fn iv_pair(rx: usize, tx: usize) -> Result<()> {
    iv_len(rx)?;
    iv_len(tx)?;
    if rx != tx {
        return Err(Error::invalid_input(
            "a TLS backend produced initialisation vectors of different lengths for the two \
             directions",
        ));
    }
    Ok(())
}

/// Checks a traffic secret a backend produced against the length ngtcp2 allocated for it.
///
/// A key update hands ngtcp2 the secrets the *next* update will start from, and ngtcp2 sized
/// those buffers from the current generation. A longer secret writes past them.
pub(crate) fn secret_len(len: usize, expected: usize) -> Result<()> {
    if len != expected {
        return Err(Error::invalid_input(
            "a TLS backend produced a traffic secret of the wrong length for the negotiated \
             cipher suite",
        ));
    }
    Ok(())
}

pub(crate) fn server_version(server: bool, client_chosen_version: u32) -> Result<()> {
    if server && crate::ffi::is_reserved_version(client_chosen_version) {
        return Err(Error::invalid_input(
            "a server cannot choose a reserved QUIC version",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_initialisation_vector_outside_the_librarys_range_is_rejected() {
        // Both bounds are enforced by `assert` in C and therefore absent from release builds,
        // which is the whole reason they are restated here. Below the floor a pointer
        // subtraction wraps; above the ceiling a 64-byte stack buffer overruns on every packet
        // received.
        assert!(iv_len(0).is_err());
        assert!(iv_len(MIN_IV_LEN - 1).is_err());
        assert!(iv_len(MIN_IV_LEN).is_ok());
        assert!(iv_len(12).is_ok(), "the length every real AEAD uses");
        assert!(iv_len(MAX_IV_LEN).is_ok());
        assert!(iv_len(MAX_IV_LEN + 1).is_err());
        assert!(iv_len(usize::MAX).is_err());
    }

    #[test]
    fn two_directions_must_agree_on_the_vector_length() {
        // ngtcp2 takes one length for both, so a mismatch reads one of them at the other's
        // length.
        assert!(iv_pair(12, 12).is_ok());
        assert!(iv_pair(12, 16).is_err());
        assert!(iv_pair(12, 4).is_err());
        assert!(
            iv_pair(4, 4).is_err(),
            "both out of range is still out of range"
        );
    }

    #[test]
    fn a_traffic_secret_must_match_the_buffer_it_is_copied_into() {
        assert!(secret_len(32, 32).is_ok());
        assert!(secret_len(33, 32).is_err());
        assert!(secret_len(31, 32).is_err());
    }

    use crate::error::ErrorKind;

    /// A settings tuple that passes, so each test can vary one field.
    const OK_SETTINGS: (u64, u64, usize, u64, u64) = (
        0,
        0,
        sys::NGTCP2_MAX_UDP_PAYLOAD_SIZE as usize,
        0,
        333_000_000,
    );

    fn check_settings(s: (u64, u64, usize, u64, u64)) -> Result<()> {
        settings(s.0, s.1, s.2, s.3, s.4)
    }

    #[test]
    fn the_baseline_settings_pass() {
        assert!(check_settings(OK_SETTINGS).is_ok());
    }

    #[test]
    fn a_zero_initial_rtt_is_rejected() {
        let mut s = OK_SETTINGS;
        s.4 = 0;
        let err = check_settings(s).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn an_oversized_window_is_rejected() {
        let mut s = OK_SETTINGS;
        s.0 = MAX_VARINT + 1;
        assert!(check_settings(s).is_err());
    }

    #[test]
    fn an_undersized_udp_payload_is_rejected() {
        let mut s = OK_SETTINGS;
        s.2 = sys::NGTCP2_MAX_UDP_PAYLOAD_SIZE as usize - 1;
        assert!(check_settings(s).is_err());
    }

    #[test]
    fn an_initial_packet_number_above_int32_max_is_rejected() {
        let mut s = OK_SETTINGS;
        s.3 = i32::MAX as u64 + 1;
        assert!(check_settings(s).is_err());
        s.3 = i32::MAX as u64;
        assert!(check_settings(s).is_ok());
    }

    /// Transport parameters that pass, so each test can vary one field.
    const OK_PARAMS: (u64, u64, u64, u64, u64, u64, u64) = (
        sys::NGTCP2_DEFAULT_ACTIVE_CONNECTION_ID_LIMIT as u64,
        0,
        0,
        0,
        0,
        30_000_000_000,
        0,
    );

    fn check_params(p: (u64, u64, u64, u64, u64, u64, u64)) -> Result<()> {
        transport_params_common(p.0, p.1, p.2, p.3, p.4, p.5, p.6)
    }

    #[test]
    fn the_baseline_transport_params_pass() {
        assert!(check_params(OK_PARAMS).is_ok());
    }

    #[test]
    fn too_few_connection_ids_is_rejected() {
        let mut p = OK_PARAMS;
        p.0 = 1;
        assert!(check_params(p).is_err());
    }

    #[test]
    fn an_idle_timeout_of_u64_max_is_rejected() {
        let mut p = OK_PARAMS;
        p.5 = u64::MAX;
        assert!(check_params(p).is_err());
    }

    #[test]
    fn an_ack_delay_at_the_limit_is_rejected_but_below_it_is_not() {
        let mut p = OK_PARAMS;
        p.6 = MAX_ACK_DELAY_LIMIT;
        assert!(check_params(p).is_err());
        p.6 = MAX_ACK_DELAY_LIMIT - 1;
        assert!(check_params(p).is_ok());
    }

    #[test]
    fn a_server_without_an_original_dcid_is_rejected() {
        // The check that turns "you did not decode the client's Initial packet" into an
        // error rather than undefined behaviour.
        let err = transport_params_role(true, false, false, false, false, false).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn a_client_with_an_original_dcid_is_rejected() {
        assert!(transport_params_role(false, true, false, false, false, false).is_err());
    }

    #[test]
    fn each_role_accepts_its_own_shape() {
        assert!(transport_params_role(true, true, false, false, false, false).is_ok());
        assert!(transport_params_role(false, false, false, false, false, false).is_ok());
    }

    #[test]
    fn server_only_parameters_are_rejected_for_a_client() {
        assert!(transport_params_role(false, false, false, true, false, false).is_err());
        assert!(transport_params_role(false, false, false, false, true, false).is_err());
        assert!(transport_params_role(false, false, false, false, false, true).is_err());
    }

    #[test]
    fn an_initial_scid_is_rejected_for_either_role() {
        assert!(transport_params_role(true, true, true, false, false, false).is_err());
        assert!(transport_params_role(false, false, true, false, false, false).is_err());
    }

    #[test]
    fn pmtud_probe_sizes_are_bounded_on_both_sides() {
        assert!(pmtud_probe(sys::NGTCP2_MAX_UDP_PAYLOAD_SIZE as u16).is_err());
        assert!(pmtud_probe(sys::NGTCP2_MAX_UDP_PAYLOAD_SIZE as u16 + 1).is_ok());
    }

    #[test]
    fn a_reserved_version_is_rejected_for_a_server_only() {
        // Reserved versions have the pattern 0x?a?a?a?a.
        let reserved = 0x0a0a_0a0au32;
        assert!(server_version(true, reserved).is_err());
        assert!(server_version(false, reserved).is_ok());
        assert!(server_version(true, sys::NGTCP2_PROTO_VER_V1).is_ok());
    }

    #[test]
    fn varint_bounds_are_inclusive() {
        assert!(varint(MAX_VARINT, "test").is_ok());
        assert!(varint(MAX_VARINT + 1, "test").is_err());
    }
}
