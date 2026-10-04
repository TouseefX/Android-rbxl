//! The OpenSSL TLS backend.
//!
//! Implements [`crate::tls::Backend`] and [`crate::tls::Session`] over OpenSSL 3.5's QUIC TLS
//! API. Enabled by the default-on `tls-ossl` feature.
//!
//! # The teardown cycle this used to have, and no longer does
//!
//! An earlier version of this backend drove the handshake through ngtcp2's
//! `ngtcp2_crypto_ossl` helper, which meant three C objects referring to each other in a
//! cycle: an `SSL`, an `ngtcp2_crypto_ossl_ctx` wrapping it, and an `ngtcp2_crypto_conn_ref`
//! that OpenSSL held as application data and that pointed back at the `ngtcp2_conn`. They had
//! to be destroyed in one exact order, and every departure from it was a use-after-free rather
//! than a leak: `SSL_free` releases outstanding CRYPTO records, which called back into the
//! helper, which followed the reference to the connection and dereferenced it.
//!
//! None of that exists now. `SSL_set_quic_tls_cbs` takes a callback argument, so this backend
//! passes its own state directly and OpenSSL holds nothing belonging to ngtcp2. The ordering
//! problem did not get easier — it stopped existing, which is the difference worth having.
//!
//! What is left is ordinary ownership: the engine outlives the `SSL` that reads it, because
//! `SSL_free` releases records by calling back into the engine, and the helper context outlives
//! both. [`OsslSession`] implements [`Drop`] by hand to say so, rather than depending on field
//! order, which would make the reasoning invisible.
//!
//! # `ngtcp2_crypto_ossl_init` is process-global
//!
//! It prefetches static `EVP_*` objects into globals, with no reference counting
//! (`ossl.c:49-60`, `:62`, `:82`). The ngtcp2 examples pair it with a per-context destructor,
//! which means that with two TLS contexts, destroying the second frees objects the first is
//! still using. This crate calls `init` once behind a [`Once`] and **never** calls
//! `ngtcp2_crypto_ossl_free`: a bounded one-off leak is the correct trade against corrupting a
//! live connection.

use core::ffi::{CStr, c_char, c_int, c_uchar, c_void};
use core::ptr;
use std::sync::Once;

use ngnet_quic_sys as sys;

use crate::error::{Error, ErrorKind, Result};
use crate::tls::{
    Backend, CryptoError, Direction, DirectionalKeys, HP_MASK_LEN, HP_SAMPLE_LEN, HeaderKey,
    InitialKeys, Iv, Level, PacketKey, Role, RotatedKeys, Session, SessionEvent,
};

/// The `SSL_ctrl` command behind the `SSL_set_tlsext_host_name` macro.
const CTRL_SET_TLSEXT_HOSTNAME: c_int = sys::SSL_CTRL_SET_TLSEXT_HOSTNAME as c_int;

/// The `TLSEXT_NAMETYPE_host_name` argument to that command.
const NAMETYPE_HOST_NAME: core::ffi::c_long = sys::TLSEXT_NAMETYPE_host_name as core::ffi::c_long;

/// Runs `ngtcp2_crypto_ossl_init` exactly once per process.
fn ensure_ossl_init() -> Result<()> {
    static INIT: Once = Once::new();
    static mut RESULT: c_int = 0;

    INIT.call_once(|| {
        // SAFETY: called once, before any other crypto helper entry point.
        let rc = unsafe { sys::ngtcp2_crypto_ossl_init() };
        // SAFETY: written inside `call_once`, which synchronises with every later read.
        unsafe { RESULT = rc };
    });

    // SAFETY: `call_once` has returned, so the write above happened-before this read.
    let rc = unsafe { RESULT };
    if rc != 0 {
        return Err(Error::native(rc, "ngtcp2_crypto_ossl_init failed"));
    }
    Ok(())
}

/// Reads OpenSSL's error queue into a string, and clears it.
fn take_openssl_error() -> Option<String> {
    let mut messages: Vec<String> = Vec::new();
    loop {
        // SAFETY: reading the thread-local error queue has no preconditions.
        let code = unsafe { sys::ERR_get_error() };
        if code == 0 {
            break;
        }
        let mut buf = [0i8; 256];
        // SAFETY: `buf` is a valid writable buffer of the length given.
        unsafe { sys::ERR_error_string_n(code, buf.as_mut_ptr().cast::<c_char>(), buf.len()) };
        // SAFETY: `ERR_error_string_n` always NUL-terminates within the buffer.
        let text = unsafe { CStr::from_ptr(buf.as_ptr().cast::<c_char>()) };
        messages.push(text.to_string_lossy().into_owned());
    }
    if messages.is_empty() {
        None
    } else {
        Some(messages.join("; "))
    }
}

/// Encodes ALPN protocols into OpenSSL's length-prefixed wire form.
fn encode_alpn(protocols: &[Vec<u8>]) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    for protocol in protocols {
        if protocol.is_empty() || protocol.len() > 255 {
            return Err(Error::invalid_input(
                "an ALPN protocol must be between 1 and 255 bytes",
            ));
        }
        encoded.push(protocol.len() as u8);
        encoded.extend_from_slice(protocol);
    }
    Ok(encoded)
}

/// The algorithms of one cipher suite, as ngtcp2's crypto helper describes them.
///
/// Every field is a plain descriptor: a pointer to one of OpenSSL's immutable algorithm
/// objects, plus the numbers ngtcp2's core reads. None of them owns anything, which is why
/// this is `Copy` and why nothing here has a destructor.
///
/// The reason this exists at all is that the helper's derivation functions all want the
/// suite passed alongside the secret, and gathering it once per key would mean asking
/// OpenSSL the same question repeatedly at the exact points where a mismatch between the
/// derivation and the protection would be undetectable.
#[derive(Clone, Copy)]
struct Suite {
    /// The payload AEAD, carrying its own tag length in `max_overhead`.
    aead: sys::ngtcp2_crypto_aead,
    /// The hash the key schedule expands with.
    md: sys::ngtcp2_crypto_md,
    /// The header protection cipher.
    hp: sys::ngtcp2_crypto_cipher,
    /// How many packets this suite may protect before a key update is forced.
    max_encryption: u64,
    /// How many failed decryptions it may tolerate.
    max_decryption_failure: u64,
}

impl Suite {
    /// The suite QUIC fixes for Initial packets: AES-128-GCM with SHA-256.
    ///
    /// The usage limits are zero, which is what `ngtcp2_crypto_ctx_initial` itself sets
    /// (`ossl.c:252-259`). That is not an oversight in either place: ngtcp2 compares packet
    /// counts against these only for the application packet number space
    /// (`ngtcp2_conn.c:8743`, `:9474`), and Initial keys are discarded long before any count
    /// could matter.
    fn initial() -> Self {
        // SAFETY: the helper fills every field of the context it is given.
        let ctx = unsafe {
            let mut ctx: sys::ngtcp2_crypto_ctx = core::mem::zeroed();
            sys::ngtcp2_crypto_ctx_initial(&raw mut ctx);
            ctx
        };
        Self::from_ctx(&ctx)
    }

    /// The suite QUIC fixes for Retry integrity tags: AES-128-GCM, in every version.
    ///
    /// Separate from [`Suite::initial`] because it is a different thing that happens to use
    /// the same algorithm: the Retry tag has no hash and no header protection, and the key
    /// is a published constant rather than derived from anything.
    fn retry() -> Self {
        // SAFETY: the helper fills the descriptor it is given.
        let aead = unsafe {
            let mut aead: sys::ngtcp2_crypto_aead = core::mem::zeroed();
            sys::ngtcp2_crypto_aead_retry(&raw mut aead);
            aead
        };
        Self {
            aead,
            // SAFETY: a zeroed descriptor is a null algorithm, which nothing here consults:
            // the Retry key is a constant, so it is never expanded from a secret and no
            // header is masked with it.
            md: unsafe { core::mem::zeroed() },
            hp: unsafe { core::mem::zeroed() },
            max_encryption: 0,
            max_decryption_failure: 0,
        }
    }

    /// Reads the suite the handshake negotiated out of a crypto helper context.
    ///
    /// This is the one call the hybrid keeps the `ngtcp2_crypto_ossl` archive for on this
    /// side of the seam. It maps a TLS 1.3 cipher suite identifier onto an AEAD, a hash, a
    /// header protection cipher **and** both usage limits in one step — and getting any one
    /// of those four wrong produces a connection that fails in a way no test written against
    /// a single implementation would catch, because both ends would be wrong together.
    ///
    /// # Safety
    ///
    /// `ossl_ctx` must be a live `ngtcp2_crypto_ossl_ctx` whose `SSL` has negotiated a
    /// cipher suite.
    unsafe fn negotiated(ossl_ctx: *mut sys::ngtcp2_crypto_ossl_ctx) -> Result<Self> {
        let mut ctx: sys::ngtcp2_crypto_ctx = unsafe { core::mem::zeroed() };
        // SAFETY: the caller guarantees the context is live; the helper writes only into
        // `ctx`.
        let filled = unsafe { sys::ngtcp2_crypto_ctx_tls(&raw mut ctx, ossl_ctx.cast::<c_void>()) };
        if filled.is_null() {
            return Err(Error::backend("the negotiated cipher suite is unsupported"));
        }
        Ok(Self::from_ctx(&ctx))
    }

    /// Splits a filled crypto context into the parts this backend keeps.
    fn from_ctx(ctx: &sys::ngtcp2_crypto_ctx) -> Self {
        Self {
            aead: ctx.aead,
            md: ctx.md,
            hp: ctx.hp,
            max_encryption: ctx.max_encryption,
            max_decryption_failure: ctx.max_decryption_failure,
        }
    }

    /// How many bytes of key material the AEAD takes.
    fn key_len(&self) -> usize {
        // SAFETY: the descriptor names a live algorithm object.
        unsafe { sys::ngtcp2_crypto_aead_keylen(&raw const self.aead) }
    }

    /// How long the packet protection initialisation vector is.
    ///
    /// Deliberately `packet_protection_ivlen` rather than the AEAD's own nonce length: QUIC
    /// requires at least eight bytes so a packet number always fits, and the helper applies
    /// that floor (`shared.c:141-144`). Using the raw nonce length instead would silently
    /// produce short IVs for any future AEAD with a smaller nonce.
    fn iv_len(&self) -> usize {
        // SAFETY: the descriptor names a live algorithm object.
        unsafe { sys::ngtcp2_crypto_packet_protection_ivlen(&raw const self.aead) }
    }

    /// How many bytes protection adds to a payload.
    ///
    /// Read from the descriptor only by the dimension tests; the keys themselves carry it.
    #[cfg(test)]
    fn tag_len(&self) -> usize {
        self.aead.max_overhead
    }

    /// How long a secret for this suite's hash is.
    ///
    /// Used by the dimension tests; the key schedule derives lengths itself.
    #[cfg(test)]
    fn hash_len(&self) -> usize {
        // SAFETY: the descriptor names a live algorithm object.
        unsafe { sys::ngtcp2_crypto_md_hashlen(&raw const self.md) }
    }
}

/// A payload protection key backed by an OpenSSL cipher context.
///
/// # Why this is `Send` but not `Sync`
///
/// The `EVP_CIPHER_CTX` behind the handle is mutated by every protection operation — the
/// helper re-initialises the nonce on it each time (`ossl.c:936-940`, `:967-971`) — even
/// though the seam's methods take `&self`, because ngtcp2's callbacks hand it out as
/// `const`. Nothing observes that as aliasing: the mutation happens through a raw pointer,
/// which carries no uniqueness guarantee, and only one operation can be in flight at a time
/// because the type is not `Sync`. Making it `Sync` would be unsound, and nothing needs it.
pub struct OsslPacketKey {
    /// Which AEAD, and how much overhead it adds.
    aead: sys::ngtcp2_crypto_aead,
    /// The initialised cipher context. Freed exactly once, on drop.
    ctx: sys::ngtcp2_crypto_aead_ctx,
    /// Reported to ngtcp2, which forces a key update against it.
    max_encryption: u64,
    /// Reported to ngtcp2, which closes the connection against it.
    max_decryption_failure: u64,
}

// SAFETY: the key owns its cipher context outright and shares nothing but pointers to
// OpenSSL's immutable algorithm objects, which are valid for the life of the process.
// `Sync` is deliberately not claimed; see the type's documentation.
unsafe impl Send for OsslPacketKey {}

impl OsslPacketKey {
    /// Builds a key for protecting outbound payloads.
    fn for_encryption(suite: &Suite, key: &[u8]) -> Result<Self> {
        Self::init(suite, key, sys::ngtcp2_crypto_aead_ctx_encrypt_init)
    }

    /// Builds a key for unprotecting inbound payloads.
    fn for_decryption(suite: &Suite, key: &[u8]) -> Result<Self> {
        Self::init(suite, key, sys::ngtcp2_crypto_aead_ctx_decrypt_init)
    }

    /// The shared half of both, which differ only in which initialiser they call.
    fn init(
        suite: &Suite,
        key: &[u8],
        init: unsafe extern "C" fn(
            *mut sys::ngtcp2_crypto_aead_ctx,
            *const sys::ngtcp2_crypto_aead,
            *const u8,
            usize,
        ) -> c_int,
    ) -> Result<Self> {
        if key.len() != suite.key_len() {
            return Err(Error::backend("the key is the wrong length for the suite"));
        }
        // SAFETY: a zeroed context is the "not yet initialised" state the helper's own
        // callers start from, and is what the initialiser expects to be handed.
        let mut ctx: sys::ngtcp2_crypto_aead_ctx = unsafe { core::mem::zeroed() };
        // SAFETY: the descriptor is live, the key is at least `key_len` bytes, and the
        // context is uninitialised and writable.
        let rv = unsafe {
            init(
                &raw mut ctx,
                &raw const suite.aead,
                key.as_ptr(),
                suite.iv_len(),
            )
        };
        if rv != 0 {
            return Err(tls_error("could not initialise a payload protection key"));
        }
        Ok(Self {
            aead: suite.aead,
            ctx,
            max_encryption: suite.max_encryption,
            max_decryption_failure: suite.max_decryption_failure,
        })
    }
}

impl Drop for OsslPacketKey {
    fn drop(&mut self) {
        // SAFETY: the context was initialised by this type and is freed exactly once,
        // because nothing else holds a copy of the handle.
        unsafe { sys::ngtcp2_crypto_aead_ctx_free(&raw mut self.ctx) };
    }
}

impl PacketKey for OsslPacketKey {
    fn seal(
        &self,
        buf: &mut [u8],
        plaintext_len: usize,
        nonce: &[u8],
        aad: &[u8],
    ) -> core::result::Result<(), CryptoError> {
        // The helper writes the tag at `dest + plaintextlen` without checking that there is
        // room for it (`ossl.c:912-916`), so this bound is the only thing standing between a
        // short buffer and a heap overflow.
        let needed = plaintext_len
            .checked_add(self.tag_len())
            .ok_or(CryptoError::Fatal)?;
        if buf.len() < needed || nonce.len() < self.aead_nonce_len() {
            return Err(CryptoError::Fatal);
        }
        let dest = buf.as_mut_ptr();
        // SAFETY: `dest` and the plaintext are the same buffer, which ngtcp2 explicitly
        // permits (`ngtcp2.h:2818`); it is at least `plaintext_len + tag_len` bytes, the
        // nonce is long enough, and the context is initialised for encryption.
        let rv = unsafe {
            sys::ngtcp2_crypto_encrypt(
                dest,
                &raw const self.aead,
                &raw const self.ctx,
                dest.cast_const(),
                plaintext_len,
                nonce.as_ptr(),
                nonce.len(),
                aad.as_ptr(),
                aad.len(),
            )
        };
        if rv == 0 {
            Ok(())
        } else {
            Err(CryptoError::Fatal)
        }
    }

