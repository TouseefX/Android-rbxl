//! Byte-exact codecs for Roblox's customized 2022 RakNet reliability layer.
//!
//! The layouts are taken from the uploaded `DatagramHeaderFormat.c`,
//! `ReliabilityLayer::CreateInternalPacketFromBitStream`,
//! `WriteToBitStreamFromInternalPacket`, and `BitStream.c` bodies. This module
//! is deliberately transport-only: callers must run SessionCrypto first when
//! RakNet encryption is negotiated, then pass the decrypted bytes here.

use crate::network_codec::{CodecError, Result};

const UINT24_MASK: u32 = 0x00ff_ffff;
const MAX_ACK_RANGES: usize = 4096;
const MAX_INTERNAL_PACKETS: usize = 65_536;
const MAX_SPLIT_PACKET_COUNT: u32 = 500_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DatagramFeatures {
    /// Congestion manager's `includeTimestampWithDatagrams()` result.
    pub include_timestamp: bool,
    /// Capability 0x8000. Adds the u16 trailing-padding count.
    pub avoid_packet_size: bool,
    /// Capability 0x200000. Adds the data-datagram JoinData bit.
    pub join_data_bit: bool,
    /// Capability 0x800000. Adds the retransmission bit.
    pub resent_bit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AckRange {
    pub min: u32,
    pub max: u32,
}

impl AckRange {
    pub fn single(value: u32) -> Self {
        Self { min: value & UINT24_MASK, max: value & UINT24_MASK }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketReliability {
    Unreliable = 0,
    UnreliableSequenced = 1,
    Reliable = 2,
    ReliableOrdered = 3,
    ReliableSequenced = 4,
    UnreliableWithAckReceipt = 5,
    ReliableWithAckReceipt = 6,
    ReliableOrderedWithAckReceipt = 7,
}

impl TryFrom<u8> for PacketReliability {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Unreliable),
            1 => Ok(Self::UnreliableSequenced),
            2 => Ok(Self::Reliable),
            3 => Ok(Self::ReliableOrdered),
            4 => Ok(Self::ReliableSequenced),
            5 => Ok(Self::UnreliableWithAckReceipt),
            6 => Ok(Self::ReliableWithAckReceipt),
            7 => Ok(Self::ReliableOrderedWithAckReceipt),
            _ => Err(CodecError::InvalidData(format!(
                "invalid RakNet reliability {value}"
            ))),
        }
    }
}

impl PacketReliability {
    fn wire_value(self) -> u8 {
        // `WriteToBitStreamFromInternalPacket` normalizes the three local
        // ACK-receipt variants before writing the three-bit field.
        match self {
            Self::UnreliableWithAckReceipt => Self::Unreliable as u8,
            Self::ReliableWithAckReceipt => Self::Reliable as u8,
            Self::ReliableOrderedWithAckReceipt => Self::ReliableOrdered as u8,
            other => other as u8,
        }
    }

    fn has_reliable_number_on_wire(self) -> bool {
        matches!(self.wire_value(), 2..=4)
    }

    fn has_sequencing_index_on_wire(self) -> bool {
        matches!(self.wire_value(), 1 | 4)
    }

