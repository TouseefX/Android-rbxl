//! End-to-end check that the generated bindings can drive a real QUIC
//! connection object.
//!
//! No sockets and no TLS handshake are involved. ngtcp2 is sans-I/O in the same
//! way nghttp3 is: it turns datagrams into stream data and back, and the caller
//! does the sending. That makes a connection object constructible, and its
//! transport parameters encodable, without anything on the wire.

use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::ptr;

use ngnet_quic_sys::*;

/// The vendored submodule this crate builds against.
const EXPECTED_VERSION: &str = "1.25.0";

#[test]
fn reports_the_vendored_version() {
    unsafe {
        let info = ngtcp2_version(0);
        assert!(!info.is_null());
        let version = CStr::from_ptr((*info).version_str)
            .to_str()
            .expect("version string is not UTF-8");
        assert_eq!(
            version, EXPECTED_VERSION,
            "linked against a different ngtcp2 than the vendored submodule"
        );
    }
}

/// Round-trips transport parameters through ngtcp2's own codec.
///
/// This is real protocol code — the varint encoding QUIC puts in the TLS
/// handshake — so it exercises rather more of the build than a version string
/// does, while needing no peer. It also pins the versioned-struct convention:
/// `ngtcp2_transport_params` is passed with an explicit version, and a mismatch
/// between the constant and the struct layout would show up here as garbage
/// rather than as a link error.
#[test]
fn transport_parameters_round_trip() {
    unsafe {
        let mut params = default_transport_params();
        params.initial_max_data = 1 << 20;
        params.initial_max_stream_data_bidi_local = 1 << 16;
        params.initial_max_streams_bidi = 100;

        let mut buf = [0u8; 256];
        let written = ngtcp2_transport_params_encode_versioned(
            buf.as_mut_ptr(),
            buf.len(),
            NGTCP2_TRANSPORT_PARAMS_VERSION as i32,
            &params,
        );
        assert!(written > 0, "encoding transport parameters failed");

        let mut decoded = MaybeUninit::<ngtcp2_transport_params>::uninit();
        let rv = ngtcp2_transport_params_decode_versioned(
            NGTCP2_TRANSPORT_PARAMS_VERSION as i32,
            decoded.as_mut_ptr(),
            buf.as_ptr(),
            written as usize,
        );
        assert_eq!(rv, 0, "decoding transport parameters failed");
        let decoded = decoded.assume_init();

        assert_eq!(decoded.initial_max_data, 1 << 20);
        assert_eq!(decoded.initial_max_stream_data_bidi_local, 1 << 16);
        assert_eq!(decoded.initial_max_streams_bidi, 100);
    }
}

/// Builds a real client `ngtcp2_conn`.
///
/// This is the test that would catch a genuinely broken build: the versioned
/// constructor has to agree with the header about the layout of three separate
/// structs (`ngtcp2_callbacks`, `ngtcp2_settings`, `ngtcp2_transport_params`),
/// and ngtcp2 asserts on the contents of the latter two. A layout mismatch
/// surfaces as an assertion failure inside C rather than as a compile error, so
/// only actually calling it proves anything.
#[test]
fn client_connection_can_be_constructed() {
    unsafe {
        let mut dcid = MaybeUninit::<ngtcp2_cid>::uninit();
        let mut scid = MaybeUninit::<ngtcp2_cid>::uninit();
        // Distinct values: ngtcp2 keeps both, and swapping them would not be
        // caught by anything below.
        ngtcp2_cid_init(dcid.as_mut_ptr(), [0xd0u8; 16].as_ptr(), 16);
        ngtcp2_cid_init(scid.as_mut_ptr(), [0x5cu8; 8].as_ptr(), 8);
        let dcid = dcid.assume_init();
        let scid = scid.assume_init();

        let mut local = loopback_sockaddr();
        let mut remote = loopback_sockaddr();
        let path = ngtcp2_path {
            local: ngtcp2_addr {
                addr: (&raw mut local).cast::<ngtcp2_sockaddr>(),
                addrlen: size_of::<libc_sockaddr_in>() as ngtcp2_socklen,
            },
            remote: ngtcp2_addr {
                addr: (&raw mut remote).cast::<ngtcp2_sockaddr>(),
                addrlen: size_of::<libc_sockaddr_in>() as ngtcp2_socklen,
            },
            user_data: ptr::null_mut(),
        };

        // Defaults matter here rather than being incidental: ngtcp2 asserts that
        // `max_tx_udp_payload_size` and `active_connection_id_limit` are within
        // protocol bounds, and a zeroed struct violates both.
        let settings = default_settings();
        let params = default_transport_params();

        let mut conn: *mut ngtcp2_conn = ptr::null_mut();
        let rv = ngtcp2_conn_client_new_versioned(
            &mut conn,
            &dcid,
            &scid,
            &path,
            NGTCP2_PROTO_VER_V1,
            NGTCP2_CALLBACKS_VERSION as i32,
            &client_callbacks(),
            NGTCP2_SETTINGS_VERSION as i32,
            &settings,
            NGTCP2_TRANSPORT_PARAMS_VERSION as i32,
            &params,
            ptr::null(),
            ptr::null_mut(),
        );
        assert_eq!(rv, 0, "ngtcp2_conn_client_new_versioned failed");
        assert!(!conn.is_null());

        // The connection should know which version it chose and that it has not
        // handshaked, which is only answerable by a properly constructed object.
        assert_eq!(
            ngtcp2_conn_get_client_chosen_version(conn),
            NGTCP2_PROTO_VER_V1
        );
        assert_eq!(
            ngtcp2_conn_get_handshake_completed(conn),
            0,
            "a freshly built connection cannot have completed a handshake"
        );

        // The client's own DCID is the one it was given.
        let got_dcid = ngtcp2_conn_get_dcid(conn);
        assert!(!got_dcid.is_null());
        assert_eq!((*got_dcid).datalen, 16);

        ngtcp2_conn_del(conn);
    }
}

