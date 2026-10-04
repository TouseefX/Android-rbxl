//! The TLS backend seam.
//!
//! QUIC and TLS are inseparable: the transport cannot send a packet before the handshake has
//! produced keys, and the handshake's own messages travel inside QUIC packets. ngtcp2 does not
//! implement TLS. It expects the application to supply a TLS stack, and ships *crypto helper*
//! libraries that bridge to a specific one.
//!
//! This module is the seam that keeps the choice open, and it is implementable in ordinary
//! safe Rust. [`Backend`] describes a configured TLS stack, [`Session`] one connection's
//! handshake, and [`Handshaking`] the connection a session is lent for the length of a call.
//! [`crate::tls_ossl`] implements them over OpenSSL and is enabled by default.
//!
//! # What a backend cannot do, and why that matters
//!
//! It cannot name ngtcp2, hold a raw pointer, or reach the connection except through the four
//! operations of [`Handshaking`]. Every call into ngtcp2 the handshake requires is made by
//! [`crate::tls_bridge`], from what a backend reports or asks for. So the hazards that used to
//! belong to every backend author — which callbacks to install, what a native handle means,
//! when a key may be released, what may be called re-entrantly — are solved once, here, and
//! are covered by tests that fail if they are undone.
//!
//! The seam this replaced was two `unsafe` traits. A backend had to hand ngtcp2 an untyped
//! pointer whose correct value was not the one an experienced OpenSSL user would reach for,
//! and fill in a callback table by hand. Both mistakes compiled cleanly and corrupted memory
//! at run time.
//!
//! # Why this is a trait at all, rather than the OpenSSL backend directly
//!
//! Each crypto helper compiles **its own copy** of ngtcp2's shared crypto code, with
//! backend-specific implementations behind identically-named symbols — `ngtcp2_crypto_ctx_tls`
//! exists separately in `ossl.c`, `wolfssl.c` and `gnutls.c`. And those symbols do not always
//! exist: `ngnet-quic-sys` includes the crypto headers only when a backend feature is on
//! (`wrapper.h:10-18`), so with `--no-default-features` there is no `ngtcp2_crypto_*` symbol in
//! the bindings at all. The seam is what lets the crate build with no TLS stack, exposing the
//! interface and nothing behind it.
//!
//! # Where the remaining difficulty is
//!
//! Not in the safety, which the compiler now carries, but in the *ordering*. Some of what a
//! session does has to take effect before its call returns, and some does not; the split is
//! not arbitrary and is explained on [`Handshaking`]. Getting it wrong produces a handshake
//! that succeeds locally and is rejected by the peer, which is the most expensive failure in
//! this design to diagnose.

use crate::error::Result;

/// Which side of a connection a session is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// Initiates the connection.
    Client,
    /// Accepts it.
    Server,
}

impl Role {
    /// Whether this is the server side.
    pub const fn is_server(self) -> bool {
        matches!(self, Self::Server)
    }
}

/// The length of a header protection mask, from `ngtcp2.h:424`.
pub const HP_MASK_LEN: usize = 5;

/// The number of bytes sampled to produce that mask, from `ngtcp2.h:432`.
pub const HP_SAMPLE_LEN: usize = 16;

/// The stage of the handshake a key or a piece of handshake data belongs to.
///
/// Closed rather than open: QUIC defines exactly these four, and a fifth would be a new
/// protocol rather than a new variant.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Level {
    /// Before any handshake key exists. Protected with keys derived from a connection
    /// identifier, which is why an observer can read them.
    Initial,
    /// Early data. Not implemented by this crate, but part of the level space.
    ZeroRtt,
    /// The handshake itself, once the first keys are agreed.
    Handshake,
    /// Application data.
    OneRtt,
}

/// Which way a key protects.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Direction {
    /// Decrypting what the peer sent.
    Read,
    /// Encrypting what this endpoint sends.
    Write,
}

/// Why a cryptographic operation did not succeed.
///
/// The distinction is the whole point of the type. A packet that fails to decrypt is an
/// **ordinary event** on a QUIC connection: it may have been reordered past the point where
/// its key was discarded, or forged by anyone who can send a datagram. Treating it as fatal
/// hands any third party a way to close the connection, and would never show up in a
/// loopback test, which has neither reordering nor attackers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CryptoError {
    /// The payload did not authenticate. The packet is discarded and the connection
    /// continues.
    Decrypt,
    /// Something is wrong with the backend or its configuration. The connection ends.
    Fatal,
}

