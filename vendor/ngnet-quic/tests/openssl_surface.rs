//! Proof that the primitives a TLS backend is built from are actually linked.
//!
//! # Why this crate binds so little of OpenSSL
//!
//! The obvious way to move the TLS seam off ngtcp2's C crypto helper is to bind OpenSSL's
//! EVP surface and reimplement what the helper was doing: AEAD, header protection, HKDF,
//! and the QUIC key schedule. That is around sixty symbols and, far worse, it is a
//! reimplementation of cryptography whose failure mode is silent.
//!
//! It is also unnecessary. Almost everything the helper does is exposed as plain functions
//! taking no `ngtcp2_conn` — `ngtcp2_crypto_encrypt`, `hp_mask`, `hkdf_expand_label`,
//! `derive_initial_secrets` — so a backend can call them directly. Only one part genuinely
//! has to be replaced: the helper's handshake driver recovers the `ngtcp2_conn` from the
//! `SSL`'s application data, and that coupling is precisely what forces this crate's TLS
//! seam to be `unsafe`.
//!
//! So the split is: OpenSSL's QUIC-TLS record layer is bound here and driven by us; every
//! cryptographic primitive stays with the helper. This file names both halves in one place,
//! so a missing allowlist entry fails immediately rather than at whichever call site first
//! reaches for it.
//!
//! # Why the addresses are collected rather than the names merely mentioned
//!
//! A bare `let _ = sys::ngtcp2_crypto_encrypt;` binding is optimised away before a
//! relocation is emitted, so it proves the *declaration* exists and nothing about the
//! *symbol*. Taking the address forces the linker to resolve it, which is the thing
//! actually in question. `versioned_ffi.rs` makes the same argument about ngtcp2's
//! versioned wrappers.
//!
//! None of this needs `unsafe`: casting a function item to a raw pointer is safe, and only
//! *calling* it would not be. That is why this file is absent from the exemption list in
//! `invariants.rs`.

#![cfg(feature = "tls-ossl")]

use ngnet_quic_sys as sys;

/// The QUIC-TLS record layer — the one part of the C helper that has to be replaced.
///
/// These arrived in OpenSSL 3.5, which the build script already requires: it refuses older
/// OpenSSL, quictls and LibreSSL precisely because `SSL_set_quic_tls_cbs` would be absent.
/// A failure here therefore means the allowlist is wrong, not that the wrong OpenSSL was
/// found.
#[test]
fn the_quic_tls_record_layer_is_linkable() {
    let addresses = [
        sys::SSL_set_quic_tls_cbs as *const () as usize,
        sys::SSL_set_quic_tls_transport_params as *const () as usize,
        sys::SSL_do_handshake as *const () as usize,
        // Not a mistake in a QUIC stack: this is how post-handshake messages get processed
        // once the handshake itself is done. Omitting it breaks session tickets and key
        // updates, silently, long after any handshake test has passed.
        sys::SSL_read as *const () as usize,
    ];

    for address in addresses {
        assert_ne!(address, 0, "a QUIC-TLS symbol resolved to a null address");
    }
}

/// Packet protection, taken from the helper rather than reimplemented.
///
/// ngtcp2's `encrypt`/`decrypt`/`hp_mask` callbacks receive neither the connection nor any
/// user pointer (`ngtcp2.h:2824`, `:2853`, `:2882`), so key state has to travel inside the
/// cipher context. These are the functions the seam's key objects wrap.
#[test]
fn the_packet_protection_primitives_are_linkable() {
    let addresses = [
        sys::ngtcp2_crypto_encrypt as *const () as usize,
        sys::ngtcp2_crypto_decrypt as *const () as usize,
        sys::ngtcp2_crypto_hp_mask as *const () as usize,
        sys::ngtcp2_crypto_aead_ctx_encrypt_init as *const () as usize,
        sys::ngtcp2_crypto_aead_ctx_decrypt_init as *const () as usize,
        sys::ngtcp2_crypto_aead_ctx_free as *const () as usize,
        sys::ngtcp2_crypto_cipher_ctx_encrypt_init as *const () as usize,
        sys::ngtcp2_crypto_cipher_ctx_free as *const () as usize,
        sys::ngtcp2_crypto_aead_init as *const () as usize,
    ];

    for address in addresses {
        assert_ne!(
            address, 0,
            "a packet protection symbol resolved to a null address"
        );
    }
}

