//! App=4 RbxTransport join-session flow.
//!
//! A faithful port of the post-handshake message sequence from the Boblox
//! client (`session.cpp`): the control hello + openU burst, the channel-1
//! early-auth / A7 / 0x90 / 0x92 / 0x8A / 0x8F burst, the 0x9B challenge
//! solve-and-answer (via [`crate::challenge`]), and the route declarations
//! sent ~60 ms after the answer.
//!
//! This module is pure logic: it receives raw stream bytes and emits
//! [`Action`]s. The QUIC session (`team_create.rs`) executes the actions
//! (open streams, write bytes, flush datagrams) and feeds received data
//! back in.

use crate::challenge;
use std::collections::HashMap;

/// BaseClient join application id on the RbxTransport multiplexer.
pub const APP: u32 = 4;
/// Dummy (health-check) application id used by the native client.
#[allow(dead_code)]
pub const APP_DUMMY: u32 = 6;
/// Control channel: the ASCII "ctrl" as a big-endian u32.
pub const CTRL_CHANNEL: u32 = 0x6374_726C;

/// Native 0x90 message template (10 432 B capture). The JSON region is
/// patched with this session's values; the leading bytes and the final 20
/// bytes are preserved verbatim.
const TEMPLATE_90: &[u8] = include_bytes!("../data/appflow/rbx90_template.bin");
/// The real 99-byte 0xA7 client-auth frame captured from a native client.
const A7_REAL: &[u8] = include_bytes!("../data/appflow/rbxa7_real.bin");

/// Route declarations sent on channel 11 ~60 ms after the 0x9B answer
/// (sessioncap s0019-s0022).
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
    pub client_ticket: String,
    pub session_id: String,
    pub random_seed1: Option<String>,
    pub api_security_token: Option<String>,
    pub serialized_client_fields: Option<String>,
    pub encrypted_server_fields: Option<String>,
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
    /// The challenge was solved locally and the answer sent.
    ChallengeAnswered { answer: u32, elapsed_ms: u64 },
    /// Local solving failed; the session will not reach the connected stage.
    ChallengeFailed(String),
    /// Post-answer traffic arrived on channel 1 (world state / peer
    /// assignment) — the BaseClient "connected" milestone.
    ConnectedStageReached { detail: String },
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
        v |= u64::from(b & 0x7F) << (7 * i);
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
        if i >= 8 {
            return None;
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
    } else if v < 0x40_0000_0000 {
        out.push(0x80 | (v >> 24) as u8);
        out.push((v >> 16) as u8);
        out.push((v >> 8) as u8);
        out.push((v & 0xFF) as u8);
    } else {
        out.push(0xC0 | (v >> 56) as u8);
        for shift in (8..56).rev().step_by(8) {
            out.push((v >> shift) as u8);
        }
    }
    out
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
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
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
    let v20 = v16
        .wrapping_xor(v15.wrapping_sub(v18.rotate_left(v19)))
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