    fn has_ordering_data_on_wire(self) -> bool {
        matches!(self.wire_value(), 1 | 3 | 4 | 7)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitPacketHeader {
    pub count: u32,
    pub id: u16,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalPacket {
    pub reliability: PacketReliability,
    /// Exact number of meaningful bits in `payload`.
    pub data_bit_length: u16,
    pub reliable_message_number: Option<u32>,
    pub sequencing_index: Option<u32>,
    pub ordering_index: Option<u32>,
    pub ordering_channel: Option<u8>,
    pub split: Option<SplitPacketHeader>,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDatagram {
    pub is_join_data: bool,
    pub is_resent: bool,
    pub is_continuous_send: bool,
    pub needs_b_and_as: bool,
    pub source_system_time: Option<u32>,
    pub datagram_number: u32,
    pub extra_padding: u16,
    pub packets: Vec<InternalPacket>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AckDatagram {
    pub has_b_and_as: bool,
    pub has_ack_timestamps: bool,
    pub source_system_time: Option<u32>,
    pub as_value: Option<u32>,
    pub extra_padding: u16,
    pub ranges: Vec<AckRange>,
    /// When `has_ack_timestamps` is set, ReliabilityLayer reads two u24s
    /// immediately after the range list.
    pub ack_timestamps: Option<[u32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NakDatagram {
    pub extra_padding: u16,
    pub ranges: Vec<AckRange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Datagram {
    Ack(AckDatagram),
    Nak(NakDatagram),
    Data(DataDatagram),
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
    end: usize,
}

impl<'a> ByteReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0, end: bytes.len() }
    }

    fn remaining(&self) -> usize {
        self.end.saturating_sub(self.offset)
    }

    fn read_u8(&mut self) -> Result<u8> {
        if self.remaining() < 1 {
            return Err(eof(1, self.remaining()));
        }
        let value = self.bytes[self.offset];
        self.offset += 1;
        Ok(value)
    }

    fn read_u16_be(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32_be(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_u24_le(&mut self) -> Result<u32> {
        let bytes = self.take(3)?;
        Ok(u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16))
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        if count > self.remaining() {
            return Err(eof(count, self.remaining()));
        }
        let start = self.offset;
        self.offset += count;
        Ok(&self.bytes[start..start + count])
    }

    fn remove_trailing_padding(&mut self, count: u16) -> Result<()> {
        let count = usize::from(count);
        if count > self.remaining() {
            return Err(CodecError::InvalidData(format!(
                "RakNet extra padding {count} exceeds {} remaining bytes",
                self.remaining()
            )));
        }
        self.end -= count;
    }
}

fn eof(needed: usize, remaining: usize) -> CodecError {
    CodecError::UnexpectedEof { needed, remaining }
}

fn push_u24_le(out: &mut Vec<u8>, value: u32) -> Result<()> {
    if value > UINT24_MASK {
        return Err(CodecError::InvalidData(format!(
            "RakNet uint24 value {value} exceeds 0xffffff"
        )));
    }
    out.extend_from_slice(&[
        value as u8,
        (value >> 8) as u8,
        (value >> 16) as u8,
    ]);
    Ok(())
}

fn parse_ranges(reader: &mut ByteReader<'_>) -> Result<Vec<AckRange>> {
    let count = reader.read_u16_be()? as usize;
    if count > MAX_ACK_RANGES {
        return Err(CodecError::LimitExceeded { wanted: count, limit: MAX_ACK_RANGES });
    }
    let mut ranges = Vec::with_capacity(count);
    for _ in 0..count {
        let single = reader.read_u8()? != 0;
        let min = reader.read_u24_le()?;
        let max = if single { min } else { reader.read_u24_le()? };
        if min > max {
            return Err(CodecError::InvalidData(format!(
                "RakNet ACK range minimum {min} exceeds maximum {max}"
            )));
        }
        ranges.push(AckRange { min, max });
    }
    Ok(ranges)
}

fn write_ranges(out: &mut Vec<u8>, ranges: &[AckRange]) -> Result<()> {
    let count = u16::try_from(ranges.len()).map_err(|_| CodecError::LimitExceeded {
        wanted: ranges.len(),
        limit: u16::MAX as usize,
    })?;
    out.extend_from_slice(&count.to_be_bytes());
    for range in ranges {
        if range.min > range.max || range.max > UINT24_MASK {
            return Err(CodecError::InvalidData(format!(
                "invalid RakNet ACK range {}..{}",
                range.min, range.max
            )));
        }
        out.push(u8::from(range.min == range.max));
        push_u24_le(out, range.min)?;
        if range.min != range.max {
            push_u24_le(out, range.max)?;
        }
    }
    Ok(())
}

fn parse_internal_packet(reader: &mut ByteReader<'_>) -> Result<InternalPacket> {
    let flags = reader.read_u8()?;
    let reliability = PacketReliability::try_from(flags >> 5)?;
    let is_split = flags & 0x10 != 0;
    let data_bit_length = reader.read_u16_be()?;
    if data_bit_length == 0 {
        return Err(CodecError::InvalidData(
            "RakNet internal packet has zero data-bit length".into(),
        ));
    }

    // The receiver checks numeric wire values 2..=4, not the local
    // ACK-receipt enum variants (which the sender normalizes).
    let reliable_message_number = if matches!(reliability as u8, 2..=4) {
        Some(reader.read_u24_le()?)
    } else {
        None
    };
    let sequencing_index = if matches!(reliability as u8, 1 | 4) {
        Some(reader.read_u24_le()?)
    } else {
        None
    };
    let (ordering_index, ordering_channel) = if matches!(reliability as u8, 1 | 3 | 4 | 7) {
        let index = reader.read_u24_le()?;
        let channel = reader.read_u8()?;
        if channel >= 32 {
            return Err(CodecError::InvalidData(format!(
                "RakNet ordering channel {channel} is outside 0..31"
            )));
        }
        (Some(index), Some(channel))
    } else {
        (None, None)
    };
    let split = if is_split {
        let split = SplitPacketHeader {
            count: reader.read_u32_be()?,
            id: reader.read_u16_be()?,
            index: reader.read_u32_be()?,
        };
        if split.count == 0 || split.count > MAX_SPLIT_PACKET_COUNT || split.index >= split.count {
            return Err(CodecError::InvalidData(format!(
                "invalid RakNet split header: index {}, count {}",
                split.index, split.count
            )));
        }
        Some(split)
    } else {
        None
    };
    let payload_size = (usize::from(data_bit_length) + 7) / 8;
    let payload = reader.take(payload_size)?.to_vec();

    Ok(InternalPacket {
        reliability,
        data_bit_length,
        reliable_message_number,
        sequencing_index,
        ordering_index,
        ordering_channel,
        split,
        payload,
    })
}

/// Parse one decrypted 2022 RakNet datagram. `features` must be the negotiated
/// common-capability/congestion settings; optional header bits cannot be
/// inferred safely from the packet itself.
pub fn parse_datagram(bytes: &[u8], features: DatagramFeatures) -> Result<Datagram> {
    let mut reader = ByteReader::new(bytes);
    let flags = reader.read_u8()?;
    if flags & 0x80 == 0 {
        return Err(CodecError::InvalidData("RakNet datagram valid bit is clear".into()));
    }

    let is_ack = flags & 0x40 != 0;
    if is_ack {
        let has_b_and_as = flags & 0x20 != 0;
        let has_ack_timestamps = flags & 0x10 != 0;
        let source_system_time = if features.include_timestamp {
            Some(reader.read_u32_be()?)
        } else {
            None
        };
        let as_value = if has_b_and_as {
            Some(reader.read_u32_be()?)
        } else {
            None
        };
        let extra_padding = if features.avoid_packet_size {
            reader.read_u16_be()?
        } else {
            0
        };
        reader.remove_trailing_padding(extra_padding)?;
        let ranges = parse_ranges(&mut reader)?;
        let ack_timestamps = if has_ack_timestamps {
            Some([reader.read_u24_le()?, reader.read_u24_le()?])
        } else {
            None
        };
        if reader.remaining() != 0 {
            return Err(CodecError::InvalidData(format!(
                "{} trailing byte(s) after RakNet ACK",
                reader.remaining()
            )));
        }
        return Ok(Datagram::Ack(AckDatagram {
            has_b_and_as,
            has_ack_timestamps,
            source_system_time,
            as_value,
            extra_padding,
            ranges,
            ack_timestamps,
        }));
    }

    let is_nak = flags & 0x20 != 0;
    if is_nak {
        let extra_padding = if features.avoid_packet_size {
            reader.read_u16_be()?
        } else {
            0
        };
        reader.remove_trailing_padding(extra_padding)?;
        let ranges = parse_ranges(&mut reader)?;
        if reader.remaining() != 0 {
            return Err(CodecError::InvalidData(format!(
                "{} trailing byte(s) after RakNet NAK",
                reader.remaining()
            )));
        }
        return Ok(Datagram::Nak(NakDatagram { extra_padding, ranges }));
    }

    let mut mask = 0x10_u8;
    let is_join_data = if features.join_data_bit {
        let value = flags & mask != 0;
        mask >>= 1;
        value
    } else {
        false
    };
    let is_resent = if features.resent_bit {
        let value = flags & mask != 0;
        mask >>= 1;
        value
    } else {
        false
    };
    // Packet-pair bit is present but hard-coded to zero by Serialize.
    if flags & mask != 0 {
        return Err(CodecError::InvalidData(
            "unsupported RakNet packet-pair bit is set".into(),
        ));
    }
    mask >>= 1;
    let is_continuous_send = flags & mask != 0;
    mask >>= 1;
    let needs_b_and_as = flags & mask != 0;

    let source_system_time = if features.include_timestamp {
        Some(reader.read_u32_be()?)
    } else {
        None
    };
    let datagram_number = reader.read_u24_le()?;
    let extra_padding = if features.avoid_packet_size {
        reader.read_u16_be()?
    } else {
        0
    };
    reader.remove_trailing_padding(extra_padding)?;

    let mut packets = Vec::new();
    while reader.remaining() != 0 {
        if packets.len() == MAX_INTERNAL_PACKETS {
            return Err(CodecError::LimitExceeded {
                wanted: packets.len() + 1,
                limit: MAX_INTERNAL_PACKETS,
            });
        }
        packets.push(parse_internal_packet(&mut reader)?);
    }
    Ok(Datagram::Data(DataDatagram {
        is_join_data,
        is_resent,
        is_continuous_send,
        needs_b_and_as,
        source_system_time,
        datagram_number,
        extra_padding,
        packets,
    }))
}

fn encode_internal_packet(out: &mut Vec<u8>, packet: &InternalPacket) -> Result<()> {
    if packet.data_bit_length == 0 {
        return Err(CodecError::InvalidData(
            "RakNet internal packet has zero data-bit length".into(),
        ));
    }
    let expected_payload = (usize::from(packet.data_bit_length) + 7) / 8;
    if packet.payload.len() != expected_payload {
        return Err(CodecError::InvalidData(format!(
            "RakNet payload has {} bytes but {} bits require {expected_payload}",
            packet.payload.len(), packet.data_bit_length
        )));
    }
    let wire_reliability = packet.reliability.wire_value();
    out.push((wire_reliability << 5) | if packet.split.is_some() { 0x10 } else { 0 });
    out.extend_from_slice(&packet.data_bit_length.to_be_bytes());

    if packet.reliability.has_reliable_number_on_wire() {
        push_u24_le(
            out,
            packet.reliable_message_number.ok_or_else(|| {
                CodecError::InvalidData("reliable RakNet packet is missing message number".into())
            })?,
        )?;
    }
    if packet.reliability.has_sequencing_index_on_wire() {
        push_u24_le(
            out,
            packet.sequencing_index.ok_or_else(|| {
                CodecError::InvalidData("sequenced RakNet packet is missing sequence index".into())
            })?,
        )?;
    }
    if packet.reliability.has_ordering_data_on_wire() {
        push_u24_le(
            out,
            packet.ordering_index.ok_or_else(|| {
                CodecError::InvalidData("ordered RakNet packet is missing ordering index".into())
            })?,
        )?;
        let channel = packet.ordering_channel.ok_or_else(|| {
            CodecError::InvalidData("ordered RakNet packet is missing ordering channel".into())
        })?;
        if channel >= 32 {
            return Err(CodecError::InvalidData(format!(
                "RakNet ordering channel {channel} is outside 0..31"
            )));
        }
        out.push(channel);
    }
    if let Some(split) = packet.split {
        if split.count == 0 || split.count > MAX_SPLIT_PACKET_COUNT || split.index >= split.count {
            return Err(CodecError::InvalidData(format!(
                "invalid RakNet split header: index {}, count {}",
                split.index, split.count
            )));
        }
        out.extend_from_slice(&split.count.to_be_bytes());
        out.extend_from_slice(&split.id.to_be_bytes());
        out.extend_from_slice(&split.index.to_be_bytes());
    }
    out.extend_from_slice(&packet.payload);
    Ok(())
}

fn append_padding(out: &mut Vec<u8>, count: u16) {
    out.resize(out.len() + usize::from(count), 0);
}

pub fn encode_data_datagram(data: &DataDatagram, features: DatagramFeatures) -> Result<Vec<u8>> {
    if data.datagram_number > UINT24_MASK {
        return Err(CodecError::InvalidData(format!(
            "RakNet datagram number {} exceeds 0xffffff",
            data.datagram_number
        )));
    }
    if !features.avoid_packet_size && data.extra_padding != 0 {
        return Err(CodecError::InvalidData(
            "RakNet extra padding requires avoid-packet-size capability".into(),
        ));
    }
    let mut flags = 0x80_u8;
    let mut mask = 0x10_u8;
    if features.join_data_bit {
        if data.is_join_data {
            flags |= mask;
        }
        mask >>= 1;
    } else if data.is_join_data {
        return Err(CodecError::InvalidData(
            "JoinData bit requested without negotiated capability".into(),
        ));
    }
    if features.resent_bit {
        if data.is_resent {
            flags |= mask;
        }
        mask >>= 1;
    } else if data.is_resent {
        return Err(CodecError::InvalidData(
            "resent bit requested without negotiated capability".into(),
        ));
    }
    mask >>= 1; // packet pair, always zero
    if data.is_continuous_send {
        flags |= mask;
    }
    mask >>= 1;
    if data.needs_b_and_as {
        flags |= mask;
    }

    let mut out = vec![flags];
    if features.include_timestamp {
        out.extend_from_slice(
            &data.source_system_time.ok_or_else(|| {
                CodecError::InvalidData("timestamped RakNet datagram is missing source time".into())
            })?
            .to_be_bytes(),
        );
    }
    push_u24_le(&mut out, data.datagram_number)?;
    if features.avoid_packet_size {
        out.extend_from_slice(&data.extra_padding.to_be_bytes());
    }
    for packet in &data.packets {
        encode_internal_packet(&mut out, packet)?;
    }
    append_padding(&mut out, data.extra_padding);
    Ok(out)
}

pub fn encode_ack_datagram(ack: &AckDatagram, features: DatagramFeatures) -> Result<Vec<u8>> {
    if !features.avoid_packet_size && ack.extra_padding != 0 {
        return Err(CodecError::InvalidData(
            "RakNet extra padding requires avoid-packet-size capability".into(),
        ));
    }
    let mut flags = 0xc0_u8;
    if ack.has_b_and_as {
        flags |= 0x20;
    }
    if ack.has_ack_timestamps {
        flags |= 0x10;
    }
    let mut out = vec![flags];
    if features.include_timestamp {
        out.extend_from_slice(
            &ack.source_system_time.ok_or_else(|| {
                CodecError::InvalidData("timestamped RakNet ACK is missing source time".into())
            })?
            .to_be_bytes(),
        );
    }
    if ack.has_b_and_as {
        out.extend_from_slice(
            &ack.as_value.ok_or_else(|| {
                CodecError::InvalidData("RakNet ACK has B/AS flag but no AS value".into())
            })?
            .to_be_bytes(),
        );
    }
    if features.avoid_packet_size {
        out.extend_from_slice(&ack.extra_padding.to_be_bytes());
    }
    write_ranges(&mut out, &ack.ranges)?;
    if ack.has_ack_timestamps {
        let timestamps = ack.ack_timestamps.ok_or_else(|| {
            CodecError::InvalidData("RakNet ACK timestamp flag has no timestamp pair".into())
        })?;
        push_u24_le(&mut out, timestamps[0])?;
        push_u24_le(&mut out, timestamps[1])?;
    }
    append_padding(&mut out, ack.extra_padding);
    Ok(out)
}

pub fn encode_nak_datagram(nak: &NakDatagram, features: DatagramFeatures) -> Result<Vec<u8>> {
    if !features.avoid_packet_size && nak.extra_padding != 0 {
        return Err(CodecError::InvalidData(
            "RakNet extra padding requires avoid-packet-size capability".into(),
        ));
    }
    let mut out = vec![0xa0];
    if features.avoid_packet_size {
        out.extend_from_slice(&nak.extra_padding.to_be_bytes());
    }
    write_ranges(&mut out, &nak.ranges)?;
    append_padding(&mut out, nak.extra_padding);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features() -> DatagramFeatures {
        DatagramFeatures {
            include_timestamp: true,
            avoid_packet_size: true,
            join_data_bit: true,
            resent_bit: true,
        }
    }

    #[test]
    fn reliable_ordered_data_round_trip() {
        let datagram = DataDatagram {
            is_join_data: true,
            is_resent: false,
            is_continuous_send: false,
            needs_b_and_as: true,
            source_system_time: Some(0x1020_3040),
            datagram_number: 0x01_0203,
            extra_padding: 3,
            packets: vec![InternalPacket {
                reliability: PacketReliability::ReliableOrdered,
                data_bit_length: 24,
                reliable_message_number: Some(0x04_0506),
                sequencing_index: None,
                ordering_index: Some(0x07_0809),
                ordering_channel: Some(2),
                split: None,
                payload: vec![0x83, 22, 3],
            }],
        };
        let encoded = encode_data_datagram(&datagram, features()).unwrap();
        assert_eq!(encoded[0], 0x91); // valid + join + needs B/AS
        assert_eq!(parse_datagram(&encoded, features()).unwrap(), Datagram::Data(datagram));
    }

    #[test]
    fn split_packet_uses_be_sizes_and_le_uint24s() {
        let packet = InternalPacket {
            reliability: PacketReliability::Reliable,
            data_bit_length: 8,
            reliable_message_number: Some(0x01_0203),
            sequencing_index: None,
            ordering_index: None,
            ordering_channel: None,
            split: Some(SplitPacketHeader { count: 2, id: 0x1122, index: 1 }),
            payload: vec![0x97],
        };
        let data = DataDatagram {
            is_join_data: false,
            is_resent: false,
            is_continuous_send: false,
            needs_b_and_as: false,
            source_system_time: None,
            datagram_number: 0x0a_0b0c,
            extra_padding: 0,
            packets: vec![packet],
        };
        let encoded = encode_data_datagram(&data, DatagramFeatures::default()).unwrap();
        assert_eq!(&encoded[1..4], &[0x0c, 0x0b, 0x0a]);
        assert_eq!(&encoded[4..7], &[0x50, 0x00, 0x08]);
        assert_eq!(&encoded[7..10], &[0x03, 0x02, 0x01]);
        assert_eq!(parse_datagram(&encoded, DatagramFeatures::default()).unwrap(), Datagram::Data(data));
    }

    #[test]
    fn ack_and_nak_round_trip() {
        let ack = AckDatagram {
            has_b_and_as: true,
            has_ack_timestamps: true,
            source_system_time: Some(123),
            as_value: Some(456),
            extra_padding: 2,
            ranges: vec![AckRange::single(7), AckRange { min: 10, max: 15 }],
            ack_timestamps: Some([20, 30]),
        };
        let encoded = encode_ack_datagram(&ack, features()).unwrap();
        assert_eq!(parse_datagram(&encoded, features()).unwrap(), Datagram::Ack(ack));

        let nak = NakDatagram {
            extra_padding: 0,
            ranges: vec![AckRange { min: 100, max: 103 }],
        };
        let encoded = encode_nak_datagram(&nak, DatagramFeatures::default()).unwrap();
        assert_eq!(parse_datagram(&encoded, DatagramFeatures::default()).unwrap(), Datagram::Nak(nak));
    }
}
