//! Current Roblox connected-RakNet transport used after `RbxOpenReply2`.
//!
//! The normal SessionCrypto framing and datagram feature switches are taken
//! from the uploaded 0.735 `SessionCrypto.c`, `ReliabilityLayer.c`, and
//! `DatagramHeaderFormat.c`. RUPP remains outside the encrypted RakNet view.

use crate::raknet_2022::{
    encode_ack_datagram, encode_data_datagram, encode_nak_datagram, parse_datagram, AckDatagram,
    AckRange, DataDatagram, Datagram, DatagramFeatures, InternalPacket, NakDatagram,
    PacketReliability,
};
use chacha20poly1305::{
    aead::{AeadInPlace, KeyInit},
    ChaCha20Poly1305, Key, Nonce, Tag,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const SESSION_AEAD_OVERHEAD: usize = 18;
// SessionCrypto's constructor initializes both connected-RakNet nonce
// counters to the little-endian integer represented by the first eight bytes
// of `UniqueNumbeR`. Consequently the first nonce is the complete literal,
// not eight zero bytes followed by `mbeR`.
const INITIAL_RAK_NONCE_COUNTER: u64 = 0x754e_6575_7169_6e55;
const ID_CONNECTION_REQUEST: u8 = 0x09;
const ID_CONNECTION_REQUEST_ACCEPTED: u8 = 0x10;
// Current Cloud Edit initializes RBX::Network::versionB to the two password
// bytes `'^'` and 17. RakNetClientConnection passes that string to Connect,
// and sendApplicationConnectionRequest appends it after the security byte.
const CLOUD_EDIT_CONNECTION_PASSWORD: [u8; 2] = [b'^', 17];
const RUPP_PROTOCOL_RAKNET: u8 = 1;
const RUPP_TLV_TOKEN: u8 = 1;
const RUPP_TOKEN_VALUE_LENGTH: usize = 17;
const RUPP_RCC_TOKEN_TYPE: u8 = 2;
const UINT24_MASK: u32 = 0x00ff_ffff;
const UINT24_HALF_RANGE: u32 = 0x0080_0000;
const INITIAL_RTO_MS: u64 = 350;
const MAX_RTO_MS: u64 = 2_000;
const MIN_CONNECT_WAIT_MS: u64 = 5_000;
const MAX_SPLIT_BYTES: usize = 64 * 1024 * 1024;

static RAKNET_TIME_ORIGIN: OnceLock<Instant> = OnceLock::new();

pub(crate) struct ConnectedConfig {
    pub client_guid: u64,
    pub mtu: u16,
    pub common_capabilities: u64,
    pub session_server_to_client: [u8; 32],
    pub session_client_to_server: [u8; 32],
    /// Current-build SHA-512 KX digest halves. Public September 2026 static
    /// evidence establishes the transcript but not the directional split, so
    /// both orientations are late diagnostics rather than native defaults.
    pub sha512_session_first_half: [u8; 32],
    pub sha512_session_second_half: [u8; 32],
    pub rupp_prefix: Vec<u8>,
    /// Current OpenReply2 can carry the server's subtype-2 route token in its
    /// outer RUPP header. Native RakPeer does not install that offline token
    /// into the newly assigned remote; it starts online traffic with the
    /// default subtype-1 header and accepts a token update only after the
    /// online packet resolves that active remote. Retain the advertised value
    /// solely for the post-native online route diagnostic.
    pub deferred_reply2_rupp_token_type: Option<u8>,
    pub deferred_reply2_rupp_token: Option<[u8; 16]>,
    /// Exact outer RUPP header returned with OpenReply2. It is retained only
    /// for a late diagnostic; audited native traffic still starts with the
    /// subtype-1 client header.
    pub deferred_reply2_rupp_prefix: Option<Vec<u8>>,
    pub timeout_ms: u64,
}

#[derive(Debug)]
pub(crate) struct ConnectionAcceptedSummary {
    pub source: SocketAddr,
    pub wire_bytes: usize,
    pub session_kdf: &'static str,
    pub client_address: String,
    pub system_index: u16,
    pub internal_address_count: usize,
    pub request_time: u64,
    pub server_time: u64,
    pub server_epoch_time_us: u64,
    pub datagram_number: u32,
    pub reliability: PacketReliability,
    pub request_acked: bool,
    pub retransmissions: usize,
    pub refreshed_rupp_tokens: usize,
    pub tx_nonce: u64,
    pub rx_nonce: u64,
    pub elapsed_ms: u128,
}

#[derive(Clone, Copy)]
struct ReceivedRuppToken {
    token_type: u8,
    value: [u8; 16],
}

#[derive(Clone, Debug)]
struct ConnectionRequestSendTrace {
    wire_bytes: usize,
    encrypted_region_bytes: usize,
    nonce_suffix: [u8; 2],
    extra_padding: u16,
    plaintext: Vec<u8>,
}

struct SessionCrypto {
    server_to_client: [u8; 32],
    client_to_server: [u8; 32],
    tx_nonce: u64,
    rx_nonce: u64,
}

impl SessionCrypto {
    fn new(server_to_client: [u8; 32], client_to_server: [u8; 32]) -> Self {
        Self {
            server_to_client,
            client_to_server,
            tx_nonce: INITIAL_RAK_NONCE_COUNTER,
            rx_nonce: INITIAL_RAK_NONCE_COUNTER,
        }
    }

    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let counter = self.tx_nonce;
        // Native increments the atomic nonce before invoking the AEAD helper,
        // so even a crypto error consumes this counter value.
        self.tx_nonce = self.tx_nonce.wrapping_add(1);
        let nonce = rak_nonce(counter);
        let mut ciphertext = plaintext.to_vec();
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.client_to_server));
        let tag = cipher
            .encrypt_in_place_detached(Nonce::from_slice(&nonce), &[], &mut ciphertext)
            .map_err(|_| "ChaCha20-Poly1305 failed to encrypt connected RakNet data".to_string())?;
        ciphertext.extend_from_slice(&nonce[..2]);
        ciphertext.extend_from_slice(&tag);
        Ok(ciphertext)
    }

    fn decrypt(&mut self, wire: &[u8]) -> Result<Vec<u8>, String> {
        if wire.len() < SESSION_AEAD_OVERHEAD {
            return Err(format!(
                "encrypted RakNet datagram is {} bytes, below the 18-byte SessionCrypto overhead",
                wire.len()
            ));
        }
        let ciphertext_len = wire.len() - SESSION_AEAD_OVERHEAD;
        let low = u16::from_le_bytes([wire[ciphertext_len], wire[ciphertext_len + 1]]);
        let (counter, next_rx_nonce) = reconstruct_rx_nonce(self.rx_nonce, low);
        let nonce = rak_nonce(counter);
        let mut plaintext = wire[..ciphertext_len].to_vec();
        let tag = Tag::from_slice(&wire[ciphertext_len + 2..]);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.server_to_client));
        cipher
            .decrypt_in_place_detached(Nonce::from_slice(&nonce), &[], &mut plaintext, tag)
            .map_err(|_| {
                format!(
                    "connected RakNet authentication failed for reconstructed receive nonce {counter}"
                )
            })?;
        // Native advances the receive reconstruction anchor only after a
        // successful detached-tag check. Old in-window packets do not rewind it.
        self.rx_nonce = next_rx_nonce;
        Ok(plaintext)
    }
}

