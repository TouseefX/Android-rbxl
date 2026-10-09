//! Address validation, and answering datagrams that belong to no connection.
//!
//! # What a server is defending against
//!
//! A QUIC server completes a handshake in response to a first packet, and the handshake is
//! several times larger than that packet. A spoofed source address therefore turns the
//! server into an amplifier pointed at whoever the attacker names, and the attacker pays a
//! fraction of what the victim receives.
//!
//! Address validation closes that. Instead of answering an unvalidated first packet with a
//! handshake, the server answers with a **Retry**: a small packet carrying an opaque token,
//! and no connection state at all. A client that genuinely holds the address it claimed
//! receives the Retry and comes back with the token; a spoofed source never does, because
//! the Retry went to the victim rather than to the attacker.
//!
//! The token has to be unforgeable, and it has to be checkable without remembering it —
//! remembering would reintroduce the state Retry exists to avoid. Both are the reason the
//! tokens here are derived rather than random: they are authenticated with a secret only
//! the server knows, and they carry the address and the time they were issued.
//!
//! # Why this needs the bundled TLS backend
//!
//! Writing a Retry packet is not a matter of assembling bytes: the packet carries an
//! integrity tag computed with AEAD, and `ngtcp2_pkt_write_retry` takes an encryption
//! callback and an initialised AEAD context to produce it. Those, and the token helpers
//! themselves, come from ngtcp2's crypto helper library — which
//! `crates/ngnet-quic-sys/wrapper.h` includes only when a TLS backend is enabled.
//!
//! So this module is gated on `tls-ossl`. A caller supplying their own TLS backend can
//! still run a server; they cannot use this, and asking for it fails loudly rather than
//! producing a server that validates nothing.

#![cfg(feature = "tls-ossl")]

use core::net::SocketAddr;

use ngnet_quic_sys as sys;

use crate::cid::ConnectionId;
use crate::error::{Error, Result};
use crate::path::PathStorage;
use crate::time::{Duration, Timestamp};

/// The largest Retry token ngtcp2 will produce.
const MAX_RETRY_TOKEN: usize = 256;

/// The length of a stateless reset token.
pub const RESET_TOKEN_LEN: usize = sys::NGTCP2_STATELESS_RESET_TOKENLEN as usize;

/// The fewest random bytes a stateless reset may carry before its token.
pub const MIN_RESET_RANDOM: usize = sys::NGTCP2_MIN_STATELESS_RESET_RANDLEN as usize;

/// The secret a server authenticates its tokens with.
///
/// One secret backs both Retry tokens and stateless reset tokens, so an operator configures
/// one thing rather than two. It must be kept private and should be stable across restarts:
/// changing it invalidates every token in flight, which costs one extra round trip per
/// client rather than anything worse.
#[derive(Clone)]
pub struct TokenSecret {
    bytes: Vec<u8>,
}

impl TokenSecret {
    /// Takes a secret.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] for a secret shorter than sixteen bytes, which
    /// is short enough that guessing it is a realistic attack rather than a theoretical one.
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    pub fn new(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 16 {
            return Err(Error::invalid_input(
                "a token secret must be at least 16 bytes",
            ));
        }
        Ok(Self {
            bytes: bytes.to_vec(),
        })
    }
}

impl core::fmt::Debug for TokenSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print the bytes. A secret in a log is a secret disclosed.
        f.debug_struct("TokenSecret").finish_non_exhaustive()
    }
}

