//! Deciding whether a first packet has earned a connection.
//!
//! The policy half of address validation; the cryptography is in [`crate::token`]. What
//! lives here is what a server *does* with a first packet, and the two things that make
//! doing it safe: never committing per-connection state before an address is validated, and
//! never answering an unmatched datagram with something bigger than it.

#![cfg(feature = "tls-ossl")]

use core::net::SocketAddr;

use crate::cid::ConnectionId;
use crate::rand::EntropySource;
use crate::time::{Duration, Timestamp};
use crate::token::{self, TokenSecret};

/// How long a Retry token stays valid.
///
/// Ten seconds, matching ngtcp2's own server. Long enough for a client on a slow path to
/// come back, short enough that a captured token is not a lasting credential.
pub const DEFAULT_TOKEN_LIFETIME: Duration = Duration::from_nanos(10_000_000_000);

/// How many stateless resets may be sent in a burst.
///
/// A budget rather than a rate limiter proper. Answering unmatched traffic is useful — it
/// tells a peer that has lost state to stop retransmitting — but doing it without limit
/// turns a flood of spoofed datagrams into a flood aimed at whoever they name.
pub const DEFAULT_RESET_BURST: u32 = 100;

/// A server's address-validation configuration.
pub(crate) struct Validation {
    secret: TokenSecret,
    lifetime: Duration,
    /// Stateless resets still permitted in this burst.
    reset_budget: u32,
    /// The size of the burst, restored as time passes.
    reset_burst: u32,
    /// When the budget was last refilled.
    refilled_at: Option<Timestamp>,
}

/// What to do with a first packet.
#[derive(Debug)]
pub(crate) enum Decision {
    /// Build a connection, using this as the identifier the client first addressed.
    Accept(ConnectionId),
    /// Answer with a Retry carrying these bytes, sent from this identifier.
    Retry {
        /// The identifier the server puts in the Retry as its source.
        scid: ConnectionId,
        /// The token to carry.
        token: Vec<u8>,
    },
    /// Answer with nothing.
    Ignore,
}

impl Validation {
    /// Builds a validating policy.
    pub(crate) fn new(secret: TokenSecret) -> Self {
        Self {
            secret,
            lifetime: DEFAULT_TOKEN_LIFETIME,
            reset_budget: DEFAULT_RESET_BURST,
            reset_burst: DEFAULT_RESET_BURST,
            refilled_at: None,
        }
    }

    /// Sets how long a token stays valid.
    pub(crate) fn lifetime(&mut self, lifetime: Duration) {
        self.lifetime = lifetime;
    }

    /// Sets the stateless reset burst.
    pub(crate) fn reset_burst(&mut self, burst: u32) {
        self.reset_burst = burst;
        self.reset_budget = burst;
    }

    /// Decides what a first packet has earned.
    ///
    /// Note what this does *not* do: it never creates state keyed on the client's address
    /// or identifiers. A Retry is computed from the packet and the server's secret and then
    /// forgotten, which is what makes it cheap enough to answer a flood with.
    pub(crate) fn decide(
        &self,
        packet: &crate::accept::InitialPacket,
        remote: SocketAddr,
        entropy: &mut dyn EntropySource,
        now: Timestamp,
    ) -> Decision {
        if let crate::accept::InitialToken::Retry(bytes) = &packet.token {
            // The identifier the token was minted against is the one the client is
            // addressing now -- that binding is what stops a token being replayed onto a
            // different connection attempt.
            // A token that does not verify is treated as no token at all: the client gets a
            // fresh Retry rather than an error, because the usual cause is a token that
            // simply expired.
            if let Ok(Some(original)) = token::verify_retry_token(
                &self.secret,
                bytes,
                packet.version,
                remote,
                &packet.dcid,
                self.lifetime,
                now,
            ) {
                return Decision::Accept(original);
            }
        }

        let Ok(scid) = ConnectionId::generate(entropy, crate::cid::DEFAULT_LEN) else {
            return Decision::Ignore;
        };
        match token::issue_retry_token(
            &self.secret,
            packet.version,
            remote,
            &scid,
            &packet.dcid,
            now,
        ) {
            Ok(token) => Decision::Retry { scid, token },
            Err(_) => Decision::Ignore,
        }
    }

    /// Takes one stateless reset from the budget, refilling it as time passes.
    ///
    /// Returns `false` when the budget is spent, which means the datagram draws silence.
    pub(crate) fn take_reset_budget(&mut self, now: Timestamp) -> bool {
        // Refill once a second, all at once. Finer granularity would be more elegant and
        // would change nothing: the budget exists to bound a flood, not to shape traffic.
        match self.refilled_at {
            Some(at) if now.as_nanos().saturating_sub(at.as_nanos()) >= 1_000_000_000 => {
                self.reset_budget = self.reset_burst;
                self.refilled_at = Some(now);
            }
            None => self.refilled_at = Some(now),
            Some(_) => {}
        }

        if self.reset_budget == 0 {
            return false;
        }
        self.reset_budget -= 1;
        true
    }

