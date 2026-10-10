//! Roblox 0x9B anti-cheat challenge solver.
//!
//! Rust port of the validated pipeline from the Boblox reference client
//! (github.com/kingdudely/Boblox, `client/src/{blob,standardize,solve}.cpp` +
//! `rbx_runtime` runner):
//!
//! ```text
//! [9b][u1 u32le][u2 u32le][len u32le][blob]
//!   -> RSB1 XOR keystream + xxhash32(seed=0x2A) verify + zstd   (blob.cpp)
//!   -> wire opcode standardization (REMAP/OPLEN tables)          (standardize.cpp)
//!   -> Luau v13 -> v11 header normalization (loader gate)        (this crate)
//!   -> run on a Luau VM with a bit-exact Roblox sandbox          (runner.cpp)
//!   -> u32 answer  ->  [9b][u2 u32le][answer u32le]              (9-byte reply)
//! ```
//!
//! Wire bytecode facts (all verified against the reference corpus):
//! * The header is Luau v13: `[ver u8][tv u8][nStrings varint][strings]
//!   [tv3 remap][nProtos varint][protos]`. Each proto starts with a
//!   `protoSize` varint (v12+) covering the rest of the proto; the loader
//!   jumps to `protoStart + protoSize` after parsing.
//! * Instructions are 32-bit words, opcode in the low byte, remapped by
//!   `INTERNAL_OP_TO_STANDARD[WIRE_OP_REMAP[wire_op]]` — operands untouched.
//! * `flags & LPF_INLINABLE (0x08)` adds a v12 `cost` varint64 field.
//!
//! The embedded luaur VM accepts bytecode versions 3..=11, so
//! [`normalize_for_luaur`] re-emits the program without the v12-only fields
//! (byte-for-byte otherwise identical). The VM then sees exactly the
//! instruction stream the native runner feeds its pinned upstream Luau.
//!
//! Verified against the reference project's captured-answer corpus: the
//! in-crate tests (`solve_matches_native_captured_answers` et al.) replay
//! `tests/fixtures/challenge/*` (independent live captures) and require all
//! answers to match the native client's.

use crate::challenge_tables::{INTERNAL_OP_TO_STANDARD, WIRE_OP_REMAP};
use std::time::Instant;

// ---------------------------------------------------------------------------
// xxh32 (reference implementation — the blob digest uses seed 0x2A)
// ---------------------------------------------------------------------------

const PRIME32_1: u32 = 0x9E37_79B1;
const PRIME32_2: u32 = 0x85EBC_A77;
const PRIME32_3: u32 = 0xC2B2_AE3D;
const PRIME32_4: u32 = 0x27D4_EB2F;
const PRIME32_5: u32 = 0x1656_67B1;

pub fn xxh32(data: &[u8], seed: u32) -> u32 {
    let n = data.len();
    let mut i: usize = 0;
    let (mut h1, mut h2, mut h3, mut h4) = (
        seed.wrapping_add(PRIME32_1).wrapping_add(PRIME32_2),
        seed.wrapping_add(PRIME32_2),
        seed,
        seed.wrapping_sub(PRIME32_1),
    );
    while i + 16 <= n {
        let rd = |off: usize| u32::from_le_bytes(data[i + off..i + off + 4].try_into().unwrap());
        h1 = h1.wrapping_add(rd(0).wrapping_mul(PRIME32_2)).rotate_left(13).wrapping_mul(PRIME32_1);
        h2 = h2.wrapping_add(rd(4).wrapping_mul(PRIME32_2)).rotate_left(13).wrapping_mul(PRIME32_1);
        h3 = h3.wrapping_add(rd(8).wrapping_mul(PRIME32_2)).rotate_left(13).wrapping_mul(PRIME32_1);
        h4 = h4.wrapping_add(rd(12).wrapping_mul(PRIME32_2)).rotate_left(13).wrapping_mul(PRIME32_1);
        i += 16;
    }
    let mut h = if i > 0 {
        h1.rotate_left(1)
            .wrapping_add(h2.rotate_left(7))
            .wrapping_add(h3.rotate_left(12))
            .wrapping_add(h4.rotate_left(18))
    } else {
        // Short-input path (reference: h = seed + P5).
        seed.wrapping_add(PRIME32_5)
    };
    h = h.wrapping_add(n as u32);
    // Note: Roblox's loader (and Boblox's reference port) use a
    // non-canonical tail: lane*P3 .. *P4 and byte*P5 .. *P1. The captured
    // blobs are the ground truth — canonical XXH32 does NOT verify them.
    while i + 4 <= n {
        let v = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
        h = h
            .wrapping_add(v.wrapping_mul(PRIME32_3))
            .rotate_left(17)
            .wrapping_mul(PRIME32_4);
        i += 4;
    }
    while i < n {
        h = h
            .wrapping_add((data[i] as u32).wrapping_mul(PRIME32_5))
            .rotate_left(11)
            .wrapping_mul(PRIME32_1);
        i += 1;
    }
    h ^= h >> 15;
    h = h.wrapping_mul(PRIME32_2);
    h ^= h >> 13;
    h = h.wrapping_mul(PRIME32_3);
    h ^= h >> 16;
    h
}

// ---------------------------------------------------------------------------
// Challenge extraction (chan1 message byte 0x9B)
// ---------------------------------------------------------------------------

