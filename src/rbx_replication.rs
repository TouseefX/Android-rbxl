//! Join snapshot and replication decoding for the RbxTransport session.
//!
//! After the join burst, channel 1 (and any reliable channel the peer assigns)
//! carries compact-varint-framed messages. This module turns those bytes into
//! typed structures:
//!
//! * `0x81` join start / peer init — the target dump calls its remaining fields
//!   opaque; the observed prefix is seven flag bytes and a LEB128 peer id, and
//!   everything after that is preserved verbatim.
//! * `0x97` snapshot envelope — Zstandard-compressed network schema.
//! * `0x83 0x16 <kind>` `JoinDataItemV2` batches — compressed, and per the
//!   target's builders the compressed and uncompressed byte counts are plain
//!   little-endian `u32`s (`FUN_145dba600` hands a native `uint32_t` to a raw
//!   byte writer), followed by an optional single zero byte. Kinds 2, 3 and 4
//!   are the target's Replication cache categories (cacheable, Part and
//!   non-cacheable instances), which the analysis shows being copied rather
//!   than framed, so this parser *tries* the length pair and falls back to
//!   lossless pass-through instead of picking a side.
//! * `0x83 <item>` delta chains — spawns, event invocations, property updates,
//!   referent lists, clock ticks and nested batches.
//!
//! The snapshot body grammar (schema dictionary, then per-instance records whose
//! property loop is `([u8 member index][value])* [0xFF]`) and the item grammars
//! follow the capture-verified layouts shipped in `dump/`
//! (`reference/boblox/replicate.h` and its parsers), cross-checked against the
//! target-side analysis in `analysis/join_replication_mapping_2026-10-16.md`.
//! That makes these *candidate* parsers, not target-version proof: where the two
//! sources disagree the parser reports which shape it accepted rather than
//! guessing silently, and every unrecognized byte string is preserved and
//! counted.
//!
//! Applying a snapshot builds a real `WeakDom`, with class and property names
//! from the snapshot's own schema and types from the reflection database, so the
//! editor can open a replicated place. Values this client cannot type are
//! counted, never invented: that includes every CFrame rotation, whose
//! quantized-axis table is not recovered, so those apply as position only and
//! say so in the report.

use std::collections::BTreeMap;

use rbx_dom_weak::types::{
    CFrame, Color3, Color3uint8, Enum, NumberRange, Rect, UDim, UDim2, Variant, VariantType,
    Vector2, Vector3,
};
use rbx_dom_weak::{InstanceBuilder, WeakDom};
use rbx_reflection::DataType;

/// Join start / peer init.
pub const JOIN_START_TAG: u8 = 0x81;
/// Replicator data packet, which also introduces delta chains.
pub const DELTA_CHAIN_TAG: u8 = 0x83;
/// `JoinDataItemV2` sub-code, written after [`DELTA_CHAIN_TAG`].
pub const JOIN_DATA_SUBCODE: u8 = 0x16;
/// Compressed network schema / snapshot packet.
pub const SNAPSHOT_TAG: u8 = 0x97;
/// Chain terminator.
pub const CHAIN_END: u8 = 0x00;
/// The implicit DataModel root every snapshot hangs from.
pub const DATAMODEL_ROOT_ID: u32 = 13;
/// Zstandard frame magic, checked before anything is allocated on a peer's
/// advertised length.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
/// Hard ceiling on one inflated frame.
pub const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024 * 1024;
/// How much of an unapplied batch is held while waiting for a schema.
const MAX_PARKED_BATCH_BYTES: usize = 32 * 1024 * 1024;
/// A channel buffer that never closes a frame is trimmed past this.
const MAX_STREAM_BUFFER_BYTES: usize = 64 * 1024 * 1024;

/// Size of a compact varint from its first byte: `1 << (top two bits)`.
pub fn compact_varint_width(first: u8) -> usize {
    1usize << (first >> 6)
}

/// Decode a compact varint at `offset`, returning `(value, width)`. `None` means
/// the buffer does not hold the whole integer, which is a "wait for more"
/// condition on a stream rather than an error.
pub fn read_compact_varint(buffer: &[u8], offset: usize) -> Option<(u64, usize)> {
    let first = *buffer.get(offset)?;
    let width = compact_varint_width(first);
    let end = offset.checked_add(width)?;
    if buffer.len() < end {
        return None;
    }
    let mut value = u64::from(first & 0x3F);
    for &byte in &buffer[offset + 1..end] {
        value = (value << 8) | u64::from(byte);
    }
    Some((value, width))
}

/// Unsigned LEB128, the exact inverse of [`Reader::read_varint`]. Every count,
/// index and integer value in this codec uses it, so a writer has to agree with
/// it byte for byte or the peer misreads the rest of the chain.
pub fn write_varint_leb(mut value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            return out;
        }
    }
}

/// Encode a compact varint. The format has exactly four widths, so a value that
/// does not fit one is rejected instead of silently truncated.
pub fn write_compact_varint(value: u64) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(8);
    if value < 0x40 {
        out.push(value as u8);
    } else if value < 0x4000 {
        out.push(0x40 | (value >> 8) as u8);
        out.push((value & 0xFF) as u8);
    } else if value < 0x4000_0000 {
        out.push(0x80 | (value >> 24) as u8);
        for shift in [16u32, 8, 0] {
            out.push((value >> shift) as u8);
        }
    } else if value < 1u64 << 56 {
        out.push(0xC0 | (value >> 56) as u8);
        for shift in [48u32, 40, 32, 24, 16, 8, 0] {
            out.push((value >> shift) as u8);
        }
    } else {
        return None;
    }
    Some(out)
}

/// A bounds-checked byte reader. Every failure names what it was reading and
/// where, because those offsets are what make a misparse diagnosable.
pub struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.position
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// The bytes consumed since `start`. This is how a fixed-extent value is
    /// captured without every reader having to build its own buffer.
    pub fn extent(&self, start: usize) -> &'a [u8] {
        &self.data[start..self.position]
    }

    pub fn take(&mut self, count: usize, what: &str) -> Result<&'a [u8], String> {
        if self.remaining() < count {
            return Err(format!(
                "truncated {what} at offset {} ({} byte(s) left, {count} wanted)",
                self.position,
                self.remaining()
            ));
        }
        let start = self.position;
        self.position += count;
        Ok(&self.data[start..self.position])
    }

    pub fn read_u8(&mut self, what: &str) -> Result<u8, String> {
        Ok(self.take(1, what)?[0])
    }

    pub fn read_u16_le(&mut self, what: &str) -> Result<u16, String> {
        let bytes = self.take(2, what)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn read_u32_le(&mut self, what: &str) -> Result<u32, String> {
        let bytes = self.take(4, what)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn read_u64_le(&mut self, what: &str) -> Result<u64, String> {
        let bytes = self.take(8, what)?;
        let mut value = 0u64;
        for (index, &byte) in bytes.iter().enumerate() {
            value |= u64::from(byte) << (8 * index);
        }
        Ok(value)
    }

    /// Unsigned LEB128, which every count and index in this codec uses.
    pub fn read_varint(&mut self, what: &str) -> Result<u64, String> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.read_u8(what)?;
            value |= u64::from(byte & 0x7F)
                .checked_shl(shift)
                .unwrap_or(0);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 64 {
                return Err(format!("varint too long ({what}) at offset {}", self.position));
            }
        }
    }

    /// Zigzag-decoded varint, used by the signed values and the `0x26` item.
    pub fn read_zigzag(&mut self, what: &str) -> Result<i64, String> {
        let raw = self.read_varint(what)?;
        Ok(((raw >> 1) as i64) ^ -((raw & 1) as i64))
    }

    /// `[varint len][bytes]`, kept as bytes because the wire is not always UTF-8.
    pub fn read_blob(&mut self, what: &str) -> Result<Vec<u8>, String> {
        let len = self.read_varint(what)?;
        let len = usize::try_from(len)
            .map_err(|_| format!("{what} length {len} exceeds this platform"))?;
        if len > self.remaining() {
            return Err(format!(
                "{what} length {len} overruns the buffer at offset {}",
                self.position
            ));
        }
        Ok(self.take(len, what)?.to_vec())
    }

    /// [`Reader::read_blob`] decoded lossily, for the schema's name tables.
    pub fn read_text(&mut self, what: &str) -> Result<String, String> {
        Ok(String::from_utf8_lossy(&self.read_blob(what)?).into_owned())
    }

    /// Accept a count only when every item behind it must cost at least one
    /// byte, which is what keeps a hostile length from becoming a long loop.
    fn read_bounded_count(&mut self, what: &str) -> Result<usize, String> {
        let count = self.read_varint(what)?;
        if count > self.remaining() as u64 {
            return Err(format!(
                "{what} count {count} exceeds the {} byte(s) left",
                self.remaining()
            ));
        }
        Ok(count as usize)
    }

    /// A `u32` field whose value indexes the buffer, checked here so every
    /// caller gets the same diagnostic.
    fn read_bounded_len(&mut self, what: &str) -> Result<usize, String> {
        let len = usize::try_from(self.read_u32_le(what)?)
            .map_err(|_| format!("{what} exceeds this platform"))?;
        if len > self.remaining() {
            return Err(format!(
                "{what} length {len} overruns the buffer at offset {}",
                self.position
            ));
        }
        Ok(len)
    }
}

/// Inflate a Zstandard frame and require the exact advertised size.
fn decompress_frame(compressed: &[u8], expected: u32, what: &str) -> Result<Vec<u8>, String> {
    if expected > MAX_DECOMPRESSED_BYTES as u32 {
        return Err(format!(
            "{what} advertises {expected} decompressed bytes, above the {MAX_DECOMPRESSED_BYTES} cap"
        ));
    }
    let decoded = zstd::bulk::decompress(compressed, expected as usize)
        .map_err(|error| format!("{what} zstd failed: {error}"))?;
    if decoded.len() != expected as usize {
        return Err(format!(
            "{what} decompressed to {} byte(s), not the advertised {expected}",
            decoded.len()
        ));
    }
    Ok(decoded)
}

fn starts_with_zstd_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&ZSTD_MAGIC)
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// One framed replication message; its id is the body's first byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplicationFrame {
    pub id: u8,
    pub body: Vec<u8>,
}

/// Result of walking a channel buffer as framed messages.
#[derive(Clone, Debug, Default)]
pub struct FrameWalk {
    pub frames: Vec<ReplicationFrame>,
    /// Bytes consumed from the input; the caller keeps everything after this.
    pub consumed: usize,
    /// The next frame is incomplete, so more data may complete it.
    pub incomplete: bool,
    /// A structural violation. The caller should report it rather than wait for
    /// bytes that will never parse.
    pub error: Option<String>,
}

/// Wrap a payload in the replication frame the control stream uses.
pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = write_compact_varint(payload.len() as u64).unwrap_or_default();
    out.extend_from_slice(payload);
    out
}

/// Walk `buffer` as `[compact varint length][body]` records.
pub fn walk_frames(buffer: &[u8]) -> FrameWalk {
    let mut walk = FrameWalk::default();
    let mut offset = 0usize;
    while offset < buffer.len() {
        let Some((length, width)) = read_compact_varint(buffer, offset) else {
            walk.incomplete = true;
            break;
        };
        if length == 0 {
            walk.error = Some(format!("empty frame at offset {offset}"));
            break;
        }
        let Ok(length) = usize::try_from(length) else {
            walk.error = Some(format!("frame length {length} exceeds this platform"));
            break;
        };
        let body_start = offset + width;
        let Some(body_end) = body_start.checked_add(length) else {
            walk.error = Some(format!("frame length {length} overflows at offset {offset}"));
            break;
        };
        if body_end > buffer.len() {
            walk.incomplete = true;
            break;
        }
        let body = &buffer[body_start..body_end];
        let Some(&id) = body.first() else {
            walk.error = Some(format!("frame without a body at offset {body_start}"));
            break;
        };
        walk.frames.push(ReplicationFrame { id, body: body.to_vec() });
        offset = body_end;
    }
    walk.consumed = offset;
    walk
}

// ---------------------------------------------------------------------------
// 0x81 join start / peer init
// ---------------------------------------------------------------------------

/// The `0x81` message. Only the seven flag bytes and the peer id are named; the
/// target's analysis calls the rest of the message opaque, so it is carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInit {
    pub enabled: u8,
    pub flags: u8,
    pub isr_read: u8,
    pub isr_write: u8,
    pub isr_legacy: u8,
    /// Present in both observed sessions; the target's field is conditional on
    /// replicator configuration, so it is reported rather than interpreted.
    pub conditional: u8,
    pub version_tail: u8,
    pub peer: u32,
    pub tail: Vec<u8>,
}

/// Parse an `0x81` body, which starts at the tag byte.
pub fn parse_peer_init(body: &[u8]) -> Result<PeerInit, String> {
    let Some(&tag) = body.first() else {
        return Err("empty 0x81 body".to_string());
    };
    if tag != JOIN_START_TAG {
        return Err(format!("expected join start 0x{JOIN_START_TAG:02x}, found 0x{tag:02x}"));
    }
    if body.len() < 9 {
        return Err(format!(
            "0x81 join start is {} byte(s); the observed prefix needs 8 bytes before the peer id",
            body.len()
        ));
    }
    let mut reader = Reader::new(&body[8..]);
    let peer = reader.read_varint("peer id")?;
    if peer > u64::from(u32::MAX) {
        return Err(format!("0x81 peer id {peer} does not fit a u32"));
    }
    Ok(PeerInit {
        enabled: body[1],
        flags: body[2],
        isr_read: body[3],
        isr_write: body[4],
        isr_legacy: body[5],
        conditional: body[6],
        version_tail: body[7],
        peer: peer as u32,
        tail: body[8 + reader.position()..].to_vec(),
    })
}

// ---------------------------------------------------------------------------
// 0x97 snapshot envelope
// ---------------------------------------------------------------------------

/// Which candidate framing a `0x97` body matched. Two shapes are in circulation
/// for this tag and they differ only in the length encoding, so the parser
/// accepts whichever one validates against the Zstandard frame and reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotFraming {
    /// `[97][codec magic u16][00 00][u32 LE size][zstd .. end of frame]`, the
    /// shape pinned against live captures.
    CodecMagic,
    /// `[97][u32 BE compressed][u32 BE uncompressed][zstd]`, the legacy
    /// `Replicator::compressBitStream` framing this repository already decodes
    /// for the RakNet path.
    LegacyBigEndian,
    /// The same frame with little-endian lengths, which is what the Studio Beta
    /// builders write.
    TargetLittleEndian,
}

/// A decoded `0x97` snapshot envelope.
#[derive(Clone, Debug)]
pub struct SnapshotEnvelope {
    pub framing: SnapshotFraming,
    /// The two codec-version bytes, when the framed form carried them.
    pub codec_magic: Option<[u8; 2]>,
    pub uncompressed_size: u32,
    pub inner: Vec<u8>,
}

/// Capture magics: Crossroads and the Brookhaven builds.
const SNAPSHOT_CODEC_MAGICS: [[u8; 2]; 3] = [[0xA3, 0xCF], [0x31, 0x8B], [0x93, 0xD2]];