/// A key that protects one direction of one encryption level's payloads.
///
/// # Why this is an object rather than a method on the session
///
/// ngtcp2's `encrypt`, `decrypt` and `hp_mask` callbacks receive neither the connection nor
/// any user pointer (`ngtcp2.h:2824`, `:2853`, `:2882`). The only state they can reach is
/// what was stored in the cipher context handed to them. A key therefore has to be a
/// self-contained object, and this shape is forced by those signatures rather than chosen.
///
/// # Why protection happens in place
///
/// ngtcp2 says of both callbacks that "`dest` and `plaintext` may point to the same buffer"
/// (`ngtcp2.h:2818`, `:2846`) — and in the ordinary case they do, because it protects the
/// packet where it already sits. Two overlapping slices, one shared and one mutable, cannot
/// be formed in safe Rust at all. Working in place sidesteps that, and has the side benefit
/// of keeping the send path free of allocation.
///
/// # Why `'static` and `Send`
///
/// The object outlives every borrow of the session that made it: it is recovered from an
/// untyped handle long afterwards, and destroyed from inside `ngtcp2_conn_del`, which runs
/// before the session itself is dropped. A key that borrowed from its session would be a
/// dangling borrow with no lifetime left to catch it. `Send` is required because a
/// connection is `Send`, and state hidden behind a raw handle would otherwise escape that
/// bound.
pub trait PacketKey: Send + 'static {
    /// Protects `buf[..plaintext_len]` in place.
    ///
    /// On entry the plaintext occupies the first `plaintext_len` bytes. On success the
    /// ciphertext and its authentication tag occupy the first `plaintext_len + tag_len()`
    /// bytes, and `buf` must be at least that long.
    fn seal(
        &self,
        buf: &mut [u8],
        plaintext_len: usize,
        nonce: &[u8],
        aad: &[u8],
    ) -> core::result::Result<(), CryptoError>;

    /// Unprotects `ciphertext` into `dest`, returning the plaintext length.
    ///
    /// On entry `ciphertext` holds the protected payload followed by its authentication tag.
    /// On success the first `ciphertext.len() - tag_len()` bytes of `dest` hold the
    /// plaintext, and that length is returned; `dest` must be at least that long.
    ///
    /// The destination and the ciphertext are separate, non-overlapping buffers. ngtcp2's
    /// core always decrypts a received packet into a buffer distinct from the packet itself,
    /// so this crate's bridge never hands a backend two aliasing pointers — even though the C
    /// header permits it (`ngtcp2.h:2846`). A backend whose primitive decrypts in place must
    /// copy `ciphertext` into `dest` itself; this crate's own does not, which is the copy
    /// this seam exists to avoid.
    ///
    /// Returns [`CryptoError::Decrypt`] when the payload does not authenticate. That is not
    /// a failure of the backend and must not end the connection.
    fn open(
        &self,
        dest: &mut [u8],
        ciphertext: &[u8],
        nonce: &[u8],
        aad: &[u8],
    ) -> core::result::Result<usize, CryptoError>;

    /// How many bytes protection adds. ngtcp2 budgets every packet against this.
    fn tag_len(&self) -> usize;

    /// How many packets may be protected with this key before it must be retired.
    ///
    /// Not advisory. ngtcp2 compares a packet count against it and forces a key update, and
    /// a zero here means "immediately", so a backend that forgets it produces a connection
    /// that rekeys constantly rather than one that never does.
    fn confidentiality_limit(&self) -> u64;

    /// How many failed decryptions may be tolerated before the connection must close.
    ///
    /// Zero has the same trap as above, in the other direction: it makes the first forged
    /// packet fatal.
    fn integrity_limit(&self) -> u64;
}

/// A key that masks the parts of a packet header which are not covered by the payload AEAD.
///
/// Kept separate from [`PacketKey`] because ngtcp2 keeps them separate — they are installed
/// as different context objects, use different ciphers, and are rotated on different
/// schedules: a key update replaces packet keys and leaves header keys alone.
pub trait HeaderKey: Send + 'static {
    /// Derives the mask ngtcp2 will apply to a header, from a sample of the protected
    /// payload.
    ///
    /// `sample` is [`HP_SAMPLE_LEN`] bytes. Returning the mask rather than applying it is
    /// what ngtcp2 asks for; note that this is the one place a rustls-backed implementation
    /// would have work to do, since rustls only ever applies header protection in place and
    /// never surfaces the mask.
    fn mask(&self, sample: &[u8]) -> core::result::Result<[u8; HP_MASK_LEN], CryptoError>;
}

