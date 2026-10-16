//! Outbound (upstream) sync for the RbxTransport Team Create session.
//!
//! Two separate things need to go back out after a join, and this module keeps
//! them apart:
//!
//! * *Liveness* — the traffic a client that is merely present emits: a
//!   flow-control/heartbeat block on the reliable channels (`0xA6`), the
//!   route/step declarations channel 11 expects, and the periodic ping and sync
//!   templates a dummy client sends (1 s and 5 s in the reference client).
//!   Everything here is a builder plus a timer; nothing claims the peer wanted
//!   them.
//! * *Property sync* — the `0x83`/`0x03` items that carry an edit back to the
//!   server. The item that a replicated edit needs is fully described on the
//!   *inbound* side, but its property id indexes the replicator's runtime table,
//!   which the snapshot does not send. This module therefore plans changes from
//!   a real DOM diff, encodes them against the candidate global id table
//!   [`crate::rbx_replication::JoinSchema::global_prop_ids`] validates, and
//!   refuses to send anything it cannot address — counting and reporting the
//!   refusal instead of guessing an id.
//!
//! The reference material in `dump/` (`reference/boblox/send.cpp` and
//! `session.cpp`) is a live-capture-derived model of the Android client, not
//! Studio Beta proof, so every byte layout here is labelled reference-modelled.
//! The one exception is the control block's `N`/`A`/`B` prefix, which the
//! reference itself marks as a framing hypothesis; that is carried through as a
//! counter to test rather than as a fact.
//!
//! Nothing in this module runs unless a session asks it to: the wire is silent
//! by default so enabling the module cannot change a handshake experiment that
//! is already in flight.

use std::collections::BTreeMap;

use rbx_dom_weak::types::{Ref, Variant};
use rbx_dom_weak::WeakDom;

use crate::rbx_replication::{
    frame, write_varint_leb, Baseline, DELTA_CHAIN_TAG, JOIN_DATA_SUBCODE,
};

/// Message id of the flow-control/heartbeat block (`FUN_1437fc810`).
pub const CONTROL_TAG: u8 = 0xA6;
/// Outbound delta chain tag; property updates ride inside it.
pub const PROPERTY_UPDATE_ITEM: u8 = 0x03;
/// Chain terminator.
pub const CHAIN_END: u8 = 0x00;
/// The channel the reference client pings on.
pub const PING_CHANNEL: u32 = 1;
/// The channel the reference client sends its sync template on.
pub const SYNC_CHANNEL: u32 = 3;
/// Ping period, from the reference client's `next_ping_ms_ += 1000`.
pub const PING_INTERVAL_MS: u64 = 1000;
/// Sync period, from `next_sync_ms_ += 5000`.
pub const SYNC_INTERVAL_MS: u64 = 5000;

// ---------------------------------------------------------------------------
// Heartbeat and flow control
// ---------------------------------------------------------------------------

/// The heartbeat block: `0xA6 0x00`, i.e. "no control entries".
pub fn heartbeat() -> Vec<u8> {
    vec![CONTROL_TAG, 0x00]
}

/// Whether a payload is a flow-control block, and whether it advertises
/// entries. Used to confirm the heartbeat went out as intended.
pub fn describe_control(payload: &[u8]) -> Option<bool> {
    let (&tag, rest) = payload.split_first()? else {
        return None;
    };
    if tag != CONTROL_TAG {
        return None;
    }
    Some(rest.first().copied().unwrap_or(0) != 0)
}

/// The tag-to-default-size map the reference client uses when a control value
/// is left empty: `0x01` presence byte sized by tag.
pub fn control_value_size(tag: u8) -> Option<usize> {
    match tag {
        0 => Some(1),
        1 | 2 | 7 => Some(4),
        3 => Some(8),
        4 => Some(5),
        _ => None,
    }
}

/// One flow-control block, as the reference client writes it:
/// `[N=1][A u32][B u32][inner u16 LE][presence bitmap][value]`.
///
/// `A` and `B` are zeroed because the reference labels them "framing-OK
/// hypothesis" — they make the record parse, not more. Slots run 0..=7 with the
/// bitmap bit at `1 << slot`.
pub fn control_block(slot: u8, tag: u8, value: &[u8]) -> Option<Vec<u8>> {
    if slot > 7 {
        return None;
    }
    let mut value = value.to_vec();
    if value.is_empty() {
        value = vec![0u8; control_value_size(tag)?];
    }
    let inner = 1usize + value.len();
    let inner = u16::try_from(inner).ok()?;
    let mut out = Vec::with_capacity(11 + value.len());
    out.push(1); // N: one block
    out.extend_from_slice(&0u32.to_le_bytes()); // A
    out.extend_from_slice(&0u32.to_le_bytes()); // B
    out.extend_from_slice(&inner.to_le_bytes());
    out.push(1u8 << slot);
    out.extend_from_slice(&value);
    Some(out)
}

/// Replace a route declaration's trailing idle `N=00` byte with a control
/// block, the way the reference client's `withControl` does.
pub fn with_control_block(route: &[u8], block: &[u8]) -> Option<Vec<u8>> {
    let (&tag, _) = route.split_first()? else {
        return None;
    };
    if tag != CONTROL_TAG || route.len() < 2 || block.is_empty() {
        return None;
    }
    let mut out = route[..route.len() - 1].to_vec();
    out.extend_from_slice(block);
    Some(out)
}

/// Patch the two little-endian step words a route declaration carries.
///
/// Returns the payload (unframed, like [`ROUTES`] in the flow) or `None` when
/// the offset does not leave room for both words.
pub fn with_steps(route: &[u8], offset: usize, first: u32, second: u32) -> Option<Vec<u8>> {
    if route.first().copied()? != CONTROL_TAG {
        return None;
    }
    let end = offset.checked_add(8)?;
    if end > route.len() {
        return None;
    }
    let mut out = route.to_vec();
    out[offset..offset + 4].copy_from_slice(&first.to_le_bytes());
    out[offset + 4..end].copy_from_slice(&second.to_le_bytes());
    Some(out)
}

/// A monotonic pair of step counters, for routes that advertise how far this
/// client has consumed. Overflow is legal here — the target's counters wrap.
#[derive(Clone, Debug, Default)]
pub struct StepClock {
    first: u32,
    second: u32,
    pub ticks: u64,
}

impl StepClock {
    pub fn new(first: u32, second: u32) -> Self {
        Self { first, second, ticks: 0 }
    }

    pub fn peek(&self) -> (u32, u32) {
        (self.first, self.second)
    }

    /// Advance both counters by `delta` and return the new pair.
    pub fn advance(&mut self, delta: u32) -> (u32, u32) {
        self.first = self.first.wrapping_add(delta);
        self.second = self.second.wrapping_add(delta);
        self.ticks += 1;
        (self.first, self.second)
    }
}

/// The idle-handshake and capability probes the reference client tried on
/// channel 1, kept together because they are the same class of experiment: a
/// hypothesis about what the peer is waiting for, not a proven requirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinProbe {
    /// `[96 00]` — the idle form.
    Idle,
    /// `[96 01 08 00 00 00]` — the feature-flag form.
    Feature,
    /// `[99 02]` then twelve zero bytes.
    Caps,
}

