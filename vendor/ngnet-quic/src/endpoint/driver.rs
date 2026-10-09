//! The loop that makes everything happen.
//!
//! # What one pass does, and why in this order
//!
//! 1. **Drain commands** from the handles, bounded, so a caller's writes reach ngtcp2
//!    before this pass produces packets rather than after.
//! 2. **Read datagrams** from the socket, bounded by
//!    [`Config::datagrams_per_pass`](super::Config), routing each to its connection.
//! 3. **Service expired timers**, since a datagram may have made a timer fire and a timer
//!    may produce something to send.
//! 4. **Write**, draining every connection that has anything until it says it has nothing.
//! 5. **Rearm** from the earliest [`Conn::expiry`](crate::Conn::expiry) across all
//!    connections.
//!
//! Steps 2 and 4 are bounded and step 5 is not skippable. Between them those two facts are
//! the whole liveness argument: a connection cannot monopolise a pass, and no pass ends
//! without a timer armed for whatever wants attention next.
//!
//! # The two mistakes this shape exists to avoid
//!
//! **Not rearming after a write pass.** ngtcp2 paces its sending, and reports the pacing
//! deadline through the same `expiry()` as everything else
//! (`crates/ngnet-quic-sys/vendor/ngtcp2/lib/ngtcp2_conn.c:11387`). A driver that only rearms after reading will
//! send one datagram and then sleep until the peer says something — which for a bulk
//! transfer is never, and which looks like a hang rather than a slow connection.
//!
//! **Calling `update_pkt_tx_time` here.** It is tempting, since this is where the pacing is
//! respected. The core's write paths already call it (`src/packet.rs:168-175`,
//! `src/stream_io.rs:177-184`), and calling it again would push the deadline forward twice
//! per packet, halving the send rate for no visible reason.

use core::net::SocketAddr;
use core::task::{Context, Poll};
use std::collections::HashMap;
use std::sync::Arc;

use crate::conn::Conn;
use crate::error::ApplicationErrorCode;
use crate::handlers::Handlers;
use crate::packet::{ExpiryOutcome, ReadOutcome, WriteOutcome};
use crate::stream::StreamId;
use crate::stream_io::StreamWrite;
use crate::time::Timestamp;
use crate::tls::{Backend as TlsBackend, Role, Session};

use super::clock::Clock;
use super::config::Config;
use super::error::{Error, ErrorKind};
use super::shared::{Command, ConnectionShared, Observed, RouteUpdate};
use super::socket::{AsyncUdpSocket, Sent};

/// Makes an entropy source for a new connection.
///
/// `Send` on the factory itself, not just on what it produces. The core already requires an
/// entropy source to be `Send` -- a `Conn` is `Send` and owns one -- so a factory that was
/// not would have to capture thread-bound state in order to produce values that are not,
/// which is a shape nothing wants. Requiring it here is what lets the driver's own `Send`
/// follow from its socket and clock rather than being blocked by this.
pub(crate) type EntropyFactory =
    Box<dyn Fn() -> Box<dyn crate::rand::EntropySource + Send> + Send>;

/// The largest datagram this endpoint will read or write.
///
/// A receive buffer must be big enough for anything a peer may legitimately send, and QUIC
/// permits up to the path MTU. The *send* side is sized from what a connection reports it
/// may currently emit; this is only the ceiling on the buffer, which is capacity rather
/// than permission.
pub(crate) const MAX_DATAGRAM: usize = 1500;

/// One connection the driver owns, with the state its handle shares.
pub(crate) struct Tracked<S: Session> {
    /// The connection, when this endpoint is the one driving it.
    ///
    /// `'static` because its handlers capture an `Arc` rather than borrowing anything the
    /// driver owns — see [`super::shared`].
    ///
    /// `None` once the connection has been *detached*: handed to a caller who drives it
    /// themselves. The endpoint keeps routing datagrams to it and sending what it produces,
    /// but no longer reads or writes its protocol state, because there is exactly one owner
    /// of that state and it is no longer this one. A consumer that must reach the connection
    /// synchronously while composing a packet — which the HTTP/3 layer does — cannot be
    /// served across a queue, so it takes the connection instead.
    pub(crate) conn: Option<Conn<'static, S>>,
    /// What the handle reads and writes.
    pub(crate) shared: Arc<ConnectionShared>,
    /// Where its datagrams go.
    pub(crate) remote: SocketAddr,
    /// A datagram the socket refused, to be offered again.
    ///
    /// Holding it here rather than dropping it is what stops a would-blocked send from
    /// silently losing a packet.
    pub(crate) pending: Option<Vec<u8>>,
    /// Whether this connection has been reported finished.
    pub(crate) finished: bool,
}

/// The next datagram a connection wants to send, and where its bytes live.
///
/// A datagram the core just produced is written into a buffer the caller lends, so sending
/// it costs no allocation; only if the socket then refuses it does it have to be copied
/// somewhere that outlives the buffer. A datagram that is already owned -- a retained
/// `pending`, or one a detached connection queued -- is forwarded as itself, because it
/// already lives long enough and copying it would be the very waste this avoids.
pub(crate) enum Produced {
    /// Written into the buffer the caller lent to `next_datagram`; its length is here.
    InBuffer(usize),
    /// Already owned, and forwarded as itself.
    Owned(Vec<u8>),
}