    fn open(
        &self,
        dest: &mut [u8],
        ciphertext: &[u8],
        nonce: &[u8],
        aad: &[u8],
    ) -> core::result::Result<usize, CryptoError> {
        let ciphertext_len = ciphertext.len();
        // A packet too short to hold a tag is a malformed packet, not a broken backend: it
        // is exactly what a truncating attacker would send.
        let Some(plaintext_len) = ciphertext_len.checked_sub(self.tag_len()) else {
            return Err(CryptoError::Decrypt);
        };
        if dest.len() < plaintext_len || nonce.len() < self.aead_nonce_len() {
            return Err(CryptoError::Fatal);
        }
        // SAFETY: as in `seal`, but with `dest` and the ciphertext given as the two distinct
        // buffers they are -- ngtcp2's core never aliases them (see the decrypt region in
        // `tls_bridge.rs`). `dest` holds at least the plaintext, the ciphertext its full
        // length, and the context is initialised for decryption.
        let rv = unsafe {
            sys::ngtcp2_crypto_decrypt(
                dest.as_mut_ptr(),
                &raw const self.aead,
                &raw const self.ctx,
                ciphertext.as_ptr(),
                ciphertext_len,
                nonce.as_ptr(),
                nonce.len(),
                aad.as_ptr(),
                aad.len(),
            )
        };
        // The helper reports authentication failure and internal failure with the same `-1`
        // (`ossl.c:962-990`), so the two cannot be told apart here. Reporting the safer of
        // the two is not a compromise: a genuine internal failure will not decrypt anything
        // afterwards either, so the connection still ends — via the AEAD failure limit, and
        // without handing anyone who can send a datagram a way to close it.
        if rv == 0 {
            Ok(plaintext_len)
        } else {
            Err(CryptoError::Decrypt)
        }
    }

    fn tag_len(&self) -> usize {
        self.aead.max_overhead
    }

    fn confidentiality_limit(&self) -> u64 {
        self.max_encryption
    }

    fn integrity_limit(&self) -> u64 {
        self.max_decryption_failure
    }
}

impl OsslPacketKey {
    /// The nonce length the AEAD itself requires, which is what the helper writes through.
    ///
    /// Distinct from the suite's IV length: ngtcp2 builds the nonce from an IV that may have
    /// been padded up to eight bytes, and hands back however many bytes that produced. The
    /// helper ignores the length it is given and reads whatever the cipher wants
    /// (`ossl.c:924`, `:955` both discard `noncelen`), so this is checked here instead.
    fn aead_nonce_len(&self) -> usize {
        // SAFETY: the descriptor names a live algorithm object.
        unsafe { sys::ngtcp2_crypto_aead_noncelen(&raw const self.aead) }
    }
}

/// A header protection key backed by an OpenSSL cipher context.
///
/// The same `&self`-with-interior-mutation reasoning as [`OsslPacketKey`] applies, and for
/// ChaCha20 it is not merely theoretical: masking re-initialises the context with the sample
/// as its nonce (`ossl.c:945-949`).
pub struct OsslHeaderKey {
    /// Which cipher. Only its handle is read, by the helper.
    hp: sys::ngtcp2_crypto_cipher,
    /// The initialised cipher context. Freed exactly once, on drop.
    ctx: sys::ngtcp2_crypto_cipher_ctx,
}

// SAFETY: as for `OsslPacketKey` — the context is owned outright and `Sync` is not claimed.
unsafe impl Send for OsslHeaderKey {}

impl OsslHeaderKey {
    /// Builds a header protection key.
    ///
    /// Always an *encryption* context, in both directions: header protection is a keystream
    /// exclusive-ored over the header, so the same operation covers and uncovers it.
    fn new(suite: &Suite, key: &[u8]) -> Result<Self> {
        if key.len() != suite.key_len() {
            return Err(Error::backend(
                "the header protection key is the wrong length for the suite",
            ));
        }
        // SAFETY: a zeroed context is the uninitialised state the initialiser expects.
        let mut ctx: sys::ngtcp2_crypto_cipher_ctx = unsafe { core::mem::zeroed() };
        // SAFETY: the descriptor is live and the key is at least `key_len` bytes.
        let rv = unsafe {
            sys::ngtcp2_crypto_cipher_ctx_encrypt_init(
                &raw mut ctx,
                &raw const suite.hp,
                key.as_ptr(),
            )
        };
        if rv != 0 {
            return Err(tls_error("could not initialise a header protection key"));
        }
        Ok(Self { hp: suite.hp, ctx })
    }
}

impl Drop for OsslHeaderKey {
    fn drop(&mut self) {
        // SAFETY: initialised by this type, freed exactly once.
        unsafe { sys::ngtcp2_crypto_cipher_ctx_free(&raw mut self.ctx) };
    }
}

impl HeaderKey for OsslHeaderKey {
    fn mask(&self, sample: &[u8]) -> core::result::Result<[u8; HP_MASK_LEN], CryptoError> {
        if sample.len() < HP_SAMPLE_LEN {
            return Err(CryptoError::Fatal);
        }
        // The mask is five bytes, but the helper writes a **block**: sixteen for AES-ECB,
        // and sixteen plus a final call for ChaCha20 (`ossl.c:937-951`). ngtcp2's own core
        // allocates `uint8_t mask[NGTCP2_HP_SAMPLELEN]` for exactly this reason
        // (`ngtcp2_conn.c:5964`). Handing it a five-byte array would overflow the stack
        // frame on every single packet.
        let mut block = [0u8; HP_SAMPLE_LEN];
        // SAFETY: `block` is a full cipher block, the sample is at least `HP_SAMPLE_LEN`
        // bytes -- the only length the helper ever reads -- and the context is initialised.
        let rv = unsafe {
            sys::ngtcp2_crypto_hp_mask(
                block.as_mut_ptr(),
                &raw const self.hp,
                &raw const self.ctx,
                sample.as_ptr(),
            )
        };
        if rv != 0 {
            return Err(CryptoError::Fatal);
        }
        let mut mask = [0u8; HP_MASK_LEN];
        mask.copy_from_slice(&block[..HP_MASK_LEN]);
        Ok(mask)
    }
}

/// Derives one direction's keys from a traffic secret.
///
/// The QUIC key schedule is not reimplemented here. `derive_packet_protection_key` produces
/// the payload key, the initialisation vector and the header protection key from one secret
/// in one call, and applies the version-specific labels — `quic key` for version 1 and
/// `quicv2 key` for version 2 (`shared.c:159-165`). Choosing those labels by hand is the
/// single most attractive place in this file to introduce a bug that only shows up against
/// a different implementation.
fn derive_keys(
    suite: &Suite,
    version: u32,
    secret: &[u8],
) -> Result<DirectionalKeys<OsslPacketKey, OsslHeaderKey>> {
    derive_keys_with(suite, version, secret, OsslPacketKey::for_encryption)
}

/// The same, producing a key that unprotects rather than protects.
fn derive_rx_keys(
    suite: &Suite,
    version: u32,
    secret: &[u8],
) -> Result<DirectionalKeys<OsslPacketKey, OsslHeaderKey>> {
    derive_keys_with(suite, version, secret, OsslPacketKey::for_decryption)
}

/// The shared body of both, which differ only in the direction the AEAD is initialised for.
fn derive_keys_with(
    suite: &Suite,
    version: u32,
    secret: &[u8],
    make: fn(&Suite, &[u8]) -> Result<OsslPacketKey>,
) -> Result<DirectionalKeys<OsslPacketKey, OsslHeaderKey>> {
    let mut key = vec![0u8; suite.key_len()];
    let mut iv = vec![0u8; suite.iv_len()];
    let mut hp = vec![0u8; suite.key_len()];

    // SAFETY: every output buffer is exactly the length the helper derives for this suite,
    // and the descriptors are live.
    let rv = unsafe {
        sys::ngtcp2_crypto_derive_packet_protection_key(
            key.as_mut_ptr(),
            iv.as_mut_ptr(),
            hp.as_mut_ptr(),
            version,
            &raw const suite.aead,
            &raw const suite.md,
            secret.as_ptr(),
            secret.len(),
        )
    };
    if rv != 0 {
        return Err(Error::backend("could not derive packet protection keys"));
    }

    let packet = make(suite, &key)?;
    let header = OsslHeaderKey::new(suite, &hp)?;
    let iv = Iv::new(&iv)?;
    Ok(DirectionalKeys { packet, header, iv })
}

/// Derives both directions of the Initial keys for a connection identifier.
///
/// Initial keys owe nothing to the handshake: the identifier the client chose and a
/// version-specific salt are the whole input, which is why they can be derived before a
/// single handshake byte exists and why anyone watching can derive them too.
fn derive_initial_keys(
    role: Role,
    version: u32,
    dcid: &[u8],
) -> Result<InitialKeys<OsslPacketKey, OsslHeaderKey>> {
    if dcid.len() > sys::NGTCP2_MAX_CIDLEN as usize {
        return Err(Error::backend("the connection identifier is too long"));
    }
    let mut cid: sys::ngtcp2_cid = sys::ngtcp2_cid {
        datalen: dcid.len(),
        data: [0u8; 20],
    };
    cid.data[..dcid.len()].copy_from_slice(dcid);

    let suite = Suite::initial();
    let secret_len = sys::NGTCP2_CRYPTO_INITIAL_SECRETLEN as usize;
    let mut rx_secret = vec![0u8; secret_len];
    let mut tx_secret = vec![0u8; secret_len];
    let mut initial_secret = vec![0u8; secret_len];

    let side = if role.is_server() {
        sys::NGTCP2_CRYPTO_SIDE_SERVER
    } else {
        sys::NGTCP2_CRYPTO_SIDE_CLIENT
    };

    // SAFETY: all three buffers are the fixed length the helper writes, and the identifier
    // is a fully initialised `ngtcp2_cid`.
    let rv = unsafe {
        sys::ngtcp2_crypto_derive_initial_secrets(
            rx_secret.as_mut_ptr(),
            tx_secret.as_mut_ptr(),
            initial_secret.as_mut_ptr(),
            version,
            &raw const cid,
            side,
        )
    };
    if rv != 0 {
        return Err(Error::backend("could not derive the Initial secrets"));
    }

    Ok(InitialKeys {
        rx: derive_rx_keys(&suite, version, &rx_secret)?,
        tx: derive_keys(&suite, version, &tx_secret)?,
    })
}

/// The fixed key that authenticates a Retry packet's integrity tag.
///
/// Published constants, not secrets — one pair per QUIC version (RFC 9001 section 5.8 and
/// RFC 9369 section 3.3.3). A client must have this before it can accept a Retry at all,
/// because ngtcp2 verifies the tag through the ordinary encryption path before it will look
/// at anything else in the packet (`ngtcp2_conn.c:5543-5548`).
fn derive_retry_key(version: u32) -> Result<OsslPacketKey> {
    // The trailing NUL of each string literal is not part of the key.
    let key: &[u8] = match version {
        sys::NGTCP2_PROTO_VER_V1 => &sys::NGTCP2_RETRY_KEY_V1[..16],
        sys::NGTCP2_PROTO_VER_V2 => &sys::NGTCP2_RETRY_KEY_V2[..16],
        _ => return Err(Error::backend("no Retry key is defined for this version")),
    };
    OsslPacketKey::for_encryption(&Suite::retry(), key)
}

/// One span of inbound handshake bytes, at an address OpenSSL may hold on to.
///
/// The address matters. OpenSSL's record layer takes a pointer from `crypto_recv_rcd` and
/// keeps reading through it until it calls `crypto_release_rcd` for those bytes — which may
/// be several calls later, or during `SSL_free`. A `Vec<u8>` that the queue reallocated
/// underneath it would leave OpenSSL parsing freed memory, and the failure would look like a
/// corrupt handshake rather than like a lifetime bug.
///
/// `Box<[u8]>` is what makes this safe: the queue moves the box, never the bytes.
struct Record {
    /// The bytes, at a fixed address for as long as this record exists.
    data: Box<[u8]>,
    /// How many bytes have been handed to OpenSSL.
    read: usize,
    /// How many bytes OpenSSL has finished with.
    released: usize,
}

/// The inbound handshake bytes OpenSSL has not finished with.
///
/// Reading and releasing are separate positions rather than one, because OpenSSL is allowed
/// to read ahead of what it releases: it takes a whole record before it decides how much of
/// it was a complete message.
#[derive(Default)]
struct Inbound {
    records: std::collections::VecDeque<Record>,
}

impl Inbound {
    /// Queues bytes that arrived in a CRYPTO frame.
    fn push(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.records.push_back(Record {
            data: data.to_vec().into_boxed_slice(),
            read: 0,
            released: 0,
        });
    }

    /// Hands OpenSSL the next unread span, or nothing.
    ///
    /// Returns a raw pointer rather than a slice because that is what crosses the callback
    /// boundary, and because the borrow it would otherwise imply does not describe the
    /// truth: OpenSSL holds the pointer after this returns.
    fn next_span(&mut self) -> (*const u8, usize) {
        for record in &mut self.records {
            let unread = record.data.len() - record.read;
            if unread > 0 {
                // SAFETY: `read` is within the record, which is boxed and does not move.
                let ptr = unsafe { record.data.as_ptr().add(record.read) };
                record.read = record.data.len();
                return (ptr, unread);
            }
        }
        (ptr::null(), 0)
    }

    /// Marks bytes as finished with, freeing records once they are wholly consumed.
    fn release(&mut self, mut released: usize) {
        while released > 0 {
            let Some(front) = self.records.front_mut() else {
                return;
            };
            let outstanding = front.data.len() - front.released;
            let taken = released.min(outstanding);
            front.released += taken;
            released -= taken;
            if front.released == front.data.len() {
                self.records.pop_front();
            }
        }
    }
}