    /// The secret, for deriving reset tokens.
    pub(crate) fn secret(&self) -> &TokenSecret {
        &self.secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accept::{InitialPacket, InitialToken};

    struct Counter(u64);

    impl EntropySource for Counter {
        fn fill(&mut self, out: &mut [u8]) -> crate::error::Result<()> {
            for byte in out.iter_mut() {
                self.0 = self.0.wrapping_add(1);
                *byte = self.0 as u8;
            }
            Ok(())
        }
    }

    fn packet(token: InitialToken) -> InitialPacket {
        InitialPacket {
            version: crate::VERSION_V1,
            dcid: ConnectionId::new(&[1; 8]).expect("valid"),
            scid: ConnectionId::new(&[2; 8]).expect("valid"),
            token,
        }
    }

    fn now() -> Timestamp {
        Timestamp::from_nanos(1_000_000_000).expect("valid")
    }

    fn remote() -> SocketAddr {
        "127.0.0.1:4433".parse().expect("valid")
    }

    fn validation() -> Validation {
        Validation::new(TokenSecret::new(&[0x11; 32]).expect("valid"))
    }

    #[test]
    fn a_first_packet_with_no_token_draws_a_retry() {
        let policy = validation();
        let mut entropy = Counter(0);
        match policy.decide(&packet(InitialToken::Absent), remote(), &mut entropy, now()) {
            Decision::Retry { token, .. } => assert!(!token.is_empty()),
            other => panic!("expected a Retry, got {other:?}"),
        }
    }

    #[test]
    fn a_retried_packet_carrying_its_token_is_accepted() {
        let policy = validation();
        let mut entropy = Counter(0);

        let Decision::Retry { scid, token } =
            policy.decide(&packet(InitialToken::Absent), remote(), &mut entropy, now())
        else {
            panic!("expected a Retry");
        };

        // What the client sends next: the token, addressed to the identifier the Retry
        // named as its source.
        let retried = InitialPacket {
            version: crate::VERSION_V1,
            dcid: scid,
            scid: ConnectionId::new(&[2; 8]).expect("valid"),
            token: InitialToken::Retry(token),
        };

        match policy.decide(&retried, remote(), &mut entropy, now()) {
            Decision::Accept(original) => assert_eq!(
                original,
                ConnectionId::new(&[1; 8]).expect("valid"),
                "the identifier the client first addressed must be recovered, or the \
                 server cannot build a connection"
            ),
            other => panic!("expected an accept, got {other:?}"),
        }
    }

    #[test]
    fn a_token_replayed_from_another_address_draws_another_retry() {
        // The property the whole mechanism rests on. If this ever accepted, a spoofed
        // source could reuse a captured token and the amplification would be back.
        let policy = validation();
        let mut entropy = Counter(0);
        let Decision::Retry { scid, token } =
            policy.decide(&packet(InitialToken::Absent), remote(), &mut entropy, now())
        else {
            panic!("expected a Retry");
        };

        let retried = InitialPacket {
            version: crate::VERSION_V1,
            dcid: scid,
            scid: ConnectionId::new(&[2; 8]).expect("valid"),
            token: InitialToken::Retry(token),
        };

        let elsewhere: SocketAddr = "127.0.0.2:9999".parse().expect("valid");
        match policy.decide(&retried, elsewhere, &mut entropy, now()) {
            Decision::Retry { .. } => {}
            other => panic!("a token from another address was accepted: {other:?}"),
        }
    }

    #[test]
    fn a_tampered_token_draws_another_retry_rather_than_an_error() {
        // A client whose token expired is indistinguishable from one that forged a bad
        // one, and the useful answer to both is a fresh Retry.
        let policy = validation();
        let mut entropy = Counter(0);
        let Decision::Retry { scid, mut token } =
            policy.decide(&packet(InitialToken::Absent), remote(), &mut entropy, now())
        else {
            panic!("expected a Retry");
        };
        let last = token.len() - 1;
        token[last] ^= 0xff;

        let retried = InitialPacket {
            version: crate::VERSION_V1,
            dcid: scid,
            scid: ConnectionId::new(&[2; 8]).expect("valid"),
            token: InitialToken::Retry(token),
        };
        assert!(matches!(
            policy.decide(&retried, remote(), &mut entropy, now()),
            Decision::Retry { .. }
        ));
    }

    #[test]
    fn the_reset_budget_runs_out_and_then_refills() {
        let mut policy = validation();
        policy.reset_burst(3);

        assert!(policy.take_reset_budget(now()));
        assert!(policy.take_reset_budget(now()));
        assert!(policy.take_reset_budget(now()));
        assert!(
            !policy.take_reset_budget(now()),
            "the budget did not run out, so a flood would be answered without limit"
        );

        let later = Timestamp::from_nanos(now().as_nanos() + 2_000_000_000).expect("valid");
        assert!(
            policy.take_reset_budget(later),
            "the budget never refilled, so one burst would silence the endpoint for good"
        );
    }

    #[test]
    fn deciding_creates_no_state() {
        // What makes Retry cheap enough to answer a flood with. If deciding grew a table,
        // an attacker would fill it -- which is the attack Retry exists to prevent, moved
        // one level down.
        let policy = validation();
        let mut entropy = Counter(0);
        for _ in 0..1000 {
            let _ = policy.decide(&packet(InitialToken::Absent), remote(), &mut entropy, now());
        }
        // `decide` takes `&self`: there is nowhere for per-client state to go, and this is
        // the assertion. The loop is here so the claim is exercised rather than merely
        // stated.
    }
}