/// Normal RakNet SessionCrypto uses a different final nonce byte than the
/// early/application crypto: native copies `UniqueNumbeR`, then replaces its
/// first eight bytes with the little-endian counter. The constructor seeds
/// that counter with little-endian `UniqueNu`, so the first nonce remains the
/// complete `UniqueNumbeR` literal.
fn rak_nonce(counter: u64) -> [u8; 12] {
    let mut nonce = *b"UniqueNumbeR";
    nonce[..8].copy_from_slice(&counter.to_le_bytes());
    nonce
}

/// Reproduce 0.735's 16-bit wire-counter reconstruction, including its exact
/// 0x8001 threshold around the nearest 64K window.
fn reconstruct_rx_nonce(current: u64, low: u16) -> (u64, u64) {
    let candidate = (current & !0xffff) | u64::from(low);
    if candidate >= current {
        if candidate - current < 0x8001 {
            (candidate, candidate)
        } else {
            (candidate.wrapping_sub(0x1_0000), current)
        }
    } else if current - candidate >= 0x8001 {
        let wrapped = candidate.wrapping_add(0x1_0000);
        (wrapped, wrapped)
    } else {
        (candidate, current)
    }
}

fn datagram_features(common_capabilities: u64) -> DatagramFeatures {
    // Both current congestion implementations return false for
    // includeTimestampWithDatagrams(). The other switches are the exact tests
    // made by current ReliabilityLayer at DatagramHeaderFormat call sites.
    DatagramFeatures {
        include_timestamp: false,
        avoid_packet_size: common_capabilities as u8 & 0x80 == 0,
        join_data_bit: common_capabilities & 0x20_0000 != 0,
        resent_bit: common_capabilities & 0x80_0000 != 0,
    }
}

fn strip_optional_rupp_prefix(packet: &[u8]) -> Result<(&[u8], Option<ReceivedRuppToken>), String> {
    if packet.first() != Some(&RUPP_PROTOCOL_RAKNET) {
        return Ok((packet, None));
    }
    if packet.len() < 4 {
        return Err("connected packet has a truncated RUPP prefix".into());
    }
    let header_len = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if header_len < 4 || header_len > packet.len() {
        return Err(format!(
            "connected packet has invalid RUPP header length {header_len}"
        ));
    }
    let mut token = None;
    let mut cursor = 4usize;
    while cursor < header_len {
        if cursor + 2 > header_len {
            return Err("connected RUPP header ends inside a TLV header".into());
        }
        let kind = packet[cursor];
        let length = usize::from(packet[cursor + 1]);
        cursor += 2;
        let value = packet
            .get(cursor..cursor + length)
            .filter(|_| cursor + length <= header_len)
            .ok_or_else(|| "connected RUPP header has a truncated TLV".to_string())?;
        if kind == RUPP_TLV_TOKEN {
            if length != RUPP_TOKEN_VALUE_LENGTH {
                return Err(format!(
                    "connected RUPP token TLV has length {length}, expected {RUPP_TOKEN_VALUE_LENGTH}"
                ));
            }
            token = Some(ReceivedRuppToken {
                token_type: value[0],
                value: value[1..17].try_into().unwrap(),
            });
        }
        cursor += length;
    }
    Ok((&packet[header_len..], token))
}

fn update_outbound_rupp_token(
    prefix: &mut [u8],
    token: ReceivedRuppToken,
) -> Result<bool, String> {
    if token.token_type != RUPP_RCC_TOKEN_TYPE {
        return Err(format!(
            "RCC returned RUPP token subtype {}, expected refreshed subtype {RUPP_RCC_TOKEN_TYPE}",
            token.token_type
        ));
    }
    if prefix.len() < 4 || prefix[0] != RUPP_PROTOCOL_RAKNET {
        return Err("RCC returned a token but the outbound RUPP header is unavailable".into());
    }
    let header_len = usize::from(u16::from_be_bytes([prefix[2], prefix[3]]));
    if header_len != prefix.len() {
        return Err(format!(
            "outbound RUPP header length {header_len} does not match {} bytes",
            prefix.len()
        ));
    }
    let mut cursor = 4usize;
    while cursor < header_len {
        if cursor + 2 > header_len {
            return Err("outbound RUPP header ends inside a TLV header".into());
        }
        let kind = prefix[cursor];
        let length = usize::from(prefix[cursor + 1]);
        cursor += 2;
        if cursor + length > header_len {
            return Err("outbound RUPP header has a truncated TLV".into());
        }
        if kind == RUPP_TLV_TOKEN {
            if length != RUPP_TOKEN_VALUE_LENGTH {
                return Err(format!(
                    "outbound RUPP token TLV has length {length}, expected {RUPP_TOKEN_VALUE_LENGTH}"
                ));
            }
            let changed = prefix[cursor] != token.token_type
                || prefix[cursor + 1..cursor + 17] != token.value;
            prefix[cursor] = token.token_type;
            prefix[cursor + 1..cursor + 17].copy_from_slice(&token.value);
            return Ok(changed);
        }
        cursor += length;
    }
    Err("outbound RUPP header has no token TLV to refresh".into())
}

fn diagnostic_reply2_rupp_prefix(config: &ConnectedConfig) -> Result<Option<Vec<u8>>, String> {
    let (Some(token_type), Some(value)) = (
        config.deferred_reply2_rupp_token_type,
        config.deferred_reply2_rupp_token,
    ) else {
        return Ok(None);
    };
    let mut prefix = config.rupp_prefix.clone();
    update_outbound_rupp_token(&mut prefix, ReceivedRuppToken { token_type, value })?;
    Ok(Some(prefix))
}

fn send_plain_datagram(
    socket: &UdpSocket,
    peer: SocketAddr,
    prefix: &[u8],
    crypto: &mut SessionCrypto,
    plaintext: &[u8],
    mtu: u16,
) -> Result<usize, String> {
    let encrypted = crypto.encrypt(plaintext)?;
    let wire_len = prefix.len() + encrypted.len();
    let max_udp_payload = usize::from(mtu).saturating_sub(40);
    if wire_len > max_udp_payload {
        return Err(format!(
            "connected datagram needs {wire_len} bytes but negotiated MTU {mtu} permits {max_udp_payload}"
        ));
    }
    let mut wire = Vec::with_capacity(wire_len);
    wire.extend_from_slice(prefix);
    wire.extend_from_slice(&encrypted);
    let sent = socket
        .send_to(&wire, peer)
        .map_err(|error| format!("connected RakNet send to {peer} failed: {error}"))?;
    if sent != wire_len {
        return Err(format!(
            "connected RakNet send to {peer} reported {sent} of {wire_len} UDP bytes"
        ));
    }
    Ok(wire_len)
}

/// Start the process-local RakNet clock before any handshake I/O. Current
/// native `Time::now<2>` measures from a lazy process-local mach-time sample,
/// normally established before RakPeer connects. Initializing our equivalent
/// while constructing ID_CONNECTION_REQUEST made its first timestamp exactly
/// zero instead.
pub(crate) fn initialize_raknet_time() {
    RAKNET_TIME_ORIGIN.get_or_init(Instant::now);
}

fn raknet_time_ms() -> u64 {
    // Current RakNet::GetTime(false) converts Time::now<2>'s monotonic elapsed
    // seconds to microseconds and then milliseconds. Only process-local elapsed
    // time is represented; there is no wall-clock or server-shared origin.
    RAKNET_TIME_ORIGIN
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis() as u64
}

