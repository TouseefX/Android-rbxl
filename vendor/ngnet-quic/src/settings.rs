//! Connection settings — the local, non-negotiated half of the configuration.
//!
//! `ngtcp2_settings` holds what this endpoint does regardless of the peer: its clock base,
//! its congestion controller, its loss-recovery starting point. The negotiated half, which
//! the peer is told about, is in [`crate::params`].
//!
//! The struct is **copied** by the constructor (`ngtcp2_conn.c:1391`), so a [`Settings`] may
//! be dropped as soon as the connection is built — with two exceptions this type handles:
//! `rand_ctx.native_handle` is retained, and the token and probe arrays are deep-copied but
//! must be alive at the moment of the call.

// `build` is consumed by the connection constructors.
#![allow(dead_code)]

use ngnet_quic_sys as sys;

use crate::error::Result;
use crate::time::{Duration, Timestamp};
use crate::validate;

/// Builder for a connection's local settings.
///
/// There is deliberately **no `Default`**. `ngtcp2_settings_default` does not set
/// `initial_ts`, so a defaulted value describes a connection whose entire clock base is
/// zero — every timer would appear to have expired at once. Since this crate does not read
/// a clock, only the caller can supply that, so [`Settings::new`] requires it.
pub struct Settings {
    raw: sys::ngtcp2_settings,
    /// The address-validation token, kept alive until the constructor copies it.
    token: Option<Vec<u8>>,
}

/// Where an address-validation token came from.
///
/// A server must tell ngtcp2 which kind it validated, because the two are checked against
/// different things: a Retry token proves the client echoed back what this server sent it
/// moments ago, while a NEW_TOKEN token was issued on an earlier connection and only proves
/// the address was reachable then.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum TokenKind {
    /// The token was carried in a Retry packet this server sent.
    Retry,
    /// The token came from a NEW_TOKEN frame on an earlier connection.
    NewToken,
}

impl Settings {
    /// Starts from ngtcp2's defaults, with the clock base the library does not default.
    ///
    /// `initial_ts` should be the same reading you will pass to the first call on the
    /// connection.
    pub fn new(initial_ts: Timestamp) -> Self {
        // SAFETY: a zeroed `ngtcp2_settings` is a valid input to the initialiser, which
        // fills it completely.
        let mut raw = unsafe { core::mem::zeroed::<sys::ngtcp2_settings>() };
        // SAFETY: `raw` is a valid, writable, correctly-sized struct.
        unsafe { crate::ffi::settings_default(&mut raw) };
        raw.initial_ts = initial_ts.as_raw();
        Self { raw, token: None }
    }

    /// Sets the estimate of the round-trip time used before one has been measured.
    ///
    /// Must be non-zero: loss recovery divides by it.
    pub fn initial_rtt(mut self, rtt: Duration) -> Self {
        self.raw.initial_rtt = rtt.as_raw();
        self
    }

    /// Sets the largest UDP payload this endpoint will send.
    pub fn max_tx_udp_payload_size(mut self, size: usize) -> Self {
        self.raw.max_tx_udp_payload_size = size;
        self
    }

    /// Sets the connection-level flow-control auto-tuning ceiling.
    pub fn max_window(mut self, bytes: u64) -> Self {
        self.raw.max_window = bytes;
        self
    }

    /// Sets the stream-level flow-control auto-tuning ceiling.
    pub fn max_stream_window(mut self, bytes: u64) -> Self {
        self.raw.max_stream_window = bytes;
        self
    }

