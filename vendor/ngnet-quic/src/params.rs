//! Transport parameters — the negotiated half of the configuration.
//!
//! These are the limits each endpoint announces to the other during the handshake: how much
//! data it will accept, how many streams it will allow, how long it will wait before
//! declaring the connection idle. They travel in a TLS extension, which the crypto helper
//! encodes and decodes.
//!
//! # The defaults are not usable
//!
//! `ngtcp2_transport_params_default` leaves **every** `initial_max_*` field and
//! `max_idle_timeout` at zero. Taken literally that describes an endpoint that will accept
//! no data, permit no streams, and never time out — a connection that completes its
//! handshake and then does nothing at all, with no error to explain why.
//!
//! So [`TransportParams::new`] applies its own defaults on top, chosen to be small but
//! functional, and documents them. A caller who wants ngtcp2's literal defaults is asking
//! for something that does not work.

// `build` is consumed by the connection constructors.
#![allow(dead_code)]

use ngnet_quic_sys as sys;

use crate::cid::ConnectionId;
use crate::error::Result;
use crate::time::Duration;
use crate::validate;

/// Flow-control credit granted per stream, in bytes, unless overridden.
///
/// 256 KiB: large enough that a single request or response is not stalled by the window,
/// small enough that a peer opening many streams cannot commit this endpoint to unbounded
/// memory.
pub const DEFAULT_STREAM_DATA: u64 = 256 * 1024;

/// Flow-control credit granted for the connection as a whole, in bytes, unless overridden.
pub const DEFAULT_CONNECTION_DATA: u64 = 1024 * 1024;

/// Concurrent streams permitted in each direction, unless overridden.
pub const DEFAULT_MAX_STREAMS: u64 = 100;

/// How long a connection may be idle before it is closed, unless overridden.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_nanos(30_000_000_000);

/// Builder for the transport parameters announced to the peer.
pub struct TransportParams {
    raw: sys::ngtcp2_transport_params,
}

impl TransportParams {
    /// Starts from a working configuration.
    ///
    /// Applies ngtcp2's defaults, then fills the fields its initialiser leaves at zero with
    /// [`DEFAULT_STREAM_DATA`], [`DEFAULT_CONNECTION_DATA`], [`DEFAULT_MAX_STREAMS`] and
    /// [`DEFAULT_IDLE_TIMEOUT`]. Without that second step the parameters describe a
    /// connection that cannot carry anything.
    pub fn new() -> Self {
        // SAFETY: a zeroed struct is a valid input to the initialiser, which fills it.
        let mut raw = unsafe { core::mem::zeroed::<sys::ngtcp2_transport_params>() };
        // SAFETY: `raw` is a valid, writable, correctly-sized struct.
        unsafe { crate::ffi::transport_params_default(&mut raw) };

        raw.initial_max_stream_data_bidi_local = DEFAULT_STREAM_DATA;
        raw.initial_max_stream_data_bidi_remote = DEFAULT_STREAM_DATA;
        raw.initial_max_stream_data_uni = DEFAULT_STREAM_DATA;
        raw.initial_max_data = DEFAULT_CONNECTION_DATA;
        raw.initial_max_streams_bidi = DEFAULT_MAX_STREAMS;
        raw.initial_max_streams_uni = DEFAULT_MAX_STREAMS;
        raw.max_idle_timeout = DEFAULT_IDLE_TIMEOUT.as_raw();

        Self { raw }
    }

    /// Sets the per-stream credit for streams this endpoint opened.
    pub fn initial_max_stream_data_bidi_local(mut self, bytes: u64) -> Self {
        self.raw.initial_max_stream_data_bidi_local = bytes;
        self
    }

    /// Sets the per-stream credit for bidirectional streams the peer opened.
    pub fn initial_max_stream_data_bidi_remote(mut self, bytes: u64) -> Self {
        self.raw.initial_max_stream_data_bidi_remote = bytes;
        self
    }