/// 0x92 nonce message: `[92] LEB(((v8<<32)|(v8^0x63E25F26))<<1 [, sign])`.
pub fn build_92(v8: u32) -> Vec<u8> {
    const V9: u64 = 0x63E2_5F26;
    let x = ((v8 as u64) << 32) | ((v8 as u64) ^ V9);
    let mut zz = x << 1;
    if v8 & 0x8000_0000 != 0 {
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

/// 0x90 join-config message: the native capture template with the JSON
/// region replaced by this session's `RandomSeed1` / `APIsecurityToken` /
/// `__joinTicket` (a JSON string of the two joinTicket fields), the LEB
/// length rewritten, and the final 20 bytes preserved. Port of
/// session.cpp `build90`.
pub fn build_90(cfg: &JoinConfig) -> Result<Vec<u8>, String> {
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

    let body = serde_json::to_string(&tmpl).map_err(|e| format!("0x90 serialize: {e}"))?;
    let mut out = raw[..start].to_vec();
    out.extend_from_slice(&leb128(body.len() as u64));
    out.extend_from_slice(body.as_bytes());
    out.extend_from_slice(&raw[raw.len() - 20..]);
    Ok(out)
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
    routes_at_ms: i64,
    post_answer_bytes: u64,
    post_answer_reported: bool,
    now_ms: u64,
    events: Vec<FlowEvent>,
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
            answered: false,
            routes_at_ms: -1,
            post_answer_bytes: 0,
            post_answer_reported: false,
            now_ms,
            events: Vec::new(),
        }
    }

    pub fn answered(&self) -> bool {
        self.answered
    }

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
        let mut ctrl = stream_header(APP, CTRL_CHANNEL).to_vec();
        ctrl.extend_from_slice(&frame(&[0x00, 0x00]));
        for &(chan, wire) in &[(3u32, 2u32), (5, 4), (6, 6), (11, 8)] {
            ctrl.extend_from_slice(&frame(&build_open_unreliable(APP, chan, wire)));
        }
        actions.push(Action::OpenStream {
            app: APP,
            chan: CTRL_CHANNEL,
            data: ctrl,
        });

        // chan1: early-auth, A7, 0x90, 0x92, 0x8A, 0x8F.
        let mut one = stream_header(APP, 1).to_vec();
        if let Some(early) = &self.cfg.early_auth {
            one.extend_from_slice(&frame(&build_early_auth(early)));
            self.log(format!("early-auth staged ({}B payload)", early.pre.len() + early.auth.len()));
        } else {
            self.log("early-auth material missing — skipping 0xA8".into());
        }
        match self.cfg.a7_mode {
            A7Mode::Real => one.extend_from_slice(&frame(A7_REAL)),
            A7Mode::Empty => one.extend_from_slice(&frame(&[0xA7, 0x00, 0x00])),
            A7Mode::Skip => {}
        }
        let m90 = match build_90(&self.cfg) {
            Ok(m) => Some(m),
            Err(e) => {
                self.log(format!("0x90 skipped: {e}"));
                None
            }
        };
        let mut v8_seed = self.now_ms.wrapping_mul(0x9E37_79B1).wrapping_add(0x51_7C_C1_B7);
        if v8_seed == 0 {
            v8_seed = 0x8000_0001;
        }
        if let Some(m90) = &m90 {
            one.extend_from_slice(&frame(m90));
        }
        one.extend_from_slice(&frame(&build_92(v8_seed)));
        one.extend_from_slice(&frame(&build_8a(&self.cfg)));
        one.extend_from_slice(&frame(&[0x8F, 0x00]));
        actions.push(Action::OpenStream {
            app: APP,
            chan: 1,
            data: one,
        });

        self.log(format!(
            "on_connected: {} actions staged (0x90 {})",
            actions.len(),
            if m90.is_some() { "included" } else { "skipped" }
        ));
        actions
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
            self.chan1_rx.extend_from_slice(data);
            if self.chan1_rx.len() > 16 << 20 {
                // The challenge walk only needs a bounded tail; keep the
                // buffer from growing for long replication sessions.
                self.chan1_rx.drain(..(self.chan1_rx.len() - (4 << 20)));
            }
            if self.challenge_seen {
                self.post_answer_bytes += data.len() as u64;
                if !self.post_answer_reported {
                    self.post_answer_reported = true;
                    self.events.push(FlowEvent::ConnectedStageReached {
                        detail: format!("post-answer channel-1 traffic: {}B (world state / peer assignment)", data.len()),
                    });
                }
            }
            self.try_challenge()
        } else {
            self.log(format!("RX app={app} chan={chan} {}B", data.len()));
            Vec::new()
        }
    }

    fn streams_key(&self, stream: i64) -> (u32, u32) {
        self.streams
            .iter()
            .find(|(_, sid)| **sid == stream)
            .map(|(k, _)| *k)
            .unwrap_or((0, 0))
    }

    /// Walk chan1 for a complete 0x9B frame; solve and answer.
    fn try_challenge(&mut self) -> Vec<Action> {
        if self.challenge_seen {
            return Vec::new();
        }
        let Some((off, len)) = challenge::find_challenge_frame(&self.chan1_rx) else {
            return Vec::new();
        };
        let msg = &self.chan1_rx[off..off + len];
        let Some((blob, u1, u2)) = challenge::extract_challenge(msg).ok() else {
            self.challenge_seen = true; // malformed; do not re-scan forever
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
            elapsed_ms: report.solve_ms,
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
            "answer 0x{} sent (u1=0x{} u2=0x{}, solved in {}ms)",
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
        if stream == self.chan1 {
            self.chan1_reset = true;
            self.log(format!("chan1 stream {stream} reset by peer — will re-open for answer"));
        }
    }

    /// Timer tick: fires the route declarations ~60 ms after the answer.
    pub fn on_tick(&mut self, now_ms: u64) -> Vec<Action> {
        self.now_ms = now_ms;
        if self.routes_at_ms >= 0 && now_ms as i64 >= self.routes_at_ms {
            self.routes_at_ms = -1;
            let mut data = stream_header(APP, 11).to_vec();
            for r in ROUTES {
                data.extend_from_slice(&frame(&hex(r)));
            }
            self.log(format!("routes: 4 declarations ({}B) on chan11", data.len()));
            return vec![Action::OpenStream {
                app: APP,
                chan: 11,
                data,
            }];
        }
        Vec::new()
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
        for v8 in [0u32, 1, 0x7FFF_FFFF, 0x8000_0000, 0x8000_0001, 0xFFFF_FFFF] {
            let m = build_92(v8);
            assert_eq!(m[0], 0x92);
            let (zz, k) = leb128_decode(&m[1..]).unwrap();
            assert_eq!(m.len(), 1 + k);
            let x = ((v8 as u64) << 32) | ((v8 as u64) ^ 0x63E2_5F26);
            let mut expect = x << 1;
            if v8 & 0x8000_0000 != 0 {
                expect = !expect;
            }
            assert_eq!(zz, expect, "v8={v8}");
        }
    }

    #[test]
    fn build_90_patches_template() {
        let cfg = JoinConfig {
            user_id: 2_655_886_518,
            client_ticket: "t;2".into(),
            session_id: "s".into(),
            random_seed1: Some("SEED-XYZ".into()),
            api_security_token: Some("TOKEN-ABC".into()),
            serialized_client_fields: Some("SCF".into()),
            encrypted_server_fields: Some("ESF".into()),
            early_auth: None,
            a7_mode: A7Mode::Skip,
        };
        let m = build_90(&cfg).expect("build_90");
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
        // Other template keys survive.
        assert_eq!(v["UserId"].as_i64(), Some(2_655_886_518));
    }

    #[test]
    fn a7_real_template_starts_with_a7() {
        assert_eq!(A7_REAL[0], 0xA7);
        assert_eq!(A7_REAL.len(), 99);
    }

    #[test]
    fn routes_parse_as_frames() {
        for r in ROUTES {
            let b = hex(r);
            // Each route is a single compact-varint-framed 0xA6 message.
            let n = 1usize << (b[0] >> 6);
            let mut v = u64::from(b[0] & 0x3F);
            for x in &b[1..n] {
                v = (v << 8) | u64::from(*x);
            }
            assert_eq!(v as usize, b.len() - n, "route {r}");
            assert_eq!(b[n], 0xA6, "route {r} tag");
        }
    }
}
