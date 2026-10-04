//! Error types.
//!
//! ngtcp2 reports failures as small negative integers, and this module keeps three things
//! separate that are easy to conflate.
//!
//! First, the **native code**: the exact value ngtcp2 returned. It is preserved rather than
//! discarded, because no fixed set of variants can cover every condition a C library
//! defines, and [`NativeCode::is_fatal`] and [`NativeCode::describe`] are the library's own
//! predicates rather than a table maintained here.
//!
//! Second, the **kind**: a coarse classification a caller can branch on without matching
//! two dozen codes.
//!
//! Third — and unlike HTTP/3, which has one — QUIC has **two error-code spaces**.
//! Transport error codes are defined by the QUIC transport specification and describe the
//! connection itself; application error codes are opaque to QUIC and mean whatever the
//! protocol running over it says they mean. The same integer means different things in
//! each, so they are different types here. Collapsing them into one would let an
//! application code be passed where a transport code is required, with nothing to catch it.

// The constructors that build an error from a native code, and the classifier behind them,
// are exercised by the tests below but have no non-test caller until a connection exists to
// return them. They are written here because this is where the classification belongs.
#![allow(dead_code)]

use core::fmt;

use ngnet_quic_sys as sys;

/// The result of an operation that can fail.
pub type Result<T> = core::result::Result<T, Error>;

/// A raw ngtcp2 error code.
///
/// Preserved alongside the [`ErrorKind`] so that a caller needing the exact condition can
/// have it, without this crate having to grow a variant for every code ngtcp2 defines.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeCode(i32);

impl NativeCode {
    /// Wraps a raw code as returned by ngtcp2.
    pub const fn new(code: i32) -> Self {
        Self(code)
    }

    /// The raw value.
    pub const fn get(self) -> i32 {
        self.0
    }

    /// Whether ngtcp2 considers this code fatal to the connection.
    ///
    /// This is the library's own predicate (`ngtcp2_err_is_fatal`), not a local guess.
    pub fn is_fatal(self) -> bool {
        // SAFETY: a pure predicate over an integer, with no preconditions.
        unsafe { sys::ngtcp2_err_is_fatal(self.0) != 0 }
    }

    /// A short description, as ngtcp2 words it.
    pub fn describe(self) -> &'static str {
        // SAFETY: `ngtcp2_strerror` returns a pointer to a static string for every input,
        // including codes it does not recognise.
        let raw = unsafe { sys::ngtcp2_strerror(self.0) };
        if raw.is_null() {
            return "unknown error";
        }
        // SAFETY: the returned string is static, NUL-terminated and never freed.
        unsafe { core::ffi::CStr::from_ptr(raw) }
            .to_str()
            .unwrap_or("unknown error")
    }
}

impl fmt::Debug for NativeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NativeCode({}: {})", self.0, self.describe())
    }
}

impl fmt::Display for NativeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.describe(), self.0)
    }
}

/// A QUIC **transport** error code.
///
/// Defined by the QUIC transport specification and meaningful to any QUIC implementation.
/// Distinct from [`ApplicationErrorCode`]: the same integer means different things in the
/// two spaces, and the type system is what keeps them apart.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TransportErrorCode(u64);

impl TransportErrorCode {
    /// No error. The peer is closing the connection without complaint.
    pub const NO_ERROR: Self = Self(sys::NGTCP2_NO_ERROR as u64);

    /// Wraps a raw transport error code.
    pub const fn new(code: u64) -> Self {
        Self(code)
    }

    /// The raw value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The transport error code ngtcp2 infers for a native error.
    ///
    /// Uses ngtcp2's own mapping rather than a table maintained here, so this crate cannot
    /// disagree with the library it wraps.
    pub fn infer(native: NativeCode) -> Self {
        // SAFETY: a pure mapping over an integer, with no preconditions.
        Self(unsafe { sys::ngtcp2_err_infer_quic_transport_error_code(native.get()) })
    }
}

impl fmt::Display for TransportErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "transport error 0x{:x}", self.0)
    }
}

/// A QUIC **application** error code.
///
/// Opaque to QUIC itself: it means whatever the protocol running over the connection says
/// it means. HTTP/3 defines its own set, for instance. Distinct from
/// [`TransportErrorCode`] for that reason.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ApplicationErrorCode(u64);