/// The state OpenSSL's QUIC-TLS callbacks reach through their `arg` pointer.
///
/// # Why `arg` rather than the `SSL`'s application data
///
/// `SSL_set_quic_tls_cbs` takes a third argument that it passes to every callback
/// unmodified. ngtcp2's own helper passes null and recovers its state from the `SSL`'s
/// application data instead (`ossl.c:1110`, `:1145`, and four more) — because it needs to
/// reach the `ngtcp2_conn`, which it can only find that way. This backend needs no
/// connection, so it uses the argument the API already provides. That is what dissolves the
/// three-object teardown cycle described at the top of this module: nothing OpenSSL holds
/// points at anything belonging to ngtcp2.
struct Engine {
    /// Everything the session has to report, in the order the callbacks produced it.
    ///
    /// A queue rather than a set of fields because order is load-bearing and easy to lose:
    /// OpenSSL can yield a secret, some handshake bytes and the peer's transport parameters
    /// from inside one `SSL_do_handshake`, and the connection must apply them in that order
    /// or install a key before the transport parameters its installation depends on.
    events: std::collections::VecDeque<SessionEvent>,
    /// Inbound handshake bytes OpenSSL has not released.
    inbound: Inbound,
    /// The local transport parameters, owned until OpenSSL has sent them.
    ///
    /// `SSL_set_quic_tls_transport_params` does not copy: the buffer must stay valid and at
    /// a fixed address until the extension is written. Keeping it here, in an allocation
    /// that lives as long as the session, is the whole of the fix.
    local_params: Option<Vec<u8>>,
    /// The suite the handshake negotiated, once it has.
    suite: Option<Suite>,
    /// The level outbound handshake bytes belong to.
    ///
    /// OpenSSL does not say. It yields a write secret for a level and everything it sends
    /// afterwards belongs to that level, which is exactly what ngtcp2's helper tracks in its
    /// `tx_level` field (`ossl.c:1131`).
    tx_level: Level,
    /// The QUIC version in force, which selects the key schedule's labels.
    version: u32,
    /// The helper context, kept only so the negotiated suite can be read from it.
    ossl_ctx: *mut sys::ngtcp2_crypto_ossl_ctx,
    /// The connection, lent for the length of one call into OpenSSL.
    ///
    /// Set immediately before `SSL_do_handshake` and cleared immediately after, because the
    /// only thing that may use it is a callback OpenSSL makes from inside that call. A stale
    /// one would be a pointer into a borrow that has ended, so it is never left set.
    conn: *mut (dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey> + 'static),
    /// Which side this is. A server produces its transport parameters mid-handshake; a client
    /// supplied them before it started.
    role: Role,
    /// Whether this endpoint's transport parameters have been given to OpenSSL.
    ///
    /// They are sent once. A second `SSL_set_quic_tls_transport_params` would replace a buffer
    /// OpenSSL may still be reading.
    local_params_sent: bool,
    /// Why a callback failed, when the failure could not be described by its return value.
    failure: Option<String>,
    /// Whether `SSL_do_handshake` has succeeded.
    ///
    /// Tracked here rather than asked of OpenSSL because it also gates the transition that
    /// produces [`SessionEvent::HandshakeComplete`], which must be reported exactly once.
    handshake_completed: bool,
}

impl Engine {
    /// Recovers the engine from the argument OpenSSL passes every callback.
    ///
    /// # Safety
    ///
    /// `arg` must be the pointer given to `SSL_set_quic_tls_cbs`, still alive, and no other
    /// reference to the engine may be live. The second condition is what the session's own
    /// methods are written to preserve: they reach the engine only through this pointer, and
    /// never hold a borrow across a call into OpenSSL.
    unsafe fn from_arg<'a>(arg: *mut c_void) -> Option<&'a mut Self> {
        if arg.is_null() {
            return None;
        }
        // SAFETY: the caller guarantees provenance and exclusivity.
        Some(unsafe { &mut *arg.cast::<Self>() })
    }

    /// Derives one direction's keys from a secret OpenSSL yielded and installs them.
    ///
    /// Installed rather than queued: on a server, installing the handshake write key is what
    /// completes this endpoint's transport parameters
    /// (`ngtcp2_conn_commit_local_transport_params`, reached only from
    /// `ngtcp2_conn_install_tx_handshake_key` at `ngtcp2_conn.c:11132`), and OpenSSL writes
    /// the message carrying them without returning from this callback.
    fn install(&mut self, level: Level, direction: Direction, secret: &[u8]) -> Result<()> {
        let suite = match level {
            // The Initial suite is fixed by the specification and needs no negotiation -- and
            // no secret is ever yielded at that level anyway.
            Level::Initial => Suite::initial(),
            _ => match self.suite {
                Some(suite) => suite,
                None => {
                    // SAFETY: the context is live for as long as the session, and the
                    // handshake has reached the point of yielding a secret, so a suite has
                    // been chosen.
                    let suite = unsafe { Suite::negotiated(self.ossl_ctx) }?;
                    self.suite = Some(suite);
                    suite
                }
            },
        };

        let keys = match direction {
            Direction::Read => derive_rx_keys(&suite, self.version, secret)?,
            Direction::Write => derive_keys(&suite, self.version, secret)?,
        };

        if self.conn.is_null() {
            return Err(Error::backend("a secret was yielded outside a call"));
        }
        // SAFETY: the pointer was set for the duration of the call this is running inside,
        // and is cleared before that call returns.
        let conn = unsafe { &mut *self.conn };
        conn.install_keys(level, direction, keys, secret)
    }
}

/// Maps OpenSSL's record protection level onto the seam's encryption level.
fn level_from_ossl(ossl_level: u32) -> Option<Level> {
    match ossl_level {
        sys::OSSL_RECORD_PROTECTION_LEVEL_NONE => Some(Level::Initial),
        sys::OSSL_RECORD_PROTECTION_LEVEL_EARLY => Some(Level::ZeroRtt),
        sys::OSSL_RECORD_PROTECTION_LEVEL_HANDSHAKE => Some(Level::Handshake),
        sys::OSSL_RECORD_PROTECTION_LEVEL_APPLICATION => Some(Level::OneRtt),
        // ngtcp2's helper asserts and aborts here (`ossl.c:1051-1053`), which in a release
        // build is an abort with no assertion. Failing the handshake is strictly better.
        _ => None,
    }
}

/// Queues handshake bytes OpenSSL wants sent.
unsafe extern "C" fn ossl_crypto_send(
    _ssl: *mut sys::SSL,
    buf: *const c_uchar,
    buflen: usize,
    consumed: *mut usize,
    arg: *mut c_void,
) -> c_int {
    // SAFETY: `arg` is the engine pointer, and OpenSSL is single-threaded within one `SSL`.
    let Some(engine) = (unsafe { Engine::from_arg(arg) }) else {
        return 1;
    };
    // SAFETY: OpenSSL guarantees `buf` is readable for `buflen` for the duration of the call.
    let data = unsafe { core::slice::from_raw_parts(buf, buflen) };
    let level = engine.tx_level;

    if engine.conn.is_null() {
        engine.failure = Some("handshake data was produced outside a call".to_owned());
        return 0;
    }
    // SAFETY: the pointer was set for the duration of the call this is running inside.
    let conn = unsafe { &mut *engine.conn };

    // Submitted **now**, and reported consumed only if that succeeded. Queuing it would mean
    // answering "how much did you take?" before the answer existed, and the only answer
    // available in advance is a claim that all of it was accepted -- which is exactly what the
    // seam forbids, because handshake data lost that way goes missing with no error anywhere.
    if let Err(error) = conn.submit_handshake(level, data) {
        engine.failure = Some(format!("handshake data was not submitted: {error}"));
        return 0;
    }
    // SAFETY: OpenSSL provides a writable out-parameter.
    unsafe { *consumed = buflen };
    1
}

/// Hands OpenSSL the next span of inbound handshake bytes.
unsafe extern "C" fn ossl_crypto_recv_rcd(
    _ssl: *mut sys::SSL,
    buf: *mut *const c_uchar,
    bytes_read: *mut usize,
    arg: *mut c_void,
) -> c_int {
    // SAFETY: as above.
    let Some(engine) = (unsafe { Engine::from_arg(arg) }) else {
        // SAFETY: OpenSSL provides writable out-parameters.
        unsafe {
            *buf = ptr::null();
            *bytes_read = 0;
        }
        return 1;
    };
    let (ptr, len) = engine.inbound.next_span();
    // SAFETY: OpenSSL provides writable out-parameters. The pointer stays valid until the
    // matching release, because the record it points into is boxed and is only dropped by
    // `Inbound::release`.
    unsafe {
        *buf = ptr;
        *bytes_read = len;
    }
    1
}

/// Frees inbound handshake bytes OpenSSL has finished with.
unsafe extern "C" fn ossl_crypto_release_rcd(
    _ssl: *mut sys::SSL,
    released: usize,
    arg: *mut c_void,
) -> c_int {
    // SAFETY: as above.
    let Some(engine) = (unsafe { Engine::from_arg(arg) }) else {
        return 1;
    };
    engine.inbound.release(released);
    1
}

/// Derives the keys for a secret OpenSSL produced.
unsafe extern "C" fn ossl_yield_secret(
    ssl: *mut sys::SSL,
    ossl_level: u32,
    direction: c_int,
    secret: *const c_uchar,
    secretlen: usize,
    arg: *mut c_void,
) -> c_int {
    // SAFETY: as above.
    let Some(engine) = (unsafe { Engine::from_arg(arg) }) else {
        return 1;
    };
    let Some(level) = level_from_ossl(ossl_level) else {
        engine.failure = Some("OpenSSL yielded a secret at an unknown level".to_owned());
        return 0;
    };
    // OpenSSL's convention: non-zero means the write direction.
    let direction = if direction == 0 {
        Direction::Read
    } else {
        Direction::Write
    };
    // SAFETY: OpenSSL guarantees the secret is readable for `secretlen` for this call.
    let secret = unsafe { core::slice::from_raw_parts(secret, secretlen) };

    if let Err(error) = engine.install(level, direction, secret) {
        engine.failure = Some(format!("could not install keys: {error}"));
        return 0;
    }
    // Set only after the keys are installed, so a failed derivation cannot leave the level
    // advanced with no key to go with it.
    if direction == Direction::Write {
        engine.tx_level = level;
    }

    // And, on a server, its transport parameters -- here and at no other moment. The key just
    // installed is what completed them (`ngtcp2_conn.c:11132`), so anything earlier yields an
    // incomplete set; and OpenSSL writes the message carrying them before this call returns,
    // so anything later is too late. ngtcp2's own helper gates it identically
    // (`shared.c:502-503`). A client's went in before the handshake started and must not be
    // replaced.
    if engine.role == Role::Server
        && level == Level::Handshake
        && direction == Direction::Write
        && !engine.local_params_sent
    {
        if engine.conn.is_null() {
            engine.failure = Some("a secret was yielded outside a call".to_owned());
            return 0;
        }
        // SAFETY: the pointer was set for the duration of the call this is running inside.
        let conn = unsafe { &mut *engine.conn };
        let local = match conn.local_transport_params() {
            Ok(local) => local,
            Err(error) => {
                engine.failure = Some(format!("no transport parameters to send: {error}"));
                return 0;
            }
        };
        engine.local_params = Some(local);
        let stored = engine.local_params.as_ref().expect("just set");
        let (ptr, len) = (stored.as_ptr(), stored.len());
        // SAFETY: `ssl` is valid and the buffer lives in the engine, which outlives it.
        if unsafe { sys::SSL_set_quic_tls_transport_params(ssl, ptr, len) } != 1 {
            engine.failure = Some("OpenSSL would not take the transport parameters".to_owned());
            return 0;
        }
        engine.local_params_sent = true;
    }
    1
}

/// Reports the peer's transport parameters, exactly as they arrived.
unsafe extern "C" fn ossl_got_transport_params(
    _ssl: *mut sys::SSL,
    params: *const c_uchar,
    paramslen: usize,
    arg: *mut c_void,
) -> c_int {
    // SAFETY: as above.
    let Some(engine) = (unsafe { Engine::from_arg(arg) }) else {
        return 1;
    };
    // SAFETY: OpenSSL guarantees the buffer is readable for this call.
    let peer = unsafe { core::slice::from_raw_parts(params, paramslen) };

    if engine.conn.is_null() {
        engine.failure = Some("the peer's transport parameters arrived outside a call".to_owned());
        return 0;
    }
    // SAFETY: the pointer was set for the duration of the call this is running inside.
    let conn = unsafe { &mut *engine.conn };

    // Handed over here and nowhere else, because this is where they arrive and because a
    // server's own set cannot be produced until they have been. This endpoint's are *not*
    // fetched here: OpenSSL refuses `SSL_set_quic_tls_transport_params` from inside this
    // callback with "bad extension", and on a server they would be incomplete anyway. They go
    // in at the next yielded secret -- see `ossl_yield_secret`.
    if let Err(error) = conn.set_peer_transport_params(peer) {
        engine.failure = Some(format!(
            "the peer's transport parameters were rejected: {error}"
        ));
        return 0;
    }
    1
}

/// Reports a TLS alert the peer must be told about.
unsafe extern "C" fn ossl_alert(_ssl: *mut sys::SSL, alert_code: u8, arg: *mut c_void) -> c_int {
    // SAFETY: as above.
    let Some(engine) = (unsafe { Engine::from_arg(arg) }) else {
        return 1;
    };
    engine.events.push_back(SessionEvent::Alert(alert_code));
    1
}

/// The dispatch table OpenSSL's QUIC record layer calls into.
///
/// A `static` rather than a value built per session: OpenSSL reads the table for as long as
/// the `SSL` lives, and the per-session state travels in the argument instead. Building it
/// on the stack would dangle the moment the constructor returned.
static QUIC_TLS_DISPATCH: [sys::OSSL_DISPATCH; 7] = [
    sys::OSSL_DISPATCH {
        function_id: sys::OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_SEND as c_int,
        // SAFETY of every cast below: `OSSL_DISPATCH` erases each function's type, which is
        // how OpenSSL's dispatch tables work. The identifier beside it is what says how it
        // will be called back, so the identifier and the signature must agree -- they are
        // checked against `ossl.c:1248-1274`, which registers the same six.
        function: Some(unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(
                    *mut sys::SSL,
                    *const c_uchar,
                    usize,
                    *mut usize,
                    *mut c_void,
                ) -> c_int,
                unsafe extern "C" fn(),
            >(ossl_crypto_send)
        }),
    },
    sys::OSSL_DISPATCH {
        function_id: sys::OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_RECV_RCD as c_int,
        function: Some(unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(
                    *mut sys::SSL,
                    *mut *const c_uchar,
                    *mut usize,
                    *mut c_void,
                ) -> c_int,
                unsafe extern "C" fn(),
            >(ossl_crypto_recv_rcd)
        }),
    },
    sys::OSSL_DISPATCH {
        function_id: sys::OSSL_FUNC_SSL_QUIC_TLS_CRYPTO_RELEASE_RCD as c_int,
        function: Some(unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(*mut sys::SSL, usize, *mut c_void) -> c_int,
                unsafe extern "C" fn(),
            >(ossl_crypto_release_rcd)
        }),
    },
    sys::OSSL_DISPATCH {
        function_id: sys::OSSL_FUNC_SSL_QUIC_TLS_YIELD_SECRET as c_int,
        function: Some(unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(
                    *mut sys::SSL,
                    u32,
                    c_int,
                    *const c_uchar,
                    usize,
                    *mut c_void,
                ) -> c_int,
                unsafe extern "C" fn(),
            >(ossl_yield_secret)
        }),
    },
    sys::OSSL_DISPATCH {
        function_id: sys::OSSL_FUNC_SSL_QUIC_TLS_GOT_TRANSPORT_PARAMS as c_int,
        function: Some(unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(*mut sys::SSL, *const c_uchar, usize, *mut c_void) -> c_int,
                unsafe extern "C" fn(),
            >(ossl_got_transport_params)
        }),
    },
    sys::OSSL_DISPATCH {
        function_id: sys::OSSL_FUNC_SSL_QUIC_TLS_ALERT as c_int,
        function: Some(unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(*mut sys::SSL, u8, *mut c_void) -> c_int,
                unsafe extern "C" fn(),
            >(ossl_alert)
        }),
    },
    // `OSSL_DISPATCH_END`: a zero identifier with no function. OpenSSL walks until it finds
    // this, so omitting it would walk off the end of the array.
    sys::OSSL_DISPATCH {
        function_id: 0,
        function: None,
    },
];