/// Decompress a `0x97` body into the schema/snapshot inner buffer.
pub fn parse_snapshot_envelope(body: &[u8]) -> Result<SnapshotEnvelope, String> {
    let Some(&tag) = body.first() else {
        return Err("empty 0x97 body".to_string());
    };
    if tag != SNAPSHOT_TAG {
        return Err(format!("expected snapshot tag 0x{SNAPSHOT_TAG:02x}, found 0x{tag:02x}"));
    }
    // Shape 1: magic, two zero bytes, u32 LE size, then the frame to the end.
    if body.len() >= 13 {
        let magic = [body[1], body[2]];
        if SNAPSHOT_CODEC_MAGICS.iter().any(|known| *known == magic)
            && body[3] == 0
            && body[4] == 0
            && starts_with_zstd_magic(&body[9..])
        {
            let size = u32::from_le_bytes([body[5], body[6], body[7], body[8]]);
            let inner = decompress_frame(&body[9..], size, "0x97 snapshot")?;
            return Ok(SnapshotEnvelope {
                framing: SnapshotFraming::CodecMagic,
                codec_magic: Some(magic),
                uncompressed_size: size,
                inner,
            });
        }
    }
    // Shapes 2 and 3: two lengths then the frame. The order that yields a
    // self-consistent frame wins, so neither endianness is assumed.
    let mut errors = Vec::new();
    for (framing, big_endian) in [
        (SnapshotFraming::LegacyBigEndian, true),
        (SnapshotFraming::TargetLittleEndian, false),
    ] {
        match parse_length_frame(&body[1..], big_endian) {
            Ok((uncompressed_size, inner)) => {
                return Ok(SnapshotEnvelope {
                    framing,
                    codec_magic: None,
                    uncompressed_size,
                    inner,
                })
            }
            Err(error) => errors.push(format!("{framing:?}: {error}")),
        }
    }
    Err(format!(
        "0x97 body matched no known framing ({} byte(s)): {}",
        body.len(),
        errors.join(" | ")
    ))
}

fn parse_length_frame(rest: &[u8], big_endian: bool) -> Result<(u32, Vec<u8>), String> {
    if rest.len() < 8 {
        return Err("buffer shorter than the two length fields".to_string());
    }
    let read = |bytes: &[u8]| -> u32 {
        if big_endian {
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        } else {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
    };
    let compressed = read(&rest[0..4]);
    let uncompressed = read(&rest[4..8]);
    let Some(end) = 8usize.checked_add(compressed as usize) else {
        return Err(format!("compressed length {compressed} overflows"));
    };
    if end > rest.len() {
        return Err(format!(
            "advertised {compressed} compressed byte(s), {} left",
            rest.len() - 8
        ));
    }
    let frame = &rest[8..end];
    if !starts_with_zstd_magic(frame) {
        return Err("no Zstandard magic behind the lengths".to_string());
    }
    Ok((uncompressed, decompress_frame(frame, uncompressed, "0x97 frame")?))
}

// ---------------------------------------------------------------------------
// JoinDataItemV2 batches
// ---------------------------------------------------------------------------

/// The byte after `0x83 0x16`. The target's `JoinDataSubtype` and the capture
/// reference agree on the values; they differ on what the value means, so the
/// parser uses it as a hint and then verifies against the payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinDataKind {
    SharedString,
    CacheableInstances,
    NotCacheableInstances,
    PartInstances,
    Compressed,
    Other(u8),
}

impl JoinDataKind {
    pub fn from_byte(value: u8) -> Self {
        match value {
            1 => Self::SharedString,
            2 => Self::CacheableInstances,
            3 => Self::NotCacheableInstances,
            4 => Self::PartInstances,
            5 => Self::Compressed,
            other => Self::Other(other),
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            Self::SharedString => 1,
            Self::CacheableInstances => 2,
            Self::NotCacheableInstances => 3,
            Self::PartInstances => 4,
            Self::Compressed => 5,
            Self::Other(value) => value,
        }
    }

    /// Whether this kind's payload is a `[u32][u32][zstd]` length pair, which is
    /// the only case where a short tail means a truncated frame. The instance
    /// categories are copied verbatim by the target, so their bytes are the whole
    /// payload however small they are.
    pub fn takes_length_pair(self) -> bool {
        matches!(self, Self::SharedString | Self::Compressed)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SharedString => "shared string",
            Self::CacheableInstances => "cacheable instances",
            Self::NotCacheableInstances => "non-cacheable instances",
            Self::PartInstances => "part instances",
            Self::Compressed => "compressed/asset batch",
            Self::Other(_) => "unrecognized kind",
        }
    }
}

/// How the bytes behind a `JoinDataItemV2` kind byte were read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinDataFraming {
    /// `[u32 LE compressed][u32 LE uncompressed][zstd]`, the shape the target's
    /// builders and the live captures agree on.
    LengthPairLittleEndian,
    /// The same pair with the legacy big-endian lengths.
    LengthPairBigEndian,
    /// No length pair validated, so the payload is kept exactly as received.
    /// The target copies the instance categories this way.
    Opaque,
    /// The message ended at the kind byte.
    Truncated,
}

impl JoinDataFraming {
    pub fn label(self) -> &'static str {
        match self {
            Self::LengthPairLittleEndian => "length pair (LE)",
            Self::LengthPairBigEndian => "length pair (BE)",
            Self::Opaque => "opaque",
            Self::Truncated => "truncated",
        }
    }
}

/// One parsed `JoinDataItemV2` message.
#[derive(Clone, Debug)]
pub struct JoinDataItem {
    pub kind: JoinDataKind,
    pub framing: JoinDataFraming,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    /// Decompressed payload, when a length pair validated.
    pub inner: Option<Vec<u8>>,
    /// The bytes after the kind byte, always preserved.
    pub opaque: Vec<u8>,
    /// True when a single zero byte followed the frame, as the target's
    /// builders may append.
    pub zero_trailer: bool,
}

/// Parse a `JoinDataItemV2` message body, which starts at the `0x83` byte.
pub fn parse_join_data_item(body: &[u8]) -> Option<JoinDataItem> {
    if body.len() < 3 || body[0] != DELTA_CHAIN_TAG || body[1] != JOIN_DATA_SUBCODE {
        return None;
    }
    let kind = JoinDataKind::from_byte(body[2]);
    let rest = &body[3..];
    let mut item = JoinDataItem {
        kind,
        framing: JoinDataFraming::Opaque,
        compressed_size: 0,
        uncompressed_size: 0,
        inner: None,
        opaque: rest.to_vec(),
        zero_trailer: false,
    };
    if rest.len() < 12 {
        // Calling a short tail "truncated" is only honest for a kind that promises
        // a length pair; for the copied categories the bytes are the payload.
        if kind.takes_length_pair() {
            item.framing = JoinDataFraming::Truncated;
        }
        return Some(item);
    }
    for (framing, big_endian) in [
        (JoinDataFraming::LengthPairLittleEndian, false),
        (JoinDataFraming::LengthPairBigEndian, true),
    ] {
        let read = |bytes: &[u8]| -> u32 {
            if big_endian {
                u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            } else {
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            }
        };
        let compressed = read(&rest[0..4]);
        let uncompressed = read(&rest[4..8]);
        let Some(end) = 8usize.checked_add(compressed as usize) else {
            continue;
        };
        if end > rest.len() {
            continue;
        }
        let frame = &rest[8..end];
        if !starts_with_zstd_magic(frame) {
            continue;
        }
        let Ok(inner) = decompress_frame(frame, uncompressed, "JoinDataItemV2") else {
            continue;
        };
        item.framing = framing;
        item.compressed_size = compressed;
        item.uncompressed_size = uncompressed;
        item.inner = Some(inner);
        item.zero_trailer = rest.get(end) == Some(&0u8);
        return Some(item);
    }
    Some(item)
}

/// One shared-string dictionary entry: a 16-byte key, a marker, then the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedStringEntry {
    pub key: [u8; 16],
    pub marker: u8,
    pub value: Vec<u8>,
}

/// Parse the shared-string run (`[16B key][marker][varint len][bytes]*`).
pub fn parse_shared_strings(data: &[u8]) -> Result<Vec<SharedStringEntry>, String> {
    let mut reader = Reader::new(data);
    let mut out = Vec::new();
    while !reader.is_empty() {
        let key = reader.take(16, "shared string key")?;
        let marker = reader.read_u8("shared string marker")?;
        if marker > 1 {
            return Err(format!(
                "unknown shared-string marker {marker} at offset {}",
                reader.position() - 1
            ));
        }
        let value = reader.read_blob("shared string value")?;
        let mut key_bytes = [0u8; 16];
        key_bytes.copy_from_slice(key);
        out.push(SharedStringEntry { key: key_bytes, marker, value });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Network schema
// ---------------------------------------------------------------------------

/// One declared property of a class, as the snapshot dictionary lists it. The
/// name is the *wire* name (lower case in captures), `tag` selects the value
/// reader, and `extra` carries the two shapes that need more: a referenced
/// class for tag 60 and an enum table index for tag 7.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaMember {
    pub name: String,
    pub tag: u8,
    pub extra: Option<u64>,
}

/// One replicable event descriptor: a name plus its three-byte parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaEvent {
    pub name: String,
    pub parameters: Vec<[u8; 3]>,
}

/// One class in the snapshot dictionary. Its index is its network class id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaClass {
    pub name: String,
    pub members: Vec<SchemaMember>,
    pub flags: u16,
    pub events: Vec<SchemaEvent>,
}

/// The snapshot schema dictionary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinSchema {
    pub version: u8,
    pub flags: u64,
    pub enums: Vec<String>,
    pub property_bound: u64,
    pub event_bound: u64,
    pub classes: Vec<SchemaClass>,
    pub url_prefixes: Vec<String>,
    pub shared_names: Vec<String>,
    /// Where the per-class tables this parser does not model begin.
    pub tail_offset: usize,
}

impl JoinSchema {
    pub fn class(&self, index: u16) -> Option<&SchemaClass> {
        self.classes.get(usize::from(index))
    }

    pub fn total_members(&self) -> usize {
        self.classes.iter().map(|class| class.members.len()).sum()
    }

    pub fn total_events(&self) -> usize {
        self.classes.iter().map(|class| class.events.len()).sum()
    }

    /// The member a record's property delta refers to. The delta is an absolute
    /// index into the class's member list, not a difference from the previous
    /// property.
    pub fn member(&self, class: u16, delta: u8) -> Option<&SchemaMember> {
        self.class(class)?.members.get(usize::from(delta))
    }

    pub fn member_index(&self, class: u16, name: &str) -> Option<usize> {
        let class = self.class(class)?;
        class
            .members
            .iter()
            .position(|member| member.name.eq_ignore_ascii_case(name))
    }

    /// Resolve a global event index to the class that owns it and the index
    /// within that class, the way the `0x07` item's handler does.
    pub fn event(&self, global: u16) -> Option<(usize, usize, &SchemaEvent)> {
        let mut offset = 0usize;
        let target = usize::from(global);
        for (index, class) in self.classes.iter().enumerate() {
            let count = class.events.len();
            if target >= offset && target < offset + count {
                return Some((index, target - offset, &class.events[target - offset]));
            }
            offset += count;
        }
        None
    }

    /// The distinct property names in the dictionary, in first-appearance order.
    fn distinct_members(&self) -> BTreeMap<String, u16> {
        let mut map = BTreeMap::new();
        let mut next = 0u16;
        for class in &self.classes {
            for member in &class.members {
                let key = member.name.to_ascii_lowercase();
                if !map.contains_key(&key) {
                    map.insert(key, next);
                    next = next.saturating_add(1);
                }
            }
        }
        map
    }

    /// The candidate global property-id table an outbound `0x03` item needs.
    ///
    /// A property update on the wire carries an id into the replicator's runtime
    /// table, not a class's member index, and that table is not sent in the
    /// snapshot. The one quantity of it that *is* on the wire is the schema's
    /// property bound, so this builds the candidate table — every distinct
    /// member name in dictionary order — and only returns it when the count
    /// agrees with that bound. A schema whose dictionary does not line up gets
    /// `None`, which keeps the outbound path from addressing properties with
    /// invented ids.
    ///
    /// This is a hypothesis, stated as one: if the bound matches but the server
    /// still rejects the ids, the assignment order and the id base are the two
    /// fields to revisit.
    pub fn global_prop_ids(&self) -> Option<BTreeMap<String, u16>> {
        let map = self.distinct_members();
        let bound = usize::try_from(self.property_bound).unwrap_or(usize::MAX);
        // The target's table is documented as `prop_bound + 1` live entries, so
        // either the bound or the bound plus one corroborates the mapping.
        if map.len() == bound || map.len() + 1 == bound {
            Some(map)
        } else {
            None
        }
    }

    /// What the property bound and the dictionary say about that hypothesis, for
    /// the session report.
    pub fn prop_id_report(&self) -> String {
        let distinct = self.distinct_members().len();
        let outcome = if self.global_prop_ids().is_some() {
            "candidate global id table accepted"
        } else {
            "no global id table: outbound property updates cannot be addressed"
        };
        format!("distinct member names {distinct} vs property bound {} -> {outcome}", self.property_bound)
    }

    pub fn summary(&self) -> String {
        format!(
            "schema v{} flags 0x{:x}: {} class(es), {} member(s), {} event(s), {} enum(s), {} shared name(s), {} URL prefix(es), bounds {}/{}",
            self.version,
            self.flags,
            self.classes.len(),
            self.total_members(),
            self.total_events(),
            self.enums.len(),
            self.shared_names.len(),
            self.url_prefixes.len(),
            self.property_bound,
            self.event_bound
        )
    }
}

/// Parse the schema dictionary prefix. Whatever follows it (the target's
/// per-class and per-id tables) is reported through `tail_offset`, not parsed.
pub fn parse_join_schema(data: &[u8]) -> Result<JoinSchema, String> {
    let mut reader = Reader::new(data);
    let version = reader.read_varint("schema version")?;
    if version > 255 {
        return Err(format!("schema version {version} does not fit a byte"));
    }
    let flags = reader.read_u64_le("schema flags")?;

    let enum_count = reader.read_bounded_count("enum count")?;
    let mut enums = Vec::with_capacity(enum_count.min(4096));
    for _ in 0..enum_count {
        let name = reader.read_text("enum name")?;
        let trailer = reader.read_u8("enum trailer")?;
        if trailer != 0x07 {
            return Err(format!(
                "enum {name:?} trailer is 0x{trailer:02x}, not the expected 0x07"
            ));
        }
        enums.push(name);
    }

    let class_count = reader.read_bounded_count("class count")?;
    let property_bound = reader.read_varint("property bound")?;
    let event_bound = reader.read_varint("event bound")?;
    let mut classes = Vec::with_capacity(class_count.min(1024));
    for _ in 0..class_count {
        let name = reader.read_text("class name")?;
        let member_count = reader.read_bounded_count("member count")?;
        let mut members = Vec::with_capacity(member_count.min(256));
        for _ in 0..member_count {
            let member_name = reader.read_text("member name")?;
            let tag = reader.read_u8("member tag")?;
            let extra = match tag {
                60 => Some(reader.read_varint("referenced class")?),
                7 => Some(u64::from(reader.read_u16_le("enum index")?)),
                _ => None,
            };
            members.push(SchemaMember { name: member_name, tag, extra });
        }
        let flags = reader.read_u16_le("class flags")?;
        let event_count = reader.read_bounded_count("event count")?;
        let mut events = Vec::with_capacity(event_count.min(64));
        for _ in 0..event_count {
            let event_name = reader.read_text("event name")?;
            let parameter_count = usize::from(reader.read_u8("event parameter count")?);
            let mut parameters = Vec::with_capacity(parameter_count);
            for _ in 0..parameter_count {
                let bytes = reader.take(3, "event parameter")?;
                parameters.push([bytes[0], bytes[1], bytes[2]]);
            }
            events.push(SchemaEvent { name: event_name, parameters });
        }
        classes.push(SchemaClass { name, members, flags, events });
    }

    let url_count = reader.read_bounded_count("URL table count")?;
    let mut url_prefixes = Vec::with_capacity(url_count.min(64));
    for _ in 0..url_count {
        url_prefixes.push(reader.read_text("URL prefix")?);
    }
    let shared_count = reader.read_bounded_count("shared name count")?;
    let mut shared_names = Vec::with_capacity(shared_count.min(256));
    for _ in 0..shared_count {
        shared_names.push(reader.read_text("shared name")?);
    }

    Ok(JoinSchema {
        version: version as u8,
        flags,
        enums,
        property_bound,
        event_bound,
        classes,
        url_prefixes,
        shared_names,
        tail_offset: reader.position(),
    })
}

