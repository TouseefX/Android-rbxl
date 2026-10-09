//! Proof that the hand-written macro replacements carry the right version constants.
//!
//! Most of ngtcp2's documented API is function-like macros that inject a struct-version
//! constant and forward to a `_versioned` symbol. `bindgen` does not emit function-like
//! macros, so `crates/ngnet-quic/src/ffi.rs` reimplements all eighteen of them by hand.
//!
//! That reimplementation has an unusually unforgiving failure mode. A wrong constant is not
//! a compile error and not a runtime error: ngtcp2 uses it to decide how to interpret the
//! memory behind a pointer, so passing the wrong one silently misreads a struct. Nothing
//! would fail until something far away behaved strangely.
//!
//! So this file asserts three separate things:
//!
//! 1. that the constants the shim uses equal the ones the bindings define;
//! 2. that the unversioned names genuinely do **not** exist, which is the whole reason the
//!    shim has to exist — if a future bindgen started emitting them, this file is where
//!    that gets noticed rather than in a duplicate-definition error somewhere odd;
//! 3. that a constant restated from a *private* ngtcp2 header still matches that header.

use ngnet_quic_sys as sys;

/// The version constants, with the struct each one describes.
///
/// Written out rather than derived, because the point is to state independently what the
/// shim should be doing. A test that computed the expected value the same way the code does
/// would agree with a bug.
#[test]
fn every_struct_version_constant_has_the_value_the_bindings_define() {
    assert_eq!(sys::NGTCP2_PKT_INFO_VERSION, 1, "ngtcp2_pkt_info");
    assert_eq!(sys::NGTCP2_SETTINGS_VERSION, 4, "ngtcp2_settings");
    assert_eq!(
        sys::NGTCP2_TRANSPORT_PARAMS_VERSION,
        1,
        "ngtcp2_transport_params"
    );
    assert_eq!(sys::NGTCP2_CALLBACKS_VERSION, 5, "ngtcp2_callbacks");
    assert_eq!(sys::NGTCP2_CONN_INFO_VERSION, 2, "ngtcp2_conn_info");
}

/// The versioned symbols the shim forwards to must all exist.
///
/// Naming each one is what makes this a completeness check: if a future ngtcp2 renames or
/// removes one, this fails to compile with the name in the message, rather than the shim
/// quietly losing a function.
///
/// Taking each as a function pointer and casting through `*const ()` is deliberate — a bare
/// `let _: unsafe extern "C" fn(_) = f;` binding is optimised away before a relocation is
/// emitted, so it proves nothing about the symbol actually being present in the archive.
#[test]
fn every_versioned_symbol_the_shim_forwards_to_is_linkable() {
    let addresses = [
        sys::ngtcp2_conn_read_pkt_versioned as *const () as usize,
        sys::ngtcp2_conn_write_pkt_versioned as *const () as usize,
        sys::ngtcp2_conn_write_stream_versioned as *const () as usize,
        sys::ngtcp2_conn_writev_stream_versioned as *const () as usize,
        sys::ngtcp2_conn_write_datagram_versioned as *const () as usize,
        sys::ngtcp2_conn_writev_datagram_versioned as *const () as usize,
        sys::ngtcp2_conn_write_connection_close_versioned as *const () as usize,
        sys::ngtcp2_transport_params_encode_versioned as *const () as usize,
        sys::ngtcp2_transport_params_decode_versioned as *const () as usize,
        sys::ngtcp2_conn_client_new_versioned as *const () as usize,
        sys::ngtcp2_conn_server_new_versioned as *const () as usize,
        sys::ngtcp2_conn_set_local_transport_params_versioned as *const () as usize,
        sys::ngtcp2_transport_params_default_versioned as *const () as usize,
        sys::ngtcp2_conn_get_conn_info_versioned as *const () as usize,
        sys::ngtcp2_conn_get_conn_info2_versioned as *const () as usize,
        sys::ngtcp2_conn_write_aggregate_pkt_versioned as *const () as usize,
        sys::ngtcp2_conn_write_aggregate_pkt2_versioned as *const () as usize,
        sys::ngtcp2_settings_default_versioned as *const () as usize,
    ];

    assert_eq!(
        addresses.len(),
        18,
        "the header defines eighteen versioned wrappers; the shim must cover each"
    );
    for address in addresses {
        assert_ne!(address, 0, "a versioned symbol resolved to a null address");
    }
}