/// Extract `(blob, u1, u2)` from a raw 0x9B message:
/// `[9b][u1 u32le][u2 u32le][len u32le][blob[len]]`.
pub fn extract_challenge(msg: &[u8]) -> Result<(&[u8], u32, u32), String> {
    if msg.len() < 13 || msg[0] != 0x9B {
        return Err("not a 0x9B message".into());
    }
    let u1 = u32::from_le_bytes(msg[1..5].try_into().unwrap());
    let u2 = u32::from_le_bytes(msg[5..9].try_into().unwrap());
    let ln = u32::from_le_bytes(msg[9..13].try_into().unwrap()) as usize;
    if ln > msg.len() - 13 {
        return Err(format!(
            "0x9B blob length {ln} exceeds message ({})",
            msg.len()
        ));
    }
    Ok((&msg[13..13 + ln], u1, u2))
}

/// Scan a stream of compact-varint-framed messages (the channel-1 receive
/// stream, with the 7-byte RbxTransport stream header already stripped) for
/// the first complete 0x9B challenge frame.
///
/// Returns `(message_offset, message_length)` in the frame stream.
pub fn find_challenge_frame(data: &[u8]) -> Option<(usize, usize)> {
    // Verbatim port of rbxclient::findChallengeFrame (Boblox blob.cpp). The
    // compactVarint size is fixed by the top two bits: 1 << (b0 >> 6) bytes,
    // value = (b0 & 0x3F) followed by n-1 raw little-endian bytes.
    //
    // Returns (body_offset, body_length) of the first complete 0x9B frame;
    // None means "wait for more data" (incomplete prefix/body) — exactly the
    // reference's false return.
    let mut off = 0usize;
    while off < data.len() {
        let b0 = data[off];
        let n = 1usize << (b0 >> 6);
        if n > data.len() - off {
            return None; // incomplete length prefix
        }
        let mut v: u64 = u64::from(b0 & 0x3F);
        for i in 1..n {
            v = (v << 8) | u64::from(data[off + i]);
        }
        if v == 0 {
            off += n;
            continue;
        }
        if v > (data.len() - off - n) as u64 {
            return None; // incomplete body: wait for more data
        }
        let mlen = v as usize;
        if mlen >= 13 && data[off + n] == 0x9B {
            return Some((off + n, mlen));
        }
        off += n + mlen;
    }
    None
}

// ---------------------------------------------------------------------------
// Blob decoding (blob.cpp)
// ---------------------------------------------------------------------------