// ---------------------------------------------------------------------------
// Tagged values
// ---------------------------------------------------------------------------

/// Skip one tagged value, advancing the reader past its exact extent.
///
/// `prefix_mode` selects the prefix-compressed reader that the event and first
/// spawn loops use for the asset and content tags; the snapshot record loop and
/// the second spawn loop use the plain one. The depth and tuple rules mirror the
/// guards the reference parsers document.
/// Measure one tagged value's extent, without the record loop around it.
///
/// Public because the outbound writer needs to prove it produces exactly the
/// width the inbound reader consumes: a writer that is one byte off desyncs the
/// peer's parser for the rest of the chain, and that is not visible in a value
/// comparison.
pub fn value_extent(reader: &mut Reader<'_>, tag: u8) -> Result<(), String> {
    skip_tagged_value(reader, tag, false, "value", 0)
}

fn skip_tagged_value(
    reader: &mut Reader<'_>,
    tag: u8,
    prefix_mode: bool,
    context: &str,
    depth: usize,
) -> Result<(), String> {
    match tag {
        // Zero-width.
        3 => Ok(()),
        // String-shaped: [varint len][bytes].
        2 | 8 => {
            reader.read_blob(context)?;
            Ok(())
        }
        // Fixed-width blobs.
        6 | 46 | 47 => reader.take(16, context).map(|_| ()),
        11 | 16 | 17 => reader.take(4, context).map(|_| ()),
        12 | 48 | 55 | 62 | 13 | 21 | 37 => reader.take(8, context).map(|_| ()),
        14 | 40 | 67 | 52 => reader.take(16, context).map(|_| ()),
        15 => reader.take(24, context).map(|_| ()),
        18 => reader.take(2, context).map(|_| ()),
        19 | 22 | 36 => reader.take(12, context).map(|_| ()),
        20 | 66 => reader.take(3, context).map(|_| ()),
        24 => reader.take(4, context).map(|_| ()),
        25 => reader.take(6, context).map(|_| ()),
        9 => reader.take(1, context).map(|_| ()),
        39 => reader.take(20, context).map(|_| ()),
        65 => reader.take(13, context).map(|_| ()),
        // Integer-shaped.
        7 | 10 | 34 | 44 => reader.read_varint(context).map(|_| ()),
        68 => {
            for _ in 0..4 {
                reader.read_varint(context)?;
            }
            Ok(())
        }
        // CFrame-A (position plus a full matrix) and CFrame-B (position plus a
        // quantized rotation). The rotation tables are not recovered, so both
        // are only measured here; the applier uses the position and says so.
        26 => {
            reader.take(12, context)?;
            if reader.read_u8("cframe kind")? == 0 {
                reader.take(36, "cframe matrix")?;
            }
            Ok(())
        }
        27 => {
            reader.take(12, context)?;
            if reader.read_u8("cframe kind")? == 0 {
                reader.take(6, "cframe quaternion")?;
            }
            Ok(())
        }
        51 => {
            let kind = reader.read_u8("pivot kind")?;
            if kind & 0x80 != 0 {
                if kind & 0x7F == 0 {
                    reader.take(6, "pivot quaternion")?;
                }
                reader.take(12, "pivot position")?;
            }
            Ok(())
        }
        // References: [varint lo]([u32 id] unless null).
        28 | 69 => {
            if reader.read_varint("reference")? != 0 {
                reader.take(4, "reference id")?;
            }
            Ok(())
        }
        49 => skip_name(reader, context),
        61 => skip_content(reader, prefix_mode, context),
        // Sequences.
        35 => skip_keypoints(reader, 12, 0x14, context),
        38 => skip_keypoints(reader, 20, 0x14, context),
        // PhysicalProperties: flags, then floats when advertised.
        41 => {
            let flags = reader.read_u8("physical flags")?;
            if flags & 1 != 0 {
                reader.take(20, "physical floats")?;
                if flags >= 2 {
                    reader.take(4, "physical extra")?;
                }
            }
            Ok(())
        }
        // FontFace: weight, a style blob, then two length-prefixed strings.
        54 => {
            reader.take(2, "font weight")?;
            let style_len = usize::from(reader.read_u8("font style length")?);
            reader.take(style_len, "font style")?;
            for _ in 0..2 {
                let len = reader.read_bounded_len("font string length")?;
                reader.take(len, "font string")?;
            }
            Ok(())
        }
        // Compressed bulk blob: [varint compressed][varint uncompressed][bytes].
        56 => {
            let compressed = reader.read_varint("bulk compressed length")?;
            let compressed = usize::try_from(compressed)
                .map_err(|_| "bulk length exceeds this platform".to_string())?;
            reader.read_varint("bulk uncompressed length")?;
            reader.take(compressed, "bulk bytes")?;
            Ok(())
        }
        // MeshScale controller blob: a 7-byte header then a bounded count.
        58 => {
            reader.take(7, "mesh scale header")?;
            let count = reader.read_u8("mesh scale count")?;
            if count >= 51 {
                return Err(format!(
                    "tag 58 ({context}) declares {count} control values at offset {}",
                    reader.position() - 1
                ));
            }
            reader.take(usize::from(count), "mesh scale values")?;
            Ok(())
        }
        // NetAsset id / LR handle.
        60 => {
            let schema = reader.read_varint("netasset schema")?;
            if schema > 0xFFFF {
                return Err(format!("netasset schema index {schema} is out of range"));
            }
            if schema != 0xFFFF {
                reader.take(8, "netasset id")?;
                reader.read_varint("netasset value")?;
            }
            Ok(())
        }
        // Snapshot asset id: [varint len][bytes], or the prefix-compressed form
        // in the loops that use it.
        33 => {
            if prefix_mode {
                skip_prefixed(reader, context)
            } else {
                reader.read_blob(context)?;
                Ok(())
            }
        }
        // Tuple family: self-describing elements.
        29 | 30 | 31 | 32 => skip_tuple(reader, tag, prefix_mode, context, depth),
        57 => Err(format!(
            "tag 57 ({context}) is a flag-variable frustum shape and is client-to-server only"
        )),
        _ => Err(format!(
            "unhandled value tag {tag} ({context}) at offset {}",
            reader.position()
        )),
    }
}

fn skip_prefixed(reader: &mut Reader<'_>, context: &str) -> Result<(), String> {
    let prefix = reader.read_u8("asset prefix")?;
    if prefix & 1 != 0 {
        reader.read_varint("asset id")?;
    } else {
        reader.read_blob(context)?;
    }
    Ok(())
}

fn skip_name(reader: &mut Reader<'_>, context: &str) -> Result<(), String> {
    let first = reader.read_u8("name length")?;
    if first & 0x80 != 0 {
        // Shared-table index, with 127 reserved for the extended form.
        if first & 0x7F == 127 {
            return Err(format!(
                "shared-name index 127 at offset {} ({context}) is the escape hatch, not a string",
                reader.position() - 1
            ));
        }
        return Ok(());
    }
    if first == 127 {
        reader.read_blob("extended name")?;
        return Ok(());
    }
    reader.take(usize::from(first), "name bytes")?;
    Ok(())
}

/// Recover the string a `49` value carries, given the schema's shared table.
pub fn name_text(schema: &JoinSchema, raw: &[u8]) -> Option<String> {
    let &first = raw.first()?;
    if first & 0x80 != 0 {
        return schema.shared_names.get(usize::from(first & 0x7F)).cloned();
    }
    if first == 127 {
        let mut reader = Reader::new(&raw[1..]);
        let bytes = reader.read_blob("name").ok()?;
        return Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    let len = usize::from(first);
    if raw.len() < 1 + len {
        return None;
    }
    Some(String::from_utf8_lossy(&raw[1..1 + len]).into_owned())
}

fn skip_content(reader: &mut Reader<'_>, prefix_mode: bool, context: &str) -> Result<(), String> {
    let form = reader.read_varint("content form")?;
    match form {
        0 => Ok(()),
        1 => {
            if prefix_mode {
                skip_prefixed(reader, context)
            } else {
                reader.read_blob("content string")?;
                Ok(())
            }
        }
        2 => {
            if reader.read_varint("content reference")? != 0 {
                reader.take(4, "content reference id")?;
            }
            Ok(())
        }
        other => Err(format!("content form {other} ({context}) is not implemented")),
    }
}

fn skip_keypoints(
    reader: &mut Reader<'_>,
    stride: usize,
    maximum: u32,
    context: &str,
) -> Result<(), String> {
    let count = reader.read_u32_le("sequence count")?;
    if count > maximum {
        return Err(format!(
            "sequence ({context}) declares {count} keypoint(s), above the {maximum} the target accepts"
        ));
    }
    // `count` was bounded by `maximum` above, so this cannot truncate on any
    // platform; the cast is on purpose rather than `usize::from`, which would
    // not compile on 32-bit targets where usize is not 64 bits.
    let bytes = (count as usize).checked_mul(stride)
        .ok_or_else(|| "sequence length overflows".to_string())?;
    reader.take(bytes, "sequence keypoints")?;
    Ok(())
}

fn skip_tuple(
    reader: &mut Reader<'_>,
    tag: u8,
    prefix_mode: bool,
    context: &str,
    depth: usize,
) -> Result<(), String> {
    if tag == 29 && depth > 0 {
        return Err(format!(
            "tag 29 ({context}) cannot be received as a member of another container"
        ));
    }
    if depth > 32 {
        return Err(format!("tag {tag} ({context}) is nested more than 32 deep"));
    }
    let count = reader.read_varint("tuple count")?;
    if count > reader.remaining() as u64 {
        return Err(format!(
            "tuple ({context}) declares {count} element(s) with only {} byte(s) left",
            reader.remaining()
        ));
    }
    for _ in 0..count {
        if tag == 31 || tag == 32 {
            reader.read_blob("tuple key")?;
        }
        let element_tag = reader.read_u8("tuple element tag")?;
        if element_tag == 0 {
            continue;
        }
        if element_tag == 7 {
            reader.take(2, "tuple element enum index")?;
        }
        skip_tagged_value(reader, element_tag, prefix_mode, context, depth + 1)?;
    }
    Ok(())
}

/// Read `[varint lo]([u32 id] unless null)` — the reference shape that records,
/// spawns and property updates share.
fn read_reference(reader: &mut Reader<'_>, what: &str) -> Result<(u64, Option<u32>), String> {
    let lo = reader.read_varint(what)?;
    if lo == 0 {
        return Ok((lo, None));
    }
    let id = reader.read_u32_le(&format!("{what} id"))?;
    Ok((lo, Some(id)))
}

/// The guid variant, where `0xFFFFFFFF` is null as well. This is how the item
/// handlers read entity and blob ids.
fn read_guid(reader: &mut Reader<'_>, what: &str) -> Result<(u64, Option<u32>), String> {
    let lo = reader.read_varint(what)?;
    if lo == 0 || lo == 0xFFFF_FFFF {
        return Ok((lo, None));
    }
    let id = reader.read_u32_le(&format!("{what} id"))?;
    Ok((lo, Some(id)))
}

// ---------------------------------------------------------------------------
// Snapshot records
// ---------------------------------------------------------------------------

/// One property of a snapshot record: the wire name, its tag and its exact
/// bytes. Typing happens at apply time, against the class's declared type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinProp {
    pub name: String,
    pub tag: u8,
    pub raw: Vec<u8>,
}

/// One snapshot record: an instance with its class, properties and parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinRecord {
    /// The reference selector that preceded the id; zero means "no id", which is
    /// how the implicit root is written.
    pub reference_lo: u64,
    pub id: Option<u32>,
    /// Index into [`JoinSchema::classes`].
    pub class: u16,
    pub properties: Vec<JoinProp>,
    pub parent_lo: u64,
    pub parent: Option<u32>,
    pub consumed: usize,
}

/// The `([u8 member index][value])* [0xFF]` property loop.
fn read_property_loop(
    reader: &mut Reader<'_>,
    schema: &JoinSchema,
    class: u16,
    prefix_mode: bool,
    out: &mut Vec<JoinProp>,
) -> Result<(), String> {
    let class_name = schema
        .class(class)
        .map(|defined| defined.name.clone())
        .unwrap_or_else(|| format!("class {class}"));
    loop {
        let delta = reader.read_u8("property delta")?;
        if delta == 0xFF {
            return Ok(());
        }
        // The extent starts after the delta byte, so a value can be re-read by
        // the applier without knowing the loop's framing.
        let start = reader.position();
        let Some(member) = schema.member(class, delta) else {
            return Err(format!(
                "property delta {delta} is out of range for {class_name} at offset {start}"
            ));
        };
        let name = member.name.clone();
        let tag = member.tag;
        skip_tagged_value(reader, tag, prefix_mode, &format!("{class_name}.{name}"), 0)?;
        let raw = reader.extent(start).to_vec();
        out.push(JoinProp { name, tag, raw });
    }
}

/// Parse one record at the front of `data`.
pub fn parse_join_record(data: &[u8], schema: &JoinSchema) -> Result<JoinRecord, String> {
    let mut reader = Reader::new(data);
    let (reference_lo, id) = read_reference(&mut reader, "record")?;
    let class = reader.read_u16_le("record class")?;
    if schema.class(class).is_none() {
        return Err(format!(
            "record class {class} is outside the {} class(es) the schema declares",
            schema.classes.len()
        ));
    }
    let mut properties = Vec::new();
    read_property_loop(&mut reader, schema, class, false, &mut properties)?;
    let (parent_lo, parent) = read_reference(&mut reader, "record parent")?;
    Ok(JoinRecord {
        reference_lo,
        id,
        class,
        properties,
        parent_lo,
        parent,
        consumed: reader.position(),
    })
}

/// Parse a chained record run of exactly `count` records, or until the buffer
/// ends when `count` is `usize::MAX`. Returns the records and how many bytes the
/// run consumed, so a caller with a counted run can keep the trailing bytes.
/// The `count` value that means "no declared count": read records until the
/// buffer is empty, and let the caller decide whether a leftover byte is an error.
pub const RECORD_RUN_TO_END: usize = usize::MAX;