fn connection_request_payload(client_guid: u64) -> (Vec<u8>, u64) {
    let request_time = raknet_time_ms();
    let mut payload = Vec::with_capacity(18 + CLOUD_EDIT_CONNECTION_PASSWORD.len());
    payload.push(ID_CONNECTION_REQUEST);
    payload.extend_from_slice(&client_guid.to_be_bytes());
    payload.extend_from_slice(&request_time.to_be_bytes());
    payload.push(0); // RakNet security/challenge flag is disabled.
    payload.extend_from_slice(&CLOUD_EDIT_CONNECTION_PASSWORD);
    (payload, request_time)
}

fn send_connection_request(
    socket: &UdpSocket,
    peer: SocketAddr,
    prefix: &[u8],
    crypto: &mut SessionCrypto,
    features: DatagramFeatures,
    mtu: u16,
    datagram_number: u32,
    reliable_message_number: u32,
    payload: &[u8],
    retransmission: bool,
    extra_padding: u16,
) -> Result<ConnectionRequestSendTrace, String> {
    let data = DataDatagram {
        is_join_data: false,
        is_resent: retransmission && features.resent_bit,
        is_continuous_send: false,
        // Both current congestion implementations begin in slow start.
        // ReliabilityLayer writes GetIsInSlowStart() into this header bit.
        needs_b_and_as: true,
        source_system_time: None,
        datagram_number,
        extra_padding,
        packets: vec![InternalPacket {
            reliability: PacketReliability::Reliable,
            data_bit_length: u16::try_from(payload.len() * 8)
                .map_err(|_| "ID_CONNECTION_REQUEST is too large".to_string())?,
            reliable_message_number: Some(reliable_message_number),
            sequencing_index: None,
            ordering_index: None,
            ordering_channel: None,
            split: None,
            payload: payload.to_vec(),
        }],
    };
    let plaintext = encode_data_datagram(&data, features)
        .map_err(|error| format!("failed to encode ID_CONNECTION_REQUEST: {error}"))?;
    let nonce_suffix = crypto.tx_nonce.to_le_bytes()[..2].try_into().unwrap();
    let encrypted_region_bytes = plaintext.len() + SESSION_AEAD_OVERHEAD;
    let wire_bytes = send_plain_datagram(socket, peer, prefix, crypto, &plaintext, mtu)?;
    Ok(ConnectionRequestSendTrace {
        wire_bytes,
        encrypted_region_bytes,
        nonce_suffix,
        extra_padding,
        plaintext,
    })
}

fn send_ack(
    socket: &UdpSocket,
    peer: SocketAddr,
    prefix: &[u8],
    crypto: &mut SessionCrypto,
    features: DatagramFeatures,
    mtu: u16,
    datagram_number: u32,
) -> Result<(), String> {
    let plaintext = encode_ack_datagram(
        &AckDatagram {
            has_b_and_as: false,
            has_ack_timestamps: false,
            source_system_time: None,
            as_value: None,
            extra_padding: 0,
            ranges: vec![AckRange::single(datagram_number)],
            ack_timestamps: None,
        },
        features,
    )
    .map_err(|error| format!("failed to encode RakNet ACK: {error}"))?;
    send_plain_datagram(socket, peer, prefix, crypto, &plaintext, mtu)?;
    Ok(())
}

fn send_nak(
    socket: &UdpSocket,
    peer: SocketAddr,
    prefix: &[u8],
    crypto: &mut SessionCrypto,
    features: DatagramFeatures,
    mtu: u16,
    ranges: Vec<AckRange>,
) -> Result<(), String> {
    if ranges.is_empty() {
        return Ok(());
    }
    let plaintext = encode_nak_datagram(
        &NakDatagram {
            extra_padding: 0,
            ranges,
        },
        features,
    )
    .map_err(|error| format!("failed to encode RakNet NAK: {error}"))?;
    send_plain_datagram(socket, peer, prefix, crypto, &plaintext, mtu)?;
    Ok(())
}

fn range_contains(range: &AckRange, value: u32) -> bool {
    range.min <= value && value <= range.max
}

fn ranges_ack_any(ranges: &[AckRange], sent_datagrams: &[u32]) -> bool {
    ranges
        .iter()
        .any(|range| sent_datagrams.iter().any(|value| range_contains(range, *value)))
}

fn u24_forward_distance(from: u32, to: u32) -> u32 {
    to.wrapping_sub(from) & UINT24_MASK
}

struct DatagramTracker {
    next: u32,
    out_of_order: HashSet<u32>,
}

impl DatagramTracker {
    fn new() -> Self {
        Self {
            next: 0,
            out_of_order: HashSet::new(),
        }
    }

    fn observe(&mut self, number: u32) -> Vec<AckRange> {
        let number = number & UINT24_MASK;
        let distance = u24_forward_distance(self.next, number);
        if distance == 0 {
            self.next = self.next.wrapping_add(1) & UINT24_MASK;
            while self.out_of_order.remove(&self.next) {
                self.next = self.next.wrapping_add(1) & UINT24_MASK;
            }
            return Vec::new();
        }
        if distance >= UINT24_HALF_RANGE {
            return Vec::new(); // duplicate/old datagram
        }
        self.out_of_order.insert(number);
        let end = number.wrapping_sub(1) & UINT24_MASK;
        if self.next <= end {
            vec![AckRange {
                min: self.next,
                max: end,
            }]
        } else {
            vec![
                AckRange {
                    min: self.next,
                    max: UINT24_MASK,
                },
                AckRange { min: 0, max: end },
            ]
        }
    }
}

#[derive(Clone)]
struct DeliveredMessage {
    reliability: PacketReliability,
    bit_length: usize,
    ordering_index: Option<u32>,
    ordering_channel: Option<u8>,
    sequencing_index: Option<u32>,
    payload: Vec<u8>,
}

struct SplitAssembly {
    reliability: PacketReliability,
    ordering_index: Option<u32>,
    ordering_channel: Option<u8>,
    sequencing_index: Option<u32>,
    fragments: Vec<Option<(Vec<u8>, usize)>>,
    byte_total: usize,
}

struct ReliabilityReceiver {
    seen_reliable: HashSet<u32>,
    splits: HashMap<u16, SplitAssembly>,
    ordering_expected: [u32; 32],
    ordered: [BTreeMap<u32, DeliveredMessage>; 32],
    latest_sequenced: [Option<u32>; 32],
}

impl ReliabilityReceiver {
    fn new() -> Self {
        Self {
            seen_reliable: HashSet::new(),
            splits: HashMap::new(),
            ordering_expected: [0; 32],
            ordered: std::array::from_fn(|_| BTreeMap::new()),
            latest_sequenced: [None; 32],
        }
    }

