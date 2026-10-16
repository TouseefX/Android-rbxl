//! Join-channel handling for the RbxTransport Team Create session.
//!
//! The app=4 flow speaks two kinds of channel: reliable ones, which ride one
//! QUIC stream each behind a 7-byte `[06 01 app][channel be-u32]` stream
//! header, and unreliable ones, which are announced to the peer with a wire id
//! and ride datagrams. Before any of that, both sides open their *send*
//! channels: the target walks a fourteen-row profile and calls
//! `openSendChannel` once per row (Studio Beta analysis: `FUN_144c25af0`
//! returns the per-row `{mode, auxiliary, flag}` triple and the setup loop
//! consumes it in profile order; `FUN_143816d50` / `FUN_143816e10` write the
//! two control bodies).
//!
//! This module owns that plane: the profile, the local unreliable-id allocator
//! with its parity rule, the two control record shapes in both directions, and
//! a [`ChannelRouter`] that reconciles what we opened with what the peer
//! announced. It is pure logic — it never touches a socket, and it does not
//! claim that any of these values satisfy a server.
//!
//! Provenance and limits: the fourteen rows, the `-2,-4,…` / `-3,-5,…` local id
//! series, the positive wire magnitude, and the opposite-parity check come from
//! the static reconstruction in `dump/` (`rbx_channel_protocol.hpp`). The
//! reliable body is `type, application, channel` (6 bytes, length-prefixed by
//! the value 6) and the unreliable body is `type, application, channel,
//! magnitude` (10 bytes); both dwords are big-endian, which is what the
//! observed RbxTransport control path writes and what the reference client
//! emits. The higher-level meaning of `auxiliary` and `flag` is *not*
//! established, so nothing here interprets them.

use std::collections::BTreeMap;

/// Rows in the profile the target's channel-setup loop walks.
pub const CHANNEL_PROFILE_COUNT: usize = 14;
/// Control record type for a reliable channel open (`FUN_143816d50`).
pub const RELIABLE_OPEN_TYPE: u8 = 1;
/// Control record type for an unreliable channel open (`FUN_143816e10`).
pub const UNRELIABLE_OPEN_TYPE: u8 = 2;
/// Control record type for closing an unreliable channel, observed next to the
/// two open types in the same switch.
pub const CLOSE_UNRELIABLE_TYPE: u8 = 3;
/// Body length of a reliable open record. The target writes this value as the
/// record's length byte before the body.
pub const RELIABLE_OPEN_BODY_BYTES: usize = 6;
/// Body length of an unreliable open record.
pub const UNRELIABLE_OPEN_BODY_BYTES: usize = 10;

/// The profile value that means "unreliable". No other mode is identified.
const UNRELIABLE_MODE: u32 = 1;

/// One row of the statically recovered channel profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelProfileRow {
    pub mode: u32,
    /// Second field of the target's per-channel switch. Deliberately neutrally
    /// named: its meaning is not established by the analysis.
    pub auxiliary: u32,
    pub flag: u8,
}

/// The fourteen rows recovered from the target, in profile order.
pub const DEFAULT_CHANNEL_PROFILE: [ChannelProfileRow; CHANNEL_PROFILE_COUNT] = [
    ChannelProfileRow { mode: 0, auxiliary: 3, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 3, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 3, flag: 0 },
    ChannelProfileRow { mode: 1, auxiliary: 3, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 3, flag: 1 },
    ChannelProfileRow { mode: 1, auxiliary: 2, flag: 0 },
    ChannelProfileRow { mode: 1, auxiliary: 2, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 3, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 3, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 4, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 2, flag: 0 },
    ChannelProfileRow { mode: 1, auxiliary: 2, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 3, flag: 0 },
    ChannelProfileRow { mode: 0, auxiliary: 3, flag: 0 },
];

/// Whether a profile row is an unreliable channel.
pub fn is_unreliable(row: &ChannelProfileRow) -> bool {
    row.mode == UNRELIABLE_MODE
}

/// The profile indices marked unreliable (3, 5, 6 and 11).
pub fn unreliable_channels(profile: &[ChannelProfileRow]) -> Vec<usize> {
    profile
        .iter()
        .enumerate()
        .filter(|(_, row)| is_unreliable(row))
        .map(|(index, _)| index)
        .collect()
}