/// A level's initialisation vector, held inline.
///
/// ngtcp2 accepts a vector between `MIN_IV_LEN` (8) and `MAX_IV_LEN` (64) bytes; both bounds
/// guard a fixed stack buffer whose overrun release builds do not catch. Storing the bytes in
/// an array of that maximum makes a longer one impossible to express rather than merely
/// rejected, so no path through a backend can hand ngtcp2 an over-length vector. Construction
/// is fallible: a backend that produces one outside the accepted range is refused here, at the
/// seam, rather than deeper in.
#[derive(Clone, PartialEq, Eq)]
pub struct Iv {
    bytes: [u8; crate::validate::MAX_IV_LEN],
    len: u8,
}

impl Iv {
    /// Copies `bytes` inline, rejecting a length ngtcp2 could not handle.
    pub fn new(bytes: &[u8]) -> Result<Self> {
        crate::validate::iv_len(bytes.len())?;
        let mut inline = [0u8; crate::validate::MAX_IV_LEN];
        inline[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            bytes: inline,
            len: bytes.len() as u8,
        })
    }

    /// The vector's bytes, without the inline padding behind them.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

impl core::ops::Deref for Iv {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl core::fmt::Debug for Iv {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The padding behind the length is not part of the value and is not shown.
        self.as_slice().fmt(f)
    }
}

/// One direction's key material for one encryption level.
///
/// The initialisation vector travels alongside the keys rather than inside them because
/// ngtcp2 installs it separately and constructs each packet's nonce from it itself.
#[derive(Debug)]
pub struct DirectionalKeys<P, H> {
    /// Protects payloads.
    pub packet: P,
    /// Masks headers.
    pub header: H,
    /// The initialisation vector the nonce is built from.
    pub iv: Iv,
}

/// Both directions of the keys derived from a connection identifier.
///
/// Initial keys are the one part of QUIC's key schedule that owes nothing to the TLS
/// handshake: they come from the client's destination connection identifier and a
/// version-specific salt, which is why an observer can read Initial packets and why these
/// have to be derivable before any handshake byte has been exchanged.
#[derive(Debug)]
pub struct InitialKeys<P, H> {
    /// For decrypting the peer.
    pub rx: DirectionalKeys<P, H>,
    /// For encrypting to the peer.
    pub tx: DirectionalKeys<P, H>,
}

/// The next generation of application keys, produced by a key update.
///
/// A separate type from [`InitialKeys`], and deliberately narrower, because a key update is
/// a narrower operation than deriving a level's keys: it rotates payload protection only.
/// Header protection keys are **not** rotated (`shared.c:1049-1063` passes a null header key
/// to the derivation), so offering a place to put them would invite a backend to derive keys
/// that are then silently discarded.
///
/// The new traffic secrets are returned alongside the keys because ngtcp2 keeps them and
/// hands them back as the input to the *next* rotation. A backend that returns keys without
/// secrets has produced a connection that can rotate exactly once.
#[derive(Debug)]
pub struct RotatedKeys<P> {
    /// The new key for decrypting the peer.
    pub rx_packet: P,
    /// Its initialisation vector.
    pub rx_iv: Iv,
    /// The secret the generation after next is derived from.
    pub rx_secret: Vec<u8>,
    /// The new key for encrypting to the peer.
    pub tx_packet: P,
    /// Its initialisation vector.
    pub tx_iv: Iv,
    /// The secret the generation after next is derived from.
    pub tx_secret: Vec<u8>,
}