/// Decode an RSB1-challenge blob into the raw wire bytecode.
///
/// Keystream: `key[i] = blob[i] ^ 'RSB1'[i] + {0,0xD7,0xAE,0x85}[i]` (i<4),
/// `plain[p] = blob[p] ^ ((41*p + key[p & 3]) & 0xFF)`. Integrity:
/// `le32(key) == xxhash32(plain, 0x2A)`. Mode at plain[4..8]: 0 = raw
/// bytecode, nonzero = zstd-decoded to exactly `mode` bytes.
pub fn decode_blob(blob: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < 13 {
        return Err(format!("challenge blob too short: {} bytes", blob.len()));
    }
    const MAGIC: [u8; 4] = *b"RSB1";
    const ADD: [u8; 4] = [0x00, 0xD7, 0xAE, 0x85];
    let key = [0, 1, 2, 3]
        .map(|i| (blob[i] ^ MAGIC[i]).wrapping_add(ADD[i]))
        .try_into()
        .unwrap();
    let want = u32::from_le_bytes(key);
    let mut plain = vec![0u8; blob.len()];
    for p in 0..blob.len() {
        plain[p] = blob[p] ^ ((41 * p as u32 + key[p & 3] as u32) as u8);
    }
    let got = xxh32(&plain, 0x2A);
    if got != want {
        return Err(format!(
            "challenge blob checksum mismatch: got 0x{got:08x}, want 0x{want:08x}"
        ));
    }
    let mode = u32::from_le_bytes(plain[4..8].try_into().unwrap());
    let payload = &plain[8..];
    if mode == 0 {
        return Ok(payload.to_vec());
    }
    if mode > 64 << 20 {
        return Err(format!("unreasonable zstd output size {mode}"));
    }
    if payload.len() < 4 || &payload[..4] != b"\x28\xb5\x2f\xfd" {
        return Err("missing zstd frame magic in challenge blob".into());
    }
    match zstd::stream::decode_all(payload) {
        Ok(out) => {
            if out.len() as u32 != mode {
                return Err(format!(
                    "zstd decoded {} bytes, mode field says {mode}",
                    out.len()
                ));
            }
            Ok(out)
        }
        Err(e) => Err(format!("zstd decode failed: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Wire bytecode layout (Luau v13) — parse, standardize, normalize
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StandardizeStats {
    pub protos: usize,
    pub starts: usize,
}

/// Roblox's loader instruction-length check (exact mask test from
/// libroblox.so `sub_27509DA`); identical to upstream `Luau::getOpLength`
/// for every opcode the loader knows.
fn op_length(std_op: u8) -> u32 {
    const MASK1: u64 = 0x8294_0000_3F02_323;
    const MASK2: u64 = 0x0001_7E7B;
    let v2 = std_op as i32 - 7;
    if (0..=0x3B).contains(&v2) && (MASK1 >> v2) & 1 == 1 {
        return 2;
    }
    let v4 = std_op as i32 - 74;
    if (0..=0x10).contains(&v4) && (MASK2 >> v4) & 1 == 1 {
        return 2;
    }
    1
}

fn varint(buf: &[u8], off: &mut usize) -> Result<u64, String> {
    let mut v: u64 = 0;
    let mut s = 0;
    loop {
        let b = *buf.get(*off).ok_or("truncated bytecode: varint")?;
        *off += 1;
        v |= (b as u64 & 0x7F) << s;
        if b & 0x80 == 0 {
            break;
        }
        s += 7;
        if s > 63 {
            return Err("truncated bytecode: varint overflow".into());
        }
    }
    Ok(v)
}

fn varint64(buf: &[u8], off: &mut usize) -> Result<u128, String> {
    let mut v: u128 = 0;
    let mut s: u32 = 0;
    loop {
        let b = *buf.get(*off).ok_or("truncated bytecode: varint64")?;
        *off += 1;
        v |= (b as u128 & 0x7F) << s;
        if b & 0x80 == 0 {
            break;
        }
        s += 7;
        if s > 126 {
            return Err("truncated bytecode: varint64 overflow".into());
        }
    }
    Ok(v)
}

/// Proto extent in the v12+ wire layout (what [`standardize`] needs).
#[derive(Clone, Copy, Debug)]
struct ProtoExtent {
    psize_off: usize, // offset of the protoSize varint
    psize_len: usize, // its byte length
    pstart: usize,    // proto body start (psize_off + psize_len)
    body_end: usize,  // pstart + psize
    flags: u8,
    code_off: usize, // start of the instruction words
    sizecode: usize, // instruction word count
}

/// Walk the program header + proto headers (v12+ layout). Mirrors
/// Boblox `standardize.cpp::parseProtos`.
fn parse_proto_extents(buf: &[u8]) -> Result<(usize, Vec<ProtoExtent>), String> {
    let mut off = 0usize;
    if buf.len() < 2 {
        return Err(format!("bytecode too short: {} bytes", buf.len()));
    }
    let version = buf[0];
    if !(12..=14).contains(&version) {
        return Err(format!(
            "bytecode version 0x{version:02x} not in the v12+ wire layout"
        ));
    }
    off += 2; // version + table version
    let nstrings = varint(buf, &mut off)? as usize;
    for _ in 0..nstrings {
        let ln = varint(buf, &mut off)? as usize;
        if off + ln > buf.len() {
            return Err("truncated bytecode: string table".into());
        }
        off += ln;
    }
    let tv = buf[1];
    if tv == 3 {
        let mut idx = *buf.get(off).ok_or("truncated bytecode: remap table")?;
        off += 1;
        while idx != 0 {
            varint(buf, &mut off)?; // string index (value not needed)
            idx = *buf.get(off).ok_or("truncated bytecode: remap table")?;
            off += 1;
        }
    }
    let nprotos = varint(buf, &mut off)? as usize;
    if nprotos > buf.len() - off {
        return Err("proto count exceeds program".into());
    }
    let mut protos = Vec::with_capacity(nprotos);
    for _ in 0..nprotos {
        let psize_off = off;
        let psize = varint(buf, &mut off)? as usize;
        let psize_len = off - psize_off;
        let pstart = off;
        if psize > buf.len() - pstart {
            return Err("proto overruns program".into());
        }
        if pstart + 5 > pstart + psize {
            return Err("truncated bytecode: proto header".into());
        }
        let flags = buf[pstart + 4];
        off += 5;
        if tv == 1 || tv == 2 || tv == 3 {
            let ts = varint(buf, &mut off)? as usize;
            if off.saturating_add(ts) > pstart + psize {
                return Err("truncated bytecode: proto types".into());
            }
            off += ts;
        }
        let sizecode = varint(buf, &mut off)? as usize;
        let code_off = off;
        if code_off + sizecode * 4 > pstart + psize {
            return Err("truncated bytecode: code section".into());
        }
        protos.push(ProtoExtent {
            psize_off,
            psize_len,
            pstart,
            body_end: pstart + psize,
            flags,
            code_off,
            sizecode,
        });
        off = pstart + psize;
    }
    Ok((off, protos))
}

/// Rewrite every instruction word's opcode byte to standard Luau (operands
/// untouched), walking each proto's code exactly like the native loader.
pub fn standardize(buf: &mut [u8]) -> Result<StandardizeStats, String> {
    let (_tail_off, protos) = parse_proto_extents(buf)?;
    let mut stats = StandardizeStats {
        protos: protos.len(),
        starts: 0,
    };
    for p in &protos {
        if p.sizecode > (buf.len() - p.code_off) / 4 {
            return Err("sizecode exceeds program".into());
        }
        let mut pc = 0usize;
        while pc < p.sizecode {
            let word_off = p.code_off + 4 * pc;
            let wire_op = buf[word_off];
            let std = INTERNAL_OP_TO_STANDARD[WIRE_OP_REMAP[wire_op as usize] as usize];
            buf[word_off] = std;
            pc += op_length(std) as usize;
            stats.starts += 1; // instructions walked (reference semantics)
        }
        if pc != p.sizecode {
            return Err(format!(
                "proto walk failed: pc {pc} != sizecode {}",
                p.sizecode
            ));
        }
    }
    Ok(stats)
}

// --- normalization to luaur's accepted range (v13 -> v11) ------------------

/// Proto body parse positions needed to cut the v12-only fields out.
struct ProtoBody {
    /// Offset (absolute) where the v12 `cost` field starts, or None.
    cost_off: Option<usize>,
    /// Offset just past the last field a v11 loader reads (past cost when
    /// present); bytes up to `body_end` after this are unknown trailing
    /// data the loader skips — dropped in the normalized stream too.
    semantic_end: usize,
}

/// Parse one proto body exactly like the upstream loader (v12+ path) and
/// report where the v12-only fields live.
fn parse_proto_body(buf: &[u8], p: &ProtoExtent, tv: u8) -> Result<ProtoBody, String> {
    let mut off = p.pstart;
    let _flags = buf[off + 4];
    off += 5;
    if tv == 1 || tv == 2 || tv == 3 {
        let ts = varint(buf, &mut off)? as usize;
        off += ts;
    }
    let sizecode = varint(buf, &mut off)? as usize;
    off += sizecode * 4; // code words

    let sizek = varint(buf, &mut off)? as usize;
    for _ in 0..sizek {
        let kind = *buf.get(off).ok_or("truncated bytecode: constant kind")?;
        off += 1;
        match kind {
            0 => {} // NIL
            1 => off += 1, // BOOLEAN
            2 => off += 8, // NUMBER (f64)
            3 => {
                varint(buf, &mut off)?;
            } // STRING (string index)
            4 => off += 4, // IMPORT (u32)
            5 => {
                let keys = varint(buf, &mut off)? as usize;
                for _ in 0..keys {
                    varint(buf, &mut off)?;
                }
            } // TABLE
            6 => {
                varint(buf, &mut off)?;
            } // CLOSURE
            7 => off += 16, // VECTOR (4 floats)
            8 => {
                let keys = varint(buf, &mut off)? as usize;
                for _ in 0..keys {
                    varint(buf, &mut off)?;
                    off += 4; // int32 constant index
                }
            } // TABLE_WITH_CONSTANTS
            9 => {
                off += 1;
                varint64(buf, &mut off)?;
            } // INTEGER
            10 => {
                varint(buf, &mut off)?;
                let num_props = varint(buf, &mut off)? as usize;
                let num_methods = varint(buf, &mut off)? as usize;
                for _ in 0..num_props + num_methods {
                    varint(buf, &mut off)?;
                }
            } // CLASS_SHAPE
            11 => off += 32, // VECTORD (4 doubles)
            k => return Err(format!("unknown constant kind {k}")),
        }
    }

    let sizep = varint(buf, &mut off)? as usize;
    for _ in 0..sizep {
        varint(buf, &mut off)?;
    }
    varint(buf, &mut off)?; // linedefined
    varint(buf, &mut off)?; // debugname (string index)

    let lineinfo = *buf.get(off).ok_or("truncated bytecode: lineinfo")?;
    off += 1;
    if lineinfo != 0 {
        let linegaplog2 = *buf.get(off).ok_or("truncated bytecode: lineinfo")? as u32;
        off += 1;
        if linegaplog2 > 31 {
            return Err("impossible linegaplog2".into());
        }
        off += sizecode; // sizecode u8 deltas
        let intervals = ((sizecode.saturating_sub(1)) >> linegaplog2) + 1;
        off += intervals * 4;
    }

    let debuginfo = *buf.get(off).ok_or("truncated bytecode: debuginfo")?;
    off += 1;
    if debuginfo != 0 {
        let nloc = varint(buf, &mut off)? as usize;
        for _ in 0..nloc {
            varint(buf, &mut off)?;
            varint(buf, &mut off)?;
            varint(buf, &mut off)?;
            off += 1;
        }
        let nups = varint(buf, &mut off)? as usize;
        for _ in 0..nups {
            varint(buf, &mut off)?;
        }
    }

    // Feedback vector (v11+): present in v13 programs too.
    let nfeedback = varint(buf, &mut off)? as usize;
    for _ in 0..nfeedback {
        off += 1; // slot type
        varint(buf, &mut off)?; // call target pc
    }

    // v12+: INLINABLE protos carry a cost varint64, then the loader skips to
    // protoStart + protoSize (unknown trailing bytes tolerated).
    let mut cost_off = None;
    if p.flags & 0x08 != 0 {
        cost_off = Some(off);
        varint64(buf, &mut off)?;
    }
    if off > p.body_end {
        return Err("proto body parse ran past protoSize".into());
    }
    Ok(ProtoBody {
        cost_off,
        semantic_end: off,
    })
}

/// Re-emit the program so the embedded luaur VM (bytecode gate 3..=11)
/// accepts it: version byte 13/12/14 -> 11, every proto's `protoSize` varint
/// removed, INLINABLE `cost` fields removed, unknown trailing proto bytes
/// dropped (the native loader skips them too). Every other byte is copied
/// verbatim, so the instruction stream is bit-identical.
pub fn normalize_for_luaur(buf: &[u8]) -> Result<Vec<u8>, String> {
    let version = buf.get(0).ok_or("empty bytecode")?;
    if *version < 12 {
        return Ok(buf.to_vec());
    }
    let tv = buf[1];
    let (tail_off, protos) = parse_proto_extents(buf)?;

    let mut out = Vec::with_capacity(buf.len());
    let mut off = 2usize;
    out.push(11);
    out.push(tv);
    // String-count varint.
    let nstrings = varint(buf, &mut off)? as usize;
    out.extend_from_slice(&buf[2..off]);
    // String table (verbatim).
    for _ in 0..nstrings {
        let ln = varint(buf, &mut off)? as usize;
        out.extend_from_slice(&buf[off..off + ln]);
        off += ln;
    }
    // tv3 userdata-remap table (verbatim).
    if tv == 3 {
        let remap_start = off;
        let mut idx = buf[off];
        off += 1;
        while idx != 0 {
            varint(buf, &mut off)?;
            idx = buf[off];
            off += 1;
        }
        out.extend_from_slice(&buf[remap_start..off]);
    }
    // Proto-count varint (verbatim).
    let pc_start = off;
    let nprotos = varint(buf, &mut off)?;
    out.extend_from_slice(&buf[pc_start..off]);
    debug_assert_eq!(nprotos as usize, protos.len());

    for p in &protos {
        let body = parse_proto_body(buf, p, tv)?;
        // Proto body starting after the psize varint, ending before the
        // v12-only cost field (and any unknown trailing bytes, which the
        // native loader's skip would discard anyway).
        let keep_end = body
            .cost_off
            .unwrap_or(body.semantic_end)
            .min(p.body_end);
        out.extend_from_slice(&buf[p.pstart..keep_end]);
    }

    // Tail: main proto id varint + anything after.
    out.extend_from_slice(&buf[tail_off..]);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Roblox's Random (bit-exact port of the native PCG implementation)
// ---------------------------------------------------------------------------

const K_MUL: u64 = 0x5851_F42D_4C95_7F2D;
const K_INC: u64 = 105;
const K_SEED_ADD: u64 = 0x399D_2694_6951_29DE;

fn d2i32(v: f64) -> i32 {
    if v >= -2147483648.0 && v < 2147483648.0 {
        v as i32
    } else {
        (v as i64) as i32
    }
}

/// Roblox's `Random`: PCG-XSH-RR (64-bit state, increment 105), seeded
/// exactly like the native implementation. Validated against 20/20 native
/// RNG extractions in the reference project's captures.
pub struct RobloxRandom {
    pub state: u64,
}

impl RobloxRandom {
    pub fn from_seed(seed: Option<f64>) -> Self {
        let state = match seed {
            Some(s) if s.is_finite() && s.abs() <= 9007199254740992.0 => {
                let v = s as i64;
                K_MUL * (v as u64) + K_SEED_ADD
            }
            _ => K_SEED_ADD,
        };
        Self { state }
    }

    fn out32(state: u64) -> u32 {
        let x = ((state >> 27) ^ (state >> 45)) as u32;
        let r = ((state >> 59) & 31) as u32;
        if r == 0 {
            x
        } else {
            x.rotate_right(r)
        }
    }

    /// `NextInteger(min, max)` — native small-range path (ranges are always
    /// <= 2^31, so the wide 64-bit path is unreachable in practice).
    pub fn next_integer(&mut self, min: f64, max: f64) -> f64 {
        let (lo, range) = if min <= max {
            (d2i32(min), (d2i32(max) as i64 - d2i32(min) as i64) as u64)
        } else {
            (d2i32(max), (d2i32(min) as i64 - d2i32(max) as i64) as u64)
        };
        let next = K_MUL * self.state + K_INC;
        let v5 = Self::out32(self.state);
        if (range >> 1) > 0x7FFF_FFFE {
            // Wide path (decompiled verbatim; dead for int32 ranges).
            let v7 = Self::out32(next);
            self.state = K_MUL * next + K_INC;
            let v8 = range + 1;
            if v8 != 0 {
                let res = (((v7 as u64) * (v8 as u32 as u64)) >> 32)
                    + (v7 as u64) * (v8 >> 32)
                    + (lo as u64)
                    + ((v5 as u64 * (v8 >> 32)) >> 32);
                return (res as u32) as i32 as f64;
            }
            return ((v5 as u64) | ((v7 as u64) << 32)) as u64 as u32 as i32 as f64;
        }
        self.state = next;
        (lo as u32 + ((v5 as u64 * (range + 1)) >> 32) as u32) as f64
    }

    /// `NextNumber(min, max)` — two consecutive outputs packed into a
    /// [0,1) double, scaled to the range (native formula).
    pub fn next_number(&mut self, min: f64, max: f64) -> f64 {
        let next = K_MUL * self.state + K_INC;
        let a = Self::out32(self.state);
        let b = Self::out32(next);
        self.state = K_MUL * next + K_INC;
        let frac = (a as u64 | (b as u64) << 32) as f64 / 18446744073709551616.0;
        min + frac * (max - min)
    }
}

// ---------------------------------------------------------------------------
// Luau sandbox + runner
// ---------------------------------------------------------------------------

/// Convert a program return value to the wire answer (native `d2ans`):
/// integers within 2^53 truncate to u32; anything else maps to 0x80000000.
pub fn double_to_answer(v: f64) -> u32 {
    if v >= -9007199254740992.0 && v <= 9007199254740992.0 {
        (v as i64) as u32
    } else {
        0x8000_0000
    }
}

#[derive(Clone, Debug)]
pub struct SolveReport {
    pub answer: u32,
    pub u1: u32,
    pub u2: u32,
    pub solve_ms: u128,
    pub blob_bytes: usize,
    pub wire_bytes: usize,
    pub protos: usize,
    pub starts: usize,
}

/// Solve a raw 0x9B challenge message (full frame, including the 0x9B tag).
pub fn solve_message(msg: &[u8], job: &str) -> Result<SolveReport, String> {
    let started = Instant::now();
    let (blob, u1, u2) = extract_challenge(msg)?;
    let wire = decode_blob(blob)?;
    let wire_bytes = wire.len();
    let mut code = wire;
    let stats = standardize(&mut code)?;
    let program = normalize_for_luaur(&code)?;
    // Roblox production VM: CALLFB feedback enabled (the bytecode uses it).
    luaur::common::FFlag::LuauCallFeedback.set(true);
    let answer = run_program(&program, u2, u1, job)?;
    Ok(SolveReport {
        answer,
        u1,
        u2,
        solve_ms: started.elapsed().as_millis(),
        blob_bytes: blob.len(),
        wire_bytes,
        protos: stats.protos,
        starts: stats.starts,
    })
}

/// Build the 9-byte reply: `[9b][u2 u32le][answer u32le]`.
pub fn build_response(u2: u32, answer: u32) -> [u8; 9] {
    let mut r = [0u8; 9];
    r[0] = 0x9B;
    r[1..5].copy_from_slice(&u2.to_le_bytes());
    r[5..9].copy_from_slice(&answer.to_le_bytes());
    r
}

fn make_random_instance(lua: &luaur::Lua, state: u64) -> luaur::rt::Result<luaur::Table> {
    use std::cell::RefCell;
    use std::rc::Rc;

    let st = Rc::new(RefCell::new(state));
    let meta = lua.create_table();
    let ni = st.clone();
    meta.set(
        "NextInteger",
        lua.create_function(move |_, (_self, min, max): (luaur::Table, f64, f64)| {
            let mut r = RobloxRandom { state: *ni.borrow() };
            let v = r.next_integer(min, max);
            *ni.borrow_mut() = r.state;
            Ok(v)
        })?,
    )?;
    let nn = st.clone();
    meta.set(
        "NextNumber",
        lua.create_function(
            move |_, (_self, rest): (luaur::Table, luaur::Variadic<f64>)| {
                let mut r = RobloxRandom { state: *nn.borrow() };
                let v = match rest.to_vec().as_slice() {
                    [] => r.next_number(0.0, 1.0),
                    [a, b, ..] => r.next_number(*a, *b),
                    _ => return Ok(r.next_number(0.0, 1.0)),
                };
                *nn.borrow_mut() = r.state;
                Ok(v)
            },
        )?,
    )?;
    let nc = st.clone();
    meta.set(
        "Clone",
        lua.create_function(move |lua, _self: luaur::Table| {
            let state = *nc.borrow();
            make_random_instance(lua, state)
        })?,
    )?;
    // Methods live on the metatable; instance lookups fall through to it.
    meta.set("__index", meta.clone())?;
    let instance = lua.create_table();
    instance.set_metatable(Some(meta))?;
    Ok(instance)
}

fn install_challenge_sandbox(lua: &luaur::Lua, job: &str) -> luaur::rt::Result<()> {
    use luaur::rt::Value;

    // Leave the standard library as-is: luaur's `os` is exactly clock/date/
    // time (no os.exit), matching Roblox's production VM, and the challenge
    // may use os.clock()/os.time().
    let g = lua.globals();

    // game.JobId + GetService("RunService"):IsStudio()
    let game = lua.create_table();
    game.set("JobId", job.to_string())?;
    let run_service = lua.create_table();
    run_service.set("IsStudio", lua.create_function(|_, (): ()| Ok(false))?)?;
    let rs_clone = run_service.clone();
    game.set(
        "GetService",
        lua.create_function(move |lua, (_game, name): (luaur::Table, String)| {
            if name == "RunService" {
                Ok(rs_clone.clone())
            } else {
                Ok(lua.create_table())
            }
        })?,
    )?;
    game.set("RunService", run_service.clone())?;
    g.set("game", game)?;
    g.set("RunService", run_service)?;

    // UserSettings(): a fresh table per call whose tostring() is
    // "a"*19 + "Q" (byte sum 1924 = 1843 + 81, matching the native object).
    let us_metatable = lua.create_table();
    let us_string = "aaaaaaaaaaaaaaaaaaaQ".to_string();
    us_metatable.set(
        "__tostring",
        lua.create_function(move |_, (): ()| Ok(us_string.clone()))?,
    )?;
    let us_meta = us_metatable.clone();
    g.set(
        "UserSettings",
        lua.create_function(move |lua, (): ()| {
            let t = lua.create_table();
            t.set_metatable(Some(us_meta.clone()))?;
            Ok(t)
        })?,
    )?;

    // newproxy(true): a table with a fresh metatable. The program pokes
    // metatable.__namecall = function(_, n) return 42 + 10 end and calls
    // p:Foo(10) — namecall dispatch (LOP_NAMECALL) is what Roblox's native
    // VM does here.
    g.set(
        "newproxy",
        lua.create_function(|lua, mt: Value| {
            let t = lua.create_table();
            if let Value::Boolean(true) = mt {
                t.set_metatable(Some(lua.create_table()))?;
            }
            Ok(t)
        })?,
    )?;

    // Roblox's Random (PCG, seeded exactly like the native class).
    let random_class = lua.create_table();
    random_class.set(
        "new",
        lua.create_function(
            move |lua, seed: Option<f64>| {
                make_random_instance(lua, RobloxRandom::from_seed(seed).state)
            },
        )?,
    )?;
    g.set("Random", random_class)?;
    Ok(())
}

/// Run one standardized challenge program on a fresh Luau state, exactly
/// like Boblox's runner: `luau_load(L, "=challenge", code, len, 0)` then a
/// protected call with the two arguments as doubles, one return.
fn run_program(code: &[u8], arg1: u32, arg2: u32, job: &str) -> Result<u32, String> {
    use core::ffi::c_char;
    use luaur::{Function, Lua, MultiValue, Value};

    let lua = Lua::new();
    install_challenge_sandbox(&lua, job)
        .map_err(|e| format!("challenge sandbox setup failed: {e}"))?;

    let chunkname = b"=challenge\0";
    // exec_raw and luau_load are unsafe fns in the luaur fork. The inner
    // block trips `unused_unsafe` (nested under the outer) — cosmetic only.
    let loaded: MultiValue = unsafe {
        lua.exec_raw((), move |state| unsafe {
            let rc = luaur::vm::functions::luau_load::luau_load(
                state,
                chunkname.as_ptr() as *const c_char,
                code.as_ptr() as *const c_char,
                code.len(),
                0,
            );
            debug_assert_eq!(rc, 1);
        })
    }
    .map_err(|e| format!("challenge bytecode load error: {e}"))?;

    let f: Function = match loaded.get(0) {
        Some(Value::Function(f)) => f.clone(),
        Some(Value::String(s)) => {
            return Err(format!(
                "challenge program rejected: {}",
                s.to_str().unwrap_or_default()
            ))
        }
        other => {
            return Err(format!(
                "challenge program load returned an unexpected value: {other:?}"
            ))
        }
    };

    // The program's arguments arrive as (u2, u1): the second challenge field
    // is the first program argument (xv), the first is yv (decompiled
    // challenge_src.lua).
    let ret: f64 = f
        .call((arg1 as f64, arg2 as f64))
        .map_err(|e| format!("challenge program error: {e}"))?;
    Ok(double_to_answer(ret))
}

// ---------------------------------------------------------------------------
// Self-tests (unit level + captured corpus replay)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xxh32_variants_are_distinct_and_deterministic() {
        // This is Roblox's loader variant (verified by the corpus blobs in
        // solve_matches_native_captured_answers), not canonical XXH32.
        assert_eq!(xxh32(b"", 0), xxh32(b"", 0));
        assert_ne!(xxh32(b"a", 0x2A), xxh32(b"b", 0x2A));
        // 17-byte input exercises stripe + 4-lane + single-byte paths.
        let v = xxh32(&[0x5Au8; 17], 0x2A);
        assert_eq!(v, xxh32(&[0x5Au8; 17], 0x2A));
    }

    #[test]
    fn random_stays_in_range_and_is_deterministic() {
        let mut r = RobloxRandom::from_seed(Some(12345.0));
        let mut r2 = RobloxRandom::from_seed(Some(12345.0));
        for _ in 0..1000 {
            let a = r.next_integer(1.0, 2147483647.0);
            let b = r2.next_integer(1.0, 2147483647.0);
            assert_eq!(a, b);
            assert!((1.0..=2147483647.0).contains(&a));
        }
        let n1 = r.next_number(0.0, 1.0);
        let n2 = r2.next_number(0.0, 1.0);
        assert_eq!(n1, n2);
        assert!((0.0..1.0).contains(&n1));
    }

    #[test]
    fn response_layout() {
        let r = build_response(0xCAFEBABE, 0x12345678);
        assert_eq!(r, [0x9B, 0xBE, 0xBA, 0xFE, 0xCA, 0x78, 0x56, 0x34, 0x12]);
    }

    #[test]
    fn extract_challenge_roundtrip() {
        let mut msg = vec![0x9B];
        msg.extend_from_slice(&0x1111_1111u32.to_le_bytes());
        msg.extend_from_slice(&0x2222_2222u32.to_le_bytes());
        msg.extend_from_slice(&3u32.to_le_bytes());
        msg.extend_from_slice(b"abc");
        let (blob, u1, u2) = extract_challenge(&msg).unwrap();
        assert_eq!(blob, b"abc");
        assert_eq!(u1, 0x1111_1111);
        assert_eq!(u2, 0x2222_2222);
    }

    #[test]
    fn find_challenge_frame_scans_frames() {
        // frame1: small non-challenge msg; frame2: challenge with 3-byte blob
        let mut stream = Vec::new();
        let mut m1 = vec![0x00u8];
        m1.push(0x01);
        stream.push(2); // compact varint len=2
        stream.extend_from_slice(&m1);
        let mut m2 = vec![0x9Bu8, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];
        m2.extend_from_slice(b"abc");
        stream.push(m2.len() as u8);
        stream.extend_from_slice(&m2);
        let (off, len) = find_challenge_frame(&stream).expect("challenge frame");
        let msg = &stream[off..off + len];
        let (blob, u1, u2) = extract_challenge(msg).unwrap();
        assert_eq!(blob, b"abc");
        assert_eq!(u1, 1);
        assert_eq!(u2, 2);
    }

    /// Full-pipeline verification against the Boblox reference project's
    /// live-capture corpus: 8 independent captured 0x9B challenges with the
    /// exact 9-byte answers the native C++ client sent (and the live server
    /// accepted). Every stage runs: extract -> RSB1/zstd decode -> opcode
    /// standardization -> v13->v11 normalization -> Luau sandbox execution.
    #[test]
    fn solve_matches_native_captured_answers() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/challenge");
        let vectors: Vec<serde_json::Value> = serde_json::from_str(
            &std::fs::read_to_string(format!("{dir}/vectors.json")).expect("vectors.json"),
        )
        .expect("vectors.json parse");
        assert!(vectors.len() >= 5, "corpus shrank?");
        for v in &vectors {
            let name = v["name"].as_str().unwrap();
            let job = v["job"].as_str().unwrap();
            let expected = v["answer"].as_u64().unwrap() as u32;
            let msg =
                std::fs::read(format!("{dir}/chal_{name}.bin")).unwrap_or_else(|e| {
                    panic!("missing fixture chal_{name}.bin: {e}")
                });
            let rep = solve_message(&msg, job)
                .unwrap_or_else(|e| panic!("{name}: solve failed: {e}"));
            assert_eq!(
                rep.u1, v["u1"].as_u64().unwrap() as u32,
                "{name}: u1 mismatch"
            );
            assert_eq!(
                rep.u2, v["u2"].as_u64().unwrap() as u32,
                "{name}: u2 mismatch"
            );
            assert_eq!(
                rep.answer, expected,
                "{name}: answer 0x{rep.answer:08x} != native 0x{expected:08x} \
                 (blob {}B wire {}B protos {} starts {} solve {}ms)",
                rep.blob_bytes,
                rep.wire_bytes,
                rep.protos,
                rep.starts,
                rep.solve_ms
            );
            let resp = build_response(rep.u2, rep.answer);
            assert_eq!(resp[0], 0x9B);
            assert_eq!(&resp[1..5], &rep.u2.to_le_bytes());
            assert_eq!(&resp[5..9], &expected.to_le_bytes());
        }
    }

    /// The chan1 walk must find the 0x9B frame inside a compactVarint-framed
    /// stream (with preceding non-challenge frames) and report exactly the
    /// frame body, as Boblox's findChallengeFrame does.
    #[test]
    fn find_challenge_frame_walks_framed_stream() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/challenge");
        let raw =
            std::fs::read(format!("{dir}/chal_comb1.bin")).expect("chal_comb1.bin fixture");
        let frame_len = raw.len() as u64; // >= 0x40: 2-byte form 0x40 | (v >> 8)
        assert!(frame_len < 0x4000);
        let mut stream = Vec::new();
        // A preceding non-challenge frame (0xA8 early-auth shaped) must be skipped.
        stream.push(0x40u8 | ((20u64 >> 8) as u8));
        stream.push(20u64 as u8);
        stream.extend_from_slice(&[0xA8, 0x00, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        stream.push(0x40u8 | ((frame_len >> 8) as u8));
        stream.push(frame_len as u8);
        stream.extend_from_slice(&raw);
        let (off, len) = find_challenge_frame(&stream).expect("challenge frame found");
        assert_eq!(&stream[off..off + len], raw.as_slice());
        // Truncated frame body -> None (wait for more data, reference
        // semantics: an incomplete body means the walk cannot conclude).
        let truncated: Vec<u8> = stream[..off + len - 4].to_vec();
        assert!(find_challenge_frame(&truncated).is_none());
        // Incomplete varint prefix -> None (wait for more data).
        let mut head = stream[..off - 1].to_vec();
        head.push(0x40);
        assert!(find_challenge_frame(&head).is_none());
        // Solve through the framed stream must match the raw-message answer.
        let rep = solve_message(&stream[off..off + len], "472bd6de-39e8-44f7-9fe9-de91abce9dc2").expect("solved");
        assert_eq!(rep.answer, 0x9b32_8c67);
    }

    /// The standardized program must equal the reference project's
    /// byte-exact standard.bin outputs (independent oracle for the
    /// REMAP/OPLEN tables + the loader walk).
    #[test]
    fn standardize_matches_reference_outputs() {
        // Byte-identity against the reference tool's output
        // (boblox tools/challenge_blob.py standardize), vendored as
        // {name}.standard.bin, plus the v13->v11 normalization invariants.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/challenge");
        let vectors: Vec<serde_json::Value> = serde_json::from_str(
            &std::fs::read_to_string(format!("{dir}/vectors.json")).expect("vectors.json"),
        )
        .expect("vectors.json parse");
        for v in &vectors {
            let name = v["name"].as_str().unwrap();
            let msg =
                std::fs::read(format!("{dir}/chal_{name}.bin")).expect("fixture");
            let (blob, u1, u2) = extract_challenge(&msg).unwrap();
            let wire = decode_blob(blob).unwrap();
            let mut code = wire.clone();
            let stats = standardize(&mut code).unwrap();
            let reference = std::fs::read(format!("{dir}/{name}.standard.bin"))
                .unwrap_or_else(|e| panic!("missing fixture {name}.standard.bin: {e}"));
            assert_eq!(
                code, reference,
                "{name}: standardized output differs from reference ({}B vs {}B, starts={})",
                code.len(),
                reference.len(),
                stats.starts
            );
            let norm = normalize_for_luaur(&code).unwrap();
            assert_eq!(norm[0], 11, "{name}: version not patched");
            assert_eq!(norm[1], 3, "{name}: tv changed");
            // The normalized program must be strictly smaller (psize
            // varints + cost fields removed) and keep the main proto tail.
            assert!(
                norm.len() < wire.len(),
                "{name}: normalized {} >= wire {}",
                norm.len(),
                wire.len()
            );
            let _ = (stats, u1, u2);
        }
    }
}