    fn receive(&mut self, packet: InternalPacket) -> Result<Vec<DeliveredMessage>, String> {
        if let Some(number) = packet.reliable_message_number {
            if !self.seen_reliable.insert(number & UINT24_MASK) {
                return Ok(Vec::new());
            }
        }
        let message = if let Some(split) = packet.split {
            let count = usize::try_from(split.count)
                .map_err(|_| "RakNet split count does not fit this platform".to_string())?;
            let index = usize::try_from(split.index)
                .map_err(|_| "RakNet split index does not fit this platform".to_string())?;
            let mut assembly_error = None;
            let complete;
            {
                let entry = self.splits.entry(split.id).or_insert_with(|| SplitAssembly {
                    reliability: packet.reliability,
                    ordering_index: packet.ordering_index,
                    ordering_channel: packet.ordering_channel,
                    sequencing_index: packet.sequencing_index,
                    fragments: vec![None; count],
                    byte_total: 0,
                });
                if entry.fragments.len() != count
                    || entry.reliability != packet.reliability
                    || entry.ordering_index != packet.ordering_index
                    || entry.ordering_channel != packet.ordering_channel
                    || entry.sequencing_index != packet.sequencing_index
                {
                    assembly_error = Some(format!(
                        "inconsistent RakNet split metadata for split id {}",
                        split.id
                    ));
                    complete = false;
                } else {
                    if entry.fragments[index].is_none() {
                        entry.byte_total = entry
                            .byte_total
                            .checked_add(packet.payload.len())
                            .ok_or_else(|| "RakNet split byte count overflow".to_string())?;
                        if entry.byte_total > MAX_SPLIT_BYTES {
                            assembly_error = Some(format!(
                                "RakNet split message exceeds {MAX_SPLIT_BYTES} bytes"
                            ));
                        } else {
                            entry.fragments[index] =
                                Some((packet.payload, usize::from(packet.data_bit_length)));
                        }
                    }
                    complete = assembly_error.is_none()
                        && entry.fragments.iter().all(Option::is_some);
                }
            }
            if let Some(error) = assembly_error {
                self.splits.remove(&split.id);
                return Err(error);
            }
            if !complete {
                return Ok(Vec::new());
            }
            let assembly = self.splits.remove(&split.id).unwrap();
            let bit_length = assembly
                .fragments
                .iter()
                .map(|fragment| fragment.as_ref().unwrap().1)
                .sum();
            let mut payload = vec![0u8; (bit_length + 7) / 8];
            let mut output_bit = 0usize;
            for fragment in assembly.fragments {
                let (bytes, bits) = fragment.unwrap();
                append_bits(&mut payload, &mut output_bit, &bytes, bits);
            }
            DeliveredMessage {
                reliability: assembly.reliability,
                bit_length,
                ordering_index: assembly.ordering_index,
                ordering_channel: assembly.ordering_channel,
                sequencing_index: assembly.sequencing_index,
                payload,
            }
        } else {
            DeliveredMessage {
                reliability: packet.reliability,
                bit_length: usize::from(packet.data_bit_length),
                ordering_index: packet.ordering_index,
                ordering_channel: packet.ordering_channel,
                sequencing_index: packet.sequencing_index,
                payload: packet.payload,
            }
        };
        self.apply_ordering(message)
    }

    fn apply_ordering(&mut self, message: DeliveredMessage) -> Result<Vec<DeliveredMessage>, String> {
        match message.reliability {
            PacketReliability::ReliableOrdered | PacketReliability::ReliableOrderedWithAckReceipt => {
                let channel = usize::from(
                    message
                        .ordering_channel
                        .ok_or_else(|| "ordered RakNet message has no channel".to_string())?,
                );
                if channel >= 32 {
                    return Err(format!("ordered RakNet channel {channel} is outside 0..31"));
                }
                let index = message
                    .ordering_index
                    .ok_or_else(|| "ordered RakNet message has no ordering index".to_string())?
                    & UINT24_MASK;
                let expected = self.ordering_expected[channel];
                let distance = u24_forward_distance(expected, index);
                if distance >= UINT24_HALF_RANGE {
                    return Ok(Vec::new());
                }
                if distance != 0 {
                    self.ordered[channel].entry(index).or_insert(message);
                    return Ok(Vec::new());
                }
                let mut delivered = vec![message];
                self.ordering_expected[channel] = expected.wrapping_add(1) & UINT24_MASK;
                while let Some(next) = self.ordered[channel].remove(&self.ordering_expected[channel]) {
                    delivered.push(next);
                    self.ordering_expected[channel] =
                        self.ordering_expected[channel].wrapping_add(1) & UINT24_MASK;
                }
                Ok(delivered)
            }
            PacketReliability::UnreliableSequenced | PacketReliability::ReliableSequenced => {
                let channel = usize::from(
                    message
                        .ordering_channel
                        .ok_or_else(|| "sequenced RakNet message has no channel".to_string())?,
                );
                if channel >= 32 {
                    return Err(format!("sequenced RakNet channel {channel} is outside 0..31"));
                }
                let sequence = message
                    .sequencing_index
                    .ok_or_else(|| "sequenced RakNet message has no sequence index".to_string())?
                    & UINT24_MASK;
                if let Some(latest) = self.latest_sequenced[channel] {
                    let distance = u24_forward_distance(latest, sequence);
                    if distance == 0 || distance >= UINT24_HALF_RANGE {
                        return Ok(Vec::new());
                    }
                }
                self.latest_sequenced[channel] = Some(sequence);
                Ok(vec![message])
            }
            _ => Ok(vec![message]),
        }
    }
}

fn append_bits(output: &mut [u8], output_bit: &mut usize, input: &[u8], bit_count: usize) {
    for input_bit in 0..bit_count {
        let value = input[input_bit / 8] & (0x80 >> (input_bit & 7));
        if value != 0 {
            output[*output_bit / 8] |= 0x80 >> (*output_bit & 7);
        }
        *output_bit += 1;
    }
}

struct AcceptedPayload {
    client_address: String,
    system_index: u16,
    internal_address_count: usize,
    request_time: u64,
    server_time: u64,
    server_epoch_time_us: u64,
}

fn parse_system_address(input: &[u8], cursor: &mut usize) -> Result<String, String> {
    let version = *input
        .get(*cursor)
        .ok_or_else(|| "ID_CONNECTION_REQUEST_ACCEPTED ends before an address version".to_string())?;
    *cursor += 1;
    if version != 4 {
        return Err(format!(
            "ID_CONNECTION_REQUEST_ACCEPTED uses unsupported SystemAddress version {version}"
        ));
    }
    let encoded = input
        .get(*cursor..*cursor + 4)
        .ok_or_else(|| "ID_CONNECTION_REQUEST_ACCEPTED has a truncated IPv4 address".to_string())?;
    *cursor += 4;
    let port = u16::from_be_bytes(
        input
            .get(*cursor..*cursor + 2)
            .ok_or_else(|| "ID_CONNECTION_REQUEST_ACCEPTED has a truncated port".to_string())?
            .try_into()
            .unwrap(),
    );
    *cursor += 2;
    let ip = Ipv4Addr::new(!encoded[0], !encoded[1], !encoded[2], !encoded[3]);
    Ok(format!("{ip}:{port}"))
}

fn read_u64_be(input: &[u8], cursor: &mut usize, name: &str) -> Result<u64, String> {
    let value = u64::from_be_bytes(
        input
            .get(*cursor..*cursor + 8)
            .ok_or_else(|| format!("ID_CONNECTION_REQUEST_ACCEPTED ends before {name}"))?
            .try_into()
            .unwrap(),
    );
    *cursor += 8;
    Ok(value)
}