impl JoinProbe {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "0x96 idle form (no features asserted)",
            Self::Feature => "0x96 with the capability flag",
            Self::Caps => "0x99 one-shot capability block",
        }
    }

    /// The payload, unframed.
    pub fn payload(self) -> Vec<u8> {
        match self {
            Self::Idle => vec![0x96, 0x00],
            Self::Feature => vec![0x96, 0x01, 0x08, 0x00, 0x00, 0x00],
            Self::Caps => {
                let mut out = vec![0x99, 0x02];
                out.resize(14, 0);
                out
            }
        }
    }

    /// The framed payload, ready to append to a stream.
    pub fn framed(self) -> Vec<u8> {
        frame(&self.payload())
    }
}

// ---------------------------------------------------------------------------
// The reference client's liveness timers
// ---------------------------------------------------------------------------

/// One timer firing, on the channel the reference client used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LivenessTick {
    pub channel: u32,
    pub name: &'static str,
    /// Framed payload, ready to append to that channel's stream.
    pub payload: Vec<u8>,
}

/// The `pingTmpl` the reference client replays every second on channel 1.
///
/// Its first byte is `0x08`, not a replication message id, which the reference
/// notes explicitly: this template was captured on the app=6 dummy connection,
/// so it is evidence of what a client sends, not proof of what a Team Create
/// server expects.
pub fn ping_template() -> Vec<u8> {
    let mut out = vec![
        0x08, 0x58, 0xFB, 0xBD, 0xED, 0x5D, 0xDC, 0x18, 0x40, 0x00, 0x00, 0x00, 0x01,
    ];
    out.resize(13 + 64, b'P');
    out
}

/// The `syncTmpl` the reference client replays every five seconds on channel 3.
pub fn sync_template() -> Vec<u8> {
    let mut out = vec![0x03, 0xFA, 0x47, 0x15, 0x01];
    out.resize(25, 0);
    out
}

/// Drives the two liveness periods. Disabled by default: these bytes are
/// reference-modelled, and a session that has not confirmed what the peer wants
/// should not be adding traffic it cannot interpret the absence of.
#[derive(Clone, Debug)]
pub struct LivenessTimers {
    enabled: bool,
    next_ping_ms: u64,
    next_sync_ms: u64,
    pub pings: u64,
    pub syncs: u64,
    /// Catch-up iterations, so a starved receive loop cannot spin here.
    pub skipped: u64,
}

impl LivenessTimers {
    pub fn new(now_ms: u64) -> Self {
        Self {
            enabled: false,
            next_ping_ms: now_ms.saturating_add(PING_INTERVAL_MS),
            next_sync_ms: now_ms.saturating_add(SYNC_INTERVAL_MS),
            pings: 0,
            syncs: 0,
            skipped: 0,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, now_ms: u64) {
        self.enabled = enabled;
        if enabled {
            self.next_ping_ms = now_ms.saturating_add(PING_INTERVAL_MS);
            self.next_sync_ms = now_ms.saturating_add(SYNC_INTERVAL_MS);
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The two frames the reference client sends immediately after connecting:
    /// sync first on channel 3, then ping on channel 1.
    pub fn initial(&mut self) -> Vec<LivenessTick> {
        self.pings += 1;
        self.syncs += 1;
        vec![
            LivenessTick {
                channel: SYNC_CHANNEL,
                name: "sync#0",
                payload: frame(&sync_template()),
            },
            LivenessTick {
                channel: PING_CHANNEL,
                name: "ping#0",
                payload: frame(&ping_template()),
            },
        ]
    }

    /// Fire whatever periods have elapsed, advancing by whole periods so a long
    /// stall produces one tick per elapsed period rather than one per template.
    pub fn on_tick(&mut self, now_ms: u64) -> Vec<LivenessTick> {
        let mut out = Vec::new();
        if !self.enabled {
            return out;
        }
        for _ in 0..4 {
            if now_ms < self.next_ping_ms {
                break;
            }
            self.next_ping_ms = self.next_ping_ms.saturating_add(PING_INTERVAL_MS);
            self.pings += 1;
            out.push(LivenessTick {
                channel: PING_CHANNEL,
                name: "ping",
                payload: frame(&ping_template()),
            });
            // One frame per period is all the reference client sends; anything
            // further behind is counted as skipped rather than replayed.
            if now_ms >= self.next_ping_ms {
                let missed = (now_ms - self.next_ping_ms) / PING_INTERVAL_MS;
                if missed > 0 {
                    self.skipped += missed;
                    self.next_ping_ms = self.next_ping_ms.saturating_add(missed * PING_INTERVAL_MS);
                }
            }
            break;
        }
        for _ in 0..4 {
            if now_ms < self.next_sync_ms {
                break;
            }
            self.next_sync_ms = self.next_sync_ms.saturating_add(SYNC_INTERVAL_MS);
            self.syncs += 1;
            out.push(LivenessTick {
                channel: SYNC_CHANNEL,
                name: "sync",
                payload: frame(&sync_template()),
            });
            if now_ms >= self.next_sync_ms {
                let missed = (now_ms - self.next_sync_ms) / SYNC_INTERVAL_MS;
                if missed > 0 {
                    self.skipped += missed;
                    self.next_sync_ms = self.next_sync_ms.saturating_add(missed * SYNC_INTERVAL_MS);
                }
            }
            break;
        }
        out
    }

    pub fn summary(&self) -> String {
        format!(
            "{} ({} ping(s) issued at {PING_INTERVAL_MS}ms on chan{PING_CHANNEL}, {} sync(s) issued at {SYNC_INTERVAL_MS}ms on chan{SYNC_CHANNEL}, {} period(s) skipped); issued counts frames the timers produced, not writes the peer received",
            if self.enabled { "enabled" } else { "disabled" },
            self.pings,
            self.syncs,
            self.skipped
        )
    }
}

// ---------------------------------------------------------------------------
// Outbound property changes
// ---------------------------------------------------------------------------

/// How a change addresses the property it writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropId {
    /// The replicator's runtime property id — the only form a `0x03` item
    /// carries.
    Runtime(u16),
    /// The class's member index, which is what a snapshot record's property loop
    /// uses. Useful for a candidate table, never sendable on its own.
    MemberIndex(u16),
}

impl PropId {
    pub fn value(self) -> u16 {
        match self {
            Self::Runtime(value) | Self::MemberIndex(value) => value,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Runtime(_) => "runtime id",
            Self::MemberIndex(_) => "member index",
        }
    }
}

/// One prepared property change: the entity, the property, the value tag and the
/// encoded value bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub entity: u32,
    pub property: PropId,
    pub tag: u8,
    pub raw: Vec<u8>,
}

/// Encode a DOM value in the wire form its tag describes.
///
/// This is the exact inverse of the extent the replication reader skips, for the
/// subset of tags whose width is derivable from a `Variant`; see
/// [`crate::rbx_replication`] for the reader. Anything else returns `None`, so a
/// caller counts it instead of shipping bytes whose length the peer would
/// misread. In particular no CFrame-family tag is encodable, because the
/// quantized rotation form is not recovered.
pub fn encode_tagged(tag: u8, value: &Variant) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    match (tag, value) {
        (2, Variant::String(text))
        | (8, Variant::String(text))
        | (33, Variant::String(text)) => {
            out.extend_from_slice(&write_varint_leb(text.len() as u64));
            out.extend_from_slice(text.as_bytes());
        }
        (9, Variant::Bool(flag)) => out.push(u8::from(*flag)),
        (7, Variant::Enum(en)) => {
            out.extend_from_slice(&write_varint_leb(u64::from(en.clone().to_u32())))
        }
        // Signed ints: tag 10 is the zigzag form, 34 and 44 are plain LEB128.
        (10, Variant::Int32(number)) => {
            let signed = i64::from(*number);
            let encoded = ((signed << 1) ^ (signed >> 63)) as u64;
            out.extend_from_slice(&write_varint_leb(encoded));
        }
        (34, Variant::Int32(number)) => {
            // Sign-extended, because the reader widens to i64 before it
            // re-narrows: `u64::from(i32)` does not exist and a plain `as u32`
            // would drop the high half of a negative value's encoding.
            out.extend_from_slice(&write_varint_leb(*number as u64))
        }
        (44, Variant::Int64(number)) => {
            out.extend_from_slice(&write_varint_leb(*number as u64))
        }
        (11, Variant::Float32(number)) | (16, Variant::Float32(number)) | (17, Variant::Float32(number)) => {
            out.extend_from_slice(&number.to_le_bytes());
        }
        (11, Variant::Int32(number)) => out.extend_from_slice(&number.to_le_bytes()),
        (12, Variant::Float64(number)) | (48, Variant::Float64(number)) => {
            out.extend_from_slice(&number.to_le_bytes());
        }
        (12, Variant::Int64(number)) | (62, Variant::Int64(number)) => {
            out.extend_from_slice(&number.to_le_bytes());
        }
        (13, Variant::UDim(udim)) => {
            out.extend_from_slice(&udim.scale.to_le_bytes());
            out.extend_from_slice(&udim.offset.to_le_bytes());
        }
        (14, Variant::UDim2(udim2)) => {
            out.extend_from_slice(&udim2.x.scale.to_le_bytes());
            out.extend_from_slice(&udim2.x.offset.to_le_bytes());
            out.extend_from_slice(&udim2.y.scale.to_le_bytes());
            out.extend_from_slice(&udim2.y.offset.to_le_bytes());
        }
        (15, Variant::Vector3(vector)) => {
            // tag 15 is a 24-byte pair of vectors; a lone vector cannot fill it,
            // so the second half is zero-filled and the reader still consumes the
            // width it advertises.
            for number in [vector.x, vector.y, vector.z, 0.0, 0.0, 0.0] {
                out.extend_from_slice(&number.to_le_bytes());
            }
        }
        (19, Variant::Vector3(vector)) | (36, Variant::Vector3(vector)) => {
            for number in [vector.x, vector.y, vector.z] {
                out.extend_from_slice(&number.to_le_bytes());
            }
        }
        (21, Variant::Vector2(vector)) | (37, Variant::Vector2(vector)) => {
            for number in [vector.x, vector.y] {
                out.extend_from_slice(&number.to_le_bytes());
            }
        }
        (22, Variant::Color3(color)) => {
            for number in [color.r, color.g, color.b] {
                out.extend_from_slice(&number.to_le_bytes());
            }
        }
        (20, Variant::Color3uint8(color)) => {
            out.push(color.r);
            out.push(color.g);
            out.push(color.b);
        }
        (49, Variant::String(text)) => {
            // The literal form only: a shared-table index would need this client
            // to have announced the string first.
            let len = u8::try_from(text.len()).ok()?;
            out.push(len);
            out.extend_from_slice(text.as_bytes());
        }
        _ => return None,
    }
    Some(out)
}

