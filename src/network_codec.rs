//! Roblox 2022 network byte-stream and schema codecs.
//!
//! Derived directly from the decompiled `NetworkStream.c` and
//! `NetworkSchema.c` uploaded in `roblox-2022-network-codec-extras.zip`.
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
//! Schema definition packets start with byte 0x97, followed by a compressed
//! NetworkStream. `parse_uncompressed_schema` parses the exact payload AFTER
//! Replicator::decompressBitStream. That stream-compression function is not in
//! either uploaded archive (it is distinct from the transform `Compressor.c`);
//! guessing it would desynchronize and corrupt every later item.

use std::fmt;

const DEFAULT_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    UnexpectedEof { needed: usize, remaining: usize },
    LimitExceeded { wanted: usize, limit: usize },
    InvalidUtf8(String),
    InvalidVarint,
    InvalidData(String),
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

    pub fn write_var_string(&mut self, value: &str) -> Result<()> {
        let len = u32::try_from(value.len())
            .map_err(|_| CodecError::LimitExceeded { wanted: value.len(), limit: u32::MAX as usize })?;
        self.write_var_u32(len);
        self.write_bytes(value.as_bytes());
        Ok(())
    }
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

/// Inspect a complete schema packet. Validates/removes the confirmed 0x97
/// packet id, then stops explicitly at the missing compression codec.
pub fn inspect_schema_packet(packet: &[u8]) -> Result<()> {
    let Some((&id, compressed)) = packet.split_first() else {
        return Err(CodecError::UnexpectedEof { needed: 1, remaining: 0 });
    };
    if id != SCHEMA_PACKET_ID {
        return Err(CodecError::InvalidData(format!(
            "expected schema packet id 0x{SCHEMA_PACKET_ID:02x}, got 0x{id:02x}"
        )));
    }
    if compressed.is_empty() {
        return Err(CodecError::UnexpectedEof { needed: 1, remaining: 0 });
    }
    Err(CodecError::MissingCodec(
        "Replicator::decompressBitStream (archive did not include implementation)",
    ))
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
    }
}
