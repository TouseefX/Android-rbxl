//! Clean-room reconstruction of the target's payload-bearing `0x80` Mrd helper.

pub const TARGET_MAX_MRD_PAYLOAD_BYTES: usize = 0x10000;
pub const TARGET_MAX_HUFFMAN_OUTPUT_BYTES: usize = 0x20000;
pub const TARGET_MAX_MRD_RECORD_BYTES: usize = 0x20000;
pub const TARGET_MAX_MRD_FIELD_VALUE: u64 = 0x3fffffffffffffff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mrd80CodecLimits {
    pub max_payload_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_record_bytes: usize,
}

impl Default for Mrd80CodecLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: TARGET_MAX_MRD_PAYLOAD_BYTES,
            max_decoded_bytes: TARGET_MAX_HUFFMAN_OUTPUT_BYTES,
            max_record_bytes: TARGET_MAX_MRD_RECORD_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MrdWireError {
    None,
    InvalidArgument,
    NeedMoreData,
    InvalidTagFamily,
    FieldValueOverflow,
    PayloadLimitExceeded,
    RecordLimitExceeded,
    InvalidHuffmanData,
    DecodedPayloadLimitExceeded,
}

impl MrdWireError {
    pub fn ok(self) -> bool {
        matches!(self, Self::None)
    }
}