/// A plan built from a DOM comparison: what changed, and everything that could
/// not become a change.
#[derive(Clone, Debug, Default)]
pub struct ChangePlan {
    pub changes: Vec<Change>,
    /// Instances the snapshot gave this client and that still exist.
    pub compared: usize,
    /// Instances only one side has: no outbound create or delete item is
    /// modelled, so these are counted and reported.
    pub only_in_before: usize,
    pub only_in_after: usize,
    /// Properties the snapshot's class dictionary did not name, or that sit at a
    /// member index a record loop cannot express (more than 255 members deep).
    pub unmapped: usize,
    /// Reparents and class changes. No outbound item is modelled for either, so
    /// these are counted and reported, never sent.
    pub parent_changes: usize,
    pub class_changes: usize,
    /// `class.property (tag N)` strings for values this writer cannot encode.
    pub unencodable: Vec<String>,
    /// Whether runtime ids were available for this schema.
    pub addressing: String,
}

impl ChangePlan {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "{} change(s) over {} instance(s); {} property(ies) not in the dictionary, {} value(s) not encodable, {} instance(s) gone, {} new, {} reparent(s) and {} class change(s) not sendable; {}",
            self.changes.len(),
            self.compared,
            self.unmapped,
            self.unencodable.len(),
            self.only_in_before,
            self.only_in_after,
            self.parent_changes,
            self.class_changes,
            self.addressing
        )
    }
}

