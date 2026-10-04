//! An asynchronous QUIC endpoint over the sans-I/O core.
//!
//! Enabled by the default `endpoint` feature. Disabling it returns the crate to a pure state
//! machine with exactly one dependency and no asynchrony of any kind.
//!
//! Everything in this subtree is confined to it: nothing outside `src/endpoint/` acquires an
//! async facility, a socket or a clock, and a structural test enforces that. It contains no
//! `unsafe` of its own — every foreign call and every `unsafe` block lives below it, in the
//! core, which is unchanged by this layer's existence and remains usable on its own.
//!
//! # What this layer is for
//!
//! The core is correct and complete, and it is also unusable without writing a UDP
//! integration by hand. A caller must bind a socket, decide which connection each arriving
//! datagram belongs to, drain outgoing datagrams until there are none, arm a timer for
//! whichever connection wants attention soonest, and take connections out of service once
//! they finish. Each of those has a way to be subtly wrong that shows up as a connection
//! which stalls rather than as an error, and this layer exists so that loop is written once.
//!
//! # What it does not supply
//!
//! A runtime. This layer names no executor, spawns nothing, and reads no clock of its own.
//! A caller describes their runtime's UDP socket and sleep to it, and a ready-made
//! description for one widely used runtime ships behind an optional feature.
//!
//! # Why one driver rather than one per connection
//!
//! `ngnet-h3`'s equivalent layer drives exactly one connection, because a caller hands it a
//! connection that is already established. Here the unit of ownership is the *socket*: every
//! connection on it shares one receive path, so one driver owns them all and the handles a
//! caller holds speak to it rather than to the connections directly.

mod clock;
mod config;
mod connection;
mod driver;
mod handle;
mod error;
mod shared;
mod socket;
#[cfg(feature = "tls-ossl")]
mod validate;

#[cfg(feature = "tokio")]
mod tokio;

#[doc(hidden)]
pub mod testing;

pub use clock::Clock;
pub use config::{Config, DEFAULT_DATAGRAMS_PER_PASS};
pub use connection::{AcceptStream, Chunk, Connection, OpenStream, ReadStream};
pub use shared::Observed;
pub use handle::Sleep;
pub use handle::{
    Accepting, Built, Connecting, DetachedConnection, Detaching, Endpoint, EndpointBuilder,
    EndpointDriver,
};
pub use error::{Error, ErrorKind, Result};
pub use socket::{AsyncUdpSocket, Received, Sent};

#[cfg(feature = "tls-ossl")]
pub use validate::{DEFAULT_RESET_BURST, DEFAULT_TOKEN_LIFETIME};

#[cfg(feature = "tokio")]
pub use tokio::{TokioClock, TokioSocket};