fn parse_connection_accepted(payload: &[u8]) -> Result<AcceptedPayload, String> {
    if payload.first() != Some(&ID_CONNECTION_REQUEST_ACCEPTED) {
        return Err("message is not ID_CONNECTION_REQUEST_ACCEPTED".into());
    }
    let mut cursor = 1usize;
    let client_address = parse_system_address(payload, &mut cursor)?;
    let system_index = u16::from_be_bytes(
        payload
            .get(cursor..cursor + 2)
            .ok_or_else(|| "ID_CONNECTION_REQUEST_ACCEPTED ends before system index".to_string())?
            .try_into()
            .unwrap(),
    );
    cursor += 2;
    let mut internal_address_count = 0usize;
    for _ in 0..10 {
        parse_system_address(payload, &mut cursor)?;
        internal_address_count += 1;
    }
    let request_time = read_u64_be(payload, &mut cursor, "request time")?;
    let server_time = read_u64_be(payload, &mut cursor, "server time")?;
    let server_epoch_time_us = read_u64_be(payload, &mut cursor, "server epoch time")?;
    if cursor != payload.len() {
        return Err(format!(
            "ID_CONNECTION_REQUEST_ACCEPTED has {} unexpected trailing byte(s)",
            payload.len() - cursor
        ));
    }
    Ok(AcceptedPayload {
        client_address,
        system_index,
        internal_address_count,
        request_time,
        server_time,
        server_epoch_time_us,
    })
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            output.push(' ');
        }
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn redacted_rupp_hex(prefix: &[u8]) -> String {
    let mut redacted = vec![false; prefix.len()];
    let mut cursor = 4usize;
    while cursor + 2 <= prefix.len() {
        let kind = prefix[cursor];
        let length = usize::from(prefix[cursor + 1]);
        cursor += 2;
        let Some(end) = cursor.checked_add(length).filter(|end| *end <= prefix.len()) else {
            break;
        };
        if kind == RUPP_TLV_TOKEN && length == RUPP_TOKEN_VALUE_LENGTH {
            // Preserve the subtype byte but never copy the refreshed routing
            // credential into screenshots, logs, or issue reports.
            redacted[cursor + 1..end].fill(true);
        }
        cursor = end;
    }

    let mut output = String::with_capacity(prefix.len() * 3);
    for (index, byte) in prefix.iter().enumerate() {
        if index != 0 {
            output.push(' ');
        }
        if redacted[index] {
            output.push_str("**");
        } else {
            let _ = write!(output, "{byte:02x}");
        }
    }
    output
}

fn describe_rupp_prefix(prefix: &[u8]) -> String {
    if prefix.len() < 4 {
        return format!("truncated {}-byte prefix", prefix.len());
    }
    let declared = usize::from(u16::from_be_bytes([prefix[2], prefix[3]]));
    let mut cursor = 4usize;
    let mut tlvs = Vec::new();
    while cursor + 2 <= prefix.len() {
        let kind = prefix[cursor];
        let length = usize::from(prefix[cursor + 1]);
        cursor += 2;
        let Some(value) = prefix.get(cursor..cursor.saturating_add(length)) else {
            tlvs.push(format!("type {kind} truncated length {length}"));
            break;
        };
        let description = match (kind, length) {
            (RUPP_TLV_TOKEN, RUPP_TOKEN_VALUE_LENGTH) => {
                format!("token(length 17, subtype {})", value[0])
            }
            (2, 6) => {
                let address = Ipv4Addr::new(value[0], value[1], value[2], value[3]);
                let port = u16::from_be_bytes([value[4], value[5]]);
                format!("ipv4(length 6, {address}:{port})")
            }
            (3, 18) => {
                let port = u16::from_be_bytes([value[16], value[17]]);
                format!("ipv6(length 18, port {port})")
            }
            _ => format!("type {kind}(length {length})"),
        };
        tlvs.push(description);
        cursor += length;
    }
    if cursor != prefix.len() {
        tlvs.push(format!("{} trailing byte(s)", prefix.len() - cursor));
    }
    format!(
        "protocol {}, flags 0x{:02x}, declared length {declared}, actual length {}, TLVs [{}]",
        prefix[0],
        prefix[1],
        prefix.len(),
        tlvs.join(", ")
    )
}

fn route_selected_source(peer: SocketAddr, bound: Option<SocketAddr>) -> Option<SocketAddr> {
    // An unconnected wildcard UDP socket keeps reporting 0.0.0.0/:: through
    // getsockname even after sendto. A throwaway connected UDP socket performs
    // the same kernel route lookup without transmitting; combine its selected
    // interface address with the real handshake socket's retained source port.
    let bind_address = if peer.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let probe = UdpSocket::bind(bind_address).ok()?;
    probe.connect(peer).ok()?;
    let mut selected = probe.local_addr().ok()?;
    if let Some(bound) = bound {
        selected.set_port(bound.port());
    }
    Some(selected)
}

fn format_request_wire_trace(
    trace: &ConnectionRequestSendTrace,
    prefix: &[u8],
    features: DatagramFeatures,
    common_capabilities: u64,
    bound_local: Option<SocketAddr>,
    route_local: Option<SocketAddr>,
    peer: SocketAddr,
    request_time: u64,
) -> String {
    let data_header_len = 1
        + (features.include_timestamp as usize) * 4
        + 3
        + (features.avoid_packet_size as usize) * 2;
    let reliability_header_len = 6usize;
    let data_header = trace
        .plaintext
        .get(..data_header_len)
        .map(hex_bytes)
        .unwrap_or_else(|| "<truncated>".into());
    let reliability_header = trace
        .plaintext
        .get(data_header_len..data_header_len + reliability_header_len)
        .map(hex_bytes)
        .unwrap_or_else(|| "<truncated>".into());
    format!(
        "wire trace: socket bound {}, route-selected source {}, destination {peer}, capabilities 0x{common_capabilities:016x}, features(timestamp {}, avoided-size field {}, join-data bit {}, resent bit {}), RUPP {{{}}}, RUPP hex (token bytes redacted) [{}], data header ({data_header_len} bytes) [{data_header}], reliable header ({reliability_header_len} bytes) [{reliability_header}], complete plaintext ({} bytes, padding {}) [{}], encrypted region {} bytes, nonce suffix [{}], final UDP payload {} bytes, request time {request_time}, connection password length {}",
        bound_local.map(|address| address.to_string()).unwrap_or_else(|| "unknown".into()),
        route_local.map(|address| address.to_string()).unwrap_or_else(|| "unknown".into()),
        features.include_timestamp,
        features.avoid_packet_size,
        features.join_data_bit,
        features.resent_bit,
        describe_rupp_prefix(prefix),
        redacted_rupp_hex(prefix),
        trace.plaintext.len(),
        trace.extra_padding,
        hex_bytes(&trace.plaintext),
        trace.encrypted_region_bytes,
        hex_bytes(&trace.nonce_suffix),
        trace.wire_bytes,
        CLOUD_EDIT_CONNECTION_PASSWORD.len(),
    )
}