/// Diff a live DOM against the state a replicated snapshot left behind.
///
/// `ids` and `baseline` come from
/// [`crate::rbx_replication::SnapshotApply`]: the wire-id map says which
/// instances the session owns, and the baseline says what each one held when it
/// was applied. Anything the wire vocabulary cannot express — a reparent, a
/// class change, an instance that appeared or vanished — is counted rather than
/// dropped silently.
///
/// The property id a change carries depends on the schema: with a corroborated
/// global table the items are runtime-addressed and [encode_property_items] will
/// accept them; without one they are member indices and the encoder refuses
/// them, which is the difference between "we know what changed" and "the peer
/// would agree".
pub fn plan_changes(
    schema: &crate::rbx_replication::JoinSchema,
    dom: &WeakDom,
    ids: &BTreeMap<u32, Ref>,
    baseline: &Baseline,
    limit: usize,
) -> ChangePlan {
    let mut plan = ChangePlan::default();
    let global = schema.global_prop_ids();
    plan.addressing = match &global {
        Some(table) => format!(
            "runtime ids from the candidate global table ({} name(s))",
            table.len()
        ),
        None => format!("member indices only — {}", schema.prop_id_report()),
    };
    // Reverse map, so a reparent can be named by the wire id the peer uses.
    // `Ref` has no `Ord`, so the reverse index is a hash map.
    let mut by_ref: std::collections::HashMap<Ref, u32> = std::collections::HashMap::new();
    for (wire_id, reference) in ids {
        by_ref.insert(*reference, *wire_id);
    }
    for (wire_id, reference) in ids {
        let before = baseline.entries.get(wire_id);
        let current = dom.get_by_ref(*reference);
        match (before, current) {
            (Some(_), None) => {
                plan.only_in_before += 1;
                continue;
            }
            (None, Some(_)) => {
                plan.only_in_after += 1;
                continue;
            }
            (None, None) => continue,
            (Some(before), Some(current)) => {
                plan.compared += 1;
                let class = current.class.to_string();
                if class != before.class {
                    plan.class_changes += 1;
                }
                let current_parent = by_ref.get(&current.parent()).copied();
                if current_parent != before.parent {
                    plan.parent_changes += 1;
                }
                let Some(class_index) = schema
                    .classes
                    .iter()
                    .position(|defined| defined.name.eq_ignore_ascii_case(&class))
                    .map(|index| index as u16)
                else {
                    plan.unmapped += 1;
                    continue;
                };
                // The instance name is a DOM field here and a member in the
                // dictionary, so it is compared separately.
                if current.name != before.name {
                    let renamed = Variant::String(current.name.clone());
                    match encode_member(
                        schema,
                        &global,
                        class_index,
                        *wire_id,
                        &class,
                        "Name",
                        &renamed,
                    ) {
                        Some(Ok(change)) => plan.changes.push(change),
                        Some(Err(reason)) => plan.unencodable.push(reason),
                        None => plan.unmapped += 1,
                    }
                    if plan.changes.len() >= limit {
                        return plan;
                    }
                }
                for (key, value) in current.properties.iter() {
                    let name: &str = key;
                    // Equal means nothing to send, and it is the cheaper test:
                    // an unchanged property that the dictionary does not name
                    // must not be reported as a mapping failure.
                    if before.property(name) == Some(value) {
                        continue;
                    }
                    match encode_member(
                        schema,
                        &global,
                        class_index,
                        *wire_id,
                        &class,
                        name,
                        value,
                    ) {
                        Some(Ok(change)) => plan.changes.push(change),
                        Some(Err(reason)) => plan.unencodable.push(reason),
                        None => plan.unmapped += 1,
                    }
                    if plan.changes.len() >= limit {
                        return plan;
                    }
                }
            }
        }
    }
    plan
}

/// Resolve one property through the snapshot's class dictionary and encode it.
///
/// `None` means the dictionary does not name the property, or names it deeper
/// than the one-byte index a record loop can carry — either way the caller counts
/// it as unmapped instead of inventing a number.
fn encode_member(
    schema: &crate::rbx_replication::JoinSchema,
    global: &Option<BTreeMap<String, u16>>,
    class_index: u16,
    entity: u32,
    class: &str,
    name: &str,
    value: &Variant,
) -> Option<Result<Change, String>> {
    let index = schema.member_index(class_index, name)?;
    if index > usize::from(u8::MAX) {
        return None;
    }
    let tag = schema.member(class_index, index as u8)?.tag;
    let Some(raw) = encode_tagged(tag, value) else {
        return Some(Err(format!(
            "{class}.{name} (tag {tag}) has no writer here"
        )));
    };
    let property = match global {
        Some(table) => table
            .get(&name.to_ascii_lowercase())
            .map(|id| PropId::Runtime(*id))
            .unwrap_or(PropId::MemberIndex(index as u16)),
        None => PropId::MemberIndex(index as u16),
    };
    Some(Ok(Change { entity, property, tag, raw }))
}

/// The property items of one delta chain:
/// `[83] ( [03][ref selector][id u32 LE][prop id u16 LE][value] )* [00]`.
///
/// Refused unless every change is addressed with a runtime property id, because
/// that is the only id kind the item carries.
pub fn encode_property_items(changes: &[Change]) -> Result<Vec<u8>, String> {
    if changes.is_empty() {
        return Err("no property changes to encode".to_string());
    }
    let mut out = Vec::new();
    out.push(DELTA_CHAIN_TAG);
    for change in changes {
        let PropId::Runtime(property) = change.property else {
            return Err(format!(
                "change for entity {} uses a {} ({}) which a 0x03 item cannot carry",
                change.entity,
                change.property.label(),
                change.property.value()
            ));
        };
        out.push(PROPERTY_UPDATE_ITEM);
        out.push(1); // reference selector: a real id follows
        out.extend_from_slice(&change.entity.to_le_bytes());
        out.extend_from_slice(&property.to_le_bytes());
        out.extend_from_slice(&change.raw);
    }
    out.push(CHAIN_END);
    Ok(out)
}

/// A coalescing queue of prepared changes, plus the flush that turns them into
/// one chain.
///
/// Multiple edits to one property within a period collapse into the last value,
/// which is the behaviour that keeps a dragged slider from writing a stream per
/// frame. Oldest changes are dropped first when the byte cap is hit, and the
/// drop count is what the report shows.
pub struct ChangeQueue {
    pending: Vec<Option<Change>>,
    index: BTreeMap<(u32, u8, u16), usize>,
    bytes: usize,
    capacity: usize,
    pub dropped: u64,
    pub coalesced: u64,
}