/// Mints a Retry token for a client at `remote`.
///
/// `retry_scid` is the identifier the server puts in the Retry packet as its own source;
/// `original_dcid` is the identifier the client addressed its first packet to. Verification
/// recovers the latter, which the server needs in order to build a connection at all.
///
/// # Errors
///
/// Returns a native error if ngtcp2 refuses, which for a well-formed call means the
/// address family is one it does not handle.
pub fn issue_retry_token(
    secret: &TokenSecret,
    version: u32,
    remote: SocketAddr,
    retry_scid: &ConnectionId,
    original_dcid: &ConnectionId,
    now: Timestamp,
) -> Result<Vec<u8>> {
    let path = PathStorage::new(remote, remote);
    let mut token = vec![0u8; MAX_RETRY_TOKEN];

    // SAFETY: the token buffer is at least the documented maximum, the secret and both
    // identifiers are readable for their lengths, and the address outlives the call.
    let written = unsafe {
        sys::ngtcp2_crypto_generate_retry_token(
            token.as_mut_ptr(),
            secret.bytes.as_ptr(),
            secret.bytes.len(),
            version,
            path.remote_sockaddr(),
            path.remote_socklen(),
            retry_scid.as_raw(),
            original_dcid.as_raw(),
            now.as_raw(),
        )
    };
    if written < 0 {
        return Err(Error::invalid_input("could not mint a Retry token"));
    }
    token.truncate(written as usize);
    Ok(token)
}

/// Checks a Retry token and recovers the identifier the client first addressed.
///
/// Returns `Ok(None)` when the token does not verify — expired, tampered with, or issued
/// to a different address. That is an ordinary outcome on a public socket rather than an
/// error, which is why it is not one: a server answers it with another Retry, not with a
/// failure.
///
/// `lifetime` is how long a token stays valid. Shorter is safer and costs a client an extra
/// round trip if it dawdles; ngtcp2's own example uses ten seconds.
///
/// # Errors
///
/// Returns [`ErrorKind::InvalidInput`] if the recovered identifier is not one QUIC permits.
///
/// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
pub fn verify_retry_token(
    secret: &TokenSecret,
    token: &[u8],
    version: u32,
    remote: SocketAddr,
    dcid: &ConnectionId,
    lifetime: Duration,
    now: Timestamp,
) -> Result<Option<ConnectionId>> {
    if token.is_empty() {
        return Ok(None);
    }
    let path = PathStorage::new(remote, remote);
    // SAFETY: a zeroed identifier is a valid output buffer for this call.
    let mut recovered = unsafe { core::mem::zeroed::<sys::ngtcp2_cid>() };

    // SAFETY: every pointer is readable for the length given alongside it, and `recovered`
    // is a writable identifier ngtcp2 fills on success.
    let rc = unsafe {
        sys::ngtcp2_crypto_verify_retry_token(
            &raw mut recovered,
            token.as_ptr(),
            token.len(),
            secret.bytes.as_ptr(),
            secret.bytes.len(),
            version,
            path.remote_sockaddr(),
            path.remote_socklen(),
            dcid.as_raw(),
            lifetime.as_raw(),
            now.as_raw(),
        )
    };
    if rc != 0 {
        return Ok(None);
    }
    Ok(Some(ConnectionId::from_raw(&recovered)))
}

/// Writes a Retry packet into `dest`.
///
/// The identifiers are **swapped** relative to the packet that prompted it, as in a Version
/// Negotiation: the client's source becomes the destination. `scid` is a fresh identifier
/// the server chooses for this Retry, and must be the same one the token was minted
/// against, or verification will reject the client's next packet.
///
/// # Errors
///
/// Returns a native error if `dest` is too small.
pub fn write_retry(
    dest: &mut [u8],
    version: u32,
    dcid: &ConnectionId,
    scid: &ConnectionId,
    original_dcid: &ConnectionId,
    token: &[u8],
) -> Result<usize> {
    // SAFETY: `dest` is writable for its length, the three identifiers are valid, and the
    // token is readable for its length. Everything is copied.
    let written = unsafe {
        sys::ngtcp2_crypto_write_retry(
            dest.as_mut_ptr(),
            dest.len(),
            version,
            dcid.as_raw(),
            scid.as_raw(),
            original_dcid.as_raw(),
            token.as_ptr(),
            token.len(),
        )
    };
    if written < 0 {
        return Err(Error::native(
            written as i32,
            "could not write a Retry packet",
        ));
    }
    Ok(written as usize)
}

