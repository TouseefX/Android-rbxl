//! Connection identifiers.

// `from_raw` and `as_raw` are consumed by the connection constructors.
#![allow(dead_code)]

use ngnet_quic_sys as sys;

use crate::error::{Error, Result};
use crate::rand::EntropySource;

/// The longest connection ID QUIC version 1 allows.
pub const MAX_LEN: usize = sys::NGTCP2_MAX_CIDLEN as usize;

/// The shortest non-empty connection ID ngtcp2 will accept.
pub const MIN_LEN: usize = sys::NGTCP2_MIN_CIDLEN as usize;

/// The identifier length this crate issues unless told otherwise.
///
/// Eight bytes, which is what most QUIC implementations use. The value matters beyond
/// taste: a **short-header** packet does not carry the length of its destination
/// identifier, because an endpoint is expected to know how long its own are. Anything
/// routing datagrams must therefore decode short headers with exactly this length, and
/// using a different one reads the wrong bytes as the identifier — so the handshake, whose
/// long headers carry explicit lengths, succeeds and every packet after it fails to route.
/// That failure looks like a connection which establishes and then goes silent.
///
/// It is one constant so that the connection builder and anything demultiplexing for it
/// cannot disagree.
pub const DEFAULT_LEN: usize = 8;

/// A QUIC connection identifier.
///
/// Wraps `ngtcp2_cid`, which stores its bytes inline rather than by pointer — so a
/// `ConnectionId` owns its data and has no lifetime attached, and passing one to ngtcp2
/// copies it.
///
/// A connection ID is not a secret, but it must be *unpredictable*: an observer who can
/// guess the identifiers an endpoint will issue can correlate or interfere with its
/// connections. That is why [`ConnectionId::generate`] takes an [`EntropySource`] rather
/// than offering a convenience constructor that picks bytes on the caller's behalf.
#[derive(Clone, Copy)]
pub struct ConnectionId(sys::ngtcp2_cid);

impl ConnectionId {
    /// Builds a connection ID from bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] if the length is not between [`MIN_LEN`] and
    /// [`MAX_LEN`]. ngtcp2 checks this with an assertion in some paths, which is compiled
    /// out of release builds, so it is checked here.
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    pub fn new(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < MIN_LEN || bytes.len() > MAX_LEN {
            return Err(Error::invalid_input(
                "a connection ID must be between NGTCP2_MIN_CIDLEN and NGTCP2_MAX_CIDLEN bytes",
            ));
        }
        let mut cid = sys::ngtcp2_cid {
            datalen: bytes.len(),
            data: [0; MAX_LEN],
        };
        cid.data[..bytes.len()].copy_from_slice(bytes);
        Ok(Self(cid))
    }

    /// Generates a connection ID of `len` bytes from an entropy source.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] if `len` is out of range, or propagates a
    /// failure from the entropy source.
    ///
    /// [`ErrorKind::InvalidInput`]: crate::ErrorKind::InvalidInput
    pub fn generate<E: EntropySource + ?Sized>(source: &mut E, len: usize) -> Result<Self> {
        if !(MIN_LEN..=MAX_LEN).contains(&len) {
            return Err(Error::invalid_input(
                "a connection ID must be between NGTCP2_MIN_CIDLEN and NGTCP2_MAX_CIDLEN bytes",
            ));
        }
        let mut bytes = [0u8; MAX_LEN];
        source.fill(&mut bytes[..len])?;
        Self::new(&bytes[..len])
    }

    /// The identifier's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0.data[..self.0.datalen]
    }

    /// The raw `ngtcp2_cid`, for passing to the library.
    pub(crate) fn as_raw(&self) -> &sys::ngtcp2_cid {
        &self.0
    }

    /// Copies a connection ID out of one ngtcp2 produced.
    pub(crate) fn from_raw(raw: &sys::ngtcp2_cid) -> Self {
        Self(*raw)
    }
}

impl PartialEq for ConnectionId {
    fn eq(&self, other: &Self) -> bool {
        // Compares the significant bytes only. `ngtcp2_cid` carries a fixed 20-byte array
        // whose tail is unspecified beyond `datalen`, so comparing the whole struct would
        // report two equal identifiers as different.
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for ConnectionId {}

impl core::hash::Hash for ConnectionId {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

impl core::fmt::Debug for ConnectionId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ConnectionId(")?;
        for byte in self.as_bytes() {
            write!(f, "{byte:02x}")?;
        }
        write!(f, ")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rand::test_support::CountingEntropy;

    #[test]
    fn a_connection_id_round_trips_its_bytes() {
        let cid = ConnectionId::new(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(cid.as_bytes(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn lengths_outside_the_permitted_range_are_rejected() {
        assert!(ConnectionId::new(&[0; MAX_LEN + 1]).is_err());
        assert!(ConnectionId::new(&[]).is_err());
        assert!(ConnectionId::new(&[0; MAX_LEN]).is_ok());
        assert!(ConnectionId::new(&[0; MIN_LEN]).is_ok());
    }

    #[test]
    fn generation_asks_the_entropy_source_for_exactly_the_length_requested() {
        let mut source = CountingEntropy::default();
        let cid = ConnectionId::generate(&mut source, 16).unwrap();
        assert_eq!(cid.as_bytes().len(), 16);
        assert_eq!(source.bytes_produced(), 16);
    }

    #[test]
    fn generation_rejects_an_out_of_range_length_without_consuming_entropy() {
        let mut source = CountingEntropy::default();
        assert!(ConnectionId::generate(&mut source, MAX_LEN + 1).is_err());
        assert_eq!(source.bytes_produced(), 0);
    }

    #[test]
    fn the_debug_form_is_hex() {
        let cid = ConnectionId::new(&[0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0]).unwrap();
        assert_eq!(format!("{cid:?}"), "ConnectionId(deadbeef00000000)");
    }
}