/// The OpenSSL crypto helper links and initialises.
///
/// Distinct from the tests above: those exercise `libngtcp2.a`, which has no TLS
/// dependency at all. This one is the only thing that proves the *second*
/// archive was built and that OpenSSL resolved on the link line.
#[cfg(feature = "crypto-ossl")]
#[test]
fn openssl_crypto_backend_initialises() {
    unsafe {
        assert_eq!(
            ngtcp2_crypto_ossl_init(),
            0,
            "ngtcp2's OpenSSL crypto backend failed to initialise"
        );

        // Allocating and freeing a context reaches further into the archive than
        // `init` alone, which on some builds does very little.
        let mut ctx: *mut ngtcp2_crypto_ossl_ctx = ptr::null_mut();
        assert_eq!(ngtcp2_crypto_ossl_ctx_new(&mut ctx, ptr::null_mut()), 0);
        assert!(!ctx.is_null());
        assert!(ngtcp2_crypto_ossl_ctx_get_ssl(ctx).is_null());
        ngtcp2_crypto_ossl_ctx_del(ctx);

        // Force the two functions that actually reach into libssl to be linked
        // in. Without this the test proves less than it appears to: the archive
        // is built with -ffunction-sections and linked with --gc-sections, so
        // the members calling SSL_set_quic_tls_cbs and friends are discarded and
        // the binary ends up needing only libcrypto — leaving `-lssl` passed but
        // never actually resolved.
        //
        // Casting through `usize` is what makes this work: a plain
        // `let _: unsafe extern "C" fn(_) -> _ = f;` binding is dropped before
        // it emits a relocation, and the sections vanish again.
        let client = ngtcp2_crypto_ossl_configure_client_session as *const () as usize;
        let server = ngtcp2_crypto_ossl_configure_server_session as *const () as usize;
        assert_ne!(client, 0);
        assert_ne!(server, 0);
        assert_ne!(client, server);

        ngtcp2_crypto_ossl_free();
    }
}