/// Which channel-control records a session stages on its control stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelOpenMode {
    /// Only the profile's unreliable rows, which is what the observed reference
    /// burst sends (`openU c3/c5/c6/c11` with wire ids 2/4/6/8).
    ReferenceUnreliable,
    /// Every profile row, in profile order: an OpenReliable record per reliable
    /// row plus an OpenUnreliable per unreliable row. This is the target's
    /// `openSendChannel` loop shape; it is a stronger claim on the wire than
    /// the reference burst, so it stays opt-in.
    FullProfile,
    /// Send nothing. Channel state is still tracked, so a run with no opens
    /// still reports which channels the peer announced.
    Off,
}

/// How this client stages its control records. Defaults to the reference
/// shape so enabling the module never silently changes the bytes on the wire.
static CHANNEL_OPEN_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Select the channel-open mode for subsequent flows.
pub fn set_channel_open_mode(mode: ChannelOpenMode) {
    CHANNEL_OPEN_MODE.store(mode as u8, std::sync::atomic::Ordering::Relaxed);
}

/// The configured channel-open mode.
pub fn channel_open_mode() -> ChannelOpenMode {
    match CHANNEL_OPEN_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => ChannelOpenMode::FullProfile,
        2 => ChannelOpenMode::Off,
        _ => ChannelOpenMode::ReferenceUnreliable,
    }
}

/// A one-line description of a mode, for the UI and the session report.
pub fn channel_open_mode_label(mode: ChannelOpenMode) -> &'static str {
    match mode {
        ChannelOpenMode::ReferenceUnreliable => "reference burst (unreliable rows only)",
        ChannelOpenMode::FullProfile => "full profile (14 openSendChannel records)",
        ChannelOpenMode::Off => "no channel opens",
    }
}

/// Local unreliable channel ids.
///
/// The target stores a *negative* internal id per unreliable channel and puts
/// its positive magnitude on the wire. Parity 0 produces `-2,-4,-6,…`; parity 1
/// produces `-3,-5,-7,…`. A peer id must have the opposite parity, which is how
/// each side tells its own allocations from the peer's.
pub struct LocalUnreliableIdAllocator {
    parity: u8,
    allocations: u64,
}

impl LocalUnreliableIdAllocator {
    pub fn new(parity: u8) -> Self {
        Self { parity: parity & 1, allocations: 0 }
    }

    pub fn parity(&self) -> u8 {
        self.parity
    }

    pub fn allocations(&self) -> u64 {
        self.allocations
    }

    /// Allocate the next internal id, or `None` when the id space is exhausted.
    pub fn allocate(&mut self) -> Option<i32> {
        let max_ordinal = (i32::MAX as u64 - u64::from(self.parity)) / 2;
        if self.allocations >= max_ordinal {
            return None;
        }
        self.allocations += 1;
        let magnitude: i32 = (2 * self.allocations | u64::from(self.parity)).try_into().ok()?;
        Some(-magnitude)
    }
}

/// The positive magnitude the wire carries for a negative internal id.
pub fn wire_magnitude(internal_id: i32) -> Option<u32> {
    if internal_id >= 0 || internal_id == i32::MIN {
        return None;
    }
    Some((-internal_id) as u32)
}

/// Whether a negative id could be a *peer* id given our local parity.
pub fn is_valid_peer_internal_id(internal_id: i32, local_parity: u8) -> bool {
    let Some(magnitude) = wire_magnitude(internal_id) else {
        return false;
    };
    magnitude != 0 && (magnitude & 1) != u32::from(local_parity & 1)
}

/// Turn a peer's advertised magnitude back into its internal id, rejecting the
/// ones that carry our own parity.
pub fn decode_peer_wire_id(magnitude: u32, local_parity: u8) -> Option<i32> {
    if magnitude == 0 || magnitude > i32::MAX as u32 {
        return None;
    }
    let internal_id = -(magnitude as i32);
    is_valid_peer_internal_id(internal_id, local_parity).then_some(internal_id)
}

/// `type, application, channel` — the reliable open body (`FUN_143816d50`).
pub fn reliable_open_body(application: u8, channel: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(RELIABLE_OPEN_BODY_BYTES);
    out.push(RELIABLE_OPEN_TYPE);
    out.push(application);
    out.extend_from_slice(&channel.to_be_bytes());
    out
}