pub fn mrd_wire_error_str(err: MrdWireError) -> &'static str {
    match err {
        MrdWireError::None => "None",
        MrdWireError::InvalidArgument => "InvalidArgument",
        MrdWireError::NeedMoreData => "NeedMoreData",
        MrdWireError::InvalidTagFamily => "InvalidTagFamily",
        MrdWireError::FieldValueOverflow => "FieldValueOverflow",
        MrdWireError::PayloadLimitExceeded => "PayloadLimitExceeded",
        MrdWireError::RecordLimitExceeded => "RecordLimitExceeded",
        MrdWireError::InvalidHuffmanData => "InvalidHuffmanData",
        MrdWireError::DecodedPayloadLimitExceeded => "DecodedPayloadLimitExceeded",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mrd80Record {
    pub back_reference_delta: u64,
    pub huffman_compressed: bool,
    pub payload: Vec<u8>,
    pub bytes_consumed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrdEncodeResult {
    pub bytes: Vec<u8>,
    pub error: MrdWireError,
}

impl MrdEncodeResult {
    pub fn ok(&self) -> bool {
        self.error.ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrdDecodeResult {
    pub record: Mrd80Record,
    pub error: MrdWireError,
}

impl MrdDecodeResult {
    pub fn ok(&self) -> bool {
        self.error.ok()
    }
}

const K_TARGET_HUFFMAN_CODES: [(u32, u32); 256] = [
    (13u32, 0xffc00000u32),
    (23u32, 0xffffb000u32),
    (28u32, 0xfffffe20u32),
    (28u32, 0xfffffe30u32),
    (28u32, 0xfffffe40u32),
    (28u32, 0xfffffe50u32),
    (28u32, 0xfffffe60u32),
    (28u32, 0xfffffe70u32),
    (28u32, 0xfffffe80u32),
    (24u32, 0xffffea00u32),
    (30u32, 0xfffffff0u32),
    (28u32, 0xfffffe90u32),
    (28u32, 0xfffffea0u32),
    (30u32, 0xfffffff4u32),
    (28u32, 0xfffffeb0u32),
    (28u32, 0xfffffec0u32),
    (28u32, 0xfffffed0u32),
    (28u32, 0xfffffee0u32),
    (28u32, 0xfffffef0u32),
    (28u32, 0xffffff00u32),
    (28u32, 0xffffff10u32),
    (28u32, 0xffffff20u32),
    (30u32, 0xfffffff8u32),
    (28u32, 0xffffff30u32),
    (28u32, 0xffffff40u32),
    (28u32, 0xffffff50u32),
    (28u32, 0xffffff60u32),
    (28u32, 0xffffff70u32),
    (28u32, 0xffffff80u32),
    (28u32, 0xffffff90u32),
    (28u32, 0xffffffa0u32),
    (28u32, 0xffffffb0u32),
    (6u32, 0x50000000u32),
    (10u32, 0xfe000000u32),
    (10u32, 0xfe400000u32),
    (12u32, 0xffa00000u32),
    (13u32, 0xffc80000u32),
    (6u32, 0x54000000u32),
    (8u32, 0xf8000000u32),
    (11u32, 0xff400000u32),
    (10u32, 0xfe800000u32),
    (10u32, 0xfec00000u32),
    (8u32, 0xf9000000u32),
    (11u32, 0xff600000u32),
    (8u32, 0xfa000000u32),
    (6u32, 0x58000000u32),
    (6u32, 0x5c000000u32),
    (6u32, 0x60000000u32),
    (5u32, 0x00000000u32),
    (5u32, 0x08000000u32),
    (5u32, 0x10000000u32),
    (6u32, 0x64000000u32),
    (6u32, 0x68000000u32),
    (6u32, 0x6c000000u32),
    (6u32, 0x70000000u32),
    (6u32, 0x74000000u32),
    (6u32, 0x78000000u32),
    (6u32, 0x7c000000u32),
    (7u32, 0xb8000000u32),
    (8u32, 0xfb000000u32),
    (15u32, 0xfff80000u32),
    (6u32, 0x80000000u32),
    (12u32, 0xffb00000u32),
    (10u32, 0xff000000u32),
    (13u32, 0xffd00000u32),
    (6u32, 0x84000000u32),
    (7u32, 0xba000000u32),
    (7u32, 0xbc000000u32),
    (7u32, 0xbe000000u32),
    (7u32, 0xc0000000u32),
    (7u32, 0xc2000000u32),
    (7u32, 0xc4000000u32),
    (7u32, 0xc6000000u32),
    (7u32, 0xc8000000u32),
    (7u32, 0xca000000u32),
    (7u32, 0xcc000000u32),
    (7u32, 0xce000000u32),
    (7u32, 0xd0000000u32),
    (7u32, 0xd2000000u32),
    (7u32, 0xd4000000u32),
    (7u32, 0xd6000000u32),
    (7u32, 0xd8000000u32),
    (7u32, 0xda000000u32),
    (7u32, 0xdc000000u32),
    (7u32, 0xde000000u32),
    (7u32, 0xe0000000u32),
    (7u32, 0xe2000000u32),
    (7u32, 0xe4000000u32),
    (8u32, 0xfc000000u32),
    (7u32, 0xe6000000u32),
    (8u32, 0xfd000000u32),
    (13u32, 0xffd80000u32),
    (19u32, 0xfffe0000u32),
    (13u32, 0xffe00000u32),
    (14u32, 0xfff00000u32),
    (6u32, 0x88000000u32),
    (15u32, 0xfffa0000u32),
    (5u32, 0x18000000u32),
    (6u32, 0x8c000000u32),
    (5u32, 0x20000000u32),
    (6u32, 0x90000000u32),
    (5u32, 0x28000000u32),
    (6u32, 0x94000000u32),
    (6u32, 0x98000000u32),
    (6u32, 0x9c000000u32),
    (5u32, 0x30000000u32),
    (7u32, 0xe8000000u32),
    (7u32, 0xea000000u32),
    (6u32, 0xa0000000u32),
    (6u32, 0xa4000000u32),
    (6u32, 0xa8000000u32),
    (5u32, 0x38000000u32),
    (6u32, 0xac000000u32),
    (7u32, 0xec000000u32),
    (6u32, 0xb0000000u32),
    (5u32, 0x40000000u32),
    (5u32, 0x48000000u32),
    (6u32, 0xb4000000u32),
    (7u32, 0xee000000u32),
    (7u32, 0xf0000000u32),
    (7u32, 0xf2000000u32),
    (7u32, 0xf4000000u32),
    (7u32, 0xf6000000u32),
    (15u32, 0xfffc0000u32),
    (11u32, 0xff800000u32),
    (14u32, 0xfff40000u32),
    (13u32, 0xffe80000u32),
    (28u32, 0xffffffc0u32),
    (20u32, 0xfffe6000u32),
    (22u32, 0xffff4800u32),
    (20u32, 0xfffe7000u32),
    (20u32, 0xfffe8000u32),
    (22u32, 0xffff4c00u32),
    (22u32, 0xffff5000u32),
    (22u32, 0xffff5400u32),
    (23u32, 0xffffb200u32),
    (22u32, 0xffff5800u32),
    (23u32, 0xffffb400u32),
    (23u32, 0xffffb600u32),
    (23u32, 0xffffb800u32),
    (23u32, 0xffffba00u32),
    (23u32, 0xffffbc00u32),
    (24u32, 0xffffeb00u32),
    (23u32, 0xffffbe00u32),
    (24u32, 0xffffec00u32),
    (24u32, 0xffffed00u32),
    (22u32, 0xffff5c00u32),
    (23u32, 0xffffc000u32),
    (24u32, 0xffffee00u32),
    (23u32, 0xffffc200u32),
    (23u32, 0xffffc400u32),
    (23u32, 0xffffc600u32),
    (23u32, 0xffffc800u32),
    (21u32, 0xfffee000u32),
    (22u32, 0xffff6000u32),
    (23u32, 0xffffca00u32),
    (22u32, 0xffff6400u32),
    (23u32, 0xffffcc00u32),
    (23u32, 0xffffce00u32),
    (24u32, 0xffffef00u32),
    (22u32, 0xffff6800u32),
    (21u32, 0xfffee800u32),
    (20u32, 0xfffe9000u32),
    (22u32, 0xffff6c00u32),
    (22u32, 0xffff7000u32),
    (23u32, 0xffffd000u32),
    (23u32, 0xffffd200u32),
    (21u32, 0xfffef000u32),
    (23u32, 0xffffd400u32),
    (22u32, 0xffff7400u32),
    (22u32, 0xffff7800u32),
    (24u32, 0xfffff000u32),
    (21u32, 0xfffef800u32),
    (22u32, 0xffff7c00u32),
    (23u32, 0xffffd600u32),
    (23u32, 0xffffd800u32),
    (21u32, 0xffff0000u32),
    (21u32, 0xffff0800u32),
    (22u32, 0xffff8000u32),
    (21u32, 0xffff1000u32),
    (23u32, 0xffffda00u32),
    (22u32, 0xffff8400u32),
    (23u32, 0xffffdc00u32),
    (23u32, 0xffffde00u32),
    (20u32, 0xfffea000u32),
    (22u32, 0xffff8800u32),
    (22u32, 0xffff8c00u32),
    (22u32, 0xffff9000u32),
    (23u32, 0xffffe000u32),
    (22u32, 0xffff9400u32),
    (22u32, 0xffff9800u32),
    (23u32, 0xffffe200u32),
    (26u32, 0xfffff800u32),
    (26u32, 0xfffff840u32),
    (20u32, 0xfffeb000u32),
    (19u32, 0xfffe2000u32),
    (22u32, 0xffff9c00u32),
    (23u32, 0xffffe400u32),
    (22u32, 0xffffa000u32),
    (25u32, 0xfffff600u32),
    (26u32, 0xfffff880u32),
    (26u32, 0xfffff8c0u32),
    (26u32, 0xfffff900u32),
    (27u32, 0xfffffbc0u32),
    (27u32, 0xfffffbe0u32),
    (26u32, 0xfffff940u32),
    (24u32, 0xfffff100u32),
    (25u32, 0xfffff680u32),
    (19u32, 0xfffe4000u32),
    (21u32, 0xffff1800u32),
    (26u32, 0xfffff980u32),
    (27u32, 0xfffffc00u32),
    (27u32, 0xfffffc20u32),
    (26u32, 0xfffff9c0u32),
    (27u32, 0xfffffc40u32),
    (24u32, 0xfffff200u32),
    (21u32, 0xffff2000u32),
    (21u32, 0xffff2800u32),
    (26u32, 0xfffffa00u32),
    (26u32, 0xfffffa40u32),
    (28u32, 0xffffffd0u32),
    (27u32, 0xfffffc60u32),
    (27u32, 0xfffffc80u32),
    (27u32, 0xfffffca0u32),
    (20u32, 0xfffec000u32),
    (24u32, 0xfffff300u32),
    (20u32, 0xfffed000u32),
    (21u32, 0xffff3000u32),
    (22u32, 0xffffa400u32),
    (21u32, 0xffff3800u32),
    (21u32, 0xffff4000u32),
    (23u32, 0xffffe600u32),
    (22u32, 0xffffa800u32),
    (22u32, 0xffffac00u32),
    (25u32, 0xfffff700u32),
    (25u32, 0xfffff780u32),
    (24u32, 0xfffff400u32),
    (24u32, 0xfffff500u32),
    (26u32, 0xfffffa80u32),
    (23u32, 0xffffe800u32),
    (26u32, 0xfffffac0u32),
    (27u32, 0xfffffcc0u32),
    (26u32, 0xfffffb00u32),
    (26u32, 0xfffffb40u32),
    (27u32, 0xfffffce0u32),
    (27u32, 0xfffffd00u32),
    (27u32, 0xfffffd20u32),
    (27u32, 0xfffffd40u32),
    (27u32, 0xfffffd60u32),
    (28u32, 0xffffffe0u32),
    (27u32, 0xfffffd80u32),
    (27u32, 0xfffffda0u32),
    (27u32, 0xfffffdc0u32),
    (27u32, 0xfffffde0u32),
    (27u32, 0xfffffe00u32),
    (26u32, 0xfffffb80u32),
];

const MAX_HUFFMAN_CODE_LENGTH: usize = 30;

fn bounded_limits(limits: &Mrd80CodecLimits) -> Mrd80CodecLimits {
    Mrd80CodecLimits {
        max_payload_bytes: limits.max_payload_bytes.min(TARGET_MAX_MRD_PAYLOAD_BYTES),
        max_decoded_bytes: limits.max_decoded_bytes.min(TARGET_MAX_HUFFMAN_OUTPUT_BYTES),
        max_record_bytes: limits.max_record_bytes.min(TARGET_MAX_MRD_RECORD_BYTES),
    }
}

fn field_mask(width: u32) -> u64 {
    (1u64 << width) - 1
}

fn uleb128_size(value: u64) -> usize {
    let mut size = 1usize;
    let mut v = value;
    while v >= 0x80 {
        v >>= 7;
        size += 1;
    }
    size
}

fn bounded_field_size(value: u64, width: u32) -> usize {
    let mask = field_mask(width);
    1 + if value < mask { 0 } else { uleb128_size(value - mask) }
}

fn append_bounded_field(output: &mut Vec<u8>, prefix: u8, value: u64, width: u32) {
    let mask = field_mask(width) as u8;
    let first_index = output.len();
    output.push(prefix);
    let preserved = prefix & !mask;
    if value < u64::from(mask) {
        output[first_index] = preserved | (value as u8);
        return;
    }
    output[first_index] = preserved | mask;
    let mut remainder = value - u64::from(mask);
    while remainder >= 0x80 {
        output.push(((remainder & 0x7f) | 0x80) as u8);
        remainder >>= 7;
    }
    output.push(remainder as u8);
}

fn read_bounded_field(
    data: &[u8],
    size: usize,
    max_record_bytes: usize,
    offset: &mut usize,
    width: u32,
    value: &mut u64,
) -> MrdWireError {
    if *offset >= max_record_bytes {
        return MrdWireError::RecordLimitExceeded;
    }
    if *offset >= size {
        return MrdWireError::NeedMoreData;
    }
    let mask = field_mask(width);
    let first = u64::from(data[*offset] & mask as u8);
    *offset += 1;
    *value = first;
    if first != mask {
        return MrdWireError::None;
    }
    let mut shift = 0u32;
    loop {
        if *offset >= max_record_bytes {
            return MrdWireError::RecordLimitExceeded;
        }
        if *offset >= size {
            return MrdWireError::NeedMoreData;
        }
        let group_byte = data[*offset];
        *offset += 1;
        let group = u64::from(group_byte & 0x7f);
        if shift >= 63 || group > ((TARGET_MAX_MRD_FIELD_VALUE - *value) >> shift) {
            return MrdWireError::FieldValueOverflow;
        }
        *value += group << shift;
        if (group_byte & 0x80) == 0 {
            return MrdWireError::None;
        }
        if shift >= 56 {
            return MrdWireError::FieldValueOverflow;
        }
        shift += 7;
    }
}

fn huffman_bit_count(input: &[u8]) -> u64 {
    let mut bits = 0u64;
    for &b in input {
        bits += u64::from(K_TARGET_HUFFMAN_CODES[b as usize].0);
    }
    bits
}

fn encode_huffman(input: &[u8], output: &mut Vec<u8>) {
    let bit_count = huffman_bit_count(input);
    output.reserve(((bit_count + 7) / 8) as usize);
    let mut current = 0u8;
    let mut used_bits = 0u32;
    for &sym in input {
        let (bit_length, left_aligned) = K_TARGET_HUFFMAN_CODES[sym as usize];
        for bit_index in 0..bit_length {
            current = ((u32::from(current) << 1) | ((left_aligned >> (31 - bit_index)) & 1)) as u8;
            used_bits += 1;
            if used_bits == 8 {
                output.push(current);
                current = 0;
                used_bits = 0;
            }
        }
    }
    if used_bits != 0 {
        let padding_bits = 8 - used_bits;
        current = (((u32::from(current) << padding_bits) | ((1u32 << padding_bits) - 1)) as u8);
        output.push(current);
    }
}

fn decode_huffman(input: &[u8], output_limit: usize, output: &mut Vec<u8>) -> MrdWireError {
    #[derive(Clone, Copy)]
    struct Node {
        child: [i32; 2],
        symbol: i32,
    }
    impl Default for Node {
        fn default() -> Self {
            Self { child: [-1, -1], symbol: -1 }
        }
    }
    let mut nodes = vec![Node::default(); 1 + 256 * MAX_HUFFMAN_CODE_LENGTH];
    let mut node_count = 1usize;
    for symbol in 0..256usize {
        let (bit_length, left_aligned) = K_TARGET_HUFFMAN_CODES[symbol];
        if bit_length == 0 || bit_length as usize > MAX_HUFFMAN_CODE_LENGTH {
            return MrdWireError::InvalidHuffmanData;
        }
        let mut node = 0usize;
        for bit_index in 0..bit_length {
            let n = &mut nodes[node];
            if n.symbol >= 0 {
                return MrdWireError::InvalidHuffmanData;
            }
            let shift = 31u32 - bit_index;
            let bit = ((left_aligned >> shift) & 1) as usize;
            let mut next = n.child[bit];
            if next < 0 {
                if node_count >= nodes.len() {
                    return MrdWireError::InvalidHuffmanData;
                }
                next = node_count as i32;
                node_count += 1;
                nodes[node].child[bit] = next;
            }
            node = next as usize;
        }
        let leaf = &mut nodes[node];
        if leaf.symbol >= 0 || leaf.child[0] >= 0 || leaf.child[1] >= 0 {
            return MrdWireError::InvalidHuffmanData;
        }
        leaf.symbol = symbol as i32;
    }

    output.clear();
    output.reserve(output_limit.min(input.len().saturating_mul(2)));
    let mut node = 0usize;
    let mut unfinished_bits = 0usize;
    let mut unfinished_are_ones = true;

    for &byte in input {
        for bit_index in (0..8).rev() {
            let bit = ((byte >> bit_index) & 1) as usize;
            let next = nodes[node].child[bit];
            if next < 0 {
                return MrdWireError::InvalidHuffmanData;
            }
            node = next as usize;
            let symbol = nodes[node].symbol;
            if symbol >= 0 {
                if output.len() >= output_limit {
                    return MrdWireError::DecodedPayloadLimitExceeded;
                }
                output.push(symbol as u8);
                node = 0;
                unfinished_bits = 0;
                unfinished_are_ones = true;
            } else {
                unfinished_bits += 1;
                unfinished_are_ones = unfinished_are_ones && (bit != 0);
            }
        }
    }

    if node != 0 && !(unfinished_bits <= 7 && unfinished_are_ones) {
        return MrdWireError::InvalidHuffmanData;
    }
    MrdWireError::None
}

pub fn encode_mrd80_record(
    back_reference_delta: u64,
    payload: &[u8],
    limits: &Mrd80CodecLimits,
) -> MrdEncodeResult {
    let limits = bounded_limits(limits);
    if back_reference_delta > TARGET_MAX_MRD_FIELD_VALUE {
        return MrdEncodeResult { bytes: Vec::new(), error: MrdWireError::FieldValueOverflow };
    }
    if payload.len() > limits.max_payload_bytes {
        return MrdEncodeResult { bytes: Vec::new(), error: MrdWireError::PayloadLimitExceeded };
    }
    let compressed_bits = huffman_bit_count(payload);
    let compressed_size = ((compressed_bits + 7) / 8) as usize;
    let use_huffman = compressed_size < payload.len();
    let stored_size = if use_huffman { compressed_size } else { payload.len() };

    let header_size = bounded_field_size(back_reference_delta, 6)
        + bounded_field_size(stored_size as u64, 7);
    if header_size > limits.max_record_bytes || stored_size > limits.max_record_bytes - header_size {
        return MrdEncodeResult { bytes: Vec::new(), error: MrdWireError::RecordLimitExceeded };
    }

    let mut bytes = Vec::with_capacity(header_size + stored_size);
    append_bounded_field(&mut bytes, 0x80, back_reference_delta, 6);
    append_bounded_field(&mut bytes, if use_huffman { 0x80 } else { 0x00 }, stored_size as u64, 7);

    if use_huffman {
        let mut encoded = Vec::new();
        encode_huffman(payload, &mut encoded);
        if encoded.len() != stored_size {
            return MrdEncodeResult { bytes: Vec::new(), error: MrdWireError::InvalidHuffmanData };
        }
        bytes.extend_from_slice(&encoded);
    } else if stored_size != 0 {
        bytes.extend_from_slice(&payload[..stored_size]);
    }
    MrdEncodeResult { bytes, error: MrdWireError::None }
}

pub fn decode_mrd80_record(data: &[u8], limits: &Mrd80CodecLimits) -> MrdDecodeResult {
    let limits = bounded_limits(limits);
    if limits.max_record_bytes == 0 {
        return MrdDecodeResult {
            record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 },
            error: MrdWireError::RecordLimitExceeded,
        };
    }
    if data.is_empty() {
        return MrdDecodeResult {
            record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 },
            error: MrdWireError::NeedMoreData,
        };
    }
    if (data[0] & 0xc0) != 0x80 {
        return MrdDecodeResult {
            record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 },
            error: MrdWireError::InvalidTagFamily,
        };
    }
    let size = data.len();
    let mut offset = 0usize;
    let mut back_reference_delta = 0u64;
    let mut error = read_bounded_field(data, size, limits.max_record_bytes, &mut offset, 6, &mut back_reference_delta);
    if error != MrdWireError::None {
        return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error };
    }
    if offset >= limits.max_record_bytes {
        return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error: MrdWireError::RecordLimitExceeded };
    }
    if offset >= size {
        return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error: MrdWireError::NeedMoreData };
    }
    let compressed = (data[offset] & 0x80) != 0;
    let mut stored_size64 = 0u64;
    error = read_bounded_field(data, size, limits.max_record_bytes, &mut offset, 7, &mut stored_size64);
    if error != MrdWireError::None {
        return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error };
    }
    if stored_size64 > limits.max_payload_bytes as u64
        || stored_size64 > usize::MAX as u64
    {
        return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error: MrdWireError::PayloadLimitExceeded };
    }
    let stored_size = stored_size64 as usize;
    if offset > limits.max_record_bytes || stored_size > limits.max_record_bytes - offset {
        return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error: MrdWireError::RecordLimitExceeded };
    }
    if stored_size > size - offset {
        return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error: MrdWireError::NeedMoreData };
    }

    let mut record = Mrd80Record {
        back_reference_delta,
        huffman_compressed: compressed,
        payload: Vec::new(),
        bytes_consumed: offset + stored_size,
    };
    if compressed {
        let mut payload = Vec::new();
        let err = decode_huffman(&data[offset..offset + stored_size], limits.max_decoded_bytes, &mut payload);
        if err != MrdWireError::None {
            return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error: err };
        }
        record.payload = payload;
    } else {
        if stored_size > limits.max_decoded_bytes {
            return MrdDecodeResult { record: Mrd80Record { back_reference_delta: 0, huffman_compressed: false, payload: Vec::new(), bytes_consumed: 0 }, error: MrdWireError::DecodedPayloadLimitExceeded };
        }
        if stored_size != 0 {
            record.payload = data[offset..offset + stored_size].to_vec();
        }
    }
    MrdDecodeResult { record, error: MrdWireError::None }
}

pub fn resolve_mrd80_prior_sequence_index(current_sequence: u64, back_reference_delta: u64) -> Option<u64> {
    if current_sequence == 0 || back_reference_delta >= current_sequence {
        return None;
    }
    Some(current_sequence - back_reference_delta - 1)
}