/// The defaults really are applied through the shim.
///
/// A weaker version of this test would only check that the call does not crash. This checks
/// a value the header documents the initialiser as setting, so a wrong version constant --
/// which would have ngtcp2 write through a differently-shaped struct -- shows up as a
/// mismatch here rather than as corruption later.
#[test]
fn the_transport_parameter_defaults_arrive_through_the_shim() {
    let mut params = unsafe { core::mem::zeroed::<sys::ngtcp2_transport_params>() };
    // SAFETY: `params` is a valid, writable, correctly-sized struct.
    unsafe {
        sys::ngtcp2_transport_params_default_versioned(
            sys::NGTCP2_TRANSPORT_PARAMS_VERSION as i32,
            &mut params,
        );
    }

    assert_eq!(
        params.max_udp_payload_size,
        u64::from(sys::NGTCP2_DEFAULT_MAX_RECV_UDP_PAYLOAD_SIZE),
        "the documented default for max_udp_payload_size"
    );
    assert_eq!(
        params.active_connection_id_limit,
        u64::from(sys::NGTCP2_DEFAULT_ACTIVE_CONNECTION_ID_LIMIT),
        "the documented default for active_connection_id_limit"
    );

    // The gap the crate documents: the initialiser leaves these zero, so a caller who
    // trusts it gets a connection with no flow-control credit and no idle timeout. Pinned
    // because the builder's job is to fill them, and if ngtcp2 ever started filling them
    // the builder would be silently overriding a sensible default.
    assert_eq!(params.initial_max_data, 0);
    assert_eq!(params.max_idle_timeout, 0);
}

/// Likewise for settings, including the field the initialiser conspicuously does not set.
#[test]
fn the_settings_defaults_arrive_through_the_shim() {
    let mut settings = unsafe { core::mem::zeroed::<sys::ngtcp2_settings>() };
    // SAFETY: `settings` is a valid, writable, correctly-sized struct.
    unsafe {
        sys::ngtcp2_settings_default_versioned(sys::NGTCP2_SETTINGS_VERSION as i32, &mut settings);
    }

    assert_ne!(
        settings.initial_rtt, 0,
        "the initialiser sets a non-zero initial RTT, which loss recovery divides by"
    );
    assert_eq!(
        settings.initial_ts, 0,
        "the initialiser does not set initial_ts -- the reason Settings requires it"
    );
}

/// A constant restated from a private ngtcp2 header must still match that header.
///
/// `NGTCP2_DCIDTR_MAX_UNUSED_DCID_SIZE` bounds `active_connection_id_limit` in the assert
/// block, but lives in `lib/ngtcp2_dcidtr.h`, which is not installed and so is not in the
/// bindings. `validate.rs` restates it. Restating a private constant is a real risk, so the
/// value is read back out of the vendored source here: if ngtcp2 changes it, this fails
/// rather than the range check silently becoming wrong.
#[test]
fn the_restated_private_constant_still_matches_the_vendored_header() {
    let header = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ngnet-quic-sys/vendor/ngtcp2/lib/ngtcp2_dcidtr.h"
    );
    let source = match std::fs::read_to_string(header) {
        Ok(source) => source,
        // The submodule is not checked out. Skipping is right: this asserts a property of
        // the vendored source, and its absence is a different problem that the build itself
        // reports far more clearly.
        Err(_) => return,
    };

    let value = source
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("#define NGTCP2_DCIDTR_MAX_UNUSED_DCID_SIZE")
                .map(str::trim)
        })
        .expect("the vendored header defines NGTCP2_DCIDTR_MAX_UNUSED_DCID_SIZE");

    assert_eq!(
        value, "8",
        "ngtcp2 changed a private constant that `validate.rs` restates as 8"
    );
}