/// Derives the stateless reset token for a connection identifier.
///
/// The same identifier and secret always give the same token, which is what lets a server
/// that has lost all its state still produce the right one — and is the entire point of a
/// stateless reset.
///
/// # Errors
///
/// Returns a native error if ngtcp2's key derivation fails.
pub fn reset_token(secret: &TokenSecret, cid: &ConnectionId) -> Result<[u8; RESET_TOKEN_LEN]> {
    let mut token = [0u8; RESET_TOKEN_LEN];
    // SAFETY: the buffer is exactly the documented length, and the secret and identifier
    // are readable for theirs.
    let rc = unsafe {
        sys::ngtcp2_crypto_generate_stateless_reset_token(
            token.as_mut_ptr(),
            secret.bytes.as_ptr(),
            secret.bytes.len(),
            cid.as_raw(),
        )
    };
    if rc != 0 {
        return Err(Error::invalid_input(
            "could not derive a stateless reset token",
        ));
    }
    Ok(token)
}

/// Writes a stateless reset into `dest`.
///
/// `random` is the unpredictable prefix that makes the packet indistinguishable from an
/// ordinary short-header one; it must be at least [`MIN_RESET_RANDOM`] bytes.
///
/// # A stateless reset must be smaller than what provoked it
///
/// Otherwise the mechanism for telling a peer "I have lost your connection" becomes an
/// amplifier of its own, and a spoofed datagram draws a larger one at the victim. The
/// caller controls that through the length of `random`, and
/// [`write_stateless_reset_smaller_than`] does the arithmetic.
///
/// # Errors
///
/// Returns a native error if `dest` is too small or `random` is too short.
pub fn write_stateless_reset(
    dest: &mut [u8],
    token: &[u8; RESET_TOKEN_LEN],
    random: &[u8],
) -> Result<usize> {
    let raw = sys::ngtcp2_stateless_reset_token { data: *token };
    // SAFETY: `dest` is writable for its length, the token is exactly the required size,
    // and `random` is readable for its length.
    let written = unsafe {
        sys::ngtcp2_pkt_write_stateless_reset2(
            dest.as_mut_ptr(),
            dest.len(),
            &raw,
            random.as_ptr(),
            random.len(),
        )
    };
    if written < 0 {
        return Err(Error::native(
            written as i32,
            "could not write a stateless reset",
        ));
    }
    Ok(written as usize)
}