    /// Sets how long the handshake may take before it is abandoned.
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.raw.handshake_timeout = timeout.as_raw();
        self
    }

    /// Sets the packet number the connection starts from.
    ///
    /// Must not exceed `i32::MAX`.
    pub fn initial_pkt_num(mut self, num: u32) -> Self {
        self.raw.initial_pkt_num = num;
        self
    }

    /// Disables path MTU discovery.
    pub fn no_pmtud(mut self, disabled: bool) -> Self {
        self.raw.no_pmtud = u8::from(disabled);
        self
    }

    /// Records an address-validation token that has **already been verified**.
    ///
    /// This does not validate anything. Verifying a token needs the secret that minted it,
    /// which is the server's, so that decision is made before this is called; what this
    /// does is tell ngtcp2 the decision was made and which kind of token it was. Passing an
    /// unverified token here is how a server would believe an address it never checked.
    ///
    /// A client uses the same setter to present a token it was given earlier, with
    /// [`TokenKind::NewToken`].
    pub fn validated_token(mut self, token: &[u8], kind: TokenKind) -> Self {
        self.raw.token_type = match kind {
            TokenKind::Retry => sys::NGTCP2_TOKEN_TYPE_RETRY,
            TokenKind::NewToken => sys::NGTCP2_TOKEN_TYPE_NEW_TOKEN,
        };
        self.token = Some(token.to_vec());
        self
    }

    /// Validates the settings and yields the raw struct.
    ///
    /// The checks mirror the assertions ngtcp2 makes at the top of its constructors, which
    /// are compiled out of release builds. See [`crate::validate`].
    ///
    /// Returns the token storage alongside the struct because ngtcp2 copies the token only
    /// *during* construction and reads it through the pointer until then. The caller must
    /// therefore keep the returned buffer alive across the constructor call. Returning it
    /// by value is safe because a `Vec`'s heap allocation does not move when the `Vec` does,
    /// so the pointer written into the struct stays valid.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] naming the offending field.
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    pub(crate) fn build(mut self) -> Result<(sys::ngtcp2_settings, Option<Vec<u8>>)> {
        validate::settings(
            self.raw.max_window,
            self.raw.max_stream_window,
            self.raw.max_tx_udp_payload_size,
            u64::from(self.raw.initial_pkt_num),
            self.raw.initial_rtt,
        )?;
        match self.token.as_ref() {
            Some(token) if !token.is_empty() => {
                self.raw.token = token.as_ptr();
                self.raw.tokenlen = token.len();
            }
            _ => {
                self.raw.token = core::ptr::null();
                self.raw.tokenlen = 0;
            }
        }
        Ok((self.raw, self.token))
    }

    /// The raw struct without validating, for tests that want to inspect a default.
    #[cfg(test)]
    pub(crate) fn as_raw(&self) -> &sys::ngtcp2_settings {
        &self.raw
    }
}

impl core::fmt::Debug for Settings {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Settings")
            .field("initial_ts", &self.raw.initial_ts)
            .field("initial_rtt", &self.raw.initial_rtt)
            .field("max_tx_udp_payload_size", &self.raw.max_tx_udp_payload_size)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    fn ts() -> Timestamp {
        Timestamp::from_nanos(1_000_000).unwrap()
    }

    #[test]
    fn the_clock_base_is_carried_through() {
        // The field ngtcp2's own initialiser leaves at zero, which is why `new` takes it.
        let settings = Settings::new(ts());
        assert_eq!(settings.as_raw().initial_ts, 1_000_000);
    }

    #[test]
    fn the_library_defaults_are_the_starting_point() {
        let settings = Settings::new(ts());
        assert_ne!(
            settings.as_raw().initial_rtt,
            0,
            "the initialiser supplies a non-zero initial RTT"
        );
    }

    #[test]
    fn builders_round_trip_into_the_raw_struct() {
        let settings = Settings::new(ts())
            .max_window(4096)
            .max_stream_window(2048)
            .no_pmtud(true);
        let raw = settings.as_raw();
        assert_eq!(raw.max_window, 4096);
        assert_eq!(raw.max_stream_window, 2048);
        assert_eq!(raw.no_pmtud, 1);
    }

    #[test]
    fn a_valid_configuration_builds() {
        assert!(Settings::new(ts()).build().is_ok());
    }

    #[test]
    fn a_zero_initial_rtt_is_rejected_at_build_time() {
        let err = Settings::new(ts())
            .initial_rtt(Duration::from_nanos(0))
            .build()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn an_undersized_udp_payload_is_rejected_at_build_time() {
        let err = Settings::new(ts())
            .max_tx_udp_payload_size(16)
            .build()
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }
}
