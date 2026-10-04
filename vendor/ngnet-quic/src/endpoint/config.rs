//! What an endpoint advertises, and how much of it runs at once.
//!
//! Every field here maps onto something the sans-I/O core already takes — a
//! [`Settings`](crate::Settings) field or a [`TransportParams`](crate::TransportParams)
//! one — except the work bound, which is about the driver rather than about QUIC. The
//! mapping is written down for each, because a configuration field whose effect nobody can
//! trace is decoration.
//!
//! The two builders at the foot of this file are what the endpoint driver constructs each
//! connection from; they are `pub(crate)` because a caller who wants the core's own
//! configuration types should use those directly rather than reach through this one.

// The builders are consumed by the driver, which is assembled after this module.
#![allow(dead_code)]

use crate::params::{
    DEFAULT_CONNECTION_DATA, DEFAULT_IDLE_TIMEOUT, DEFAULT_MAX_STREAMS, DEFAULT_STREAM_DATA,
};
use crate::time::Duration;

/// How many datagrams one scheduling pass may take from the socket.
///
/// The default matches `ngnet-h3`'s `events_per_pass`, for the same reason: a driver that
/// drained the socket until it was empty would let one busy endpoint monopolise the
/// runtime, and would never get to its timers on a socket that is never empty.
pub const DEFAULT_DATAGRAMS_PER_PASS: usize = 64;

/// Settings for an endpoint and the connections it carries.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub(crate) datagrams_per_pass: usize,
    pub(crate) initial_max_stream_data: u64,
    pub(crate) initial_max_data: u64,
    pub(crate) max_streams_bidi: u64,
    pub(crate) max_streams_uni: u64,
    pub(crate) max_idle_timeout: Duration,
    pub(crate) initial_rtt: Option<Duration>,
    pub(crate) handshake_timeout: Option<Duration>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            datagrams_per_pass: DEFAULT_DATAGRAMS_PER_PASS,
            initial_max_stream_data: DEFAULT_STREAM_DATA,
            initial_max_data: DEFAULT_CONNECTION_DATA,
            max_streams_bidi: DEFAULT_MAX_STREAMS,
            max_streams_uni: DEFAULT_MAX_STREAMS,
            max_idle_timeout: DEFAULT_IDLE_TIMEOUT,
            initial_rtt: None,
            handshake_timeout: None,
        }
    }
}

impl Config {
    /// The defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many datagrams one scheduling pass may take from the socket.
    ///
    /// Lower means fairer and more syscalls; higher means fewer wakeups and a longer time
    /// before the timers are looked at. Zero is treated as one, because a bound of zero
    /// would be an endpoint that never reads.
    ///
    /// Purely a driver concern: nothing is advertised to the peer.
    #[must_use]
    pub fn datagrams_per_pass(mut self, datagrams: usize) -> Self {
        self.datagrams_per_pass = datagrams.max(1);
        self
    }

    /// How many bytes the peer may send on each stream before waiting for credit.
    ///
    /// Maps onto all three of the core's per-stream limits at once —
    /// `initial_max_stream_data_bidi_local`, `_bidi_remote` and `_uni` — because
    /// distinguishing them is a tuning decision this layer has no opinion about, and a
    /// caller who wants them separate can build the transport parameters directly.
    #[must_use]
    pub fn initial_max_stream_data(mut self, bytes: u64) -> Self {
        self.initial_max_stream_data = bytes;
        self
    }

    /// How many bytes the peer may send across all streams before waiting for credit.
    ///
    /// Maps onto `initial_max_data`. Setting this below
    /// [`Config::initial_max_stream_data`] means a single stream can exhaust the whole
    /// connection window, which is a legitimate thing to want and an easy thing to do by
    /// accident.
    #[must_use]
    pub fn initial_max_data(mut self, bytes: u64) -> Self {
        self.initial_max_data = bytes;
        self
    }

    /// How many bidirectional streams the peer may open.
    ///
    /// Maps onto `initial_max_streams_bidi`.
    #[must_use]
    pub fn max_streams_bidi(mut self, count: u64) -> Self {
        self.max_streams_bidi = count;
        self
    }

    /// How many unidirectional streams the peer may open.
    ///
    /// Maps onto `initial_max_streams_uni`.
    #[must_use]
    pub fn max_streams_uni(mut self, count: u64) -> Self {
        self.max_streams_uni = count;
        self
    }