/// The reliable open body with the length byte the target writes in front of
/// it. `appflow::frame` produces the same prefix for a 6-byte body, which is
/// what the live session sends.
#[allow(dead_code)]
pub fn reliable_open_record(application: u8, channel: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(RELIABLE_OPEN_BODY_BYTES + 1);
    out.push(RELIABLE_OPEN_BODY_BYTES as u8);
    out.extend_from_slice(&reliable_open_body(application, channel));
    out
}

/// `type, application, channel, |internal id|` — the unreliable open body.
pub fn unreliable_open_body(
    application: u8,
    channel: u32,
    internal_id: i32,
) -> Option<Vec<u8>> {
    let magnitude = wire_magnitude(internal_id)?;
    let mut out = Vec::with_capacity(UNRELIABLE_OPEN_BODY_BYTES);
    out.push(UNRELIABLE_OPEN_TYPE);
    out.push(application);
    out.extend_from_slice(&channel.to_be_bytes());
    out.extend_from_slice(&magnitude.to_be_bytes());
    Some(out)
}

/// Close an unreliable channel. Same shape as the reliable open body with a
/// different type byte. Kept because a session that opens a channel it cannot
/// service needs to say so; nothing on the current flow path sends one.
#[allow(dead_code)]
pub fn close_unreliable_body(application: u8, channel: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(RELIABLE_OPEN_BODY_BYTES);
    out.push(CLOSE_UNRELIABLE_TYPE);
    out.push(application);
    out.extend_from_slice(&channel.to_be_bytes());
    out
}

/// A peer's unreliable open record, decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerUnreliableOpen {
    pub application: u8,
    pub channel: u32,
    pub magnitude: u32,
    /// The peer's internal id (`-magnitude`), or `None` when the magnitude
    /// carries our local parity — the shape the target rejects.
    pub internal_id: Option<i32>,
}

/// Parse a 10-byte unreliable open body. Returns `None` for anything that is
/// not that record, so a caller can fall through to another handler.
pub fn parse_unreliable_open(body: &[u8]) -> Option<PeerUnreliableOpen> {
    if body.len() != UNRELIABLE_OPEN_BODY_BYTES || body[0] != UNRELIABLE_OPEN_TYPE {
        return None;
    }
    let channel = u32::from_be_bytes([body[2], body[3], body[4], body[5]]);
    let magnitude = u32::from_be_bytes([body[6], body[7], body[8], body[9]]);
    Some(PeerUnreliableOpen {
        application: body[1],
        channel,
        magnitude,
        internal_id: (magnitude != 0 && magnitude <= i32::MAX as u32)
            .then(|| -(magnitude as i32)),
    })
}

/// Parse a 6-byte reliable open body into `(application, channel)`.
pub fn parse_reliable_open(body: &[u8]) -> Option<(u8, u32)> {
    if body.len() != RELIABLE_OPEN_BODY_BYTES || body[0] != RELIABLE_OPEN_TYPE {
        return None;
    }
    Some((
        body[1],
        u32::from_be_bytes([body[2], body[3], body[4], body[5]]),
    ))
}

/// Everything the router knows about one channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinChannel {
    pub application: u32,
    pub channel: u32,
    pub unreliable: bool,
    /// Our internal id for this channel's send side (unreliable only).
    pub local_id: Option<i32>,
    /// The peer's internal id, as recovered from its announced magnitude.
    pub peer_id: Option<i32>,
    /// True once the peer announced this channel on a control stream.
    pub peer_announced: bool,
    /// True once we staged our own open record for it.
    pub self_opened: bool,
    /// The QUIC stream carrying this channel, once one is assigned.
    pub stream: Option<i64>,
    pub frames: u64,
    pub bytes: u64,
}

/// Reconciles this client's channel opens with the peer's announcements and
/// keeps the per-channel byte accounting the session report shows.
pub struct ChannelRouter {
    application: u32,
    local_parity: u8,
    allocator: LocalUnreliableIdAllocator,
    channels: BTreeMap<u32, JoinChannel>,
    by_stream: BTreeMap<i64, u32>,
    /// Announcements for applications this router does not own, as
    /// `(application, channel, wire magnitude)`.
    foreign: Vec<(u32, u32, Option<u32>)>,
    notes: Vec<String>,
    staged_mode: Option<ChannelOpenMode>,
}