impl ChangeQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            pending: Vec::new(),
            index: BTreeMap::new(),
            bytes: 0,
            capacity: capacity.max(1024),
            dropped: 0,
            coalesced: 0,
        }
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1024);
    }

    pub fn len(&self) -> usize {
        self.pending.iter().filter(|slot| slot.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Add or replace a change. Returns true when it was a new slot.
    pub fn queue(&mut self, change: Change) -> bool {
        let key = (
            change.entity,
            match change.property {
                PropId::Runtime(_) => 0u8,
                PropId::MemberIndex(_) => 1,
            },
            change.property.value(),
        );
        let size = 12 + change.raw.len();
        if let Some(slot) = self.index.get(&key).copied() {
            if let Some(existing) = self.pending.get_mut(slot).and_then(|slot| slot.as_mut()) {
                self.bytes = self.bytes.saturating_sub(12 + existing.raw.len());
                existing.raw = change.raw;
                existing.tag = change.tag;
                self.bytes += size;
                self.coalesced += 1;
                return false;
            }
        }
        self.pending.push(Some(change));
        let slot = self.pending.len() - 1;
        self.index.insert(key, slot);
        self.bytes += size;
        while self.bytes > self.capacity {
            match self.pending.iter().position(|slot| slot.is_some()) {
                Some(oldest) => {
                    if let Some(dropped) = self.pending[oldest].take() {
                        self.bytes = self.bytes.saturating_sub(12 + dropped.raw.len());
                        self.dropped += 1;
                    }
                }
                None => break,
            }
            if self.bytes <= self.capacity {
                break;
            }
        }
        true
    }

    /// Take up to `limit` queued changes, oldest first, and compact the queue.
    pub fn take(&mut self, limit: usize) -> Vec<Change> {
        let mut out = Vec::with_capacity(limit.min(self.len()));
        for slot in &mut self.pending {
            if out.len() >= limit {
                break;
            }
            if let Some(change) = slot.take() {
                self.bytes = self.bytes.saturating_sub(12 + change.raw.len());
                out.push(change);
            }
        }
        self.pending.retain(|slot| slot.is_some());
        self.index.clear();
        for (slot, change) in self.pending.iter().enumerate() {
            if let Some(change) = change {
                let key = (
                    change.entity,
                    match change.property {
                        PropId::Runtime(_) => 0u8,
                        PropId::MemberIndex(_) => 1,
                    },
                    change.property.value(),
                );
                self.index.insert(key, slot);
            }
        }
        out
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.index.clear();
        self.bytes = 0;
    }
}

/// The outbound sync scheduler: a queue, a flush period, and the counters that
/// make a partial path visible.
///
/// Disabled by default. Even when enabled, nothing is sent unless the schema
/// corroborated the candidate global id table, because the alternative is
/// addressing a property with a guessed number.
pub struct SyncEngine {
    queue: ChangeQueue,
    enabled: bool,
    interval_ms: u64,
    next_flush_ms: u64,
    max_items_per_flush: usize,
    pub flushes: u64,
    pub items_sent: u64,
    pub bytes_sent: u64,
    /// Flushes refused because the changes were not addressable, with the first
    /// reason so the report can say why.
    pub refusals: u64,
    pub last_refusal: String,
    pub plans: u64,
}

impl SyncEngine {
    pub fn new(now_ms: u64, interval_ms: u64) -> Self {
        Self {
            queue: ChangeQueue::new(4 << 20),
            enabled: false,
            interval_ms: interval_ms.max(50),
            next_flush_ms: now_ms,
            max_items_per_flush: 256,
            flushes: 0,
            items_sent: 0,
            bytes_sent: 0,
            refusals: 0,
            last_refusal: String::new(),
            plans: 0,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool, now_ms: u64) {
        self.enabled = enabled;
        self.next_flush_ms = now_ms;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    pub fn set_interval_ms(&mut self, interval_ms: u64) {
        self.interval_ms = interval_ms.max(50);
    }

    pub fn set_max_items_per_flush(&mut self, limit: usize) {
        self.max_items_per_flush = limit.max(1);
    }

    pub fn next_flush_ms(&self) -> u64 {
        self.next_flush_ms
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn pending_bytes(&self) -> usize {
        self.queue.bytes()
    }

    pub fn dropped(&self) -> u64 {
        self.queue.dropped
    }

    pub fn coalesced(&self) -> u64 {
        self.queue.coalesced
    }

    /// Fold a plan into the queue. Returns the number of changes accepted.
    pub fn queue_plan(&mut self, plan: &ChangePlan) -> usize {
        let mut accepted = 0;
        for change in &plan.changes {
            if self.queue.queue(change.clone()) {
                accepted += 1;
            }
            if self.queue.len() >= self.max_items_per_flush * 4 {
                break;
            }
        }
        self.plans += 1;
        accepted
    }

    /// Whether a flush is due.
    pub fn due(&self, now_ms: u64) -> bool {
        self.enabled && now_ms >= self.next_flush_ms && !self.queue.is_empty()
    }

    /// Encode and consume one flush. `None` means nothing went out, and the
    /// counters and `last_refusal` say why.
    pub fn flush(&mut self, now_ms: u64) -> Option<Vec<u8>> {
        if !self.due(now_ms) {
            return None;
        }
        self.next_flush_ms = now_ms.saturating_add(self.interval_ms);
        let changes = self.queue.take(self.max_items_per_flush);
        match encode_property_items(&changes) {
            Ok(payload) => {
                self.flushes += 1;
                self.items_sent += changes.len() as u64;
                self.bytes_sent += payload.len() as u64;
                Some(payload)
            }
            Err(reason) => {
                self.refusals += 1;
                self.last_refusal = reason;
                // The changes stay dropped: re-queueing them every period would
                // hide the fact that they can never be addressed.
                None
            }
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} ({} flush(es), {} item(s), {} byte(s), {} pending/{}B, {} coalesced, {} dropped{}{})",
            if self.enabled { "enabled" } else { "disabled" },
            self.flushes,
            self.items_sent,
            self.bytes_sent,
            self.queue.len(),
            self.queue.bytes(),
            self.queue.coalesced,
            self.queue.dropped,
            if self.refusals > 0 {
                format!(", {} refusal(s)", self.refusals)
            } else {
                String::new()
            },
            if self.last_refusal.is_empty() {
                String::new()
            } else {
                format!("; last: {}", self.last_refusal)
            }
        )
    }
}

/// The channel a batch or chain goes out on, given what the peer announced:
/// reliable channels take a stream, and everything else shares channel 11 with
/// the route declarations. Kept here so the flow and the tests agree on it.
pub fn upstream_channel(reliable: bool) -> u32 {
    if reliable { 1 } else { 11 }
}

/// Whether a payload is the `JoinDataItemV2` sub-code, which never chains.
pub fn is_batch_payload(payload: &[u8]) -> bool {
    payload.len() >= 2 && payload[0] == DELTA_CHAIN_TAG && payload[1] == JOIN_DATA_SUBCODE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbx_replication::{
        parse_join_data_item, value_extent, walk_delta_chain, walk_frames, BaselineInstance,
        Reader, JOIN_START_TAG,
    };
    use rbx_dom_weak::{
        types::{CFrame, Color3, Color3uint8, Enum, UDim, UDim2, Vector2, Vector3},
        InstanceBuilder, WeakDom,
    };

    #[test]
    fn heartbeat_and_control_blocks_frame_cleanly() {
        let beat = heartbeat();
        assert_eq!(beat, vec![0xA6, 0x00]);
        assert_eq!(describe_control(&beat), Some(false));
        // The flow's framing turns a 2-byte payload into a single-byte prefix.
        assert_eq!(frame(&beat), vec![0x02, 0xA6, 0x00]);

        let block = control_block(0, 1, &[]).expect("slot 0, tag 1");
        // [N=1][A][B][inner=5][presence=01][value]
        assert_eq!(block.len(), 16);
        assert_eq!(block[0], 1);
        assert_eq!(&block[1..9], &[0; 8]);
        assert_eq!(&block[9..11], &[5, 0]);
        assert_eq!(block[11], 1);
        assert_eq!(&block[12..], &[0, 0, 0, 0]);
        // An explicit value keeps its own length, and the inner count follows it.
        let with_value = control_block(3, 0, &[7]).expect("bool slot 3");
        assert_eq!(&with_value[9..11], &[2, 0]);
        assert_eq!(with_value[11], 1 << 3);
        assert_eq!(with_value.last(), Some(&7u8));
        assert_eq!(control_block(8, 0, &[]), None, "slots run 0..=7");
        assert_eq!(control_block(0, 6, &[]), None, "unknown tags fail closed");

        // Splicing a block onto a route replaces the trailing idle N=00 only.
        let route = crate::appflow::route_template(0);
        let patched = with_control_block(&route, &block).expect("patched route");
        assert_eq!(&patched[..route.len() - 1], &route[..route.len() - 1]);
        assert_eq!(patched.len(), route.len() - 1 + block.len());
        assert_eq!(describe_control(&patched), Some(true));
        // A route is the only thing this splices: the tag is checked.
        assert_eq!(with_control_block(&[0x90, 0x00], &block), None);
    }

    #[test]
    fn step_patch_agrees_with_the_route_layout() {
        let route = crate::appflow::route_template(0);
        assert_eq!(route[0], CONTROL_TAG);
        // The route's two step words sit at offset 5, which is where the
        // reference client patches them.
        let patched = with_steps(&route, 5, 0x1122_3344, 0x5566_7788).expect("patched");
        assert_eq!(&patched[5..9], &0x1122_3344u32.to_le_bytes());
        assert_eq!(&patched[9..13], &0x5566_7788u32.to_le_bytes());
        // Everything else is byte-identical, including the head and the tail.
        assert_eq!(&patched[1..5], &route[1..5]);
        assert_eq!(&patched[13..], &route[13..]);
        // An offset that would run past the payload is refused rather than
        // silently truncating the route.
        assert!(with_steps(&route, route.len() - 4, 1, 2).is_none());
        assert!(with_steps(&[0x90, 0x00], 0, 1, 2).is_none());

        let mut clock = StepClock::new(20, 20);
        assert_eq!(clock.advance(3), (23, 23));
        assert_eq!(clock.peek(), (23, 23));
        assert_eq!(clock.ticks, 1);
    }

    #[test]
    fn liveness_templates_keep_their_captured_sizes() {
        let ping = ping_template();
        assert_eq!(ping.len(), 77);
        assert_eq!(ping[0], 0x08);
        assert!(ping[13..].iter().all(|byte| *byte == b'P'));
        let sync = sync_template();
        assert_eq!(sync.len(), 25);
        assert_eq!(&sync[..5], &[0x03, 0xFA, 0x47, 0x15, 0x01]);
        assert!(sync[5..].iter().all(|byte| *byte == 0));

        // The frames carry the compact-varint prefix the stream expects; 77 is
        // past the one-byte form, which is why its prefix is two bytes.
        let framed = frame(&ping);
        assert_eq!(&framed[..2], &[0x40, 0x4D]);
        assert_eq!(framed.len(), 79);
        assert_eq!(frame(&sync).len(), 26);
    }

    #[test]
    fn liveness_timers_are_off_until_asked_and_never_replay_a_backlog() {
        let mut timers = LivenessTimers::new(1000);
        assert!(timers.on_tick(100_000).is_empty(), "disabled by default");
        timers.set_enabled(true, 0);
        assert!(timers.on_tick(999).is_empty(), "nothing is due yet");
        let ticks = timers.on_tick(1000);
        assert_eq!(ticks.len(), 1, "only the ping period has elapsed");
        assert_eq!(ticks[0].name, "ping");
        assert_eq!(ticks[0].channel, PING_CHANNEL);
        let ticks = timers.on_tick(5000);
        assert_eq!(ticks.len(), 2, "the sync period, with the ping period owed");
        assert_eq!(ticks[1].channel, SYNC_CHANNEL);
        assert_eq!(timers.pings, 2);
        assert_eq!(timers.syncs, 1);
        assert!(timers.skipped > 0, "{}", timers.summary());
        // A long stall is one tick per template, not a replay of every period.
        let ticks = timers.on_tick(1_000_000);
        assert_eq!(ticks.len(), 2);
        assert!(timers.skipped > 100, "{} skipped", timers.skipped);
        assert!(timers.summary().contains("enabled"));
        let initial = timers.initial();
        assert_eq!(initial.len(), 2);
        assert_eq!(initial[0].name, "sync#0", "the reference sends sync first");
        assert_eq!(initial[1].name, "ping#0");
    }

    #[test]
    fn probes_are_framed_and_labelled() {
        for probe in [JoinProbe::Idle, JoinProbe::Feature, JoinProbe::Caps] {
            let payload = probe.payload();
            let framed = probe.framed();
            assert_eq!(framed.len(), payload.len() + 1);
            assert_eq!(framed[1], payload[0]);
            let walk = walk_frames(&framed);
            assert_eq!(walk.frames.len(), 1, "{}", probe.label());
            assert_eq!(walk.frames[0].id, payload[0]);
        }
        assert_eq!(JoinProbe::Idle.payload(), vec![0x96, 0x00]);
        assert_eq!(JoinProbe::Caps.payload().len(), 14);
        assert_eq!(
            JoinProbe::Feature.payload(),
            vec![0x96, 0x01, 0x08, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn encoded_values_are_the_extents_the_reader_skips() {
        // Every writer here must produce exactly the extent the inbound reader
        // consumes; a writer one byte off desyncs the peer's parser for the rest
        // of the chain, and a value comparison would not show it.
        let cases: Vec<(u8, Variant, usize)> = vec![
            (2, Variant::String("hello".into()), 6),
            (9, Variant::Bool(true), 1),
            (7, Variant::Enum(Enum::from_u32(3)), 1),
            (10, Variant::Int32(-7), 1),
            (34, Variant::Int32(9), 1),
            (44, Variant::Int64(300), 2),
            (11, Variant::Float32(1.5), 4),
            (11, Variant::Int32(-1), 4),
            (12, Variant::Float64(2.5), 8),
            (62, Variant::Int64(-2), 8),
            (13, Variant::UDim(UDim::new(0.5, 10)), 8),
            (14, Variant::UDim2(UDim2::new(UDim::new(1.0, 2), UDim::new(3.0, 4))), 16),
            (15, Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)), 24),
            (19, Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)), 12),
            (21, Variant::Vector2(Vector2::new(1.0, 2.0)), 8),
            (22, Variant::Color3(Color3::new(0.1, 0.2, 0.3)), 12),
            (20, Variant::Color3uint8(Color3uint8::new(1, 2, 3)), 3),
            (49, Variant::String("Box".into()), 4),
        ];
        for (tag, value, expected) in cases {
            let encoded = encode_tagged(tag, &value)
                .unwrap_or_else(|| panic!("tag {tag} should be encodable"));
            assert_eq!(encoded.len(), expected, "tag {tag} length");
            let mut reader = Reader::new(&encoded);
            value_extent(&mut reader, tag)
                .unwrap_or_else(|error| panic!("tag {tag}: {error}"));
            assert_eq!(reader.position(), encoded.len(), "tag {tag} extent");
        }
        // Refused shapes: a reference value (needs the snapshot id map), a CFrame
        // (the rotation form is not recovered), a name too long for the literal
        // form, and any tag with no entry in the writer.
        assert!(encode_tagged(28, &Variant::Int32(1)).is_none());
        assert!(encode_tagged(27, &Variant::CFrame(CFrame::identity())).is_none());
        assert!(encode_tagged(49, &Variant::String("x".repeat(300))).is_none());
        assert!(encode_tagged(61, &Variant::String("rbxassetid://1".into())).is_none());
        assert!(encode_tagged(2, &Variant::Int32(1)).is_none(), "tag and type must agree");
    }

    #[test]
    fn a_property_chain_round_trips_through_the_replication_reader() {
        let changes = vec![
            Change {
                entity: 4,
                property: PropId::Runtime(11),
                tag: 9,
                raw: vec![1],
            },
            Change {
                entity: 9,
                property: PropId::Runtime(2),
                tag: 11,
                raw: 1.5f32.to_le_bytes().to_vec(),
            },
        ];
        let payload = encode_property_items(&changes).expect("encoded");
        // The exact chain, so a change in either the writer or the reader shows
        // up here rather than in a live session.
        assert_eq!(
            payload,
            vec![
                0x83, 0x03, 0x01, 4, 0, 0, 0, 11, 0, 1, //
                0x03, 0x01, 9, 0, 0, 0, 2, 0, 0x00, 0x00, 0xC0, 0x3F, //
                0x00
            ]
        );
        // The inbound walker reads the first item and then pins its stop point
        // at the second, because the value tags a 0x03 item needs are not on the
        // wire. That is the reader's documented limit, asserted not hidden.
        let chain = walk_delta_chain(&payload, None).expect("walk");
        assert_eq!(chain.property_updates, 1);
        assert_eq!(chain.items.len(), 1);
        assert!(!chain.complete);
        let stop = chain.stopped_at.expect("stop point");
        assert!(stop.reason.contains("runtime"), "{}", stop.reason);
        match &chain.items[0] {
            crate::rbx_replication::ChainItem::Property(update) => {
                assert_eq!(update.entity, Some(4));
                assert_eq!(update.property, 11);
            }
            other => panic!("expected a property update, got {other:?}"),
        }

        // A member-index change is not addressable, and saying so is the point.
        let unaddressable = vec![Change {
            entity: 4,
            property: PropId::MemberIndex(3),
            tag: 9,
            raw: vec![0],
        }];
        let error = encode_property_items(&unaddressable).unwrap_err();
        assert!(error.contains("member index"), "{error}");
        assert!(encode_property_items(&[]).is_err());

        assert!(is_batch_payload(&[DELTA_CHAIN_TAG, JOIN_DATA_SUBCODE, 1]));
        assert!(!is_batch_payload(&[JOIN_START_TAG, JOIN_DATA_SUBCODE, 1]));
        assert_eq!(upstream_channel(true), 1);
        assert_eq!(upstream_channel(false), 11);
        // The batch sub-code is what keeps a chain walk from misreading it.
        assert!(
            parse_join_data_item(&[DELTA_CHAIN_TAG, JOIN_DATA_SUBCODE, 1, 0, 0, 0, 0, 0, 0, 0, 0])
                .is_some()
        );
    }

    #[test]
    fn the_queue_coalesces_by_entity_and_property() {
        let mut queue = ChangeQueue::new(4 << 20);
        let change = |value: u8| Change {
            entity: 1,
            property: PropId::Runtime(7),
            tag: 9,
            raw: vec![value],
        };
        assert!(queue.queue(change(1)), "the first change is a new slot");
        assert!(!queue.queue(change(2)), "a second edit to the same property coalesces");
        assert!(!queue.queue(change(3)));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.coalesced, 2);
        let taken = queue.take(10);
        assert_eq!(taken.len(), 1, "the last value wins");
        assert_eq!(taken[0].raw, vec![3]);
        assert!(queue.is_empty());
        assert_eq!(queue.bytes(), 0);
    }

    #[test]
    fn the_queue_drops_oldest_when_full_and_keeps_id_kinds_separate() {
        let mut queue = ChangeQueue::new(1024);
        for index in 0..200u32 {
            queue.queue(Change {
                entity: index,
                property: PropId::Runtime(index as u16),
                tag: 2,
                raw: vec![0u8; 40],
            });
        }
        assert!(queue.bytes() <= 1024, "{} bytes", queue.bytes());
        assert!(queue.dropped > 100, "{} dropped", queue.dropped);
        let taken = queue.take(1000);
        assert!(taken.iter().all(|change| change.raw.len() == 40));
        assert!(taken.last().unwrap().entity > 100, "the newest survive");

        // Runtime id 1 and member index 1 are different slots.
        let mut queue = ChangeQueue::new(4 << 20);
        queue.queue(Change { entity: 5, property: PropId::Runtime(1), tag: 9, raw: vec![1] });
        queue.queue(Change { entity: 5, property: PropId::MemberIndex(1), tag: 9, raw: vec![2] });
        assert_eq!(queue.len(), 2);
        queue.clear();
        assert!(queue.is_empty());
        assert_eq!(queue.bytes(), 0);
    }

    #[test]
    fn the_engine_refuses_to_flush_unaddressable_changes() {
        let mut engine = SyncEngine::new(0, 500);
        engine.queue_plan(&ChangePlan {
            changes: vec![Change {
                entity: 4,
                property: PropId::MemberIndex(2),
                tag: 9,
                raw: vec![0],
            }],
            ..Default::default()
        });
        assert_eq!(engine.pending(), 1);
        assert!(engine.flush(1000).is_none(), "disabled by default");
        engine.set_enabled(true, 1000);
        assert!(engine.due(1000));
        assert!(engine.flush(1000).is_none(), "the change is not addressable");
        assert_eq!(engine.refusals, 1);
        assert!(engine.last_refusal.contains("member index"), "{}", engine.last_refusal);
        assert!(engine.flush(1100).is_none(), "the refusal consumed the queue");
        assert_eq!(engine.items_sent, 0);
        assert!(engine.summary().contains("1 refusal(s)"), "{}", engine.summary());

        // With runtime ids the same path produces a chain.
        let mut engine = SyncEngine::new(0, 500);
        engine.set_enabled(true, 0);
        engine.queue_plan(&ChangePlan {
            changes: vec![Change {
                entity: 4,
                property: PropId::Runtime(2),
                tag: 9,
                raw: vec![1],
            }],
            ..Default::default()
        });
        let payload = engine.flush(10).expect("flushed");
        assert_eq!(payload[0], DELTA_CHAIN_TAG);
        assert_eq!(engine.items_sent, 1);
        assert_eq!(engine.bytes_sent, payload.len() as u64);
        assert_eq!(engine.next_flush_ms(), 510);
        assert_eq!(engine.pending(), 0);
        assert!(!engine.due(600), "the period has not elapsed");
    }

    #[test]
    fn a_dom_edit_against_the_applied_baseline_becomes_one_change() {
        let schema = test_schema(3);
        let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
        let part = dom.insert(
            dom.root_ref(),
            InstanceBuilder::new("Part")
                .with_name("Box")
                .with_property("Size", Vector3::new(1.0, 2.0, 3.0)),
        );
        let ids: BTreeMap<u32, Ref> = [(7u32, part)].into_iter().collect();
        let baseline = baseline_of(&dom, &ids);
        assert_eq!(baseline.len(), 1);
        // Nothing has been edited yet, so a diff against the applied state is
        // empty even though the instance has properties.
        let plan = plan_changes(&schema, &dom, &ids, &baseline, 32);
        assert!(plan.is_empty(), "{}", plan.summary());
        assert_eq!(plan.compared, 1);

        if let Some(instance) = dom.get_by_ref_mut(part) {
            instance
                .properties
                .insert(rbx_dom_weak::Ustr::from("Size"), Variant::Vector3(Vector3::new(1.0, 2.0, 4.0)));
            instance.name = "Long Box".to_owned();
        }
        let plan = plan_changes(&schema, &dom, &ids, &baseline, 32);
        assert_eq!(plan.changes.len(), 2, "{}", plan.summary());
        let size = plan
            .changes
            .iter()
            .find(|change| change.tag == 19)
            .expect("the size change");
        assert_eq!(size.entity, 7);
        assert_eq!(size.tag, 19, "the dictionary's tag for size");
        assert_eq!(size.raw.len(), 12);
        assert_eq!(
            size.property,
            PropId::Runtime(0),
            "size is the first distinct member name"
        );
        let name = plan
            .changes
            .iter()
            .find(|change| change.tag == 49)
            .expect("the name change");
        assert_eq!(name.property, PropId::Runtime(1));
        assert!(plan.addressing.contains("candidate global table"), "{}", plan.addressing);
        // And the chain that would go out is encodable.
        let payload = encode_property_items(&plan.changes).expect("encodable");
        assert_eq!(payload[0], DELTA_CHAIN_TAG);
    }

    #[test]
    fn a_dictionary_that_does_not_line_up_yields_no_runtime_ids() {
        // The bound is wrong for the three names, so the candidate global table
        // is refused and the change is addressed by member index - which the
        // encoder then refuses to send. That refusal is the whole point of the
        // binding check: an invented property id would look like a successful
        // edit and be nothing of the kind.
        let schema = test_schema(999);
        assert!(schema.global_prop_ids().is_none());
        let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
        let part = dom.insert(
            dom.root_ref(),
            InstanceBuilder::new("Part").with_property("Size", Vector3::new(1.0, 2.0, 3.0)),
        );
        let ids: BTreeMap<u32, Ref> = [(3u32, part)].into_iter().collect();
        let baseline = baseline_of(&dom, &ids);
        if let Some(instance) = dom.get_by_ref_mut(part) {
            instance
                .properties
                .insert(rbx_dom_weak::Ustr::from("Size"), Variant::Vector3(Vector3::new(9.0, 2.0, 3.0)));
        }
        let plan = plan_changes(&schema, &dom, &ids, &baseline, 32);
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].property, PropId::MemberIndex(0));
        assert!(plan.addressing.contains("member indices"), "{}", plan.addressing);
        assert!(encode_property_items(&plan.changes).is_err());
    }

    #[test]
    fn ids_the_baseline_does_not_cover_are_counted_not_invented() {
        let schema = test_schema(3);
        let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
        let part = dom.insert(
            dom.root_ref(),
            InstanceBuilder::new("Part").with_property("Size", Vector3::new(1.0, 2.0, 3.0)),
        );
        let known: BTreeMap<u32, Ref> = [(7u32, part)].into_iter().collect();
        let mut baseline = baseline_of(&dom, &known);
        // The snapshot had applied a second instance that this DOM no longer
        // holds. `Ref` has no null sentinel, so a referent from another DOM
        // stands in for the dropped instance: only its absence matters.
        let dropped = WeakDom::new(InstanceBuilder::new("DataModel"));
        baseline.entries.insert(
            8,
            BaselineInstance {
                class: "Part".to_owned(),
                name: "Part".to_owned(),
                properties: BTreeMap::new(),
                parent: None,
            },
        );
        let ids: BTreeMap<u32, Ref> =
            [(7u32, part), (8u32, dropped.root_ref())].into_iter().collect();
        let plan = plan_changes(&schema, &dom, &ids, &baseline, 32);
        assert!(plan.changes.is_empty(), "{}", plan.summary());
        assert_eq!(plan.only_in_before, 1);
        assert_eq!(plan.compared, 1);
        let extra = dom.insert(
            dom.root_ref(),
            InstanceBuilder::new("Part").with_property("Size", Vector3::new(4.0, 4.0, 4.0)),
        );
        let plan = plan_changes(&schema, &dom, &ids, &baseline, 32);
        assert!(plan.changes.is_empty(), "a new instance is not sendable");
        let _ = extra;
        // Reparenting is the same story: counted, and never turned into a
        // property change, because no outbound item is modelled for it.
        let mut moved = WeakDom::new(InstanceBuilder::new("DataModel"));
        let parent = moved.insert(moved.root_ref(), InstanceBuilder::new("Model"));
        let child = moved.insert(moved.root_ref(), InstanceBuilder::new("Part"));
        let ids: BTreeMap<u32, Ref> = [(7u32, parent), (8u32, child)].into_iter().collect();
        let before = baseline_of(&moved, &ids);
        moved.transfer_within(child, parent);
        let plan = plan_changes(&schema, &moved, &ids, &before, 32);
        assert!(plan.changes.is_empty(), "{}", plan.summary());
        assert_eq!(plan.parent_changes, 1, "{}", plan.summary());
    }

    /// Capture the applied state the way `SnapshotApply::baseline` does, so the
    /// tests exercise the same contract the flow uses.
    fn baseline_of(dom: &WeakDom, ids: &BTreeMap<u32, Ref>) -> Baseline {
        let mut entries = BTreeMap::new();
        for (wire_id, reference) in ids {
            let Some(instance) = dom.get_by_ref(*reference) else { continue };
            let mut properties = BTreeMap::new();
            for (key, value) in &instance.properties {
                properties.insert(key.to_ascii_lowercase(), value.clone());
            }
            entries.insert(
                *wire_id,
                BaselineInstance {
                    class: instance.class.to_string(),
                    name: instance.name.clone(),
                    properties,
                    parent: None,
                },
            );
        }
        Baseline { entries }
    }

    /// A schema declaring `size` (tag 19), `name` (tag 49) and `anchored`
    /// (tag 9) on class Part, with a caller-chosen property bound so both sides
    /// of the id-binding rule can be exercised.
    fn test_schema(bound: u64) -> crate::rbx_replication::JoinSchema {
        let mut data = vec![3u8];
        data.extend_from_slice(&15u64.to_le_bytes());
        data.push(0); // no enums
        data.push(1); // one class
        data.extend_from_slice(&write_varint_leb(bound));
        data.push(0); // event bound
        data.push(4);
        data.extend_from_slice(b"Part");
        data.push(3); // three members
        data.push(4);
        data.extend_from_slice(b"size");
        data.push(19);
        data.push(4);
        data.extend_from_slice(b"name");
        data.push(49);
        data.push(8);
        data.extend_from_slice(b"anchored");
        data.push(9);
        data.extend_from_slice(&0u16.to_le_bytes());
        data.push(0); // no events
        data.push(0); // no URL prefixes
        data.push(0); // no shared names
        crate::rbx_replication::parse_join_schema(&data).expect("test schema")
    }
}