impl ApplicationErrorCode {
    /// Wraps a raw application error code.
    pub const fn new(code: u64) -> Self {
        Self(code)
    }

    /// The raw value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ApplicationErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "application error 0x{:x}", self.0)
    }
}

/// Who ended a connection, and on what terms.
///
/// Reported by [`Conn::close_error`](crate::Conn::close_error). The distinction that
/// matters most is between an *application* close, whose code means whatever the protocol
/// running over QUIC says it means, and everything else — because only the application
/// close carries information the application itself sent.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum CloseReason {
    /// A QUIC-level error. `NO_ERROR` here is the ordinary way a connection ends.
    Transport(TransportErrorCode),
    /// The protocol running over QUIC closed the connection with its own code.
    Application(ApplicationErrorCode),
    /// No version in common.
    VersionNegotiation,
    /// Nothing arrived for the idle timeout, so the connection lapsed.
    ///
    /// Worth distinguishing from a transport close: no frame was exchanged and the peer may
    /// not know the connection is over.
    IdleTimeout,
    /// ngtcp2 abandoned the connection without sending anything.
    Dropped,
    /// A Retry packet ended this connection attempt.
    Retry,
}

/// Why a connection closed.
///
/// Only meaningful once something *has* closed it — a
/// [`ReadOutcome::Draining`](crate::ReadOutcome), a
/// [`ReadOutcome::Closing`](crate::ReadOutcome), an
/// [`ExpiryOutcome::IdleClose`](crate::ExpiryOutcome), or a local close. Before then it
/// reports the connection's initial, unset error, which is indistinguishable from a
/// graceful close with `NO_ERROR` — the two really are the same bytes, so this type does
/// not pretend to tell them apart.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CloseError {
    reason: CloseReason,
    frame_type: u64,
    phrase: Vec<u8>,
}

impl CloseError {
    /// Builds from ngtcp2's own error struct.
    ///
    /// # Safety
    ///
    /// `raw.reason` must be null or point to `raw.reasonlen` readable bytes.
    pub(crate) unsafe fn from_raw(raw: &ngnet_quic_sys::ngtcp2_ccerr) -> Self {
        use ngnet_quic_sys as sys;

        let reason = match raw.type_ {
            sys::NGTCP2_CCERR_TYPE_APPLICATION => {
                CloseReason::Application(ApplicationErrorCode::new(raw.error_code))
            }
            sys::NGTCP2_CCERR_TYPE_VERSION_NEGOTIATION => CloseReason::VersionNegotiation,
            sys::NGTCP2_CCERR_TYPE_IDLE_CLOSE => CloseReason::IdleTimeout,
            sys::NGTCP2_CCERR_TYPE_DROP_CONN => CloseReason::Dropped,
            sys::NGTCP2_CCERR_TYPE_RETRY => CloseReason::Retry,
            // Including `NGTCP2_CCERR_TYPE_TRANSPORT`. An unrecognised type is reported as
            // a transport close rather than dropped, because the code is still meaningful.
            _ => CloseReason::Transport(TransportErrorCode::new(raw.error_code)),
        };

        // ngtcp2 truncates a received phrase to 1024 bytes, so this is bounded by the
        // library rather than by the peer.
        let phrase = if raw.reason.is_null() || raw.reasonlen == 0 {
            Vec::new()
        } else {
            // SAFETY: the caller guarantees `reasonlen` bytes are readable at `reason`.
            unsafe { core::slice::from_raw_parts(raw.reason, raw.reasonlen) }.to_vec()
        };

        Self {
            reason,
            frame_type: raw.frame_type,
            phrase,
        }
    }

    /// Who closed the connection, and with what code.
    pub fn reason(&self) -> &CloseReason {
        &self.reason
    }

    /// The frame type that triggered the closure, or zero if it is not known.
    pub fn frame_type(&self) -> u64 {
        self.frame_type
    }

    /// The reason phrase, which may be empty and is not guaranteed to be text.
    pub fn phrase(&self) -> &[u8] {
        &self.phrase
    }