pub fn parse_record_run(
    data: &[u8],
    schema: &JoinSchema,
    count: usize,
) -> Result<(Vec<JoinRecord>, usize), String> {
    let mut records = Vec::new();
    let mut offset = 0usize;
    let counted = count != RECORD_RUN_TO_END;
    while offset < data.len() && records.len() < count {
        let record = parse_join_record(&data[offset..], schema)
            .map_err(|error| format!("record {} at offset {offset}: {error}", records.len()))?;
        if record.consumed == 0 {
            return Err(format!("record {} consumed no bytes", records.len()));
        }
        offset += record.consumed;
        records.push(record);
    }
    // A counted run has to be satisfied; an uncounted one stops at the end of the
    // buffer and says nothing about a count it was never given.
    if counted && records.len() < count {
        return Err(format!(
            "the run holds {} record(s) but {count} were declared",
            records.len()
        ));
    }
    Ok((records, offset))
}

/// Parse a whole buffer as a record run. A batch inner is pure records, so a
/// leftover byte is a parse failure rather than padding.
pub fn parse_join_records(data: &[u8], schema: &JoinSchema) -> Result<Vec<JoinRecord>, String> {
    let (records, consumed) = parse_record_run(data, schema, RECORD_RUN_TO_END)?;
    if consumed != data.len() {
        return Err(format!(
            "{consumed} byte(s) of records left {} byte(s) over",
            data.len() - consumed
        ));
    }
    Ok(records)
}

// ---------------------------------------------------------------------------
// 0x83 delta chains
// ---------------------------------------------------------------------------

/// A live spawn item: one record plus the second property loop its handler reads
/// before the parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnRecord {
    pub reference_lo: u64,
    pub id: Option<u32>,
    pub class: u16,
    pub first_loop: Vec<JoinProp>,
    pub second_loop: Vec<JoinProp>,
    pub parent_lo: u64,
    pub parent: Option<u32>,
}

/// One `0x07` event invocation, resolved through the schema's event lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventInvocation {
    pub entity: Option<u32>,
    pub event: u16,
    pub class_index: usize,
    pub event_index: usize,
    pub class_name: String,
    pub event_name: String,
    /// The exact bytes the parameter loop consumed, kept for later mapping.
    pub parameter_bytes: Vec<u8>,
}

/// One `0x03` property update. The id it carries indexes the replicator's
/// runtime property table rather than a class's member list, so the value's tag
/// is only known when a caller supplies that table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyUpdate {
    pub entity: Option<u32>,
    pub property: u16,
    pub raw: Vec<u8>,
}

/// The counted record run inside the `0x0d` item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeltaEntries {
    pub count: u32,
    pub records: Vec<JoinRecord>,
    /// Bytes the inflated frame held after the run; they belong to the chain,
    /// not to the records.
    pub trailing: Vec<u8>,
}

/// One item of a `0x83` delta chain. Types whose grammar is not recovered stop
/// the walk and are reported as a stop point instead of being guessed at.
///
/// No `PartialEq`: a `Batch` item carries a decoded `JoinDataItem`, whose
/// decoded-inner shape is deliberately not comparable, and comparing chain items
/// is never what a caller wants anyway - the counters and stop points are.
#[derive(Clone, Debug)]
pub enum ChainItem {
    EntityRef { lo: u64, entity: Option<u32> },
    Spawn(SpawnRecord),
    Event(EventInvocation),
    Property(PropertyUpdate),
    Batch(JoinDataItem),
    Clock { value: u32 },
    Word { kind: u8, value: u32 },
    GuidPair { first: (u64, Option<u32>), second: (u64, Option<u32>) },
    GuidList(Vec<(u64, Option<u32>)>),
    Info { reference: (u64, Option<u32>), first: Vec<u8>, second: Vec<u8> },
    Blob { head: u32, bytes: Vec<u8> },
    SizedBlob { selector: u8, second: u8, dropped: bool, bytes: Vec<u8> },
    Zigzag { value: i64 },
    Floats { head: u32, bytes: Vec<u8> },
    Entries(DeltaEntries),
    RefList(Vec<((u64, Option<u32>), u8)>),
}

impl ChainItem {
    /// The chain item type byte this was decoded from, for the histogram.
    pub fn kind(&self) -> u8 {
        match self {
            Self::EntityRef { .. } => 0x01,
            Self::Spawn(_) => 0x02,
            Self::Property(_) => 0x03,
            Self::Word { kind, .. } => *kind,
            Self::Event(_) => 0x07,
            Self::Entries(_) => 0x0d,
            Self::Batch(_) => JOIN_DATA_SUBCODE,
            Self::GuidList(_) => 0x1b,
            Self::Info { .. } => 0x15,
            Self::SizedBlob { .. } => 0x1f,
            Self::Blob { .. } => 0x20,
            Self::Clock { .. } => 0x23,
            Self::Zigzag { .. } => 0x26,
            Self::Floats { .. } => 0x27,
            Self::RefList(_) => 0x29,
            Self::GuidPair { .. } => 0x13,
        }
    }
}

/// Where a chain walk stopped and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopPoint {
    pub item: u8,
    pub offset: usize,
    pub reason: String,
}

/// The result of walking one delta chain.
#[derive(Clone, Debug, Default)]
pub struct DeltaChain {
    pub items: Vec<ChainItem>,
    /// True when the walk reached the `00` marker or ran out of buffer.
    pub complete: bool,
    pub stopped_at: Option<StopPoint>,
    pub spawns: usize,
    pub events: usize,
    pub property_updates: usize,
    pub batches: usize,
    pub records: usize,
}

/// An item decoded from a chain, with the bytes it took and an optional reason
/// to stop walking after it.
struct ParsedItem {
    item: ChainItem,
    used: usize,
    stop: Option<String>,
}

/// Parse one `0x02` spawn at the front of `data`. `has_pad` selects the byte the
/// item's handler reads before the property loops when the flag it tests is set;
/// the reference could not pin down which flag that is in this frame, so the
/// caller decides.
pub fn parse_spawn_item(
    data: &[u8],
    schema: &JoinSchema,
    has_pad: bool,
) -> Result<(SpawnRecord, usize), String> {
    let mut reader = Reader::new(data);
    let (reference_lo, id) = read_reference(&mut reader, "spawn")?;
    let class = reader.read_u16_le("spawn class")?;
    if schema.class(class).is_none() {
        return Err(format!(
            "spawn class {class} is outside the schema's {} class(es)",
            schema.classes.len()
        ));
    }
    if has_pad {
        reader.read_u8("spawn pad")?;
    }
    let mut first_loop = Vec::new();
    read_property_loop(&mut reader, schema, class, true, &mut first_loop)?;
    let mut second_loop = Vec::new();
    read_property_loop(&mut reader, schema, class, false, &mut second_loop)?;
    let (parent_lo, parent) = read_reference(&mut reader, "spawn parent")?;
    Ok((
        SpawnRecord { reference_lo, id, class, first_loop, second_loop, parent_lo, parent },
        reader.position(),
    ))
}

/// Parse one `0x07` event invocation at the front of `data`. Its parameters are
/// driven by the schema's event list, so an out-of-range id is refused and the
/// payload bytes are kept rather than re-derived.
pub fn parse_event_item(data: &[u8], schema: &JoinSchema) -> Result<(EventInvocation, usize), String> {
    let mut reader = Reader::new(data);
    let (_, entity) = read_reference(&mut reader, "event entity")?;
    let event = reader.read_u16_le("event id")?;
    let Some((class_index, event_index, descriptor)) = schema.event(event) else {
        return Err(format!(
            "event id {event} is outside the {} event(s) the schema declares",
            schema.total_events()
        ));
    };
    let class_name = schema.classes[class_index].name.clone();
    let event_name = descriptor.name.clone();
    let parameters = descriptor.parameters.clone();
    let start = reader.position();
    for (index, parameter) in parameters.iter().enumerate() {
        let tag = parameter[0];
        let context = format!("{class_name}.{event_name} parameter {index}");
        if (29..=32).contains(&tag) {
            skip_tuple(&mut reader, tag, true, &context, 0)?;
        } else {
            skip_tagged_value(&mut reader, tag, true, &context, 0)?;
        }
    }
    let parameter_bytes = reader.extent(start).to_vec();
    Ok((
        EventInvocation {
            entity,
            event,
            class_index,
            event_index,
            class_name,
            event_name,
            parameter_bytes,
        },
        reader.position(),
    ))
}

/// Parse the item whose type byte is `body[index]`, returning it and the number
/// of bytes consumed after that type byte.
fn parse_chain_item(
    item: u8,
    body: &[u8],
    index: usize,
    schema: Option<&JoinSchema>,
) -> Result<ParsedItem, String> {
    let rest = &body[index..];
    let mut reader = Reader::new(rest);
    match item {
        0x23 => {
            let value = reader.read_u32_le("clock")?;
            Ok(ParsedItem { item: ChainItem::Clock { value }, used: reader.position(), stop: None })
        }
        0x04 | 0x10 | 0x24 | 0x28 => {
            let value = reader.read_u32_le("word item")?;
            Ok(ParsedItem {
                item: ChainItem::Word { kind: item, value },
                used: reader.position(),
                stop: None,
            })
        }
        0x01 => {
            let (lo, entity) = read_reference(&mut reader, "0x01 entity")?;
            Ok(ParsedItem {
                item: ChainItem::EntityRef { lo, entity },
                used: reader.position(),
                stop: None,
            })
        }
        0x1d | 0x1e => {
            let (lo, entity) = read_guid(&mut reader, "entity")?;
            Ok(ParsedItem {
                item: ChainItem::EntityRef { lo, entity },
                used: reader.position(),
                stop: None,
            })
        }
        0x13 => {
            let first = read_guid(&mut reader, "0x13 first")?;
            let second = read_guid(&mut reader, "0x13 second")?;
            Ok(ParsedItem {
                item: ChainItem::GuidPair { first, second },
                used: reader.position(),
                stop: None,
            })
        }
        0x26 => {
            let value = reader.read_zigzag("0x26 value")?;
            Ok(ParsedItem { item: ChainItem::Zigzag { value }, used: reader.position(), stop: None })
        }
        0x20 => {
            let head = reader.read_u32_le("0x20 length")?;
            let len = usize::try_from(head).map_err(|_| "0x20 length overflows".to_string())?;
            let bytes = reader.take(len, "0x20 blob")?.to_vec();
            Ok(ParsedItem { item: ChainItem::Blob { head, bytes }, used: reader.position(), stop: None })
        }
        0x27 => {
            let head = reader.read_u32_le("0x27 id")?;
            let bytes = reader.take(60, "0x27 floats")?.to_vec();
            Ok(ParsedItem { item: ChainItem::Floats { head, bytes }, used: reader.position(), stop: None })
        }
        0x1f => {
            let selector = reader.read_u8("0x1f selector")?;
            let second = reader.read_u8("0x1f second")?;
            // The target null-drops the item for selector 1 or 2 without
            // consuming the blob, and rejects an over-capacity blob; both keep
            // the chain alive.
            if selector == 1 || selector == 2 {
                return Ok(ParsedItem {
                    item: ChainItem::SizedBlob { selector, second, dropped: true, bytes: Vec::new() },
                    used: reader.position(),
                    stop: None,
                });
            }
            let length = reader.read_u32_le("0x1f length")?;
            if length > 0x40_0000 {
                return Err(format!(
                    "0x1f blob of {length} byte(s) is over the target's capacity check"
                ));
            }
            let bytes = reader.take(length as usize, "0x1f blob")?.to_vec();
            Ok(ParsedItem {
                item: ChainItem::SizedBlob { selector, second, dropped: false, bytes },
                used: reader.position(),
                stop: None,
            })
        }
        0x15 => {
            let reference = read_guid(&mut reader, "0x15 ref")?;
            let first = reader.read_blob("0x15 first string")?;
            let second = reader.read_blob("0x15 second string")?;
            Ok(ParsedItem {
                item: ChainItem::Info { reference, first, second },
                used: reader.position(),
                stop: None,
            })
        }
        0x1b => {
            let count = reader.read_u32_le("0x1b count")?;
            let count = usize::try_from(count).map_err(|_| "0x1b count overflows".to_string())?;
            if count > reader.remaining() {
                return Err(format!(
                    "0x1b declares {count} reference(s) with only {} byte(s) left",
                    reader.remaining()
                ));
            }
            let mut refs = Vec::with_capacity(count);
            for _ in 0..count {
                refs.push(read_reference(&mut reader, "0x1b entry")?);
            }
            Ok(ParsedItem { item: ChainItem::GuidList(refs), used: reader.position(), stop: None })
        }
        0x29 => {
            let count = reader.read_u32_le("0x29 count")?;
            let count = usize::try_from(count).map_err(|_| "0x29 count overflows".to_string())?;
            if count > reader.remaining() {
                return Err(format!(
                    "0x29 declares {count} entries with only {} byte(s) left",
                    reader.remaining()
                ));
            }
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                let reference = read_guid(&mut reader, "0x29 entry")?;
                let flag = reader.read_u8("0x29 flag")?;
                entries.push((reference, flag));
            }
            Ok(ParsedItem { item: ChainItem::RefList(entries), used: reader.position(), stop: None })
        }
        0x02 => {
            let Some(schema) = schema else {
                return Err("a spawn needs the network schema, which has not arrived".to_string());
            };
            let (spawn, used) = parse_spawn_item(&body[index..], schema, false)?;
            Ok(ParsedItem { item: ChainItem::Spawn(spawn), used, stop: None })
        }
        0x07 => {
            let Some(schema) = schema else {
                return Err("an event item needs the network schema, which has not arrived".to_string());
            };
            let (event, used) = parse_event_item(&body[index..], schema)?;
            Ok(ParsedItem { item: ChainItem::Event(event), used, stop: None })
        }
        0x0d => {
            let Some(schema) = schema else {
                return Err("a counted record run needs the network schema".to_string());
            };
            let count = reader.read_u32_le("0x0d count")?;
            let mut entries = DeltaEntries::default();
            entries.count = count;
            if count != 0 {
                let declared = usize::try_from(count)
                    .map_err(|_| "0x0d count overflows".to_string())?;
                let compressed = reader.read_u32_le("0x0d compressed size")?;
                let uncompressed = reader.read_u32_le("0x0d uncompressed size")?;
                let len = usize::try_from(compressed)
                    .map_err(|_| "0x0d length overflows".to_string())?;
                let frame = reader.take(len, "0x0d frame")?.to_vec();
                if !starts_with_zstd_magic(&frame) {
                    return Err("0x0d frame has no Zstandard magic".to_string());
                }
                let inner = decompress_frame(&frame, uncompressed, "0x0d entries")?;
                let (records, used) = parse_record_run(&inner, schema, declared)?;
                entries.trailing = inner[used..].to_vec();
                entries.records = records;
            }
            // Element run: [u8 flags][payload?]* while bit 3 is set.
            loop {
                let flags = reader.read_u8("0x0d element flags")?;
                if flags & 0xF7 == 0 {
                    let value_flag = reader.read_u8("0x0d element value")?;
                    reader.take(if value_flag != 0 { 12 } else { 3 }, "0x0d element")?;
                }
                if flags & 8 == 0 {
                    break;
                }
            }
            Ok(ParsedItem { item: ChainItem::Entries(entries), used: reader.position(), stop: None })
        }
        0x03 => {
            let (_, entity) = read_reference(&mut reader, "0x03 entity")?;
            let property = reader.read_u16_le("0x03 property id")?;
            Ok(ParsedItem {
                item: ChainItem::Property(PropertyUpdate {
                    entity,
                    property,
                    raw: rest[reader.position()..].to_vec(),
                }),
                used: reader.position(),
                // The value is typed by the replicator's runtime property table,
                // which is not on the wire, so the walk cannot continue past it.
                stop: Some(
                    "0x03 property values are typed by the replicator's runtime \
                     property table, which is not on the wire; the update is \
                     recorded and the walk stops here"
                        .to_string(),
                ),
            })
        }
        0x16 => Err("a JoinDataItemV2 batch closes its message and never chains".to_string()),
        _ => Err(format!("item 0x{item:02x} has no parser here")),
    }
}

