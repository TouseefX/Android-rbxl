//! App=4 RbxTransport join-session flow.
//!
//! This models the observed post-handshake sequence from the Boblox client
//! (`session.cpp`): the control hello + openU burst, attempts at the channel-1
//! early-auth / A7 / 0x90 / 0x92 / 0x8A / 0x8F burst, local 0x9B challenge solving (via
//! [`crate::challenge`]), and route declarations scheduled about 60 ms after
//! the local solve. These queued actions do not confirm peer receipt.
//! It is a partial protocol implementation, not proof of native BaseClient
//! connection or Team Create acceptance.
//!
//! This module is pure logic: it receives raw stream bytes and emits
//! [`Action`]s. The QUIC session (`team_create.rs`) executes the actions
//! (open streams, write bytes, flush datagrams) and feeds received data
//! back in.
//!
//! Three planes sit behind opt-in switches here, because a session that has
//! only completed a handshake and a burst cannot say what any of them should
//! do on the wire:
//!
//! * *Join channels* — [`crate::rbx_channels`]: the fourteen-row profile, the
//!   staged `openSendChannel` records, and the reconciliation of our opens with
//!   the peer's announcements. [`ChannelRouter`] is fed by the same control
//!   records the flow already parses.
//! * *Replication* — [`crate::rbx_replication`]: channel-1 (and any reliable
//!   channel) bytes are walked as framed messages into a
//!   [`ReplicationStream`], which decodes the snapshot schema, the
//!   `JoinDataItemV2` batches and the live delta chains, and can materialize a
//!   place the editor opens.
//! * *Syncing* — [`crate::rbx_sync`]: the flow-control/heartbeat block, the
//!   reference client's ping and sync periods, and the coalescing queue of
//!   outbound property changes.
//!
//! All three are off by default, so enabling the crate changes no bytes: each
//! one is listed in the session report together with what it did or refused to
//! do.

use crate::challenge;
use crate::rbx_channels::{self, ChannelOpenMode, ChannelRouter, DEFAULT_CHANNEL_PROFILE};
use crate::rbx_replication::{
    join_tag_label, ReplicationNote, ReplicationStream, MARKER_ITEM,
};
use crate::rbx_sync::{LivenessTimers, SyncEngine};
use std::collections::HashMap;

/// BaseClient join application id on the RbxTransport multiplexer.
pub const APP: u32 = 4;
/// Dummy (health-check) application id used by the native client.
#[allow(dead_code)]
pub const APP_DUMMY: u32 = 6;
/// Control channel: the ASCII "ctrl" as a big-endian u32.
pub const CTRL_CHANNEL: u32 = 0x6374_726C;

/// App id written into the ctrl stream header.
///
/// Native 0x90 message template (10 432 B capture). The JSON region is
/// patched with this session's values; the leading bytes and the final 20
/// bytes are preserved verbatim.
const TEMPLATE_90: &[u8] = include_bytes!("../data/appflow/rbx90_template.bin");
/// The real 99-byte 0xA7 client-auth frame captured from a native client.
const A7_REAL: &[u8] = include_bytes!("../data/appflow/rbxa7_real.bin");

/// Route declarations scheduled on channel 11 about 60 ms after local
/// challenge solving; this client-side timer does not imply peer receipt.
const ROUTES: &[&str] = &[
    "a60101000114000000140000000000",
    "a60201000118000000180000000000040400",
    "a603010101001c0000001c0000000000040400040400",
    "a60401000120000000200000000000040400040400040400",
];

/// 0xA7 client-auth variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A7Mode {
    /// Send the captured 99-byte real A7 frame.
    Real,
    /// Send the 3-byte empty A7 (`a7 00 00`).
    Empty,
    /// Send no A7 at all (some native captures skip it).
    Skip,
}

/// Runtime override for which 0xA7 the join flow sends.
///
/// The real mode replays the captured 99-byte frame; the other modes send the
/// empty frame or omit it. This enum describes client behavior only; it does not
/// establish server requirements or interpret the frame's semantics.
static A7_MODE_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Choose which 0xA7 variant subsequent join flows send.
pub fn set_a7_mode(mode: A7Mode) {
    A7_MODE_OVERRIDE.store(mode as u8, std::sync::atomic::Ordering::Relaxed);
}

/// The currently selected 0xA7 variant. Defaults to `Real`.
pub fn a7_mode() -> A7Mode {
    match A7_MODE_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => A7Mode::Empty,
        2 => A7Mode::Skip,
        _ => A7Mode::Real,
    }
}

/// A one-line description of a mode, for the UI and the session log.
pub fn a7_mode_label(mode: A7Mode) -> &'static str {
    match mode {
        A7Mode::Real => "real 99-byte capture",
        A7Mode::Empty => "empty a7 00 00",
        A7Mode::Skip => "omitted",
    }
}

/// Application byte configured for the control-stream header. The default is
/// zero; the UI can select 0, 1, or 4 for a flow attempt. This setting is
/// independent of [`APP`], which identifies the join-data application streams.
static CTRL_APP_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Choose the control stream's application byte for subsequent join flows.
pub fn set_ctrl_app(app: u8) {
    CTRL_APP_OVERRIDE.store(app, std::sync::atomic::Ordering::Relaxed);
}

/// The control stream's application byte. Defaults to 0.
pub fn ctrl_app() -> u32 {
    CTRL_APP_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) as u32
}

/// Which of the three post-join planes a flow runs. These are separate switches
/// on purpose: a session that decodes replication is not a session that sends
/// liveness traffic, and a report that conflates them cannot be compared with a
/// previous run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JoinPlanes {
    /// Reconcile channel opens against the fourteen-row profile.
    pub channels: bool,
    /// Decode the snapshot schema, batches and delta chains.
    pub replication: bool,
    /// Send flow control, the reference client's liveness periods, and any
    /// queued outbound property changes.
    pub sync: bool,
}

impl Default for JoinPlanes {
    fn default() -> Self {
        Self { channels: false, replication: false, sync: false }
    }
}

impl JoinPlanes {
    fn bits(self) -> u8 {
        u8::from(self.channels) | (u8::from(self.replication) << 1) | (u8::from(self.sync) << 2)
    }

    /// One line naming what is on, for the report and the UI.
    pub fn label(self) -> String {
        let mut on = Vec::new();
        if self.channels {
            on.push(format!(
                "channels ({})",
                rbx_channels::channel_open_mode_label(rbx_channels::channel_open_mode())
            ));
        }
        if self.replication {
            on.push("replication decode".to_string());
        }
        if self.sync {
            on.push("outbound liveness/sync".to_string());
        }
        if on.is_empty() {
            "none (handshake and burst only)".to_string()
        } else {
            on.join(", ")
        }
    }
}

static JOIN_PLANES: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Select which post-join planes subsequent flows run.
pub fn set_join_planes(planes: JoinPlanes) {
    JOIN_PLANES.store(planes.bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The configured post-join planes. Defaults to all off.
pub fn join_planes() -> JoinPlanes {
    let bits = JOIN_PLANES.load(std::sync::atomic::Ordering::Relaxed);
    JoinPlanes {
        channels: bits & 1 != 0,
        replication: bits & 2 != 0,
        sync: bits & 4 != 0,
    }
}

/// The unframed route declaration payload for one of the four routes, exposed
/// so [`crate::rbx_sync`] can splice control blocks and step counters onto the
/// same bytes the flow sends.
pub fn route_template(index: usize) -> Vec<u8> {
    ROUTES.get(index).map(|route| hex(route)).unwrap_or_default()
}

/// Decoded early-auth material (the base64 fields 2 and 3 of the
/// `ClientTicket` split on semicolons, plus the trailing version).
#[derive(Clone, Debug)]
pub struct EarlyAuth {
    pub version: u8,
    pub pre: Vec<u8>,
    pub auth: Vec<u8>,
}

/// Everything the flow needs from the gamejoin response.
#[derive(Clone, Debug)]
pub struct JoinConfig {
    pub user_id: i64,
    /// Account name / display name / domain id from the gamejoin response.
    /// `None` when the response did not carry them, in which case the 0x90
    /// keeps the captured template's value and says so in the report.
    pub user_name: Option<String>,
    pub display_name: Option<String>,
    pub domain_user_id: Option<i64>,
    pub client_ticket: String,
    pub session_id: String,
    pub random_seed1: Option<String>,
    pub api_security_token: Option<String>,
    /// Kept as parsed JSON rather than a string: a live Team Create config
    /// carried `EncryptedServerFields` as a non-string value, and requiring
    /// `as_str()` silently dropped it, which cost the whole 0x90.
    pub serialized_client_fields: Option<serde_json::Value>,
    pub encrypted_server_fields: Option<serde_json::Value>,
    pub early_auth: Option<EarlyAuth>,
    pub a7_mode: A7Mode,
}

/// An action for the QUIC session to execute.
#[derive(Debug)]
pub enum Action {
    /// Open a new client-initiated bidirectional stream carrying `data`
    /// (which must include the 7-byte RbxTransport stream header).
    OpenStream { app: u32, chan: u32, data: Vec<u8> },
    /// Write more bytes to an already-opened stream (raw stream id).
    Write { stream: i64, data: Vec<u8> },
}

/// Flow-level notifications for the UI/session log.
#[derive(Clone, Debug)]
pub enum FlowEvent {
    /// Generic status line.
    Log(String),
    /// A complete 0x9B challenge frame was received on channel 1.
    ChallengeReceived { u1: u32, u2: u32, blob_len: u32 },
    /// The challenge was solved locally and a response action was generated.
    /// This event does not report write success, peer receipt, or acceptance.
    ChallengeAnswered { answer: u32, elapsed_ms: u64 },
    /// Parsing the received 0x9B frame or solving its challenge failed.
    ChallengeFailed(String),
    /// Channel-1 traffic arrived after a 0x9B frame was seen. This is a local
    /// diagnostic milestone, not confirmation of native BaseClient connection.
    ConnectedStageReached { detail: String },
    /// A channel open, a peer announcement, or the accounting that followed.
    /// Carries a sentence, not a claim that the channel was accepted.
    ChannelUpdate(String),
    /// One decoded replication note: a schema, a batch, a delta chain, or the
    /// reason a message was left unparsed.
    ReplicationStatus(String),
    /// The join gate moved. `stage` names the gate (`marker-ack` when the
    /// client has answered a snapshot-done marker, `join-data-done` when tag 13
    /// closed the snapshot); `detail` carries the id or tag. Acking is local —
    /// it does not claim the server received or accepted the ack.
    JoinStateChanged { stage: String, detail: String },
    /// A replicated place was materialized from the snapshot. `summary` says
    /// what the applier could and could not type; this is not a statement that
    /// the session is a live Team Create client.
    SnapshotReady { bytes: Vec<u8>, summary: String },
}

fn hex(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len() / 2);
    for chunk in b.chunks_exact(2) {
        let hi = (chunk[0] as char)
            .to_digit(16)
            .expect("route hex: high nibble");
        let lo = (chunk[1] as char)
            .to_digit(16)
            .expect("route hex: low nibble");
        out.push((hi * 16 + lo) as u8);
    }
    out
}

/// Unsigned LEB128 (the "inner-field" varint used inside 0x8A/0x92/0x90).
pub fn leb128(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        } else {
            out.push(b | 0x80);
        }
    }
    out
}

/// Decode unsigned LEB128; returns (value, bytes_consumed).
#[allow(dead_code)]
pub fn leb128_decode(data: &[u8]) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for (i, &b) in data.iter().enumerate() {
        // A u64 takes up to 10 bytes, and the tenth carries a single bit. The
        // old bound of 8 rejected exactly the wide values a zigzagged 0x92
        // nonce produces, which is why its layout test never passed.
        if i >= 10 {
            return None;
        }
        let shift = 7 * i;
        let bits = u64::from(b & 0x7F);
        if shift == 63 && bits > 1 {
            return None;
        }
        v |= bits << shift;
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

/// RobloxTransport compact varint: size fixed by the top two bits
/// (1/2/4/8 bytes), value = (b0 & 0x3F) followed by n-1 raw big-endian
/// bytes (as in Boblox util.h `compactVarint` / the native transport).
pub fn compact_varint(v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    if v < 0x40 {
        out.push(v as u8);
    } else if v < 0x4000 {
        out.push(0x40 | (v >> 8) as u8);
        out.push((v & 0xFF) as u8);
    } else if v < 0x4000_0000 {
        out.push(0x80 | (v >> 24) as u8);
        out.push((v >> 16) as u8);
        out.push((v >> 8) as u8);
        out.push((v & 0xFF) as u8);
    } else {
        // The 8-byte form puts six bits in the tag byte and then the remaining
        // seven bytes, most significant first. The shift list is written out
        // because `(8..56).rev().step_by(8)` steps the *reversed* sequence and
        // lands on 55, 47, 39 - which silently misaligns every frame over
        // 2^56 bytes and, with the 4-byte bound above set to 2^38, mis-encoded
        // 2^30..2^32 as an 8-byte varint.
        out.push(0xC0 | (v >> 56) as u8);
        for shift in [48u32, 40, 32, 24, 16, 8, 0] {
            out.push((v >> shift) as u8);
        }
    }
    out
}

/// Inverse of [`compact_varint`]: returns the value and how many bytes it
/// occupied, or `None` if the buffer does not hold a complete varint.
pub fn compact_varint_decode(buf: &[u8]) -> Option<(u64, usize)> {
    let &b0 = buf.first()?;
    let extra = match b0 >> 6 {
        0 => 0usize,
        1 => 1,
        2 => 3,
        _ => 7,
    };
    if buf.len() < 1 + extra {
        return None;
    }
    let mut value = u64::from(b0 & 0x3F);
    for byte in &buf[1..1 + extra] {
        value = (value << 8) | u64::from(*byte);
    }
    Some((value, 1 + extra))
}

/// Big-endian u32 from a four-byte slice.
fn be32(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) << 24
        | u32::from(bytes[1]) << 16
        | u32::from(bytes[2]) << 8
        | u32::from(bytes[3])
}