/// The constants restated in `wrapper.h` must survive with the right values
/// *and* the right widths.
///
/// ngtcp2 writes these with a cast, which bindgen's macro evaluator silently
/// drops — an absent constant is not a build error, it is just missing. And
/// bindgen sizes what it does emit by value, so a duration small enough to fit
/// in 32 bits arrives as `u32` unless build.rs says otherwise. Both failures are
/// quiet, so both are asserted here rather than assumed.
#[test]
fn restated_constants_keep_their_values_and_widths() {
    // Durations are ngtcp2_duration, i.e. uint64_t. The annotations are the
    // assertion: this does not compile if build.rs stops widening them.
    let nanosecond: u64 = NGTCP2_NANOSECONDS;
    let microsecond: u64 = NGTCP2_MICROSECONDS;
    let millisecond: u64 = NGTCP2_MILLISECONDS;
    let second: u64 = NGTCP2_SECONDS;
    let minute: u64 = NGTCP2_MINUTES;
    let initial_rtt: u64 = NGTCP2_DEFAULT_INITIAL_RTT;
    let max_ack_delay: u64 = NGTCP2_DEFAULT_MAX_ACK_DELAY;

    assert_eq!(nanosecond, 1);
    assert_eq!(microsecond, 1_000 * nanosecond);
    assert_eq!(millisecond, 1_000 * microsecond);
    assert_eq!(second, 1_000 * millisecond);
    assert_eq!(minute, 60 * second);
    assert_eq!(initial_rtt, 333 * millisecond);
    assert_eq!(max_ack_delay, 25 * millisecond);

    // The width is the point of this one: as u32 it would overflow, and the
    // multiplication is exactly the shape ngtcp2's own API invites.
    assert_eq!(30 * second, 30_000_000_000);

    let v1: u32 = NGTCP2_PROTO_VER_V1;
    let v2: u32 = NGTCP2_PROTO_VER_V2;
    assert_eq!(v1, 0x0000_0001);
    assert_eq!(v2, 0x6b33_43cf);
    assert_eq!(NGTCP2_PROTO_VER_MIN, v1);
    assert_eq!(NGTCP2_PROTO_VER_MAX, v1);

    // ngtcp2 agrees these are the versions it supports, which is what makes the
    // restatement above safe to rely on rather than merely self-consistent.
    unsafe {
        assert_eq!(ngtcp2_is_supported_version(v1), 1);
        assert_eq!(ngtcp2_is_supported_version(0xdead_beef), 0);
    }
}