    /// An idle-timeout close, for tests that need one without waiting for a timeout.
    #[cfg(test)]
    pub(crate) fn idle_for_test() -> Self {
        Self {
            reason: CloseReason::IdleTimeout,
            frame_type: 0,
            phrase: Vec::new(),
        }
    }
}

impl fmt::Display for CloseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reason {
            CloseReason::Transport(code) => write!(f, "closed by {code}")?,
            CloseReason::Application(code) => write!(f, "closed by {code}")?,
            CloseReason::VersionNegotiation => f.write_str("closed: no common QUIC version")?,
            CloseReason::IdleTimeout => f.write_str("closed: idle timeout")?,
            CloseReason::Dropped => f.write_str("closed: connection dropped")?,
            CloseReason::Retry => f.write_str("closed: superseded by a Retry")?,
        }
        if !self.phrase.is_empty()
            && let Ok(text) = core::str::from_utf8(&self.phrase)
        {
            write!(f, ": {text}")?;
        }
        Ok(())
    }
}

/// The broad category of a failure.
///
/// Deliberately coarse. The exact condition is always available through
/// [`Error::native_code`]; these variants exist so a caller can decide what to *do* without
/// matching on two dozen codes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The peer violated the transport protocol, or sent something this endpoint will not
    /// accept.
    Protocol,
    /// An allocation failed. Always fatal to the connection.
    Exhausted,
    /// This crate's caller passed something invalid.
    ///
    /// Distinguished from [`ErrorKind::Protocol`] because it indicates a bug on this side
    /// of the connection rather than misbehaviour on the other. ngtcp2 checks most of these
    /// with `assert()`, which is compiled out of release builds, so this crate checks them
    /// itself.
    InvalidInput,
    /// The TLS handshake failed.
    Crypto,
    /// The connection is no longer usable and only dropping it is permitted.
    ConnectionUnusable,
    /// The connection is closing or draining and will accept no new work.
    Closing,
    /// The peer's limits prevent this right now; it may succeed later.
    ///
    /// Covers the stream-count and flow-control blocks, which are ordinary conditions in a
    /// working connection rather than failures.
    Blocked,
    /// The stream cannot carry more data, and never will.
    ///
    /// Its write side is closed — this endpoint finished or reset it, or the stream is gone
    /// altogether. Distinguished from [`ErrorKind::InvalidInput`], which it used to share,
    /// because the two call for opposite responses: invalid input is a bug to fix, while
    /// this is a normal end that a caller must be able to recognise in order to stop
    /// offering bytes and move on. A layer multiplexing many streams over one connection
    /// needs to tell "this stream is finished" from "you called this wrongly" without
    /// inspecting a message.
    StreamClosed,
    /// Anything else, including conditions added by a future ngtcp2.
    Internal,
}

/// A failure from ngtcp2, or from this crate's own validation.
#[derive(Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    native: Option<NativeCode>,
    context: &'static str,
    /// Whether the connection this came from is now unusable.
    ///
    /// Not derivable from the code alone: the same condition can be recoverable from one
    /// entry point and terminal from another, and only the connection knows which happened.
    unusable: bool,
}

impl Error {
    /// Builds an error a TLS backend can report.
    ///
    /// The only publicly constructible error, and it exists for one reason: the safe TLS
    /// seam has methods returning [`Result`], so a backend that cannot build an [`Error`]
    /// cannot implement the seam. Everything else in this type is built from an ngtcp2
    /// return code, which a backend has no business inventing.
    ///
    /// The kind is fixed at [`ErrorKind::Crypto`] rather than taken as an argument, because
    /// the alternative is asking backend authors to classify their failure against a set of
    /// conditions that mostly describe transport faults they cannot cause. What a backend
    /// can usefully say is *why*, which is what `context` carries.
    ///
    /// Note this is for failures of the backend itself. A payload that does not
    /// authenticate is not one of these — that is [`crate::CryptoError::Decrypt`], and
    /// reporting it here would turn an ordinary event into a closed connection.
    pub const fn backend(context: &'static str) -> Self {
        Self {
            kind: ErrorKind::Crypto,
            native: None,
            context,
            unusable: false,
        }
    }

    /// Builds an error from a raw ngtcp2 code.
    pub(crate) fn native(code: i32, context: &'static str) -> Self {
        let native = NativeCode::new(code);
        Self {
            kind: classify(native),
            native: Some(native),
            context,
            unusable: false,
        }
    }