    /// How long a connection may sit idle before it lapses.
    ///
    /// Maps onto `max_idle_timeout`, which is negotiated: the effective timeout is the
    /// smaller of the two endpoints' values, so setting this high does not extend a peer
    /// that set it low.
    #[must_use]
    pub fn max_idle_timeout(mut self, timeout: Duration) -> Self {
        self.max_idle_timeout = timeout;
        self
    }

    /// The round-trip estimate to use before one has been measured.
    ///
    /// Maps onto `Settings::initial_rtt`. It sets the first retransmission timeout, so a
    /// value far from the truth costs a visible delay on the very first loss — too low and
    /// the first flight is retransmitted needlessly, too high and a genuine loss takes that
    /// long to notice.
    #[must_use]
    pub fn initial_rtt(mut self, rtt: Duration) -> Self {
        self.initial_rtt = Some(rtt);
        self
    }

    /// How long a handshake may take before it is abandoned.
    ///
    /// Maps onto `Settings::handshake_timeout`, and is what turns a connect to an address
    /// where nothing is listening into a failure rather than an indefinite wait.
    #[must_use]
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = Some(timeout);
        self
    }

    /// Builds the core transport parameters this configuration describes.
    pub(crate) fn transport_params(&self) -> crate::params::TransportParams {
        crate::params::TransportParams::new()
            .initial_max_stream_data_bidi_local(self.initial_max_stream_data)
            .initial_max_stream_data_bidi_remote(self.initial_max_stream_data)
            .initial_max_stream_data_uni(self.initial_max_stream_data)
            .initial_max_data(self.initial_max_data)
            .initial_max_streams_bidi(self.max_streams_bidi)
            .initial_max_streams_uni(self.max_streams_uni)
            .max_idle_timeout(self.max_idle_timeout)
    }

    /// Builds the core settings this configuration describes.
    pub(crate) fn settings(&self, now: crate::time::Timestamp) -> crate::settings::Settings {
        let mut settings = crate::settings::Settings::new(now);
        if let Some(rtt) = self.initial_rtt {
            settings = settings.initial_rtt(rtt);
        }
        if let Some(timeout) = self.handshake_timeout {
            settings = settings.handshake_timeout(timeout);
        }
        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_work_bound_is_raised_to_one() {
        // An endpoint that reads no datagrams per pass would never make progress, and
        // would do so silently. Clamping is more useful than an error nobody expects from
        // a setter.
        assert_eq!(Config::new().datagrams_per_pass(0).datagrams_per_pass, 1);
    }

    #[test]
    fn the_defaults_match_the_cores_own() {
        let config = Config::new();
        assert_eq!(config.initial_max_stream_data, DEFAULT_STREAM_DATA);
        assert_eq!(config.initial_max_data, DEFAULT_CONNECTION_DATA);
        assert_eq!(config.max_streams_bidi, DEFAULT_MAX_STREAMS);
        assert_eq!(config.max_idle_timeout, DEFAULT_IDLE_TIMEOUT);
    }

    #[test]
    fn every_field_reaches_the_core() {
        // The claim the mapping documentation makes. A field that did not reach the core
        // would be a setter with no effect, which is worse than an absent one.
        let config = Config::new()
            .initial_max_stream_data(1234)
            .initial_max_data(5678)
            .max_streams_bidi(7)
            .max_streams_uni(9)
            .max_idle_timeout(Duration::from_nanos(1_000_000));
        let params = config.transport_params().build(false).expect("valid");
        assert_eq!(params.initial_max_stream_data_bidi_local, 1234);
        assert_eq!(params.initial_max_stream_data_bidi_remote, 1234);
        assert_eq!(params.initial_max_stream_data_uni, 1234);
        assert_eq!(params.initial_max_data, 5678);
        assert_eq!(params.initial_max_streams_bidi, 7);
        assert_eq!(params.initial_max_streams_uni, 9);
        assert_eq!(params.max_idle_timeout, 1_000_000);
    }

    #[test]
    fn an_unset_timing_field_leaves_the_cores_default_alone() {
        // `initial_rtt` and `handshake_timeout` are `Option` precisely so that not setting
        // them is distinguishable from setting them to zero -- which for `initial_rtt` the
        // core rejects, because loss recovery divides by it.
        let now = crate::time::Timestamp::from_nanos(1).expect("valid");
        let default = Config::new().settings(now).build().expect("valid");
        assert!(default.0.initial_rtt != 0);

        let set = Config::new()
            .initial_rtt(Duration::from_nanos(5_000_000))
            .settings(now)
            .build()
            .expect("valid");
        assert_eq!(set.0.initial_rtt, 5_000_000);
    }
}
