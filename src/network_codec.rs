//! Roblox 2022 network byte-stream and schema codecs.
//!
//! Derived directly from the decompiled `NetworkStream.c` and
//! `NetworkSchema.c` uploaded in `roblox-2022-network-codec-extras.zip`,
//! plus the complete `VariantValueSetter.c` uploaded in
//! `roblox-2022-raknet-and-schema.zip`.
//! Important wire facts confirmed by those functions:
//!
//! - NetworkStream is BYTE-aligned (despite several callers naming it a
//!   BitStream); offsets and bounds are stored in bytes.
//! - Multi-byte primitives and IEEE float/double bit patterns are BIG-endian.
//! - `NetworkStream::readString` = u32-BE length + raw UTF-8 bytes.
//! - Schema helper strings/counts use unsigned base-128 varints.
//! - Vector3 = 3 floats; UDim = float scale + i32 offset.
//! - CoordinateFrame = 3 position floats + one orientation byte. Values 1..
//!   select a standard orientation (id = byte-1); zero uses the six-byte
//!   smallest-three quaternion from `Compressor::readRotation`.
//! - `Compressor.c` additionally defines exact compact rotation, translation,
//!   full velocity, and compact velocity codecs; all readers are below.
//!
//! Schema definition packets start with byte 0x97, followed by the exact
//! `Replicator::compressBitStream` frame found in Replicator.c: compressed and
//! uncompressed u32-BE lengths, then a Zstandard frame. Both framing directions
//! and complete schema packet decoding are implemented below.

use std::fmt;

const DEFAULT_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    UnexpectedEof { needed: usize, remaining: usize },
    LimitExceeded { wanted: usize, limit: usize },
    InvalidUtf8(String),
    InvalidVarint,
    InvalidData(String),
    Compression(String),
    /// The 2022 decompile calls a codec whose implementation was not present
    /// in the uploaded archive.
    MissingCodec(&'static str),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { needed, remaining } => {
                write!(f, "network stream ended early: need {needed} byte(s), have {remaining}")
            }
            Self::LimitExceeded { wanted, limit } => {
                write!(f, "network value length {wanted} exceeds safety limit {limit}")
            }
            Self::InvalidUtf8(e) => write!(f, "network string is not UTF-8: {e}"),
            Self::InvalidVarint => write!(f, "invalid/overflowing unsigned varint"),
            Self::InvalidData(e) => write!(f, "invalid network data: {e}"),
            Self::Compression(e) => write!(f, "network Zstandard error: {e}"),
            Self::MissingCodec(name) => write!(f, "missing decompiled wire codec: {name}"),
        }
    }
}

impl std::error::Error for CodecError {}

pub type Result<T> = std::result::Result<T, CodecError>;

/// Read-only counterpart of the 2022 `RBX::Network::NetworkStream`.
pub struct NetworkReader<'a> {
    data: &'a [u8],
    offset: usize,
    limit: usize,
}