impl ChannelRouter {
    /// Build a router for `application`, pre-seeded with the profile rows so an
    /// announcement for a channel the profile does not list is still visible.
    pub fn new(application: u32, profile: &[ChannelProfileRow]) -> Self {
        let local_parity: u8 = 0;
        let mut channels = BTreeMap::new();
        for (index, row) in profile.iter().enumerate() {
            let channel = index as u32;
            channels.insert(
                channel,
                JoinChannel {
                    application,
                    channel,
                    unreliable: is_unreliable(row),
                    local_id: None,
                    peer_id: None,
                    peer_announced: false,
                    self_opened: false,
                    stream: None,
                    frames: 0,
                    bytes: 0,
                },
            );
        }
        Self {
            application,
            local_parity,
            allocator: LocalUnreliableIdAllocator::new(local_parity),
            channels,
            by_stream: BTreeMap::new(),
            foreign: Vec::new(),
            notes: Vec::new(),
            staged_mode: None,
        }
    }

    #[allow(dead_code)]
    pub fn application(&self) -> u32 {
        self.application
    }

    #[allow(dead_code)]
    pub fn local_parity(&self) -> u8 {
        self.local_parity
    }

    /// Change the parity used for our own unreliable ids. The target picks the
    /// series from the channel object, so this is a client-side experiment
    /// switch, not a claim about what the peer expects.
    pub fn set_local_parity(&mut self, parity: u8) {
        self.local_parity = parity & 1;
        self.allocator = LocalUnreliableIdAllocator::new(self.local_parity);
    }

    /// The channel-control bodies to stage on the control stream, in profile
    /// order, framed by the caller. Unreliable rows consume the local id
    /// allocator, so calling this twice stages a second, different series.
    pub fn stage_opens(&mut self, mode: ChannelOpenMode, application: u8) -> Vec<Vec<u8>> {
        self.staged_mode = Some(mode);
        let mut bodies = Vec::new();
        if mode == ChannelOpenMode::Off {
            self.notes.push(format!(
                "channel opens disabled ({})",
                channel_open_mode_label(mode)
            ));
            return bodies;
        }
        for (index, row) in DEFAULT_CHANNEL_PROFILE.iter().enumerate() {
            let channel = index as u32;
            let unreliable = is_unreliable(row);
            if mode == ChannelOpenMode::ReferenceUnreliable && !unreliable {
                continue;
            }
            let app_id = self.application;
            let entry = self.channels.entry(channel).or_insert_with(|| JoinChannel {
                application: app_id,
                channel,
                unreliable,
                local_id: None,
                peer_id: None,
                peer_announced: false,
                self_opened: false,
                stream: None,
                frames: 0,
                bytes: 0,
            });
            entry.self_opened = true;
            if unreliable {
                let Some(id) = self.allocator.allocate() else {
                    self.notes
                        .push(format!("channel {channel}: local unreliable id space exhausted"));
                    continue;
                };
                entry.local_id = Some(id);
                match unreliable_open_body(application, channel, id) {
                    Some(body) => bodies.push(body),
                    None => self.notes.push(format!(
                        "channel {channel}: unreliable open body not buildable from id {id}"
                    )),
                }
            } else {
                bodies.push(reliable_open_body(application, channel));
            }
        }
        self.notes.push(format!(
            "staged {} channel open record(s) — {}",
            bodies.len(),
            channel_open_mode_label(mode)
        ));
        bodies
    }

    /// The mode this router staged opens with, if it staged any.
    pub fn staged_mode(&self) -> Option<ChannelOpenMode> {
        self.staged_mode
    }

    /// The mode the router used, defaulting to the configured one for a flow
    /// that never staged opens.
    pub fn effective_mode(&self) -> ChannelOpenMode {
        self.staged_mode.unwrap_or_else(channel_open_mode)
    }