/// Builds the handler set a driven connection uses.
///
/// Every closure captures an `Arc` and nothing else, which is what makes the resulting
/// `Conn` `'static` and satisfies the `Send` bound the core requires.
pub(crate) fn handlers_for(shared: &Arc<ConnectionShared>) -> Handlers<'static> {
    let data = Arc::clone(shared);
    let opened = Arc::clone(shared);
    let closed = Arc::clone(shared);
    let reset = Arc::clone(shared);
    let stop = Arc::clone(shared);
    let acked = Arc::clone(shared);
    let established = Arc::clone(shared);
    let minted = Arc::clone(shared);
    let retired = Arc::clone(shared);
    let bidi = Arc::clone(shared);
    let uni = Arc::clone(shared);

    Handlers::new()
        .on_stream_data(move |id, bytes, fin| {
            data.observe(Observed::Data(id, bytes.to_vec(), fin));
        })
        .on_stream_open(move |id| opened.observe(Observed::Opened(id)))
        .on_stream_close(move |id, reason| closed.observe(Observed::Closed(id, reason)))
        .on_stream_reset(move |id, code| reset.observe(Observed::Reset(id, code)))
        .on_stop_sending(move |id, code| stop.observe(Observed::StopSending(id, code)))
        .on_acked_stream_data(move |id, len| acked.observe(Observed::Acked(id, len)))
        .on_handshake_completed(move || established.observe(Observed::HandshakeCompleted))
        // Identifier changes go to the routing queue rather than the observation queue: the
        // endpoint needs them whoever is driving the connection, and a detached connection's
        // driver would otherwise consume them and leave the endpoint routing to a stale set.
        .on_extend_max_local_streams_bidi(move |max| {
            bidi.observe(Observed::StreamsExtended(max));
            bidi.wake_all();
        })
        .on_extend_max_local_streams_uni(move |max| {
            uni.observe(Observed::StreamsExtended(max));
            uni.wake_all();
        })
        .on_new_connection_id(move |cid| minted.observe_route(RouteUpdate::Minted(*cid)))
        .on_remove_connection_id(move |cid| retired.observe_route(RouteUpdate::Retired(*cid)))
}

/// Everything the driver owns.
pub(crate) struct Inner<Sock, Clk, B>
where
    Sock: AsyncUdpSocket,
    Clk: Clock,
    B: TlsBackend,
{
    pub(crate) socket: Sock,
    pub(crate) clock: Clk,
    pub(crate) backend: B,
    pub(crate) config: Config,
    /// Makes an entropy source for each new connection.
    ///
    /// Supplied by the caller for the same reason the clock is: this crate reads no clock
    /// and owns no random number generator, and QUIC needs cryptographically secure
    /// randomness for connection identifiers and stateless reset tokens. Picking one here
    /// would be picking it on the caller's behalf, and picking a weak one would be a
    /// security defect nothing in the API would reveal.
    pub(crate) entropy: EntropyFactory,
    /// Connections, keyed by an index that never repeats.
    pub(crate) connections: HashMap<u64, Tracked<B::Session>>,
    /// Which connection each identifier routes to.
    pub(crate) routes: HashMap<Vec<u8>, u64>,
    /// The next connection index.
    pub(crate) next_index: u64,
    /// A receive buffer, reused so a pass allocates nothing.
    pub(crate) buffer: Vec<u8>,
    /// A send buffer, reused so producing a datagram the socket accepts allocates nothing.
    ///
    /// The core writes each datagram it produces into here, and it is sent straight from
    /// this buffer when the socket takes it. Only a datagram the socket refuses -- one that
    /// has to be retained until a later pass, past the point where this buffer is reused --
    /// is copied out into an owned buffer of its own. Reused across passes, like `buffer`.
    pub(crate) send_buffer: Vec<u8>,
    /// A scratch list of connection indices, reused so iterating the connections while the
    /// loop body mutates them costs no allocation. `service` and `flush` both need to walk
    /// the connections without holding a borrow of the map across a body that inserts,
    /// removes or re-borrows; each takes this with `mem::take`, fills it, and restores it,
    /// the same discipline `buffer` above uses. They never overlap -- `service` restores it
    /// before calling `flush` -- so one buffer serves both.
    pub(crate) index_scratch: Vec<u64>,
    /// Whether this endpoint accepts connections it did not initiate.
    pub(crate) accepts: bool,
    /// Datagrams that belong to no connection -- Retry, Version Negotiation, stateless
    /// reset. They cannot be queued on a connection because the whole point of each is that
    /// there is not one.
    pub(crate) outbox: std::collections::VecDeque<(SocketAddr, Vec<u8>)>,
    /// How address validation is configured, if at all.
    #[cfg(feature = "tls-ossl")]
    pub(crate) validation: Option<super::validate::Validation>,
    /// Connections handed to callers who drive them themselves.
    pub(crate) detached: Arc<super::handle::DetachQueue<B::Session>>,
    /// The endpoint's clock, erased once at construction.
    ///
    /// A detached connection has to read the *same* timescale the endpoint drove its
    /// handshake against, or every timestamp afterwards is incomparable with the ones
    /// already recorded. Erased here rather than at each hand-over so the bounds that
    /// requires sit in one place -- see `EndpointBuilder::build`.
    /// `None` on an endpoint built with `build` rather than `build_detachable`, where no
    /// connection can be handed over and so no shared timescale is needed.
    pub(crate) timescale: Option<Arc<dyn Fn() -> Timestamp + Send + Sync>>,
    /// Sleeps on that same clock. See `DetachedConnection::sleep_until`.
    pub(crate) sleeper: Option<Arc<dyn Fn(Timestamp) -> super::handle::Sleep + Send + Sync>>,
    /// The armed sleep, if any.
    pub(crate) sleeping: Option<Clk::Sleep>,
    /// The deadline the armed sleep is for.
    pub(crate) sleeping_until: Option<Timestamp>,
}