    /// Marks this failure as one that left the connection unusable.
    pub(crate) fn into_unusable(mut self) -> Self {
        self.unusable = true;
        self
    }

    /// Builds an error for a precondition this crate checked itself.
    ///
    /// `const` so that validating constructors can be `const` too, and a bad literal fails
    /// to compile rather than at run time.
    pub(crate) const fn invalid_input(context: &'static str) -> Self {
        Self {
            kind: ErrorKind::InvalidInput,
            native: None,
            context,
            unusable: false,
        }
    }

    /// Builds an error of a given kind with no native code behind it.
    ///
    /// For failures that originate outside ngtcp2 — a TLS backend, say — where there is a
    /// meaningful classification but no ngtcp2 error to attach.
    pub(crate) const fn with_kind(kind: ErrorKind, context: &'static str) -> Self {
        Self {
            kind,
            native: None,
            context,
            unusable: false,
        }
    }

    /// Builds the error every operation returns once the connection is poisoned.
    pub(crate) fn unusable() -> Self {
        Self {
            kind: ErrorKind::ConnectionUnusable,
            native: None,
            context: "the connection encountered an unrecoverable error and cannot be used",
            unusable: true,
        }
    }

    /// The broad category of this failure.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The underlying ngtcp2 code, if this came from ngtcp2 rather than from this crate's
    /// own validation.
    pub fn native_code(&self) -> Option<NativeCode> {
        self.native
    }

    /// Whether this failure left the connection unusable.
    pub fn is_fatal(&self) -> bool {
        self.unusable
            || self.kind == ErrorKind::ConnectionUnusable
            || self.native.is_some_and(NativeCode::is_fatal)
    }

    /// The QUIC transport error code to close the connection with.
    ///
    /// Returns `None` for failures that did not come from ngtcp2, which have no transport
    /// meaning to convey.
    pub fn transport_error_code(&self) -> Option<TransportErrorCode> {
        self.native.map(TransportErrorCode::infer)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("native", &self.native)
            .field("context", &self.context)
            .field("unusable", &self.unusable)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.native {
            Some(native) => write!(f, "{}: {native}", self.context),
            None => write!(f, "{}", self.context),
        }
    }
}

impl core::error::Error for Error {}

