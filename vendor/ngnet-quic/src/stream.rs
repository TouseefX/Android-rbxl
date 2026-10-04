//! Stream identifiers.
//!
//! QUIC encodes two things in the low bits of a stream ID: who opened it, and whether it is
//! bidirectional. Bit 0 is the initiator (0 = client, 1 = server) and bit 1 the
//! directionality (0 = bidirectional, 1 = unidirectional). The rest is a counter.
//!
//! Encoding that in the type means a caller can ask what a stream *is* without remembering
//! the bit layout, and cannot construct an identifier that is out of range.

use crate::error::{Error, Result};

/// Which endpoint opened a stream.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Initiator {
    /// The client opened it.
    Client,
    /// The server opened it.
    Server,
}

/// Whether a stream carries data in both directions.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Directionality {
    /// Both endpoints may send.
    Bidirectional,
    /// Only the initiator may send.
    Unidirectional,
}

/// A QUIC stream identifier.
///
/// Always non-negative: ngtcp2 uses `-1` internally to mean "no stream", and this type
/// exists partly so that sentinel can never be mistaken for a real identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId(i64);

impl StreamId {
    /// The largest identifier QUIC permits.
    ///
    /// Stream IDs are 62-bit variable-length integers.
    pub const MAX: i64 = (1 << 62) - 1;

    /// Wraps a raw identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] for a negative value or one above [`MAX`].
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    /// [`MAX`]: StreamId::MAX
    pub const fn new(id: i64) -> Result<Self> {
        if id < 0 {
            return Err(Error::invalid_input(
                "a stream identifier cannot be negative",
            ));
        }
        if id > Self::MAX {
            return Err(Error::invalid_input(
                "a stream identifier cannot exceed 2^62 - 1",
            ));
        }
        Ok(Self(id))
    }

    /// The raw identifier.
    pub const fn get(self) -> i64 {
        self.0
    }

    /// Which endpoint opened this stream.
    pub const fn initiator(self) -> Initiator {
        if self.0 & 0x1 == 0 {
            Initiator::Client
        } else {
            Initiator::Server
        }
    }

    /// Whether this stream is bidirectional.
    pub const fn directionality(self) -> Directionality {
        if self.0 & 0x2 == 0 {
            Directionality::Bidirectional
        } else {
            Directionality::Unidirectional
        }
    }

    /// Whether the given role may send on this stream.
    ///
    /// A unidirectional stream can only be written by whoever opened it, which is the rule
    /// most easily forgotten when a write silently fails.
    pub const fn is_writable_by(self, is_server: bool) -> bool {
        match self.directionality() {
            Directionality::Bidirectional => true,
            Directionality::Unidirectional => {
                matches!(
                    (self.initiator(), is_server),
                    (Initiator::Server, true) | (Initiator::Client, false)
                )
            }
        }
    }
}

impl core::fmt::Debug for StreamId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let initiator = match self.initiator() {
            Initiator::Client => "client",
            Initiator::Server => "server",
        };
        let direction = match self.directionality() {
            Directionality::Bidirectional => "bidi",
            Directionality::Unidirectional => "uni",
        };
        write!(f, "StreamId({}, {initiator} {direction})", self.0)
    }
}

impl core::fmt::Display for StreamId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sentinel_ngtcp2_uses_for_no_stream_is_not_a_valid_identifier() {
        // ngtcp2 returns -1 to mean "no stream". If that could be wrapped, a caller could
        // not tell it apart from a real identifier.
        assert!(StreamId::new(-1).is_err());
        assert!(StreamId::new(0).is_ok());
    }

    #[test]
    fn identifiers_above_the_varint_range_are_rejected() {
        assert!(StreamId::new(StreamId::MAX).is_ok());
        assert!(StreamId::new(StreamId::MAX + 1).is_err());
    }

    #[test]
    fn the_low_two_bits_decode_as_quic_specifies() {
        // 0x00 client bidi, 0x01 server bidi, 0x02 client uni, 0x03 server uni.
        let cases = [
            (0, Initiator::Client, Directionality::Bidirectional),
            (1, Initiator::Server, Directionality::Bidirectional),
            (2, Initiator::Client, Directionality::Unidirectional),
            (3, Initiator::Server, Directionality::Unidirectional),
        ];
        for (raw, initiator, directionality) in cases {
            let id = StreamId::new(raw).unwrap();
            assert_eq!(id.initiator(), initiator, "initiator of {raw}");
            assert_eq!(id.directionality(), directionality, "direction of {raw}");
        }
    }

    #[test]
    fn the_counter_above_the_low_bits_does_not_disturb_the_decoding() {
        let id = StreamId::new(0xff_00 | 0x3).unwrap();
        assert_eq!(id.initiator(), Initiator::Server);
        assert_eq!(id.directionality(), Directionality::Unidirectional);
    }

    #[test]
    fn only_the_opener_may_write_a_unidirectional_stream() {
        let client_uni = StreamId::new(2).unwrap();
        let server_uni = StreamId::new(3).unwrap();
        assert!(client_uni.is_writable_by(false));
        assert!(!client_uni.is_writable_by(true));
        assert!(server_uni.is_writable_by(true));
        assert!(!server_uni.is_writable_by(false));
    }

    #[test]
    fn both_roles_may_write_a_bidirectional_stream() {
        for raw in [0, 1] {
            let id = StreamId::new(raw).unwrap();
            assert!(id.is_writable_by(true));
            assert!(id.is_writable_by(false));
        }
    }

    #[test]
    fn the_debug_form_names_the_role_and_direction() {
        assert_eq!(
            format!("{:?}", StreamId::new(3).unwrap()),
            "StreamId(3, server uni)"
        );
    }
}