/// Frame a payload: `compactVarint(len) + payload`.
pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = compact_varint(payload.len() as u64);
    out.extend_from_slice(payload);
    out
}

/// Stream header: `[06 01 app][channel u32 BE]`.
pub fn stream_header(app: u32, chan: u32) -> [u8; 7] {
    [0x06, 0x01, app as u8, (chan >> 24) as u8, (chan >> 16) as u8, (chan >> 8) as u8, chan as u8]
}

/// Standard-base64 decode with Boblox's `a2b_base64` semantics: every
/// character outside the alphabet is ignored (no padding validation).
#[allow(dead_code)]
pub fn b64_decode(text: &str) -> Vec<u8> {
    fn val(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a' + 26) as u32),
            b'0'..=b'9' => Some((b - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut bits: u32 = 0;
    let mut n = 0u32;
    for &b in text.as_bytes() {
        let Some(v) = val(b) else {
            continue;
        };
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1u32 << n) - 1;
        }
    }
    out
}

/// `v31 = obfuscated(xxh32(ClientTicket, seed=1))` — the OLLVM-folded
/// expression from session.cpp, ported verbatim (the compiler folds the
/// constant branch chain to the same few lines).
pub fn ticket_v31(ticket: &[u8]) -> u32 {
    const S: u32 = 0x7137_5635;
    let v7 = challenge::xxh32(ticket, 1);
    let v8 = (0u32.wrapping_sub(17506u32.wrapping_mul(S))) & 0xFFFF;
    let v9 = if v8 & 2 != 0 { 7 } else { 25 };
    let v12 = if v8 & 4 != 0 { 0u32.wrapping_sub(S) } else { 1434170839 };
    let v13 = v7
        .wrapping_add(1434170839)
        .rotate_left(v9)
        .wrapping_add(v12);
    let v14 = if v8 & 8 != 0 { S } else { 1434170839 };
    let v15 = if v8 & 0x20 != 0 { S } else { 1434170839 };
    let v16 = if v8 & 0x40 != 0 { S } else { 1434170839 };
    let v17 = if v8 & 0x4000 != 0 { S } else { 1434170839 };
    let v18 = v13.wrapping_mul(v14);
    let v19 = if v8 & 0x10 != 0 { 13 } else { 19 };
    let v20 = (v16 ^ v15.wrapping_sub(v18.rotate_left(v19)))
        .rotate_left(2 * ((v8 & 0xFF) >> 7) + 15);
    let v11 = if v8 & 0x100 != 0 { S } else { 0u32.wrapping_sub(S) };
    let v21 = v11.wrapping_add(1434170839);
    let mut v22 = v11.wrapping_sub(1434170839);
    if v8 & 0x200 != 0 {
        v22 = v21;
    }
    let v23 = v20.wrapping_add(v22);
    let v24 = if v8 & 0x400 != 0 { 23 } else { 9 };
    let v25 = v23.rotate_left(v24);
    let v26 = if v8 & 0x800 != 0 { 0u32.wrapping_sub(v25) } else { v25 };
    let v27 = S.wrapping_add(v26);
    let v28 = if v8 & 0x1000 != 0 { 0u32.wrapping_sub(v27) } else { v27 };
    let v29 = v28.wrapping_add(1434170839);
    let v30 = if v8 & 0x2000 != 0 { 29 } else { 3 };
    v17 ^ v29.rotate_left(v30)
}

/// 0xA8 early-auth: `[a8][version][pre len][pre][auth len][auth]`.
pub fn build_early_auth(early: &EarlyAuth) -> Vec<u8> {
    let mut out = vec![0xA8, early.version];
    out.push(early.pre.len() as u8);
    out.extend_from_slice(&early.pre);
    out.push(early.auth.len() as u8);
    out.extend_from_slice(&early.auth);
    out
}

/// Control "open unreliable channel" frame: `[02][app][chan u32 BE][wire u32 BE]`.
pub fn build_open_unreliable(app: u32, chan: u32, wire: u32) -> Vec<u8> {
    vec![
        2,
        app as u8,
        (chan >> 24) as u8,
        (chan >> 16) as u8,
        (chan >> 8) as u8,
        chan as u8,
        (wire >> 24) as u8,
        (wire >> 16) as u8,
        (wire >> 8) as u8,
        wire as u8,
    ]
}

/// 0x92 nonce message: `[92] LEB(zigzag((a << 32) | (b ^ a)))`.
///
/// Read out of 0.741's builder, which draws **two independent** 32-bit values and
/// writes `CONCAT44(a, b ^ a)`: one from a PCG-style PRNG
/// (`state * 0x5851_f42d_4c95_7f2d + 0x69`, output rotated by a byte of the
/// state) and one from a GF(2) bit-mixer seeded off a global. The zigzag and
/// LEB128 match too - the writer does `x * 2 ^ (x >> 63)`.
///
/// The port this replaces used a single value XORed with `0x63E2_5F26`. That
/// constant appears nowhere in the 219 MB binary and was never a constant: it is
/// the second value from one capture. The consequence is that `low ^ high` came
/// out as the same 32 bits on every single join, where the real client's differs
/// every time - the exact shape of thing an anti-replay or entropy check keys on,
/// and the server's stated reason for closing is a security-key mismatch.
pub fn build_92(a: u32, b: u32) -> Vec<u8> {
    let x = ((a as u64) << 32) | ((b ^ a) as u64);
    let mut zz = x << 1;
    if x >> 63 != 0 {
        zz = !zz;
    }
    let mut out = vec![0x92];
    out.extend_from_slice(&leb128(zz));
    out
}

/// 0x8A device/session message — port of session.cpp `build8A`.
pub fn build_8a(cfg: &JoinConfig) -> Vec<u8> {
    const CONST74: &str =
        "2e427f51c4dab762fe9e3471c6cfa1650841723b!6e8e47e92778f00efbb13c7bb151ea88.";
    let ticket = cfg.client_ticket.as_bytes();
    let v31 = ticket_v31(ticket);
    let zz = (cfg.user_id as u64) << 1;
    let mut out = vec![0x8A];
    out.extend_from_slice(&leb128(zz));
    out.extend_from_slice(&leb128(ticket.len() as u64));
    out.extend_from_slice(ticket);
    out.extend_from_slice(&36u32.to_le_bytes());
    out.extend_from_slice(&leb128(74));
    out.extend_from_slice(CONST74.as_bytes());
    out.extend_from_slice(&leb128(7));
    out.extend_from_slice(b"Android");
    out.push(1);
    out.push(b'?');
    out.extend_from_slice(&leb128(v31 as u64));
    out.extend_from_slice(&leb128(v31.wrapping_sub(0x0BAD_F00D) as u64));
    out.push(0x00);
    out.extend_from_slice(&leb128(cfg.session_id.len() as u64));
    out.extend_from_slice(cfg.session_id.as_bytes());
    out.extend_from_slice(&[0xFE, 0xCA, 0x01, 0xC0]);
    out
}

/// A built 0x90 plus an account of how much of it is this session's.
///
/// The 0x90 is a captured template, so every key this struct does not claim to
/// have refreshed still holds the captured account's value. Reporting the split
/// is what keeps a stale identity field from looking like a working join.
pub struct Build90 {
    /// The frame bytes.
    pub bytes: Vec<u8>,
    /// Template keys replaced with this session's values.
    pub refreshed: Vec<String>,
    /// Template keys still carrying the captured account's values.
    pub frozen: Vec<String>,
    /// Identity keys the gamejoin response did not carry at all, so nothing
    /// could replace them. Distinct from `frozen`, which also covers keys whose
    /// live value merely equalled the capture.
    pub missing_identity: Vec<String>,
}

/// 0x90 join-config message: the native capture template with the JSON
/// region replaced by this session's `RandomSeed1` / `APIsecurityToken` /
/// `__joinTicket` (a JSON string of the two joinTicket fields), the LEB
/// length rewritten, and the final 20 bytes preserved. Port of
/// session.cpp `build90`.
///
/// The template's own identity fields are rewritten too. They used to be left
/// alone, which made the burst self-contradictory: 0x8A carries
/// `cfg.user_id` while 0x90 carried the captured account's, so the two frames
/// named different users.
pub fn build_90(cfg: &JoinConfig) -> Result<Build90, String> {
    if cfg.random_seed1.is_none() || cfg.api_security_token.is_none() {
        return Err("0x90 needs joinScript.RandomSeed1 / APIsecurityToken".into());
    }
    if cfg.serialized_client_fields.is_none() || cfg.encrypted_server_fields.is_none() {
        return Err("0x90 needs joinTicket SerializedClientFields / EncryptedServerFields".into());
    }
    let raw = TEMPLATE_90;
    let i = raw
        .windows(9)
        .position(|w| w == b"{\"UserId\"")
        .ok_or("0x90 template: JSON marker not found")?;
    if raw.len() < 20 || i >= raw.len() - 20 {
        return Err("0x90 template: too small".into());
    }
    // Walk back over the LEB length that precedes the JSON.
    let mut start = i.saturating_sub(1);
    while start > 0 && raw[start - 1] & 0x80 != 0 {
        start -= 1;
    }
    let template_json =
        std::str::from_utf8(&raw[i..raw.len() - 20]).map_err(|_| "0x90 template: bad JSON")?;
    let mut tmpl: serde_json::Map<String, serde_json::Value> = serde_json::from_str(template_json)
        .map_err(|e| format!("0x90 template: JSON parse failed: {e}"))?;
    // Snapshotted before ANY substitution. Taking this later - which is what the
    // first version did - folded RandomSeed1, APIsecurityToken and __joinTicket
    // into the baseline, so three fields that were always replaced read as
    // "still frozen from the capture".
    let pristine: serde_json::Map<String, serde_json::Value> = tmpl.clone();

    tmpl.insert(
        "RandomSeed1".into(),
        serde_json::Value::String(cfg.random_seed1.clone().unwrap()),
    );
    tmpl.insert(
        "APIsecurityToken".into(),
        serde_json::Value::String(cfg.api_security_token.clone().unwrap()),
    );
    let ticket = serde_json::json!({
        "SerializedClientFields": cfg.serialized_client_fields.clone().unwrap(),
        "EncryptedServerFields": cfg.encrypted_server_fields.clone().unwrap(),
    });
    tmpl.insert(
        "__joinTicket".into(),
        serde_json::Value::String(
            serde_json::to_string(&ticket).map_err(|e| format!("0x90 ticket: {e}"))?,
        ),
    );

    for (key, value) in [
        ("UserName", cfg.user_name.as_ref().map(|v| serde_json::Value::from(v.clone()))),
        (
            "DisplayName",
            cfg.display_name.as_ref().map(|v| serde_json::Value::from(v.clone())),
        ),
        (
            "DomainUserId",
            cfg.domain_user_id.map(serde_json::Value::from),
        ),
    ] {
        if let Some(value) = value {
            tmpl.insert(key.into(), value);
        }
    }

    // The template froze one account's identity. Rewrite the parts this
    // session can source, and say which parts it could not.
    tmpl.insert("UserId".into(), serde_json::Value::from(cfg.user_id));
    tmpl.insert(
        "CharacterAppearanceId".into(),
        serde_json::Value::from(cfg.user_id),
    );
    // The avatar-fetch URL names the user in a query parameter, not a field.
    if let Some(serde_json::Value::String(url)) = tmpl.get("CharacterAppearance") {
        let url = url.clone();
        if let Some(at) = url.find("userId=") {
            let from = at + "userId=".len();
            let to = url[from..]
                .find(|c: char| !c.is_ascii_digit())
                .map_or(url.len(), |i| from + i);
            tmpl.insert(
                "CharacterAppearance".into(),
                serde_json::Value::String(format!("{}{}{}", &url[..from], cfg.user_id, &url[to..])),
            );
        }
    }

    // The live join ticket is base64-encoded JSON and carries the same account
    // fields the template froze. It arrives either as that string or, on some
    // configs, already decoded into an object.
    let ticket_fields: Option<serde_json::Map<String, serde_json::Value>> =
        match cfg.serialized_client_fields.as_ref() {
            Some(serde_json::Value::String(text)) => {
                serde_json::from_slice(&b64_decode(text)).ok()
            }
            Some(serde_json::Value::Object(map)) => Some(map.clone()),
            _ => None,
        };
    if let Some(fields) = ticket_fields {
        // (SerializedClientFields key, the 0x90 key it feeds)
        for (src, dst) in [
            ("AccountAge", "AccountAge"),
            ("CountryCode", "CountryRegionCode"),
            ("MembershipType", "MembershipType"),
            ("HasVerifiedBadge", "HasVerifiedBadge"),
            ("GameLocale", "LocaleId"),
            ("GameLocale", "LocaleIdInternal"),
        ] {
            if let Some(value) = fields.get(src) {
                tmpl.insert(dst.into(), value.clone());
            }
        }
    }

    let refreshed: Vec<String> = pristine
        .keys()
        .filter(|key| tmpl.get(*key) != pristine.get(*key))
        .cloned()
        .collect();
    let frozen: Vec<String> = pristine
        .keys()
        .filter(|key| tmpl.get(*key) == pristine.get(*key))
        .cloned()
        .collect();
    // A field can be untouched either because nothing could source it or because
    // the live value happened to match the capture. Those need telling apart, so
    // name the identity fields the gamejoin response did not carry.
    let missing_identity: Vec<String> = [
        ("UserName", cfg.user_name.is_some()),
        ("DisplayName", cfg.display_name.is_some()),
        ("DomainUserId", cfg.domain_user_id.is_some()),
    ]
    .into_iter()
    .filter(|(_, present)| !present)
    .map(|(key, _)| key.to_owned())
    .collect();

    let body = serde_json::to_string(&tmpl).map_err(|e| format!("0x90 serialize: {e}"))?;
    let mut out = raw[..start].to_vec();
    out.extend_from_slice(&leb128(body.len() as u64));
    out.extend_from_slice(body.as_bytes());
    out.extend_from_slice(&raw[raw.len() - 20..]);
    Ok(Build90 {
        bytes: out,
        refreshed,
        frozen,
        missing_identity,
    })
}

fn hex32(v: u32) -> String {
    format!("{v:08x}")
}

/// The whole flow state machine.
pub struct AppFlow {
    cfg: JoinConfig,
    /// (app, chan) -> raw stream id for client-opened streams.
    streams: HashMap<(u32, u32), i64>,
    /// server-opened stream id -> (app, chan) once its header parsed.
    server_streams: HashMap<i64, (u32, u32)>,
    /// Partial 7-byte headers for server-opened streams not yet identified.
    server_hdr: HashMap<i64, Vec<u8>>,
    chan1: Option<i64>,
    chan1_rx: Vec<u8>,
    chan1_reset: bool,
    challenge_seen: bool,
    answered: bool,
    /// How far the marker walk has consumed `chan1_rx`.
    marker_off: usize,
    marker_seen: bool,
    marker_id: u32,
    markers_acked: u64,
    /// Marker ids already answered with a `0x04` ack (bare or in-run shape).
    /// Top-level `0x84` frames are acked per frame like the reference; the
    /// `0x04` shapes are acked once per id so an echo of our own ack - the
    /// same bytes the server accepts as the reply - cannot loop.
    acked_marker_ids: std::collections::BTreeSet<u32>,
    /// `0x04` marker items answered from inside `0x83` item runs.
    marker_items_acked: u64,
    /// Whether the join gate has been taken: a snapshot-done marker was
    /// answered. The server continues the join on that ack.
    join_gate_taken: bool,
    routes_at_ms: i64,
    // Channel-1 bytes received after the flow has seen a 0x9B frame.
    post_answer_bytes: u64,
    post_answer_reported: bool,
    now_ms: u64,
    events: Vec<FlowEvent>,
    /// Last drain-loop outcome, for the session report.
    drain_summary: String,
    /// Which 0x90 template fields were refreshed and which are still captured.
    join90_report: String,
    /// Partial control records per stream, since a record can straddle chunks.
    ctrl_rx: HashMap<i64, Vec<u8>>,
    /// Channel opens the peer announced: (app, channel, wire id if unreliable).
    channel_opens: Vec<(u32, u32, Option<u32>)>,
    /// Stream writes ngtcp2 could not take yet, to be offered again from the
    /// receive loop. `Blocked` means "try again later", not "finished", and the
    /// window only reopens once the peer's frames have been processed - which
    /// only happens there.
    pending: std::collections::VecDeque<(i64, Vec<u8>)>,
    /// Pieces of the burst that were left out, and why. These used to reach
    /// only a Status event, so a session that silently dropped the 0x90 read
    /// exactly like one that sent it.
    skipped: Vec<String>,
    /// Which of the post-join planes this flow runs, captured at construction so
    /// a report describes the session it belongs to.
    planes: JoinPlanes,
    /// Join-channel state: profile, staged opens, peer announcements, bytes.
    channels: ChannelRouter,
    replication: ReplicationStream,
    liveness: LivenessTimers,
    sync: SyncEngine,
    /// Editor edits waiting to be addressed. This is the seam between the place the
    /// editor is showing and this session's outbound queue, held behind a lock so a
    /// caller that is not the flow can offer one; the flow is the only thing that
    /// takes them out.
    edits: std::sync::Arc<std::sync::Mutex<Vec<crate::rbx_sync::SyncEdit>>>,
    /// How many offered edits this flow has taken out of that outbox, whether or
    /// not they could be addressed.
    edits_offered: u64,
    /// A snapshot is emitted once, however many times the apply is attempted.
    snapshot_emitted: bool,
    /// Whether the "nothing decoded to apply" line has been said. It is said
    /// once and the apply is still retried, because a schema can arrive after
    /// the marker that first asked for the place.
    snapshot_empty_reported: bool,
    /// Channels this client asked to open but that no stream id is bound to yet.
    unbound_open: std::collections::BTreeSet<u32>,
}

impl AppFlow {
    pub fn new(cfg: JoinConfig, now_ms: u64) -> Self {
        Self {
            cfg,
            streams: HashMap::new(),
            server_streams: HashMap::new(),
            server_hdr: HashMap::new(),
            chan1: None,
            chan1_rx: Vec::new(),
            chan1_reset: false,
            challenge_seen: false,
            marker_off: 0,
            marker_seen: false,
            marker_id: 0,
            markers_acked: 0,
            acked_marker_ids: std::collections::BTreeSet::new(),
            marker_items_acked: 0,
            join_gate_taken: false,
            skipped: Vec::new(),
            pending: std::collections::VecDeque::new(),
            drain_summary: String::new(),
            join90_report: String::new(),
            ctrl_rx: HashMap::new(),
            channel_opens: Vec::new(),
            answered: false,
            routes_at_ms: -1,
            post_answer_bytes: 0,
            post_answer_reported: false,
            now_ms,
            events: Vec::new(),
            planes: join_planes(),
            channels: ChannelRouter::new(APP, &DEFAULT_CHANNEL_PROFILE),
            replication: ReplicationStream::default(),
            liveness: LivenessTimers::new(now_ms),
            sync: SyncEngine::new(now_ms, 1000),
            edits: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            edits_offered: 0,
            snapshot_emitted: false,
            snapshot_empty_reported: false,
            unbound_open: std::collections::BTreeSet::new(),
        }
    }

    /// Whether a valid challenge was solved locally and a response action built.
    /// This does not confirm that the peer received or accepted the response.
    pub fn answered(&self) -> bool {
        self.answered
    }

    /// Channel-1 bytes received after a 0x9B frame was seen (legacy name).
    pub fn post_answer_bytes(&self) -> u64 {
        self.post_answer_bytes
    }

    pub fn drain_events(&mut self) -> Vec<FlowEvent> {
        std::mem::take(&mut self.events)
    }

    fn log(&mut self, msg: String) {
        self.events.push(FlowEvent::Log(msg));
    }

    /// The post-handshake burst, in native order.
    pub fn on_connected(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();

        // ctrl stream: hello, then the four openU frames. They land on one
        // QUIC stream, so a single write is byte-identical to the native
        // per-frame sends.
        // The reference client opens the control stream on the *application* id,
        // not on app 0: `openStream(APP, CTRL)`. The server itself opens control
        // streams under both ids, so app 0 parsed fine and hid the difference -
        // but the bytes on the wire were not the ones a working client sends.
        let ctrl_app = ctrl_app();
        let mut ctrl = stream_header(ctrl_app, CTRL_CHANNEL).to_vec();
        ctrl.extend_from_slice(&frame(&[0x00, 0x00]));
        if self.planes.channels {
            // The fourteen-row profile drives the open records. In the reference
            // mode this is byte-identical to the hard-coded list in the `else`
            // arm - a test pins that equality - so the only mode that changes
            // the wire is `FullProfile`, which also opens the reliable rows.
            let bodies = self.channels.stage_opens(rbx_channels::channel_open_mode(), APP as u8);
            for body in &bodies {
                ctrl.extend_from_slice(&frame(body));
            }
            self.log(format!(
                "channel router staged {} open record(s), mode {}",
                bodies.len(),
                rbx_channels::channel_open_mode_label(rbx_channels::channel_open_mode())
            ));
        } else {
            for &(chan, wire) in &[(3u32, 2u32), (5, 4), (6, 6), (11, 8)] {
                ctrl.extend_from_slice(&frame(&build_open_unreliable(APP, chan, wire)));
            }
        }
        // Log the header verbatim: whether the server routes on app 0 or app 4
        // is unresolved, and the only way to compare against a capture is to
        // see the exact seven bytes we put on the wire.
        self.events.push(FlowEvent::Log(format!(
            "ctrl stream header: {} (application {})",
            stream_header(ctrl_app, CTRL_CHANNEL)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" "),
            ctrl_app
        )));
        actions.push(Action::OpenStream {
            app: ctrl_app,
            chan: CTRL_CHANNEL,
            data: ctrl,
        });

        // chan1: early-auth, A7, 0x90, 0x92, 0x8A, 0x8F.
        let mut one = stream_header(APP, 1).to_vec();
        if let Some(early) = &self.cfg.early_auth {
            one.extend_from_slice(&frame(&build_early_auth(early)));
            self.log(format!("early-auth staged ({}B payload)", early.pre.len() + early.auth.len()));
        } else {
            let why = "early-auth material missing — 0xA8 not sent".to_string();
            self.skipped.push(why.clone());
            self.log(why);
        }
        match self.cfg.a7_mode {
            A7Mode::Real => one.extend_from_slice(&frame(A7_REAL)),
            A7Mode::Empty => one.extend_from_slice(&frame(&[0xA7, 0x00, 0x00])),
            A7Mode::Skip => {}
        }
        let m90 = match build_90(&self.cfg) {
            Ok(built) => {
                self.join90_report = format!(
                    "refreshed from this session [{}]; not carried by the gamejoin response [{}]; unchanged [{}]",
                    built.refreshed.join(", "),
                    built.missing_identity.join(", "),
                    built.frozen.join(", ")
                );
                self.log(format!("0x90 {}", self.join90_report));
                Some(built.bytes)
            }
            Err(e) => {
                let why = format!("0x90 not sent: {e}");
                self.skipped.push(why.clone());
                self.log(why);
                None
            }
        };
        // 0.741 draws two independent values for the 0x92. Sharing one seed is
        // what pinned `low ^ high` to a constant.
        let mut seed_a =
            (self.now_ms.wrapping_mul(0x9E37_79B1).wrapping_add(0x51_7C_C1_B7)) as u32;
        let mut seed_b =
            (self.now_ms.wrapping_mul(0x85EB_CA6B).wrapping_add(0x27D4_EB2F)) as u32;
        if seed_a == 0 {
            seed_a = 0x8000_0001;
        }
        if seed_b == 0 {
            seed_b = 0x8000_0001;
        }
        if let Some(m90) = &m90 {
            one.extend_from_slice(&frame(m90));
        }
        one.extend_from_slice(&frame(&build_92(seed_a, seed_b)));
        one.extend_from_slice(&frame(&build_8a(&self.cfg)));
        one.extend_from_slice(&frame(&[0x8F, 0x00]));
        actions.push(Action::OpenStream {
            app: APP,
            chan: 1,
            data: one,
        });

        if self.planes.sync {
            // The reference client sends its first sync and ping immediately
            // after connecting, then on their periods. These are the only
            // outbound frames this flow adds unbidden, and only when the sync
            // plane is on.
            let now_ms = self.now_ms;
            self.liveness.set_enabled(true, now_ms);
            self.sync.set_enabled(true, now_ms);
            for tick in self.liveness.initial() {
                self.send_on_channel(tick.channel, tick.payload, &mut actions, tick.name);
            }
        }
        self.log(format!(
            "on_connected: {} actions staged (0x90 {})",
            actions.len(),
            if m90.is_some() { "included" } else { "skipped" }
        ));
        actions
    }

    /// Put a framed payload on the stream that carries `chan`, opening that
    /// stream once if this client has not opened it yet.
    ///
    /// A stream this client asked for is not writable until the peer has bound an
    /// id to it, so those frames are counted as skipped rather than answered by a
    /// second `OpenStream` for the same channel — which is what a naive
    /// implementation does and what makes a session's byte count meaningless.
    fn send_on_channel(
        &mut self,
        chan: u32,
        payload: Vec<u8>,
        actions: &mut Vec<Action>,
        what: &str,
    ) {
        if payload.is_empty() {
            return;
        }
        let bound = self
            .streams
            .get(&(APP, chan))
            .copied()
            .or(if chan == 1 { self.valid_chan1() } else { None });
        if let Some(stream) = bound {
            actions.push(Action::Write { stream, data: payload });
            return;
        }
        if chan == 1 || chan == CTRL_CHANNEL || !self.unbound_open.insert(chan) {
            self.skipped.push(format!(
                "{what}: chan{chan} has no bound stream id yet ({} byte(s) not sent)",
                payload.len()
            ));
            return;
        }
        let mut data = stream_header(APP, chan).to_vec();
        data.extend_from_slice(&payload);
        let written = data.len();
        actions.push(Action::OpenStream { app: APP, chan, data });
        self.log(format!("{what}: opening app={APP} chan={chan} ({written}B)"));
    }

    /// Queue stream bytes that could not be written yet.
    pub fn queue_pending(&mut self, entry: (i64, Vec<u8>)) {
        if !entry.1.is_empty() {
            self.pending.push_back(entry);
        }
    }

    /// Take everything queued for retry.
    pub fn take_pending(&mut self) -> Vec<(i64, Vec<u8>)> {
        self.pending.drain(..).collect()
    }

    /// Bytes still waiting to be written.
    pub fn pending_bytes(&self) -> usize {
        self.pending.iter().map(|(_, data)| data.len()).sum()
    }

    /// Burst pieces that were left out, with reasons.
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// The same notes, consumed. The session report collects them twice - once
    /// after the initial actions and once after the receive loop - and reading the
    /// list rather than draining it printed every skipped frame twice.
    pub fn take_skipped(&mut self) -> Vec<String> {
        std::mem::take(&mut self.skipped)
    }

    /// The session tells us which raw stream id an OpenStream action got.
    pub fn note_opened(&mut self, app: u32, chan: u32, stream: i64) {
        if (app, chan) == (APP, 1) {
            self.chan1 = Some(stream);
            self.chan1_reset = false;
        }
        self.streams.insert((app, chan), stream);
    }

    /// Whether this stream is already identified (client-opened or a
    /// server stream whose header parsed).
    fn known_stream(&self, stream: i64) -> bool {
        self.chan1 == Some(stream)
            || self.server_streams.contains_key(&stream)
            || self.streams.values().any(|sid| *sid == stream)
    }

    /// Feed raw bytes for any stream. Unknown (server-opened) streams get
    /// their leading 7-byte RbxTransport header buffered first — mirroring
    /// Boblox's `hdr_buf_` handling. Returns actions (e.g. a re-opened
    /// chan1 answer) if the data completes a 0x9B challenge.
    pub fn on_raw_stream(&mut self, stream: i64, data: &[u8]) -> Vec<Action> {
        if data.is_empty() {
            return Vec::new();
        }
        if self.known_stream(stream) {
            return self.on_stream_data(stream, data);
        }
        let buf = self.server_hdr.entry(stream).or_default();
        buf.extend_from_slice(data);
        if buf.len() < 7 {
            return Vec::new();
        }
        if !buf.starts_with(&[0x06, 0x01]) {
            self.server_hdr.remove(&stream);
            self.log(format!("stream {stream}: unrecognized header — dropped"));
            return Vec::new();
        }
        let buf = std::mem::take(buf);
        self.server_hdr.remove(&stream);
        let app = buf[2] as u32;
        let chan = u32::from_be_bytes(buf[3..7].try_into().unwrap());
        self.on_server_stream_header(stream, app, chan);
        let mut actions = Vec::new();
        if buf.len() > 7 {
            actions.extend(self.on_stream_data(stream, &buf[7..]));
        }
        actions
    }

    /// A server-opened stream's 7-byte header parsed.
    pub fn on_server_stream_header(&mut self, stream: i64, app: u32, chan: u32) {
        self.server_streams.insert(stream, (app, chan));
        self.log(format!("server stream {stream}: app={app} chan={chan}"));
    }

    /// Feed raw stream bytes (stream header already identified by the
    /// session, or this is a known client stream).
    pub fn on_stream_data(&mut self, stream: i64, data: &[u8]) -> Vec<Action> {
        if data.is_empty() {
            return Vec::new();
        }
        let is_chan1 = self.chan1.map(|sid| sid == stream).unwrap_or(false);
        let (app, chan) = if is_chan1 {
            (APP, 1)
        } else {
            self.server_streams
                .get(&stream)
                .copied()
                .unwrap_or_else(|| self.streams_key(stream))
        };

        if (app, chan) == (APP, 1) {
            // The frame count comes from the replication walk, so the channel
            // report counts messages that were actually delimited rather than
            // assuming one frame per read.
            let mut frames = 0usize;
            if self.planes.replication {
                frames = self.replication.push(data);
            }
            if self.planes.channels {
                self.channels.note_join_stream(stream);
                self.channels.note_bytes(1, data.len(), frames);
            }
            self.chan1_rx.extend_from_slice(data);
            if self.chan1_rx.len() > 16 << 20 {
                // Challenge/marker parsing needs only a bounded tail; cap
                // receive-buffer growth in long-lived sessions.
                self.chan1_rx.drain(..(self.chan1_rx.len() - (4 << 20)));
            }
            if self.challenge_seen {
                self.post_answer_bytes += data.len() as u64;
                if !self.post_answer_reported {
                    self.post_answer_reported = true;
                    self.events.push(FlowEvent::ConnectedStageReached {
                        detail: format!("channel-1 traffic after the 0x9B frame: {}B", data.len()),
                    });
                }
            }
            let mut actions = self.try_marker();
            actions.extend(self.pump_join_items());
            actions.extend(self.try_challenge());
            actions
        } else {
            let mut frames = 0usize;
            if self.planes.replication && chan != CTRL_CHANNEL {
                // A reliable channel the peer opened carries replication data
                // too; the control channel carries channel announcements, and
                // framing those as replication messages would be a false read.
                frames = self.replication.push(data);
            }
            if self.planes.channels {
                self.channels.note_stream(chan, stream);
                self.channels.note_bytes(chan, data.len(), frames);
            }
            self.feed_ctrl(stream, app, chan, data);
            // Replication-carrying channels can hold marker items too; acking
            // them here is what lets a channel other than 1 carry the join
            // gate.
            self.pump_join_items()
        }
    }

    /// Walk a control stream's bytes as length-prefixed records. The peer
    /// announces its channel assignments here; dropping them is what left twelve
    /// opened channels unacknowledged.
    fn feed_ctrl(&mut self, stream: i64, app: u32, chan: u32, data: &[u8]) {
        // Taken out rather than borrowed through `entry`: `note_control_record`
        // needs `&mut self`, which an outstanding borrow of `ctrl_rx` forbids.
        let mut buf = self.ctrl_rx.remove(&stream).unwrap_or_default();
        buf.extend_from_slice(data);
        let mut off = 0usize;
        while off < buf.len() {
            let Some((len, used)) = compact_varint_decode(&buf[off..]) else {
                break; // partial length prefix
            };
            let end = off + used + len as usize;
            if len == 0 || end > buf.len() {
                break; // partial body: wait for more
            }
            let body = buf[off + used..end].to_vec();
            off = end;
            self.note_control_record(app, chan, &body);
        }
        buf.drain(..off);
        if !buf.is_empty() {
            self.ctrl_rx.insert(stream, buf);
        }
    }

    fn note_control_record(&mut self, app: u32, chan: u32, body: &[u8]) {
        match (body.first().copied(), body.len()) {
            // OpenReliable: type 1, app, channel - 6 bytes.
            (Some(1), 6) => {
                let peer_app = u32::from(body[1]);
                let peer_chan = be32(&body[2..6]);
                self.channel_opens.push((peer_app, peer_chan, None));
                self.log(format!(
                    "ctrl OpenReliable app={peer_app} chan={peer_chan} (on app={app} chan={chan})"
                ));
                if self.planes.channels {
                    if let Some(note) = self.channels.note_peer_open(peer_app, peer_chan, None) {
                        self.events.push(FlowEvent::ChannelUpdate(note));
                    }
                }
            }
            // OpenUnreliable: type 2, app, channel, wire id - 10 bytes. This is
            // the channel->wire mapping the BaseClient path could not resolve.
            (Some(2), 10) => {
                let peer_app = u32::from(body[1]);
                let peer_chan = be32(&body[2..6]);
                let wire = be32(&body[6..10]);
                self.channel_opens.push((peer_app, peer_chan, Some(wire)));
                self.log(format!(
                    "ctrl OpenUnreliable app={peer_app} chan={peer_chan} wire={wire} (on app={app} chan={chan})"
                ));
                if self.planes.channels {
                    // The router also checks the parity rule the target uses to
                    // tell a peer id from one of its own.
                    if let Some(note) =
                        self.channels.note_peer_open(peer_app, peer_chan, Some(wire))
                    {
                        self.events.push(FlowEvent::ChannelUpdate(note));
                    }
                }
            }
            (Some(tag), len) => {
                let head: Vec<String> =
                    body.iter().take(16).map(|b| format!("{b:02x}")).collect();
                self.log(format!(
                    "ctrl record tag 0x{tag:02x} ({len}B) not recognised: {}",
                    head.join(" ")
                ));
            }
            (None, _) => {}
        }
    }

    /// Answer one snapshot-done marker with the proven `0x04`+id frame (the
    /// reply the server parses as `DeserializedMarkerItem`, and the gate it
    /// keeps the joining client behind). Top-level `0x84` frames are acked per
    /// frame like the reference; the `0x04` shapes are acked once per id so an
    /// echo of our own ack - the same bytes the server accepts as the reply -
    /// cannot turn into a loop. `shape` labels the ack in the log and reports.
    fn ack_marker(&mut self, id: u32, shape: &str, once_per_id: bool) -> Option<Action> {
        let chan1 = self.chan1?;
        if once_per_id && !self.acked_marker_ids.insert(id) {
            return None;
        }
        self.marker_seen = true;
        self.marker_id = id;
        self.markers_acked += 1;
        if shape == "0x04 marker item" {
            self.marker_items_acked += 1;
        }
        let mut ack = vec![0x04u8];
        ack.extend_from_slice(&id.to_le_bytes());
        if self.planes.replication {
            // The marker is the point the target releases the joining
            // client, so it is also where this client asks for the place to be
            // materialized from whatever it has decoded.
            self.replication.request_apply();
        }
        self.log(format!("{shape} id={id} acknowledged"));
        if !self.join_gate_taken {
            self.join_gate_taken = true;
            self.events.push(FlowEvent::JoinStateChanged {
                stage: "marker-ack".to_string(),
                detail: format!(
                    "{shape} id={id} answered; the server continues the join on this ack"
                ),
            });
        }
        Some(Action::Write { stream: chan1, data: frame(&ack) })
    }

    /// Walk channel 1 for the server's snapshot-done marker and acknowledge it.
    ///
    /// Two five-byte frames carry it: `0x84` with a little-endian id (the 2026
    /// top-level shape) and a bare `0x04` with the same id (the 2021
    /// `MarkerItem` shape). The reply is `0x04` followed by the same id either
    /// way. Every complete frame is stepped over whether or not it is a
    /// marker; `0x84` markers are acked once per frame like the reference, and
    /// `0x04` shapes once per id.
    fn try_marker(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        while self.marker_off < self.chan1_rx.len() {
            let Some((len, used)) = compact_varint_decode(&self.chan1_rx[self.marker_off..])
            else {
                break;
            };
            let end = self.marker_off + used + len as usize;
            if len == 0 || end > self.chan1_rx.len() {
                break; // partial tail: wait for the rest
            }
            // Scoped so the borrow of `chan1_rx` ends before `self` is mutated.
            // Two five-byte shapes carry the same little-endian id: the 2026
            // top-level `0x84` and the 2021 `MarkerItem`'s bare `[0x04][id]`.
            let marker = {
                let body = &self.chan1_rx[self.marker_off + used..end];
                if body.len() == 5 && (body[0] == 0x84 || body[0] == MARKER_ITEM) {
                    Some((u32::from_le_bytes(body[1..5].try_into().unwrap()), body[0]))
                } else {
                    None
                }
            };
            self.marker_off = end;
            if let Some((id, tag)) = marker {
                let (shape, once_per_id) = if tag == 0x84 {
                    ("0x84 marker", false)
                } else {
                    ("0x04 marker frame", true)
                };
                if let Some(action) = self.ack_marker(id, shape, once_per_id) {
                    actions.push(action);
                }
            }
        }
        actions
    }

    /// Drain what the replication walk decoded from item runs: `0x04` marker
    /// items (the 2021 join gate, invisible to the top-level `0x84` scan) and
    /// `0x10` tag items (the join-phase completions). Called right after every
    /// `replication.push`, so an ack leaves in the same receive turn.
    fn pump_join_items(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        for id in self.replication.take_marker_items() {
            if let Some(action) = self.ack_marker(id, "0x04 marker item", true) {
                actions.push(action);
            }
        }
        for raw in self.replication.take_join_tags() {
            let (value, label, swapped) = join_tag_label(raw);
            let order = if swapped { " (byte-swapped from the wire order)" } else { "" };
            self.log(format!("join tag {value}{order}: {label}"));
            match value {
                12 => {
                    self.events.push(FlowEvent::JoinStateChanged {
                        stage: "replicated-first-done".to_string(),
                        detail: format!("tag 12{order}: {label}"),
                    });
                }
                13 => {
                    // Tag 13 is the server's own end-of-snapshot: materialize
                    // the place now even if no marker came with it.
                    if self.planes.replication {
                        self.replication.request_apply();
                    }
                    self.events.push(FlowEvent::JoinStateChanged {
                        stage: "join-data-done".to_string(),
                        detail: format!("tag 13{order}: {label}"),
                    });
                }
                _ => {}
            }
        }
        actions
    }

    /// How many snapshot-done markers were acknowledged.
    pub fn markers_acked(&self) -> u64 {
        self.markers_acked
    }

    /// The join gate and the server's join send, for the session report. This
    /// is the "way to join in": the snapshot-done marker the server continues
    /// the join on once its `0x04` ack goes out, plus the tag items that close
    /// the server's own join phases. Every claim here is about bytes this
    /// client sent or decoded, not about what the server did with them.
    pub fn join_report(&self) -> String {
        format!(
            "{}; join gate: {}; markers acked: {} total ({} via the channel-1 frame scan, {} as 0x04 items in item runs)",
            self.replication.join_report(),
            if self.join_gate_taken {
                "taken (0x04 marker ack sent)"
            } else {
                "not taken (no snapshot-done marker arrived)"
            },
            self.markers_acked,
            self.markers_acked.saturating_sub(self.marker_items_acked),
            self.marker_items_acked,
        )
    }

    /// Every complete framed record on channel 1, as (first body byte, body
    /// length). The first frame alone answered nothing - this shows the whole
    /// record vocabulary the peer used, so a challenge or marker buried after
    /// the settings dictionary cannot hide.
    fn chan1_frames(&self) -> Vec<(u8, usize)> {
        let mut out = Vec::new();
        let mut off = 0usize;
        while off < self.chan1_rx.len() {
            let Some((len, used)) = compact_varint_decode(&self.chan1_rx[off..]) else {
                break;
            };
            let end = off + used + len as usize;
            if len == 0 || end > self.chan1_rx.len() {
                break;
            }
            let body = &self.chan1_rx[off + used..end];
            out.push((body[0], body.len()));
            off = end;
        }
        out
    }

    /// The selected 0xA7 mode and control-stream application byte.
    /// These settings are user-selectable, so include them in the session report.
    pub fn flow_config_summary(&self) -> String {
        format!(
            "0xA7 = {}, control stream application = {}, planes = {}",
            a7_mode_label(self.cfg.a7_mode),
            ctrl_app(),
            self.planes.label()
        )
    }

    /// Which 0x90 template fields were refreshed from this session versus kept.
    pub fn join90_report(&self) -> &str {
        if self.join90_report.is_empty() {
            "0x90 was not built"
        } else {
            &self.join90_report
        }
    }

    /// Records how the queued-write drain went.
    pub fn set_drain_summary(&mut self, summary: String) {
        self.drain_summary = summary;
    }

    /// How the queued-write drain went.
    pub fn drain_summary(&self) -> &str {
        if self.drain_summary.is_empty() {
            "no initial-burst bytes remained queued for retry"
        } else {
            &self.drain_summary
        }
    }

    /// Channel assignments the peer announced.
    pub fn channel_opens(&self) -> &[(u32, u32, Option<u32>)] {
        &self.channel_opens
    }

    /// Where the 0x9B challenge stands, for the session report. Every other
    /// signal about it went to Status events, so a challenge that arrived but
    /// did not parse was indistinguishable from one that never came.
    pub fn challenge_summary(&self) -> String {
        let head: Vec<String> = self
            .chan1_rx
            .iter()
            .take(256)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        // Every printable run of four or more bytes, in order. Team Create
        // carries collaborative state - scripting sync, cursors, viewports -
        // alongside instance replication, and the names are the fastest way to
        // tell which kind of frame this is without knowing its layout yet.
        let mut strings: Vec<String> = Vec::new();
        let mut run: Vec<u8> = Vec::new();
        for byte in self.chan1_rx.iter().chain(std::iter::once(&0u8)) {
            if byte.is_ascii_graphic() || *byte == b' ' {
                run.push(*byte);
            } else {
                if run.len() >= 4 {
                    if let Ok(text) = std::str::from_utf8(&run) {
                        if strings.len() < 60 {
                            strings.push(text.to_owned());
                        }
                    }
                }
                run.clear();
            }
        }
        let tags: Vec<String> = self
            .chan1_rx
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == 0x9B)
            .take(8)
            .map(|(offset, _)| format!("@{offset}"))
            .collect();
        let frames: Vec<String> = self
            .chan1_frames()
            .iter()
            .take(24)
            .map(|(tag, len)| format!("0x{tag:02x}/{len}B"))
            .collect();
        format!(
            "chan1 {} bytes buffered, challenge_seen={}, answered={}, markers_acked={}{}; framed records [{}]; 0x9B tag at offsets [{}]; first bytes: {}",
            self.chan1_rx.len(),
            self.challenge_seen,
            self.answered,
            self.markers_acked,
            if self.marker_seen {
                format!(" (last id={})", self.marker_id)
            } else {
                String::new()
            },
            frames.join(" "),
            tags.join(" "),
            head.join(" ")
        ) + &format!("; printable runs ({}): {}", strings.len(), strings.join(" | "))
    }

    fn streams_key(&self, stream: i64) -> (u32, u32) {
        self.streams
            .iter()
            .find(|(_, sid)| **sid == stream)
            .map(|(k, _)| *k)
            .unwrap_or((0, 0))
    }

    /// Find a complete 0x9B frame, solve it locally, and generate a response action.
    fn try_challenge(&mut self) -> Vec<Action> {
        if self.challenge_seen {
            return Vec::new();
        }
        let Some((off, len)) = challenge::find_challenge_frame(&self.chan1_rx) else {
            return Vec::new();
        };
        let msg = &self.chan1_rx[off..off + len];
        let Some((blob, u1, u2)) = challenge::extract_challenge(msg).ok() else {
            // A frame with the 0x9B tag arrived but did not parse. Report its
            // bytes so this is distinguishable from receiving no challenge frame.
            self.challenge_seen = true; // do not re-scan the same frame forever
            let head: Vec<String> = msg
                .iter()
                .take(64)
                .map(|byte| format!("{byte:02x}"))
                .collect();
            self.events.push(FlowEvent::ChallengeFailed(format!(
                "0x9B frame received ({} bytes) but did not parse as [9b][u1][u2][len][blob]; first bytes: {}",
                msg.len(),
                head.join(" ")
            )));
            return Vec::new();
        };
        self.challenge_seen = true;
        self.events.push(FlowEvent::ChallengeReceived {
            u1,
            u2,
            blob_len: blob.len() as u32,
        });
        let job = self.cfg.session_id.clone();
        let report = match challenge::solve_message(msg, &job) {
            Ok(r) => r,
            Err(e) => {
                self.events
                    .push(FlowEvent::ChallengeFailed(format!("local solve failed: {e}")));
                self.chan1_rx.clear();
                return Vec::new();
            }
        };
        self.events.push(FlowEvent::ChallengeAnswered {
            answer: report.answer,
            elapsed_ms: report.solve_ms as u64,
        });
        self.answered = true;
        self.routes_at_ms = self.now_ms.saturating_add(60) as i64;
        self.chan1_rx.clear();
        let resp = challenge::build_response(report.u2, report.answer);
        let mut actions = Vec::new();
        match self.valid_chan1() {
            Some(sid) => actions.push(Action::Write {
                stream: sid,
                data: frame(&resp),
            }),
            // The peer reset chan1 after the challenge arrived: re-open it
            // and send the answer as the first data (native behavior).
            None => {
                let mut data = stream_header(APP, 1).to_vec();
                data.extend_from_slice(&frame(&resp));
                actions.push(Action::OpenStream {
                    app: APP,
                    chan: 1,
                    data,
                });
            }
        }
        self.log(format!(
            "response action generated for answer 0x{} (u1=0x{} u2=0x{}, solved in {}ms)",
            hex32(report.answer),
            hex32(report.u1),
            hex32(report.u2),
            report.solve_ms
        ));
        actions
    }

    /// The live chan1 stream id, if not reset.
    fn valid_chan1(&self) -> Option<i64> {
        self.chan1.filter(|_| !self.chan1_reset)
    }

    /// A stream was reset by the peer.
    pub fn on_reset(&mut self, stream: i64) {
        if Some(stream) == self.chan1 {
            self.chan1_reset = true;
            self.log(format!("chan1 stream {stream} reset by peer — will re-open for answer"));
        }
    }

    /// Fire route declarations about 60 ms after the local challenge solve, and
    /// run the opt-in planes' own timers.
    pub fn on_tick(&mut self, now_ms: u64) -> Vec<Action> {
        self.now_ms = now_ms;
        let mut actions = Vec::new();
        if self.routes_at_ms >= 0 && now_ms as i64 >= self.routes_at_ms {
            self.routes_at_ms = -1;
            let mut data = stream_header(APP, 11).to_vec();
            for route in ROUTES {
                data.extend_from_slice(&frame(&hex(route)));
            }
            self.log(format!("routes: 4 declarations ({}B) on chan11", data.len()));
            actions.push(Action::OpenStream { app: APP, chan: 11, data });
        }
        if self.planes.sync {
            for tick in self.liveness.on_tick(now_ms) {
                self.send_on_channel(tick.channel, tick.payload, &mut actions, tick.name);
            }
            self.pump_edits();
            if let Some(payload) = self.sync.flush(now_ms) {
                let framed = frame(&payload);
                self.log(format!(
                    "upstream sync: {} byte(s) of property items on chan{}",
                    framed.len(),
                    crate::rbx_sync::upstream_channel(true)
                ));
                self.send_on_channel(
                    crate::rbx_sync::upstream_channel(true),
                    framed,
                    &mut actions,
                    "upstream-sync",
                );
            }
        }
        if self.planes.replication {
            self.drain_replication(false);
        }
        if self.planes.channels {
            for note in self.channels.notes() {
                self.events.push(FlowEvent::ChannelUpdate(note));
            }
        }
        actions
    }

    /// Move replication notes into events, and hand out a built snapshot once.
    ///
    /// `at_end` distinguishes the last chance (the session is closing) from a
    /// marker that merely arrived early: mid-session, an apply that has nothing
    /// to build from is retried on the next tick in silence, because the schema
    /// and the batches can still be on their way. At the end of a session the same
    /// silence would read as success, so it is reported.
    fn drain_replication(&mut self, at_end: bool) {
        self.pump_replication_notes();
        if self.snapshot_emitted || !self.replication.apply_requested() {
            return;
        }
        // `apply` builds the place once and keeps it, so a later tick can still
        // pick up records that arrived after the marker. It hands back a borrow of
        // the stream, so reduce it to an owned verdict right here: holding that
        // borrow across the `&mut self` calls below is what the borrow checker
        // refuses, and a clone of the place is the one thing this must not do.
        const NOTHING: u8 = 0;
        const BUILT: u8 = 1;
        const REFUSED: u8 = 2;
        let verdict = match self.replication.apply() {
            None => NOTHING,
            Some(Ok(_)) => BUILT,
            Some(Err(_)) => REFUSED,
        };
        self.pump_replication_notes();
        // Bound before the match for the same reason: a `match` scrutinee keeps
        // its borrow alive through every arm.
        let built = self.replication.snapshot_rbxlx();
        match built {
            Ok(Some(bytes)) => {
                self.snapshot_emitted = true;
                let summary = self.snapshot_summary();
                self.log(format!(
                    "snapshot ready: {} byte(s) of rbxlx — {summary}",
                    bytes.len()
                ));
                self.events.push(FlowEvent::SnapshotReady { bytes, summary });
            }
            Ok(None) => {
                // A refused apply has already said why, and will keep refusing on
                // the same bytes, so it stops being retried; nothing decoded yet
                // is reported once and retried, because the schema and the
                // batches may still be on their way.
                if verdict == REFUSED {
                    self.snapshot_emitted = true;
                } else if verdict == NOTHING && !self.snapshot_empty_reported {
                    self.snapshot_empty_reported = true;
                    let text = format!(
                        "{}: {}",
                        if at_end {
                            "nothing to build a snapshot from when the session closed"
                        } else {
                            "no snapshot to apply yet"
                        },
                        self.replication.summary()
                    );
                    self.log(text.clone());
                    self.events.push(FlowEvent::ReplicationStatus(text));
                }
            }
            Err(error) => {
                self.snapshot_emitted = true;
                self.events.push(FlowEvent::ReplicationStatus(error));
            }
        }
    }

    /// Forward every queued replication note as a status line.
    fn pump_replication_notes(&mut self) {
        for note in self.replication.take_notes() {
            let text = match note {
                ReplicationNote::Warning(text) => format!("replication: {text}"),
                other => format!("replication: {}", other.text()),
            };
            self.events.push(FlowEvent::ReplicationStatus(text));
        }
    }

    /// What the applier did, plus the property-id binding a caller would need to
    /// sync local edits back.
    pub fn snapshot_summary(&self) -> String {
        let stats = self
            .replication
            .snapshot_stats()
            .map(|stats| stats.summary())
            .unwrap_or_else(|| "no snapshot applied".to_string());
        let binding = self
            .replication
            .prop_id_report()
            .unwrap_or_else(|| "no schema arrived, so property ids are unknown".to_string());
        format!("{stats}; outbound addressing: {binding}")
    }

    /// Ask for the place to be built now, used when a session ends without ever
    /// sending a snapshot-done marker. Reports what it could not do.
    pub fn request_snapshot_apply(&mut self) {
        if !self.planes.replication {
            return;
        }
        self.replication.request_apply();
        self.drain_replication(true);
    }

    /// Channel accounting for the session report.
    pub fn channel_report(&self) -> String {
        if !self.planes.channels {
            return "channel plane off; opens are the four reference unreliable records".to_string();
        }
        // The mode is part of the line, so a report can be read without the UI
        // state that produced it.
        let mode = rbx_channels::channel_open_mode_label(self.channels.effective_mode());
        let base = self.channels.summary();
        let gaps = self.channels.gaps();
        if gaps.is_empty() {
            format!("mode {mode}; {base}")
        } else {
            format!("mode {mode}; {base}; gaps: {}", gaps.join("; "))
        }
    }

    /// Replication accounting for the session report.
    pub fn replication_report(&self) -> String {
        if !self.planes.replication {
            return "replication plane off; channel-1 bytes were scanned for markers and 0x9B only".to_string();
        }
        self.replication.summary()
    }

    /// Outbound accounting for the session report.
    /// The outbox the editor writes into. Cloning this handle is how a caller that
    /// is not this flow offers an edit; nothing about it implies the peer will
    /// accept the edit, only that the flow will try to address it.
    pub fn edits_handle(
        &self,
    ) -> std::sync::Arc<std::sync::Mutex<Vec<crate::rbx_sync::SyncEdit>>> {
        self.edits.clone()
    }

    /// Offer one edit directly, for a caller holding the flow.
    pub fn offer_edit(&mut self, edit: crate::rbx_sync::SyncEdit) {
        if let Ok(mut outbox) = self.edits.lock() {
            outbox.push(edit);
        }
    }

    /// Move whatever the editor offered into the sync engine's queue.
    ///
    /// An edit needs two numbers this client only owns once a place has been built:
    /// the wire instance id the peer assigned and the property id from the peer's
    /// dictionary. Without them the edit is dropped where the report can see it,
    /// because an outbox that grows across a whole session would read as a backlog
    /// rather than as a missing schema.
    fn pump_edits(&mut self) {
        let mut outbox = match self.edits.lock() {
            Ok(outbox) => outbox,
            Err(_) => return,
        };
        if outbox.is_empty() {
            return;
        }
        let edits = std::mem::take(&mut *outbox);
        drop(outbox);
        self.edits_offered += edits.len() as u64;
        let plan = {
            let schema = self.replication.schema();
            let ids = self.replication.snapshot_ids();
            let dom = self.replication.snapshot_dom();
            match (schema, ids, dom) {
                (Some(schema), Some(ids), Some(dom)) => {
                    Some(crate::rbx_sync::plan_edits(schema, dom, ids, &edits))
                }
                _ => None,
            }
        };
        match plan {
            Some(plan) => {
                for reason in &plan.unencodable {
                    self.skipped.push(format!("editor edit: {reason}"));
                }
                if plan.unmapped > 0 {
                    self.skipped.push(format!(
                        "{} editor edit(s) refused: no wire id for that instance, or the peer's dictionary names no such member",
                        plan.unmapped
                    ));
                }
                let accepted = self.sync.queue_plan(&plan);
                self.log(format!(
                    "editor edits: {} offered, {} queued; {}",
                    edits.len(),
                    accepted,
                    plan.summary()
                ));
            }
            None => {
                let missing = if self.replication.schema().is_some() {
                    "no applied snapshot to name an instance"
                } else {
                    "no snapshot schema to name a property"
                };
                self.skipped.push(format!(
                    "{} editor edit(s) dropped: {missing}",
                    edits.len()
                ));
            }
        }
    }

    pub fn sync_report(&self) -> String {
        if !self.planes.sync {
            return "outbound plane off; no flow control, liveness or property items were sent".to_string();
        }
        format!(
            "liveness: {}; upstream: {}; queue: {} pending, {} dropped, {} coalesced; editor edits {} offered",
            self.liveness.summary(),
            self.sync.summary(),
            self.sync.pending(),
            self.sync.dropped(),
            self.sync.coalesced(),
            self.edits_offered
        )
    }

    /// Which planes this flow was built with.
    pub fn planes_summary(&self) -> String {
        self.planes.label()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trips() {
        for v in [
            0u64,
            1,
            0x3F,
            0x40,
            0x4000 - 1,
            0x4000,
            0x3F_FF_FF,
            0x40_0000_0000 - 1,
            0x40_0000_0000,
            0xDEAD_BEEF_CAFE,
        ] {
            let enc = compact_varint(v);
            let n = 1usize << (enc[0] >> 6);
            assert_eq!(enc.len(), n, "size for {v}");
            let mut x = u64::from(enc[0] & 0x3F);
            for b in &enc[1..] {
                x = (x << 8) | u64::from(*b);
            }
            assert_eq!(x, v, "round trip {v}");
        }
        // 0x90 template LEB check: 9021 -> 0xBD 0x46 (verified against the
        // native capture).
        assert_eq!(leb128(9021), vec![0xBD, 0x46]);
    }

    #[test]
    fn ticket_v31_matches_python_reference() {
        // Vectors from Boblox py/msgbuild.py ticket_v31.
        assert_eq!(ticket_v31(b"test-ticket-12345"), 0x0406_6521);
        assert_eq!(ticket_v31(b""), 0x7C2D_65B4);
        assert_eq!(ticket_v31(&[b'A'; 100]), 0xC8D3_EF74);
    }

    #[test]
    fn b64_decode_ignores_non_alphabet() {
        assert_eq!(b64_decode("AAEC"), vec![0, 1, 2]);
        assert_eq!(b64_decode("AA\nEC"), vec![0, 1, 2]);
        assert_eq!(
            b64_decode("oKGio6SlpqeoqaqrrK2urw=="),
            vec![0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF]
        );
    }

    #[test]
    fn early_auth_frame_shape() {
        let early = EarlyAuth {
            version: 6,
            pre: b64_decode("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="),
            auth: b64_decode("oKGio6SlpqeoqaqrrK2urw=="),
        };
        let m = build_early_auth(&early);
        assert_eq!(m[0], 0xA8);
        assert_eq!(m[1], 6);
        assert_eq!(m[2], early.pre.len() as u8);
        assert_eq!(&m[3..3 + early.pre.len()], &early.pre[..]);
        assert_eq!(m[3 + early.pre.len()], early.auth.len() as u8);
        assert_eq!(&m[4 + early.pre.len()..], &early.auth[..]);
    }

    #[test]
    fn build_8a_layout() {
        let cfg = JoinConfig {
            user_id: 2_655_886_518,
            user_name: None,
            display_name: None,
            domain_user_id: None,
            client_ticket: "prefix;ignored;AAAA;BBBB;6".into(),
            session_id: "session-xyz".into(),
            random_seed1: None,
            api_security_token: None,
            serialized_client_fields: None,
            encrypted_server_fields: None,
            early_auth: None,
            a7_mode: A7Mode::Skip,
        };
        let m = build_8a(&cfg);
        assert_eq!(m[0], 0x8A);
        // Walk: LEB(2*uid), LEB(ticketlen), ticket, u32le(36), LEB(74), 74B,
        // LEB(7), "Android", 1, '?', LEB(v31), LEB(v31-0x0BADF00D), 0,
        // LEB(sesslen), session, FE CA 01 C0.
        let mut i = 1usize;
        let (zz, k) = leb128_decode(&m[i..]).unwrap();
        i += k;
        assert_eq!(zz, (cfg.user_id as u64) << 1);
        let (tlen, k) = leb128_decode(&m[i..]).unwrap();
        i += k;
        assert_eq!(tlen as usize, cfg.client_ticket.len());
        assert_eq!(&m[i..i + tlen as usize], cfg.client_ticket.as_bytes());
        i += tlen as usize;
        assert_eq!(&m[i..i + 4], &36u32.to_le_bytes());
        i += 4;
        let (c74, k) = leb128_decode(&m[i..]).unwrap();
        i += k;
        assert_eq!(c74, 74);
        i += 74;
        let (android, k) = leb128_decode(&m[i..]).unwrap();
        i += k;
        assert_eq!(android, 7);
        assert_eq!(&m[i..i + 7], b"Android");
        i += 7;
        assert_eq!(m[i], 1);
        assert_eq!(m[i + 1], b'?');
        let (v31, k) = leb128_decode(&m[i + 2..]).unwrap();
        i += 2 + k;
        let expected_v31 = ticket_v31(cfg.client_ticket.as_bytes());
        assert_eq!(v31 as u32, expected_v31);
        let (v31b, k) = leb128_decode(&m[i..]).unwrap();
        i += k;
        assert_eq!(v31b as u32, expected_v31.wrapping_sub(0x0BAD_F00D));
        assert_eq!(m[i], 0x00);
        i += 1;
        let (slen, k) = leb128_decode(&m[i..]).unwrap();
        i += k;
        assert_eq!(slen as usize, cfg.session_id.len());
        assert_eq!(&m[i..i + slen as usize], cfg.session_id.as_bytes());
        i += slen as usize;
        assert_eq!(&m[i..], &[0xFE, 0xCA, 0x01, 0xC0]);
    }

    #[test]
    fn build_92_layout() {
        let pairs = [
            (0u32, 0u32),
            (1, 1),
            (0x7FFF_FFFF, 0x8000_0000),
            (0x8000_0000, 1),
            (0x8000_0001, 0xFFFF_FFFF),
            (0xFFFF_FFFF, 0x1234_5678),
        ];
        for (a, b) in pairs {
            let m = build_92(a, b);
            assert_eq!(m[0], 0x92);
            let (zz, k) = leb128_decode(&m[1..]).unwrap();
            assert_eq!(m.len(), 1 + k);
            let x = ((a as u64) << 32) | ((b ^ a) as u64);
            let mut expect = x << 1;
            if x >> 63 != 0 {
                expect = !expect;
            }
            assert_eq!(zz, expect, "a={a} b={b}");
        }

        // The property the single-value formula broke: `low ^ high` reduces to
        // `b`, independent of `a`, so it changes on every join instead of being
        // pinned to one constant.
        let low_xor_high = |a: u32, b: u32| {
            let x = ((a as u64) << 32) | ((b ^ a) as u64);
            (x as u32) ^ ((x >> 32) as u32)
        };
        // 0x63E2_5F26 - the old "constant" - is just a value of b.
        assert_eq!(low_xor_high(0xDEAD_BEEF, 0x63E2_5F26), 0x63E2_5F26);
        // a does not affect it, b does.
        assert_eq!(low_xor_high(1, 0x63E2_5F26), 0x63E2_5F26);
        assert_ne!(low_xor_high(1, 1), low_xor_high(1, 2));
    }

    #[test]
    fn build_90_patches_template() {
        let cfg = JoinConfig {
            user_id: 2_655_886_518,
            user_name: None,
            display_name: None,
            domain_user_id: None,
            client_ticket: "t;2".into(),
            session_id: "s".into(),
            random_seed1: Some("SEED-XYZ".into()),
            api_security_token: Some("TOKEN-ABC".into()),
            serialized_client_fields: Some("SCF".into()),
            encrypted_server_fields: Some("ESF".into()),
            early_auth: None,
            a7_mode: A7Mode::Skip,
        };
        let built = build_90(&cfg).expect("build_90");
        // No identity fields supplied, so all three are reported as unavailable
        // rather than quietly leaving the captured account's in place.
        assert_eq!(
            built.missing_identity,
            ["UserName", "DisplayName", "DomainUserId"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        let m = built.bytes;
        // Prefix (everything before the JSON LEB) is byte-identical.
        let raw = TEMPLATE_90;
        let i = raw.windows(9).position(|w| w == b"{\"UserId\"").unwrap();
        let mut start = i - 1;
        while raw[start - 1] & 0x80 != 0 {
            start -= 1;
        }
        assert_eq!(&m[..start], &raw[..start]);
        assert_eq!(&m[m.len() - 20..], &raw[raw.len() - 20..]);
        // LEB length matches the JSON body.
        let (json_len, k) = leb128_decode(&m[start..]).unwrap();
        let body = &m[start + k..start + k + json_len as usize];
        assert_eq!(json_len as usize, body.len());
        let v: serde_json::Value = serde_json::from_slice(body).expect("json");
        assert_eq!(v["RandomSeed1"].as_str(), Some("SEED-XYZ"));
        assert_eq!(v["APIsecurityToken"].as_str(), Some("TOKEN-ABC"));
        let jt: serde_json::Value =
            serde_json::from_str(v["__joinTicket"].as_str().unwrap()).expect("ticket json");
        assert_eq!(jt["SerializedClientFields"].as_str(), Some("SCF"));
        assert_eq!(jt["EncryptedServerFields"].as_str(), Some("ESF"));
        // UserId now follows cfg, which here happens to be the captured value.
        assert_eq!(v["UserId"].as_i64(), Some(2_655_886_518));
    }

    /// The 0x90 template froze one account. Every identity field this session
    /// can source has to follow `cfg`, or the burst contradicts itself: 0x8A
    /// carries `cfg.user_id` and 0x90 would carry somebody else's.
    #[test]
    fn build_90_replaces_the_captured_identity() {
        let cfg = JoinConfig {
            user_id: 777,
            user_name: Some("live_name".into()),
            display_name: Some("live display".into()),
            domain_user_id: Some(424242),
            client_ticket: "t;2".into(),
            session_id: "s".into(),
            random_seed1: Some("SEED-XYZ".into()),
            api_security_token: Some("TOKEN-ABC".into()),
            // base64 JSON, as the live gamejoin response delivers it
            serialized_client_fields: Some(
                "eyJBY2NvdW50QWdlIjo0MiwiQ291bnRyeUNvZGUiOiJBRSIsIk1lbWJlcnNoaXBUeXBlIjoiUHJlbWl1bSIsIkhhc1ZlcmlmaWVkQmFkZ2UiOnRydWUsIkdhbWVMb2NhbGUiOiJlbl9nYiJ9"
                    .into(),
            ),
            encrypted_server_fields: Some("ESF".into()),
            early_auth: None,
            a7_mode: A7Mode::Skip,
        };
        let built = build_90(&cfg).expect("build_90");
        let raw = TEMPLATE_90;
        let i = raw.windows(9).position(|w| w == b"{\"UserId\"").unwrap();
        let mut start = i - 1;
        while raw[start - 1] & 0x80 != 0 {
            start -= 1;
        }
        let (json_len, k) = leb128_decode(&built.bytes[start..]).unwrap();
        let body = &built.bytes[start + k..start + k + json_len as usize];
        let v: serde_json::Value = serde_json::from_slice(body).expect("json");

        assert_eq!(v["UserId"].as_i64(), Some(777));
        assert_eq!(v["CharacterAppearanceId"].as_i64(), Some(777));
        // The remaining identity fields come from the gamejoin response when it
        // carries them; the captured account's values must not survive.
        assert_eq!(v["UserName"].as_str(), Some("live_name"));
        assert_eq!(v["DisplayName"].as_str(), Some("live display"));
        assert_eq!(v["DomainUserId"].as_i64(), Some(424242));
        assert!(!built.frozen.iter().any(|k| k == "UserName"));
        assert!(!built.frozen.iter().any(|k| k == "DomainUserId"));
        assert!(
            v["CharacterAppearance"].as_str().unwrap().contains("userId=777"),
            "the avatar URL names the user in a query parameter: {}",
            v["CharacterAppearance"]
        );
        assert!(!v["CharacterAppearance"].as_str().unwrap().contains("2655886518"));

        // The ticket's own account fields override the frozen ones.
        assert_eq!(v["AccountAge"].as_i64(), Some(42));
        assert_eq!(v["CountryRegionCode"].as_str(), Some("AE"));
        assert_eq!(v["MembershipType"].as_str(), Some("Premium"));
        assert_eq!(v["HasVerifiedBadge"].as_bool(), Some(true));
        assert_eq!(v["LocaleId"].as_str(), Some("en_gb"));
        assert_eq!(v["LocaleIdInternal"].as_str(), Some("en_gb"));

        // The report says what is still the captured account's, rather than
        // leaving it to be discovered on the server's side.
        assert!(built.refreshed.iter().any(|k| k == "UserId"));
        // These are replaced on every build; the baseline must be the pristine
        // template or they get reported as untouched.
        for key in ["RandomSeed1", "APIsecurityToken", "__joinTicket"] {
            assert!(
                built.refreshed.iter().any(|k| k == key),
                "{key} is always substituted and must read as refreshed"
            );
        }
        assert!(built.missing_identity.is_empty(), "all three identity fields were supplied");
        assert!(built.frozen.iter().any(|k| k == "UserName"));
        assert!(!built.frozen.iter().any(|k| k == "UserId"));
    }

    /// The server's snapshot-done marker is `0x84` plus a little-endian id, and
    /// the reply is `0x04` plus the same id on the same stream. Join progress is
    /// gated on that ack, so the wire format has to be exact.
    #[test]
    fn marker_is_acknowledged_with_its_own_id() {
        let cfg = JoinConfig {
            user_id: 1,
            user_name: None,
            display_name: None,
            domain_user_id: None,
            client_ticket: "t;2".into(),
            session_id: "s".into(),
            random_seed1: None,
            api_security_token: None,
            serialized_client_fields: None,
            encrypted_server_fields: None,
            early_auth: None,
            a7_mode: A7Mode::Skip,
        };
        let id: u32 = 0x0102_0304;
        let mut body = vec![0x84u8];
        body.extend_from_slice(&id.to_le_bytes());
        let whole = frame(&body);

        let mut flow = AppFlow::new(cfg.clone(), 0);
        flow.note_opened(APP, 1, 7);
        let actions = flow.on_stream_data(7, &whole);
        assert_eq!(actions.len(), 1, "one marker, one ack");
        match &actions[0] {
            Action::Write { stream, data } => {
                assert_eq!(*stream, 7, "the ack goes back on channel 1");
                let (len, used) = compact_varint_decode(data).unwrap();
                assert_eq!(len as usize, 5);
                assert_eq!(&data[used..], &[0x04, 0x04, 0x03, 0x02, 0x01]);
            }
            other => panic!("expected a Write, got {other:?}"),
        }
        assert_eq!(flow.markers_acked(), 1);

        // A marker split across two reads is acked once, after it completes -
        // not twice, and not never.
        let mut split_flow = AppFlow::new(cfg, 0);
        split_flow.note_opened(APP, 1, 9);
        let at = whole.len() / 2;
        assert!(split_flow.on_stream_data(9, &whole[..at]).is_empty());
        assert_eq!(split_flow.markers_acked(), 0, "a partial frame is not a marker");
        assert_eq!(split_flow.on_stream_data(9, &whole[at..]).len(), 1);
        assert_eq!(split_flow.markers_acked(), 1);
    }

    #[test]
    fn marker_item_in_an_item_run_is_acked_once() {
        // The 2021 server's MarkerItem rides inside a `0x83` item run, where
        // the top-level `0x84` scan cannot see it. The ack is the join gate:
        // the server keeps the joining client waiting until the same id comes
        // back (parsed there as `DeserializedMarkerItem`).
        let cfg = JoinConfig {
            user_id: 1,
            user_name: None,
            display_name: None,
            domain_user_id: None,
            client_ticket: "t;2".into(),
            session_id: "s".into(),
            random_seed1: None,
            api_security_token: None,
            serialized_client_fields: None,
            encrypted_server_fields: None,
            early_auth: None,
            a7_mode: A7Mode::Skip,
        };
        let id: u32 = 0x1122_3344;
        let mut body = vec![0x83u8, MARKER_ITEM];
        body.extend_from_slice(&id.to_le_bytes());
        body.push(0x00); // chain end
        let whole = frame(&body);

        let mut flow = AppFlow::new(cfg, 0);
        flow.note_opened(APP, 1, 7);
        let actions = flow.on_stream_data(7, &whole);
        assert_eq!(actions.len(), 1, "one marker item, one ack");
        match &actions[0] {
            Action::Write { stream, data } => {
                assert_eq!(*stream, 7, "the ack goes back on channel 1");
                let (len, used) = compact_varint_decode(data).unwrap();
                assert_eq!(len as usize, 5);
                assert_eq!(&data[used..], &[0x04, 0x44, 0x33, 0x22, 0x11]);
            }
            other => panic!("expected a Write, got {other:?}"),
        }
        assert_eq!(flow.markers_acked(), 1);
        assert!(flow.join_report().contains("join gate: taken"), "{}", flow.join_report());

        // The same id again - an echo of our own ack is the same bytes the
        // server accepts as the reply - is not acked twice, and cannot loop.
        let again = flow.on_stream_data(7, &whole);
        assert!(again.is_empty(), "an answered marker id is not acked again: {again:?}");
        assert_eq!(flow.markers_acked(), 1);
    }

    #[test]
    fn bare_marker_frame_is_acked_once_per_id() {
        // A MarkerItem can also arrive bare on channel 1 as `[0x04][id]` - the
        // exact bytes this client writes as the reply.
        let cfg = JoinConfig {
            user_id: 1,
            user_name: None,
            display_name: None,
            domain_user_id: None,
            client_ticket: "t;2".into(),
            session_id: "s".into(),
            random_seed1: None,
            api_security_token: None,
            serialized_client_fields: None,
            encrypted_server_fields: None,
            early_auth: None,
            a7_mode: A7Mode::Skip,
        };
        let id: u32 = 0x0a0b_0c0d;
        let mut body = vec![MARKER_ITEM];
        body.extend_from_slice(&id.to_le_bytes());
        let whole = frame(&body);

        let mut flow = AppFlow::new(cfg, 0);
        flow.note_opened(APP, 1, 7);
        let actions = flow.on_stream_data(7, &whole);
        assert_eq!(actions.len(), 1, "one bare marker frame, one ack");
        match &actions[0] {
            Action::Write { stream, data } => {
                assert_eq!(*stream, 7);
                let (len, used) = compact_varint_decode(data).unwrap();
                assert_eq!(len as usize, 5);
                assert_eq!(&data[used..], &[0x04, 0x0d, 0x0c, 0x0b, 0x0a]);
            }
            other => panic!("expected a Write, got {other:?}"),
        }
        assert_eq!(flow.markers_acked(), 1);
        let echo = flow.on_stream_data(7, &whole);
        assert!(echo.is_empty(), "a repeated bare marker id is acked once: {echo:?}");
    }

    #[test]
    fn join_phase_tags_advance_the_join_report() {
        // Tag 12 and tag 13 close the server's two join phases (the recovered
        // `ID_REPLIC_TAG` vocabulary). Tag 13 is the end of the snapshot, so it
        // also asks for the place to be materialized even if no marker came.
        let _lock = plane_serial();
        let _guard = PlaneGuard::with(
            JoinPlanes { channels: false, replication: true, sync: false },
            ChannelOpenMode::Off,
        );
        let mut body = vec![0x83u8, 0x10];
        body.extend_from_slice(&12u32.to_le_bytes());
        body.push(0x10);
        body.extend_from_slice(&13u32.to_le_bytes());
        body.push(0x00);

        let mut flow = flow_with(JoinPlanes { channels: false, replication: true, sync: false });
        flow.note_opened(APP, 1, 7);
        let _ = flow.on_stream_data(7, &frame(&body));
        assert!(flow.replication.apply_requested(), "tag 13 closes the snapshot and requests the apply");
        let report = flow.join_report();
        assert!(report.contains("tag12=seen"), "{report}");
        assert!(report.contains("tag13=seen"), "{report}");
        assert!(report.contains("replicated-first-done(tag 12)"), "{report}");
        assert!(report.contains("join-data-done(tag 13)"), "{report}");
        let events = flow.drain_events();
        let joined: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                FlowEvent::JoinStateChanged { stage, detail } => {
                    Some(format!("{stage}: {detail}"))
                }
                _ => None,
            })
            .collect();
        assert_eq!(joined.len(), 2, "one milestone per phase tag: {joined:?}");
        assert!(joined[0].contains("replicated-first-done"), "{joined:?}");
        assert!(joined[1].contains("join-data-done"), "{joined:?}");
    }

    #[test]
    fn join_tags_are_named_in_both_byte_orders() {
        // The 2026 capture reads tag ids little-endian; the 2021 model writes
        // them big-endian. Both orders name the known tags, and `swapped` says
        // which one matched.
        assert_eq!(join_tag_label(13), (13, "JoinData replication finished", false));
        assert_eq!(
            join_tag_label(13u32.swap_bytes()),
            (13, "JoinData replication finished", true)
        );
        assert_eq!(join_tag_label(12).1, "ReplicatedFirst replication finished");
        assert_eq!(join_tag_label(999), (999, "unrecovered tag id", false));
    }

    #[test]
    fn a7_real_template_starts_with_a7() {
        // The control application byte defaults to 0, which is the value every
        // multi-second session used.
        assert_eq!(ctrl_app(), 0, "default ctrl app must stay 0");
        for app in [0u8, 1, 4] {
            set_ctrl_app(app);
            assert_eq!(ctrl_app(), app as u32);
        }
        set_ctrl_app(0);
        // The UI selector must round-trip, or the three-way test it exists for
        // silently sends the same frame every time.
        for (mode, expect) in [
            (A7Mode::Real, A7Mode::Real),
            (A7Mode::Empty, A7Mode::Empty),
            (A7Mode::Skip, A7Mode::Skip),
        ] {
            set_a7_mode(mode);
            assert_eq!(a7_mode(), expect, "{mode:?} did not round-trip");
        }
        assert_eq!(a7_mode_label(A7Mode::Real), "real 99-byte capture");
        set_a7_mode(A7Mode::Real);
        assert_eq!(A7_REAL[0], 0xA7);
        assert_eq!(A7_REAL.len(), 99);
    }

    /// Save and restore the plane switches and the channel mode, because a flow
    /// captures them when it is constructed.
    /// The planes are process-wide switches the UI sets, and `cargo test` runs
    /// cases on parallel threads: any test that reads a byte the channel mode
    /// decides has to hold this, or another test's mode is what it sees.
    static PLANE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn plane_serial() -> std::sync::MutexGuard<'static, ()> {
        PLANE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct PlaneGuard {
        planes: JoinPlanes,
        mode: ChannelOpenMode,
    }

    impl PlaneGuard {
        fn with(planes: JoinPlanes, mode: ChannelOpenMode) -> Self {
            let saved = Self {
                planes: join_planes(),
                mode: rbx_channels::channel_open_mode(),
            };
            set_join_planes(planes);
            rbx_channels::set_channel_open_mode(mode);
            saved
        }
    }

    impl Drop for PlaneGuard {
        fn drop(&mut self) {
            set_join_planes(self.planes);
            rbx_channels::set_channel_open_mode(self.mode);
        }
    }

    fn planes_cfg() -> JoinConfig {
        JoinConfig {
            user_id: 1,
            user_name: None,
            display_name: None,
            domain_user_id: None,
            client_ticket: "t;2".into(),
            session_id: "planes".into(),
            random_seed1: None,
            api_security_token: None,
            serialized_client_fields: None,
            encrypted_server_fields: None,
            early_auth: None,
            a7_mode: A7Mode::Skip,
        }
    }

    /// A flow built at t=100, asserting that it captured the switches it was
    /// built with — a flow that re-read the globals later would let one test's
    /// toggles leak into another's byte stream.
    fn flow_with(planes: JoinPlanes) -> AppFlow {
        let mut flow = AppFlow::new(planes_cfg(), 100);
        assert_eq!(flow.planes, planes, "the flow must capture the plane switches");
        flow
    }

    fn hx(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    }

    /// The control-stream payload the flow wrote at connect time, which is where
    /// every channel record has to appear.
    fn ctrl_burst(flow: &mut AppFlow) -> Vec<u8> {
        flow.on_connected()
            .into_iter()
            .find_map(|action| match action {
                Action::OpenStream { app, chan, data } => {
                    if chan == CTRL_CHANNEL {
                        let _ = app;
                        Some(data)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .expect("the flow opens the control stream at connect")
    }

    fn events_of(flow: &mut AppFlow, wanted: fn(&FlowEvent) -> Option<String>) -> Vec<String> {
        flow.drain_events().iter().filter_map(wanted).collect()
    }

    #[test]
    fn planes_default_off_and_change_nothing() {
        let _serial = plane_serial();
        let _guard = PlaneGuard::with(JoinPlanes::default(), ChannelOpenMode::Off);
        assert_eq!(join_planes(), JoinPlanes::default());
        assert_eq!(join_planes().label(), "none (handshake and burst only)");
        {
            let all = JoinPlanes { channels: true, replication: true, sync: true };
            let _guard = PlaneGuard::with(all, ChannelOpenMode::FullProfile);
            assert_eq!(join_planes(), all);
            let label = join_planes().label();
            assert!(
                label.contains("full profile") && label.contains("replication decode"),
                "{label}"
            );
        }

        let mut flow = flow_with(JoinPlanes::default());
        assert_eq!(flow.planes_summary(), "none (handshake and burst only)");
        assert!(flow.channel_report().starts_with("channel plane off"));
        assert!(flow.replication_report().starts_with("replication plane off"));
        assert!(flow.sync_report().starts_with("outbound plane off"));
        // Only the control channel and chan1 are opened, and no timer fires.
        let data = ctrl_burst(&mut flow);
        // The handshake logs its own stream header whatever the switches say; the
        // claim under test is that a disabled plane adds nothing to it.
        let _ = flow.drain_events();
        assert!(flow.on_tick(10_000).is_empty(), "no liveness traffic when off");
        // Bound to channel 1 so the bytes take the join path, not the control
        // path: the control parser logs unrecognised records whatever the planes
        // say, and that would be a false pass here.
        flow.note_opened(APP, 1, 7);
        flow.on_stream_data(7, &frame(&[0x83, 0x16, 0x09, 0x00]));
        let events = flow.drain_events();
        assert!(
            !events.iter().any(|event| matches!(
                event,
                FlowEvent::ChannelUpdate(_)
                    | FlowEvent::ReplicationStatus(_)
                    | FlowEvent::SnapshotReady { .. }
            )),
            "a disabled plane reports nothing: {events:?}"
        );
        // And the control burst is the four reference records, untouched. The
        // 7-byte header is skipped because the application byte inside it is a
        // global switch other tests are allowed to move.
        let mut expected = frame(&[0x00, 0x00]);
        for &(chan, wire) in &[(3u32, 2u32), (5, 4), (6, 6), (11, 8)] {
            expected.extend_from_slice(&frame(&build_open_unreliable(APP, chan, wire)));
        }
        assert_eq!(&data[7..], &expected[..], "{}", hx(&data));
    }

    #[test]
    fn channel_plane_reference_mode_matches_the_hardcoded_burst() {
        let _serial = plane_serial();
        let off = {
            let _guard = PlaneGuard::with(JoinPlanes::default(), ChannelOpenMode::Off);
            ctrl_burst(&mut flow_with(JoinPlanes::default()))
        };
        let on = {
            let _guard = PlaneGuard::with(
                JoinPlanes { channels: true, ..JoinPlanes::default() },
                ChannelOpenMode::ReferenceUnreliable,
            );
            ctrl_burst(&mut flow_with(JoinPlanes {
                channels: true,
                ..JoinPlanes::default()
            }))
        };
        // Turning the plane on must not move a byte in the reference mode, or a
        // passing session stops being comparable with the capture before it. The
        // stream header is excluded for the same reason as above: the control
        // application byte is a global switch, not part of what this compares.
        assert_eq!(off[7..], on[7..], "\noff: {}\non:  {}", hx(&off), hx(&on));
    }

    #[test]
    fn channel_plane_full_profile_adds_reliable_open_records() {
        let _serial = plane_serial();
        let planes = JoinPlanes { channels: true, ..JoinPlanes::default() };
        let _guard = PlaneGuard::with(planes, ChannelOpenMode::FullProfile);
        let mut flow = flow_with(planes);
        let data = ctrl_burst(&mut flow);
        // Rebuild the burst from the profile table independently: one hello,
        // then one record per row in profile order — an OpenReliable for the ten
        // reliable rows and an OpenUnreliable carrying the parity-0 series for
        // the four unreliable ones. The router and the table have to agree.
        let mut want = frame(&[0x00, 0x00]);
        let mut next_wire = 2u32;
        for (index, row) in DEFAULT_CHANNEL_PROFILE.iter().enumerate() {
            let record = if rbx_channels::is_unreliable(row) {
                let body = build_open_unreliable(APP, index as u32, next_wire);
                next_wire += 2;
                body
            } else {
                rbx_channels::reliable_open_body(APP as u8, index as u32)
            };
            want.extend_from_slice(&frame(&record));
        }
        assert_eq!(hx(&data[7..]), hx(&want));
        // The mode is in the report, so a run is reproducible from its log alone.
        assert!(flow.on_tick(10_000).is_empty());
        let report = flow.channel_report();
        assert!(report.contains("full profile"), "{report}");
        assert!(report.contains("staged 14"), "{report}");
        assert!(
            report.contains("opened here with no matching peer announcement"),
            "{report}"
        );
    }

    #[test]
    fn channel_plane_reconciles_a_peer_announcement() {
        let _serial = plane_serial();
        let planes = JoinPlanes { channels: true, ..JoinPlanes::default() };
        let _guard = PlaneGuard::with(planes, ChannelOpenMode::ReferenceUnreliable);
        let mut flow = flow_with(planes);
        ctrl_burst(&mut flow);
        flow.drain_events();
        // A control record announcing chan 5 unreliable with wire id 10 (the
        // peer's parity, so it decodes) must be routed and reported.
        // A peer id must carry the *opposite* parity to ours, so 11 (odd) is a
        // peer id and 10 would be rejected as one of our own.
        let mut record = vec![rbx_channels::UNRELIABLE_OPEN_TYPE, APP as u8];
        record.extend_from_slice(&5u32.to_be_bytes());
        record.extend_from_slice(&11u32.to_be_bytes());
        flow.note_control_record(APP, CTRL_CHANNEL, &record);
        let notes = events_of(&mut flow, |event| match event {
            FlowEvent::ChannelUpdate(text) => Some(text.clone()),
            _ => None,
        });
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].contains("chan 5 unreliable wire=11"), "{}", notes[0]);
        assert!(notes[0].contains("internal id -11"), "{}", notes[0]);
        let report = flow.channel_report();
        assert!(report.contains("peer-announced 1"), "{report}");
    }

    #[test]
    fn replication_plane_reports_what_it_cannot_parse() {
        let _serial = plane_serial();
        let planes = JoinPlanes { replication: true, ..JoinPlanes::default() };
        let _guard = PlaneGuard::with(planes, ChannelOpenMode::Off);
        let mut flow = flow_with(planes);
        flow.note_opened(APP, 1, 7);
        flow.on_stream_data(7, &frame(&[0x83, 0x16, 0x09, 0x00]));
        flow.on_tick(101);
        let notes = events_of(&mut flow, |event| match event {
            FlowEvent::ReplicationStatus(text) => Some(text.clone()),
            FlowEvent::SnapshotReady { summary, .. } => Some(format!("snapshot: {summary}")),
            _ => None,
        });
        assert!(!notes.is_empty(), "a frame must be accounted for");
        let text = notes.join("\n");
        // The batch is reported as the sub-code it is, with its byte count, and
        // not passed off as a decoded record run.
        assert!(text.contains("JoinDataItemV2"), "{text}");
        assert!(text.contains("no compressed frame"), "{text}");
        let report = flow.replication_report();
        assert!(report.contains("1 frame(s)"), "{report}");
        assert!(report.contains("1 skipped"), "{report}");
        assert!(report.contains("schema not received"), "{report}");
        // No schema arrived, so asking for the place must say so instead of
        // inventing a DataModel.
        flow.request_snapshot_apply();
        let notes = events_of(&mut flow, |event| match event {
            FlowEvent::ReplicationStatus(text) => Some(text.clone()),
            _ => None,
        });
        assert!(notes.iter().any(|n| n.contains("snapshot")), "{notes:?}");
        assert!(flow.snapshot_summary().contains("no snapshot applied"));
    }

    #[test]
    fn marker_asks_the_replication_stream_for_the_place() {
        let _serial = plane_serial();
        let planes = JoinPlanes { replication: true, ..JoinPlanes::default() };
        let _guard = PlaneGuard::with(planes, ChannelOpenMode::Off);
        let mut flow = flow_with(planes);
        flow.note_opened(APP, 1, 7);
        let mut body = vec![0x84u8, 0x01, 0x00, 0x00, 0x00];
        let whole = frame(&body);
        let actions = flow.on_stream_data(7, &whole);
        assert_eq!(actions.len(), 1, "the marker is still acknowledged once");
        assert_eq!(flow.markers_acked(), 1);
        // The apply was requested, and had nothing to apply.
        flow.on_tick(101);
        let notes = events_of(&mut flow, |event| match event {
            FlowEvent::ReplicationStatus(text) => Some(text.clone()),
            _ => None,
        });
        assert!(
            notes.iter().any(|n| n.contains("no snapshot to apply yet")),
            "{notes:?}"
        );
    }

    #[test]
    fn sync_plane_sends_the_reference_liveness_periods() {
        let _serial = plane_serial();
        let planes = JoinPlanes { sync: true, ..JoinPlanes::default() };
        let _guard = PlaneGuard::with(planes, ChannelOpenMode::Off);
        let mut flow = flow_with(planes);
        let actions = flow.on_connected();
        // The initial sync rides a fresh chan3 stream, framed exactly as the
        // reference template plus its compact-varint length.
        let chan3 = actions
            .iter()
            .find_map(|action| match action {
                Action::OpenStream { app, chan, data } => {
                    if *chan == 3 {
                        let _ = app;
                        Some(data.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .expect("the initial sync opens channel 3");
        assert_eq!(&chan3[..7], &stream_header(APP, 3)[..], "{}", hx(&chan3));
        assert_eq!(&chan3[7..], &frame(&crate::rbx_sync::sync_template()));
        // The ping has nowhere to go yet: chan1 has no bound stream id, and a
        // second OpenStream for it would corrupt the byte counts the report
        // compares. It is skipped and named.
        assert!(
            flow.skipped().iter().any(|s| s.contains("ping#0") && s.contains("chan1")),
            "{:?}",
            flow.skipped()
        );
        // Once chan1 is bound, the periods fire on it.
        flow.note_opened(APP, 1, 10);
        let at_1100 = flow.on_tick(1_100);
        assert_eq!(at_1100.len(), 1, "one ping at the ping period");
        match &at_1100[0] {
            Action::Write { stream, data } => {
                assert_eq!(*stream, 10);
                assert_eq!(data, &frame(&crate::rbx_sync::ping_template()));
            }
            other => panic!("expected a chan1 write, got {other:?}"),
        }
        // The sync period arrives together with a ping, and channel 3 is still
        // unacknowledged, so only the ping goes out.
        let at_5100 = flow.on_tick(5_100);
        assert_eq!(at_5100.len(), 1, "ping only: {at_5100:?}");
        let report = flow.sync_report();
        // One of the three pings and one of the two syncs were produced but not
        // sent, which the flow's own skip list accounts for separately.
        assert!(report.contains("3 ping(s)"), "{report}");
        assert!(report.contains("2 sync(s)"), "{report}");
        assert!(report.contains("0 flush"), "{report}");
    }

    #[test]
    fn sync_plane_does_not_queue_changes_it_cannot_address() {
        let _serial = plane_serial();
        let planes = JoinPlanes { sync: true, ..JoinPlanes::default() };
        let _guard = PlaneGuard::with(planes, ChannelOpenMode::Off);
        let flow = flow_with(planes);
        // Nothing is queued by construction: an editor edit reaches the session
        // only through the flow's outbox, and the UI does not offer one on the
        // current path. The report has to say so in the same terms.
        assert!(flow.sync_report().contains("0 pending"), "{}", flow.sync_report());
    }

    /// What the route strings actually are: unframed `0xA6` payloads, framed by
    /// `on_tick` at send time. The assertion this replaces treated the leading
    /// `a6` as a compact-varint length, which it is not, so it failed against
    /// bytes that a live session sends unchanged.
    /// The outbox is the editor's seam into a running session, and an edit this
    /// client cannot address has to be dropped loudly rather than kept: a queue that
    /// holds an edit forever reads as pending traffic, while the truth is that no
    /// place has been built to point at.
    #[test]
    fn sync_plane_drops_an_editor_edit_it_cannot_address() {
        let _serial = plane_serial();
        let planes = JoinPlanes { sync: true, ..JoinPlanes::default() };
        let _guard = PlaneGuard::with(planes, ChannelOpenMode::Off);
        let mut flow = flow_with(planes);
        flow.offer_edit(crate::rbx_sync::SyncEdit {
            entity: 7,
            property: "Size".to_string(),
            value: rbx_dom_weak::types::Variant::Vector3(rbx_dom_weak::types::Vector3::new(
                1.0, 2.0, 4.0,
            )),
        });
        flow.on_tick(10_000);
        let report = flow.sync_report();
        assert!(report.contains("0 pending"), "{report}");
        assert!(report.contains("editor edits 1 offered"), "{report}");
        assert!(
            flow.skipped()
                .iter()
                .any(|note| note.contains("dropped: no snapshot schema")),
            "{:?}",
            flow.skipped()
        );
        // Taken out of the outbox, so a second tick cannot resurrect it.
        flow.on_tick(20_000);
        assert!(
            flow.sync_report().contains("editor edits 1 offered"),
            "the edit was consumed, not queued: {}",
            flow.sync_report()
        );
    }

    #[test]
    fn routes_are_unframed_payloads_that_frame_to_exactly_one_message() {
        assert_eq!(ROUTES.len(), 4);
        for (index, route) in ROUTES.iter().enumerate() {
            let payload = hex(route);
            // Every route is one flow-control message whose payload starts with
            // the 0xA6 tag, followed by its 1-based declaration index.
            assert_eq!(payload[0], 0xA6, "route {index} tag");
            assert_eq!(payload[1], (index + 1) as u8, "route {index} declaration index");
            // Framing it yields exactly one complete frame and no tail, which is
            // what the flow relies on when it concatenates all four onto chan11.
            let framed = frame(&payload);
            let (length, used) = compact_varint_decode(&framed).expect("a length prefix");
            assert_eq!(length as usize, payload.len(), "route {index} framed length");
            let offset = used + payload.len();
            assert_eq!(offset, framed.len(), "route {index} has no tail");
            // The step words the flow could patch are inside the payload.
            assert!(crate::rbx_sync::with_steps(&payload, 5, 1, 2).is_some(), "route {index}");
        }
        // The reference client's canonical routes and this list agree on three
        // of four; route 2 differs by one byte. Recorded here because a future
        // re-capture is the only way to settle it, and silently "fixing" it
        // would change bytes a live session sends.
        let third = hex(ROUTES[2]);
        assert_eq!(&third[1..5], &[0x03, 0x01, 0x01, 0x01], "route 2 head");
    }
}