/// The connection, lent to a session for the length of one call.
///
/// # Why this exists at all
///
/// The seam would be simpler if a session only ever *reported* things and the crate applied
/// them afterwards. That was this crate's original design, and it is wrong — not stylistically,
/// but in a way that produces a QUIC server no other implementation will talk to. Three facts
/// combine:
///
/// 1. A server's own transport parameters name the version it settled on, and the QUIC library
///    fills that in only while decoding the *peer's* parameters. Encoded before that, the field
///    is zero, which the peer rejects as malformed.
/// 2. They also carry the connection identifier the server is using, which the library fills in
///    only while installing the handshake write key. Encoded before that, the parameter is
///    absent, and the peer rejects the set as incomplete.
/// 3. Both must land before the TLS stack writes the message carrying them, which it does
///    without returning from the call that delivered the peer's.
///
/// There is therefore no moment *between* those steps at which the crate is in control. These
/// operations have to take effect while the TLS stack is still on the stack, so a session
/// performs them rather than reporting them.
///
/// Both failure modes were found by running handshakes rather than by reading headers, and both
/// looked identical from this side: everything local succeeded, and the peer went quiet.
///
/// # Why it is not a hole in the safety claim
///
/// It carries bytes and key objects. It names nothing belonging to the QUIC library, offers no
/// operation beyond these four, and — being borrowed rather than owned — cannot be kept past
/// the call that lent it. The compiler enforces that last point rather than the documentation:
///
/// ```compile_fail
/// use ngnet_quic::{Handshaking, Level, Result, Session};
/// # use ngnet_quic::{CryptoError, HeaderKey, InitialKeys, PacketKey, RotatedKeys, SessionEvent};
/// # struct K;
/// # impl PacketKey for K {
/// #     fn seal(&self, _: &mut [u8], _: usize, _: &[u8], _: &[u8]) -> core::result::Result<(), CryptoError> { Ok(()) }
/// #     fn open(&self, dest: &mut [u8], ciphertext: &[u8], _: &[u8], _: &[u8]) -> core::result::Result<usize, CryptoError> { dest[..ciphertext.len()].copy_from_slice(ciphertext); Ok(ciphertext.len()) }
/// #     fn tag_len(&self) -> usize { 16 }
/// #     fn confidentiality_limit(&self) -> u64 { 1 }
/// #     fn integrity_limit(&self) -> u64 { 1 }
/// # }
/// # impl HeaderKey for K {
/// #     fn mask(&self, _: &[u8]) -> core::result::Result<[u8; 5], CryptoError> { Ok([0; 5]) }
/// # }
/// // A backend that keeps the connection instead of using it and letting it go.
/// struct Hoarder<'a> {
///     kept: Option<&'a mut dyn Handshaking<K, K>>,
/// }
///
/// impl Session for Hoarder<'static> {
///     type PacketKey = K;
///     type HeaderKey = K;
///
///     fn read_handshake(
///         &mut self,
///         _level: Level,
///         _data: &[u8],
///         conn: &mut dyn Handshaking<K, K>,
///     ) -> Result<()> {
///         self.kept = Some(conn); // the borrow does not outlive the call, so this cannot compile
///         Ok(())
///     }
/// #   fn initial_keys(&mut self, _: u32, _: &[u8]) -> Result<InitialKeys<K, K>> { unimplemented!() }
/// #   fn retry_key(&mut self, _: u32) -> Result<K> { unimplemented!() }
/// #   fn set_local_transport_params(&mut self, _: &[u8]) -> Result<()> { Ok(()) }
/// #   fn start_handshake(&mut self, _: &mut dyn Handshaking<K, K>) -> Result<()> { Ok(()) }
/// #   fn poll_event(&mut self) -> Option<SessionEvent> { None }
/// #   fn rotate_keys(&mut self, _: &[u8], _: &[u8]) -> Result<RotatedKeys<K>> { unimplemented!() }
/// #   fn negotiated_alpn(&self) -> Option<Vec<u8>> { None }
/// }
/// ```
pub trait Handshaking<P, H> {
    /// Hands over the peer's transport parameters, exactly as they arrived.
    ///
    /// Must come before [`Handshaking::local_transport_params`], because on a server it is what
    /// determines part of this endpoint's own set. Offering them twice is an error: a peer that
    /// sends two sets has contradicted itself, and the QUIC library will not notice.
    fn set_peer_transport_params(&mut self, peer: &[u8]) -> Result<()>;

    /// Returns this endpoint's transport parameters, encoded and ready to send.
    ///
    /// A **server** must call this only after installing its handshake write key, because that
    /// is the moment the QUIC library completes the set. Calling it earlier is an error rather
    /// than a short answer — the library will otherwise encode an incomplete set quite happily,
    /// and the mistake surfaces as a peer that stops responding.
    ///
    /// A **client** does not call it at all: its parameters are settled before the handshake
    /// begins and arrive through [`Session::set_local_transport_params`].
    fn local_transport_params(&mut self) -> Result<Vec<u8>>;