impl<Sock, Clk, B> Inner<Sock, Clk, B>
where
    Sock: AsyncUdpSocket,
    Clk: Clock,
    B: TlsBackend,
{
    /// Registers a connection under every identifier it currently answers to.
    pub(crate) fn install_routes(&mut self, index: u64) {
        let Some(tracked) = self.connections.get(&index) else {
            return;
        };
        let Some(conn) = tracked.conn.as_ref() else {
            return;
        };
        let mut identifiers: Vec<Vec<u8>> = conn
            .scids()
            .iter()
            .map(|c| c.as_bytes().to_vec())
            .collect();
        identifiers.push(conn.scid().as_bytes().to_vec());
        for id in identifiers {
            self.routes.insert(id, index);
        }
    }

    /// Finds the connection a datagram is addressed to.
    ///
    /// By connection identifier, never by source address: QUIC connections survive an
    /// address change, and a table keyed on the address would lose a connection the moment
    /// a NAT rebound. ngtcp2's own server routes the same way.
    pub(crate) fn route(&self, datagram: &[u8]) -> Option<u64> {
        let inspection = crate::accept::inspect(datagram, crate::cid::DEFAULT_LEN).ok()?;
        let dcid = match inspection {
            crate::accept::Inspection::Supported { dcid, .. }
            | crate::accept::Inspection::UnsupportedVersion { dcid, .. }
            | crate::accept::Inspection::ShortHeader { dcid } => dcid,
        };
        self.routes.get(dcid.as_bytes()).copied()
    }

    /// Feeds a datagram to a connection and acts on what its handlers saw.
    pub(crate) fn deliver(&mut self, index: u64, datagram: &[u8]) {
        let now = self.clock.now();
        let Some(tracked) = self.connections.get_mut(&index) else {
            return;
        };
        let Some(conn) = tracked.conn.as_mut() else {
            // Detached: the datagram belongs to whoever holds the connection, and reading it
            // here would be a second reader of state that admits only one. So it is queued
            // for that owner to read on a pass of its own.
            //
            // This copy is forced, and once the attached path stops copying it is the only
            // one left on the receive path. `datagram` borrows the endpoint's reusable
            // receive buffer, whose contents the next `poll_recv` overwrites: the borrow's
            // lifetime ends when this pass does. The owner may not collect until a later
            // pass, so the bytes have to outlive the borrow, which means owning them.
            // Handing the owner a borrow that reached across passes would alias a buffer the
            // endpoint reuses, so it is not an option -- the copy stays.
            let shared = Arc::clone(&tracked.shared);
            shared.deliver_inbound(datagram.to_vec());
            return;
        };
        let outcome = conn.read_pkt(datagram, now);
        let shared = Arc::clone(&tracked.shared);

        match outcome {
            Ok(ReadOutcome::Processed | ReadOutcome::SendRetry | ReadOutcome::DropSilently) => {}
            Ok(ReadOutcome::Draining | ReadOutcome::Closing) => {
                // The peer closed. What it *said* is on the connection, and is the only
                // application-level explanation available.
                if let Some(conn) = tracked.conn.as_ref() {
                    shared.fail_with_close(conn.close_error());
                }
            }
            Err(err) => {
                // A connection the transport has given up on has nothing more to send and
                // nothing more to read, so it is evictable rather than lingering until some
                // other condition happens to catch it.
                tracked.finished = true;
                shared.fail(Error::from(err));
            }
        }
        self.absorb(index);
    }

    /// Drains what a connection's handlers recorded and acts on it.
    pub(crate) fn absorb(&mut self, index: u64) {
        let Some(tracked) = self.connections.get(&index) else {
            return;
        };
        let shared = Arc::clone(&tracked.shared);
        let observed = shared.take_observed();
        if observed.is_empty() {
            return;
        }

        let mut established = false;
        for event in &observed {
            if matches!(event, Observed::HandshakeCompleted) {
                established = true;
            }
        }

        if let Some(tracked) = self.connections.get_mut(&index)
            && let Some(conn) = tracked.conn.as_ref()
        {
            tracked.shared.set_retained(conn.retained_bytes() as u64);
        }

        if established {
            shared.mark_established();
        }

        // A connection asked for by a caller who will drive it is handed over the moment it
        // can carry anything. Until then the endpoint completes the handshake, which is the
        // part worth having written once.
        if shared.wants_detach() && shared.is_established() {
            self.hand_over(index);
        }

        // Put back what the handle still needs to see. Identifier changes never reach here:
        // they go to the routing queue instead, because the endpoint needs them whoever
        // drives the connection.
        {
            let mut inner = shared.lock();
            for event in observed {
                inner.observed.push(event);
            }
        }
        shared.wake_all();
    }

    /// Gives a connection to the caller that asked for it.
    ///
    /// The endpoint keeps the entry, because it still routes datagrams here and still has to
    /// release the identifiers when the caller is done. What it gives up is the protocol
    /// state, which admits exactly one owner.
    pub(crate) fn hand_over(&mut self, index: u64) {
        let Some(tracked) = self.connections.get_mut(&index) else {
            return;
        };
        let Some(conn) = tracked.conn.take() else {
            return;
        };
        // Anything already produced but not yet sent stays the endpoint's to flush; it was
        // written before the hand-over and the peer is owed it either way.
        let shared = Arc::clone(&tracked.shared);
        let remote = tracked.remote;
        // An accepted connection goes to whoever is accepting, under the reserved key; a
        // dialled one goes to the caller who asked for it, under its own. Sharing a key
        // would let an acceptor take a connection this endpoint dialled.
        let key = if shared.detaches_to_acceptor() {
            0
        } else {
            Arc::as_ptr(&shared) as *const u8 as usize
        };
        let Some(timescale) = self.timescale.clone() else {
            // Asked to hand over a connection from an endpoint that cannot share its
            // timescale. Failing the connection says so; handing it over anyway would give
            // the caller one whose timestamps do not line up with its own handshake.
            tracked.conn = Some(conn);
            shared.fail(Error::new(
                ErrorKind::InvalidInput,
                "this endpoint cannot detach connections; build it with build_detachable",
            ));
            return;
        };
        let Some(sleeper) = self.sleeper.clone() else {
            tracked.conn = Some(conn);
            shared.fail(Error::new(
                ErrorKind::InvalidInput,
                "this endpoint cannot detach connections; build it with build_detachable",
            ));
            return;
        };
        let detached = Arc::clone(&self.detached);
        detached.deliver(
            key,
            super::handle::DetachedConnection::new(conn, shared, remote, timescale, sleeper),
        );
    }

    /// Applies a connection's identifier changes to the routing table.
    ///
    /// Runs for every connection, detached or not: routing is the endpoint's job in both
    /// cases, and a connection whose new identifiers were never installed answers on the one
    /// it started with and on nothing else. Since a peer switches to a new identifier at a
    /// time of its choosing, that failure appears as a connection that works and then stops.
    ///
    /// Applied before anything is sent, so an identifier is routable before the packet
    /// announcing it leaves.
    pub(crate) fn apply_routes(&mut self, index: u64) {
        let Some(tracked) = self.connections.get(&index) else {
            return;
        };
        let updates = tracked.shared.take_routes();
        for update in updates {
            match update {
                RouteUpdate::Minted(cid) => {
                    self.routes.insert(cid.as_bytes().to_vec(), index);
                }
                RouteUpdate::Retired(cid) => {
                    self.routes.remove(cid.as_bytes());
                }
            }
        }
    }

    /// Runs whatever a handle asked for on one connection.
    pub(crate) fn apply_commands(&mut self, index: u64, cx: &mut Context<'_>) {
        let Some(tracked) = self.connections.get(&index) else {
            return;
        };
        if tracked.conn.is_none() {
            // Detached: its owner drives it directly and issues no commands here.
            return;
        }
        let shared = Arc::clone(&tracked.shared);
        for command in shared.take_commands() {
            let Some(tracked) = self.connections.get_mut(&index) else {
                return;
            };
            let Some(conn) = tracked.conn.as_mut() else {
                return;
            };
            match command {
                Command::OpenStream { bidi } => {
                    let opened = if bidi {
                        conn.open_bidi_stream()
                    } else {
                        conn.open_uni_stream()
                    };
                    match opened {
                        Ok(id) => {
                            shared.observe(Observed::LocallyOpened(id));
                            shared.wake_all();
                        }
                        // Running out of stream credit is an ordinary condition in a
                        // working connection, not a failure of it: the peer advertised a
                        // limit and this endpoint has reached it. The request waits for the
                        // peer to raise it rather than tearing the connection down, which
                        // is what a caller means by "open a stream".
                        Err(err) if err.kind() == crate::ErrorKind::Blocked => {
                            shared.push(Command::OpenStream { bidi });
                        }
                        Err(err) => shared.fail(Error::from(err)),
                    }
                }
                Command::Write { stream, data, fin } => {
                    self.write_stream(index, stream, &data, fin, cx);
                }
                Command::Reset(stream, code) => {
                    let _ = conn.reset_stream(stream, code);
                }
                Command::StopSending(stream, code) => {
                    let _ = conn.stop_sending(stream, code);
                }
                Command::Close(code, reason) => {
                    self.close_connection(index, code, &reason);
                }
                Command::ExtendCredit { stream, bytes } => {
                    // Credit is returned when the *application* consumes bytes, not when
                    // they are delivered. Returning it on delivery would make the
                    // flow-control window advisory: a peer could stream indefinitely past a
                    // reader that never reads, and the bytes would pile up in this process
                    // until it ran out of memory. Tying it to consumption is what makes the
                    // window an actual bound.
                    //
                    // Both levels, because the connection window is shared across every
                    // stream: extending only the stream level stalls the connection once
                    // enough total bytes have flowed, late and with nothing to explain it.
                    let _ = conn.extend_max_stream_offset(stream, bytes);
                    conn.extend_max_offset(bytes);
                }
            }
        }
    }

    /// Writes stream data, in chunks a single packet can carry.
    ///
    /// Chunked deliberately. The core copies whatever it is offered and holds the copy
    /// until the peer acknowledges it, so offering a whole large payload on every attempt
    /// would recopy the remainder for every datagram produced. One packet's worth per
    /// offer keeps that bounded.
    pub(crate) fn write_stream(
        &mut self,
        index: u64,
        stream: StreamId,
        data: &[u8],
        fin: bool,
        cx: &mut Context<'_>,
    ) {
        let now = self.clock.now();
        // The core writes into the reusable send buffer, taken out for the call the way
        // `flush` takes it and restored on every path out. A write that produces no datagram
        // -- blocked, or a packet that filled with control frames -- then allocates nothing.
        let mut buffer = core::mem::take(&mut self.send_buffer);
        let Some(tracked) = self.connections.get_mut(&index) else {
            self.send_buffer = buffer;
            return;
        };

        // One datagram is in flight at a time per connection, so if the previous one has
        // not been handed to the socket yet, this write waits its turn rather than
        // overwriting it -- which would be a silently dropped packet.
        if tracked.pending.is_some() {
            tracked.shared.push(Command::Write {
                stream,
                data: data.to_vec(),
                fin,
            });
            self.send_buffer = buffer;
            return;
        }

        // Offer at most one packet's worth. The core copies whatever it accepts and holds
        // the copy until the peer acknowledges it, so offering a whole large payload on
        // every attempt would recopy the remainder for every datagram produced.
        let Some(conn) = tracked.conn.as_mut() else {
            self.send_buffer = buffer;
            return;
        };
        let permitted = conn.max_tx_udp_payload_size().max(1);
        let chunk_len = data.len().min(permitted);
        // The end-of-stream flag belongs on the *last* write, never the first: setting it
        // early closes the write side, and the next attempt is refused with
        // `STREAM_SHUT_WR`.
        let last = fin && chunk_len == data.len();

        let outcome = conn.write_stream(&mut buffer, stream, &data[..chunk_len], last, now);

        match outcome {
            Ok(StreamWrite::Datagram { len, accepted }) => {
                // The datagram is written into the reusable buffer. Offer it to the socket
                // *before* the buffer is reused, exactly as `flush` does with a core-produced
                // datagram: a datagram the socket accepts immediately is sent straight from
                // the buffer and never copied. Only one the socket refuses is copied into a
                // buffer of its own as `tracked.pending`, because a later write in the same
                // pass -- or `flush` reusing the buffer afterwards -- would overwrite it.
                // That copy is the single allocation this path can owe, and it is paid only
                // on refusal, not on every produced datagram.
                let held = conn.retained_bytes() as u64;
                let remote = tracked.remote;
                let disposition = self.socket.poll_send(cx, remote, &buffer[..len]);
                if let Some(tracked) = self.connections.get_mut(&index) {
                    match disposition {
                        // Sent. The bytes stay in the reusable buffer to be overwritten by
                        // the next datagram; nothing is allocated.
                        Poll::Ready(Ok(Sent::Complete)) => {}
                        // Refused (`WouldBlock`/`Pending`) or the socket errored: keep the
                        // datagram so a busy socket does not silently lose it. A socket error
                        // is resurfaced by `flush` on the next pass when it retries `pending`.
                        _ => {
                            tracked.pending = Some(buffer[..len].to_vec());
                        }
                    }
                    tracked.shared.set_retained(held);
                    // Whatever was not accepted -- which may be everything, since a packet
                    // can fill with control frames instead -- goes back on the queue.
                    if accepted < data.len() {
                        tracked.shared.push(Command::Write {
                            stream,
                            data: data[accepted..].to_vec(),
                            fin,
                        });
                    }
                }
            }
            Ok(StreamWrite::StreamBlocked
            | StreamWrite::ConnectionBlocked
            | StreamWrite::Blocked
            | StreamWrite::Idle) => {
                // Not an error and not the end. Requeue and try again once credit or the
                // congestion window allows; treating any of these as "finished" is the
                // classic QUIC stall.
                tracked.shared.push(Command::Write {
                    stream,
                    data: data.to_vec(),
                    fin,
                });
            }
            Err(err) => {
                let shared = Arc::clone(&tracked.shared);
                shared.fail(Error::from(err));
            }
        }
        self.send_buffer = buffer;
    }

    /// Writes a connection close and keeps it for the closing period.
    pub(crate) fn close_connection(
        &mut self,
        index: u64,
        code: ApplicationErrorCode,
        reason: &[u8],
    ) {
        let now = self.clock.now();
        let Some(tracked) = self.connections.get_mut(&index) else {
            return;
        };
        let mut datagram = vec![0u8; MAX_DATAGRAM];
        if let Some(conn) = tracked.conn.as_mut()
            && let Ok(len) = conn.write_connection_close(&mut datagram, code, reason, now)
            && len > 0
        {
            datagram.truncate(len);
            // The pending slot may already hold a datagram -- one produced earlier in this
            // same command batch, or one the socket refused. Overwriting it would silently
            // drop a packet, which is the exact thing that slot exists to prevent, so the
            // close queues behind it instead.
            match tracked.pending.take() {
                Some(waiting) => {
                    tracked.pending = Some(waiting);
                    self.outbox.push_back((tracked.remote, datagram));
                }
                None => tracked.pending = Some(datagram),
            }
        }

        // Closing locally ends the connection, and nothing more will be read from it -- so
        // it becomes evictable once its close datagram has gone.
        tracked.finished = true;
        tracked
            .shared
            .fail(Error::new(ErrorKind::LocallyClosed, "closed by this endpoint"));
    }

    /// Produces the next datagram a connection wants to send, if any.
    ///
    /// The core's output goes into `buffer`, which the caller owns and reuses across
    /// connections and passes; an already-owned datagram is returned as itself. The caller
    /// sends from whichever this reports and copies out only what the socket refuses.
    pub(crate) fn next_datagram(&mut self, index: u64, buffer: &mut [u8]) -> Option<Produced> {
        let now = self.clock.now();
        let tracked = self.connections.get_mut(&index)?;

        if let Some(pending) = tracked.pending.take() {
            return Some(Produced::Owned(pending));
        }

        // A detached connection produces its own datagrams; the endpoint only forwards what
        // it has queued.
        if let Some(queued) = tracked.shared.take_outbound() {
            return Some(Produced::Owned(queued));
        }

        let conn = tracked.conn.as_mut()?;
        match conn.write_pkt(buffer, now) {
            Ok(WriteOutcome::Datagram { len }) => Some(Produced::InBuffer(len)),
            Ok(WriteOutcome::Blocked | WriteOutcome::Idle) => None,
            Err(err) => {
                let shared = Arc::clone(&tracked.shared);
                shared.fail(Error::from(err));
                None
            }
        }
    }

    /// Services a connection whose timer has fired.
    pub(crate) fn handle_expiry(&mut self, index: u64) {
        let now = self.clock.now();
        let Some(tracked) = self.connections.get_mut(&index) else {
            return;
        };
        // A detached connection owns its own timer. Firing one here as well would run its
        // loss detection twice on the endpoint's schedule instead of its own.
        let Some(conn) = tracked.conn.as_mut() else {
            return;
        };
        if conn.expiry().is_none_or(|at| at > now) {
            return;
        }
        let shared = Arc::clone(&tracked.shared);
        match conn.handle_expiry(now) {
            Ok(ExpiryOutcome::Handled) => {}
            Ok(ExpiryOutcome::IdleClose) => {
                // ngtcp2 says to drop the connection without writing a close, so there is
                // nothing left to send and it is evictable immediately.
                let close = conn.close_error();
                tracked.finished = true;
                shared.fail_with_close(close);
            }
            Ok(ExpiryOutcome::Terminal) => {
                tracked.finished = true;
                shared.fail(Error::new(
                    ErrorKind::Transport,
                    "the connection reached a terminal state",
                ));
            }
            Err(err) => {
                // A handshake that ran out of time is not a transport failure: nothing
                // refused anything, and a caller may reasonably retry. ngtcp2 reports it
                // only from here, which is why the distinction is drawn at this call site
                // rather than in the general mapping.
                let timed_out = err
                    .native_code()
                    .is_some_and(|code| code.get() == crate::raw::NGTCP2_ERR_HANDSHAKE_TIMEOUT);
                if timed_out {
                    shared.fail(Error::new(
                        ErrorKind::HandshakeTimeout,
                        "the handshake did not complete in time",
                    ));
                } else {
                    shared.fail(Error::from(err));
                }
            }
        }
        self.absorb(index);
    }

    /// The earliest deadline across every connection.
    pub(crate) fn earliest_expiry(&self) -> Option<Timestamp> {
        self.connections
            .values()
            // A finished connection's deadline is in the past and will never move, so
            // including it would pin the timer there and starve every other connection's.
            .filter(|t| !t.finished)
            // A detached connection's deadline belongs to its owner, who arms a timer of
            // their own against it. Including it here would wake this driver for work it
            // cannot do.
            .filter_map(|t| t.conn.as_ref().and_then(|c| c.expiry()))
            .min()
    }

    /// Removes connections that have finished and released their identifiers.
    ///
    /// A connection is finished when it has closed *and* has nothing left to send. The
    /// second half matters: evicting one with a close datagram still queued would drop the
    /// packet that tells the peer why, and the peer would wait out its idle timeout instead.
    ///
    /// `finished` is set on every path that ends a connection without the peer closing it —
    /// a local close, an idle timeout, a terminal transport failure — because
    /// `in_draining_period` covers only the peer-initiated case. Relying on that alone left
    /// every other kind of dead connection in the table for good, which is a leak an
    /// endpoint accumulates for as long as it runs.
    pub(crate) fn evict(&mut self) {
        let mut dead = Vec::new();
        for (index, tracked) in &self.connections {
            let done = match tracked.conn.as_ref() {
                Some(conn) => {
                    tracked.shared.is_closed()
                        && tracked.pending.is_none()
                        && (conn.in_draining_period() || tracked.finished)
                }
                // A detached connection cannot be asked whether it is draining, because the
                // endpoint does not hold it. Its owner says when it is finished instead, and
                // until it does the entry stays -- an endpoint that guessed would either
                // leak entries for its whole life or cut a connection off mid-close.
                None => tracked.shared.is_terminal() && !tracked.shared.has_outbound(),
            };
            if done {
                dead.push(*index);
            }
        }
        for index in dead {
            self.connections.remove(&index);
            // Every identifier that routed here goes with it, or a later datagram would be
            // delivered to a connection that no longer exists.
            self.routes.retain(|_, target| *target != index);
        }
    }

    /// Builds a connection this endpoint initiated.
    pub(crate) fn dial(
        &mut self,
        remote: SocketAddr,
        server_name: Option<&str>,
        shared: Arc<ConnectionShared>,
    ) -> Result<u64, Error> {
        let local = self
            .socket
            .local_addr()
            .map_err(|err| Error::new(ErrorKind::Socket, "the socket has no address").with_source(SocketError(err.to_string())))?;

        let session = self
            .backend
            .new_session(Role::Client, server_name)
            .map_err(Error::from)?;

        let now = self.clock.now();
        let conn = crate::conn::ConnBuilder::new(
            Role::Client,
            self.config.settings(now),
            self.config.transport_params(),
            (self.entropy)(),
            session,
            local,
            remote,
        )
        .build(handlers_for(&shared))
        .map_err(Error::from)?;

        let index = self.next_index;
        self.next_index += 1;
        self.connections.insert(
            index,
            Tracked {
                conn: Some(conn),
                shared,
                remote,
                pending: None,
                finished: false,
            },
        );
        self.install_routes(index);
        Ok(index)
    }

    /// Builds a connection a peer initiated.
    pub(crate) fn accept(
        &mut self,
        remote: SocketAddr,
        packet: &crate::accept::InitialPacket,
        original_dcid: &crate::cid::ConnectionId,
        retried: bool,
        shared: Arc<ConnectionShared>,
    ) -> Result<u64, Error> {
        let local = self
            .socket
            .local_addr()
            .map_err(|err| Error::new(ErrorKind::Socket, "the socket has no address").with_source(SocketError(err.to_string())))?;

        let session = self
            .backend
            .new_session(Role::Server, None)
            .map_err(Error::from)?;

        let now = self.clock.now();

        // A server cannot be built without the identifier the client first addressed:
        // ngtcp2 asserts on it, and the assertion is compiled out of release builds.
        let mut params = self.config.transport_params().original_dcid(original_dcid);
        let mut settings = self.config.settings(now);
        if retried {
            // Both halves are required after a Retry and neither errors when omitted. The
            // client verifies that the identifier it was told to address really came from a
            // Retry this server sent, so without `retry_scid` the handshake never
            // completes -- indistinguishable from an unreachable server.
            params = params.retry_scid(&packet.dcid);
            settings = settings.validated_token(packet.token.bytes(), crate::TokenKind::Retry);
        }

        let conn = crate::conn::ConnBuilder::new(
            Role::Server,
            settings,
            params,
            (self.entropy)(),
            session,
            local,
            remote,
        )
        .dcid(packet.scid)
        .build(handlers_for(&shared))
        .map_err(Error::from)?;

        let index = self.next_index;
        self.next_index += 1;
        self.connections.insert(
            index,
            Tracked {
                conn: Some(conn),
                shared,
                remote,
                pending: None,
                finished: false,
            },
        );
        self.install_routes(index);
        // The identifier the client addressed also routes here until the handshake gives
        // the connection its own: the client's next packet still carries it.
        self.routes.insert(packet.dcid.as_bytes().to_vec(), index);
        Ok(index)
    }

    /// Arms the sleep for the earliest deadline, if there is one.
    ///
    /// Returns [`Poll::Ready`] when a deadline has been reached, which is the driver's
    /// signal to service timers. Rearming after *every* pass — including a pass that only
    /// wrote — is what keeps a paced connection sending: ngtcp2 reports its pacing deadline
    /// through the same `expiry()` as everything else, so a driver that rearms only after
    /// reading sends one datagram and then sleeps until the peer speaks.
    pub(crate) fn rearm(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let Some(deadline) = self.earliest_expiry() else {
            self.sleeping = None;
            self.sleeping_until = None;
            return Poll::Pending;
        };

        if self.sleeping_until != Some(deadline) {
            self.sleeping = Some(self.clock.sleep_until(deadline));
            self.sleeping_until = Some(deadline);
        }

        let Some(sleep) = self.sleeping.as_mut() else {
            return Poll::Pending;
        };
        // `Clock::Sleep` is `Unpin`, which is what lets this be written without `unsafe` --
        // the reason that bound is on the trait.
        match core::pin::Pin::new(sleep).poll(cx) {
            Poll::Ready(()) => {
                self.sleeping = None;
                self.sleeping_until = None;
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }

    /// Sends whatever every connection has to send.
    pub(crate) fn flush(&mut self, cx: &mut Context<'_>) -> Result<(), Error> {
        // Connectionless answers first. They are small, they are replies to something that
        // already arrived, and holding them behind a bulk transfer would make a Retry
        // arrive after the client had given up.
        while let Some((destination, datagram)) = self.outbox.pop_front() {
            match self.socket.poll_send(cx, destination, &datagram) {
                Poll::Ready(Ok(Sent::Complete)) => {}
                Poll::Ready(Ok(Sent::WouldBlock)) | Poll::Pending => {
                    self.outbox.push_front((destination, datagram));
                    break;
                }
                Poll::Ready(Err(err)) => {
                    return Err(Error::new(ErrorKind::Socket, "the socket failed")
                        .with_source(SocketError(err.to_string())));
                }
            }
        }

        let mut indices = core::mem::take(&mut self.index_scratch);
        indices.clear();
        indices.extend(self.connections.keys().copied());
        // The send buffer comes out too, for the same reason the index scratch does: the
        // core writes each produced datagram into it, and it cannot be borrowed from `self`
        // across a body that re-borrows `self` to send. Taken here, lent to `next_datagram`,
        // and put back on every path out -- including the error return -- so it is never
        // leaked.
        let mut buffer = core::mem::take(&mut self.send_buffer);
        // Indexed rather than iterated so the scratch is not borrowed across the body, which
        // lets the error path below move it back before returning. The body re-borrows
        // `connections`, so holding a borrow of it here would not compile anyway.
        for i in 0..indices.len() {
            let index = indices[i];
            while let Some(produced) = self.next_datagram(index, &mut buffer) {
                let Some(tracked) = self.connections.get(&index) else {
                    break;
                };
                let remote = tracked.remote;
                let bytes = match &produced {
                    Produced::InBuffer(len) => &buffer[..*len],
                    Produced::Owned(datagram) => &datagram[..],
                };
                match self.socket.poll_send(cx, remote, bytes) {
                    // Sent. A datagram written into the reusable buffer is simply left there
                    // to be overwritten by the next one, so a completed send allocates
                    // nothing; an owned one is dropped here.
                    Poll::Ready(Ok(Sent::Complete)) => {}
                    Poll::Ready(Ok(Sent::WouldBlock)) | Poll::Pending => {
                        // Not sent. Keeping it is what stops a busy socket from silently
                        // losing packets, which QUIC would recover from slowly enough to
                        // look like a hang. This is the one allocation the send path can
                        // owe: a datagram still in the reusable buffer must be copied into
                        // one of its own before the next pass overwrites the buffer. A
                        // datagram that was already owned is retained as itself, for free.
                        if let Some(tracked) = self.connections.get_mut(&index) {
                            tracked.pending = Some(match produced {
                                Produced::InBuffer(len) => buffer[..len].to_vec(),
                                Produced::Owned(datagram) => datagram,
                            });
                        }
                        break;
                    }
                    Poll::Ready(Err(err)) => {
                        // The scratch and the send buffer are restored on this path too, not
                        // just the normal one: both are reused across passes, so an error
                        // must not leak either.
                        indices.clear();
                        self.index_scratch = indices;
                        self.send_buffer = buffer;
                        return Err(Error::new(ErrorKind::Socket, "the socket failed")
                            .with_source(SocketError(err.to_string())));
                    }
                }
            }
        }
        indices.clear();
        self.index_scratch = indices;
        self.send_buffer = buffer;
        Ok(())
    }

    /// Fails every connection, which is what a dead socket or a dropped driver means.
    pub(crate) fn fail_all(&mut self, make: impl Fn() -> Error) {
        for tracked in self.connections.values() {
            tracked.shared.fail(make());
        }
    }

    /// Wakes every connection's handle.
    pub(crate) fn wake_all(&self) {
        for tracked in self.connections.values() {
            tracked.shared.wake_all();
        }
    }

    /// Whether anything is waiting to be sent.
    pub(crate) fn has_pending(&self) -> bool {
        !self.outbox.is_empty() || self.connections.values().any(|t| t.pending.is_some())
    }


}

/// A socket failure, rendered as text so it can cross the error boundary.
///
/// [`AsyncUdpSocket::Error`] is only `Debug + Display`, deliberately — requiring
/// `std::error::Error` would exclude error types that do not implement it, for no benefit
/// beyond a nicer chain. Rendering it here keeps what it said.
#[derive(Debug)]
pub(crate) struct SocketError(pub(crate) String);

impl core::fmt::Display for SocketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::error::Error for SocketError {}