/// Walk a `0x83` message body as a delta chain. `schema` is required for the
/// record-carrying items; without it the walk stops at the first one instead of
/// inventing a class table.
pub fn walk_delta_chain(body: &[u8], schema: Option<&JoinSchema>) -> Result<DeltaChain, String> {
    let Some(&tag) = body.first() else {
        return Err("empty delta chain".to_string());
    };
    if tag != DELTA_CHAIN_TAG {
        return Err(format!(
            "expected delta chain tag 0x{DELTA_CHAIN_TAG:02x}, found 0x{tag:02x}"
        ));
    }
    let mut chain = DeltaChain::default();
    if body.len() < 2 {
        chain.complete = true;
        return Ok(chain);
    }
    // A batch is its own message: read it as one and stop.
    if body[1] == JOIN_DATA_SUBCODE {
        let Some(item) = parse_join_data_item(body) else {
            return Err("JoinDataItemV2 batch did not parse".to_string());
        };
        chain.batches += 1;
        chain.items.push(ChainItem::Batch(item));
        chain.complete = true;
        return Ok(chain);
    }

    let mut index = 1usize;
    while index < body.len() {
        let item = body[index];
        let type_offset = index;
        index += 1;
        if item == CHAIN_END {
            chain.complete = true;
            break;
        }
        match parse_chain_item(item, body, index, schema) {
            Ok(parsed) => {
                match &parsed.item {
                    ChainItem::Spawn(_) => chain.spawns += 1,
                    ChainItem::Event(_) => chain.events += 1,
                    ChainItem::Property(_) => chain.property_updates += 1,
                    ChainItem::Entries(entries) => chain.records += entries.records.len(),
                    _ => {}
                }
                if parsed.used == 0 {
                    return Err(format!(
                        "item 0x{item:02x} at offset {type_offset} consumed no bytes"
                    ));
                }
                chain.items.push(parsed.item);
                index += parsed.used;
                if let Some(reason) = parsed.stop {
                    chain.stopped_at = Some(StopPoint { item, offset: type_offset, reason });
                    return Ok(chain);
                }
            }
            Err(reason) => {
                chain.stopped_at = Some(StopPoint { item, offset: type_offset, reason });
                return Ok(chain);
            }
        }
    }
    if index >= body.len() {
        chain.complete = true;
    }
    Ok(chain)
}

// ---------------------------------------------------------------------------
// Applying a snapshot to a DataModel
// ---------------------------------------------------------------------------

/// What the applier did and, more importantly, what it refused to guess.
#[derive(Clone, Debug, Default)]
pub struct ApplyStats {
    pub spawned: usize,
    pub skipped_unknown_class: usize,
    pub properties_set: usize,
    pub properties_undeclared: usize,
    pub properties_undecoded: usize,
    pub cframes_position_only: usize,
    pub parents_grafted: usize,
    pub names_set: usize,
    /// `what` -> count, for the audit surface in the report.
    pub skip_reasons: Vec<(String, usize)>,
}

impl ApplyStats {
    fn note_skip(&mut self, key: String) {
        if let Some((_, count)) = self.skip_reasons.iter_mut().find(|(name, _)| *name == key) {
            *count += 1;
            return;
        }
        self.skip_reasons.push((key, 1));
    }

    pub fn summary(&self) -> String {
        let mut reasons: Vec<(String, usize)> = self.skip_reasons.clone();
        reasons.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let top: Vec<String> = reasons
            .into_iter()
            .take(8)
            .map(|(name, count)| format!("{name} x{count}"))
            .collect();
        format!(
            "{} instance(s) spawned, {} property value(s) applied ({} name(s), {} CFrame(s) position only), {} undeclared, {} undecoded, {} parent(s) grafted to the root, {} record(s) skipped for an unknown class{}",
            self.spawned,
            self.properties_set,
            self.names_set,
            self.cframes_position_only,
            self.properties_undeclared,
            self.properties_undecoded,
            self.parents_grafted,
            self.skipped_unknown_class,
            if top.is_empty() {
                String::new()
            } else {
                format!("; most-skipped [{}]", top.join(", "))
            }
        )
    }
}

/// An applied snapshot: the place itself, plus the accounting and the id map a
/// later delta needs to find the instance it updates.
pub struct SnapshotApply {
    pub dom: WeakDom,
    pub stats: ApplyStats,
    /// Wire instance id -> referent.
    pub ids: BTreeMap<u32, rbx_dom_weak::types::Ref>,
}

/// One instance exactly as the snapshot left it.
///
/// This is what an outbound diff compares against. A `WeakDom` cannot be
/// duplicated cheaply, and the editor mutates its own tree in place, so the
/// applied state is captured here instead: it is plain data, it crosses the
/// worker boundary, and it keys properties lower-case so a wire name and a
/// canonical editor name find the same entry.
///
/// `PartialEq` only: the values are `Variant`s, which carry floats and so are
/// not `Eq`, and a diff that compares a float property has to accept that.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BaselineInstance {
    pub class: String,
    pub name: String,
    /// Lower-cased property name -> value.
    pub properties: BTreeMap<String, Variant>,
    /// The wire id of the parent, when the snapshot gave it one.
    pub parent: Option<u32>,
}

impl BaselineInstance {
    pub fn property(&self, name: &str) -> Option<&Variant> {
        self.properties.get(&name.to_ascii_lowercase())
    }
}

/// The applied state of a whole snapshot, keyed by wire instance id.
#[derive(Clone, Debug, Default)]
pub struct Baseline {
    pub entries: BTreeMap<u32, BaselineInstance>,
}

impl Baseline {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl SnapshotApply {
    /// Capture the applied state for an outbound diff.
    pub fn baseline(&self) -> Baseline {
        // `HashMap`, not `BTreeMap`: `Ref` is `Hash + Eq` but deliberately has no
        // ordering, and this map is only ever probed by referent.
        let mut by_ref: std::collections::HashMap<rbx_dom_weak::types::Ref, u32> =
            std::collections::HashMap::new();
        for (id, reference) in &self.ids {
            by_ref.insert(*reference, *id);
        }
        let mut entries = BTreeMap::new();
        for (id, reference) in &self.ids {
            let Some(instance) = self.dom.get_by_ref(*reference) else {
                continue;
            };
            let mut properties = BTreeMap::new();
            for (key, value) in &instance.properties {
                properties.insert(key.to_ascii_lowercase(), value.clone());
            }
            // A parent the snapshot did not create (the DataModel root, or a
            // service that was never replicated) reads as no parent, which is
            // what `apply_join_snapshot` grafted it to.
            let parent = by_ref.get(&instance.parent()).copied();
            entries.insert(
                *id,
                BaselineInstance {
                    class: instance.class.to_string(),
                    name: instance.name.clone(),
                    properties,
                    parent,
                },
            );
        }
        Baseline { entries }
    }