    /// Installs one direction's key material for one encryption level.
    ///
    /// Immediate rather than reported, because installing the handshake write key is one of the
    /// two steps that complete a server's transport parameters.
    fn install_keys(
        &mut self,
        level: Level,
        direction: Direction,
        keys: DirectionalKeys<P, H>,
        secret: &[u8],
    ) -> Result<()>;

    /// Submits outbound handshake bytes at a level.
    ///
    /// Immediate for a different reason: a TLS stack must be told how much of what it offered
    /// was taken *before* it returns. Deferring the submission means answering that question
    /// before the answer exists, and the only answer available in advance — "all of it" — is a
    /// claim that handshake data was accepted when it may not have been.
    ///
    /// All of `data` is submitted or none of it is. There is no partial success to report.
    fn submit_handshake(&mut self, level: Level, data: &[u8]) -> Result<()>;
}

/// The things a session reports that nothing has to observe before its call returns.
///
/// Two, and no more. Everything else a session has to say — handshake bytes, key material, the
/// peer's transport parameters — has an effect something downstream depends on immediately,
/// and so travels through [`Handshaking`] instead. What is left here genuinely can wait: a
/// completed handshake and an alert are both facts about what has *already* happened, and
/// nothing the TLS stack does next reads them back.
///
/// A queue rather than a pair of flags because order still matters between the two. A
/// handshake that completes and then alerts is a different connection from one that alerts
/// and then completes.
///
/// No longer generic over the key types, because keys no longer travel this way.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SessionEvent {
    /// The TLS handshake completed. Distinct from the QUIC handshake being confirmed.
    HandshakeComplete,
    /// The peer must be told the handshake failed, with this alert code.
    Alert(u8),
}

/// One connection's TLS state.
///
/// # What this trait deliberately cannot do
///
/// It cannot reach the connection. Every ngtcp2 call the handshake requires — installing
/// keys, submitting handshake data, setting the alert, decoding transport parameters — is
/// made by the crate, from what this trait reports. That is the difference between this and
/// the seam it replaces, and it is what makes implementing it safe: there is no pointer to
/// misuse and no ordering to get wrong, because the ordering is not the backend's to choose.
///
/// It also has no source of randomness. The connection already has one, and the reason it
/// has exactly one is that two could diverge.
pub trait Session: Send + 'static {
    /// The payload protection key this backend produces.
    ///
    /// A concrete type rather than a boxed trait object, for a mechanical reason: the key is
    /// stored in a single untyped pointer, and a `Box<dyn Trait>` is two words wide. It
    /// would not fit. Monomorphisation also means only this session's keys can ever reach
    /// this session's callbacks, so a key from one backend cannot be handed to another.
    type PacketKey: PacketKey;

    /// The header protection key this backend produces.
    type HeaderKey: HeaderKey;

    /// Derives the Initial keys for a connection identifier and QUIC version.
    ///
    /// Called before the handshake starts, and again if the server sends a Retry — in which
    /// case the identifier is the one the Retry carried, and the previous Initial keys are
    /// discarded. Also called for a version the peer negotiated down to.
    fn initial_keys(
        &mut self,
        version: u32,
        dcid: &[u8],
    ) -> Result<InitialKeys<Self::PacketKey, Self::HeaderKey>>;

    /// The fixed key that authenticates a Retry packet's integrity tag.
    ///
    /// Clients only, and not optional for them: ngtcp2 verifies the tag before it will
    /// accept a Retry at all, using this key through the ordinary encryption path. The key
    /// is not secret — it is a constant in the specification, one per version — so this
    /// exists to say which constant, not to protect anything.
    fn retry_key(&mut self, version: u32) -> Result<Self::PacketKey>;

    /// Hands the session the local transport parameters to send.
    ///
    /// The **client's** route, and only the client's: a client's parameters are final before
    /// its first flight. A server's are not final until the handshake is under way, so a
    /// server obtains them through [`Handshaking::local_transport_params`] instead.
    fn set_local_transport_params(&mut self, params: &[u8]) -> Result<()>;

    /// Starts the handshake. Clients only; a server starts when the client's first flight
    /// arrives.
    ///
    /// `conn` is lent for the duration of the call and cannot be retained.
    fn start_handshake(
        &mut self,
        conn: &mut dyn Handshaking<Self::PacketKey, Self::HeaderKey>,
    ) -> Result<()>;

    /// Feeds the session handshake bytes that arrived at `level`.
    ///
    /// # This is a stream, not a message
    ///
    /// One call does **not** mean one handshake message. The QUIC library deliberately splits
    /// a client's first flight across CRYPTO frames, so the first thing a server sees here is
    /// typically a few bytes of a message tens of bytes long, followed by the rest across
    /// several more calls. A backend must reassemble.
    ///
    /// This is worth stating plainly because a real TLS stack already does it — OpenSSL's
    /// record layer buffers, so the OpenSSL backend never had to think about it — and a
    /// backend written against the obvious reading of this signature fails on the first
    /// connection it makes. That is exactly how it was found here.
    ///
    /// `conn` is lent on the same terms as above. Everything the session must make happen
    /// *while the TLS stack is still running* goes through it; everything else is reported
    /// afterwards through [`Session::poll_event`].
    fn read_handshake(
        &mut self,
        level: Level,
        data: &[u8],
        conn: &mut dyn Handshaking<Self::PacketKey, Self::HeaderKey>,
    ) -> Result<()>;

    /// Takes the next thing the session has to report, or `None` when it has nothing.
    ///
    /// Drained to exhaustion after every call that can produce events.
    fn poll_event(&mut self) -> Option<SessionEvent>;

    /// Rotates the application keys, given the secrets currently in use.
    ///
    /// Returns the next generation for both directions, with the secrets the generation
    /// after that will be derived from. Only the session can do this: it alone knows which
    /// hash the negotiated cipher suite uses.
    fn rotate_keys(
        &mut self,
        rx_secret: &[u8],
        tx_secret: &[u8],
    ) -> Result<RotatedKeys<Self::PacketKey>>;

    /// The application protocol the handshake agreed on, once it has one.
    fn negotiated_alpn(&self) -> Option<Vec<u8>>;

    /// Why the handshake failed, in whatever terms the backend can offer.
    ///
    /// Exists so a failure can name certificate verification rather than only reporting that
    /// the handshake did not complete.
    fn failure_reason(&self) -> Option<String> {
        None
    }
}