/// Send encrypted reliable `ID_CONNECTION_REQUEST`, process encrypted
/// ACK/NAK/data datagrams, retransmit with fresh datagram/session nonces, and
/// ACK the accepted response before returning.
pub(crate) fn establish_connected_session(
    socket: &UdpSocket,
    peer: SocketAddr,
    mut config: ConnectedConfig,
) -> Result<ConnectionAcceptedSummary, String> {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(config.timeout_ms.max(MIN_CONNECT_WAIT_MS));
    let features = datagram_features(config.common_capabilities);
    let mut crypto = SessionCrypto::new(
        config.session_server_to_client,
        config.session_client_to_server,
    );
    // The 0.735 binary directly proves BLAKE2b and remains the native first
    // path. A current September 2026 client trace instead proves SHA-512 over
    // the normal-session KX transcript, but leaves digest-half direction
    // unresolved. Keep independent nonce streams for both evidence-backed
    // orientations so a failed BLAKE2b packet does not consume their expected
    // first nonce.
    let mut sha512_first_rx_crypto = SessionCrypto::new(
        config.sha512_session_first_half,
        config.sha512_session_second_half,
    );
    let mut sha512_first_tx_crypto = SessionCrypto::new(
        config.sha512_session_second_half,
        config.sha512_session_first_half,
    );
    let (request_payload, request_time) = connection_request_payload(config.client_guid);
    // Keep native subtype-1 traffic first. These alternatives are emitted
    // only after several unanswered native retransmissions. Preserve the
    // exact token-only outer header returned with Reply2 in addition to the
    // receive-updater shape (refreshed token in the client endpoint-bearing
    // header); those are observably different routed packets.
    let diagnostic_exact_reply2_prefix = config.deferred_reply2_rupp_prefix.clone();
    let diagnostic_refreshed_prefix = diagnostic_reply2_rupp_prefix(&config)?;
    let mut route_diagnostics_sent = false;
    let mut route_diagnostic_count = 0usize;
    let mut kdf_diagnostic_count = 0usize;
    let mut next_datagram_number = 0u32;
    let reliable_message_number = 0u32;
    let mut sent_datagrams = Vec::new();
    let mut retransmissions = 0usize;
    let mut request_acked = false;
    let mut refreshed_rupp_tokens = 0usize;
    let mut send_traces = Vec::new();

    let first_send = send_connection_request(
        socket,
        peer,
        &config.rupp_prefix,
        &mut crypto,
        features,
        config.mtu,
        next_datagram_number,
        reliable_message_number,
        &request_payload,
        false,
        0,
    )?;
    let bound_local = socket.local_addr().ok();
    let first_wire_trace = format_request_wire_trace(
        &first_send,
        &config.rupp_prefix,
        features,
        config.common_capabilities,
        bound_local,
        route_selected_source(peer, bound_local),
        peer,
        request_time,
    );
    send_traces.push(format!(
        "native subtype-1 datagram {next_datagram_number}, {} bytes, padding {}, nonce suffix [{}]",
        first_send.wire_bytes,
        first_send.extra_padding,
        hex_bytes(&first_send.nonce_suffix)
    ));
    let mut last_wire_bytes = first_send.wire_bytes;
    sent_datagrams.push(next_datagram_number);
    next_datagram_number = next_datagram_number.wrapping_add(1) & UINT24_MASK;

    let mut rto = Duration::from_millis(INITIAL_RTO_MS);
    let mut next_retransmit = Instant::now() + rto;
    let mut receive_buf = vec![0u8; usize::from(config.mtu).saturating_add(512).max(4096)];
    let mut datagram_tracker = DatagramTracker::new();
    let mut reliability = ReliabilityReceiver::new();
    let mut errors = Vec::new();

    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        if !request_acked && now >= next_retransmit {
            // Keep retransmissions byte-for-byte native unless an avoided-size
            // list is actually known. The capability only enables the u16
            // padding-count field; it does not itself require padding. The old
            // unconditional one-byte retry was a diagnostic experiment, not a
            // wire behavior established by the current build.
            let retry_padding = 0;
            let retransmit = send_connection_request(
                socket,
                peer,
                &config.rupp_prefix,
                &mut crypto,
                features,
                config.mtu,
                next_datagram_number,
                reliable_message_number,
                &request_payload,
                // InternalPacket::timesSent is incremented to one for the
                // first retransmission; current ReliabilityLayer marks the
                // optional resent bit only once that counter reaches two.
                retransmissions >= 1,
                retry_padding,
            )?;
            last_wire_bytes = retransmit.wire_bytes;
            send_traces.push(format!(
                "native subtype-1 datagram {next_datagram_number}, {} bytes, padding {}, nonce suffix [{}]",
                retransmit.wire_bytes,
                retransmit.extra_padding,
                hex_bytes(&retransmit.nonce_suffix)
            ));
            sent_datagrams.push(next_datagram_number);
            next_datagram_number = next_datagram_number.wrapping_add(1) & UINT24_MASK;
            retransmissions += 1;

            // Send evidence-backed KDF and route alternatives once only after
            // three unanswered native retransmissions. Any ACK distinguishes
            // the relevant boundary; no diagnostic changes the retained
            // native BLAKE2b/subtype-1 outbound state.
            if retransmissions == 3 && !route_diagnostics_sent {
                route_diagnostics_sent = true;

                let sha_first_rx = send_connection_request(
                    socket,
                    peer,
                    &config.rupp_prefix,
                    &mut sha512_first_rx_crypto,
                    features,
                    config.mtu,
                    next_datagram_number,
                    reliable_message_number,
                    &request_payload,
                    true,
                    0,
                )?;
                last_wire_bytes = sha_first_rx.wire_bytes;
                send_traces.push(format!(
                    "online diagnostic SHA-512 KDF first-half RX datagram {next_datagram_number}, {} bytes, nonce suffix [{}]",
                    sha_first_rx.wire_bytes,
                    hex_bytes(&sha_first_rx.nonce_suffix)
                ));
                sent_datagrams.push(next_datagram_number);
                next_datagram_number = next_datagram_number.wrapping_add(1) & UINT24_MASK;
                kdf_diagnostic_count += 1;

                let sha_first_tx = send_connection_request(
                    socket,
                    peer,
                    &config.rupp_prefix,
                    &mut sha512_first_tx_crypto,
                    features,
                    config.mtu,
                    next_datagram_number,
                    reliable_message_number,
                    &request_payload,
                    true,
                    0,
                )?;
                last_wire_bytes = sha_first_tx.wire_bytes;
                send_traces.push(format!(
                    "online diagnostic SHA-512 KDF first-half TX datagram {next_datagram_number}, {} bytes, nonce suffix [{}]",
                    sha_first_tx.wire_bytes,
                    hex_bytes(&sha_first_tx.nonce_suffix)
                ));
                sent_datagrams.push(next_datagram_number);
                next_datagram_number = next_datagram_number.wrapping_add(1) & UINT24_MASK;
                kdf_diagnostic_count += 1;

                if let Some(prefix) = diagnostic_exact_reply2_prefix.as_deref() {
                    let diagnostic = send_connection_request(
                        socket,
                        peer,
                        prefix,
                        &mut crypto,
                        features,
                        config.mtu,
                        next_datagram_number,
                        reliable_message_number,
                        &request_payload,
                        true,
                        0,
                    )?;
                    last_wire_bytes = diagnostic.wire_bytes;
                    send_traces.push(format!(
                        "online diagnostic exact Reply2 outer-header datagram {next_datagram_number} ({}-byte RUPP), {} bytes, padding {}, nonce suffix [{}]",
                        prefix.len(),
                        diagnostic.wire_bytes,
                        diagnostic.extra_padding,
                        hex_bytes(&diagnostic.nonce_suffix)
                    ));
                    sent_datagrams.push(next_datagram_number);
                    next_datagram_number = next_datagram_number.wrapping_add(1) & UINT24_MASK;
                    route_diagnostic_count += 1;
                }

                if let Some(prefix) = diagnostic_refreshed_prefix.as_deref() {
                    let diagnostic = send_connection_request(
                        socket,
                        peer,
                        prefix,
                        &mut crypto,
                        features,
                        config.mtu,
                        next_datagram_number,
                        reliable_message_number,
                        &request_payload,
                        true,
                        0,
                    )?;
                    last_wire_bytes = diagnostic.wire_bytes;
                    send_traces.push(format!(
                        "online diagnostic refreshed endpoint-bearing token datagram {next_datagram_number}, {} bytes, padding {}, nonce suffix [{}]",
                        diagnostic.wire_bytes,
                        diagnostic.extra_padding,
                        hex_bytes(&diagnostic.nonce_suffix)
                    ));
                    sent_datagrams.push(next_datagram_number);
                    next_datagram_number = next_datagram_number.wrapping_add(1) & UINT24_MASK;
                    route_diagnostic_count += 1;
                }

                let diagnostic = send_connection_request(
                    socket,
                    peer,
                    &[],
                    &mut crypto,
                    features,
                    config.mtu,
                    next_datagram_number,
                    reliable_message_number,
                    &request_payload,
                    true,
                    0,
                )?;
                last_wire_bytes = diagnostic.wire_bytes;
                send_traces.push(format!(
                    "online diagnostic flow-affinity datagram {next_datagram_number} (no RUPP), {} bytes, padding {}, nonce suffix [{}]",
                    diagnostic.wire_bytes,
                    diagnostic.extra_padding,
                    hex_bytes(&diagnostic.nonce_suffix)
                ));
                sent_datagrams.push(next_datagram_number);
                next_datagram_number = next_datagram_number.wrapping_add(1) & UINT24_MASK;
                route_diagnostic_count += 1;
            }

            rto = (rto * 2).min(Duration::from_millis(MAX_RTO_MS));
            next_retransmit = Instant::now() + rto;
        }

        let wake = if request_acked {
            deadline
        } else {
            std::cmp::min(deadline, next_retransmit)
        };
        let remaining = wake
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::from_millis(1))
            .max(Duration::from_millis(1));
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|error| format!("failed to set connected RakNet timeout: {error}"))?;

        let (wire_length, source) = match socket.recv_from(&mut receive_buf) {
            Ok(received) => received,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(format!("connected RakNet receive failed: {error}")),
        };
        if source != peer {
            errors.push(format!("ignored {wire_length} bytes from unexpected peer {source}"));
            continue;
        }

        let (encrypted, returned_token) = match strip_optional_rupp_prefix(&receive_buf[..wire_length]) {
            Ok(value) => value,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        if let Some(token) = returned_token {
            match update_outbound_rupp_token(&mut config.rupp_prefix, token) {
                Ok(true) => refreshed_rupp_tokens += 1,
                Ok(false) => {}
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            }
        }
        let (plaintext, crypto_path) = match crypto.decrypt(encrypted) {
            Ok(value) => (value, 0u8),
            Err(blake_error) => match sha512_first_rx_crypto.decrypt(encrypted) {
                Ok(value) => (value, 1u8),
                Err(sha_first_rx_error) => match sha512_first_tx_crypto.decrypt(encrypted) {
                    Ok(value) => (value, 2u8),
                    Err(sha_first_tx_error) => {
                        errors.push(format!(
                            "connected RakNet authentication failed for all KDF candidates (BLAKE2b: {blake_error}; SHA-512 first-half RX: {sha_first_rx_error}; SHA-512 first-half TX: {sha_first_tx_error})"
                        ));
                        continue;
                    }
                },
            },
        };
        let datagram = match parse_datagram(&plaintext, features) {
            Ok(value) => value,
            Err(error) => {
                errors.push(format!("connected RakNet datagram decode failed: {error}"));
                continue;
            }
        };

        match datagram {
            Datagram::Ack(ack) => {
                if ranges_ack_any(&ack.ranges, &sent_datagrams) {
                    request_acked = true;
                }
            }
            Datagram::Nak(nak) => {
                if !request_acked && ranges_ack_any(&nak.ranges, &sent_datagrams) {
                    next_retransmit = Instant::now();
                }
            }
            Datagram::Data(data) => {
                match crypto_path {
                    0 => send_ack(
                        socket,
                        peer,
                        &config.rupp_prefix,
                        &mut crypto,
                        features,
                        config.mtu,
                        data.datagram_number,
                    )?,
                    1 => send_ack(
                        socket,
                        peer,
                        &config.rupp_prefix,
                        &mut sha512_first_rx_crypto,
                        features,
                        config.mtu,
                        data.datagram_number,
                    )?,
                    2 => send_ack(
                        socket,
                        peer,
                        &config.rupp_prefix,
                        &mut sha512_first_tx_crypto,
                        features,
                        config.mtu,
                        data.datagram_number,
                    )?,
                    _ => unreachable!(),
                }
                let missing = datagram_tracker.observe(data.datagram_number);
                match crypto_path {
                    0 => send_nak(
                        socket,
                        peer,
                        &config.rupp_prefix,
                        &mut crypto,
                        features,
                        config.mtu,
                        missing,
                    )?,
                    1 => send_nak(
                        socket,
                        peer,
                        &config.rupp_prefix,
                        &mut sha512_first_rx_crypto,
                        features,
                        config.mtu,
                        missing,
                    )?,
                    2 => send_nak(
                        socket,
                        peer,
                        &config.rupp_prefix,
                        &mut sha512_first_tx_crypto,
                        features,
                        config.mtu,
                        missing,
                    )?,
                    _ => unreachable!(),
                }
                for packet in data.packets {
                    let delivered = match reliability.receive(packet) {
                        Ok(value) => value,
                        Err(error) => {
                            errors.push(error);
                            continue;
                        }
                    };
                    for message in delivered {
                        if message.bit_length < 8
                            || message.payload.first() != Some(&ID_CONNECTION_REQUEST_ACCEPTED)
                        {
                            continue;
                        }
                        let accepted = parse_connection_accepted(&message.payload)?;
                        if accepted.request_time != request_time {
                            return Err(format!(
                                "ID_CONNECTION_REQUEST_ACCEPTED echoed request time {} instead of {request_time}",
                                accepted.request_time
                            ));
                        }
                        let (session_kdf, tx_nonce, rx_nonce) = match crypto_path {
                            0 => ("BLAKE2b (0.735 crypto_kx)", crypto.tx_nonce, crypto.rx_nonce),
                            1 => (
                                "SHA-512 (first digest half server-to-client)",
                                sha512_first_rx_crypto.tx_nonce,
                                sha512_first_rx_crypto.rx_nonce,
                            ),
                            2 => (
                                "SHA-512 (first digest half client-to-server)",
                                sha512_first_tx_crypto.tx_nonce,
                                sha512_first_tx_crypto.rx_nonce,
                            ),
                            _ => unreachable!(),
                        };
                        return Ok(ConnectionAcceptedSummary {
                            source,
                            wire_bytes: wire_length,
                            session_kdf,
                            client_address: accepted.client_address,
                            system_index: accepted.system_index,
                            internal_address_count: accepted.internal_address_count,
                            request_time: accepted.request_time,
                            server_time: accepted.server_time,
                            server_epoch_time_us: accepted.server_epoch_time_us,
                            datagram_number: data.datagram_number,
                            reliability: message.reliability,
                            request_acked,
                            retransmissions,
                            refreshed_rupp_tokens,
                            tx_nonce,
                            rx_nonce,
                            elapsed_ms: started.elapsed().as_millis(),
                        });
                    }
                }
            }
        }
    }

    let detail = if errors.is_empty() {
        String::new()
    } else {
        format!("; receive diagnostics: {}", errors.join("; "))
    };
    let kdf_state = format!(
        "current-build KDF state: {kdf_diagnostic_count} SHA-512 orientation diagnostic(s) also received no usable response; the authenticated early channel still proves its separate BLAKE2b derivation"
    );
    let route_state = match config.deferred_reply2_rupp_token_type {
        Some(token_type) => format!(
            "native-matched route state: the initial subtype-1 GameService token remained on normal outbound online packets; OpenReply2 advertised outer token subtype {token_type} in a {}-byte exact header, whose installation is deferred until an online packet resolves the active remote; after native retries failed, {route_diagnostic_count} one-shot online route diagnostic(s) also received no usable response",
            config.deferred_reply2_rupp_prefix.as_ref().map(Vec::len).unwrap_or(0)
        ),
        None => format!(
            "native-matched route state: the initial GameService token remained on normal outbound online packets; OpenReply2 advertised no outer token update; after native retries failed, {route_diagnostic_count} one-shot online route diagnostic(s) also received no usable response"
        ),
    };
    Err(format!(
        "sent {last_wire_bytes}-byte encrypted reliable ID_CONNECTION_REQUEST in {} datagram(s), but no ID_CONNECTION_REQUEST_ACCEPTED arrived in {} ms (request ACKed: {request_acked}, BLAKE2b tx nonce {}, rx nonce {}){detail}; sends: {}; {first_wire_trace}; {kdf_state}; {route_state}",
        sent_datagrams.len(),
        started.elapsed().as_millis(),
        crypto.tx_nonce,
        crypto.rx_nonce,
        send_traces.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_request_appends_current_cloud_edit_password() {
        let guid = 0x0102_0304_0506_0708;
        let (payload, request_time) = connection_request_payload(guid);

        assert_eq!(payload.len(), 20);
        assert_eq!(payload[0], ID_CONNECTION_REQUEST);
        assert_eq!(&payload[1..9], &guid.to_be_bytes());
        assert_eq!(&payload[9..17], &request_time.to_be_bytes());
        assert_eq!(payload[17], 0);
        assert_eq!(&payload[18..], &[0x5e, 0x11]);
    }

    #[test]
    fn connected_crypto_starts_with_native_unique_number_nonce() {
        let server_to_client = [0x22; 32];
        let client_to_server = [0x11; 32];
        let mut crypto = SessionCrypto::new(server_to_client, client_to_server);
        let plaintext = b"connected-raknet";
        let wire = crypto.encrypt(plaintext).unwrap();
        assert_eq!(wire.len(), plaintext.len() + SESSION_AEAD_OVERHEAD);
        assert_eq!(&wire[plaintext.len()..plaintext.len() + 2], b"Un");

        let nonce = rak_nonce(INITIAL_RAK_NONCE_COUNTER);
        assert_eq!(&nonce, b"UniqueNumbeR");
        let mut decoded = wire[..plaintext.len()].to_vec();
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&client_to_server));
        cipher
            .decrypt_in_place_detached(
                Nonce::from_slice(&nonce),
                &[],
                &mut decoded,
                Tag::from_slice(&wire[plaintext.len() + 2..]),
            )
            .unwrap();
        assert_eq!(decoded, plaintext);
        assert_eq!(crypto.tx_nonce, INITIAL_RAK_NONCE_COUNTER + 1);
    }

    #[test]
    fn receive_nonce_reconstructs_both_sides_of_wrap() {
        assert_eq!(reconstruct_rx_nonce(0xffff, 0), (0x1_0000, 0x1_0000));
        assert_eq!(reconstruct_rx_nonce(0x1_0000, 0xffff), (0xffff, 0x1_0000));
        assert_eq!(reconstruct_rx_nonce(7, 6), (6, 7));
    }

    #[test]
    fn refreshed_rupp_token_changes_lineage_and_value_only() {
        let mut prefix = vec![1, 0, 0, 31, 1, 17, 1];
        prefix.extend_from_slice(&[0x11; 16]);
        prefix.extend_from_slice(&[2, 6, 10, 0, 0, 1, 0x1f, 0x90]);
        let endpoint = prefix[23..].to_vec();
        assert!(
            update_outbound_rupp_token(
                &mut prefix,
                ReceivedRuppToken {
                    token_type: 2,
                    value: [0x22; 16],
                },
            )
            .unwrap()
        );
        assert_eq!(prefix[6], 2);
        assert_eq!(&prefix[7..23], &[0x22; 16]);
        assert_eq!(&prefix[23..], endpoint);
    }

    #[test]
    fn wire_diagnostics_redact_refreshed_rupp_token() {
        let mut prefix = vec![1, 0, 0, 31, 1, 17, 2];
        prefix.extend_from_slice(&[0x22; 16]);
        prefix.extend_from_slice(&[2, 6, 10, 0, 0, 1, 0x1f, 0x90]);
        let diagnostic = redacted_rupp_hex(&prefix);
        assert!(diagnostic.starts_with("01 00 00 1f 01 11 02 ** **"));
        assert!(diagnostic.ends_with("02 06 0a 00 00 01 1f 90"));
        assert!(!diagnostic.contains("22 22"));
    }

    #[test]
    fn split_reassembly_precedes_reliable_order_delivery() {
        let mut receiver = ReliabilityReceiver::new();
        let fragment = |reliable_message_number, index, payload| InternalPacket {
            reliability: PacketReliability::ReliableOrdered,
            data_bit_length: 8,
            reliable_message_number: Some(reliable_message_number),
            sequencing_index: None,
            ordering_index: Some(0),
            ordering_channel: Some(0),
            split: Some(crate::raknet_2022::SplitPacketHeader {
                count: 2,
                id: 7,
                index,
            }),
            payload: vec![payload],
        };
        assert!(receiver.receive(fragment(1, 1, 0xbb)).unwrap().is_empty());
        let delivered = receiver.receive(fragment(0, 0, 0xaa)).unwrap();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].bit_length, 16);
        assert_eq!(delivered[0].payload, vec![0xaa, 0xbb]);
    }

    #[test]
    fn parses_current_connection_accepted_payload() {
        fn push_address(out: &mut Vec<u8>, ip: [u8; 4], port: u16) {
            out.push(4);
            out.extend(ip.map(|byte| !byte));
            out.extend_from_slice(&port.to_be_bytes());
        }

        let mut payload = vec![ID_CONNECTION_REQUEST_ACCEPTED];
        push_address(&mut payload, [203, 0, 113, 9], 49_152);
        payload.extend_from_slice(&3u16.to_be_bytes());
        for index in 0..10u8 {
            push_address(&mut payload, [10, 0, 0, index], 0);
        }
        payload.extend_from_slice(&11u64.to_be_bytes());
        payload.extend_from_slice(&22u64.to_be_bytes());
        payload.extend_from_slice(&33u64.to_be_bytes());
        let accepted = parse_connection_accepted(&payload).unwrap();
        assert_eq!(accepted.client_address, "203.0.113.9:49152");
        assert_eq!(accepted.system_index, 3);
        assert_eq!(accepted.internal_address_count, 10);
        assert_eq!(accepted.request_time, 11);
        assert_eq!(accepted.server_time, 22);
        assert_eq!(accepted.server_epoch_time_us, 33);
    }
}