    /// Sets the per-stream credit for unidirectional streams.
    pub fn initial_max_stream_data_uni(mut self, bytes: u64) -> Self {
        self.raw.initial_max_stream_data_uni = bytes;
        self
    }

    /// Sets the connection-wide credit.
    pub fn initial_max_data(mut self, bytes: u64) -> Self {
        self.raw.initial_max_data = bytes;
        self
    }

    /// Sets how many bidirectional streams the peer may open.
    pub fn initial_max_streams_bidi(mut self, count: u64) -> Self {
        self.raw.initial_max_streams_bidi = count;
        self
    }

    /// Sets how many unidirectional streams the peer may open.
    pub fn initial_max_streams_uni(mut self, count: u64) -> Self {
        self.raw.initial_max_streams_uni = count;
        self
    }

    /// Advertises the maximum QUIC DATAGRAM frame size this endpoint will receive.
    ///
    /// A value of zero disables DATAGRAM support. Values above the QUIC varint limit are
    /// rejected when the parameters are built.
    pub fn max_datagram_frame_size(mut self, bytes: u64) -> Self {
        self.raw.max_datagram_frame_size = bytes;
        self
    }

    /// Sets the idle timeout. Zero disables it.
    pub fn max_idle_timeout(mut self, timeout: Duration) -> Self {
        self.raw.max_idle_timeout = timeout.as_raw();
        self
    }

    /// Sets how many connection IDs the peer may keep active.
    ///
    /// Must be at least 2 and at most 8; ngtcp2 cannot track more.
    pub fn active_connection_id_limit(mut self, limit: u64) -> Self {
        self.raw.active_connection_id_limit = limit;
        self
    }

    /// Sets the longest a packet may be delayed before acknowledgement.
    pub fn max_ack_delay(mut self, delay: Duration) -> Self {
        self.raw.max_ack_delay = delay.as_raw();
        self
    }

    /// Records the destination connection ID from the client's first Initial packet.
    ///
    /// **Servers only, and required for them.** A server cannot be constructed without it:
    /// ngtcp2 asserts its presence (`ngtcp2_conn.c:1264-1265`), and since that assertion is
    /// compiled out of release builds, omitting it there is undefined behaviour rather than
    /// a crash. The value comes from decoding the client's first packet.
    /// The source identifier this server used in a Retry packet.
    ///
    /// Servers only, and **required** after a Retry: the client checks that the identifier
    /// it was told to address really did come from a Retry this server sent, and rejects
    /// the handshake if the parameter is missing. ngtcp2 states the obligation directly --
    /// "server application must set Destination Connection ID field from the Initial packet
    /// to this field" once it has verified a token (`ngtcp2.h:832`).
    ///
    /// Omitting it produces a handshake that simply never completes, which looks like an
    /// unreachable server rather than a protocol error.
    pub fn retry_scid(mut self, scid: &ConnectionId) -> Self {
        self.raw.retry_scid = *scid.as_raw();
        self.raw.retry_scid_present = 1;
        self
    }

    /// The identifier the client first addressed, before any Retry.
    ///
    /// Servers only, and required for them.
    pub fn original_dcid(mut self, dcid: &ConnectionId) -> Self {
        self.raw.original_dcid = *dcid.as_raw();
        self.raw.original_dcid_present = 1;
        self
    }

    /// Validates for the given role and yields the raw struct.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] naming the offending field. The role-dependent
    /// checks are the ones most worth having: several fields are required for a server and
    /// forbidden for a client.
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    pub(crate) fn build(self, server: bool) -> Result<sys::ngtcp2_transport_params> {
        validate::transport_params_common(
            self.raw.active_connection_id_limit,
            self.raw.initial_max_data,
            self.raw.initial_max_stream_data_bidi_local,
            self.raw.initial_max_stream_data_bidi_remote,
            self.raw.initial_max_stream_data_uni,
            self.raw.max_idle_timeout,
            self.raw.max_ack_delay,
        )?;
        validate::varint(
            self.raw.max_datagram_frame_size,
            "params.max_datagram_frame_size exceeds the maximum QUIC varint",
        )?;
        validate::transport_params_role(
            server,
            self.raw.original_dcid_present != 0,
            self.raw.initial_scid_present != 0,
            self.raw.stateless_reset_token_present != 0,
            self.raw.preferred_addr_present != 0,
            self.raw.retry_scid_present != 0,
        )?;
        Ok(self.raw)
    }