/// A configured TLS stack, from which per-connection sessions are made.
pub trait Backend {
    /// The session type this backend produces.
    type Session: Session;

    /// Creates a session for one connection.
    ///
    /// `server_name` is used for SNI and certificate verification on the client side, and
    /// ignored by a server.
    fn new_session(&self, role: Role, server_name: Option<&str>) -> Result<Self::Session>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_knows_whether_it_is_a_server() {
        assert!(Role::Server.is_server());
        assert!(!Role::Client.is_server());
    }

    /// The header protection lengths are restated here, so they must match the library's.
    ///
    /// They are restated rather than re-exported because they belong to the seam's
    /// vocabulary: a backend author should not have to reach into the raw bindings to learn
    /// how long a sample is. That convenience is only safe while the two agree, and a
    /// mismatch would be a buffer of the wrong length handed to a cipher, so it is pinned
    /// rather than assumed.
    #[test]
    fn the_header_protection_lengths_match_the_library() {
        assert_eq!(
            HP_MASK_LEN,
            ngnet_quic_sys::NGTCP2_HP_MASKLEN as usize,
            "header protection mask length"
        );
        assert_eq!(
            HP_SAMPLE_LEN,
            ngnet_quic_sys::NGTCP2_HP_SAMPLELEN as usize,
            "header protection sample length"
        );
    }

    /// A failed decryption is not a failed backend, and the type has to keep them apart.
    ///
    /// Trivial as an assertion, and worth having as a statement: every other error in this
    /// crate means something went wrong, and this one usually means the connection is
    /// working correctly in the presence of a reordered or forged packet.
    #[test]
    fn a_failed_decryption_is_distinguishable_from_a_fatal_error() {
        assert_ne!(CryptoError::Decrypt, CryptoError::Fatal);
    }

    /// Every encryption level is distinct, since they index key state.
    #[test]
    fn the_encryption_levels_are_distinct() {
        let levels = [
            Level::Initial,
            Level::ZeroRtt,
            Level::Handshake,
            Level::OneRtt,
        ];
        for (index, left) in levels.iter().enumerate() {
            for right in &levels[index + 1..] {
                assert_ne!(left, right, "two encryption levels compare equal");
            }
        }
        assert_ne!(Direction::Read, Direction::Write);
    }
}