    /// Record a channel announcement from a peer control record. `wire` is the
    /// magnitude for an unreliable open and `None` for a reliable one. Returns
    /// the log line describing the result, if there is anything to say.
    pub fn note_peer_open(
        &mut self,
        application: u32,
        channel: u32,
        wire: Option<u32>,
    ) -> Option<String> {
        if application != self.application {
            self.foreign.push((application, channel, wire));
            return Some(format!(
                "peer opened app={application} chan={channel}{} (not this session's application {})",
                match wire {
                    Some(magnitude) => format!(" wire={magnitude}"),
                    None => " reliable".to_string(),
                },
                self.application
            ));
        }
        let local_parity = self.local_parity;
        let entry = self.channels.entry(channel).or_insert_with(|| JoinChannel {
            application,
            channel,
            unreliable: wire.is_some(),
            local_id: None,
            peer_id: None,
            peer_announced: false,
            self_opened: false,
            stream: None,
            frames: 0,
            bytes: 0,
        });
        entry.peer_announced = true;
        entry.unreliable |= wire.is_some();
        match wire {
            None => Some(format!("peer announced chan {channel} reliable (app={application})")),
            Some(magnitude) => {
                match decode_peer_wire_id(magnitude, local_parity) {
                    Some(id) => {
                        entry.peer_id = Some(id);
                        Some(format!(
                            "peer announced chan {channel} unreliable wire={magnitude} (internal id {id})"
                        ))
                    }
                    None => Some(format!(
                        "peer announced chan {channel} with wire={magnitude}, which carries our local parity {}; treated as a local id and not routed",
                        local_parity
                    )),
                }
            }
        }
    }

    /// Bind a QUIC stream to a channel so received bytes account to it.
    pub fn note_stream(&mut self, channel: u32, stream: i64) {
        self.by_stream.insert(stream, channel);
        if let Some(entry) = self.channels.get_mut(&channel) {
            entry.stream = Some(stream);
        }
    }

    /// Bind a stream to this router's channel 1, the join message channel.
    pub fn note_join_stream(&mut self, stream: i64) {
        self.note_stream(1, stream);
    }

    /// The channel a stream belongs to, once bound.
    pub fn channel_for_stream(&self, stream: i64) -> Option<u32> {
        self.by_stream.get(&stream).copied()
    }

    /// Account received bytes for a channel, by index.
    pub fn note_bytes(&mut self, channel: u32, bytes: usize, frames: usize) {
        let application = self.application;
        let entry = self.channels.entry(channel).or_insert_with(|| JoinChannel {
            application,
            channel,
            unreliable: false,
            local_id: None,
            peer_id: None,
            peer_announced: false,
            self_opened: false,
            stream: None,
            frames: 0,
            bytes: 0,
        });
        entry.bytes += bytes as u64;
        entry.frames += frames as u64;
    }

    /// Account received bytes for a stream, resolving its channel first.
    pub fn note_stream_bytes(&mut self, stream: i64, bytes: usize, frames: usize) {
        if let Some(channel) = self.channel_for_stream(stream) {
            self.note_bytes(channel, bytes, frames);
        }
    }

    pub fn channels(&self) -> &BTreeMap<u32, JoinChannel> {
        &self.channels
    }

    pub fn notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Channels the profile or the peer know about that we never opened, and
    /// the reverse. These are the gaps the session report is supposed to show
    /// rather than let a partial channel setup read as a complete one.
    pub fn gaps(&self) -> Vec<String> {
        let mut gaps = Vec::new();
        let mut announced_unopened = Vec::new();
        let mut opened_unannounced = Vec::new();
        let mut unrouted = Vec::new();
        for (channel, entry) in &self.channels {
            if entry.peer_announced && !entry.self_opened {
                announced_unopened.push(channel.to_string());
            }
            if entry.self_opened && !entry.peer_announced {
                opened_unannounced.push(channel.to_string());
            }
            if entry.bytes == 0 && entry.channel != 0 {
                unrouted.push(channel.to_string());
            }
        }
        if !announced_unopened.is_empty() {
            gaps.push(format!(
                "announced by the peer but never opened here: [{}]",
                announced_unopened.join(", ")
            ));
        }
        if !opened_unannounced.is_empty() {
            gaps.push(format!(
                "opened here with no matching peer announcement: [{}]",
                opened_unannounced.join(", ")
            ));
        }
        if !unrouted.is_empty() {
            gaps.push(format!(
                "no bytes routed: [{}]",
                unrouted.join(", ")
            ));
        }
        if !self.foreign.is_empty() {
            let foreign: Vec<String> = self
                .foreign
                .iter()
                .map(|(app, channel, wire)| match wire {
                    Some(magnitude) => format!("{app}/{channel}@{magnitude}"),
                    None => format!("{app}/{channel}"),
                })
                .collect();
            gaps.push(format!(
                "announcements for other applications (never routed): [{}]",
                foreign.join(", ")
            ));
        }
        gaps
    }