    /// Serialize the snapshot as an XML place, which the editor can open
    /// directly and which needs no asset round trip.
    pub fn to_rbxlx(&self) -> Result<Vec<u8>, String> {
        let roots: Vec<rbx_dom_weak::types::Ref> = self.dom.root().children().to_vec();
        let mut buffer = Vec::new();
        rbx_xml::to_writer_default(std::io::Cursor::new(&mut buffer), &self.dom, &roots)
            .map_err(|error| format!("serializing the replicated place: {error}"))?;
        Ok(buffer)
    }
}

/// Build a place from a decoded snapshot.
pub fn apply_join_snapshot(
    schema: &JoinSchema,
    records: &[JoinRecord],
) -> Result<SnapshotApply, String> {
    let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
    let mut ids: BTreeMap<u32, rbx_dom_weak::types::Ref> = BTreeMap::new();
    let mut stats = ApplyStats::default();
    // Reparenting is a second pass, because a child can precede its parent.
    let mut parents: Vec<(u32, u32)> = Vec::new();
    let root = dom.root_ref();

    for record in records {
        let Some(id) = record.id else {
            // A sentinel record carries no id; the target treats it as the root.
            continue;
        };
        let Some(class) = schema.class(record.class) else {
            stats.skipped_unknown_class += 1;
            continue;
        };
        let class_name = class.name.clone();
        if !crate::schema::class_exists(&class_name) {
            stats.skipped_unknown_class += 1;
            stats.note_skip(format!("{class_name} (not in the reflection database)"));
            continue;
        }
        let mut builder = InstanceBuilder::new(class_name.as_str());
        for property in &record.properties {
            if property.name.eq_ignore_ascii_case("name") {
                if let Some(text) = name_text(schema, &property.raw) {
                    builder = builder.with_name(text);
                    stats.names_set += 1;
                    continue;
                }
            }
            let Some(canonical) =
                crate::schema::canonical_property_name(&class_name, &property.name)
            else {
                stats.properties_undeclared += 1;
                stats.note_skip(format!("{class_name}.{} (wire name)", property.name));
                continue;
            };
            match decode_property(&class_name, &canonical, property.tag, &property.raw, schema, &mut stats) {
                Some(value) => {
                    builder = builder.with_property(canonical.as_str(), value);
                    stats.properties_set += 1;
                }
                None => stats.properties_undecoded += 1,
            }
        }
        let created = dom.insert(root, builder);
        ids.insert(id, created);
        stats.spawned += 1;
        if let Some(parent) = record.parent {
            parents.push((id, parent));
        } else if record.parent_lo == 0 {
            stats.parents_grafted += 1;
        }
    }

    for (id, parent) in parents {
        let Some(child) = ids.get(&id).copied() else { continue };
        if parent == DATAMODEL_ROOT_ID {
            // Every spawn starts as a child of the DataModel root, which is
            // exactly where the target's root id points.
            continue;
        }
        match ids.get(&parent).copied() {
            Some(target) if target != child => {
                dom.transfer_within(child, target);
            }
            _ => stats.parents_grafted += 1,
        }
    }

    Ok(SnapshotApply { dom, stats, ids })
}

/// Materialize one property value against the class's declared type. The raw
/// extent came from [`skip_tagged_value`], so the length checks here are the
/// second half of the same contract: a size that disagrees with the declared
/// type is reported, not forced into it. Every fixed-size value type therefore
/// demands its exact extent (the Color3uint8 prefix and the quantized CFrame
/// forms being the documented exceptions), and the skip note carries the tag and
/// the length so a report can show what was refused.
fn decode_property(
    class: &str,
    property: &str,
    tag: u8,
    raw: &[u8],
    schema: &JoinSchema,
    stats: &mut ApplyStats,
) -> Option<Variant> {
    let data_type = crate::schema::resolve_property_type(class, property)?;
    let unsupported = |stats: &mut ApplyStats| {
        stats.note_skip(format!(
            "{class}.{property} (tag {tag}, {} byte(s))",
            raw.len()
        ));
        None
    };
    let value = match data_type {
        DataType::Enum(_) => Variant::Enum(Enum::from_u32(varint_value(raw)? as u32)),
        DataType::Value(VariantType::Bool) => Variant::Bool(raw.first().copied().unwrap_or(0) != 0),
        DataType::Value(VariantType::String) => {
            let text = match tag {
                49 => name_text(schema, raw)?,
                2 | 8 | 33 | 61 => {
                    let mut reader = Reader::new(raw);
                    if tag == 61 {
                        // A Content value in its string form: form byte then the
                        // length-prefixed string.
                        let form = reader.read_varint("content form").ok()?;
                        if form != 1 {
                            return unsupported(stats);
                        }
                    }
                    let bytes = reader.read_blob("string value").ok()?;
                    String::from_utf8_lossy(&bytes).into_owned()
                }
                _ => return unsupported(stats),
            };
            Variant::String(text)
        }
        DataType::Value(VariantType::Float32) if raw.len() == 4 => Variant::Float32(le_f32(raw, 0)?),
        DataType::Value(VariantType::Float64) if raw.len() == 8 => Variant::Float64(le_f64(raw, 0)?),
        DataType::Value(VariantType::Int32) => Variant::Int32(varint_value(raw)? as i32),
        DataType::Value(VariantType::Int64) => {
            if raw.len() == 8 {
                Variant::Int64(le_i64(raw, 0)?)
            } else {
                Variant::Int64(varint_value(raw)? as i64)
            }
        }
        DataType::Value(VariantType::Vector3) if raw.len() == 12 => Variant::Vector3(Vector3::new(
            le_f32(raw, 0)?,
            le_f32(raw, 4)?,
            le_f32(raw, 8)?,
        )),
        DataType::Value(VariantType::Vector2) if raw.len() == 8 => {
            Variant::Vector2(Vector2::new(le_f32(raw, 0)?, le_f32(raw, 4)?))
        }
        DataType::Value(VariantType::Color3) if raw.len() == 12 => Variant::Color3(Color3::new(
            le_f32(raw, 0)?.clamp(0.0, 1.0),
            le_f32(raw, 4)?.clamp(0.0, 1.0),
            le_f32(raw, 8)?.clamp(0.0, 1.0),
        )),
        // The exception to exact extents: a Color3uint8 can arrive behind a leading
        // form byte, so three bytes or more are read as channels and only a shorter
        // run is refused.
        DataType::Value(VariantType::Color3uint8) if raw.len() >= 3 => {
            Variant::Color3uint8(Color3uint8::new(raw[0], raw[1], raw[2]))
        }
        DataType::Value(VariantType::UDim) if raw.len() == 8 => {
            Variant::UDim(UDim::new(le_f32(raw, 0)?, le_i32(raw, 4)?))
        }
        DataType::Value(VariantType::UDim2) if raw.len() == 16 => Variant::UDim2(UDim2::new(
            UDim::new(le_f32(raw, 0)?, le_i32(raw, 4)?),
            UDim::new(le_f32(raw, 8)?, le_i32(raw, 12)?),
        )),
        DataType::Value(VariantType::NumberRange) if raw.len() == 8 => {
            Variant::NumberRange(NumberRange::new(le_f32(raw, 0)?, le_f32(raw, 4)?))
        }
        DataType::Value(VariantType::Rect) if raw.len() == 16 => Variant::Rect(Rect::new(
            Vector2::new(le_f32(raw, 0)?, le_f32(raw, 4)?),
            Vector2::new(le_f32(raw, 8)?, le_f32(raw, 12)?),
        )),
        // The quantized-rotation tables are not recovered, so CFrame-derived
        // values keep their position and an identity orientation, and the report
        // counts how many were affected.
        DataType::Value(VariantType::CFrame) if matches!(tag, 26 | 27 | 51) => {
            let identity = CFrame::identity();
            let position = if tag == 51 {
                let kind = *raw.first()?;
                if kind & 0x80 == 0 {
                    identity.position
                } else if raw.len() >= 19 {
                    Vector3::new(le_f32(raw, 7)?, le_f32(raw, 11)?, le_f32(raw, 15)?)
                } else if raw.len() >= 13 {
                    Vector3::new(le_f32(raw, 1)?, le_f32(raw, 5)?, le_f32(raw, 9)?)
                } else {
                    return unsupported(stats);
                }
            } else {
                Vector3::new(le_f32(raw, 0)?, le_f32(raw, 4)?, le_f32(raw, 8)?)
            };
            stats.cframes_position_only += 1;
            Variant::CFrame(CFrame::new(position, identity.orientation))
        }
        other => {
            stats.note_skip(format!("{class}.{property} ({other:?} has no snapshot decoder here)"));
            return None;
        }
    };
    Some(value)
}

fn le_f32(bytes: &[u8], offset: usize) -> Option<f32> {
    Some(f32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn le_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn le_f64(bytes: &[u8], offset: usize) -> Option<f64> {
    let mut buffer = [0u8; 8];
    buffer.copy_from_slice(bytes.get(offset..offset + 8)?);
    Some(f64::from_le_bytes(buffer))
}

fn le_i64(bytes: &[u8], offset: usize) -> Option<i64> {
    Some(i64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

/// Decode the leading LEB128 of a raw value extent.
fn varint_value(raw: &[u8]) -> Option<u64> {
    let mut reader = Reader::new(raw);
    reader.read_varint("value").ok()
}

// ---------------------------------------------------------------------------
// The resumable per-channel state machine
// ---------------------------------------------------------------------------

/// Something the stream wants the session log to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplicationNote {
    PeerInit(String),
    Schema(String),
    Batch(String),
    Applied(String),
    Deltas(String),
    Warning(String),
}

impl ReplicationNote {
    /// The text to show a user, without the enum wrapper.
    pub fn text(&self) -> &str {
        match self {
            Self::PeerInit(text)
            | Self::Schema(text)
            | Self::Batch(text)
            | Self::Applied(text)
            | Self::Deltas(text)
            | Self::Warning(text) => text,
        }
    }
}

/// One channel's replication state: the unparsed tail of the frame stream, the
/// schema once it arrives, the batch inners waiting for it, and the counters.
pub struct ReplicationStream {
    buffer: Vec<u8>,
    schema: Option<JoinSchema>,
    records: Vec<JoinRecord>,
    /// Decompressed batch inners that arrived before the schema did.
    waiting: Vec<(u8, Vec<u8>)>,
    waiting_bytes: usize,
    apply_requested: bool,
    applied: bool,
    /// The built place, kept so the serialization, the applied id map and the
    /// diff baseline can all be asked for after the fact rather than rebuilt.
    snapshot: Option<SnapshotApply>,
    notes: Vec<ReplicationNote>,
    pub frames: u64,
    pub bytes: u64,
    pub batches: u64,
    pub shared_strings: u64,
    pub skipped_batches: u64,
    pub chains: u64,
    pub spawns: u64,
    pub events: u64,
    pub property_updates: u64,
    pub clock_ticks: u64,
    pub stopped_chains: u64,
    pub unparsed: u64,
    /// Structural violations in the frame walk, kept apart from `unparsed`: a
    /// stream that dropped a bad length byte and resynchronized is a different
    /// failure from a stream that carried a message id nobody recognizes.
    pub walk_errors: u64,
    /// Chain item types seen, for the histogram in the report.
    pub item_types: BTreeMap<u8, u64>,
}

impl Default for ReplicationStream {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            schema: None,
            records: Vec::new(),
            waiting: Vec::new(),
            waiting_bytes: 0,
            apply_requested: false,
            applied: false,
            snapshot: None,
            notes: Vec::new(),
            frames: 0,
            bytes: 0,
            batches: 0,
            shared_strings: 0,
            skipped_batches: 0,
            chains: 0,
            spawns: 0,
            events: 0,
            property_updates: 0,
            clock_ticks: 0,
            stopped_chains: 0,
            unparsed: 0,
            walk_errors: 0,
            item_types: BTreeMap::new(),
        }
    }
}

impl ReplicationStream {
    /// Append received bytes and process every frame they close out.
    /// Returns how many complete frames this call consumed, so a caller routing
    /// bytes for the channel report can count messages rather than guesses.
    pub fn push(&mut self, bytes: &[u8]) -> usize {
        if bytes.is_empty() {
            return 0;
        }
        self.bytes += bytes.len() as u64;
        self.buffer.extend_from_slice(bytes);
        let walk = walk_frames(&self.buffer);
        if !walk.frames.is_empty() {
            self.frames += walk.frames.len() as u64;
        }
        let frames = walk.frames;
        let completed = frames.len();
        let consumed = walk.consumed;
        let error = walk.error;
        for frame in frames {
            self.handle_frame(frame);
        }
        if consumed > 0 {
            self.buffer.drain(..consumed);
        }
        if let Some(error) = error {
            // A structural violation is not something more bytes can fix, so the
            // offending length byte is dropped and the stream resynchronizes.
            self.walk_errors += 1;
            if !self.buffer.is_empty() {
                self.buffer.remove(0);
            }
            self.notes.push(ReplicationNote::Warning(format!("frame walk: {error}")));
        }
        if self.buffer.len() > MAX_STREAM_BUFFER_BYTES {
            let keep = 4 * 1024 * 1024;
            let drop_to = self.buffer.len() - keep;
            self.buffer.drain(..drop_to);
            self.notes.push(ReplicationNote::Warning(format!(
                "replication buffer exceeded {MAX_STREAM_BUFFER_BYTES} byte(s) without closing a frame; {drop_to} byte(s) dropped"
            )));
        }
        completed
    }

    fn handle_frame(&mut self, frame: ReplicationFrame) {
        match frame.id {
            JOIN_START_TAG => {
                let note = match parse_peer_init(&frame.body) {
                    Ok(init) => ReplicationNote::PeerInit(format!(
                        "0x81 join start: enabled={} flags={} isr={}/{}/{} conditional={} tail byte={} peer={} ({} byte(s) of unparsed fields)",
                        init.enabled,
                        init.flags,
                        init.isr_read,
                        init.isr_write,
                        init.isr_legacy,
                        init.conditional,
                        init.version_tail,
                        init.peer,
                        init.tail.len()
                    )),
                    Err(error) => {
                        self.unparsed += 1;
                        ReplicationNote::Warning(format!("0x81 join start: {error}"))
                    }
                };
                self.notes.push(note);
            }
            SNAPSHOT_TAG => {
                let note = match parse_snapshot_envelope(&frame.body) {
                    Ok(envelope) => match parse_join_schema(&envelope.inner) {
                        Ok(schema) => {
                            let had_tail = schema.tail_offset < envelope.inner.len();
                            let note = ReplicationNote::Schema(format!(
                                "0x97 snapshot, {} framing ({} -> {} byte(s)): {}",
                                match envelope.framing {
                                    SnapshotFraming::CodecMagic => "codec-magic",
                                    SnapshotFraming::LegacyBigEndian => "legacy BE",
                                    SnapshotFraming::TargetLittleEndian => "LE",
                                },
                                envelope.uncompressed_size,
                                envelope.inner.len(),
                                schema.summary()
                            ));
                            self.schema = Some(schema);
                            self.drain_waiting();
                            self.notes.push(note);
                            if had_tail {
                                self.notes.push(ReplicationNote::Warning(
                                    "the snapshot schema dictionary is followed by per-class tables this build does not parse"
                                        .into(),
                                ));
                            }
                            return;
                        }
                        Err(error) => {
                            self.unparsed += 1;
                            ReplicationNote::Warning(format!("0x97 schema: {error}"))
                        }
                    },
                    Err(error) => {
                        self.unparsed += 1;
                        ReplicationNote::Warning(format!("0x97 snapshot: {error}"))
                    }
                };
                self.notes.push(note);
            }
            DELTA_CHAIN_TAG => self.handle_chain(frame.body),
            _ => self.unparsed += 1,
        }
    }

    fn handle_chain(&mut self, body: Vec<u8>) {
        if body.len() >= 2 && body[1] == JOIN_DATA_SUBCODE {
            let Some(item) = parse_join_data_item(&body) else {
                self.unparsed += 1;
                return;
            };
            self.batches += 1;
            self.absorb_batch(&item);
            return;
        }
        let walked = walk_delta_chain(&body, self.schema.as_ref());
        let chain = match walked {
            Ok(chain) => chain,
            Err(error) => {
                self.unparsed += 1;
                self.notes
                    .push(ReplicationNote::Warning(format!("0x83 chain: {error}")));
                return;
            }
        };
        self.chains += 1;
        self.spawns += chain.spawns as u64;
        self.events += chain.events as u64;
        self.property_updates += chain.property_updates as u64;
        for item in &chain.items {
            *self.item_types.entry(item.kind()).or_insert(0) += 1;
            if let ChainItem::Clock { .. } = item {
                self.clock_ticks += 1;
            }
            if let ChainItem::Entries(entries) = item {
                let mut records = entries.records.clone();
                self.records.append(&mut records);
            }
        }
        match &chain.stopped_at {
            Some(stop) => {
                self.stopped_chains += 1;
                self.notes.push(ReplicationNote::Deltas(format!(
                    "0x83 chain: {} item(s), then stopped at 0x{:02x} (offset {}): {}",
                    chain.items.len(),
                    stop.item,
                    stop.offset,
                    stop.reason
                )));
            }
            None if !chain.items.is_empty() => {
                self.notes.push(ReplicationNote::Deltas(format!(
                    "0x83 chain: {} item(s), {} spawn(s), {} event(s), {} property update(s), terminated={}",
                    chain.items.len(),
                    chain.spawns,
                    chain.events,
                    chain.property_updates,
                    chain.complete
                )));
            }
            None => {}
        }
    }

    fn absorb_batch(&mut self, item: &JoinDataItem) {
        let label = format!("{:?} ({})", item.kind, item.kind.label());
        let Some(inner) = item.inner.clone() else {
            self.skipped_batches += 1;
            self.notes.push(ReplicationNote::Batch(format!(
                "JoinDataItemV2 {label}: no compressed frame; {} byte(s) kept as received ({})",
                item.opaque.len(),
                item.framing.label()
            )));
            return;
        };
        if inner.is_empty() {
            self.notes
                .push(ReplicationNote::Batch(format!("JoinDataItemV2 {label}: empty batch")));
            return;
        }
        match item.kind {
            JoinDataKind::SharedString => match parse_shared_strings(&inner) {
                Ok(entries) => {
                    self.shared_strings += entries.len() as u64;
                    self.notes.push(ReplicationNote::Batch(format!(
                        "JoinDataItemV2 shared-string batch: {} string(s) from {} inflated byte(s){}",
                        entries.len(),
                        inner.len(),
                        if item.zero_trailer { ", zero trailer" } else { "" }
                    )));
                    return;
                }
                Err(error) => {
                    self.skipped_batches += 1;
                    self.notes.push(ReplicationNote::Warning(format!(
                        "JoinDataItemV2 shared-string batch did not parse: {error}"
                    )));
                    return;
                }
            },
            JoinDataKind::Compressed | JoinDataKind::Other(_) => {
                self.skipped_batches += 1;
                self.notes.push(ReplicationNote::Batch(format!(
                    "JoinDataItemV2 {label}: {} inflated byte(s) kept; asset and unrecognized payloads are not modelled here",
                    inner.len()
                )));
                return;
            }
            _ => {}
        }
        if self.schema.is_none() {
            self.park_batch(item.kind.byte(), inner);
            return;
        }
        self.parse_batch_records(item.kind.byte(), inner);
    }

    fn park_batch(&mut self, kind: u8, inner: Vec<u8>) {
        let len = inner.len();
        self.waiting_bytes += len;
        self.waiting.push((kind, inner));
        while self.waiting_bytes > MAX_PARKED_BATCH_BYTES {
            let Some((_, oldest)) = self.waiting.first().cloned() else { break };
            self.waiting.remove(0);
            self.waiting_bytes = self.waiting_bytes.saturating_sub(oldest.len());
            self.skipped_batches += 1;
            self.notes.push(ReplicationNote::Warning(
                "dropped the oldest waiting batch: no schema arrived".into(),
            ));
        }
        self.notes.push(ReplicationNote::Batch(format!(
            "JoinDataItemV2 kind {kind}: {len} byte(s) parked until a schema arrives ({} waiting)",
            self.waiting.len()
        )));
    }

    fn drain_waiting(&mut self) {
        let waiting = std::mem::take(&mut self.waiting);
        self.waiting_bytes = 0;
        for (kind, inner) in waiting {
            self.parse_batch_records(kind, inner);
        }
    }

    fn parse_batch_records(&mut self, kind: u8, inner: Vec<u8>) {
        // The schema is borrowed only for the call, so the notes and the record
        // list can be mutated afterwards without cloning the dictionary.
        let parsed = match self.schema.as_ref() {
            Some(schema) => parse_join_records(&inner, schema),
            None => return,
        };
        match parsed {
            Ok(mut records) => {
                self.notes.push(ReplicationNote::Batch(format!(
                    "JoinDataItemV2 kind {kind}: {} record(s) from {} inflated byte(s)",
                    records.len(),
                    inner.len()
                )));
                self.records.append(&mut records);
            }
            Err(error) => {
                self.skipped_batches += 1;
                self.notes.push(ReplicationNote::Warning(format!(
                    "JoinDataItemV2 kind {kind} ({} byte(s)) did not parse as a record run: {error}",
                    inner.len()
                )));
            }
        }
    }

    /// Ask for the snapshot to be materialized. The flow calls this when the
    /// peer's snapshot-done marker arrives, because that is the point the target
    /// itself uses to release the joining client.
    pub fn request_apply(&mut self) {
        self.apply_requested = true;
    }

    pub fn schema(&self) -> Option<&JoinSchema> {
        self.schema.as_ref()
    }

    pub fn records(&self) -> usize {
        self.records.len()
    }

    pub fn apply_requested(&self) -> bool {
        self.apply_requested
    }

    /// Build the place from everything decoded so far. Idempotent: once built,
    /// the same snapshot is returned.
    pub fn apply(&mut self) -> Option<Result<&SnapshotApply, String>> {
        if !self.apply_requested || self.applied {
            return None;
        }
        if self.records.is_empty() || self.schema.is_none() {
            return None;
        }
        self.applied = true;
        let built = match self.schema.as_ref() {
            Some(schema) => apply_join_snapshot(schema, &self.records),
            None => return None,
        };
        match built {
            Ok(applied) => {
                let count = self.records.len();
                let stats = applied.stats.summary();
                self.notes.push(ReplicationNote::Applied(format!(
                    "replicated place built from {count} record(s): {stats}"
                )));
                self.snapshot = Some(applied);
                Some(Ok(self.snapshot.as_ref()?))
            }
            Err(error) => {
                self.applied = false;
                self.notes
                    .push(ReplicationNote::Warning(format!("snapshot apply failed: {error}")));
                Some(Err(error))
            }
        }
    }

    /// The serialized place, if one has been built.
    pub fn snapshot_rbxlx(&self) -> Result<Option<Vec<u8>>, String> {
        match &self.snapshot {
            Some(applied) => applied.to_rbxlx().map(Some),
            None => Ok(None),
        }
    }

    pub fn snapshot_stats(&self) -> Option<&ApplyStats> {
        self.snapshot.as_ref().map(|applied| &applied.stats)
    }

    /// The wire-id map and applied state of the built snapshot, for a caller
    /// that wants to diff local edits against what this client received.
    pub fn snapshot_ids(&self) -> Option<&BTreeMap<u32, rbx_dom_weak::types::Ref>> {
        self.snapshot.as_ref().map(|applied| &applied.ids)
    }

    pub fn snapshot_baseline(&self) -> Option<Baseline> {
        self.snapshot.as_ref().map(|applied| applied.baseline())
    }

    /// The schema's property-id binding report, if a schema arrived.
    pub fn prop_id_report(&self) -> Option<String> {
        self.schema.as_ref().map(|schema| schema.prop_id_report())
    }

    pub fn take_notes(&mut self) -> Vec<ReplicationNote> {
        std::mem::take(&mut self.notes)
    }

    pub fn summary(&self) -> String {
        let items: Vec<String> = self
            .item_types
            .iter()
            .map(|(kind, count)| format!("0x{kind:02x} x{count}"))
            .collect();
        format!(
            "{} frame(s) over {} byte(s); {} batch(es), {} record(s), {} shared string(s), {} skipped; {} chain(s): {} spawn(s), {} event(s), {} property update(s), {} clock tick(s), {} stopped; {} message(s) unrecognized, {} frame walk error(s); schema {}; snapshot {}; items [{}]",
            self.frames,
            self.bytes,
            self.batches,
            self.records.len(),
            self.shared_strings,
            self.skipped_batches,
            self.chains,
            self.spawns,
            self.events,
            self.property_updates,
            self.clock_ticks,
            self.stopped_chains,
            self.unparsed,
            self.walk_errors,
            if self.schema.is_some() { "received" } else { "not received" },
            if self.snapshot.is_some() {
                "built"
            } else if self.apply_requested {
                "requested, waiting on records"
            } else {
                "not requested"
            },
            items.join(" ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest schema this codec accepts, hand encoded: one class with
    /// three members, no enums, events or tables. Every record test uses it so
    /// the wire bytes can be written out by hand and checked exactly.
    fn encode_schema_dictionary() -> Vec<u8> {
        let mut data = Vec::new();
        data.push(3u8); // version, as one LEB128 byte
        data.extend_from_slice(&15u64.to_le_bytes()); // flags
        data.push(0); // enum count
        data.push(1); // class count
        data.push(3); // property bound
        data.push(0); // event bound
        data.push(4); // class name length
        data.extend_from_slice(b"Part");
        data.push(3); // member count
        data.push(4);
        data.extend_from_slice(b"Name"); // tag 49
        data.push(49);
        data.push(4);
        data.extend_from_slice(b"Size"); // tag 15: 24 bytes
        data.push(15);
        data.push(8);
        data.extend_from_slice(b"Anchored"); // tag 9: one byte
        data.push(9);
        data.extend_from_slice(&0u16.to_le_bytes()); // class flags
        data.push(0); // event count
        data.push(0); // URL count
        data.push(0); // shared name count
        data
    }

    fn build_schema() -> JoinSchema {
        let data = encode_schema_dictionary();
        let schema = parse_join_schema(&data).expect("schema parses");
        assert_eq!(schema.tail_offset, data.len(), "no tail");
        schema
    }

    /// Compress with the framing the target's builders write, so the batch
    /// tests do not need a captured blob.
    fn zstd(bytes: &[u8]) -> Vec<u8> {
        zstd::bulk::compress(bytes, 1).expect("compress")
    }

    fn join_data_batch(kind: u8, inner: &[u8]) -> Vec<u8> {
        let compressed = zstd(inner);
        let mut body = vec![DELTA_CHAIN_TAG, JOIN_DATA_SUBCODE, kind];
        body.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        body.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        body.extend_from_slice(&compressed);
        body.push(CHAIN_END);
        body
    }

    #[test]
    fn compact_varint_matches_the_frame_format() {
        for value in [
            0u64,
            1,
            0x3F,
            0x40,
            0x3FFF,
            0x4000,
            0x3FFF_FFFF,
            0x4000_0000,
            1 << 40,
            (1u64 << 56) - 1,
        ] {
            let encoded = write_compact_varint(value).expect("encodable");
            let (decoded, width) = read_compact_varint(&encoded, 0).expect("decodable");
            assert_eq!(decoded, value, "{value} round trip");
            assert_eq!(encoded.len(), width, "{value} width");
        }
        // A value the format cannot express is refused, not truncated.
        assert!(write_compact_varint(1u64 << 60).is_none());
        // And the widths are the four the format defines.
        assert_eq!(compact_varint_width(0x00), 1);
        assert_eq!(compact_varint_width(0x40), 2);
        assert_eq!(compact_varint_width(0x80), 4);
        assert_eq!(compact_varint_width(0xC0), 8);
    }

    #[test]
    fn frame_walk_consumes_complete_frames_only() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&frame(&[JOIN_START_TAG, 1, 2, 3]));
        buffer.extend_from_slice(&frame(&[DELTA_CHAIN_TAG, JOIN_DATA_SUBCODE, 1]));
        let walk = walk_frames(&buffer);
        assert!(walk.error.is_none(), "{:?}", walk.error);
        assert_eq!(walk.frames.len(), 2);
        assert_eq!(walk.consumed, buffer.len());
        assert!(!walk.incomplete);

        // A half-written frame is waited for, not treated as an error.
        let walk = walk_frames(&buffer[..buffer.len() - 1]);
        assert_eq!(walk.frames.len(), 1);
        assert!(walk.incomplete);
        assert_eq!(walk.consumed, 5);

        // A zero length is structural, since a frame body must carry an id.
        let walk = walk_frames(&[0x00]);
        assert!(walk.error.as_ref().unwrap().contains("empty frame"));
        // A length byte that does not fit in the buffer is a wait, not an error.
        let walk = walk_frames(&[0xC0, 1, 2]);
        assert!(walk.error.is_none());
        assert!(walk.incomplete);
        assert_eq!(walk.consumed, 0);
    }

    #[test]
    fn peer_init_keeps_the_fields_this_build_cannot_name() {
        let mut body = vec![JOIN_START_TAG, 1, 0, 2, 3, 4, 5, 6];
        body.push(42); // LEB128 peer id
        body.extend_from_slice(&[0xAA, 0xBB]);
        let init = parse_peer_init(&body).expect("peer init");
        assert_eq!(init.enabled, 1);
        assert_eq!(init.isr_read, 2);
        assert_eq!(init.peer, 42);
        assert_eq!(init.tail, vec![0xAA, 0xBB]);
        assert!(parse_peer_init(&[0x82, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(parse_peer_init(&[JOIN_START_TAG, 0, 0, 0, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn schema_dictionary_round_trips() {
        let schema = build_schema();
        assert_eq!(schema.version, 3);
        assert_eq!(schema.flags, 15);
        assert_eq!(schema.classes.len(), 1);
        assert_eq!(schema.classes[0].name, "Part");
        assert_eq!(schema.classes[0].members.len(), 3);
        assert_eq!(schema.classes[0].members[0].tag, 49);
        assert_eq!(schema.member_index(0, "size"), Some(1), "wire names are case-insensitive");
        assert_eq!(schema.member(0, 2).map(|member| member.name.as_str()), Some("Anchored"));
        assert_eq!(schema.property_bound, 3);
        assert!(schema.member(0, 3).is_none());
        assert!(schema.summary().contains("1 class(es)"));
    }

    #[test]
    fn schema_rejects_a_count_larger_than_the_buffer() {
        let mut data = vec![3u8];
        data.extend_from_slice(&0u64.to_le_bytes());
        // A 4-byte varint for 16777216 enum entries, with nothing behind it.
        data.extend_from_slice(&[0x80, 0x80, 0x80, 0x08]);
        let error = parse_join_schema(&data).unwrap_err();
        assert!(error.contains("exceeds"), "{error}");
    }

    #[test]
    fn records_parse_to_the_end_of_their_buffer() {
        let schema = build_schema();
        let data = one_part_record();
        let records = parse_join_records(&data, &schema).expect("records");
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.id, Some(7));
        assert_eq!(record.class, 0);
        assert_eq!(record.parent, Some(DATAMODEL_ROOT_ID));
        assert_eq!(record.properties.len(), 2);
        assert_eq!(record.properties[0].name, "Name");
        assert_eq!(record.properties[0].tag, 49);
        assert_eq!(name_text(&schema, &record.properties[0].raw).as_deref(), Some("Box"));
        assert_eq!(record.properties[1].name, "Size");
        // The raw extent is the value alone - tag 15 is 24 bytes - which is what
        // lets the applier re-read it without knowing the loop's framing.
        assert_eq!(record.properties[1].raw.len(), 24);
        assert_eq!(record.consumed, data.len(), "the run is exact");
    }

    fn one_part_record() -> Vec<u8> {
        let mut data = Vec::new();
        data.push(1); // reference selector, then a little-endian id
        data.extend_from_slice(&7u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes()); // class
        data.push(0); // member 0 = Name
        data.push(3);
        data.extend_from_slice(b"Box");
        data.push(1); // member 1 = Size
        for value in [2.0f32, 1.0, 1.0, 16.0, 16.0, 16.0] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.push(0xFF); // end of the property loop
        data.push(1);
        data.extend_from_slice(&DATAMODEL_ROOT_ID.to_le_bytes());
        data
    }

    #[test]
    fn a_record_run_can_be_counted() {
        let schema = build_schema();
        let mut data = one_part_record();
        data.extend_from_slice(&one_part_record());
        // Exactly one record, so the second is reported as trailing bytes.
        let (records, used) = parse_record_run(&data, &schema, 1).expect("one record");
        assert_eq!(records.len(), 1);
        assert_eq!(used, data.len() / 2);
        assert_eq!(parse_join_records(&data, &schema).expect("two records").len(), 2);
        // Declaring more than the buffer holds is an error, not a short read.
        assert!(parse_record_run(&data, &schema, 3).unwrap_err().contains("were declared"));
    }

    #[test]
    fn an_out_of_range_property_delta_fails_loudly() {
        let schema = build_schema();
        let mut data = Vec::new();
        data.push(1);
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.push(9); // the class declares three members
        let error = parse_join_records(&data, &schema).unwrap_err();
        assert!(error.contains("out of range"), "{error}");
    }

    #[test]
    fn every_documented_value_tag_is_measured() {
        // Fixed-width and self-describing tags must be skipped exactly, so a
        // record loop can continue after them. This list is the set of widths the
        // reader implements; a regression here means a value silently changed
        // size and every later field would be read from the wrong offset.
        let cases: Vec<(u8, usize, Vec<u8>)> = vec![
            (3, 0, Vec::new()),
            (9, 1, vec![1]),
            (18, 2, vec![0; 2]),
            (11, 4, vec![0; 4]),
            (20, 3, vec![0; 3]),
            (66, 3, vec![0; 3]),
            (24, 4, vec![0; 4]),
            (25, 6, vec![0; 6]),
            (12, 8, vec![0; 8]),
            (13, 8, vec![0; 8]),
            (19, 12, vec![0; 12]),
            (14, 16, vec![0; 16]),
            (6, 16, vec![0; 16]),
            (15, 24, vec![0; 24]),
            (65, 13, vec![0; 13]),
            (39, 20, vec![0; 20]),
            (35, 4, vec![0; 4]), // a zero-keypoint sequence
            (26, 13, concat(&[&[0; 12], &[1]])), // kind != 0: no matrix follows
            (27, 13, concat(&[&[0; 12], &[2]])),
            (28, 5, concat(&[&[1], &[0; 4]])),
            (69, 1, vec![0]), // null reference, no id
            (41, 1, vec![0]), // no floats advertised
            (61, 1, vec![0]), // empty content
            (56, 4, vec![2, 2, 0, 0]),
            (60, 3, vec![0xFF, 0xFF, 0x03]), // the 0xFFFF sentinel
            (49, 4, concat(&[&[3], b"abc"])),
            (7, 1, vec![5]),
            (68, 4, vec![1, 2, 3, 4]),
        ];
        for (tag, expected, payload) in cases {
            let mut reader = Reader::new(&payload);
            skip_tagged_value(&mut reader, tag, false, "test", 0)
                .unwrap_or_else(|error| panic!("tag {tag}: {error}"));
            assert_eq!(reader.position(), expected, "tag {tag} extent");
        }
        // Refused shapes must say so rather than mis-skip.
        let mut reader = Reader::new(&[1]);
        assert!(skip_tagged_value(&mut reader, 57, false, "test", 0).is_err());
        let mut reader = Reader::new(&[0x00]);
        assert!(skip_tagged_value(&mut reader, 0x7B, false, "test", 0).is_err());
        // A fixed-width tag whose payload is short is a truncation, not a skip.
        let mut reader = Reader::new(&[0; 3]);
        assert!(skip_tagged_value(&mut reader, 19, false, "test", 0).is_err());
    }

    fn concat(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|part| part.iter().copied()).collect()
    }

    #[test]
    fn a_name_value_carries_three_encodings() {
        let schema = {
            // One class, one member, and a shared name table to point at.
            let mut data = vec![3u8];
            data.extend_from_slice(&15u64.to_le_bytes());
            data.push(0);
            data.push(1);
            data.push(1);
            data.push(0);
            data.push(4);
            data.extend_from_slice(b"Part");
            data.push(1);
            data.push(4);
            data.extend_from_slice(b"Name");
            data.push(49);
            data.extend_from_slice(&0u16.to_le_bytes());
            data.push(0); // no events
            data.push(0); // no URL prefixes
            data.push(1); // one shared name
            data.push(7);
            data.extend_from_slice(b"Spawner");
            parse_join_schema(&data).expect("schema with a shared name table")
        };
        // Literal, shared-table index, then the extended form.
        assert_eq!(name_text(&schema, &[3, b'a', b'b', b'c']).as_deref(), Some("abc"));
        // The high bit marks a shared-table index, so `0x80` is entry zero.
        assert_eq!(name_text(&schema, &[0x80]).as_deref(), Some("Spawner"));
        assert_eq!(name_text(&schema, &[127, 2, b'x', b'y']).as_deref(), Some("xy"));
        // Index 127 is the escape hatch and the skip path refuses it.
        let mut reader = Reader::new(&[0xFF]);
        assert!(skip_name(&mut reader, "test").is_err());
    }

    #[test]
    fn snapshot_envelope_accepts_the_shapes_in_circulation() {
        let inner = b"schema bytes here";
        let compressed = zstd(inner);

        // The codec-magic form.
        let mut body = vec![SNAPSHOT_TAG, 0xA3, 0xCF, 0, 0];
        body.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        body.extend_from_slice(&compressed);
        let envelope = parse_snapshot_envelope(&body).expect("envelope");
        assert_eq!(envelope.framing, SnapshotFraming::CodecMagic);
        assert_eq!(envelope.inner, inner.as_slice());
        assert_eq!(envelope.codec_magic, Some([0xA3, 0xCF]));

        // The same payload in the legacy BE frame, and in LE.
        let mut be = vec![SNAPSHOT_TAG];
        be.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        be.extend_from_slice(&(inner.len() as u32).to_be_bytes());
        be.extend_from_slice(&compressed);
        let envelope = parse_snapshot_envelope(&be).expect("BE envelope");
        assert_eq!(envelope.framing, SnapshotFraming::LegacyBigEndian);
        assert_eq!(envelope.inner, inner.as_slice());

        let mut le = vec![SNAPSHOT_TAG];
        le.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        le.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        le.extend_from_slice(&compressed);
        let envelope = parse_snapshot_envelope(&le).expect("LE envelope");
        assert_eq!(envelope.framing, SnapshotFraming::TargetLittleEndian);

        // An unknown magic with no valid frame behind it is reported, not
        // guessed at.
        let mut other = body.clone();
        other[1] = 0x11;
        other[2] = 0x22;
        assert!(parse_snapshot_envelope(&other).is_err());
    }

    #[test]
    fn a_decompression_advertisement_over_the_cap_is_refused() {
        // The length field is attacker-controlled, so an enormous size must not
        // reach the allocator.
        let compressed = zstd(b"tiny");
        let mut body = vec![DELTA_CHAIN_TAG, JOIN_DATA_SUBCODE, 2];
        body.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        body.extend_from_slice(&((MAX_DECOMPRESSED_BYTES + 1) as u32).to_le_bytes());
        body.extend_from_slice(&compressed);
        let item = parse_join_data_item(&body).expect("item");
        assert!(item.inner.is_none(), "framing {:?}", item.framing);
        assert_eq!(item.framing, JoinDataFraming::Opaque);
    }

    #[test]
    fn join_data_batch_reads_both_length_orders() {
        let inner = b"hello snapshot";
        let item = parse_join_data_item(&join_data_batch(2, inner)).expect("batch");
        assert_eq!(item.kind, JoinDataKind::CacheableInstances);
        assert_eq!(item.framing, JoinDataFraming::LengthPairLittleEndian);
        assert_eq!(item.inner.as_deref(), Some(&inner[..]));
        assert!(item.zero_trailer);

        let compressed = zstd(inner);
        let mut big = vec![DELTA_CHAIN_TAG, JOIN_DATA_SUBCODE, 2];
        big.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        big.extend_from_slice(&(inner.len() as u32).to_be_bytes());
        big.extend_from_slice(&compressed);
        let item = parse_join_data_item(&big).expect("big-endian batch");
        assert_eq!(item.framing, JoinDataFraming::LengthPairBigEndian);
        assert_eq!(item.inner.as_deref(), Some(&inner[..]));
        assert!(!item.zero_trailer);

        // A blob copied without a length pair (what the target does for the
        // instance categories) stays byte-exact and unclaimed.
        let raw = vec![DELTA_CHAIN_TAG, JOIN_DATA_SUBCODE, 4, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let item = parse_join_data_item(&raw).expect("opaque batch");
        assert_eq!(item.framing, JoinDataFraming::Opaque);
        assert_eq!(&item.opaque[..], &raw[3..]);
        assert!(item.inner.is_none());

        // Not a batch at all.
        assert!(parse_join_data_item(&[DELTA_CHAIN_TAG, 0x03, 1]).is_none());
    }

    #[test]
    fn chain_walk_reports_where_it_stops() {
        let schema = build_schema();
        let mut body = vec![DELTA_CHAIN_TAG];
        body.push(0x01);
        body.push(1);
        body.extend_from_slice(&5u32.to_le_bytes());
        body.push(0x23);
        body.extend_from_slice(&777u32.to_le_bytes());
        body.push(0x11); // unreadable in the reference notes too
        let chain = walk_delta_chain(&body, Some(&schema)).expect("walk");
        assert_eq!(chain.items.len(), 2);
        assert!(!chain.complete);
        let stop = chain.stopped_at.clone().expect("stop point");
        assert_eq!(stop.item, 0x11);
        assert_eq!(stop.offset, 12);
        match &chain.items[1] {
            ChainItem::Clock { value } => assert_eq!(*value, 777),
            other => panic!("expected a clock, got {other:?}"),
        }
        assert_eq!(chain.items[0].kind(), 0x01);
    }

    #[test]
    fn a_terminated_chain_is_complete() {
        let schema = build_schema();
        let mut body = vec![DELTA_CHAIN_TAG, 0x23, 9, 0, 0, 0, CHAIN_END];
        let chain = walk_delta_chain(&body, Some(&schema)).expect("walk");
        assert!(chain.complete);
        assert!(chain.stopped_at.is_none());
        body.pop();
        // Running out of buffer is also a complete walk, just not a terminated
        // one: the reference reports it that way and so does this.
        let chain = walk_delta_chain(&body, Some(&schema)).expect("walk");
        assert!(chain.complete);
        assert_eq!(chain.items.len(), 1);
    }

    #[test]
    fn items_that_need_the_schema_wait_for_it() {
        let body = vec![DELTA_CHAIN_TAG, 0x02, 1, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0];
        let chain = walk_delta_chain(&body, None).expect("walk");
        let stop = chain.stopped_at.expect("stop point");
        assert_eq!(stop.item, 0x02);
        assert!(stop.reason.contains("network schema"), "{}", stop.reason);
    }

    #[test]
    fn a_batch_inside_a_chain_closes_it() {
        let schema = build_schema();
        let body = join_data_batch(2, &one_part_record());
        let chain = walk_delta_chain(&body, Some(&schema)).expect("walk");
        assert_eq!(chain.items.len(), 1);
        assert!(chain.complete);
        assert_eq!(chain.batches, 1);
        match &chain.items[0] {
            ChainItem::Batch(item) => {
                assert_eq!(item.inner.as_deref(), Some(one_part_record().as_slice()))
            }
            other => panic!("expected a batch, got {other:?}"),
        }
    }

    #[test]
    fn a_property_update_is_recorded_then_stops_the_walk() {
        let schema = build_schema();
        let mut body = vec![DELTA_CHAIN_TAG, 0x03];
        body.push(1);
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(&11u16.to_le_bytes());
        body.extend_from_slice(&[0xDE, 0xAD]);
        let chain = walk_delta_chain(&body, Some(&schema)).expect("walk");
        assert_eq!(chain.property_updates, 1);
        assert!(chain.stopped_at.is_some());
        match &chain.items[0] {
            ChainItem::Property(update) => {
                assert_eq!(update.entity, Some(4));
                assert_eq!(update.property, 11);
                assert_eq!(update.raw, vec![0xDE, 0xAD]);
            }
            other => panic!("expected a property update, got {other:?}"),
        }
    }

    #[test]
    fn the_stream_parks_records_until_a_schema_arrives_then_builds() {
        let mut stream = ReplicationStream::default();
        // A record batch first: it cannot be read without the class table.
        stream.push(&frame(&join_data_batch(2, &one_part_record())));
        assert_eq!(stream.records(), 0);
        let notes = stream.take_notes();
        assert!(
            notes.iter().any(|note| note.text().contains("parked until a schema")),
            "{notes:?}"
        );

        // Then the schema, which must release the parked batch.
        let schema_bytes = encode_schema_dictionary();
        let compressed = zstd(&schema_bytes);
        let mut snapshot = vec![SNAPSHOT_TAG, 0xA3, 0xCF, 0, 0];
        snapshot.extend_from_slice(&(schema_bytes.len() as u32).to_le_bytes());
        snapshot.extend_from_slice(&compressed);
        stream.push(&frame(&snapshot));
        let notes = stream.take_notes();
        assert!(notes.iter().any(|note| matches!(note, ReplicationNote::Schema(_))), "{notes:?}");
        assert!(
            notes.iter().any(|note| note.text().contains("1 record(s)")),
            "{notes:?}"
        );
        assert_eq!(stream.records(), 1);

        // The marker is what asks for the place to be built.
        stream.request_apply();
        let applied = stream.apply().expect("apply attempted").expect("built");
        assert_eq!(applied.stats.spawned, 1);
        let bytes = stream.snapshot_rbxlx().expect("serializable").expect("present");
        assert!(bytes.contains(&b'<'));
        assert!(stream.summary().contains("snapshot built"));
    }

    #[test]
    fn a_split_frame_continues_on_the_next_push() {
        let mut stream = ReplicationStream::default();
        let batch = frame(&join_data_batch(1, &shared_string_run()));
        let (head, tail) = batch.split_at(batch.len() / 2);
        assert_eq!(stream.push(head), 0, "a partial frame completes nothing");
        assert_eq!(stream.frames, 0);
        assert_eq!(stream.push(tail), 1, "the second read closes the frame");
        assert_eq!(stream.frames, 1);
        let notes = stream.take_notes();
        assert!(
            notes.iter().any(|note| note.text().contains("shared-string batch")),
            "{notes:?}"
        );
    }

    fn shared_string_run() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[7u8; 16]); // key
        data.push(1); // marker
        data.push(3); // value length
        data.extend_from_slice(b"abc");
        data
    }

    #[test]
    fn cframe_values_apply_as_position_only_and_say_so() {
        // A one-member schema whose member is CFrame (tag 27), then a record
        // carrying position (1,2,3) and a non-zero rotation kind.
        let mut data = vec![3u8];
        data.extend_from_slice(&0u64.to_le_bytes());
        data.push(0); // no enums
        data.push(1); // one class
        data.push(1); // property bound
        data.push(0); // event bound
        data.push(4);
        data.extend_from_slice(b"Part");
        data.push(1); // one member
        data.push(6);
        data.extend_from_slice(b"CFrame");
        data.push(27);
        data.extend_from_slice(&0u16.to_le_bytes());
        data.push(0); // no events
        data.push(0); // no URLs
        data.push(0); // no shared names
        let schema = parse_join_schema(&data).expect("schema");

        let mut records_data = Vec::new();
        records_data.push(1);
        records_data.extend_from_slice(&1u32.to_le_bytes());
        records_data.extend_from_slice(&0u16.to_le_bytes());
        records_data.push(0); // member 0
        for value in [1.0f32, 2.0, 3.0] {
            records_data.extend_from_slice(&value.to_le_bytes());
        }
        records_data.push(2); // kind != 0, so no quantized rotation follows
        records_data.push(0xFF);
        records_data.push(1);
        records_data.extend_from_slice(&DATAMODEL_ROOT_ID.to_le_bytes());
        let records = parse_join_records(&records_data, &schema).expect("records");
        let applied = apply_join_snapshot(&schema, &records).expect("apply");
        assert_eq!(applied.stats.cframes_position_only, 1);
        assert_eq!(applied.stats.properties_set, 1);
        let root = applied.dom.root_ref();
        let children = applied.dom.get_by_ref(root).expect("root").children().to_vec();
        let part = applied.dom.get_by_ref(children[0]).expect("part");
        let Some(Variant::CFrame(frame)) = part.properties.get(&rbx_dom_weak::Ustr::from("CFrame"))
        else {
            panic!("expected a CFrame property, got {:?}", part.properties);
        };
        assert_eq!(frame.position, Vector3::new(1.0, 2.0, 3.0));
        // The identity orientation is the honest fallback, and the stats say so.
        assert!(applied.stats.summary().contains("1 CFrame(s) position only"));
    }

    #[test]
    fn snapshot_apply_types_what_it_can_and_counts_the_rest() {
        let schema = build_schema();
        let records = parse_join_records(&one_part_record(), &schema).expect("records");
        let applied = apply_join_snapshot(&schema, &records).expect("apply");
        assert_eq!(applied.stats.spawned, 1);
        assert_eq!(applied.stats.names_set, 1);
        // Member one is `size`, which the reflection database types as a Vector3 -
        // twelve bytes. The record carries a twenty-four-byte extent, so the value
        // is counted as undecoded rather than read out of a longer payload.
        assert_eq!(applied.stats.properties_undecoded, 1);
        assert_eq!(applied.stats.properties_set, 1, "only Anchored is typed here");
        assert_eq!(applied.stats.parents_grafted, 0, "parent 13 is the root");
        let root = applied.dom.root_ref();
        let children = applied.dom.get_by_ref(root).expect("root").children().to_vec();
        assert_eq!(children.len(), 1);
        let part = applied.dom.get_by_ref(children[0]).expect("part");
        assert_eq!(part.class, "Part");
        assert_eq!(part.name.to_string(), "Box");
        assert!(
            !part.properties.contains_key(&rbx_dom_weak::Ustr::from("Size")),
            "a refused extent leaves no value behind"
        );
        assert_eq!(
            part.properties.get(&rbx_dom_weak::Ustr::from("Anchored")),
            Some(&Variant::Bool(true))
        );
        let bytes = applied.to_rbxlx().expect("serializes");
        assert!(bytes.starts_with(b"<roblox") || bytes.starts_with(b"<?xml"));
    }

    #[test]
    fn parent_links_are_resolved_in_a_second_pass() {
        // A child that precedes its parent, which is the normal order in a
        // compressed record run.
        let schema = build_schema();
        let mut data = Vec::new();
        data.push(1);
        data.extend_from_slice(&20u32.to_le_bytes()); // child
        data.extend_from_slice(&0u16.to_le_bytes());
        data.push(0xFF);
        data.push(1);
        data.extend_from_slice(&21u32.to_le_bytes()); // parent, not yet seen
        data.push(1);
        data.extend_from_slice(&21u32.to_le_bytes()); // parent
        data.extend_from_slice(&0u16.to_le_bytes());
        data.push(0xFF);
        data.push(1);
        data.extend_from_slice(&DATAMODEL_ROOT_ID.to_le_bytes());
        let records = parse_join_records(&data, &schema).expect("records");
        let applied = apply_join_snapshot(&schema, &records).expect("apply");
        assert_eq!(applied.stats.spawned, 2);
        assert_eq!(applied.stats.parents_grafted, 0);
        let root = applied.dom.root_ref();
        let top = applied.dom.get_by_ref(root).expect("root").children().to_vec();
        assert_eq!(top.len(), 1, "only the parent is at the top");
        let parent = applied.dom.get_by_ref(top[0]).expect("parent");
        assert_eq!(parent.children().len(), 1);
    }

    #[test]
    fn unknown_messages_are_counted_not_guessed() {
        let mut stream = ReplicationStream::default();
        stream.push(&frame(&[0xC3, 1, 2, 3]));
        stream.push(&frame(&[0x9B, 1, 2, 3]));
        assert_eq!(stream.unparsed, 2);
        assert_eq!(stream.frames, 2);
        assert!(stream.take_notes().is_empty(), "no note for an unknown id");
        assert!(stream.summary().contains("2 message(s) unrecognized"));
    }

    #[test]
    fn a_broken_frame_resynchronizes_instead_of_stalling_the_stream() {
        let mut stream = ReplicationStream::default();
        // A zero-length frame cannot hold a message id, so it is structural.
        assert_eq!(stream.push(&[0x00]), 0);
        assert!(stream
            .take_notes()
            .iter()
            .any(|note| note.text().contains("frame walk")));
        assert!(stream.buffer.is_empty(), "the bad byte was dropped");
        // And a well-formed frame after it still parses.
        stream.push(&frame(&[0xC3]));
        assert_eq!(stream.frames, 1);
        assert_eq!(stream.unparsed, 1);
        // The resynchronization is its own fact: it is not an unknown message id.
        assert_eq!(stream.walk_errors, 1);
    }
}