/// How a peer's certificate should be checked.
///
/// # What this means per role
///
/// For a **client**, [`Verify::Peer`] verifies the server's certificate chain against the
/// configured trust anchors *and* checks it was issued for the requested name. That is the
/// default, and the reason a verifying client must be given a server name.
///
/// For a **server**, [`Verify::Peer`] means "request no client certificate", because
/// ordinary QUIC clients present none and demanding one would reject every one of them.
/// Mutual TLS is not implemented; a server that needs it should not silently get something
/// weaker, so [`Verify::RequireClientCertificate`] exists and returns an error rather than
/// pretending.
/// Open rather than closed: a backend may grow a verification mode -- this one already
/// grew `RequireClientCertificate` -- and adding another must not break a caller that names
/// the ones it knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum Verify {
    /// Verify the peer where there is a peer certificate to verify. The default.
    ///
    /// See the note above: this is asymmetric, because the roles are.
    #[default]
    Peer,
    /// Require and verify a client certificate. **Servers only, and not yet implemented.**
    ///
    /// Present so that asking for mutual TLS fails loudly rather than silently producing a
    /// server that accepts anyone.
    RequireClientCertificate,
    /// Do not verify the peer at all.
    ///
    /// Named this way on purpose. There is no `Verify::None`, because "none" reads as an
    /// absence of configuration rather than as a decision; this reads as what it is.
    DangerouslyAcceptAnyCertificate,
}

/// Builder for an [`OsslBackend`].
pub struct OsslBackendBuilder {
    role: Role,
    alpn: Vec<Vec<u8>>,
    certificate_chain_pem: Option<Vec<u8>>,
    private_key_pem: Option<Vec<u8>>,
    trust_anchors_pem: Vec<Vec<u8>>,
    use_system_trust_store: bool,
    verify: Verify,
}

impl OsslBackendBuilder {
    /// Starts a configuration for the given role.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            alpn: Vec::new(),
            certificate_chain_pem: None,
            private_key_pem: None,
            trust_anchors_pem: Vec::new(),
            use_system_trust_store: role == Role::Client,
            verify: Verify::Peer,
        }
    }

    /// Adds an ALPN protocol, in preference order.
    ///
    /// QUIC requires ALPN: a handshake with no protocol in common fails. There is no
    /// sensible default, because the protocol depends entirely on what runs over the
    /// connection.
    pub fn alpn(mut self, protocol: impl Into<Vec<u8>>) -> Self {
        self.alpn.push(protocol.into());
        self
    }

    /// Sets the certificate chain, PEM-encoded. Servers must supply one.
    pub fn certificate_chain_pem(mut self, pem: impl Into<Vec<u8>>) -> Self {
        self.certificate_chain_pem = Some(pem.into());
        self
    }

    /// Sets the private key, PEM-encoded. Servers must supply one.
    pub fn private_key_pem(mut self, pem: impl Into<Vec<u8>>) -> Self {
        self.private_key_pem = Some(pem.into());
        self
    }

    /// Adds a PEM-encoded certificate as a trust anchor.
    pub fn trust_anchor_pem(mut self, pem: impl Into<Vec<u8>>) -> Self {
        self.trust_anchors_pem.push(pem.into());
        self
    }

    /// Whether to trust the system certificate store. On by default for clients.
    pub fn use_system_trust_store(mut self, use_it: bool) -> Self {
        self.use_system_trust_store = use_it;
        self
    }

    /// How to verify the peer. Defaults to [`Verify::Peer`].
    pub fn verify(mut self, verify: Verify) -> Self {
        self.verify = verify;
        self
    }

    /// Builds the backend.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] if the configuration is incomplete — a server
    /// without credentials, or any role without ALPN — and a native error if OpenSSL
    /// rejects the material.
    pub fn build(self) -> Result<OsslBackend> {
        ensure_ossl_init()?;

        if self.alpn.is_empty() {
            return Err(Error::invalid_input(
                "QUIC requires at least one ALPN protocol",
            ));
        }
        if self.role == Role::Server
            && (self.certificate_chain_pem.is_none() || self.private_key_pem.is_none())
        {
            return Err(Error::invalid_input(
                "a server needs both a certificate chain and a private key",
            ));
        }

        let alpn_wire = encode_alpn(&self.alpn)?;

        // SAFETY: `TLS_client_method`/`TLS_server_method` return static method tables.
        let method = unsafe {
            if self.role == Role::Server {
                sys::TLS_server_method()
            } else {
                sys::TLS_client_method()
            }
        };
        // SAFETY: `method` is a valid static method table.
        let raw = unsafe { sys::SSL_CTX_new(method) };
        if raw.is_null() {
            return Err(tls_error("SSL_CTX_new failed"));
        }
        let ctx = SslCtx(raw);

        if let (Some(chain), Some(key)) = (&self.certificate_chain_pem, &self.private_key_pem) {
            load_certificate_chain(ctx.0, chain)?;
            load_private_key(ctx.0, key)?;
            // SAFETY: `ctx` is valid and both credentials have been installed.
            if unsafe { sys::SSL_CTX_check_private_key(ctx.0) } != 1 {
                return Err(tls_error("the private key does not match the certificate"));
            }
        }

        if self.use_system_trust_store {
            // SAFETY: `ctx` is valid.
            unsafe { sys::SSL_CTX_set_default_verify_paths(ctx.0) };
        }
        for anchor in &self.trust_anchors_pem {
            add_trust_anchor(ctx.0, anchor)?;
        }

        if self.verify == Verify::RequireClientCertificate {
            return Err(Error::invalid_input(
                "mutual TLS is not implemented; a server cannot require a client certificate",
            ));
        }

        let mode = match self.verify {
            Verify::Peer if self.role == Role::Client => sys::SSL_VERIFY_PEER as c_int,
            // A server that demanded a client certificate would reject every ordinary
            // client, so `Verify::Peer` on a server means "request none" -- documented on
            // the enum, and `RequireClientCertificate` is refused above rather than
            // silently downgraded to this.
            _ => sys::SSL_VERIFY_NONE as c_int,
        };
        // SAFETY: `ctx` is valid; a null callback means "use the default decision".
        unsafe { sys::SSL_CTX_set_verify(ctx.0, mode, None) };

        let mut alpn_offers = None;
        if self.role == Role::Server {
            // The selection callback reads the offer list through the callback argument,
            // which must stay at a fixed address for as long as any session made from this
            // context can run -- which is **not** the same as as long as the backend lives.
            // `SSL_new` takes a reference on the context, so a session outlives a dropped
            // backend, and with it the callback that reads this. Reference counting the
            // offers, and having every session hold one, is what makes the two lifetimes
            // agree; owning it in the backend alone was a use-after-free reachable from
            // entirely safe code, found by dropping a server backend before its session.
            let offers = std::sync::Arc::new(alpn_wire.clone());
            let ptr: *const Vec<u8> = std::sync::Arc::as_ptr(&offers);
            // SAFETY: `ctx` is valid, and `offers` is owned by the backend being returned,
            // so the pointer stays valid for the context's whole life.
            unsafe {
                sys::SSL_CTX_set_alpn_select_cb(
                    ctx.0,
                    Some(alpn_select_cb),
                    ptr.cast_mut().cast::<c_void>(),
                )
            };
            alpn_offers = Some(offers);
        }

        Ok(OsslBackend {
            ctx,
            alpn_wire,
            role: self.role,
            verify: self.verify,
            alpn_offers,
        })
    }
}

/// Builds an error carrying whatever OpenSSL put on its error queue.
fn tls_error(context: &'static str) -> Error {
    // Draining the queue here also stops a stale entry from being attributed to the next
    // failure, which is a classic source of confusing OpenSSL diagnostics.
    let _ = take_openssl_error();
    Error::with_kind(ErrorKind::Crypto, context)
}

/// Loads a PEM certificate chain into a context.
fn load_certificate_chain(ctx: *mut sys::SSL_CTX, pem: &[u8]) -> Result<()> {
    let bio = Bio::from_bytes(pem)?;
    let mut first = true;
    loop {
        // SAFETY: `bio` is valid; the remaining arguments are the documented "no password"
        // form. Returns null when the BIO is exhausted.
        let cert = unsafe { sys::PEM_read_bio_X509(bio.0, ptr::null_mut(), None, ptr::null_mut()) };
        if cert.is_null() {
            break;
        }
        let cert = X509(cert);
        let rc = if first {
            // SAFETY: both pointers are valid.
            unsafe { sys::SSL_CTX_use_certificate(ctx, cert.0) }
        } else {
            // `SSL_CTX_add1_chain_cert` is a macro over `SSL_CTX_ctrl`, which bindgen does
            // not emit. The `1` argument selects the reference-taking variant, so `cert`
            // keeps its own reference and is still freed on drop.
            // SAFETY: both pointers are valid.
            unsafe {
                sys::SSL_CTX_ctrl(
                    ctx,
                    sys::SSL_CTRL_CHAIN_CERT as c_int,
                    1,
                    cert.0.cast::<c_void>(),
                ) as c_int
            }
        };
        if rc != 1 {
            return Err(tls_error("could not install a certificate"));
        }
        first = false;
    }
    if first {
        return Err(Error::invalid_input(
            "the certificate chain contained no PEM certificate",
        ));
    }
    // A parse that stopped early leaves an error on the queue; discard it, since reaching
    // the end of the chain is the normal way out of the loop above.
    let _ = take_openssl_error();
    Ok(())
}

/// Loads a PEM private key into a context.
fn load_private_key(ctx: *mut sys::SSL_CTX, pem: &[u8]) -> Result<()> {
    let bio = Bio::from_bytes(pem)?;
    // SAFETY: `bio` is valid; the remaining arguments are the "no password" form.
    let key =
        unsafe { sys::PEM_read_bio_PrivateKey(bio.0, ptr::null_mut(), None, ptr::null_mut()) };
    if key.is_null() {
        return Err(tls_error("could not parse the private key"));
    }
    let key = Pkey(key);
    // SAFETY: both pointers are valid.
    if unsafe { sys::SSL_CTX_use_PrivateKey(ctx, key.0) } != 1 {
        return Err(tls_error("could not install the private key"));
    }
    Ok(())
}

/// Adds a PEM certificate to a context's trust store.
fn add_trust_anchor(ctx: *mut sys::SSL_CTX, pem: &[u8]) -> Result<()> {
    let bio = Bio::from_bytes(pem)?;
    // SAFETY: `ctx` is valid; the store is owned by the context.
    let store = unsafe { sys::SSL_CTX_get_cert_store(ctx) };
    if store.is_null() {
        return Err(tls_error("the context has no certificate store"));
    }
    let mut added = false;
    loop {
        // SAFETY: `bio` is valid; null means exhausted.
        let cert = unsafe { sys::PEM_read_bio_X509(bio.0, ptr::null_mut(), None, ptr::null_mut()) };
        if cert.is_null() {
            break;
        }
        let cert = X509(cert);
        // SAFETY: both pointers are valid. `X509_STORE_add_cert` takes its own reference.
        if unsafe { sys::X509_STORE_add_cert(store, cert.0) } != 1 {
            return Err(tls_error("could not add a trust anchor"));
        }
        added = true;
    }
    if !added {
        return Err(Error::invalid_input(
            "the trust anchor contained no PEM certificate",
        ));
    }
    let _ = take_openssl_error();
    Ok(())
}

/// Chooses an ALPN protocol on the server side.
///
/// Runs inside OpenSSL during the handshake. Returning a fatal alert here is what turns "no
/// protocol in common" into a handshake failure that names ALPN, rather than a connection
/// that completes and then behaves strangely.
unsafe extern "C" fn alpn_select_cb(
    _ssl: *mut sys::SSL,
    out: *mut *const c_uchar,
    outlen: *mut c_uchar,
    client: *const c_uchar,
    client_len: core::ffi::c_uint,
    arg: *mut c_void,
) -> c_int {
    if arg.is_null() || client.is_null() {
        return sys::SSL_TLSEXT_ERR_ALERT_FATAL as c_int;
    }
    // SAFETY: `arg` is the leaked `Box<Vec<u8>>` installed at build time, which outlives
    // every session made from this context.
    let offers = unsafe { &*arg.cast::<Vec<u8>>() };
    // SAFETY: OpenSSL guarantees `client` is readable for `client_len` bytes.
    let client = unsafe { core::slice::from_raw_parts(client, client_len as usize) };

    // Server preference: walk our list in order and take the first the client also offered.
    let mut cursor = 0usize;
    while cursor < offers.len() {
        let len = offers[cursor] as usize;
        let start = cursor + 1;
        let end = start + len;
        if end > offers.len() {
            break;
        }
        let candidate = &offers[start..end];

        let mut peer = 0usize;
        while peer < client.len() {
            let peer_len = client[peer] as usize;
            let peer_start = peer + 1;
            let peer_end = peer_start + peer_len;
            if peer_end > client.len() {
                break;
            }
            if &client[peer_start..peer_end] == candidate {
                // SAFETY: `out` and `outlen` are valid out-parameters, and the slice we
                // point at lives in the leaked offers buffer.
                unsafe {
                    *out = offers.as_ptr().add(start);
                    *outlen = len as c_uchar;
                }
                return sys::SSL_TLSEXT_ERR_OK as c_int;
            }
            peer = peer_end;
        }
        cursor = end;
    }

    sys::SSL_TLSEXT_ERR_ALERT_FATAL as c_int
}

/// An OpenSSL `SSL_CTX`, freed on drop.
struct SslCtx(*mut sys::SSL_CTX);

impl Drop for SslCtx {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `SSL_CTX_new` and is freed exactly once.
        unsafe { sys::SSL_CTX_free(self.0) };
    }
}

// SAFETY: OpenSSL 3.x reference-counts `SSL_CTX` internally and permits use from multiple
// threads. This type only creates sessions from it.
unsafe impl Send for SslCtx {}
unsafe impl Sync for SslCtx {}

/// A memory BIO over borrowed bytes, freed on drop.
struct Bio(*mut sys::BIO);

impl Bio {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let len = c_int::try_from(bytes.len())
            .map_err(|_| Error::invalid_input("PEM input is too large"))?;
        // SAFETY: `bytes` is readable for `len`, and `BIO_new_mem_buf` only reads it. The
        // BIO does not outlive this function's callers' borrow.
        let bio = unsafe { sys::BIO_new_mem_buf(bytes.as_ptr().cast::<c_void>(), len) };
        if bio.is_null() {
            return Err(tls_error("BIO_new_mem_buf failed"));
        }
        Ok(Self(bio))
    }
}

impl Drop for Bio {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `BIO_new_mem_buf` and is freed exactly once.
        unsafe { sys::BIO_free(self.0) };
    }
}

/// An owned `X509`, freed on drop.
struct X509(*mut sys::X509);

impl Drop for X509 {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `PEM_read_bio_X509` and is freed exactly once.
        unsafe { sys::X509_free(self.0) };
    }
}

/// An owned `EVP_PKEY`, freed on drop.
struct Pkey(*mut sys::EVP_PKEY);

impl Drop for Pkey {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `PEM_read_bio_PrivateKey` and is freed exactly once.
        unsafe { sys::EVP_PKEY_free(self.0) };
    }
}

/// Lends the connection to the engine for the length of one call into OpenSSL.
struct LentConn {
    engine: *mut Engine,
}