/// The symbols a safe wrapper would be built on must all be reachable. Naming
/// them here means a bindgen allowlist regression fails this crate's own tests
/// rather than surfacing later as a confusing error in a dependent crate.
#[test]
fn the_symbols_a_wrapper_needs_are_generated() {
    let _: unsafe extern "C" fn(_, _, _, _, _, _, _, _, _, _, _, _, _) -> _ =
        ngtcp2_conn_client_new_versioned;
    let _: unsafe extern "C" fn(_, _, _, _, _, _, _, _, _, _, _, _, _) -> _ =
        ngtcp2_conn_server_new_versioned;
    // Reading datagrams in, and writing stream data out: the two halves of the
    // transport loop a QUIC backend has to drive.
    let _: unsafe extern "C" fn(_, _, _, _, _, _, _) -> _ = ngtcp2_conn_read_pkt_versioned;
    let _: unsafe extern "C" fn(_, _, _, _, _, _, _, _, _, _, _, _) -> _ =
        ngtcp2_conn_writev_stream_versioned;
    let _: unsafe extern "C" fn(_, _, _, _) -> _ = ngtcp2_conn_shutdown_stream_write;
    let _: unsafe extern "C" fn(_, _, _) -> _ = ngtcp2_conn_extend_max_stream_offset;
    let _: unsafe extern "C" fn(_, _) = ngtcp2_conn_extend_max_offset;
    let _: unsafe extern "C" fn(_, _, _) -> _ = ngtcp2_conn_open_bidi_stream;
    let _: unsafe extern "C" fn(_, _, _) -> _ = ngtcp2_conn_open_uni_stream;
    let _: unsafe extern "C" fn(_) -> _ = ngtcp2_conn_get_expiry;
    let _: unsafe extern "C" fn(_, _) -> _ = ngtcp2_conn_handle_expiry;
    let _: unsafe extern "C" fn(_) -> _ = ngtcp2_err_is_fatal;
    let _: unsafe extern "C" fn(_) -> _ = ngtcp2_strerror;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `ngtcp2_sockaddr` is `struct sockaddr`, which is too small to hold an IPv4
/// address; ngtcp2 reads `addrlen` bytes through it. This is the storage those
/// bytes actually live in.
#[repr(C)]
#[derive(Clone, Copy)]
struct libc_sockaddr_in {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

fn loopback_sockaddr() -> libc_sockaddr_in {
    libc_sockaddr_in {
        sin_family: 2, // AF_INET
        sin_port: 443u16.to_be(),
        sin_addr: u32::from_be_bytes([127, 0, 0, 1]).to_be(),
        sin_zero: [0; 8],
    }
}

fn default_settings() -> ngtcp2_settings {
    unsafe {
        let mut settings = MaybeUninit::<ngtcp2_settings>::uninit();
        ngtcp2_settings_default_versioned(NGTCP2_SETTINGS_VERSION as i32, settings.as_mut_ptr());
        settings.assume_init()
    }
}

fn default_transport_params() -> ngtcp2_transport_params {
    unsafe {
        let mut params = MaybeUninit::<ngtcp2_transport_params>::uninit();
        ngtcp2_transport_params_default_versioned(
            NGTCP2_TRANSPORT_PARAMS_VERSION as i32,
            params.as_mut_ptr(),
        );
        params.assume_init()
    }
}

/// The minimum set of callbacks ngtcp2 asserts a client has.
///
/// None of them is ever called: the connection is built and dropped without a
/// packet being fed to it. They exist because the constructor checks for their
/// presence, which is itself worth exercising — it is the check that would fail
/// if `ngtcp2_callbacks` were laid out differently than the header says.
fn client_callbacks() -> ngtcp2_callbacks {
    let mut callbacks: ngtcp2_callbacks = unsafe { std::mem::zeroed() };
    callbacks.client_initial = Some(stub_client_initial);
    callbacks.recv_crypto_data = Some(stub_recv_crypto_data);
    callbacks.encrypt = Some(stub_encrypt);
    callbacks.decrypt = Some(stub_decrypt);
    callbacks.hp_mask = Some(stub_hp_mask);
    callbacks.recv_retry = Some(stub_recv_retry);
    callbacks.rand = Some(fill_with_zeroes);
    callbacks.get_new_connection_id = Some(stub_get_new_connection_id);
    callbacks.update_key = Some(stub_update_key);
    callbacks.delete_crypto_aead_ctx = Some(stub_delete_crypto_aead_ctx);
    callbacks.delete_crypto_cipher_ctx = Some(stub_delete_crypto_cipher_ctx);
    callbacks.get_path_challenge_data = Some(stub_get_path_challenge_data);
    callbacks
}

const FATAL: i32 = NGTCP2_ERR_CALLBACK_FAILURE;

unsafe extern "C" fn stub_client_initial(_: *mut ngtcp2_conn, _: *mut std::ffi::c_void) -> i32 {
    FATAL
}

unsafe extern "C" fn stub_recv_crypto_data(
    _: *mut ngtcp2_conn,
    _: ngtcp2_encryption_level,
    _: u64,
    _: *const u8,
    _: usize,
    _: *mut std::ffi::c_void,
) -> i32 {
    FATAL
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn stub_encrypt(
    _: *mut u8,
    _: *const ngtcp2_crypto_aead,
    _: *const ngtcp2_crypto_aead_ctx,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
) -> i32 {
    FATAL
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn stub_decrypt(
    _: *mut u8,
    _: *const ngtcp2_crypto_aead,
    _: *const ngtcp2_crypto_aead_ctx,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
) -> i32 {
    FATAL
}

unsafe extern "C" fn stub_hp_mask(
    _: *mut u8,
    _: *const ngtcp2_crypto_cipher,
    _: *const ngtcp2_crypto_cipher_ctx,
    _: *const u8,
) -> i32 {
    FATAL
}

unsafe extern "C" fn stub_recv_retry(
    _: *mut ngtcp2_conn,
    _: *const ngtcp2_pkt_hd,
    _: *mut std::ffi::c_void,
) -> i32 {
    FATAL
}

unsafe extern "C" fn stub_get_new_connection_id(
    _: *mut ngtcp2_conn,
    _: *mut ngtcp2_cid,
    _: *mut u8,
    _: usize,
    _: *mut std::ffi::c_void,
) -> i32 {
    FATAL
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn stub_update_key(
    _: *mut ngtcp2_conn,
    _: *mut u8,
    _: *mut u8,
    _: *mut ngtcp2_crypto_aead_ctx,
    _: *mut u8,
    _: *mut ngtcp2_crypto_aead_ctx,
    _: *mut u8,
    _: *const u8,
    _: *const u8,
    _: usize,
    _: *mut std::ffi::c_void,
) -> i32 {
    FATAL
}

unsafe extern "C" fn stub_delete_crypto_aead_ctx(
    _: *mut ngtcp2_conn,
    _: *mut ngtcp2_crypto_aead_ctx,
    _: *mut std::ffi::c_void,
) {
}

unsafe extern "C" fn stub_delete_crypto_cipher_ctx(
    _: *mut ngtcp2_conn,
    _: *mut ngtcp2_crypto_cipher_ctx,
    _: *mut std::ffi::c_void,
) {
}

unsafe extern "C" fn stub_get_path_challenge_data(
    _: *mut ngtcp2_conn,
    _: *mut u8,
    _: *mut std::ffi::c_void,
) -> i32 {
    FATAL
}

/// ngtcp2 asks for unpredictable bytes when it needs them. Determinism is
/// preferable in a test, and nothing here depends on the values.
unsafe extern "C" fn fill_with_zeroes(dest: *mut u8, destlen: usize, _: *const ngtcp2_rand_ctx) {
    unsafe { ptr::write_bytes(dest, 0, destlen) }
}