/// The QUIC key schedule, likewise taken rather than rewritten.
///
/// This is the part worth being most careful about. QUIC derives Initial secrets outside
/// the TLS handshake entirely, from the client's destination connection identifier and a
/// version-specific salt. A subtly wrong salt or label yields two endpoints that agree with
/// each other and with nobody else — which every test in this repository would pass,
/// because both ends are built from this same code. Not writing it is the strongest
/// available mitigation.
///
/// Several of these are declared in the helper's internal `shared.h`, which the install
/// step does not publish. bindgen reads the vendored header directly, so a signature that
/// changes upstream is a compile error here rather than a silent argument mismatch.
#[test]
fn the_key_schedule_is_linkable() {
    let addresses = [
        sys::ngtcp2_crypto_derive_initial_secrets as *const () as usize,
        sys::ngtcp2_crypto_derive_packet_protection_key as *const () as usize,
        sys::ngtcp2_crypto_hkdf_expand_label as *const () as usize,
        sys::ngtcp2_crypto_hkdf_extract as *const () as usize,
        sys::ngtcp2_crypto_hkdf_expand as *const () as usize,
        sys::ngtcp2_crypto_update_traffic_secret as *const () as usize,
        sys::ngtcp2_crypto_packet_protection_ivlen as *const () as usize,
        sys::ngtcp2_crypto_aead_keylen as *const () as usize,
        sys::ngtcp2_crypto_aead_noncelen as *const () as usize,
        sys::ngtcp2_crypto_md_hashlen as *const () as usize,
    ];

    for address in addresses {
        assert_ne!(
            address, 0,
            "a key schedule symbol resolved to a null address"
        );
    }
}

/// Cipher suite discovery, which also supplies the AEAD usage limits.
///
/// `ngtcp2_crypto_ctx_tls` reads the negotiated suite off the TLS session and fills in the
/// AEAD, the message digest, the header protection cipher **and** `max_encryption` /
/// `max_decryption_failure`. Those last two are easy to overlook: ngtcp2 needs both, and
/// leaving them zero makes the first failed decryption fatal and forces an immediate key
/// update. Taking them from here means they cannot be forgotten.
#[test]
fn cipher_suite_discovery_is_linkable() {
    let addresses = [
        sys::ngtcp2_crypto_ctx_tls as *const () as usize,
        sys::ngtcp2_crypto_ctx_tls_early as *const () as usize,
    ];

    for address in addresses {
        assert_ne!(
            address, 0,
            "a suite discovery symbol resolved to a null address"
        );
    }
}

/// The dispatch slot identifiers, and the protection levels secrets are reported against.
///
/// Constants rather than symbols, so the failure mode is a compile error rather than a null
/// address — but the reason for naming them is the same. They are asserted for distinctness
/// rather than for particular values, because the values are OpenSSL's to choose; what this
/// crate depends on is that there are six separate slots and four separate levels to map
/// onto ngtcp2's encryption levels.
#[test]
fn every_dispatch_slot_and_protection_level_is_distinct() {
    let slots = [
        sys::OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_SEND,
        sys::OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_RECV_RCD,
        sys::OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_RELEASE_RCD,
        sys::OSSL_FUNC_SSL_QUIC_TLS_YIELD_SECRET,
        sys::OSSL_FUNC_SSL_QUIC_TLS_GOT_TRANSPORT_PARAMS,
        sys::OSSL_FUNC_SSL_QUIC_TLS_ALERT,
    ];
    let mut sorted = slots;
    sorted.sort_unstable();
    assert!(
        sorted.windows(2).all(|pair| pair[0] != pair[1]),
        "two QUIC-TLS dispatch slots share an identifier"
    );

    let levels = [
        sys::OSSL_RECORD_PROTECTION_LEVEL_NONE,
        sys::OSSL_RECORD_PROTECTION_LEVEL_EARLY,
        sys::OSSL_RECORD_PROTECTION_LEVEL_HANDSHAKE,
        sys::OSSL_RECORD_PROTECTION_LEVEL_APPLICATION,
    ];
    let mut sorted = levels;
    sorted.sort_unstable();
    assert!(
        sorted.windows(2).all(|pair| pair[0] != pair[1]),
        "two OpenSSL protection levels share a value, so the mapping onto ngtcp2's \
         encryption levels cannot be one-to-one"
    );
}

/// The AEAD usage limits the helper defines, which QUIC requires per cipher.
///
/// Named here so that AES-CCM stays in view: the helper special-cases it in three separate
/// places, and its limits are not powers of two like the others but a specific packet count
/// from its confidentiality analysis. Dropping AES-CCM during the conversion would be an
/// omission rather than a decision.
#[test]
fn the_aead_usage_limits_are_defined() {
    assert_eq!(
        sys::NGTCP2_CRYPTO_MAX_ENCRYPTION_AES_GCM,
        1u64 << 23,
        "AES-GCM confidentiality limit"
    );
    assert_eq!(
        sys::NGTCP2_CRYPTO_MAX_ENCRYPTION_CHACHA20_POLY1305,
        1u64 << 62,
        "ChaCha20-Poly1305 confidentiality limit"
    );
    assert_eq!(
        sys::NGTCP2_CRYPTO_MAX_ENCRYPTION_AES_CCM,
        2_965_820,
        "AES-CCM confidentiality limit"
    );
    assert_eq!(
        sys::NGTCP2_CRYPTO_MAX_DECRYPTION_FAILURE_AES_GCM,
        1u64 << 52,
        "AES-GCM integrity limit"
    );
    assert_eq!(
        sys::NGTCP2_CRYPTO_MAX_DECRYPTION_FAILURE_CHACHA20_POLY1305,
        1u64 << 36,
        "ChaCha20-Poly1305 integrity limit"
    );
    assert_eq!(
        sys::NGTCP2_CRYPTO_MAX_DECRYPTION_FAILURE_AES_CCM,
        2_965_820,
        "AES-CCM integrity limit"
    );
}