/// Writes a stateless reset strictly smaller than the datagram that provoked it.
///
/// Returns `Ok(None)` when no such packet can be built — when the triggering datagram is
/// too small to leave room for a token and the minimum random prefix. Declining is correct
/// there: sending anyway would amplify.
///
/// # Errors
///
/// Returns a native error if ngtcp2 refuses to write the packet.
pub fn write_stateless_reset_smaller_than(
    dest: &mut [u8],
    token: &[u8; RESET_TOKEN_LEN],
    random: &[u8],
    provoking_len: usize,
) -> Result<Option<usize>> {
    // The packet is the random prefix plus the token, so the prefix is what the budget
    // buys. One byte smaller than the trigger is the requirement.
    let Some(budget) = provoking_len.checked_sub(RESET_TOKEN_LEN + 1) else {
        return Ok(None);
    };
    if budget < MIN_RESET_RANDOM {
        return Ok(None);
    }
    let prefix = budget.min(random.len());
    if prefix < MIN_RESET_RANDOM {
        return Ok(None);
    }

    let written = write_stateless_reset(dest, token, &random[..prefix])?;
    if written >= provoking_len {
        // ngtcp2 truncates rather than failing, so this is belt and braces -- but the
        // property is the whole point of the function, so it is checked rather than assumed.
        return Ok(None);
    }
    Ok(Some(written))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> TokenSecret {
        TokenSecret::new(&[0x5a; 32]).expect("a valid secret")
    }

    fn addr() -> SocketAddr {
        "127.0.0.1:4433".parse().expect("valid")
    }

    fn now() -> Timestamp {
        Timestamp::from_nanos(1_000_000_000).expect("valid")
    }

    fn lifetime() -> Duration {
        Duration::from_nanos(10_000_000_000)
    }

    #[test]
    fn a_short_secret_is_refused() {
        assert!(TokenSecret::new(&[0; 8]).is_err());
        assert!(TokenSecret::new(&[0; 16]).is_ok());
    }

    #[test]
    fn a_secret_does_not_print_itself() {
        // A secret in a log is a secret disclosed, and `Debug` is how that happens.
        let rendered = format!("{:?}", secret());
        assert!(!rendered.contains("5a"), "the secret appeared in its Debug");
        assert!(!rendered.contains("90"), "the secret appeared in its Debug");
    }

    #[test]
    fn a_freshly_minted_token_verifies_and_recovers_the_original_identifier() {
        let scid = ConnectionId::new(&[1; 8]).expect("valid");
        let odcid = ConnectionId::new(&[2; 8]).expect("valid");
        let token =
            issue_retry_token(&secret(), crate::VERSION_V1, addr(), &scid, &odcid, now())
                .expect("minting");

        let recovered = verify_retry_token(
            &secret(),
            &token,
            crate::VERSION_V1,
            addr(),
            &scid,
            lifetime(),
            now(),
        )
        .expect("verifying")
        .expect("a fresh token must verify");

        assert_eq!(
            recovered, odcid,
            "verification must recover the identifier the client first addressed, or the \
             server cannot build a connection for it"
        );
    }

    #[test]
    fn a_tampered_token_is_rejected() {
        let scid = ConnectionId::new(&[1; 8]).expect("valid");
        let odcid = ConnectionId::new(&[2; 8]).expect("valid");
        let mut token =
            issue_retry_token(&secret(), crate::VERSION_V1, addr(), &scid, &odcid, now())
                .expect("minting");
        let last = token.len() - 1;
        token[last] ^= 0xff;

        let verified = verify_retry_token(
            &secret(),
            &token,
            crate::VERSION_V1,
            addr(),
            &scid,
            lifetime(),
            now(),
        )
        .expect("verifying");
        assert!(verified.is_none(), "a tampered token was accepted");
    }

    #[test]
    fn a_token_issued_to_another_address_is_rejected() {
        // The property the whole mechanism rests on. A token that verified regardless of
        // address would let an attacker replay one from a spoofed source, which is exactly
        // the amplification Retry exists to prevent.
        let scid = ConnectionId::new(&[1; 8]).expect("valid");
        let odcid = ConnectionId::new(&[2; 8]).expect("valid");
        let token =
            issue_retry_token(&secret(), crate::VERSION_V1, addr(), &scid, &odcid, now())
                .expect("minting");

        let elsewhere: SocketAddr = "127.0.0.2:4433".parse().expect("valid");
        let verified = verify_retry_token(
            &secret(),
            &token,
            crate::VERSION_V1,
            elsewhere,
            &scid,
            lifetime(),
            now(),
        )
        .expect("verifying");
        assert!(
            verified.is_none(),
            "a token minted for one address verified for another"
        );
    }

    #[test]
    fn an_expired_token_is_rejected() {
        let scid = ConnectionId::new(&[1; 8]).expect("valid");
        let odcid = ConnectionId::new(&[2; 8]).expect("valid");
        let token =
            issue_retry_token(&secret(), crate::VERSION_V1, addr(), &scid, &odcid, now())
                .expect("minting");

        let much_later =
            Timestamp::from_nanos(now().as_nanos() + 60_000_000_000).expect("valid");
        let verified = verify_retry_token(
            &secret(),
            &token,
            crate::VERSION_V1,
            addr(),
            &scid,
            lifetime(),
            much_later,
        )
        .expect("verifying");
        assert!(verified.is_none(), "an expired token was accepted");
    }

    #[test]
    fn a_token_from_another_secret_is_rejected() {
        let scid = ConnectionId::new(&[1; 8]).expect("valid");
        let odcid = ConnectionId::new(&[2; 8]).expect("valid");
        let token =
            issue_retry_token(&secret(), crate::VERSION_V1, addr(), &scid, &odcid, now())
                .expect("minting");

        let other = TokenSecret::new(&[0xa5; 32]).expect("valid");
        let verified = verify_retry_token(
            &other,
            &token,
            crate::VERSION_V1,
            addr(),
            &scid,
            lifetime(),
            now(),
        )
        .expect("verifying");
        assert!(
            verified.is_none(),
            "a token forged under a different secret was accepted"
        );
    }

    #[test]
    fn a_retry_packet_can_be_written() {
        let dcid = ConnectionId::new(&[3; 8]).expect("valid");
        let scid = ConnectionId::new(&[1; 8]).expect("valid");
        let odcid = ConnectionId::new(&[2; 8]).expect("valid");
        let token =
            issue_retry_token(&secret(), crate::VERSION_V1, addr(), &scid, &odcid, now())
                .expect("minting");

        let mut buf = [0u8; 512];
        let written =
            write_retry(&mut buf, crate::VERSION_V1, &dcid, &scid, &odcid, &token).expect("writing");
        assert!(written > 0);
        // Long header, and the version field is the one asked for.
        assert_eq!(buf[0] & 0b1000_0000, 0b1000_0000);
        assert_eq!(&buf[1..5], &crate::VERSION_V1.to_be_bytes());
    }

    #[test]
    fn a_reset_token_is_stable_for_an_identifier_and_differs_between_them() {
        // Stability is what lets a server with no state produce the right token; difference
        // is what stops one connection's token resetting another.
        let a = ConnectionId::new(&[7; 8]).expect("valid");
        let b = ConnectionId::new(&[8; 8]).expect("valid");
        let first = reset_token(&secret(), &a).expect("deriving");
        let again = reset_token(&secret(), &a).expect("deriving");
        let other = reset_token(&secret(), &b).expect("deriving");

        assert_eq!(first, again);
        assert_ne!(first, other);
    }

    #[test]
    fn a_stateless_reset_is_smaller_than_what_provoked_it() {
        // The amplification property. A reset larger than its trigger turns "I lost your
        // connection" into a reflector.
        let cid = ConnectionId::new(&[9; 8]).expect("valid");
        let token = reset_token(&secret(), &cid).expect("deriving");
        let random = [0x33u8; 256];
        let mut buf = [0u8; 1500];

        for provoking in [64usize, 200, 512, 1200] {
            let written = write_stateless_reset_smaller_than(&mut buf, &token, &random, provoking)
                .expect("writing")
                .unwrap_or_else(|| panic!("no reset produced for a {provoking}-byte trigger"));
            assert!(
                written < provoking,
                "a {written}-byte reset answered a {provoking}-byte datagram"
            );
        }
    }

    #[test]
    fn a_datagram_too_small_to_answer_safely_draws_nothing() {
        // Declining is correct: any reset for a trigger this small would be larger than it.
        let cid = ConnectionId::new(&[9; 8]).expect("valid");
        let token = reset_token(&secret(), &cid).expect("deriving");
        let random = [0x33u8; 64];
        let mut buf = [0u8; 512];

        for tiny in [0usize, 8, 16, RESET_TOKEN_LEN + MIN_RESET_RANDOM] {
            let written = write_stateless_reset_smaller_than(&mut buf, &token, &random, tiny)
                .expect("writing");
            assert!(
                written.is_none(),
                "a {tiny}-byte datagram drew a reset, which would amplify"
            );
        }
    }
}