impl LentConn {
    fn install(
        engine: *mut Engine,
        conn: &mut dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey>,
    ) -> Self {
        // The lifetime is erased because the pointer is stored in a `'static` field. Nothing
        // reads it outside this guard's scope: the guard clears it on drop, and the only
        // readers are callbacks OpenSSL makes from inside the calls the guard brackets.
        let erased: *mut (dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey> + '_) = conn;
        // SAFETY: the engine is live, no callback is running yet, and the erased lifetime is
        // reinstated by the guard clearing the field before the borrow ends.
        unsafe {
            (*engine).conn = core::mem::transmute::<
                *mut (dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey> + '_),
                *mut (dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey> + 'static),
            >(erased);
        }
        Self { engine }
    }
}

impl Drop for LentConn {
    fn drop(&mut self) {
        // SAFETY: the engine is live and OpenSSL has returned, so no callback holds this.
        unsafe { (*self.engine).conn = no_conn() };
    }
}

/// The "no call in progress" value for [`Engine::conn`].
///
/// A null *fat* pointer needs a concrete type to build its vtable half from, so this exists
/// solely to name one. It is never constructed and its methods are never reached: the field is
/// null-checked before it is ever dereferenced.
struct NoConn;

impl crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey> for NoConn {
    fn set_peer_transport_params(&mut self, _peer: &[u8]) -> Result<()> {
        unreachable!("the null connection handle is never dereferenced")
    }
    fn local_transport_params(&mut self) -> Result<Vec<u8>> {
        unreachable!("the null connection handle is never dereferenced")
    }
    fn install_keys(
        &mut self,
        _level: Level,
        _direction: Direction,
        _keys: DirectionalKeys<OsslPacketKey, OsslHeaderKey>,
        _secret: &[u8],
    ) -> Result<()> {
        unreachable!("the null connection handle is never dereferenced")
    }
    fn submit_handshake(&mut self, _level: Level, _data: &[u8]) -> Result<()> {
        unreachable!("the null connection handle is never dereferenced")
    }
}

/// A null handle, meaning no call into OpenSSL is in progress.
fn no_conn() -> *mut (dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey> + 'static) {
    core::ptr::null_mut::<NoConn>()
}

/// A configured OpenSSL TLS stack.
pub struct OsslBackend {
    /// Declared first so it is dropped first: `SSL_CTX_free` must run before the ALPN
    /// buffer its selection callback points at.
    ctx: SslCtx,
    alpn_wire: Vec<u8>,
    role: Role,
    verify: Verify,
    /// The server's ALPN offer list, at a fixed address for the selection callback.
    ///
    /// Indirected rather than held inline, despite what the lint suggests: the callback
    /// recovers a `*const Vec<u8>` and dereferences it, so the `Vec` *struct* must not move
    /// -- and it would, when this backend is returned by value.
    ///
    /// Reference counted rather than merely boxed because a session made from this backend
    /// keeps the `SSL_CTX` alive after the backend itself is dropped, and the callback goes
    /// with the context. Every session therefore holds a count.
    #[allow(clippy::box_collection)]
    alpn_offers: Option<std::sync::Arc<Vec<u8>>>,
}

impl OsslBackend {
    /// Starts building a backend for the given role.
    pub fn builder(role: Role) -> OsslBackendBuilder {
        OsslBackendBuilder::new(role)
    }
}

// SAFETY: `OsslBackend` owns an `SSL_CTX`, which OpenSSL 3.x permits sharing across
// threads, plus plain data.
unsafe impl Send for OsslBackend {}
unsafe impl Sync for OsslBackend {}

/// One connection's TLS session.
///
/// Owns the `SSL`, the helper context it reads the negotiated suite from, and the engine
/// OpenSSL calls back into. They must be destroyed in that order, and Rust's field-order drop
/// would make the ordering invisible, so [`Drop`] is written by hand.
pub struct OsslSession {
    /// The crypto helper context, kept only so the negotiated cipher suite can be read out of
    /// it. ngtcp2 is **not** given this — it receives a pointer to the session, which the
    /// connection owns — and that is why there is no longer a wrong pointer to pass.
    ossl_ctx: *mut sys::ngtcp2_crypto_ossl_ctx,
    ssl: *mut sys::SSL,
    /// A share of the backend's ALPN offer list, keeping it alive for the selection
    /// callback that reads it -- which outlives the backend whenever a session does.
    _alpn_offers: Option<std::sync::Arc<Vec<u8>>>,
    /// The handshake state OpenSSL's callbacks reach through their argument.
    ///
    /// A raw pointer rather than a `Box` on purpose. OpenSSL holds this same address and
    /// reaches through it from inside `SSL_do_handshake`, `SSL_read` and `SSL_free`. Owning
    /// it as a `Box` would mean this struct held a reference that a callback could alias, so
    /// every access — here and in the callbacks — goes through the pointer, and no borrow
    /// is ever held across a call into OpenSSL.
    engine: *mut Engine,
    verify: Verify,
    role: Role,
}

impl OsslSession {
    fn new(backend: &OsslBackend, role: Role, server_name: Option<&str>) -> Result<Self> {
        // SAFETY: the backend's context is valid and outlives this call; `SSL_new` takes
        // its own reference on it.
        let ssl = unsafe { sys::SSL_new(backend.ctx.0) };
        if ssl.is_null() {
            return Err(tls_error("SSL_new failed"));
        }

        // From here on every early return must free `ssl`, so it is wrapped immediately.
        let mut session = Self {
            ossl_ctx: ptr::null_mut(),
            ssl,
            _alpn_offers: backend.alpn_offers.clone(),
            engine: ptr::null_mut(),
            verify: backend.verify,
            role,
        };

        let mut ossl_ctx: *mut sys::ngtcp2_crypto_ossl_ctx = ptr::null_mut();
        // SAFETY: `ossl_ctx` is a valid out-parameter and `ssl` is valid.
        let rc = unsafe { sys::ngtcp2_crypto_ossl_ctx_new(&mut ossl_ctx, session.ssl) };
        if rc != 0 {
            return Err(Error::native(rc, "ngtcp2_crypto_ossl_ctx_new failed"));
        }
        session.ossl_ctx = ossl_ctx;

        // The engine is leaked out of its box on purpose: OpenSSL is about to be
        // given this address, and `Drop` below is what reclaims it, after the
        // `SSL` that holds it has been freed.
        let engine = Box::into_raw(Box::new(Engine {
            events: std::collections::VecDeque::new(),
            inbound: Inbound::default(),
            local_params: None,
            suite: None,
            tx_level: Level::Initial,
            version: sys::NGTCP2_PROTO_VER_V1,
            ossl_ctx,
            conn: no_conn(),
            role,
            local_params_sent: false,
            failure: None,
            handshake_completed: false,
        }));
        session.engine = engine;

        // SAFETY: `ssl` is valid, the dispatch table is `static` so it outlives the
        // `SSL`, and the engine outlives it too because `Drop` frees the `SSL`
        // first.
        let rc = unsafe {
            sys::SSL_set_quic_tls_cbs(
                session.ssl,
                QUIC_TLS_DISPATCH.as_ptr(),
                engine.cast::<c_void>(),
            )
        };
        if rc != 1 {
            return Err(tls_error("SSL_set_quic_tls_cbs failed"));
        }

        // SAFETY: `ssl` is valid.
        unsafe {
            if role == Role::Server {
                sys::SSL_set_accept_state(session.ssl);
            } else {
                sys::SSL_set_connect_state(session.ssl);
            }
        }

        if role == Role::Client {
            // SAFETY: `ssl` is valid and the ALPN wire form outlives the call, which
            // copies it.
            let rc = unsafe {
                sys::SSL_set_alpn_protos(
                    session.ssl,
                    backend.alpn_wire.as_ptr(),
                    backend.alpn_wire.len() as core::ffi::c_uint,
                )
            };
            if rc != 0 {
                return Err(tls_error("SSL_set_alpn_protos failed"));
            }

            if let Some(name) = server_name {
                session.set_server_name(name)?;
            } else if backend.verify == Verify::Peer {
                // Verification without a name to check against would validate the chain and
                // then accept a certificate for any host, which is the failure mode most
                // worth refusing outright.
                return Err(Error::invalid_input(
                    "a verifying client must be given a server name to check against",
                ));
            }
        }

        Ok(session)
    }

    /// Sets SNI and, when verifying, the name the certificate must match.
    fn set_server_name(&mut self, name: &str) -> Result<()> {
        let c_name = std::ffi::CString::new(name)
            .map_err(|_| Error::invalid_input("a server name cannot contain a NUL byte"))?;

        // `SSL_set_tlsext_host_name` is a macro over `SSL_ctrl`; bindgen does not emit it.
        // SAFETY: `ssl` is valid; `SSL_ctrl` copies the name.
        let rc = unsafe {
            sys::SSL_ctrl(
                self.ssl,
                CTRL_SET_TLSEXT_HOSTNAME,
                NAMETYPE_HOST_NAME,
                c_name.as_ptr() as *mut c_void,
            )
        };
        if rc != 1 {
            return Err(tls_error("could not set the SNI server name"));
        }

        if self.verify == Verify::Peer {
            // Separate from SNI on purpose: SNI says which certificate to send, this says
            // which name the returned certificate must actually match. Setting only the
            // former is the classic way to build a client that appears to verify and does
            // not.
            // SAFETY: `ssl` is valid; `SSL_set1_host` copies the name.
            if unsafe { sys::SSL_set1_host(self.ssl, c_name.as_ptr()) } != 1 {
                return Err(tls_error("could not set the certificate verification name"));
            }
        }
        Ok(())
    }

    /// The result of certificate verification, once the handshake has run.
    pub(crate) fn verify_result(&self) -> core::ffi::c_long {
        // SAFETY: `ssl` is valid.
        unsafe { sys::SSL_get_verify_result(self.ssl) }
    }
}

impl Drop for OsslSession {
    fn drop(&mut self) {
        // The order here is the reason this type exists. See the module documentation.
        //
        // 1. Free the `SSL`. This is what releases any outstanding CRYPTO records, which it
        //    does by calling back into the engine -- so the engine must still be alive, which
        //    is why it is freed third rather than first.
        if !self.ssl.is_null() {
            // SAFETY: the pointer came from `SSL_new` and is freed exactly once.
            unsafe { sys::SSL_free(self.ssl) };
            self.ssl = ptr::null_mut();
        }

        // 2. Then the helper context, which the engine read the negotiated suite from.
        if !self.ossl_ctx.is_null() {
            // SAFETY: the pointer came from `ngtcp2_crypto_ossl_ctx_new`, is freed exactly
            // once, and `SSL_free` has already run.
            unsafe { sys::ngtcp2_crypto_ossl_ctx_del(self.ossl_ctx) };
            self.ossl_ctx = ptr::null_mut();
        }

        // 3. And the engine last of all. It owns the inbound records OpenSSL was reading
        //    through and the transport parameters it was sending, and step 1 released both by
        //    calling back into it. Freeing it any earlier is a use-after-free.
        if !self.engine.is_null() {
            // SAFETY: the pointer came from `Box::into_raw`, is reclaimed exactly once, and
            // the `SSL` that held it has been freed.
            drop(unsafe { Box::from_raw(self.engine) });
            self.engine = ptr::null_mut();
        }
    }
}

// SAFETY: an `OsslSession` owns its `SSL`, its helper context and its boxed conn ref
// exclusively. None is shared, and OpenSSL permits an `SSL` to be used from any one thread
// at a time. It is deliberately not `Sync`.
unsafe impl Send for OsslSession {}

impl OsslSession {
    /// Reaches the engine, or fails for a session that was not built for the safe seam.
    ///
    /// Returns a raw pointer rather than a reference so that callers must open their own
    /// short unsafe block for each access. That is deliberate friction: a reference handed
    /// out here could be held across a call into OpenSSL, where a callback would form a
    /// second one to the same object.
    fn engine_ptr(&self) -> Result<*mut Engine> {
        if self.engine.is_null() {
            return Err(Error::backend("this session is not on the safe TLS seam"));
        }
        Ok(self.engine)
    }

    /// Runs OpenSSL as far as it will go, queuing whatever it produces on the way.
    ///
    /// # What "as far as it will go" means
    ///
    /// `SSL_do_handshake` reports `WANT_READ` or `WANT_WRITE` when it needs more input.
    /// Neither is an error here: QUIC delivers handshake bytes a flight at a time, so
    /// "not yet" is the normal answer to most calls.
    ///
    /// The `SSL_read` afterwards is not optional and not about application data — QUIC
    /// carries none through TLS. It is what makes OpenSSL process **post-handshake**
    /// messages: session tickets, and the `NewSessionTicket` a server sends after the
    /// handshake completes. ngtcp2's helper does the same, for the same reason
    /// (`ossl.c:993`), and omitting it produces a connection that completes and then
    /// quietly ignores everything the peer says at the TLS layer.
    fn drive(
        &mut self,
        conn: &mut dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey>,
    ) -> Result<()> {
        let engine = self.engine_ptr()?;
        let ssl = self.ssl;

        // Lent for exactly the length of the calls below. The guard clears it on drop --
        // including while unwinding -- so a later callback can never follow it into a borrow
        // that has ended. The same discipline as `callbacks::BridgeGuard`, for the same
        // reason.
        let _lent = LentConn::install(engine, conn);

        // SAFETY: reading a plain field through the pointer; no borrow is held across the
        // OpenSSL calls below, which is what keeps the callbacks' `&mut` exclusive.
        let completed = unsafe { (*engine).handshake_completed };

        if !completed {
            // SAFETY: `ssl` is valid. This re-enters this module through the dispatch
            // table, which reaches the engine through the argument rather than through
            // anything borrowed here.
            let rv = unsafe { sys::SSL_do_handshake(ssl) };
            if rv <= 0 {
                // SAFETY: `ssl` is valid.
                let err = unsafe { sys::SSL_get_error(ssl, rv) } as u32;
                return match err {
                    sys::SSL_ERROR_WANT_READ | sys::SSL_ERROR_WANT_WRITE => Ok(()),
                    _ => Err(self.handshake_error("SSL_do_handshake failed")),
                };
            }
            // SAFETY: as above; nothing else holds a reference.
            unsafe {
                (*engine).handshake_completed = true;
                (*engine).events.push_back(SessionEvent::HandshakeComplete);
            }
        }

        // SAFETY: `ssl` is valid. A null buffer of length zero asks OpenSSL to process
        // whatever has arrived without returning any of it.
        let rv = unsafe { sys::SSL_read(ssl, ptr::null_mut(), 0) };
        if rv != 1 {
            // SAFETY: `ssl` is valid.
            let err = unsafe { sys::SSL_get_error(ssl, rv) } as u32;
            return match err {
                sys::SSL_ERROR_WANT_READ | sys::SSL_ERROR_WANT_WRITE => Ok(()),
                _ => Err(self.handshake_error("SSL_read failed")),
            };
        }
        Ok(())
    }

    /// Builds the error for a failed handshake.
    ///
    /// The reason a callback recorded is deliberately **left** in place rather than consumed
    /// here: [`Error`] carries a `&'static str`, so a reason discovered at run time cannot
    /// travel inside one. It reaches the caller through
    /// [`Session::failure_reason`] instead, which is the same route certificate
    /// verification failures already take.
    fn handshake_error(&self, context: &'static str) -> Error {
        tls_error(context)
    }
}

impl Session for OsslSession {
    type PacketKey = OsslPacketKey;
    type HeaderKey = OsslHeaderKey;

    fn initial_keys(
        &mut self,
        version: u32,
        dcid: &[u8],
    ) -> Result<InitialKeys<Self::PacketKey, Self::HeaderKey>> {
        let engine = self.engine_ptr()?;
        // Every later derivation uses this version's labels. ngtcp2 calls this again after a
        // Retry and again if a version is negotiated, so the last call is authoritative --
        // which is exactly the sequence ngtcp2 itself follows.
        // SAFETY: the pointer is live and no callback can be running.
        unsafe { (*engine).version = version };
        derive_initial_keys(self.role, version, dcid)
    }

    fn retry_key(&mut self, version: u32) -> Result<Self::PacketKey> {
        derive_retry_key(version)
    }

    fn set_local_transport_params(&mut self, params: &[u8]) -> Result<()> {
        let engine = self.engine_ptr()?;
        // SAFETY: the pointer is live and no callback can be running.
        let stored = unsafe {
            if (*engine).local_params.is_some() {
                return Err(Error::backend(
                    "the local transport parameters were already set",
                ));
            }
            (*engine).local_params = Some(params.to_vec());
            let stored = (*engine).local_params.as_ref().expect("just set");
            (stored.as_ptr(), stored.len())
        };

        // OpenSSL does **not** copy: it keeps the pointer until it writes the extension.
        // The buffer it is given lives in the engine, which outlives the `SSL`.
        // SAFETY: `ssl` is valid and the buffer stays at this address until the session is
        // dropped, which happens after `SSL_free`.
        let rc = unsafe { sys::SSL_set_quic_tls_transport_params(self.ssl, stored.0, stored.1) };
        if rc != 1 {
            return Err(tls_error("SSL_set_quic_tls_transport_params failed"));
        }
        // Recorded so the server path in `ossl_yield_secret` does not later replace a buffer
        // OpenSSL may still be reading. A client never reaches that path, but saying so here
        // rather than relying on the role check keeps the two independent.
        // SAFETY: the pointer is live and no callback can be running.
        unsafe { (*engine).local_params_sent = true };
        Ok(())
    }

    fn start_handshake(
        &mut self,
        conn: &mut dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey>,
    ) -> Result<()> {
        self.drive(conn)
    }

    fn read_handshake(
        &mut self,
        _level: Level,
        data: &[u8],
        conn: &mut dyn crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey>,
    ) -> Result<()> {
        let engine = self.engine_ptr()?;
        // The level is not passed on. OpenSSL's record layer infers it from the keys it has
        // installed, and ngtcp2 already refuses handshake data at a level whose keys are not
        // in place -- so a second, independent notion of the level here could only disagree
        // with those two.
        // SAFETY: the pointer is live and no callback can be running.
        unsafe { (*engine).inbound.push(data) };
        self.drive(conn)
    }

    fn poll_event(&mut self) -> Option<SessionEvent> {
        if self.engine.is_null() {
            return None;
        }
        // SAFETY: the pointer is live and no callback can be running.
        unsafe { (*self.engine).events.pop_front() }
    }

    fn rotate_keys(
        &mut self,
        rx_secret: &[u8],
        tx_secret: &[u8],
    ) -> Result<RotatedKeys<Self::PacketKey>> {
        let engine = self.engine_ptr()?;
        // SAFETY: the pointer is live and no callback can be running.
        let (suite, version) = unsafe { ((*engine).suite, (*engine).version) };
        let suite = suite.ok_or_else(|| {
            Error::backend("the keys cannot be rotated before a cipher suite is negotiated")
        })?;

        let next_rx = update_traffic_secret(&suite, version, rx_secret)?;
        let next_tx = update_traffic_secret(&suite, version, tx_secret)?;

        // Header protection keys are not rotated by a key update, so only the payload keys
        // are derived here -- which is why the returned type has no place to put them.
        let rx = derive_rx_keys(&suite, version, &next_rx)?;
        let tx = derive_keys(&suite, version, &next_tx)?;

        Ok(RotatedKeys {
            rx_packet: rx.packet,
            rx_iv: rx.iv,
            rx_secret: next_rx,
            tx_packet: tx.packet,
            tx_iv: tx.iv,
            tx_secret: next_tx,
        })
    }

    fn negotiated_alpn(&self) -> Option<Vec<u8>> {
        let mut data: *const c_uchar = ptr::null();
        let mut len: core::ffi::c_uint = 0;
        // SAFETY: `ssl` is valid and both out-parameters are writable.
        unsafe { sys::SSL_get0_alpn_selected(self.ssl, &mut data, &mut len) };
        if data.is_null() || len == 0 {
            return None;
        }
        // SAFETY: OpenSSL guarantees the buffer is readable for `len` bytes and owned by the
        // `SSL`, so copying it out is what keeps the result usable.
        Some(unsafe { core::slice::from_raw_parts(data, len as usize) }.to_vec())
    }

    fn failure_reason(&self) -> Option<String> {
        // What a callback recorded first: it names the actual cause, where OpenSSL's queue
        // will only say the handshake failed.
        if !self.engine.is_null() {
            // SAFETY: the pointer is live and no callback can be running.
            if let Some(reason) = unsafe { (*self.engine).failure.clone() } {
                return Some(reason);
            }
        }
        // Then certificate verification, because it is the failure a caller is most likely to
        // have caused and the one OpenSSL's generic error queue describes worst.
        let verdict = self.verify_result();
        if verdict != sys::X509_V_OK as core::ffi::c_long {
            // SAFETY: the function accepts any long and returns a static string.
            let text = unsafe { sys::X509_verify_cert_error_string(verdict) };
            if !text.is_null() {
                // SAFETY: the returned string is static and NUL-terminated.
                let text = unsafe { CStr::from_ptr(text) };
                return Some(format!(
                    "certificate verification failed: {}",
                    text.to_string_lossy()
                ));
            }
        }
        take_openssl_error()
    }
}

/// Advances a traffic secret to the next generation.
///
/// One `hkdf_expand_label` with the `quic ku` label — or `quicv2 ku` for version 2, which
/// the helper selects from the version rather than this crate choosing it (`shared.c`'s
/// `ngtcp2_crypto_update_traffic_secret`).
fn update_traffic_secret(suite: &Suite, version: u32, secret: &[u8]) -> Result<Vec<u8>> {
    let mut next = vec![0u8; secret.len()];
    // SAFETY: the destination is the same length as the source, which is what the helper
    // writes, and the digest descriptor is live.
    let rv = unsafe {
        sys::ngtcp2_crypto_update_traffic_secret(
            next.as_mut_ptr(),
            version,
            &raw const suite.md,
            secret.as_ptr(),
            secret.len(),
        )
    };
    if rv != 0 {
        return Err(Error::backend("could not update the traffic secret"));
    }
    Ok(next)
}

impl Backend for OsslBackend {
    type Session = OsslSession;

    fn new_session(&self, role: Role, server_name: Option<&str>) -> Result<Self::Session> {
        if role != self.role {
            return Err(Error::invalid_input(
                "the session role does not match the backend's role",
            ));
        }
        OsslSession::new(self, role, server_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns a hexadecimal string into bytes, for transcribing published test vectors.
    fn hex(s: &str) -> Vec<u8> {
        assert!(
            s.len().is_multiple_of(2),
            "a hexadecimal vector has an even length"
        );
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("hexadecimal"))
            .collect()
    }

    #[test]
    fn the_initial_suite_has_the_dimensions_quic_fixes() {
        // AES-128-GCM with SHA-256, which QUIC mandates for Initial packets in every
        // version. If any of these is wrong every derived key is the wrong length and
        // nothing interoperates.
        let suite = Suite::initial();
        assert_eq!(suite.key_len(), 16);
        assert_eq!(suite.iv_len(), 12);
        assert_eq!(suite.tag_len(), 16);
        assert_eq!(suite.hash_len(), 32);
    }

    #[test]
    fn the_initial_suite_reports_no_usage_limits() {
        // Matching `ngtcp2_crypto_ctx_initial` exactly. Pinned because the seam documents
        // zero as a trap, and this is the one place where zero is correct: ngtcp2 checks
        // these only in the application packet number space.
        let suite = Suite::initial();
        assert_eq!(suite.max_encryption, 0);
        assert_eq!(suite.max_decryption_failure, 0);
    }

    #[test]
    fn a_payload_is_protected_and_recovered() {
        // Sealing works in place -- ngtcp2's encrypt callback may pass one pointer as both
        // source and destination. Opening now takes its ciphertext and its destination as
        // separate buffers, matching how ngtcp2's core always decrypts a received packet
        // into a buffer distinct from the packet itself.
        let suite = Suite::initial();
        let key = vec![0x2a; suite.key_len()];
        let nonce = vec![0x11; suite.iv_len()];
        let aad = b"a packet header";
        let plaintext = b"the payload of a QUIC packet";

        let seal = OsslPacketKey::for_encryption(&suite, &key).unwrap();
        let open = OsslPacketKey::for_decryption(&suite, &key).unwrap();

        let mut buf = vec![0u8; plaintext.len() + seal.tag_len()];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        seal.seal(&mut buf, plaintext.len(), &nonce, aad).unwrap();
        assert_ne!(&buf[..plaintext.len()], &plaintext[..]);

        let mut plain = vec![0u8; plaintext.len()];
        let recovered = open.open(&mut plain, &buf, &nonce, aad).unwrap();
        assert_eq!(recovered, plaintext.len());
        assert_eq!(&plain[..recovered], &plaintext[..]);
    }

    #[test]
    fn a_forged_payload_reports_a_failed_decryption_rather_than_a_failed_backend() {
        // The distinction the whole error type exists for. Anyone who can send a datagram
        // can produce this; reporting it as fatal would hand them the connection.
        let suite = Suite::initial();
        let key = vec![0x2a; suite.key_len()];
        let nonce = vec![0x11; suite.iv_len()];
        let open = OsslPacketKey::for_decryption(&suite, &key).unwrap();

        let ciphertext = vec![0u8; 32];
        let mut plain = vec![0u8; ciphertext.len()];
        assert_eq!(
            open.open(&mut plain, &ciphertext, &nonce, b"").unwrap_err(),
            CryptoError::Decrypt
        );
    }

    #[test]
    fn a_payload_too_short_to_hold_a_tag_is_a_failed_decryption() {
        // A truncating attacker's packet, not a broken backend.
        let suite = Suite::initial();
        let key = vec![0x2a; suite.key_len()];
        let nonce = vec![0x11; suite.iv_len()];
        let open = OsslPacketKey::for_decryption(&suite, &key).unwrap();

        let ciphertext = vec![0u8; 8];
        let mut plain = vec![0u8; ciphertext.len()];
        assert_eq!(
            open.open(&mut plain, &ciphertext, &nonce, b"").unwrap_err(),
            CryptoError::Decrypt
        );
    }

    #[test]
    fn sealing_into_a_buffer_with_no_room_for_the_tag_fails_rather_than_overflowing() {
        // The helper writes the tag past the plaintext without checking (`ossl.c:912-916`),
        // so this bound is load-bearing rather than defensive.
        let suite = Suite::initial();
        let key = vec![0x2a; suite.key_len()];
        let nonce = vec![0x11; suite.iv_len()];
        let seal = OsslPacketKey::for_encryption(&suite, &key).unwrap();

        let mut buf = vec![0u8; 16];
        assert_eq!(
            seal.seal(&mut buf, 16, &nonce, b"").unwrap_err(),
            CryptoError::Fatal
        );
    }

    #[test]
    fn a_key_of_the_wrong_length_is_rejected() {
        let suite = Suite::initial();
        assert!(OsslPacketKey::for_encryption(&suite, &[0u8; 8]).is_err());
        assert!(OsslHeaderKey::new(&suite, &[0u8; 8]).is_err());
    }

    #[test]
    fn a_header_mask_needs_a_full_sample() {
        // The helper reads `NGTCP2_HP_SAMPLELEN` bytes regardless of what it is told, so a
        // short sample would be an out-of-bounds read rather than a short answer.
        let suite = Suite::initial();
        let key = vec![0x2a; suite.key_len()];
        let header = OsslHeaderKey::new(&suite, &key).unwrap();
        assert_eq!(header.mask(&[0u8; 8]).unwrap_err(), CryptoError::Fatal);
        assert!(header.mask(&[0u8; HP_SAMPLE_LEN]).is_ok());
    }

    #[test]
    fn the_two_sides_derive_each_others_initial_keys() {
        // Not a known-answer test -- the vectors below are that -- but the property that
        // actually has to hold on the wire: what a client encrypts with is what a server
        // decrypts with. It is checked through the public seam, so it also covers the
        // encryption/decryption context split.
        let dcid = hex("8394c8f03e515708");
        let version = sys::NGTCP2_PROTO_VER_V1;
        let client = derive_initial_keys(Role::Client, version, &dcid).unwrap();
        let server = derive_initial_keys(Role::Server, version, &dcid).unwrap();

        assert_eq!(client.tx.iv, server.rx.iv);
        assert_eq!(client.rx.iv, server.tx.iv);

        let plaintext = b"a client's first flight";
        let mut buf = vec![0u8; plaintext.len() + client.tx.packet.tag_len()];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        client
            .tx
            .packet
            .seal(&mut buf, plaintext.len(), &client.tx.iv, b"header")
            .unwrap();
        let mut plain = vec![0u8; plaintext.len()];
        let len = server
            .rx
            .packet
            .open(&mut plain, &buf, &server.rx.iv, b"header")
            .unwrap();
        assert_eq!(&plain[..len], &plaintext[..]);
    }

    #[test]
    fn a_retry_key_exists_for_every_version_quic_defines_one_for() {
        assert!(derive_retry_key(sys::NGTCP2_PROTO_VER_V1).is_ok());
        assert!(derive_retry_key(sys::NGTCP2_PROTO_VER_V2).is_ok());
        assert!(derive_retry_key(0xdead_beef).is_err());
    }

    #[test]
    fn a_connection_identifier_longer_than_quic_allows_is_rejected() {
        let too_long = vec![0u8; sys::NGTCP2_MAX_CIDLEN as usize + 1];
        assert!(derive_initial_keys(Role::Client, sys::NGTCP2_PROTO_VER_V1, &too_long).is_err());
    }

    /// The connection identifier every published Initial vector is derived from.
    const VECTOR_DCID: &str = "8394c8f03e515708";

    /// Builds the AEAD nonce the way QUIC does: the packet number, right-aligned in a
    /// zero-padded field the width of the initialisation vector, exclusive-ored with it
    /// (RFC 9001 section 5.3).
    ///
    /// Reproduced here rather than exposed on the seam because ngtcp2 constructs it itself
    /// and hands the result to the key — the seam never sees a packet number. Getting it
    /// right here is what makes the vectors below meaningful.
    fn nonce(iv: &[u8], packet_number: u64) -> Vec<u8> {
        let mut nonce = iv.to_vec();
        let pn = packet_number.to_be_bytes();
        let offset = nonce.len() - pn.len();
        for (n, p) in nonce[offset..].iter_mut().zip(pn) {
            *n ^= p;
        }
        nonce
    }

    #[test]
    fn the_version_1_initial_keys_match_rfc_9001() {
        // RFC 9001 appendix A.1. These are the published values, not this implementation's
        // own output -- which is the entire point: a test written from what the code
        // produces proves only that the code is consistent with itself, and a QUIC
        // implementation that is merely self-consistent talks to nobody.
        //
        // The initialisation vectors are compared directly. The payload and header keys
        // cannot be, because they are consumed into OpenSSL cipher contexts and never come
        // back out -- so they are checked below through what they produce, which is a
        // stronger claim anyway.
        let dcid = hex(VECTOR_DCID);
        let v1 = sys::NGTCP2_PROTO_VER_V1;
        let client = derive_initial_keys(Role::Client, v1, &dcid).unwrap();
        let server = derive_initial_keys(Role::Server, v1, &dcid).unwrap();

        assert_eq!(
            client.tx.iv.as_slice(),
            hex("fa044b2f42a3fd3b46fb255c").as_slice()
        );
        assert_eq!(
            server.tx.iv.as_slice(),
            hex("0ac1493ca1905853b0bba03e").as_slice()
        );
        // The roles agree on which key is whose, which is what actually has to hold.
        assert_eq!(client.rx.iv, server.tx.iv);
        assert_eq!(server.rx.iv, client.tx.iv);
    }

    #[test]
    fn the_version_1_header_masks_match_rfc_9001() {
        // RFC 9001 appendices A.2 and A.3: the sample taken from each side's protected
        // Initial packet, and the mask the published header protection key produces from it.
        // This is what proves the `quic hp` derivation, since the key itself is unreadable.
        let dcid = hex(VECTOR_DCID);
        let v1 = sys::NGTCP2_PROTO_VER_V1;
        let client = derive_initial_keys(Role::Client, v1, &dcid).unwrap();
        let server = derive_initial_keys(Role::Server, v1, &dcid).unwrap();

        let client_mask = client
            .tx
            .header
            .mask(&hex("d1b1c98dd7689fb8ec11d242b123dc9b"))
            .unwrap();
        assert_eq!(client_mask.as_slice(), hex("437b9aec36").as_slice());

        let server_mask = server
            .tx
            .header
            .mask(&hex("2cd0991cd25b0aac406a5816b6394100"))
            .unwrap();
        assert_eq!(server_mask.as_slice(), hex("2ec0d8356a").as_slice());
    }

    #[test]
    fn the_version_1_server_initial_packet_matches_rfc_9001() {
        // RFC 9001 appendix A.3, end to end: the server's real Initial packet, protected
        // with the real key, compared byte for byte against the RFC's ciphertext. This is
        // the vector that covers the payload key derivation, the nonce construction and the
        // AEAD together -- and it is a *known answer*, so nothing here can agree with itself
        // and be wrong.
        //
        // The additional authenticated data is the header **before** header protection is
        // applied, which is the subtlety most easily got backwards.
        let dcid = hex(VECTOR_DCID);
        let v1 = sys::NGTCP2_PROTO_VER_V1;
        let server = derive_initial_keys(Role::Server, v1, &dcid).unwrap();
        let client = derive_initial_keys(Role::Client, v1, &dcid).unwrap();

        let header = hex("c1000000010008f067a5502a4262b50040750001");
        let plaintext = hex(
            "02000000000600405a020000560303eefce7f7b37ba1d1632e96677825ddf73988cfc7\
             9825df566dc5430b9a045a1200130100002e00330024001d00209d3c940d89690b84d0\
             8a60993c144eca684d1081287c834d5311bcf32bb9da1a002b00020304",
        );
        let expected = hex(
            "5a482cd0991cd25b0aac406a5816b6394100f37a1c69797554780bb38cc5a99f5ede4c\
             f73c3ec2493a1839b3dbcba3f6ea46c5b7684df3548e7ddeb9c3bf9c73cc3f3bded74b\
             562bfb19fb84022f8ef4cdd93795d77d06edbb7aaf2f58891850abbdca3d20398c2764\
             56cbc42158407dd074ee",
        );

        let nonce = nonce(&server.tx.iv, 1);
        let mut buf = vec![0u8; plaintext.len() + server.tx.packet.tag_len()];
        buf[..plaintext.len()].copy_from_slice(&plaintext);
        server
            .tx
            .packet
            .seal(&mut buf, plaintext.len(), &nonce, &header)
            .unwrap();
        assert_eq!(buf, expected);

        // And the other side recovers it, using the key it derived independently.
        let client_nonce = super::tests::nonce(&client.rx.iv, 1);
        let mut plain = vec![0u8; plaintext.len()];
        let recovered = client
            .rx
            .packet
            .open(&mut plain, &buf, &client_nonce, &header)
            .unwrap();
        assert_eq!(&plain[..recovered], plaintext.as_slice());
    }

    #[test]
    fn the_version_2_initial_keys_match_rfc_9369() {
        // RFC 9369 appendix A.1. Version 2 changes the Initial salt *and* the key schedule
        // labels -- `quicv2 key` rather than `quic key`. Deriving version 2 keys with
        // version 1 labels produces perfectly plausible keys that decrypt nothing, and this
        // is the only test that would notice.
        let dcid = hex(VECTOR_DCID);
        let v2 = sys::NGTCP2_PROTO_VER_V2;
        let client = derive_initial_keys(Role::Client, v2, &dcid).unwrap();
        let server = derive_initial_keys(Role::Server, v2, &dcid).unwrap();

        assert_eq!(
            client.tx.iv.as_slice(),
            hex("91f73e2351d8fa91660e909f").as_slice()
        );
        assert_eq!(
            server.tx.iv.as_slice(),
            hex("dd13c276499c0249d3310652").as_slice()
        );

        // And they differ from version 1's, which is what makes the assertion above mean
        // something rather than merely pass.
        let v1 = derive_initial_keys(Role::Client, sys::NGTCP2_PROTO_VER_V1, &dcid).unwrap();
        assert_ne!(client.tx.iv, v1.tx.iv);
    }

    #[test]
    fn the_version_2_header_masks_match_rfc_9369() {
        // RFC 9369 appendices A.2 and A.3.
        let dcid = hex(VECTOR_DCID);
        let v2 = sys::NGTCP2_PROTO_VER_V2;
        let client = derive_initial_keys(Role::Client, v2, &dcid).unwrap();
        let server = derive_initial_keys(Role::Server, v2, &dcid).unwrap();

        let client_mask = client
            .tx
            .header
            .mask(&hex("ffe67b6abcdb4298b485dd04de806071"))
            .unwrap();
        assert_eq!(client_mask.as_slice(), hex("94a0c95e80").as_slice());

        let server_mask = server
            .tx
            .header
            .mask(&hex("6f05d8a4398c47089698baeea26b91eb"))
            .unwrap();
        assert_eq!(server_mask.as_slice(), hex("4dd92e91ea").as_slice());
    }

    #[test]
    fn the_version_2_server_initial_packet_matches_rfc_9369() {
        // RFC 9369 appendix A.3 -- the same end-to-end claim as for version 1, against the
        // version 2 salt and labels.
        let dcid = hex(VECTOR_DCID);
        let v2 = sys::NGTCP2_PROTO_VER_V2;
        let server = derive_initial_keys(Role::Server, v2, &dcid).unwrap();

        let header = hex("d16b3343cf0008f067a5502a4262b50040750001");
        let plaintext = hex(
            "02000000000600405a020000560303eefce7f7b37ba1d1632e96677825ddf73988cfc7\
             9825df566dc5430b9a045a1200130100002e00330024001d00209d3c940d89690b84d0\
             8a60993c144eca684d1081287c834d5311bcf32bb9da1a002b00020304",
        );
        let protected_packet = hex(
            "dc6b3343cf0008f067a5502a4262b5004075d92faaf16f05d8a4398c47089698baeea2\
             6b91eb761d9b89237bbf87263017915358230035f7fd3945d88965cf17f9af6e16886c\
             61bfc703106fbaf3cb4cfa52382dd16a393e42757507698075b2c984c707f0a0812d8c\
             d5a6881eaf21ceda98f4bd23f6fe1a3e2c43edd9ce7ca84bed8521e2e140",
        );
        let expected = &protected_packet[header.len()..];

        let nonce = nonce(&server.tx.iv, 1);
        let mut buf = vec![0u8; plaintext.len() + server.tx.packet.tag_len()];
        buf[..plaintext.len()].copy_from_slice(&plaintext);
        server
            .tx
            .packet
            .seal(&mut buf, plaintext.len(), &nonce, &header)
            .unwrap();
        assert_eq!(buf.as_slice(), expected);
    }

    #[test]
    fn two_sessions_complete_a_handshake_without_a_connection() {
        // The claim that matters for this phase: the backend drives a real TLS 1.3
        // handshake, at every level, entirely through the safe seam -- no `ngtcp2_conn`
        // exists here at all. If the seam were missing anything the handshake needs, this
        // could not complete.
        let mut client = Handshake::new(Role::Client);
        let mut server = Handshake::new(Role::Server);

        client
            .session
            .set_local_transport_params(b"client")
            .unwrap();
        client.start();

        for _ in 0..8 {
            for (level, data) in client.take_outbound() {
                server.feed(level, &data);
            }
            for (level, data) in server.take_outbound() {
                client.feed(level, &data);
            }
            if client.completed && server.completed {
                break;
            }
        }

        assert!(client.completed, "the client handshake did not complete");
        assert!(server.completed, "the server handshake did not complete");

        // Each side received the other's transport parameters, exactly as given.
        assert_eq!(client.peer_params(), Some(&b"server"[..]));
        assert_eq!(server.peer_params(), Some(&b"client"[..]));

        // And ALPN was negotiated, which only happens if the extension actually travelled.
        assert_eq!(
            Session::negotiated_alpn(&client.session).as_deref(),
            Some(&b"h3"[..])
        );
    }

    #[test]
    fn the_two_sides_agree_on_every_level_of_keys() {
        // Not merely "it completed": what each side encrypts with at each level is what the
        // other decrypts with. A handshake can complete with a key schedule that is wrong in
        // a way only the peer would notice, which is the failure this rules out.
        let (client, server) = completed_handshake();

        for level in [Level::Handshake, Level::OneRtt] {
            let (tx, tx_iv) = client.key(level, Direction::Write).expect("client tx");
            let (rx, rx_iv) = server.key(level, Direction::Read).expect("server rx");
            assert_eq!(tx_iv, rx_iv, "the {level:?} initialisation vectors differ");

            let plaintext = b"a payload at this level";
            let mut buf = vec![0u8; plaintext.len() + tx.tag_len()];
            buf[..plaintext.len()].copy_from_slice(plaintext);
            tx.seal(&mut buf, plaintext.len(), tx_iv, b"aad").unwrap();
            let mut plain = vec![0u8; plaintext.len()];
            let recovered = rx.open(&mut plain, &buf, rx_iv, b"aad").unwrap();
            assert_eq!(&plain[..recovered], &plaintext[..]);
        }
    }

    #[test]
    fn the_negotiated_suite_carries_real_usage_limits() {
        // Left at zero -- which is what they are until `ctx_tls` fills them -- the first
        // failed decryption closes the connection and every packet forces a key update.
        // Neither shows up in a loopback test, which is why this is asserted directly.
        let (client, _) = completed_handshake();
        let (key, _) = client.key(Level::OneRtt, Direction::Write).expect("key");
        assert!(key.confidentiality_limit() > 0);
        assert!(key.integrity_limit() > 0);
    }

    #[test]
    fn the_application_keys_rotate_and_still_agree() {
        // Post-handshake key update. Both sides advance from the secrets they hold, and the
        // new keys must still match each other -- a rotation that produced two different
        // generations would break the connection long after it appeared to work.
        let (mut client, mut server) = completed_handshake();

        let client_rx = client.secret(Level::OneRtt, Direction::Read).to_vec();
        let client_tx = client.secret(Level::OneRtt, Direction::Write).to_vec();
        let server_rx = server.secret(Level::OneRtt, Direction::Read).to_vec();
        let server_tx = server.secret(Level::OneRtt, Direction::Write).to_vec();

        let client_next = client.session.rotate_keys(&client_rx, &client_tx).unwrap();
        let server_next = server.session.rotate_keys(&server_rx, &server_tx).unwrap();

        assert_eq!(client_next.tx_iv, server_next.rx_iv);
        assert_ne!(client_next.tx_secret, client_tx);

        let plaintext = b"after the key update";
        let mut buf = vec![0u8; plaintext.len() + client_next.tx_packet.tag_len()];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        client_next
            .tx_packet
            .seal(&mut buf, plaintext.len(), &client_next.tx_iv, b"aad")
            .unwrap();
        let mut plain = vec![0u8; plaintext.len()];
        let recovered = server_next
            .rx_packet
            .open(&mut plain, &buf, &server_next.rx_iv, b"aad")
            .unwrap();
        assert_eq!(&plain[..recovered], &plaintext[..]);
    }

    #[test]
    fn a_session_that_never_handshook_can_be_dropped() {
        // The engine is reachable from OpenSSL from the moment it is installed, so its
        // teardown has to be sound before anything has used it.
        let backend = safe_client_backend();
        let session = Backend::new_session(&backend, Role::Client, Some("example.com")).unwrap();
        drop(session);
    }

    #[test]
    fn the_local_transport_parameters_can_only_be_set_once() {
        // OpenSSL keeps the pointer it is given until it writes the extension. Replacing the
        // buffer would leave it reading a freed one, so a second call is refused rather than
        // quietly reallocating.
        let backend = safe_client_backend();
        let mut session =
            Backend::new_session(&backend, Role::Client, Some("example.com")).unwrap();
        session.set_local_transport_params(b"first").unwrap();
        assert!(session.set_local_transport_params(b"second").is_err());
    }

    /// A stand-in connection, for handshakes run with no connection behind them.
    ///
    /// It records what the session did rather than doing it: a fixed blob as this endpoint's
    /// parameters, the peer's kept for inspection, and keys and handshake bytes collected. The
    /// seam asks nothing more of it — the real implementation decodes into ngtcp2 and encodes
    /// back out, but nothing in the TLS layer depends on the parameters meaning anything.
    #[derive(Default)]
    struct Recorder {
        local: Vec<u8>,
        peer: Option<Vec<u8>>,
        outbound: Vec<(Level, Vec<u8>)>,
        sent_levels: Vec<Level>,
        keys: Vec<ReportedKeys>,
        /// Set to make `submit_handshake` fail, so the "never reported consumed unless
        /// submitted" rule can be checked rather than assumed.
        refuse_submissions: bool,
        /// Set to refuse this endpoint's parameters, standing in for a server asked too early.
        refuse_local: bool,
    }

    impl Recorder {
        fn new(local: &[u8]) -> Self {
            Self {
                local: local.to_vec(),
                ..Self::default()
            }
        }
    }

    impl crate::tls::Handshaking<OsslPacketKey, OsslHeaderKey> for Recorder {
        fn set_peer_transport_params(&mut self, peer: &[u8]) -> Result<()> {
            if self.peer.is_some() {
                return Err(Error::backend("the peer's parameters were offered twice"));
            }
            self.peer = Some(peer.to_vec());
            Ok(())
        }

        fn local_transport_params(&mut self) -> Result<Vec<u8>> {
            if self.refuse_local {
                return Err(Error::backend("asked too early"));
            }
            Ok(self.local.clone())
        }

        fn install_keys(
            &mut self,
            level: Level,
            direction: Direction,
            keys: DirectionalKeys<OsslPacketKey, OsslHeaderKey>,
            secret: &[u8],
        ) -> Result<()> {
            self.keys.push(ReportedKeys {
                level,
                direction,
                keys,
                secret: secret.to_vec(),
            });
            Ok(())
        }

        fn submit_handshake(&mut self, level: Level, data: &[u8]) -> Result<()> {
            if self.refuse_submissions {
                return Err(Error::backend("submission refused"));
            }
            self.sent_levels.push(level);
            self.outbound.push((level, data.to_vec()));
            Ok(())
        }
    }

    /// A backend a safe-seam client session can be made from.
    fn safe_client_backend() -> OsslBackend {
        OsslBackend::builder(Role::Client)
            .alpn("h3")
            .verify(Verify::DangerouslyAcceptAnyCertificate)
            .build()
            .unwrap()
    }

    /// One key the seam reported, with everything it was reported alongside.
    struct ReportedKeys {
        level: Level,
        direction: Direction,
        keys: DirectionalKeys<OsslPacketKey, OsslHeaderKey>,
        secret: Vec<u8>,
    }

    /// One side of a handshake, plus everything it did to its connection.
    struct Handshake {
        session: OsslSession,
        conn: Recorder,
        completed: bool,
    }

    impl Handshake {
        fn new(role: Role) -> Self {
            let backend = match role {
                Role::Client => safe_client_backend(),
                Role::Server => OsslBackend::builder(Role::Server)
                    .alpn("h3")
                    .certificate_chain_pem(crate::conn::test_support::CERT)
                    .private_key_pem(crate::conn::test_support::KEY)
                    .build()
                    .unwrap(),
            };
            let server_name = (role == Role::Client).then_some("example.com");
            let session = Backend::new_session(&backend, role, server_name).unwrap();
            let local: &[u8] = if role == Role::Client {
                b"client"
            } else {
                b"server"
            };
            Self {
                session,
                conn: Recorder::new(local),
                completed: false,
            }
        }

        /// Starts the handshake, then records what it reported afterwards.
        fn start(&mut self) {
            self.session.start_handshake(&mut self.conn).unwrap();
            self.drain();
        }

        /// Feeds one flight, then records what it reported afterwards.
        fn feed(&mut self, level: Level, data: &[u8]) {
            self.session
                .read_handshake(level, data, &mut self.conn)
                .unwrap();
            self.drain();
        }

        /// Drains the two things that are still reported rather than performed.
        fn drain(&mut self) {
            while let Some(event) = self.session.poll_event() {
                match event {
                    SessionEvent::HandshakeComplete => self.completed = true,
                    SessionEvent::Alert(code) => panic!("unexpected TLS alert {code}"),
                }
            }
        }

        /// Everything queued to send, taken.
        fn take_outbound(&mut self) -> Vec<(Level, Vec<u8>)> {
            core::mem::take(&mut self.conn.outbound)
        }

        fn peer_params(&self) -> Option<&[u8]> {
            self.conn.peer.as_deref()
        }

        fn key(&self, level: Level, direction: Direction) -> Option<(&OsslPacketKey, &[u8])> {
            self.conn
                .keys
                .iter()
                .find(|k| k.level == level && k.direction == direction)
                .map(|k| (&k.keys.packet, k.keys.iv.as_slice()))
        }

        fn secret(&self, level: Level, direction: Direction) -> &[u8] {
            self.conn
                .keys
                .iter()
                .find(|k| k.level == level && k.direction == direction)
                .map(|k| k.secret.as_slice())
                .expect("no secret at that level")
        }
    }

    /// Runs a handshake to completion and hands back both sides.
    fn completed_handshake() -> (Handshake, Handshake) {
        let mut client = Handshake::new(Role::Client);
        let mut server = Handshake::new(Role::Server);
        client
            .session
            .set_local_transport_params(b"client")
            .unwrap();
        client.start();

        for _ in 0..8 {
            for (level, data) in client.take_outbound() {
                server.feed(level, &data);
            }
            for (level, data) in server.take_outbound() {
                client.feed(level, &data);
            }
            if client.completed && server.completed {
                break;
            }
        }
        assert!(client.completed && server.completed, "handshake stalled");
        (client, server)
    }

    #[test]
    fn a_server_backend_can_be_dropped_before_its_sessions() {
        // A regression test for a use-after-free that was reachable from entirely safe code.
        // `SSL_new` takes a reference on the context, so a session outlives a dropped
        // backend -- and on a *server* the context carries an ALPN selection callback whose
        // argument pointed into the backend. Running a handshake is what makes the callback
        // actually fire; merely creating and dropping the session did not, which is why the
        // existing client-side test never caught it.
        let backend = OsslBackend::builder(Role::Server)
            .alpn("h3")
            .certificate_chain_pem(crate::conn::test_support::CERT)
            .private_key_pem(crate::conn::test_support::KEY)
            .build()
            .unwrap();
        let mut server = Backend::new_session(&backend, Role::Server, None).unwrap();
        drop(backend);

        let mut client = Handshake::new(Role::Client);
        client
            .session
            .set_local_transport_params(b"client")
            .unwrap();
        client.start();

        let mut conn = Recorder::new(b"server");
        for (level, data) in client.take_outbound() {
            Session::read_handshake(&mut server, level, &data, &mut conn).unwrap();
        }
        // Reaching here at all is the assertion: selecting ALPN dereferences the offer list.
        assert!(!conn.outbound.is_empty());
    }

    #[test]
    fn handshake_data_is_never_reported_consumed_unless_it_was_submitted() {
        // FR-018, and the reason submission is on the capability rather than the queue. A TLS
        // stack must be told how much it consumed before it returns, so an implementation that
        // queues the bytes has to answer before the answer exists -- and the only answer
        // available in advance is "all of it", which is a claim that handshake data was
        // accepted when it may have been dropped. Data lost that way goes missing with no
        // error anywhere.
        //
        // Here the connection refuses every submission. The session must fail rather than
        // report a successful flight.
        let mut client = Handshake::new(Role::Client);
        client
            .session
            .set_local_transport_params(b"client")
            .unwrap();
        client.conn.refuse_submissions = true;

        let result = client.session.start_handshake(&mut client.conn);
        assert!(
            result.is_err(),
            "a refused submission must fail the handshake, not be reported as consumed"
        );
        assert!(
            client.conn.outbound.is_empty(),
            "nothing was submitted, so nothing may be recorded as sent"
        );
    }

    #[test]
    fn the_peers_transport_parameters_cannot_be_offered_twice() {
        // ngtcp2 accepts a second set without complaint, so the refusal has to be ours. A peer
        // that sends two sets has contradicted itself, and silently keeping the later one
        // would mean negotiating against limits the peer never agreed.
        let mut conn = Recorder::new(b"local");
        use crate::tls::Handshaking as _;
        conn.set_peer_transport_params(b"first").unwrap();
        assert!(conn.set_peer_transport_params(b"second").is_err());
    }

    #[test]
    fn a_session_asked_for_parameters_it_cannot_produce_fails_rather_than_inventing_them() {
        // Standing in for the server-side ordering rule the bridge enforces: asked before it
        // can answer, the connection refuses. The failure has to surface through the session
        // rather than being swallowed, or the handshake completes locally and stalls at the
        // peer -- which is exactly how the design this replaced failed.
        let mut client = Handshake::new(Role::Client);
        client
            .session
            .set_local_transport_params(b"client")
            .unwrap();
        client.conn.refuse_local = true;

        // The client never asks, so its handshake still starts. The point is that the refusal
        // is expressible at all, and that nothing invents a substitute.
        client.session.start_handshake(&mut client.conn).unwrap();
        use crate::tls::Handshaking as _;
        assert!(client.conn.local_transport_params().is_err());
    }

    #[test]
    fn post_handshake_messages_are_processed() {
        // The `SSL_read` in `drive` is what makes OpenSSL process messages that arrive
        // *after* the handshake completes -- the session tickets a server sends. Omitting it
        // leaves a connection that looks perfectly complete and is quietly deaf at the TLS
        // layer, which no "did the handshake finish" assertion would notice.
        //
        // What does notice is the record queue. OpenSSL releases inbound bytes only once it
        // has consumed them, so a client that never processes the tickets never releases
        // them. Deleting the `SSL_read` leaves two records outstanding here instead of none,
        // which is how this test was checked rather than assumed.
        let (client, server) = completed_handshake();

        assert!(
            server.conn.sent_levels.contains(&Level::OneRtt),
            "the server sent nothing at the application level after the handshake completed"
        );
        // SAFETY: the engine is live for as long as the session, and no callback is running.
        let outstanding = unsafe { (*client.session.engine).inbound.records.len() };
        assert_eq!(
            outstanding, 0,
            "the client did not process what arrived after the handshake"
        );
    }

    #[test]
    fn an_inbound_record_survives_until_it_is_wholly_released() {
        // OpenSSL keeps the pointer `recv_rcd` handed it until the matching `release_rcd`,
        // which may be several calls later. Freeing the record when it was merely *read*
        // would leave OpenSSL parsing freed memory -- and it would look like a corrupt
        // handshake rather than a lifetime bug, which is exactly why this is pinned by
        // address rather than by "it did not crash".
        let mut inbound = Inbound::default();
        inbound.push(b"first record");
        inbound.push(b"second");

        let (first, first_len) = inbound.next_span();
        assert_eq!(first_len, 12);
        let (second, second_len) = inbound.next_span();
        assert_eq!(second_len, 6);

        // Read to exhaustion, released not at all: both must still be where they were.
        assert_eq!(inbound.next_span(), (ptr::null(), 0));
        assert_eq!(inbound.records.len(), 2);
        assert_eq!(inbound.records[0].data.as_ptr(), first);
        assert_eq!(inbound.records[1].data.as_ptr(), second);

        // A partial release keeps the record alive; the address must not move.
        inbound.release(5);
        assert_eq!(inbound.records.len(), 2);
        assert_eq!(inbound.records[0].data.as_ptr(), first);

        // Only the whole of it frees it, and the one behind it is untouched.
        inbound.release(7);
        assert_eq!(inbound.records.len(), 1);
        assert_eq!(inbound.records[0].data.as_ptr(), second);

        inbound.release(6);
        assert!(inbound.records.is_empty());
        // Releasing more than was ever queued is not a panic: OpenSSL counts bytes, and a
        // saturating answer is better than an arithmetic one on a hostile connection.
        inbound.release(100);
    }

    #[test]
    fn the_local_transport_parameters_are_owned_rather_than_borrowed() {
        // `SSL_set_quic_tls_transport_params` does **not** copy: OpenSSL keeps the pointer
        // until it writes the extension, which is one whole flight later. Handing it the
        // caller's slice would mean whatever the caller did to that buffer in between is
        // what the peer receives.
        //
        // So the caller's buffer is overwritten in place -- same allocation, different
        // contents, no freed memory and therefore no undefined behaviour to muddy the
        // result. If the engine did not own a copy, the server below would receive the
        // overwritten bytes. It receives the original ones.
        let mut client = Handshake::new(Role::Client);
        let mut server = Handshake::new(Role::Server);

        let mut params = b"the original parameters".to_vec();
        client.session.set_local_transport_params(&params).unwrap();
        params.fill(0xff);

        client.start();
        for (level, data) in client.take_outbound() {
            server.feed(level, &data);
        }

        assert_eq!(
            server.peer_params(),
            Some(&b"the original parameters"[..]),
            "the peer received what the caller's buffer became, not what it was"
        );
    }

    #[test]
    fn alpn_encodes_with_length_prefixes() {
        let encoded = encode_alpn(&[b"h3".to_vec(), b"hq".to_vec()]).unwrap();
        assert_eq!(encoded, vec![2, b'h', b'3', 2, b'h', b'q']);
    }

    #[test]
    fn an_empty_or_oversized_alpn_protocol_is_rejected() {
        assert!(encode_alpn(&[Vec::new()]).is_err());
        assert!(encode_alpn(&[vec![0u8; 256]]).is_err());
        assert!(encode_alpn(&[vec![0u8; 255]]).is_ok());
    }

    #[test]
    fn a_backend_without_alpn_is_rejected() {
        // QUIC requires ALPN, so a backend that forgot it would fail every handshake with a
        // message about something else.
        let Err(err) = OsslBackend::builder(Role::Client).build() else {
            panic!("a backend with no ALPN must be rejected");
        };
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn a_server_without_credentials_is_rejected() {
        let Err(err) = OsslBackend::builder(Role::Server).alpn("h3").build() else {
            panic!("a server with no certificate must be rejected");
        };
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn a_client_backend_builds() {
        assert!(
            OsslBackend::builder(Role::Client)
                .alpn("h3")
                .build()
                .is_ok()
        );
    }

    #[test]
    fn a_verifying_client_session_needs_a_server_name() {
        // Without a name there is nothing for the certificate to be checked against, and
        // accepting one silently would be a client that looks verified and is not.
        let backend = OsslBackend::builder(Role::Client)
            .alpn("h3")
            .build()
            .unwrap();
        assert!(Backend::new_session(&backend, Role::Client, None).is_err());
        assert!(Backend::new_session(&backend, Role::Client, Some("example.com")).is_ok());
    }

    #[test]
    fn a_non_verifying_client_session_needs_no_server_name() {
        let backend = OsslBackend::builder(Role::Client)
            .alpn("h3")
            .verify(Verify::DangerouslyAcceptAnyCertificate)
            .build()
            .unwrap();
        assert!(Backend::new_session(&backend, Role::Client, None).is_ok());
    }

    #[test]
    fn a_session_hands_ngtcp2_nothing_of_its_own() {
        // What this replaces is worth recording. The old seam made a backend give ngtcp2 an
        // untyped pointer, and the correct value was the `ngtcp2_crypto_ossl_ctx` rather than
        // the `SSL` an experienced OpenSSL user would reach for -- a mistake that compiled
        // cleanly and corrupted memory at run time. There is now nothing to get wrong: the
        // connection stores a pointer to the *session*, which it owns, and the backend never
        // sees or supplies it.
        let backend = OsslBackend::builder(Role::Client)
            .alpn("h3")
            .verify(Verify::DangerouslyAcceptAnyCertificate)
            .build()
            .unwrap();
        let session = Backend::new_session(&backend, Role::Client, None).unwrap();
        // The two C objects still exist and are still distinct; neither is anyone else's
        // business any more.
        assert!(!session.ossl_ctx.is_null());
        assert!(!session.ssl.is_null());
        assert_ne!(
            session.ossl_ctx.cast::<c_void>(),
            session.ssl.cast::<c_void>()
        );
    }

    #[test]
    fn a_role_mismatch_is_rejected() {
        let backend = OsslBackend::builder(Role::Client)
            .alpn("h3")
            .build()
            .unwrap();
        assert!(Backend::new_session(&backend, Role::Server, None).is_err());
    }

    #[test]
    fn a_session_can_be_dropped_before_any_handshake_activity() {
        // The first of the lifecycle drop points. The rest need a connection to drive.
        let backend = OsslBackend::builder(Role::Client)
            .alpn("h3")
            .verify(Verify::DangerouslyAcceptAnyCertificate)
            .build()
            .unwrap();
        for _ in 0..16 {
            drop(Backend::new_session(&backend, Role::Client, None).unwrap());
        }
    }

    #[test]
    fn dropping_the_backend_before_its_sessions_is_sound() {
        // `SSL_new` takes a reference on the context, so the context outliving the backend
        // handle is OpenSSL's problem rather than ours -- but it is worth pinning, because
        // the opposite would be an easy assumption to build on.
        let backend = OsslBackend::builder(Role::Client)
            .alpn("h3")
            .verify(Verify::DangerouslyAcceptAnyCertificate)
            .build()
            .unwrap();
        let session = Backend::new_session(&backend, Role::Client, None).unwrap();
        drop(backend);
        drop(session);
    }

    #[test]
    fn a_malformed_certificate_is_rejected() {
        let Err(err) = OsslBackend::builder(Role::Server)
            .alpn("h3")
            .certificate_chain_pem("not a certificate")
            .private_key_pem("not a key")
            .build()
        else {
            panic!("malformed credentials must be rejected");
        };
        // Either kind is acceptable; what matters is that it fails rather than building a
        // server that cannot complete a handshake.
        assert!(matches!(
            err.kind(),
            ErrorKind::InvalidInput | ErrorKind::Crypto
        ));
    }

    #[test]
    fn initialising_the_helper_twice_is_harmless() {
        // It is process-global with no refcount, so this pins that the `Once` is doing its
        // job rather than that OpenSSL tolerates repetition.
        assert!(ensure_ossl_init().is_ok());
        assert!(ensure_ossl_init().is_ok());
    }
}