impl<'a> NetworkReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0, limit: DEFAULT_LIMIT }
    }

    pub fn with_limit(data: &'a [u8], limit: usize) -> Self {
        Self { data, offset: 0, limit }
    }

    pub fn position(&self) -> usize {
        self.offset
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn set_position(&mut self, offset: usize) -> Result<()> {
        if offset > self.data.len() {
            return Err(CodecError::UnexpectedEof {
                needed: offset - self.data.len(),
                remaining: 0,
            });
        }
        self.offset = offset;
        Ok(())
    }

    pub fn skip(&mut self, count: usize) -> Result<()> {
        self.take(count).map(|_| ())
    }

    pub fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        if count > self.remaining() {
            return Err(CodecError::UnexpectedEof {
                needed: count,
                remaining: self.remaining(),
            });
        }
        let start = self.offset;
        self.offset += count;
        Ok(&self.data[start..start + count])
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn read_i8(&mut self) -> Result<i8> {
        Ok(self.read_u8()? as i8)
    }

    pub fn read_bool(&mut self) -> Result<bool> {
        Ok(self.read_u8()? != 0)
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn read_i16(&mut self) -> Result<i16> {
        Ok(self.read_u16()? as i16)
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_i32(&mut self) -> Result<i32> {
        Ok(self.read_u32()? as i32)
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        Ok(u64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn read_i64(&mut self) -> Result<i64> {
        Ok(self.read_u64()? as i64)
    }

    pub fn read_f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.read_u32()?))
    }

    pub fn read_f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.read_u64()?))
    }

    /// `NetworkStream::readString`: fixed u32-BE byte length.
    pub fn read_string(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        self.read_utf8_bytes(len)
    }

    /// Schema/helper string: unsigned-varint byte length.
    pub fn read_var_string(&mut self) -> Result<String> {
        let len = self.read_var_u32()? as usize;
        self.read_utf8_bytes(len)
    }

    fn read_utf8_bytes(&mut self, len: usize) -> Result<String> {
        if len > self.limit {
            return Err(CodecError::LimitExceeded { wanted: len, limit: self.limit });
        }
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|e| CodecError::InvalidUtf8(e.to_string()))
    }

    /// Unsigned LEB128/base-128 varint used by
    /// `deserializeUnsignedVarint` in NetworkSchema.
    pub fn read_var_u32(&mut self) -> Result<u32> {
        let mut value = 0u32;
        for shift in (0..35).step_by(7) {
            let byte = self.read_u8()?;
            if shift == 28 && byte > 0x0f {
                return Err(CodecError::InvalidVarint);
            }
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(CodecError::InvalidVarint)
    }

    pub fn read_var_u64(&mut self) -> Result<u64> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let byte = self.read_u8()?;
            if shift == 63 && byte > 1 {
                return Err(CodecError::InvalidVarint);
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(CodecError::InvalidVarint)
    }

    /// Signed varints used by NetworkValueFormat_Int/Int64. Roblox's
    /// `deserializeSignedVarint` family uses ZigZag on the unsigned base-128
    /// representation, keeping small negative and positive values compact.
    pub fn read_var_i32(&mut self) -> Result<i32> {
        let encoded = self.read_var_u32()?;
        Ok(((encoded >> 1) as i32) ^ -((encoded & 1) as i32))
    }

    pub fn read_var_i64(&mut self) -> Result<i64> {
        let encoded = self.read_var_u64()?;
        Ok(((encoded >> 1) as i64) ^ -((encoded & 1) as i64))
    }

    pub fn read_var_bytes(&mut self) -> Result<Vec<u8>> {
        let len = self.read_var_u32()? as usize;
        if len > self.limit {
            return Err(CodecError::LimitExceeded { wanted: len, limit: self.limit });
        }
        Ok(self.take(len)?.to_vec())
    }

    pub fn read_vector3(&mut self) -> Result<[f32; 3]> {
        Ok([self.read_f32()?, self.read_f32()?, self.read_f32()?])
    }

    pub fn read_vector2_i16(&mut self) -> Result<[i16; 2]> {
        Ok([self.read_i16()?, self.read_i16()?])
    }

    pub fn read_vector3_i16(&mut self) -> Result<[i16; 3]> {
        Ok([self.read_i16()?, self.read_i16()?, self.read_i16()?])
    }

    pub fn read_color3_u8(&mut self) -> Result<[u8; 3]> {
        Ok([self.read_u8()?, self.read_u8()?, self.read_u8()?])
    }

    pub fn read_udim(&mut self) -> Result<UDim> {
        Ok(UDim { scale: self.read_f32()?, offset: self.read_i32()? })
    }

    pub fn read_udim2(&mut self) -> Result<UDim2> {
        Ok(UDim2 { x: self.read_udim()?, y: self.read_udim()? })
    }

    /// Reads the position and orientation marker of a CoordinateFrame.
    /// Non-zero markers select one of Roblox's standard axis-aligned matrices
    /// (`marker - 1`). Zero is followed by the six-byte smallest-three
    /// quaternion encoded by `Compressor::readRotation`.
    pub fn read_coordinate_frame(&mut self) -> Result<CoordinateFrame> {
        let position = self.read_vector3()?;
        let marker = self.read_u8()?;
        let rotation = if marker == 0 {
            self.read_rotation()?
        } else {
            Rotation::Standard(marker - 1)
        };
        Ok(CoordinateFrame { position, rotation })
    }

    /// Exact six-byte `Compressor::readRotation` codec: three signed 15-bit
    /// smallest-three quaternion components and a two-bit omitted-axis index.
    pub fn read_rotation(&mut self) -> Result<Rotation> {
        let qx_encoded = self.read_u16()?;
        let qyz_encoded = self.read_u32()?;
        let omitted = (qyz_encoded >> 30) as usize;
        let packed = [
            sign_extend(u32::from(qx_encoded) & 0x7fff, 15),
            sign_extend((qyz_encoded >> 15) & 0x7fff, 15),
            sign_extend(qyz_encoded & 0x7fff, 15),
        ];
        let q = decode_smallest_three(packed, 16383.0, omitted);
        Ok(Rotation::Matrix(quaternion_to_matrix(q)))
    }

    /// Exact four-byte compact rotation codec: three signed 10-bit
    /// smallest-three components plus the omitted-axis index.
    pub fn read_rotation_compact(&mut self) -> Result<Rotation> {
        let encoded = self.read_u32()?;
        let omitted = (encoded >> 30) as usize;
        let packed = [
            sign_extend((encoded >> 20) & 0x3ff, 10),
            sign_extend((encoded >> 10) & 0x3ff, 10),
            sign_extend(encoded & 0x3ff, 10),
        ];
        let q = decode_smallest_three(packed, 511.0, omitted);
        Ok(Rotation::Matrix(quaternion_to_matrix(q)))
    }

    /// Exact variable-width `Compressor::readTranslation` codec. The header
    /// stores a five-bit exponent and the sign bit of each coordinate; the
    /// remaining magnitudes use 10, 16, or 21 bits depending on the exponent.
    pub fn read_translation(&mut self) -> Result<[f32; 3]> {
        let header = self.read_u8()?;
        let exponent = u32::from(header >> 3);
        let signs = [u32::from((header >> 2) & 1), u32::from((header >> 1) & 1), u32::from(header & 1)];
        let scale = 2.0_f32.powi(exponent as i32);

        let (x, y, z, denominator) = if exponent <= 4 {
            let packed = self.read_u32()?;
            (
                sign_extend((signs[0] << 10) | (packed >> 20), 11),
                sign_extend((signs[1] << 10) | ((packed >> 10) & 0x3ff), 11),
                sign_extend((signs[2] << 10) | (packed & 0x3ff), 11),
                1023.0,
            )
        } else if exponent <= 10 {
            (
                sign_extend((signs[0] << 16) | u32::from(self.read_u16()?), 17),
                sign_extend((signs[1] << 16) | u32::from(self.read_u16()?), 17),
                sign_extend((signs[2] << 16) | u32::from(self.read_u16()?), 17),
                65535.0,
            )
        } else {
            let xy = self.read_u32()?;
            let yz = self.read_u32()?;
            (
                sign_extend((signs[0] << 21) | (xy >> 11), 22),
                sign_extend((signs[1] << 21) | ((xy & 0x7ff) << 10) | (yz >> 21), 22),
                sign_extend((signs[2] << 21) | (yz & 0x1f_ffff), 22),
                2_097_151.0,
            )
        };
        Ok([
            x as f32 / denominator * scale,
            y as f32 / denominator * scale,
            z as f32 / denominator * scale,
        ])
    }

    /// Exact five-byte `Compressor::readVelocity` codec. A zero header is the
    /// special all-zero vector; otherwise it carries an exponent and the high
    /// four bits of Z while a u32 carries three signed 12-bit components.
    pub fn read_velocity(&mut self) -> Result<[f32; 3]> {
        let header = self.read_u8()?;
        if header == 0 {
            return Ok([0.0; 3]);
        }
        let packed = self.read_u32()?;
        let x = sign_extend(packed >> 20, 12);
        let y = sign_extend((packed >> 8) & 0xfff, 12);
        let z = sign_extend((u32::from(header & 0x0f) << 8) | (packed & 0xff), 12);
        let scale = 2.0_f32.powi(i32::from(header >> 4) - 1);
        Ok([
            x as f32 / 2047.0 * scale,
            y as f32 / 2047.0 * scale,
            z as f32 / 2047.0 * scale,
        ])
    }

    /// Exact three-byte compact velocity codec: signed five-bit exponent and
    /// three signed six-bit components. As in the full codec, zero is special.
    pub fn read_velocity_compact(&mut self) -> Result<[f32; 3]> {
        let a = self.read_u8()?;
        if a == 0 {
            return Ok([0.0; 3]);
        }
        let b = self.read_u8()?;
        let c = self.read_u8()?;
        let exponent = sign_extend(u32::from((a >> 2) & 0x1f), 5);
        let x = sign_extend((u32::from(a & 0x03) << 4) | u32::from(b >> 4), 6);
        let y = sign_extend((u32::from(b & 0x0f) << 2) | u32::from(c >> 6), 6);
        let z = sign_extend(u32::from(c & 0x3f), 6);
        let scale = 2.0_f32.powi(exponent);
        Ok([
            x as f32 / 31.0 * scale,
            y as f32 / 31.0 * scale,
            z as f32 / 31.0 * scale,
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UDim {
    pub scale: f32,
    pub offset: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UDim2 {
    pub x: UDim,
    pub y: UDim,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rotation {
    /// Roblox standard orientation id (wire marker minus one).
    Standard(u8),
    /// Row-major 3×3 rotation matrix decoded from a compressed quaternion.
    Matrix([[f32; 3]; 3]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoordinateFrame {
    pub position: [f32; 3],
    pub rotation: Rotation,
}

const ROTATION_MAX_COMPONENT: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

/// Rebuild a normalized [x,y,z,w] quaternion from Roblox's smallest-three
/// representation. `omitted` is the index of the non-negative largest
/// component; the remaining indices stay in natural ascending order.
fn decode_smallest_three(encoded: [i32; 3], denominator: f32, omitted: usize) -> [f32; 4] {
    let mut q = [0.0_f32; 4];
    let mut src = 0;
    for (axis, component) in q.iter_mut().enumerate() {
        if axis != omitted {
            *component = encoded[src] as f32 / denominator * ROTATION_MAX_COMPONENT;
            src += 1;
        }
    }
    let sum_other = q.iter().map(|v| v * v).sum::<f32>();
    q[omitted] = (1.0 - sum_other).max(0.0).sqrt();
    let norm = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut q {
            *v /= norm;
        }
    }
    q
}

fn quaternion_to_matrix(q: [f32; 4]) -> [[f32; 3]; 3] {
    let [x, y, z, w] = q;
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;
    [
        [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)],
        [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)],
        [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)],
    ]
}

/// Write-only counterpart for the primitives whose exact layout is confirmed
/// by NetworkStream.c. Useful later for client→server edit items.
#[derive(Default, Debug, Clone)]
pub struct NetworkWriter {
    data: Vec<u8>,
}

impl NetworkWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.data
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }

    pub fn write_u8(&mut self, v: u8) {
        self.data.push(v);
    }

    pub fn write_bool(&mut self, v: bool) {
        self.write_u8(u8::from(v));
    }

    pub fn write_u16(&mut self, v: u16) {
        self.write_bytes(&v.to_be_bytes());
    }

    pub fn write_i16(&mut self, v: i16) {
        self.write_bytes(&v.to_be_bytes());
    }

    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_be_bytes());
    }

    pub fn write_i32(&mut self, v: i32) {
        self.write_bytes(&v.to_be_bytes());
    }

    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_be_bytes());
    }

    pub fn write_f32(&mut self, v: f32) {
        self.write_u32(v.to_bits());
    }

    pub fn write_f64(&mut self, v: f64) {
        self.write_u64(v.to_bits());
    }

    pub fn write_string(&mut self, value: &str) -> Result<()> {
        let len = u32::try_from(value.len())
            .map_err(|_| CodecError::LimitExceeded { wanted: value.len(), limit: u32::MAX as usize })?;
        self.write_u32(len);
        self.write_bytes(value.as_bytes());
        Ok(())
    }

    pub fn write_var_u32(&mut self, mut value: u32) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            self.write_u8(byte);
            if value == 0 {
                break;
            }
        }
    }

    pub fn write_var_u64(&mut self, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            self.write_u8(byte);
            if value == 0 {
                break;
            }
        }
    }

    pub fn write_var_i32(&mut self, value: i32) {
        self.write_var_u32(((value as u32) << 1) ^ ((value >> 31) as u32));
    }

    pub fn write_var_i64(&mut self, value: i64) {
        self.write_var_u64(((value as u64) << 1) ^ ((value >> 63) as u64));
    }

    pub fn write_var_string(&mut self, value: &str) -> Result<()> {
        let len = u32::try_from(value.len())
            .map_err(|_| CodecError::LimitExceeded { wanted: value.len(), limit: u32::MAX as usize })?;
        self.write_var_u32(len);
        self.write_bytes(value.as_bytes());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Replicator Zstandard framing
// ---------------------------------------------------------------------------

/// Maximum uncompressed frame accepted from the network. The 2022 client
/// trusts the transmitted u32 enough to allocate it; Android should not let a
/// hostile/malformed server request 4 GiB, so our client uses a defensive cap.
pub const MAX_UNCOMPRESSED_FRAME: usize = 256 * 1024 * 1024;

/// Exact `Replicator::compressBitStream` framing from Replicator.c:
/// `[compressed_size:u32 BE][uncompressed_size:u32 BE][ZSTD frame]`.
pub fn compress_stream_frame(data: &[u8], compression_level: i32) -> Result<Vec<u8>> {
    if data.len() > u32::MAX as usize {
        return Err(CodecError::LimitExceeded {
            wanted: data.len(),
            limit: u32::MAX as usize,
        });
    }
    let compressed = zstd::bulk::compress(data, compression_level)
        .map_err(|e| CodecError::Compression(e.to_string()))?;
    if compressed.len() > u32::MAX as usize {
        return Err(CodecError::LimitExceeded {
            wanted: compressed.len(),
            limit: u32::MAX as usize,
        });
    }
    let mut out = NetworkWriter::new();
    out.write_u32(compressed.len() as u32);
    out.write_u32(data.len() as u32);
    out.write_bytes(&compressed);
    Ok(out.into_inner())
}

/// Exact `Replicator::decompressBitStream` behavior. It reads the two BE
/// lengths, decompresses exactly `compressed_size` bytes with ZSTD, verifies
/// the result is exactly `uncompressed_size`, and leaves the reader positioned
/// after the frame so JoinDataV2 can consume its trailing metadata.
pub fn decompress_stream_frame(reader: &mut NetworkReader<'_>) -> Result<Vec<u8>> {
    let compressed_size = reader.read_u32()? as usize;
    let uncompressed_size = reader.read_u32()? as usize;
    if uncompressed_size > MAX_UNCOMPRESSED_FRAME {
        return Err(CodecError::LimitExceeded {
            wanted: uncompressed_size,
            limit: MAX_UNCOMPRESSED_FRAME,
        });
    }
    let compressed = reader.take(compressed_size)?;
    let decoded = zstd::bulk::decompress(compressed, uncompressed_size)
        .map_err(|e| CodecError::Compression(e.to_string()))?;
    if decoded.len() != uncompressed_size {
        return Err(CodecError::InvalidData(format!(
            "expected ZSTD frame to decode to {uncompressed_size} bytes, got {}",
            decoded.len()
        )));
    }
    Ok(decoded)
}

// ---------------------------------------------------------------------------
// JoinDataItemV2 envelope (before stream decompression)
// ---------------------------------------------------------------------------

/// The 2022 server writes this RakNet user-packet marker first for each
/// JoinDataV2 blob (`LOBYTE(inByteArray) = -125`).
pub const ROBLOX_DATA_PACKET_ID: u8 = 0x83;
/// `ItemTypeJoinDataV2`, written as byte 22 by constructBlobStreams.
pub const JOIN_DATA_V2_ITEM_TYPE: u8 = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JoinDataSubtype {
    SharedString = 1,
    CacheableInstances = 2,
    NotCacheableInstances = 3,
    PartInstances = 4,
}

impl TryFrom<u8> for JoinDataSubtype {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::SharedString),
            2 => Ok(Self::CacheableInstances),
            3 => Ok(Self::NotCacheableInstances),
            4 => Ok(Self::PartInstances),
            _ => Err(CodecError::InvalidData(format!(
                "unknown JoinDataV2 subtype {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinDataV2Header {
    pub subtype: JoinDataSubtype,
    /// Offset at which the self-delimiting compressed stream begins.
    pub compressed_offset: usize,
}

/// Parse the three confirmed bytes preceding every JoinDataV2 compressed
/// stream: 0x83, item type 22, subtype 1..4. The compressed frame itself is
/// self-delimiting; after decompression advances the input cursor, instance
/// subtypes append u32-BE metadata count + invalid-range varints.
pub fn parse_join_data_v2_header(packet: &[u8]) -> Result<JoinDataV2Header> {
    let mut r = NetworkReader::new(packet);
    let packet_id = r.read_u8()?;
    if packet_id != ROBLOX_DATA_PACKET_ID {
        return Err(CodecError::InvalidData(format!(
            "expected Roblox data packet 0x{ROBLOX_DATA_PACKET_ID:02x}, got 0x{packet_id:02x}"
        )));
    }
    let item_type = r.read_u8()?;
    if item_type != JOIN_DATA_V2_ITEM_TYPE {
        return Err(CodecError::InvalidData(format!(
            "expected JoinDataV2 item type {JOIN_DATA_V2_ITEM_TYPE}, got {item_type}"
        )));
    }
    Ok(JoinDataV2Header {
        subtype: JoinDataSubtype::try_from(r.read_u8()?)?,
        compressed_offset: r.position(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidJoinRange {
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinChildOrder {
    pub number_of_children: u32,
    pub index_in_parent: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedJoinDataV2 {
    pub subtype: JoinDataSubtype,
    pub stream: Vec<u8>,
    /// None for SHARED_STRING, otherwise the advertised number of instances.
    pub metadata_count: Option<u32>,
    pub invalid_ranges: Vec<InvalidJoinRange>,
    /// Present only for the deprecated ordered variant (one pair per instance).
    pub child_order: Vec<JoinChildOrder>,
    /// Server appended ItemTypeEnd (zero) after this JoinData item.
    pub has_end_marker: bool,
}

/// Decode the complete outer JoinDataV2 packet through ZSTD and parse its
/// trailing metadata exactly as `DeserializingJoinDataItemV2::prepWork` does.
/// `no_order_info` is the negotiated getJDIv2NoOrderInfo feature flag; when
/// false, the legacy child-count/index pair follows for every instance.
pub fn decode_join_data_v2_packet(
    packet: &[u8],
    no_order_info: bool,
) -> Result<DecodedJoinDataV2> {
    let header = parse_join_data_v2_header(packet)?;
    let mut reader = NetworkReader::new(packet);
    reader.set_position(header.compressed_offset)?;
    let stream = decompress_stream_frame(&mut reader)?;

    let mut metadata_count = None;
    let mut invalid_ranges = Vec::new();
    let mut child_order = Vec::new();
    if header.subtype != JoinDataSubtype::SharedString {
        let count = reader.read_u32()?;
        metadata_count = Some(count);
        let invalid_count = checked_count(reader.read_var_u32()?, "invalid JoinData range")?;
        invalid_ranges.reserve(invalid_count);
        for _ in 0..invalid_count {
            let range = InvalidJoinRange {
                offset: reader.read_var_u32()?,
                length: reader.read_var_u32()?,
            };
            let end = u64::from(range.offset) + u64::from(range.length);
            if end > stream.len() as u64 {
                return Err(CodecError::InvalidData(format!(
                    "invalid JoinData range {}+{} exceeds decoded stream length {}",
                    range.offset,
                    range.length,
                    stream.len()
                )));
            }
            invalid_ranges.push(range);
        }
        invalid_ranges.sort_by_key(|r| r.offset);

        if !no_order_info {
            let count_usize = usize::try_from(count).map_err(|_| {
                CodecError::InvalidData(format!("JoinData metadata count {count} is too large"))
            })?;
            if count_usize > u16::MAX as usize * 16 {
                return Err(CodecError::LimitExceeded {
                    wanted: count_usize,
                    limit: u16::MAX as usize * 16,
                });
            }
            child_order.reserve(count_usize);
            for _ in 0..count_usize {
                child_order.push(JoinChildOrder {
                    number_of_children: reader.read_var_u32()?,
                    index_in_parent: reader.read_var_u32()?,
                });
            }
        }
    }

    let has_end_marker = if reader.is_empty() {
        false
    } else {
        let marker = reader.read_u8()?;
        if marker != 0 {
            return Err(CodecError::InvalidData(format!(
                "expected JoinData ItemTypeEnd marker 0, got {marker}"
            )));
        }
        true
    };
    if !reader.is_empty() {
        return Err(CodecError::InvalidData(format!(
            "{} trailing byte(s) after JoinDataV2",
            reader.remaining()
        )));
    }

    Ok(DecodedJoinDataV2 {
        subtype: header.subtype,
        stream,
        metadata_count,
        invalid_ranges,
        child_order,
        has_end_marker,
    })
}

// ---------------------------------------------------------------------------
// NetworkSchema (uncompressed payload)
// ---------------------------------------------------------------------------

/// Packet id written immediately before `compressBitStream` in
/// `generateSchemaDefinitionPacket` (decompile address 0x141111953).
pub const SCHEMA_PACKET_ID: u8 = 0x97;
/// Raw enum id used when a property/event argument is not an enum.
pub const NO_ENUM_ID: u16 = 0xffff;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaEnum {
    pub network_id: u16,
    pub name: String,
    pub value_format: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaType {
    pub value_format: u8,
    pub enum_id: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaProperty {
    pub network_id: u16,
    pub name: String,
    pub ty: SchemaType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaEvent {
    pub network_id: u16,
    pub name: String,
    pub arguments: Vec<SchemaType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaClass {
    pub network_id: u16,
    pub name: String,
    /// Global property IDs assigned consecutively across all classes.
    pub properties: Vec<SchemaProperty>,
    /// 0xffff means this class has no special CFrame property.
    pub cframe_property_id: u16,
    /// Global event IDs assigned consecutively across all classes.
    pub events: Vec<SchemaEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSchema {
    pub enums: Vec<SchemaEnum>,
    pub classes: Vec<SchemaClass>,
    /// Always one greater than the transmitted property count because the
    /// client appends the synthetic Parent property (format 28).
    pub transmitted_property_count: u32,
    pub transmitted_event_count: u32,
    /// Prefix table; the 2022 client stores an empty entry at index zero.
    pub known_prefixes: Vec<String>,
    pub fixed_dictionary: Vec<String>,
}

impl NetworkSchema {
    pub fn class(&self, id: u16) -> Option<&SchemaClass> {
        self.classes.get(id as usize)
    }

    pub fn property(&self, id: u16) -> Option<&SchemaProperty> {
        self.classes
            .iter()
            .flat_map(|c| c.properties.iter())
            .find(|p| p.network_id == id)
    }

    pub fn event(&self, id: u16) -> Option<&SchemaEvent> {
        self.classes
            .iter()
            .flat_map(|c| c.events.iter())
            .find(|e| e.network_id == id)
    }
}

// ---------------------------------------------------------------------------
// Schema value decoder
// ---------------------------------------------------------------------------

/// Numeric IDs assigned by the 2022 `NetworkValueFormat` enum. The mapping
/// comes from `defaultSchemaTypeForReflectionType` (which writes the numeric
/// IDs) cross-referenced with the named cases in the complete
/// `schemaReadValue<VariantValueSetter>` instantiation.
pub mod value_format {
    pub const UNSUPPORTED: u8 = 0;
    pub const STRING: u8 = 2;
    pub const PROTECTED_STRING_SERVER_INDEX: u8 = 3;
    pub const PROTECTED_STRING_SOURCE: u8 = 4;
    pub const ENUM_VARINT: u8 = 7;
    pub const BINARY_STRING: u8 = 8;
    pub const BOOL: u8 = 9;
    pub const INT: u8 = 10;
    pub const FLOAT: u8 = 11;
    pub const DOUBLE: u8 = 12;
    pub const UDIM: u8 = 13;
    pub const UDIM2: u8 = 14;
    pub const RAY: u8 = 15;
    pub const FACES: u8 = 16;
    pub const AXES: u8 = 17;
    pub const BRICK_COLOR: u8 = 18;
    pub const COLOR3: u8 = 19;
    pub const COLOR3_UINT8: u8 = 20;
    pub const VECTOR2: u8 = 21;
    pub const VECTOR3: u8 = 22;
    pub const VECTOR2_INT16: u8 = 24;
    pub const VECTOR3_INT16: u8 = 25;
    pub const CFRAME_EXACT: u8 = 26;
    pub const CFRAME: u8 = 27;
    pub const INSTANCE_GUID: u8 = 28;
    pub const TUPLE: u8 = 29;
    pub const VALUE_ARRAY: u8 = 30;
    pub const VALUE_TABLE: u8 = 31;
    pub const VALUE_MAP: u8 = 32;
    pub const CONTENT_ID: u8 = 33;
    pub const SYSTEM_ADDRESS: u8 = 34;
    pub const NUMBER_SEQUENCE: u8 = 35;
    pub const NUMBER_SEQUENCE_KEYPOINT: u8 = 36;
    pub const NUMBER_RANGE: u8 = 37;
    pub const COLOR_SEQUENCE: u8 = 38;
    pub const COLOR_SEQUENCE_KEYPOINT: u8 = 39;
    pub const RECT2D: u8 = 40;
    pub const PHYSICAL_PROPERTIES: u8 = 41;
    pub const REGION3: u8 = 42;
    pub const REGION3_INT16: u8 = 43;
    pub const INT64: u8 = 44;
    pub const SHARED_STRING: u8 = 46;
    pub const PROTECTED_STRING_BYTECODE: u8 = 47;
    pub const DATE_TIME: u8 = 48;
    pub const STRING_FIXED_DICTIONARY: u8 = 49;
    pub const OPTIONAL_CFRAME_EXACT: u8 = 50;
    pub const OPTIONAL_CFRAME: u8 = 51;
    pub const UNIQUE_ID: u8 = 52;
    pub const PATH_WAYPOINT: u8 = 53;
    pub const FONT: u8 = 54;
}

#[derive(Debug, Clone, PartialEq)]
pub enum NetworkValue {
    String(String),
    Binary(Vec<u8>),
    Bool(bool),
    Int(i32),
    Int64(i64),
    Float(f32),
    Double(f64),
    Enum(u32),
    UDim(UDim),
    UDim2(UDim2),
    Ray { origin: [f32; 3], direction: [f32; 3] },
    BitMask(i32),
    BrickColor(u16),
    Color3([f32; 3]),
    Color3Uint8([u8; 3]),
    Vector2([f32; 2]),
    Vector3([f32; 3]),
    Vector2Int16([i16; 2]),
    Vector3Int16([i16; 3]),
    CoordinateFrame(CoordinateFrame),
    ContentId(String),
    SystemAddress(u32),
    NumberSequence(Vec<[f32; 3]>),
    NumberSequenceKeypoint([f32; 3]),
    NumberRange([f32; 2]),
    ColorSequence(Vec<[f32; 5]>),
    ColorSequenceKeypoint([f32; 5]),
    Rect2d { min: [f32; 2], max: [f32; 2] },
    /// None means Roblox's default physical properties; Some stores density,
    /// friction, elasticity, friction weight, and elasticity weight.
    PhysicalProperties(Option<[f32; 5]>),
    Region3 { low: [f32; 3], high: [f32; 3] },
    Region3Int16 { min: [i16; 3], max: [i16; 3] },
    /// MD5-like 16-byte dictionary key. JoinData subtype 1 supplies the body.
    SharedStringHash([u8; 16]),
    DateTimeMillis(i64),
    OptionalCoordinateFrame(Option<CoordinateFrame>),
    UniqueId { raw_bits: u64, timestamp: u32, index: u32 },
    PathWaypoint { position: [f32; 3], action: u8, label: String },
    Font { weight: u16, style: u8, family: String, cached_face_id: String },
    FixedDictionaryString { network_id: u32, value: String },
    /// Format 3 intentionally carries no bytes in the 2022 reader; Studio
    /// leaves the destination value unchanged.
    UnavailableProtectedString,
}

pub fn network_value_format_name(format: u8) -> &'static str {
    use value_format::*;
    match format {
        UNSUPPORTED => "Unsupported",
        STRING => "String_NeverDictionary",
        PROTECTED_STRING_SERVER_INDEX => "ProtectedStringServerIndexString",
        PROTECTED_STRING_SOURCE => "ProtectedStringSource",
        ENUM_VARINT => "Enum_VarInt",
        BINARY_STRING => "BinaryString",
        BOOL => "Bool",
        INT => "Int",
        FLOAT => "Float",
        DOUBLE => "Double",
        UDIM => "UDim",
        UDIM2 => "UDim2",
        RAY => "Ray",
        FACES => "Faces",
        AXES => "Axes",
        BRICK_COLOR => "BrickColor",
        COLOR3 => "Color3",
        COLOR3_UINT8 => "Color3uint8",
        VECTOR2 => "Vector2",
        VECTOR3 => "Vector3_Fixed12Bytes",
        VECTOR2_INT16 => "Vector2int16",
        VECTOR3_INT16 => "Vector3int16",
        CFRAME_EXACT => "CoordinateFrame_ExactEncoding",
        CFRAME => "CoordinateFrame_GeneralEncoding",
        INSTANCE_GUID => "InstanceGuid",
        TUPLE => "Tuple",
        VALUE_ARRAY => "ValueArray",
        VALUE_TABLE => "ValueTable",
        VALUE_MAP => "ValueMap",
        CONTENT_ID => "ContentId",
        SYSTEM_ADDRESS => "SystemAddress",
        NUMBER_SEQUENCE => "NumberSequence",
        NUMBER_SEQUENCE_KEYPOINT => "NumberSequenceKeypoint",
        NUMBER_RANGE => "NumberRange",
        COLOR_SEQUENCE => "ColorSequence",
        COLOR_SEQUENCE_KEYPOINT => "ColorSequenceKeypoint",
        RECT2D => "Rect2d",
        PHYSICAL_PROPERTIES => "PhysicalProperties",
        REGION3 => "Region3",
        REGION3_INT16 => "Region3int16",
        INT64 => "Int64",
        SHARED_STRING => "SharedString",
        PROTECTED_STRING_BYTECODE => "ProtectedStringBytecode",
        DATE_TIME => "DateTime",
        STRING_FIXED_DICTIONARY => "String_FixedDictionary",
        OPTIONAL_CFRAME_EXACT => "OptionalCoordinateFrame_ExactEncoding",
        OPTIONAL_CFRAME => "OptionalCoordinateFrame_GeneralEncoding",
        UNIQUE_ID => "UniqueId",
        PATH_WAYPOINT => "PathWaypointWithLabel",
        FONT => "Font",
        _ => "Unknown",
    }
}

fn read_hash16(reader: &mut NetworkReader<'_>) -> Result<[u8; 16]> {
    Ok(reader.take(16)?.try_into().unwrap())
}

fn read_float_array<const N: usize>(reader: &mut NetworkReader<'_>) -> Result<[f32; N]> {
    let mut result = [0.0; N];
    for value in &mut result {
        *value = reader.read_f32()?;
    }
    Ok(result)
}

fn read_i16_array<const N: usize>(reader: &mut NetworkReader<'_>) -> Result<[i16; N]> {
    let mut result = [0; N];
    for value in &mut result {
        *value = reader.read_i16()?;
    }
    Ok(result)
}

fn read_sequence<const N: usize>(reader: &mut NetworkReader<'_>, kind: &str) -> Result<Vec<[f32; N]>> {
    let count = reader.read_u32()? as usize;
    // Both 2022 NetworkStream readers reject over 20 keypoints.
    if count > 20 {
        return Err(CodecError::InvalidData(format!("{kind} has {count} keypoints; maximum is 20")));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(read_float_array(reader)?);
    }
    Ok(values)
}

fn read_optional_cframe(reader: &mut NetworkReader<'_>) -> Result<Option<CoordinateFrame>> {
    // Unlike ordinary CFrame, OptionalCFrame packs presence into the high bit
    // of the orientation marker and writes rotation BEFORE translation.
    let marker = reader.read_u8()?;
    if marker & 0x80 == 0 {
        return Ok(None);
    }
    let rotation_marker = marker & 0x7f;
    let rotation = if rotation_marker == 0 {
        reader.read_rotation()?
    } else {
        Rotation::Standard(rotation_marker - 1)
    };
    let position = reader.read_vector3()?;
    Ok(Some(CoordinateFrame { position, rotation }))
}

/// Decode one property/event value according to the value format carried by
/// NetworkSchema. This covers every fixed-layout format in the uploaded 2022
/// `VariantValueSetter` body. Dynamic containers, Instance GUID scope coding,
/// exact-CFrame coding, and prefixed ContentIds deliberately return
/// `MissingCodec` rather than guessing.
pub fn read_schema_value(
    reader: &mut NetworkReader<'_>,
    ty: &SchemaType,
    schema: &NetworkSchema,
    use_dictionaries: bool,
) -> Result<NetworkValue> {
    use value_format::*;
    match ty.value_format {
        STRING => Ok(NetworkValue::String(reader.read_var_string()?)),
        PROTECTED_STRING_SERVER_INDEX => Ok(NetworkValue::UnavailableProtectedString),
        PROTECTED_STRING_SOURCE | PROTECTED_STRING_BYTECODE => {
            Ok(NetworkValue::SharedStringHash(read_hash16(reader)?))
        }
        ENUM_VARINT => Ok(NetworkValue::Enum(reader.read_var_u32()?)),
        BINARY_STRING => Ok(NetworkValue::Binary(reader.read_var_bytes()?)),
        BOOL => Ok(NetworkValue::Bool(reader.read_bool()?)),
        INT => Ok(NetworkValue::Int(reader.read_var_i32()?)),
        FLOAT => Ok(NetworkValue::Float(reader.read_f32()?)),
        DOUBLE => Ok(NetworkValue::Double(reader.read_f64()?)),
        UDIM => Ok(NetworkValue::UDim(reader.read_udim()?)),
        UDIM2 => Ok(NetworkValue::UDim2(reader.read_udim2()?)),
        RAY => Ok(NetworkValue::Ray {
            origin: reader.read_vector3()?,
            direction: reader.read_vector3()?,
        }),
        FACES | AXES => Ok(NetworkValue::BitMask(reader.read_i32()?)),
        BRICK_COLOR => Ok(NetworkValue::BrickColor(reader.read_u16()?)),
        COLOR3 => Ok(NetworkValue::Color3(reader.read_vector3()?)),
        COLOR3_UINT8 => Ok(NetworkValue::Color3Uint8(reader.read_color3_u8()?)),
        VECTOR2 => Ok(NetworkValue::Vector2(read_float_array(reader)?)),
        VECTOR3 => Ok(NetworkValue::Vector3(reader.read_vector3()?)),
        VECTOR2_INT16 => Ok(NetworkValue::Vector2Int16(reader.read_vector2_i16()?)),
        VECTOR3_INT16 => Ok(NetworkValue::Vector3Int16(reader.read_vector3_i16()?)),
        CFRAME_EXACT => Err(CodecError::MissingCodec("deserializeCoordinateFrameExact")),
        CFRAME => Ok(NetworkValue::CoordinateFrame(reader.read_coordinate_frame()?)),
        INSTANCE_GUID => Err(CodecError::MissingCodec("deserializeGuidScope")),
        TUPLE | VALUE_ARRAY | VALUE_TABLE | VALUE_MAP => {
            Err(CodecError::MissingCodec("dynamic Variant container"))
        }
        CONTENT_ID if use_dictionaries => Err(CodecError::MissingCodec("readPrefixedContentId")),
        CONTENT_ID => Ok(NetworkValue::ContentId(reader.read_var_string()?.replace('\\', "/"))),
        SYSTEM_ADDRESS => Ok(NetworkValue::SystemAddress(reader.read_var_u32()?)),
        NUMBER_SEQUENCE => Ok(NetworkValue::NumberSequence(read_sequence(reader, "NumberSequence")?)),
        NUMBER_SEQUENCE_KEYPOINT => {
            Ok(NetworkValue::NumberSequenceKeypoint(read_float_array(reader)?))
        }
        NUMBER_RANGE => Ok(NetworkValue::NumberRange(read_float_array(reader)?)),
        COLOR_SEQUENCE => Ok(NetworkValue::ColorSequence(read_sequence(reader, "ColorSequence")?)),
        COLOR_SEQUENCE_KEYPOINT => {
            Ok(NetworkValue::ColorSequenceKeypoint(read_float_array(reader)?))
        }
        RECT2D => {
            let a: [f32; 2] = read_float_array(reader)?;
            let b: [f32; 2] = read_float_array(reader)?;
            Ok(NetworkValue::Rect2d {
                min: [a[0].min(b[0]), a[1].min(b[1])],
                max: [a[0].max(b[0]), a[1].max(b[1])],
            })
        }
        PHYSICAL_PROPERTIES => {
            let value = if reader.read_bool()? {
                Some(read_float_array(reader)?)
            } else {
                None
            };
            Ok(NetworkValue::PhysicalProperties(value))
        }
        REGION3 => {
            let low = reader.read_vector3()?;
            let high = reader.read_vector3()?;
            Ok(NetworkValue::Region3 { low, high })
        }
        REGION3_INT16 => {
            let min = read_i16_array(reader)?;
            let max = read_i16_array(reader)?;
            Ok(NetworkValue::Region3Int16 { min, max })
        }
        INT64 => Ok(NetworkValue::Int64(reader.read_var_i64()?)),
        SHARED_STRING => Ok(NetworkValue::SharedStringHash(read_hash16(reader)?)),
        DATE_TIME => Ok(NetworkValue::DateTimeMillis(reader.read_i64()?)),
        STRING_FIXED_DICTIONARY => {
            let first = reader.read_u8()?;
            if first & 0x80 != 0 {
                let short_id = u32::from(first & 0x7f);
                let network_id = if short_id < 0x7f {
                    short_id
                } else {
                    127_u32.checked_add(reader.read_var_u32()?).ok_or(CodecError::InvalidVarint)?
                };
                let value = schema
                    .fixed_dictionary
                    .get(network_id as usize)
                    .ok_or_else(|| CodecError::InvalidData(format!(
                        "fixed dictionary id {network_id} is out of range ({})",
                        schema.fixed_dictionary.len()
                    )))?
                    .clone();
                Ok(NetworkValue::FixedDictionaryString { network_id, value })
            } else {
                let value = if first < 0x7f {
                    reader.read_utf8_bytes(first as usize)?
                } else {
                    reader.read_var_string()?
                };
                Ok(NetworkValue::String(value))
            }
        }
        OPTIONAL_CFRAME_EXACT => Err(CodecError::MissingCodec("deserializeOptionalCoordinateFrameExact")),
        OPTIONAL_CFRAME => Ok(NetworkValue::OptionalCoordinateFrame(read_optional_cframe(reader)?)),
        UNIQUE_ID => Ok(NetworkValue::UniqueId {
            raw_bits: reader.read_u64()?,
            timestamp: reader.read_u32()?,
            index: reader.read_u32()?,
        }),
        PATH_WAYPOINT => Ok(NetworkValue::PathWaypoint {
            position: reader.read_vector3()?,
            action: reader.read_u8()?,
            label: reader.read_string()?,
        }),
        FONT => Ok(NetworkValue::Font {
            weight: reader.read_u16()?,
            style: reader.read_u8()?,
            family: reader.read_string()?.replace('\\', "/"),
            cached_face_id: reader.read_string()?.replace('\\', "/"),
        }),
        UNSUPPORTED => Err(CodecError::InvalidData("value uses unsupported network format".into())),
        other => Err(CodecError::InvalidData(format!(
            "unknown NetworkValueFormat {other}"
        ))),
    }
}

/// Parse the exact UNCOMPRESSED payload consumed by
/// `NetworkSchema::initializeFromSchemaDefinitionPacket` after its call to
/// `Replicator::decompressBitStream`.
pub fn parse_uncompressed_schema(bytes: &[u8]) -> Result<NetworkSchema> {
    let mut r = NetworkReader::new(bytes);

    let enum_count = checked_count(r.read_var_u32()?, "enum")?;
    let mut enums = Vec::with_capacity(enum_count);
    for id in 0..enum_count {
        enums.push(SchemaEnum {
            network_id: checked_u16(id, "enum")?,
            name: r.read_var_string()?,
            value_format: r.read_u8()?,
        });
    }

    let class_count = checked_count(r.read_var_u32()?, "class")?;
    let property_count = r.read_var_u32()?;
    let event_count = r.read_var_u32()?;
    let mut classes = Vec::with_capacity(class_count);
    let mut next_property = 0usize;
    let mut next_event = 0usize;

    for class_id in 0..class_count {
        let name = r.read_var_string()?;
        let class_property_count = checked_count(r.read_var_u32()?, "class property")?;
        let mut properties = Vec::with_capacity(class_property_count);
        for _ in 0..class_property_count {
            let prop_name = r.read_var_string()?;
            let value_format = r.read_u8()?;
            // initializeFromSchemaDefinitionPacket reads an enum id only for
            // property format 7. It always reads one for event arguments.
            let enum_id = if value_format == 7 { r.read_u16()? } else { NO_ENUM_ID };
            properties.push(SchemaProperty {
                network_id: checked_u16(next_property, "property")?,
                name: prop_name,
                ty: SchemaType { value_format, enum_id },
            });
            next_property += 1;
        }

        let cframe_property_id = r.read_u16()?;
        let class_event_count = checked_count(r.read_var_u32()?, "class event")?;
        let mut events = Vec::with_capacity(class_event_count);
        for _ in 0..class_event_count {
            let event_name = r.read_var_string()?;
            let argument_count = checked_count(r.read_var_u32()?, "event argument")?;
            let mut arguments = Vec::with_capacity(argument_count);
            for _ in 0..argument_count {
                arguments.push(SchemaType {
                    value_format: r.read_u8()?,
                    enum_id: r.read_u16()?,
                });
            }
            events.push(SchemaEvent {
                network_id: checked_u16(next_event, "event")?,
                name: event_name,
                arguments,
            });
            next_event += 1;
        }

        classes.push(SchemaClass {
            network_id: checked_u16(class_id, "class")?,
            name,
            properties,
            cframe_property_id,
            events,
        });
    }

    if next_property != property_count as usize {
        return Err(CodecError::InvalidData(format!(
            "schema advertised {property_count} properties but classes contained {next_property}"
        )));
    }
    if next_event != event_count as usize {
        return Err(CodecError::InvalidData(format!(
            "schema advertised {event_count} events but classes contained {next_event}"
        )));
    }

    // These two tables were appended later in the protocol and the 2022
    // reader deliberately checks remaining bytes before each, preserving
    // backwards compatibility with schemas that end after classes.
    let mut known_prefixes = vec![String::new()];
    if !r.is_empty() {
        let count = checked_count(r.read_var_u32()?, "known prefix")?;
        known_prefixes.reserve(count);
        for _ in 0..count {
            known_prefixes.push(r.read_var_string()?);
        }
    }

    let mut fixed_dictionary = Vec::new();
    if !r.is_empty() {
        let count = checked_count(r.read_var_u32()?, "fixed dictionary")?;
        fixed_dictionary.reserve(count);
        for _ in 0..count {
            fixed_dictionary.push(r.read_var_string()?);
        }
    }

    if !r.is_empty() {
        return Err(CodecError::InvalidData(format!(
            "{} trailing byte(s) after network schema",
            r.remaining()
        )));
    }

    Ok(NetworkSchema {
        enums,
        classes,
        transmitted_property_count: property_count,
        transmitted_event_count: event_count,
        known_prefixes,
        fixed_dictionary,
    })
}

/// Decode a complete 2022 schema packet: validate/remove packet id 0x97,
/// decompress its length-prefixed ZSTD stream, and parse all dictionaries.
pub fn parse_schema_packet(packet: &[u8]) -> Result<NetworkSchema> {
    let mut reader = NetworkReader::new(packet);
    let id = reader.read_u8()?;
    if id != SCHEMA_PACKET_ID {
        return Err(CodecError::InvalidData(format!(
            "expected schema packet id 0x{SCHEMA_PACKET_ID:02x}, got 0x{id:02x}"
        )));
    }
    let uncompressed = decompress_stream_frame(&mut reader)?;
    if !reader.is_empty() {
        return Err(CodecError::InvalidData(format!(
            "{} trailing byte(s) after schema ZSTD frame",
            reader.remaining()
        )));
    }
    parse_uncompressed_schema(&uncompressed)
}

fn checked_count(value: u32, kind: &str) -> Result<usize> {
    let count = value as usize;
    // Network IDs are u16 and the decompiled reader stores these counts in
    // vectors addressed by u16 IDs. Reject hostile/invalid allocations early.
    if count > u16::MAX as usize {
        return Err(CodecError::InvalidData(format!("{kind} count {count} exceeds u16 IDs")));
    }
    Ok(count)
}

fn checked_u16(value: usize, kind: &str) -> Result<u16> {
    u16::try_from(value)
        .map_err(|_| CodecError::InvalidData(format!("{kind} id {value} exceeds u16")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_are_big_endian_and_round_trip() {
        let mut w = NetworkWriter::new();
        w.write_u16(0x1234);
        w.write_u32(0x89abcdef);
        w.write_f32(1.5);
        w.write_string("hello").unwrap();
        w.write_var_u32(300);
        assert_eq!(&w.as_slice()[..6], &[0x12, 0x34, 0x89, 0xab, 0xcd, 0xef]);

        let data = w.into_inner();
        let mut r = NetworkReader::new(&data);
        assert_eq!(r.read_u16().unwrap(), 0x1234);
        assert_eq!(r.read_u32().unwrap(), 0x89abcdef);
        assert_eq!(r.read_f32().unwrap(), 1.5);
        assert_eq!(r.read_string().unwrap(), "hello");
        assert_eq!(r.read_var_u32().unwrap(), 300);
        assert!(r.is_empty());
    }

    #[test]
    fn transform_compressors_decode_identity_and_zero() {
        // Smallest-three identity quaternion: omitted component W (index 3),
        // all transmitted components zero.
        let full_rotation = [0x00, 0x00, 0xc0, 0x00, 0x00, 0x00];
        let mut r = NetworkReader::new(&full_rotation);
        match r.read_rotation().unwrap() {
            Rotation::Matrix(m) => assert_eq!(m, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            Rotation::Standard(_) => panic!("expected compressed matrix"),
        }

        let compact_rotation = [0xc0, 0x00, 0x00, 0x00];
        let mut r = NetworkReader::new(&compact_rotation);
        match r.read_rotation_compact().unwrap() {
            Rotation::Matrix(m) => assert_eq!(m, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            Rotation::Standard(_) => panic!("expected compressed matrix"),
        }

        let mut r = NetworkReader::new(&[0, 0, 0, 0, 0]);
        assert_eq!(r.read_translation().unwrap(), [0.0; 3]);
        let mut r = NetworkReader::new(&[0]);
        assert_eq!(r.read_velocity().unwrap(), [0.0; 3]);
        let mut r = NetworkReader::new(&[0]);
        assert_eq!(r.read_velocity_compact().unwrap(), [0.0; 3]);
    }

    #[test]
    fn parses_join_data_v2_envelope() {
        let h = parse_join_data_v2_header(&[0x83, 22, 3, 0xaa]).unwrap();
        assert_eq!(h.subtype, JoinDataSubtype::NotCacheableInstances);
        assert_eq!(h.compressed_offset, 3);
    }

    #[test]
    fn zstd_framing_and_join_metadata_round_trip() {
        let payload = b"one serialized instance";
        let framed = compress_stream_frame(payload, 3).unwrap();
        let mut reader = NetworkReader::new(&framed);
        assert_eq!(decompress_stream_frame(&mut reader).unwrap(), payload);
        assert!(reader.is_empty());

        let mut packet = vec![0x83, 22, 3];
        packet.extend_from_slice(&framed);
        packet.extend_from_slice(&1_u32.to_be_bytes()); // metadata count
        packet.push(0); // invalid-range count varint
        packet.push(0); // ItemTypeEnd
        let join = decode_join_data_v2_packet(&packet, true).unwrap();
        assert_eq!(join.subtype, JoinDataSubtype::NotCacheableInstances);
        assert_eq!(join.stream, payload);
        assert_eq!(join.metadata_count, Some(1));
        assert!(join.invalid_ranges.is_empty());
        assert!(join.has_end_marker);
    }

    #[test]
    fn signed_varints_round_trip() {
        let mut w = NetworkWriter::new();
        for value in [i32::MIN, -65, -1, 0, 1, 64, i32::MAX] {
            w.write_var_i32(value);
        }
        for value in [i64::MIN, -65, -1, 0, 1, 64, i64::MAX] {
            w.write_var_i64(value);
        }
        let bytes = w.into_inner();
        let mut r = NetworkReader::new(&bytes);
        for value in [i32::MIN, -65, -1, 0, 1, 64, i32::MAX] {
            assert_eq!(r.read_var_i32().unwrap(), value);
        }
        for value in [i64::MIN, -65, -1, 0, 1, 64, i64::MAX] {
            assert_eq!(r.read_var_i64().unwrap(), value);
        }
        assert!(r.is_empty());
    }

    #[test]
    fn decodes_schema_values_and_fixed_dictionary() {
        let schema = NetworkSchema {
            enums: Vec::new(),
            classes: Vec::new(),
            transmitted_property_count: 0,
            transmitted_event_count: 0,
            known_prefixes: vec![String::new()],
            fixed_dictionary: vec!["Name".into(), "Parent".into()],
        };

        let mut w = NetworkWriter::new();
        w.write_var_i32(-1234);
        w.write_f32(1.25);
        w.write_u8(0x81); // short fixed-dictionary id 1
        let bytes = w.into_inner();
        let mut r = NetworkReader::new(&bytes);

        assert_eq!(
            read_schema_value(
                &mut r,
                &SchemaType { value_format: value_format::INT, enum_id: NO_ENUM_ID },
                &schema,
                false,
            )
            .unwrap(),
            NetworkValue::Int(-1234)
        );
        assert_eq!(
            read_schema_value(
                &mut r,
                &SchemaType { value_format: value_format::FLOAT, enum_id: NO_ENUM_ID },
                &schema,
                false,
            )
            .unwrap(),
            NetworkValue::Float(1.25)
        );
        assert_eq!(
            read_schema_value(
                &mut r,
                &SchemaType {
                    value_format: value_format::STRING_FIXED_DICTIONARY,
                    enum_id: NO_ENUM_ID,
                },
                &schema,
                false,
            )
            .unwrap(),
            NetworkValue::FixedDictionaryString { network_id: 1, value: "Parent".into() }
        );
        assert!(r.is_empty());
    }

    #[test]
    fn parses_minimal_schema_exactly() {
        let mut w = NetworkWriter::new();
        // enums: one
        w.write_var_u32(1);
        w.write_var_string("Material").unwrap();
        w.write_u8(7);
        // one class, one total property, one total event
        w.write_var_u32(1);
        w.write_var_u32(1);
        w.write_var_u32(1);
        w.write_var_string("Part").unwrap();
        w.write_var_u32(1);
        w.write_var_string("Material").unwrap();
        w.write_u8(7);
        w.write_u16(0);
        w.write_u16(0xffff); // no special cframe property
        w.write_var_u32(1);
        w.write_var_string("Touched").unwrap();
        w.write_var_u32(1);
        w.write_u8(28);
        w.write_u16(0xffff);
        // known prefixes, then fixed dictionary
        w.write_var_u32(1);
        w.write_var_string("rbxassetid://").unwrap();
        w.write_var_u32(1);
        w.write_var_string("Name").unwrap();

        let schema = parse_uncompressed_schema(w.as_slice()).unwrap();
        assert_eq!(schema.enums[0].name, "Material");
        assert_eq!(schema.classes[0].name, "Part");
        assert_eq!(schema.classes[0].properties[0].ty.enum_id, 0);
        assert_eq!(schema.classes[0].events[0].name, "Touched");
        assert_eq!(schema.known_prefixes, vec!["", "rbxassetid://"]);
        assert_eq!(schema.fixed_dictionary, vec!["Name"]);

        let mut packet = vec![SCHEMA_PACKET_ID];
        packet.extend_from_slice(&compress_stream_frame(w.as_slice(), 3).unwrap());
        let from_packet = parse_schema_packet(&packet).unwrap();
        assert_eq!(from_packet, schema);
    }
}