/// Maps a raw ngtcp2 code onto a coarse category.
///
/// Codes not named here fall to [`ErrorKind::Internal`], which is why that variant exists:
/// a future ngtcp2 may add codes this build has never heard of, and they must still arrive
/// as something rather than being lost.
fn classify(native: NativeCode) -> ErrorKind {
    match native.get() {
        sys::NGTCP2_ERR_NOMEM => ErrorKind::Exhausted,
        sys::NGTCP2_ERR_INVALID_ARGUMENT | sys::NGTCP2_ERR_INVALID_STATE => ErrorKind::InvalidInput,
        sys::NGTCP2_ERR_CRYPTO
        | sys::NGTCP2_ERR_REQUIRED_TRANSPORT_PARAM
        | sys::NGTCP2_ERR_MALFORMED_TRANSPORT_PARAM
        | sys::NGTCP2_ERR_TRANSPORT_PARAM
        | sys::NGTCP2_ERR_VERSION_NEGOTIATION_FAILURE => ErrorKind::Crypto,
        // Genuinely transient: the same call may succeed later. `NOBUF` belongs here rather
        // than with the internal errors -- it means the buffer offered was too small or the
        // amplification limit is in force, and retrying with more room or after more credit
        // can succeed.
        sys::NGTCP2_ERR_STREAM_DATA_BLOCKED
        | sys::NGTCP2_ERR_STREAM_ID_BLOCKED
        | sys::NGTCP2_ERR_CONN_ID_BLOCKED
        | sys::NGTCP2_ERR_NOBUF => ErrorKind::Blocked,
        // Not transient, and not a caller mistake either: a stream whose write side is
        // closed, or one that no longer exists, will refuse every further write. A caller
        // multiplexing streams needs to recognise that and stop offering, which it cannot do
        // if this is indistinguishable from having called the API wrongly.
        sys::NGTCP2_ERR_STREAM_SHUT_WR | sys::NGTCP2_ERR_STREAM_NOT_FOUND => {
            ErrorKind::StreamClosed
        }
        sys::NGTCP2_ERR_CLOSING | sys::NGTCP2_ERR_DRAINING => ErrorKind::Closing,
        sys::NGTCP2_ERR_PROTO
        | sys::NGTCP2_ERR_FRAME_ENCODING
        | sys::NGTCP2_ERR_ACK_FRAME
        | sys::NGTCP2_ERR_FINAL_SIZE
        | sys::NGTCP2_ERR_FLOW_CONTROL
        | sys::NGTCP2_ERR_STREAM_LIMIT => ErrorKind::Protocol,
        // Terminal. `AEAD_LIMIT_REACHED` is the one worth naming: ngtcp2 documents that the
        // connection must be dropped immediately, because continuing would reuse key
        // material past its safe limit. Classifying it as `Internal` would leave
        // `is_fatal()` false and invite a caller to carry on.
        sys::NGTCP2_ERR_CALLBACK_FAILURE
        | sys::NGTCP2_ERR_INTERNAL
        | sys::NGTCP2_ERR_AEAD_LIMIT_REACHED => ErrorKind::ConnectionUnusable,
        _ => ErrorKind::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend-reported failure carries no native code, and says so.
    ///
    /// The absence matters. `native_code()` is how a caller tells a fault the library
    /// diagnosed from one an application invented, and attributing a made-up code to ngtcp2
    /// would make that distinction useless. It is also why this constructor takes no code
    /// rather than taking an optional one.
    #[test]
    fn a_backend_error_is_a_crypto_failure_with_no_native_code() {
        let error = Error::backend("the backend could not derive a key");
        assert_eq!(error.kind(), ErrorKind::Crypto);
        assert_eq!(error.native_code(), None);
        assert!(
            format!("{error}").contains("could not derive a key"),
            "a backend's explanation must survive into the message a caller sees"
        );
    }

    #[test]
    fn native_codes_describe_themselves_using_the_library() {
        let code = NativeCode::new(sys::NGTCP2_ERR_NOMEM);
        assert!(!code.describe().is_empty());
        assert_ne!(code.describe(), "unknown error");
    }

    #[test]
    fn a_code_ngtcp2_does_not_know_still_yields_a_description() {
        // The point is that `describe` never returns a dangling or empty string, even for
        // a value ngtcp2 has no name for.
        let code = NativeCode::new(-999_999);
        assert!(!code.describe().is_empty());
    }

    #[test]
    fn out_of_memory_is_fatal_and_classified_as_exhausted() {
        let code = NativeCode::new(sys::NGTCP2_ERR_NOMEM);
        assert!(code.is_fatal());
        assert_eq!(classify(code), ErrorKind::Exhausted);
    }

    #[test]
    fn blocking_conditions_are_not_protocol_errors() {
        // These are ordinary states of a working connection, and a caller that treats them
        // as failures will close connections it should have waited on.
        for raw in [
            sys::NGTCP2_ERR_STREAM_DATA_BLOCKED,
            sys::NGTCP2_ERR_STREAM_ID_BLOCKED,
        ] {
            assert_eq!(classify(NativeCode::new(raw)), ErrorKind::Blocked);
        }
    }

    #[test]
    fn an_unknown_code_classifies_as_internal_rather_than_being_lost() {
        assert_eq!(classify(NativeCode::new(-31_337)), ErrorKind::Internal);
    }

    #[test]
    fn validation_errors_carry_no_native_code() {
        let err = Error::invalid_input("test");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(err.native_code().is_none());
        assert!(err.transport_error_code().is_none());
    }

    #[test]
    fn the_two_error_code_spaces_are_distinct_types() {
        // Same integer, different meaning. This test exists to pin that they cannot be
        // interchanged; it would fail to compile if they became one type.
        let transport = TransportErrorCode::new(0x0a);
        let application = ApplicationErrorCode::new(0x0a);
        assert_eq!(transport.get(), application.get());
    }

    #[test]
    fn transport_codes_are_inferred_by_the_library() {
        let inferred = TransportErrorCode::infer(NativeCode::new(sys::NGTCP2_ERR_FLOW_CONTROL));
        assert_ne!(inferred.get(), 0);
    }
}