    /// One-line channel accounting for the session report.
    pub fn summary(&self) -> String {
        let unreliable = self.channels.values().filter(|entry| entry.unreliable).count();
        let announced = self.channels.values().filter(|entry| entry.peer_announced).count();
        let opened = self.channels.values().filter(|entry| entry.self_opened).count();
        let routed = self.channels.values().filter(|entry| entry.bytes > 0).count();
        let bytes: u64 = self.channels.values().map(|entry| entry.bytes).sum();
        let frames: u64 = self.channels.values().map(|entry| entry.frames).sum();
        let ids: Vec<String> = self
            .channels
            .values()
            .filter_map(|entry| {
                entry
                    .local_id
                    .map(|id| format!("chan{}=>{}", entry.channel, id))
            })
            .collect();
        format!(
            "{} profile rows ({} unreliable); staged {opened}, peer-announced {announced}, routed {routed} ({} frame(s), {bytes}B); local ids [{}]",
            self.channels.len(),
            unreliable,
            frames,
            ids.join(" ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_marks_the_four_known_unreliable_channels() {
        assert_eq!(unreliable_channels(&DEFAULT_CHANNEL_PROFILE), vec![3, 5, 6, 11]);
        assert_eq!(DEFAULT_CHANNEL_PROFILE.len(), CHANNEL_PROFILE_COUNT);
    }

    /// The point where the recovered profile and the reference burst meet: the
    /// parity-0 series turns channels 3/5/6/11 into exactly wire ids 2/4/6/8,
    /// which is what the observed native client sends. If this test ever fails,
    /// either the profile or the burst has changed and both need re-reading.
    #[test]
    fn profile_reproduces_the_reference_unreliable_burst() {
        let mut router = ChannelRouter::new(crate::appflow::APP, &DEFAULT_CHANNEL_PROFILE);
        let bodies = router.stage_opens(ChannelOpenMode::ReferenceUnreliable, 4);
        let expected = [(3u32, 2u32), (5, 4), (6, 6), (11, 8)];
        assert_eq!(bodies.len(), expected.len());
        for (body, (channel, wire)) in bodies.iter().zip(expected.iter()) {
            assert_eq!(body.len(), UNRELIABLE_OPEN_BODY_BYTES);
            assert_eq!(body[0], UNRELIABLE_OPEN_TYPE);
            assert_eq!(body[1], 4);
            assert_eq!(&body[2..6], &channel.to_be_bytes());
            assert_eq!(&body[6..10], &wire.to_be_bytes());
            // The same bytes as the flow's own builder, so enabling the router
            // cannot silently restyle the burst.
            assert_eq!(
                body,
                &crate::appflow::build_open_unreliable(4, *channel, *wire)
            );
        }
        // And those wire ids are the peer-visible form of internal ids -2..-8.
        let ids: Vec<i32> = router
            .channels()
            .values()
            .filter_map(|entry| entry.local_id)
            .collect();
        assert_eq!(ids, vec![-2, -4, -6, -8]);
    }

    #[test]
    fn full_profile_adds_the_reliable_rows() {
        let mut router = ChannelRouter::new(crate::appflow::APP, &DEFAULT_CHANNEL_PROFILE);
        let bodies = router.stage_opens(ChannelOpenMode::FullProfile, 4);
        assert_eq!(bodies.len(), CHANNEL_PROFILE_COUNT);
        // Reliable rows are 6 bytes and their framed form is the target's
        // 7-byte record, where the leading length byte is the body size.
        let reliable = &bodies[0];
        assert_eq!(reliable.len(), RELIABLE_OPEN_BODY_BYTES);
        assert_eq!(reliable[0], RELIABLE_OPEN_TYPE);
        assert_eq!(
            &reliable_open_record(4, 0)[1..],
            reliable.as_slice()
        );
        assert_eq!(reliable_open_record(4, 0)[0], RELIABLE_OPEN_BODY_BYTES as u8);
        // Unreliable rows keep their profile order in the wire ids.
        assert_eq!(&bodies[3][6..10], &2u32.to_be_bytes());
        assert_eq!(&bodies[11][6..10], &8u32.to_be_bytes());
    }

    #[test]
    fn off_mode_still_tracks_the_profile() {
        let mut router = ChannelRouter::new(crate::appflow::APP, &DEFAULT_CHANNEL_PROFILE);
        assert!(router.stage_opens(ChannelOpenMode::Off, 4).is_empty());
        assert_eq!(router.channels().len(), CHANNEL_PROFILE_COUNT);
        assert!(router.gaps().iter().any(|gap| gap.starts_with("no bytes routed")));
    }

    #[test]
    fn parity_selects_the_id_series() {
        let mut even = LocalUnreliableIdAllocator::new(0);
        let mut odd = LocalUnreliableIdAllocator::new(1);
        assert_eq!(
            (0..4).map(|_| even.allocate().unwrap()).collect::<Vec<i32>>(),
            vec![-2, -4, -6, -8]
        );
        assert_eq!(
            (0..4).map(|_| odd.allocate().unwrap()).collect::<Vec<i32>>(),
            vec![-3, -5, -7, -9]
        );
        assert_eq!(even.allocations(), 4);
    }

    #[test]
    fn wire_magnitude_and_peer_ids() {
        assert_eq!(wire_magnitude(-2), Some(2));
        assert_eq!(wire_magnitude(i32::MIN), None);
        assert_eq!(wire_magnitude(0), None);
        assert_eq!(wire_magnitude(5), None);
        assert!(is_valid_peer_internal_id(-3, 0));
        assert!(!is_valid_peer_internal_id(-2, 0));
        assert_eq!(decode_peer_wire_id(3, 0), Some(-3));
        assert_eq!(decode_peer_wire_id(2, 0), None);
        assert_eq!(decode_peer_wire_id(0, 0), None);
        assert_eq!(decode_peer_wire_id(i32::MAX as u32 + 1, 0), None);
    }

    #[test]
    fn unreliable_open_round_trips() {
        let body = unreliable_open_body(4, 11, -8).unwrap();
        let parsed = parse_unreliable_open(&body).unwrap();
        assert_eq!(parsed.application, 4);
        assert_eq!(parsed.channel, 11);
        assert_eq!(parsed.magnitude, 8);
        assert_eq!(parsed.internal_id, Some(-8));
        // A reliable body never parses as one, and vice versa.
        assert!(parse_unreliable_open(&reliable_open_body(4, 3)).is_none());
        assert_eq!(parse_reliable_open(&reliable_open_body(4, 3)), Some((4, 3)));
        assert!(parse_reliable_open(&body).is_none());
        assert!(parse_unreliable_open(&body[..9]).is_none());
    }

    #[test]
    fn close_record_keeps_the_channel_fields() {
        let body = close_unreliable_body(4, 5);
        assert_eq!(body.len(), RELIABLE_OPEN_BODY_BYTES);
        assert_eq!(body[0], CLOSE_UNRELIABLE_TYPE);
        assert_eq!(&body[2..6], &5u32.to_be_bytes());
    }

    #[test]
    fn peer_announcements_are_matched_to_local_opens() {
        let mut router = ChannelRouter::new(crate::appflow::APP, &DEFAULT_CHANNEL_PROFILE);
        router.stage_opens(ChannelOpenMode::ReferenceUnreliable, 4);
        assert_eq!(
            router.note_peer_open(4, 3, Some(3)),
            Some("peer announced chan 3 unreliable wire=3 (internal id -3)".into())
        );
        // Same parity as ours: not a peer id.
        let note = router.note_peer_open(4, 5, Some(4)).unwrap();
        assert!(note.contains("carries our local parity"), "{note}");
        assert_eq!(router.channels()[&5].peer_id, None);
        assert!(router.channels()[&5].peer_announced);
        // A channel the profile does not list is still recorded.
        assert!(router.note_peer_open(4, 99, None).unwrap().contains("reliable"));
        assert!(router.channels().contains_key(&99));
        // Other applications are kept out of the routing tables.
        assert!(router.note_peer_open(6, 3, Some(2)).unwrap().contains("not this session"));
        assert_eq!(router.foreign.len(), 1);
        let summary = router.summary();
        assert!(summary.contains("peer-announced 3"), "{summary}");
    }

    #[test]
    fn stream_binding_accounts_bytes_to_a_channel() {
        let mut router = ChannelRouter::new(crate::appflow::APP, &DEFAULT_CHANNEL_PROFILE);
        router.note_join_stream(7);
        assert_eq!(router.channel_for_stream(7), Some(1));
        router.note_stream_bytes(7, 128, 3);
        router.note_stream_bytes(9, 4096, 9); // unknown stream: unaccounted
        assert_eq!(router.channels()[&1].bytes, 128);
        assert_eq!(router.channels()[&1].frames, 3);
        assert!(router.summary().contains("routed 1"));
    }
}