    /// The raw struct without validating, for tests.
    #[cfg(test)]
    pub(crate) fn as_raw(&self) -> &sys::ngtcp2_transport_params {
        &self.raw
    }
}

impl Default for TransportParams {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for TransportParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TransportParams")
            .field("initial_max_data", &{ self.raw.initial_max_data })
            .field("initial_max_streams_bidi", &{
                self.raw.initial_max_streams_bidi
            })
            .field("max_idle_timeout", &{ self.raw.max_idle_timeout })
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn the_defaults_describe_a_connection_that_can_actually_carry_data() {
        // The whole reason `new` does more than call the library initialiser.
        let params = TransportParams::new();
        let raw = params.as_raw();
        assert_ne!(raw.initial_max_data, 0);
        assert_ne!(raw.initial_max_stream_data_bidi_local, 0);
        assert_ne!(raw.initial_max_stream_data_bidi_remote, 0);
        assert_ne!(raw.initial_max_stream_data_uni, 0);
        assert_ne!(raw.initial_max_streams_bidi, 0);
        assert_ne!(raw.max_idle_timeout, 0);
    }

    #[test]
    fn the_library_defaults_survive_where_they_are_useful() {
        let params = TransportParams::new();
        assert_eq!(
            params.as_raw().active_connection_id_limit,
            u64::from(sys::NGTCP2_DEFAULT_ACTIVE_CONNECTION_ID_LIMIT)
        );
    }

    #[test]
    fn a_client_configuration_builds() {
        assert!(TransportParams::new().build(false).is_ok());
    }

    #[test]
    fn a_server_without_an_original_dcid_is_rejected() {
        // The check that catches "you did not decode the client's Initial packet first".
        let Err(err) = TransportParams::new().build(true) else {
            panic!("a server without an original_dcid must be rejected");
        };
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn a_server_with_an_original_dcid_builds() {
        let dcid = ConnectionId::new(&[9; 8]).unwrap();
        assert!(
            TransportParams::new()
                .original_dcid(&dcid)
                .build(true)
                .is_ok()
        );
    }

    #[test]
    fn a_client_with_an_original_dcid_is_rejected() {
        let dcid = ConnectionId::new(&[9; 8]).unwrap();
        assert!(
            TransportParams::new()
                .original_dcid(&dcid)
                .build(false)
                .is_err()
        );
    }

    #[test]
    fn the_original_dcid_bytes_are_carried_through() {
        let dcid = ConnectionId::new(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let params = TransportParams::new().original_dcid(&dcid);
        let raw = params.as_raw();
        assert_eq!(raw.original_dcid_present, 1);
        assert_eq!(&raw.original_dcid.data[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn too_few_active_connection_ids_is_rejected() {
        let Err(err) = TransportParams::new()
            .active_connection_id_limit(1)
            .build(false)
        else {
            panic!("an active_connection_id_limit below 2 must be rejected");
        };
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn builders_round_trip_into_the_raw_struct() {
        let params = TransportParams::new()
            .initial_max_data(1234)
            .initial_max_streams_bidi(7)
            .max_idle_timeout(Duration::from_nanos(99));
        let raw = params.as_raw();
        assert_eq!(raw.initial_max_data, 1234);
        assert_eq!(raw.initial_max_streams_bidi, 7);
        assert_eq!(raw.max_idle_timeout, 99);
    }
}
