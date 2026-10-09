//! What can go wrong once there is a socket involved.
//!
//! The sans-I/O core's [`Error`](crate::Error) describes what ngtcp2 refused to do. This
//! layer additionally has a socket that can fail, a handshake that can be rejected, a peer
//! that can go quiet and a caller that can drop things — so a caller here needs to tell
//! apart failures the core never had to distinguish.
//!
//! # Why the kinds are what they are
//!
//! Each variant exists because a caller would plausibly *do* something different about it,
//! not because the failures arise in different places. "The server refused my certificate"
//! and "nothing was listening" are both a failed connect, and a caller retries one and not
//! the other. That is the test applied to every variant below.

// The constructors are used by the driver and the handles, which are assembled after this
// module; the type has to exist before the things that produce it.
#![allow(dead_code)]

use core::fmt;

use crate::error::CloseError;

/// The result of an endpoint operation.
pub type Result<T> = core::result::Result<T, Error>;

/// What went wrong.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The TLS handshake was rejected — a certificate that did not verify, a name that did
    /// not match, no application protocol in common.
    ///
    /// Retrying without changing something will fail the same way.
    HandshakeRejected,

    /// The handshake did not complete within the configured timeout.
    ///
    /// Distinct from a rejection because nothing refused anything: this is what an
    /// unreachable address, a dropped path or a silent middlebox looks like, and retrying
    /// is reasonable.
    HandshakeTimeout,

    /// The peer sent nothing for the idle timeout, so the connection lapsed.
    ///
    /// Distinct from a close because no frame was exchanged and the peer may not know the
    /// connection is over.
    IdleTimeout,

    /// The peer closed the connection.
    ///
    /// The code and reason it sent are on the error itself; see [`Error::close_error`].
    PeerClosed,

    /// This endpoint closed the connection.
    LocallyClosed,

    /// The transport failed: a protocol violation, an unusable connection, an
    /// unrecoverable state inside ngtcp2.
    Transport,

    /// The socket failed.
    ///
    /// Fatal to every connection on that socket, not just this one, because there is no
    /// longer a way to send or receive for any of them.
    Socket,

    /// The driver is gone, so nothing will make progress.
    ///
    /// What a caller sees when it holds a handle whose driver was dropped, or whose driver
    /// returned. Never a transport condition — always a mistake in how the endpoint is
    /// being run, which is why it is not folded into [`ErrorKind::LocallyClosed`].
    DriverGone,

    /// The operation was abandoned because its handle was dropped.
    Cancelled,

    /// The caller asked for something this build cannot do.
    ///
    /// Address validation without the bundled TLS backend is the case that exists today:
    /// writing a Retry packet needs packet protection the backend supplies, so the request
    /// fails loudly rather than producing a server that validates nothing.
    Unsupported,

    /// The caller's configuration is inconsistent.
    InvalidInput,

    /// The peer reset the stream being read.
    ///
    /// Distinct from an ordinary end-of-stream: bytes already delivered are still valid,
    /// but no more are coming and the peer chose a code to say why.
    StreamReset,

    /// The peer asked this endpoint to stop sending on the stream being written.
    ///
    /// Writing more would spend the connection's flow-control window on bytes nothing will
    /// read, so a write is refused rather than silently accepted.
    StreamStopped,
}

impl ErrorKind {
    /// Whether retrying the same operation could plausibly succeed.
    ///
    /// Advisory. It says the failure was not a refusal, not that a retry will work.
    pub fn is_retriable(self) -> bool {
        matches!(
            self,
            Self::HandshakeTimeout | Self::IdleTimeout | Self::PeerClosed
        )
    }
}

/// An endpoint failure.
pub struct Error {
    kind: ErrorKind,
    context: &'static str,
    close: Option<CloseError>,
    stream_code: Option<crate::error::ApplicationErrorCode>,
    source: Option<Box<dyn core::error::Error + Send + Sync>>,
}

impl Error {
    /// Builds an error of `kind` with a static description.
    pub(crate) fn new(kind: ErrorKind, context: &'static str) -> Self {
        Self {
            kind,
            context,
            close: None,
            stream_code: None,
            source: None,
        }
    }

    /// Attaches the reason a connection closed.
    pub(crate) fn with_close(mut self, close: CloseError) -> Self {
        self.close = Some(close);
        self
    }

    /// Attaches an underlying cause.
    pub(crate) fn with_source(
        mut self,
        source: impl core::error::Error + Send + Sync + 'static,
    ) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Attaches an already-boxed cause, for sockets whose error type is not `'static`
    /// enough to box directly.
    pub(crate) fn with_boxed_source(
        mut self,
        source: Box<dyn core::error::Error + Send + Sync>,
    ) -> Self {
        self.source = Some(source);
        self
    }

    /// Attaches the application error code a stream carried.
    pub(crate) fn with_stream_code(mut self, code: crate::error::ApplicationErrorCode) -> Self {
        self.stream_code = Some(code);
        self
    }

    /// The application error code the peer sent, for a stream reset or stop-sending.
    ///
    /// This is the number the protocol above QUIC chose, so it means whatever that protocol
    /// says it means.
    pub fn stream_code(&self) -> Option<crate::error::ApplicationErrorCode> {
        self.stream_code
    }

    /// What kind of failure this is.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Why the connection closed, when the failure was a close.
    ///
    /// Carries the peer's application error code and reason phrase for
    /// [`ErrorKind::PeerClosed`], which is the only place an application-level explanation
    /// is available.
    pub fn close_error(&self) -> Option<&CloseError> {
        self.close.as_ref()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.context)?;
        if let Some(close) = &self.close {
            write!(f, ": {close}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("context", &self.context)
            .field("close", &self.close)
            .field("stream_code", &self.stream_code)
            .field("source", &self.source.as_ref().map(|s| s.to_string()))
            .finish()
    }
}

impl core::error::Error for Error {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &(dyn core::error::Error + 'static))
    }
}

impl From<crate::error::Error> for Error {
    /// Maps a core failure onto an endpoint one.
    ///
    /// Everything the core reports is a transport condition by definition — it has no
    /// socket, no handshake timeout and no notion of a dropped handle — except a
    /// configuration mistake, which stays a configuration mistake.
    fn from(err: crate::error::Error) -> Self {
        let kind = match err.kind() {
            crate::ErrorKind::InvalidInput => ErrorKind::InvalidInput,
            crate::ErrorKind::Closing => ErrorKind::LocallyClosed,
            _ => ErrorKind::Transport,
        };
        Self::new(kind, "the transport failed").with_source(err)
    }
}
