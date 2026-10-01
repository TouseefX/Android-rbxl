//! Team Create (cloud edit) client — Stage 1 groundwork.
//!
//! The REST control plane and join negotiation live in `roblox_api.rs`
//! (see team-create-sessions-explained.md §1a/§1b). This module owns what
//! comes AFTER a successful `POST gamejoin.roblox.com/v1/team-create`:
//!
//! - parsing the returned join config into concrete UDP endpoints
//!   (`Address`/`Port`, `ServerPort` + `UdmuxEndpoints`, or the classic
//!   joinScript `MachineAddress`/`ServerPort` shapes — the decompile's
//!   `fillServerConnectionsArrayFromConfig` accepts several), and
//! - transport selection after the join config is decoded. Legacy RakNet
//!   configs still use Roblox's customized 2022 RUPP-routed
//!   `RbxOpenRequest1`/`RbxOpenReply1` flow followed by encrypted
//!   `RbxOpenRequest2`/`RbxOpenReply2`; configs that satisfy the current
//!   0.741 RbxTransport selector are now mapped onto the QUIC-first
//!   RbxTransport path instead of being forced through the RakNet probe.
//!
//! The legacy RakNet probe can continue through normal SessionCrypto,
//! reliable `ID_CONNECTION_REQUEST`, encrypted ACK/NAK handling, and
//! `ID_CONNECTION_REQUEST_ACCEPTED`. JoinData → live change-item application
//! remains the next Stage 1 layer after the selected transport is live.

use crate::connected_raknet::{
    establish_connected_session, initialize_raknet_time, ConnectedConfig,
    DiagnosticSessionKeyCandidate, SessionCipher,
};
use blake2::{
    digest::{consts::U32, Mac},
    Blake2b512, Blake2bMac, Digest,
};
use chacha20poly1305::{
    aead::{AeadInPlace, KeyInit},
    ChaCha20Poly1305, Key, Nonce, Tag,
};
use sha2::Sha512;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub address: String,
    pub port: u16,
}

impl Endpoint {
    pub fn label(&self) -> String {
        match self.address.parse::<IpAddr>() {
            Ok(IpAddr::V6(_)) => format!("[{}]:{}", self.address, self.port),
            _ => format!("{}:{}", self.address, self.port),
        }
    }

    fn bind_address(&self) -> &'static str {
        match self.address.parse::<IpAddr>() {
            Ok(IpAddr::V6(_)) => "[::]:0",
            _ => "0.0.0.0:0",
        }
    }
}

/// Case-insensitive JSON object field lookup (gamejoin responses have used
/// both PascalCase and camelCase across fleet versions).
fn get_ci<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    let obj = v.as_object()?;
    if let Some(x) = obj.get(key) {
        return Some(x);
    }
    obj.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, x)| x)
}

fn as_port(v: &serde_json::Value) -> Option<u16> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
}

fn as_addr(v: &serde_json::Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn diagnostic_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(value) if value.len() <= 64 => value.clone(),
        serde_json::Value::String(value) => format!("string({} chars)", value.len()),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Array(value) => format!("array({} items)", value.len()),
        serde_json::Value::Object(value) => format!("object({} fields)", value.len()),
    }
}

fn diagnostic_number_with_hex(v: &serde_json::Value) -> String {
    let value = v
        .as_u64()
        .or_else(|| v.as_str().and_then(|text| text.trim().parse::<u64>().ok()));
    match value {
        Some(number) => format!("{number} (0x{number:08x})"),
        None => diagnostic_scalar(v),
    }
}

fn diagnostic_token_value_shape(v: &serde_json::Value) -> String {
    match v.as_str() {
        Some(text) => {
            let mut shape = format!("string({} chars)", text.len());
            if decode_token_16(text).is_some() {
                shape.push_str(", resolves to redacted 16-byte token");
            } else if let Some(decoded) = decode_base64(text) {
                shape.push_str(&format!(", Base64-decodes {} bytes", decoded.len()));
            } else {
                shape.push_str(", does not decode as a 16-byte token");
            }
            shape
        }
        None => diagnostic_scalar(v),
    }
}

fn diagnostic_base64_secret_shape(v: &serde_json::Value) -> String {
    match v.as_str() {
        Some(text) => match decode_base64(text) {
            Some(decoded) => format!(
                "string({} chars), Base64-decodes {} bytes",
                text.len(),
                decoded.len()
            ),
            None => format!("string({} chars), Base64 decode failed", text.len()),
        },
        None => diagnostic_scalar(v),
    }
}

/// Collect endpoints from ONE object level: direct `Address`+`Port`,
/// `MachineAddress`+`ServerPort` (classic join script), and the
/// `UdmuxEndpoints` array (per-entry `Port` falling back to `ServerPort`).
fn collect_level(v: &serde_json::Value, out: &mut Vec<Endpoint>) {
    let server_port = get_ci(v, "ServerPort").and_then(as_port);

    // UDMUX multiplexed endpoints come first: when present they are the
    // addresses the client is expected to use.
    if let Some(arr) = get_ci(v, "UdmuxEndpoints").and_then(|u| u.as_array()) {
        for e in arr {
            let addr = get_ci(e, "Address").and_then(as_addr);
            let port = get_ci(e, "Port").and_then(as_port).or(server_port);
            if let (Some(address), Some(port)) = (addr, port) {
                push_unique(out, Endpoint { address, port });
            }
        }
    }

    // Some Team Create fleet versions pair Address with ServerPort instead
    // of Port, so accept either at the same object level.
    if let (Some(address), Some(port)) = (
        get_ci(v, "Address").and_then(as_addr),
        get_ci(v, "Port").and_then(as_port).or(server_port),
    ) {
        push_unique(out, Endpoint { address, port });
    }

    if let (Some(address), Some(port)) = (
        get_ci(v, "MachineAddress").and_then(as_addr),
        server_port,
    ) {
        push_unique(out, Endpoint { address, port });
    }
}

fn push_unique(out: &mut Vec<Endpoint>, e: Endpoint) {
    if !out.contains(&e) {
        out.push(e);
    }
}

/// Walk wrapper objects used by different gamejoin fleet versions. Team
/// Create responses have placed the same config under `joinScript`,
/// `settings`, and `joinTicket`; limiting parsing to one named wrapper makes
/// a valid response look endpoint-less. A small depth cap also lets us read
/// wrappers serialized as JSON strings without accepting unbounded input.
fn collect_recursive(v: &serde_json::Value, out: &mut Vec<Endpoint>, depth: usize) {
    if depth > 12 {
        return;
    }
    match v {
        serde_json::Value::Object(map) => {
            collect_level(v, out);
            for child in map.values() {
                collect_recursive(child, out, depth + 1);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_recursive(child, out, depth + 1);
            }
        }
        serde_json::Value::String(text) => {
            let text = text.trim();
            if (text.starts_with('{') || text.starts_with('[')) && text.len() <= 2_000_000 {
                if let Ok(decoded) = serde_json::from_str::<serde_json::Value>(text) {
                    collect_recursive(&decoded, out, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn parse_all_join_endpoints(v: &serde_json::Value) -> Vec<Endpoint> {
    let mut out = Vec::new();
    collect_recursive(v, &mut out, 0);
    out
}

/// Find a field through the same response wrappers accepted by the endpoint
/// parser. Returning an owned value also permits wrappers encoded as JSON
/// strings without borrowing a temporary decoded document.
fn find_field_ci(v: &serde_json::Value, key: &str, depth: usize) -> Option<serde_json::Value> {
    if depth > 12 {
        return None;
    }
    match v {
        serde_json::Value::Object(map) => {
            if let Some((_, value)) = map.iter().find(|(name, _)| name.eq_ignore_ascii_case(key)) {
                return Some(value.clone());
            }
            map.values()
                .find_map(|child| find_field_ci(child, key, depth + 1))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|child| find_field_ci(child, key, depth + 1)),
        serde_json::Value::String(text) => {
            let text = text.trim();
            if (text.starts_with('{') || text.starts_with('[')) && text.len() <= 2_000_000 {
                serde_json::from_str::<serde_json::Value>(text)
                    .ok()
                    .and_then(|decoded| find_field_ci(&decoded, key, depth + 1))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn decode_hex_16(text: &str) -> Option<[u8; 16]> {
    let text = text.trim();
    if text.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Minimal standard/URL-safe Base64 decoder. The 2022 join configuration
/// uses Base64 for the RUPP token, key-ring entries, and both early-auth
/// blobs. Keeping one strict local decoder avoids a native Android dependency.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(text.len().saturating_mul(3) / 4 + 3);
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut saw_padding = false;
    for byte in text.trim().bytes() {
        let value = match byte {
            b'A'..=b'Z' if !saw_padding => byte - b'A',
            b'a'..=b'z' if !saw_padding => byte - b'a' + 26,
            b'0'..=b'9' if !saw_padding => byte - b'0' + 52,
            b'+' | b'-' if !saw_padding => 62,
            b'/' | b'_' if !saw_padding => 63,
            b'=' => {
                saw_padding = true;
                continue;
            }
            b' ' | b'\r' | b'\n' | b'\t' => continue,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        bit_count += 6;
        if bit_count >= 8 {
            bit_count -= 8;
            decoded.push((bits >> bit_count) as u8);
            bits &= (1u32 << bit_count).wrapping_sub(1);
        }
    }
    Some(decoded)
}

/// Match the `RBX::Url::urlDecode` step used by Studio before it installs
/// `EphemeralEarlyPubKey` into the legacy RakNet KeyRing application. This is
/// percent decoding, not form decoding: an unescaped `+` remains the Base64
/// alphabet character rather than becoming a space.
fn url_percent_decode(text: &str) -> Result<String, String> {
    fn hex_nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let input = text.trim().as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0usize;
    while index < input.len() {
        if input[index] == b'%' {
            if index + 2 >= input.len() {
                return Err("EphemeralEarlyPubKey ends with an incomplete percent escape".into());
            }
            let high = hex_nibble(input[index + 1]).ok_or_else(|| {
                format!(
                    "EphemeralEarlyPubKey has an invalid percent escape at byte {index}"
                )
            })?;
            let low = hex_nibble(input[index + 2]).ok_or_else(|| {
                format!(
                    "EphemeralEarlyPubKey has an invalid percent escape at byte {index}"
                )
            })?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(output)
        .map_err(|_| "URL-decoded EphemeralEarlyPubKey is not UTF-8 Base64 text".to_string())
}

fn decode_base64_16(text: &str) -> Option<[u8; 16]> {
    decode_base64(text)?.try_into().ok()
}

fn decode_token_16(text: &str) -> Option<[u8; 16]> {
    decode_base64_16(text)
        .or_else(|| decode_hex_16(text))
        .or_else(|| <[u8; 16]>::try_from(text.as_bytes()).ok())
}

fn is_internal_address(address: &str) -> bool {
    fn v4_is_internal(o: [u8; 4]) -> bool {
        o[0] == 0
            || o[0] == 10
            || o[0] == 127
            || (o[0] == 169 && o[1] == 254)
            || (o[0] == 172 && (16..=31).contains(&o[1]))
            || (o[0] == 192 && o[1] == 168)
            || (o[0] == 100 && (64..=127).contains(&o[1]))
    }

    match address.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => v4_is_internal(ip.octets()),
        Ok(IpAddr::V6(ip)) => {
            let o = ip.octets();
            ip.is_unspecified()
                || ip.is_loopback()
                || (o[0] & 0xfe) == 0xfc // fc00::/7 unique-local
                || (o[0] == 0xfe && (o[1] & 0xc0) == 0x80) // fe80::/10 link-local
                || (o[..12] == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff]
                    && v4_is_internal([o[12], o[13], o[14], o[15]]))
        }
        // A hostname may be publicly routable; do not discard it.
        Err(_) => false,
    }
}

fn prefer_public_endpoints(all: Vec<Endpoint>) -> Vec<Endpoint> {
    if all.iter().any(|e| !is_internal_address(&e.address)) {
        all.into_iter()
            .filter(|e| !is_internal_address(&e.address))
            .collect()
    } else {
        all
    }
}

/// Extract usable server endpoints from a team-create response, independent
/// of which response wrapper contains the config. If Roblox supplies public
/// UDMUX and private RCC addresses together, keep the public targets: a phone
/// on the Internet cannot route to addresses such as 10.x.x.x.
pub fn parse_join_config(v: &serde_json::Value) -> Vec<Endpoint> {
    prefer_public_endpoints(parse_all_join_endpoints(v))
}

/// True when a response contains no value other than nulls/empty containers.
/// HTTP 2xx with an all-null gamejoin object is a failed negotiation, not a
/// usable config and must not enable the UDP probe.
pub fn join_response_is_all_null(v: &serde_json::Value) -> bool {
    fn has_value(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Null => false,
            serde_json::Value::Array(items) => items.iter().any(has_value),
            serde_json::Value::Object(map) => map.values().any(has_value),
            serde_json::Value::String(s) => !s.trim().is_empty(),
            serde_json::Value::Bool(_) | serde_json::Value::Number(_) => true,
        }
    }
    !has_value(v)
}

/// RakNet offline-message magic, retained by Roblox's customized open
/// handshake. The layouts below come from the exact 2022 Studio
/// `sendRbxOpenRequest1`, `sendRbxOpenReply1`, and
/// `processRbxOpenReply1` bodies.
const OFFLINE_MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56,
    0x78,
];

pub const RBX_OPEN_REQUEST_1: u8 = 0x7b;
pub const RBX_OPEN_REPLY_1: u8 = 0x7e;
pub const RBX_OPEN_REQUEST_2: u8 = 0x78;
pub const RBX_OPEN_REPLY_2: u8 = 0x7d;
pub const RBX_PROTOCOL_VERSION: u8 = 5;
const RBX_OPEN_REQUEST_2_VERSION: u8 = 3;
// PlayerConfigurer directly emits the URL-decoded legacy RakNet key at
// id/send/revert 5. Studio 0.735 uses the same application and generated
// config shape; its R8D helper argument is omitted by the pseudocode export,
// so 5 is the evidence-backed paired-path value rather than a visible Studio
// call-site literal. RbxTransportEphemeralEarlyPublicKey remains separate at 1.
const RAKNET_EPHEMERAL_EARLY_KEY_VERSION: u16 = 5;
pub const DEFAULT_PROBE_MTU: u16 = 1200;
const IPV6_UDP_HEADER_BYTES: usize = 40;
const EARLY_AEAD_OVERHEAD: usize = 28; // 12-byte nonce + 16-byte detached tag
// Current 0.735 `buildCapabilitiesHelper(..., PeerType::Client)` always sets
// these bits. Additional bits are runtime-flag gated; do not advertise those
// until the corresponding behavior is implemented. The old 2022 constant
// falsely negotiated server bits 19 and 25 in the latest live session.
const RAK_PEER_CAPABILITIES_0735_CLIENT_FLOOR: u64 = 0x0000_0203_58b7_eafa;

// Exact current RUPP values from Rupp::{serialize,TokenTlv,Ipv4Tlv,Ipv6Tlv}. 0.735
// retains the same wire values used by the earlier transport implementation.

const RUPP_PROTOCOL_RAKNET: u8 = 1;
const RUPP_FLAG_DIRECT_SERVER_RETURN: u8 = 1;
const RUPP_TLV_TOKEN: u8 = 1;
const RUPP_TLV_IPV4: u8 = 2;
const RUPP_TLV_IPV6: u8 = 3;
const RUPP_TOKEN_VALUE_LENGTH: u8 = 17; // subtype byte + 16-byte token

#[derive(Clone, Debug)]
struct RuppConnectedRouteMaterial {
    token: [u8; 16],
    rcc_endpoint: Endpoint,
}

#[derive(Clone, Debug)]
struct RuppProbeMaterial {
    token: [u8; 16],
    rcc_endpoint: Endpoint,
    direct_server_return: bool,
    /// Native Team Create parses a separate `NetStackTokenValue` and
    /// `NetStackPort` into the optional RbxTransport/RUPP client
    /// configuration. OpenRequest1/OpenRequest2 still use `TokenValue`, but
    /// connected traffic should prefer this route when the config supplies it.
    connected_route: Option<RuppConnectedRouteMaterial>,
}


#[derive(Clone, Debug)]
struct RbxTransportEarlyKeyMaterial {
    source: String,
    version: u16,
    public_key: [u8; 32],
}

#[derive(Clone, Debug)]
struct RbxTransportConnectPlan {
    public_endpoint: Endpoint,
    rcc_endpoint: Endpoint,
    rbx_transport_port: u16,
    token: [u8; 16],
    token_shape: String,
    early_key: RbxTransportEarlyKeyMaterial,
    game_fqdn: Option<String>,
}

impl RbxTransportConnectPlan {
    fn summary(&self) -> String {
        let mut text = format!(
            "\nRbxTransport/QUIC remap ready: runtime flags FFlagUseRbxTransport + FFlagStudioClientServerMDI2 are treated as enabled, so the 0.741 selector maps this Team Create config to selectedTransport=RbxTransport (NetStack port/address/pubkey all present).\nRbxTransport selected UDP/UDMUX target: {}\nRbxTransport RCC/RUPP config: RCC {} with NetStackPort {}, NetStackTokenValue {} ({} decoded bytes)\nRbxTransport early pubkey: {} version {}, {} bytes",
            self.public_endpoint.label(),
            self.rcc_endpoint.label(),
            self.rbx_transport_port,
            self.token_shape,
            self.token.len(),
            self.early_key.source,
            self.early_key.version,
            self.early_key.public_key.len(),
        );
        if let Some(game_fqdn) = &self.game_fqdn {
            text.push_str(&format!("\nRbxTransport GameFqdn: {game_fqdn}"));
        }
        text.push_str(
            "\nLegacy RakNet connected packets are intentionally skipped for this config; the remaining implementation target is the RbxTransport QUIC/BaseClient auth + channel-open path, not another RakNet route/KDF matrix.",
        );
        text
    }
}

fn parse_rbx_transport_key_ring_app(
    config: &serde_json::Value,
) -> Result<RbxTransportEarlyKeyMaterial, String> {
    let key_ring_text = find_field_ci(config, "ClientPublicKeyData", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| "ClientPublicKeyData absent".to_string())?;
    let key_ring: serde_json::Value = serde_json::from_str(&key_ring_text)
        .map_err(|error| format!("ClientPublicKeyData is not valid KeyRing JSON: {error}"))?;
    let applications = get_ci(&key_ring, "applications")
        .ok_or_else(|| "ClientPublicKeyData has no applications object".to_string())?;
    let application = get_ci(applications, "RbxTransportEphemeralEarlyPublicKey")
        .ok_or_else(|| {
            "ClientPublicKeyData has no RbxTransportEphemeralEarlyPublicKey application".to_string()
        })?;
    let send_version = get_ci(application, "send")
        .and_then(json_u16)
        .ok_or_else(|| {
            "RbxTransportEphemeralEarlyPublicKey has no valid send version".to_string()
        })?;
    let versions = get_ci(application, "versions")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "RbxTransportEphemeralEarlyPublicKey has no versions array".to_string())?;
    let selected = versions
        .iter()
        .find(|entry| get_ci(entry, "id").and_then(json_u16) == Some(send_version))
        .ok_or_else(|| {
            format!("RbxTransportEphemeralEarlyPublicKey send version {send_version} is absent")
        })?;
    if !get_ci(selected, "allowed")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return Err(format!(
            "RbxTransportEphemeralEarlyPublicKey send version {send_version} is not production-allowed"
        ));
    }
    let value = get_ci(selected, "value")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            format!("RbxTransportEphemeralEarlyPublicKey version {send_version} has no value")
        })?;
    let decoded = decode_base64(value).ok_or_else(|| {
        format!("RbxTransportEphemeralEarlyPublicKey version {send_version} is not valid Base64")
    })?;
    let public_key: [u8; 32] = decoded.try_into().map_err(|decoded: Vec<u8>| {
        format!(
            "RbxTransportEphemeralEarlyPublicKey version {send_version} is {} bytes, not exactly 32",
            decoded.len()
        )
    })?;
    Ok(RbxTransportEarlyKeyMaterial {
        source: "ClientPublicKeyData/RbxTransportEphemeralEarlyPublicKey".into(),
        version: send_version,
        public_key,
    })
}

fn extract_rbx_transport_early_key(
    config: &serde_json::Value,
) -> Result<RbxTransportEarlyKeyMaterial, String> {
    if let Some(ephemeral_value) = find_field_ci(config, "EphemeralEarlyPubKey", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.is_empty())
    {
        let decoded_text = url_percent_decode(&ephemeral_value)?;
        let decoded = decode_base64(&decoded_text)
            .ok_or_else(|| "URL-decoded EphemeralEarlyPubKey is not valid Base64".to_string())?;
        let public_key: [u8; 32] = decoded.try_into().map_err(|decoded: Vec<u8>| {
            format!(
                "URL-decoded EphemeralEarlyPubKey is {} bytes, not exactly 32",
                decoded.len()
            )
        })?;
        return Ok(RbxTransportEarlyKeyMaterial {
            source: "URL-decoded EphemeralEarlyPubKey override".into(),
            version: 1,
            public_key,
        });
    }

    parse_rbx_transport_key_ring_app(config)
}

fn extract_rbx_transport_connect_plan(
    config: &serde_json::Value,
) -> Result<RbxTransportConnectPlan, String> {
    let all_endpoints = parse_all_join_endpoints(config);
    let public_endpoint = prefer_public_endpoints(all_endpoints.clone())
        .into_iter()
        .next()
        .ok_or_else(|| "no public UDP/UDMUX endpoint found".to_string())?;
    let rcc_endpoint = all_endpoints
        .iter()
        .find(|endpoint| is_internal_address(&endpoint.address))
        .cloned()
        .or_else(|| all_endpoints.first().cloned())
        .ok_or_else(|| "no RCC endpoint found".to_string())?;
    let rbx_transport_port = find_field_ci(config, "NetStackPort", 0)
        .or_else(|| find_field_ci(config, "RbxTransportPort", 0))
        .and_then(|value| as_port(&value))
        .ok_or_else(|| "NetStackPort/RbxTransportPort absent or zero".to_string())?;
    let token_value = find_field_ci(config, "NetStackTokenValue", 0)
        .or_else(|| find_field_ci(config, "RbxTransportToken", 0))
        .ok_or_else(|| "NetStackTokenValue/RbxTransportToken absent".to_string())?;
    let token_text = token_value
        .as_str()
        .ok_or_else(|| "NetStackTokenValue/RbxTransportToken is not a string".to_string())?;
    let token = decode_token_16(token_text).ok_or_else(|| {
        "NetStackTokenValue/RbxTransportToken is not a 16-byte raw, hexadecimal, or Base64 token"
            .to_string()
    })?;
    let early_key = extract_rbx_transport_early_key(config)?;
    let game_fqdn = find_field_ci(config, "GameFqdn", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.trim().is_empty());
    Ok(RbxTransportConnectPlan {
        public_endpoint,
        rcc_endpoint,
        rbx_transport_port,
        token,
        token_shape: diagnostic_token_value_shape(&token_value),
        early_key,
        game_fqdn,
    })
}

fn extract_rupp_probe_material(config: &serde_json::Value) -> Result<RuppProbeMaterial, String> {
    let token_text = find_field_ci(config, "TokenValue", 0)
        .and_then(|v| v.as_str().map(str::to_string))
        .ok_or_else(|| "join config has no TokenValue for the RUPP token TLV".to_string())?;
    let token = decode_token_16(&token_text).ok_or_else(|| {
        "join config TokenValue is not a 16-byte raw, hexadecimal, or Base64 token".to_string()
    })?;
    let all_endpoints = parse_all_join_endpoints(config);
    let rcc_endpoint = all_endpoints
        .iter()
        .find(|endpoint| is_internal_address(&endpoint.address))
        .cloned()
        .or_else(|| all_endpoints.first().cloned())
        .ok_or_else(|| "join config has no RCC endpoint for the RUPP endpoint TLV".to_string())?;
    let direct_server_return = find_field_ci(config, "DirectServerReturn", 0)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let connected_route = match (
        find_field_ci(config, "NetStackTokenValue", 0)
            .and_then(|v| v.as_str().map(str::to_string))
            .and_then(|text| decode_token_16(&text)),
        find_field_ci(config, "NetStackPort", 0).and_then(|v| as_port(&v)),
    ) {
        (Some(token), Some(port)) => Some(RuppConnectedRouteMaterial {
            token,
            rcc_endpoint: Endpoint {
                address: rcc_endpoint.address.clone(),
                port,
            },
        }),
        _ => None,
    };
    Ok(RuppProbeMaterial {
        token,
        rcc_endpoint,
        direct_server_return,
        connected_route,
    })
}

/// Serialize the exact RUPP prefix built by RakPeer::setupRupp: token TLV
/// first, private RCC endpoint TLV second, then a four-byte RUPP envelope.
fn build_rupp_header_for(
    token: &[u8; 16],
    rcc_endpoint: &Endpoint,
    direct_server_return: bool,
    token_type: u8,
) -> Result<Vec<u8>, String> {
    if !(1..=2).contains(&token_type) {
        return Err(format!("unsupported 2022 RUPP token subtype {token_type}"));
    }
    let mut tlvs = Vec::with_capacity(27);
    tlvs.extend_from_slice(&[RUPP_TLV_TOKEN, RUPP_TOKEN_VALUE_LENGTH, token_type]);
    tlvs.extend_from_slice(token);
    match rcc_endpoint.address.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            tlvs.extend_from_slice(&[RUPP_TLV_IPV4, 6]);
            tlvs.extend_from_slice(&ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            tlvs.extend_from_slice(&[RUPP_TLV_IPV6, 18]);
            tlvs.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            return Err(format!(
                "RUPP RCC endpoint must be an IP literal, got {}",
                rcc_endpoint.address
            ));
        }
    }
    tlvs.extend_from_slice(&rcc_endpoint.port.to_be_bytes());
    let header_len = 4usize
        .checked_add(tlvs.len())
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(|| "RUPP header is too long".to_string())?;
    let mut header = Vec::with_capacity(usize::from(header_len));
    header.push(RUPP_PROTOCOL_RAKNET);
    header.push(if direct_server_return {
        RUPP_FLAG_DIRECT_SERVER_RETURN
    } else {
        0
    });
    header.extend_from_slice(&header_len.to_be_bytes());
    header.extend_from_slice(&tlvs);
    Ok(header)
}

/// Serialize the RUPP prefix used by offline OpenRequest1/OpenRequest2.
fn build_rupp_header(
    material: &RuppProbeMaterial,
    token_type: u8,
) -> Result<Vec<u8>, String> {
    build_rupp_header_for(
        &material.token,
        &material.rcc_endpoint,
        material.direct_server_return,
        token_type,
    )
}

/// Serialize the native connected RUPP route. Team Create's 0.735 path reads
/// `NetStackTokenValue`/`NetStackPort` separately for the RbxTransport client
/// configuration; when absent, it falls back to the GameService TokenValue
/// route used by the offline opener.
fn build_connected_rupp_header(
    material: &RuppProbeMaterial,
    token_type: u8,
) -> Result<Vec<u8>, String> {
    if let Some(route) = &material.connected_route {
        build_rupp_header_for(
            &route.token,
            &route.rcc_endpoint,
            material.direct_server_return,
            token_type,
        )
    } else {
        build_rupp_header(material, token_type)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EarlyAuthData {
    auth_version: u8,
    preauth_blob: Vec<u8>,
    auth_blob: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Request2Material {
    early_key_version: u16,
    early_key_send_version: u16,
    early_key_revert_version: u16,
    early_key_uses_revert: bool,
    early_key_hashes_job_id: bool,
    early_key_uses_ephemeral_override: bool,
    server_early_public_key: [u8; 32],
    normal_session_seed: Option<Vec<u8>>,
    auth: EarlyAuthData,
}

/// `Network::Client::setTicket` in the 2022 client splits ClientTicket on
/// semicolons (keeping empty fields). With at least five fields, field 2 is
/// the Base64 pre-auth blob, field 3 is the Base64 auth blob, and the final
/// field is the numeric auth version.
fn parse_early_auth_data(client_ticket: &str) -> Result<EarlyAuthData, String> {
    let fields: Vec<&str> = client_ticket.split(';').collect();
    if fields.len() < 5 {
        return Err(format!(
            "ClientTicket has {} semicolon fields; 2022 early auth requires at least 5",
            fields.len()
        ));
    }
    let auth_version = fields
        .last()
        .and_then(|field| field.trim().parse::<u8>().ok())
        .ok_or_else(|| "ClientTicket has no valid final early-auth version".to_string())?;
    let preauth_blob = decode_base64(fields[2])
        .ok_or_else(|| "ClientTicket pre-auth field is not valid Base64".to_string())?;
    let auth_blob = decode_base64(fields[3])
        .ok_or_else(|| "ClientTicket auth field is not valid Base64".to_string())?;
    if preauth_blob.len() > u8::MAX as usize {
        return Err(format!(
            "ClientTicket pre-auth blob is too large: {} bytes",
            preauth_blob.len()
        ));
    }
    if auth_blob.len() > u8::MAX as usize {
        return Err(format!(
            "ClientTicket auth blob is too large: {} bytes",
            auth_blob.len()
        ));
    }
    Ok(EarlyAuthData {
        auth_version,
        preauth_blob,
        auth_blob,
    })
}

fn json_u16(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
        .and_then(|number| u16::try_from(number).ok())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerEarlyKey {
    version: u16,
    send_version: u16,
    revert_version: u16,
    uses_revert: bool,
    allowed: bool,
    hashes_job_id: bool,
    uses_ephemeral_override: bool,
    bytes: [u8; 32],
}

fn keyed_blake2b_256(key: &[u8], data: &[u8]) -> Result<[u8; 32], String> {
    let mut mac = <Blake2bMac<U32> as Mac>::new_from_slice(key)
        .map_err(|_| "KeyRing key is not a valid BLAKE2b secret".to_string())?;
    Mac::update(&mut mac, data);
    let digest = Mac::finalize(mac).into_bytes();
    let mut output = [0u8; 32];
    output.copy_from_slice(&digest);
    Ok(output)
}

/// Reproduce Studio 0.735's Team Create KeyRing setup. A nonempty
/// `EphemeralEarlyPubKey` takes the entire legacy RakNet branch: Studio URL
/// decodes it, generates a one-version `RakNetEarlyPublicKey` application at
/// the paired-path version 5, and does not parse `ClientPublicKeyData`. Only
/// when the ephemeral value is absent or empty does Studio parse the supplied
/// KeyRing.
/// In that fallback, `parseVersion` conditionally replaces a decoded value
/// with keyed BLAKE2b-256(jobId) when `hashJobId` is true.
fn parse_server_early_key_with_revert(
    config: &serde_json::Value,
    key_ring_revert: bool,
) -> Result<ServerEarlyKey, String> {
    if let Some(ephemeral_value) = find_field_ci(config, "EphemeralEarlyPubKey", 0) {
        let raw_ephemeral = ephemeral_value.as_str().ok_or_else(|| {
            "join config EphemeralEarlyPubKey is present but is not a string".to_string()
        })?;
        if !raw_ephemeral.is_empty() {
            let encoded = url_percent_decode(raw_ephemeral)?;
            let decoded = decode_base64(&encoded).ok_or_else(|| {
                "URL-decoded EphemeralEarlyPubKey is not valid Base64".to_string()
            })?;
            let bytes: [u8; 32] = decoded.try_into().map_err(|decoded: Vec<u8>| {
                format!(
                    "URL-decoded EphemeralEarlyPubKey is {} bytes, not exactly 32",
                    decoded.len()
                )
            })?;
            return Ok(ServerEarlyKey {
                version: RAKNET_EPHEMERAL_EARLY_KEY_VERSION,
                send_version: RAKNET_EPHEMERAL_EARLY_KEY_VERSION,
                revert_version: RAKNET_EPHEMERAL_EARLY_KEY_VERSION,
                uses_revert: key_ring_revert,
                allowed: true,
                hashes_job_id: false,
                uses_ephemeral_override: true,
                bytes,
            });
        }
    }

    let key_ring_text = find_field_ci(config, "ClientPublicKeyData", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| "join config has no ClientPublicKeyData".to_string())?;
    let key_ring: serde_json::Value = serde_json::from_str(&key_ring_text)
        .map_err(|error| format!("ClientPublicKeyData is not valid KeyRing JSON: {error}"))?;
    let applications = get_ci(&key_ring, "applications")
        .ok_or_else(|| "ClientPublicKeyData has no applications object".to_string())?;
    let application = get_ci(applications, "RakNetEarlyPublicKey")
        .ok_or_else(|| "ClientPublicKeyData has no RakNetEarlyPublicKey application".to_string())?;
    let send_version = get_ci(application, "send")
        .and_then(json_u16)
        .ok_or_else(|| "RakNetEarlyPublicKey has no valid send version".to_string())?;
    let revert_version = get_ci(application, "revert")
        .and_then(json_u16)
        .ok_or_else(|| "RakNetEarlyPublicKey has no valid revert version".to_string())?;
    let versions = get_ci(application, "versions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "RakNetEarlyPublicKey has no versions array".to_string())?;
    let send = versions
        .iter()
        .find(|version| get_ci(version, "id").and_then(json_u16) == Some(send_version))
        .ok_or_else(|| format!("RakNetEarlyPublicKey send version {send_version} is absent"))?;
    let revert = versions
        .iter()
        .find(|version| get_ci(version, "id").and_then(json_u16) == Some(revert_version))
        .ok_or_else(|| {
            format!("RakNetEarlyPublicKey revert version {revert_version} is absent")
        })?;
    let send_allowed = get_ci(send, "allowed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !send_allowed {
        // Native parseConfig rejects an application whose normal send version
        // is not production-allowed. A revert version may be disallowed and
        // retained because DFFlag::KeyRingRevert is an emergency bypass.
        return Err(format!(
            "RakNetEarlyPublicKey send version {send_version} is not production-allowed"
        ));
    }
    let (selected_version, selected) = if key_ring_revert {
        (revert_version, revert)
    } else {
        (send_version, send)
    };
    let selected_allowed = get_ci(selected, "allowed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let encoded = get_ci(selected, "value")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("RakNetEarlyPublicKey version {selected_version} has no value"))?;
    let decoded = decode_base64(encoded).ok_or_else(|| {
        format!("RakNetEarlyPublicKey version {selected_version} is not valid Base64")
    })?;
    if decoded.is_empty() {
        return Err(format!(
            "RakNetEarlyPublicKey version {selected_version} decodes to an empty key"
        ));
    }
    let hashes_job_id = get_ci(selected, "hashJobId")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let effective_key = if hashes_job_id {
        let job_id = find_field_ci(config, "GameId", 0)
            .and_then(|value| value.as_str().map(str::to_owned))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                format!(
                    "RakNetEarlyPublicKey version {selected_version} requires a GameId for hashJobId"
                )
            })?;
        keyed_blake2b_256(&decoded, job_id.as_bytes())?
    } else {
        decoded.try_into().map_err(|_| {
            format!("RakNetEarlyPublicKey version {selected_version} is not exactly 32 bytes")
        })?
    };
    Ok(ServerEarlyKey {
        version: selected_version,
        send_version,
        revert_version,
        uses_revert: key_ring_revert,
        allowed: selected_allowed,
        hashes_job_id,
        uses_ephemeral_override: false,
        bytes: effective_key,
    })
}

fn parse_server_early_key(config: &serde_json::Value) -> Result<ServerEarlyKey, String> {
    // DFFlag::KeyRingRevert is false during normal production operation.
    parse_server_early_key_with_revert(config, false)
}

fn extract_request2_material_with_revert(
    config: &serde_json::Value,
    key_ring_revert: bool,
) -> Result<Request2Material, String> {
    let early_key = parse_server_early_key_with_revert(config, key_ring_revert)?;
    if !early_key.allowed {
        // RakPeerCrypto::getClientKeyInfo replaces a disallowed production
        // selection with Studio's compiled-in kPublicEarlyTestKey while
        // retaining the selected version ID. That constant is not supplied by
        // gamejoin, so using the JSON value would be a deterministic mismatch.
        return Err(format!(
            "selected RakNetEarlyPublicKey version {} is disallowed; native Studio would substitute its unavailable compiled-in test key",
            early_key.version
        ));
    }
    let client_ticket = find_field_ci(config, "ClientTicket", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| "join config has no ClientTicket for early authentication".to_string())?;
    let auth = parse_early_auth_data(&client_ticket)?;
    let normal_session_seed = find_field_ci(config, "RandomSeed1", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
        .and_then(|text| decode_base64(&text))
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= 1024);
    Ok(Request2Material {
        early_key_version: early_key.version,
        early_key_send_version: early_key.send_version,
        early_key_revert_version: early_key.revert_version,
        early_key_uses_revert: early_key.uses_revert,
        early_key_hashes_job_id: early_key.hashes_job_id,
        early_key_uses_ephemeral_override: early_key.uses_ephemeral_override,
        server_early_public_key: early_key.bytes,
        normal_session_seed,
        auth,
    })
}

fn extract_request2_material(config: &serde_json::Value) -> Result<Request2Material, String> {
    extract_request2_material_with_revert(config, false)
}

struct Request2Crypto {
    client_secret: StaticSecret,
    client_public: [u8; 32],
    early_server_to_client: [u8; 32],
    early_client_to_server: [u8; 32],
    client_guid: u64,
}

/// Pure-Rust, byte-exact equivalent of libsodium
/// `crypto_kx_client_session_keys`: X25519 followed by BLAKE2b-512 over
/// `shared || client_pk || server_pk`, with RX in the first half and TX in
/// the second half.
fn derive_client_session_keys(
    client_secret: &StaticSecret,
    client_public: &[u8; 32],
    server_public: &[u8; 32],
) -> Result<([u8; 32], [u8; 32]), String> {
    let server_public = PublicKey::from(*server_public);
    let shared = client_secret.diffie_hellman(&server_public);
    if shared.as_bytes().iter().all(|byte| *byte == 0) {
        return Err("X25519 peer public key produced the forbidden all-zero secret".into());
    }
    let mut hash = Blake2b512::new();
    hash.update(shared.as_bytes());
    hash.update(client_public);
    hash.update(server_public.as_bytes());
    let digest = hash.finalize();
    let mut rx = [0u8; 32];
    let mut tx = [0u8; 32];
    rx.copy_from_slice(&digest[..32]);
    tx.copy_from_slice(&digest[32..]);
    Ok((rx, tx))
}

/// A September 2026 current-client static trace reports that normal ephemeral
/// RakNet KX changed from libsodium's BLAKE2b crypto_kx construction to
/// SHA-512 over the same `shared || client public || server public` transcript.
/// The public trace does not yet prove which digest half is each direction, so
/// connected transport tests both orderings only after the 0.735-native path
/// has gone unanswered. Early OpenRequest2 crypto remains BLAKE2b: successful
/// authenticated Reply2 packets directly prove that for the live fleet.
fn derive_client_session_keys_sha512(
    client_secret: &StaticSecret,
    client_public: &[u8; 32],
    server_public: &[u8; 32],
) -> Result<([u8; 32], [u8; 32]), String> {
    derive_client_session_keys_sha512_with_seed(client_secret, client_public, server_public, None, false)
}

fn derive_client_session_keys_sha512_with_seed(
    client_secret: &StaticSecret,
    client_public: &[u8; 32],
    server_public: &[u8; 32],
    seed: Option<&[u8]>,
    seed_first: bool,
) -> Result<([u8; 32], [u8; 32]), String> {
    let server_public = PublicKey::from(*server_public);
    let shared = client_secret.diffie_hellman(&server_public);
    if shared.as_bytes().iter().all(|byte| *byte == 0) {
        return Err("X25519 peer public key produced the forbidden all-zero secret".into());
    }
    let mut hash = Sha512::new();
    if seed_first {
        if let Some(seed) = seed {
            hash.update(seed);
        }
    }
    hash.update(shared.as_bytes());
    hash.update(client_public);
    hash.update(server_public.as_bytes());
    if !seed_first {
        if let Some(seed) = seed {
            hash.update(seed);
        }
    }
    let digest = hash.finalize();
    let mut first = [0u8; 32];
    let mut second = [0u8; 32];
    first.copy_from_slice(&digest[..32]);
    second.copy_from_slice(&digest[32..]);
    Ok((first, second))
}

fn begin_request2_crypto(material: &Request2Material) -> Result<Request2Crypto, String> {
    let mut secret_bytes = [0u8; 32];
    getrandom::fill(&mut secret_bytes)
        .map_err(|error| format!("Android secure random failed for X25519: {error}"))?;
    let client_secret = StaticSecret::from(secret_bytes);
    let client_public = PublicKey::from(&client_secret).to_bytes();
    let (early_server_to_client, early_client_to_server) = derive_client_session_keys(
        &client_secret,
        &client_public,
        &material.server_early_public_key,
    )?;
    let mut guid_bytes = [0u8; 8];
    getrandom::fill(&mut guid_bytes)
        .map_err(|error| format!("Android secure random failed for RakNet GUID: {error}"))?;
    let mut client_guid = u64::from_le_bytes(guid_bytes);
    if client_guid == 0 || client_guid == u64::MAX {
        client_guid ^= 0xa5a5_5a5a_d3c4_b2e1;
    }
    Ok(Request2Crypto {
        client_secret,
        client_public,
        early_server_to_client,
        early_client_to_server,
        client_guid,
    })
}

fn application_nonce(counter: u64) -> [u8; 12] {
    let mut nonce = *b"UniqueNumber";
    let base = u64::from_le_bytes(nonce[..8].try_into().unwrap());
    nonce[..8].copy_from_slice(&base.wrapping_add(counter).to_le_bytes());
    nonce
}

/// RakNet's `BitStream::Write<SystemAddress>` specialization writes version
/// 4, the bitwise complement of the four address bytes, and the network-order
/// port. (Its IPv6 branch writes the platform sockaddr_in6 memory image, so it
/// is deliberately rejected until a captured 2022 IPv6 route requires it.)
fn write_system_address(address: SocketAddr, out: &mut Vec<u8>) -> Result<(), String> {
    let SocketAddr::V4(address) = address else {
        return Err("2022 OpenRequest2 IPv6 SystemAddress is not implemented".into());
    };
    out.push(4);
    out.extend(address.ip().octets().map(|byte| !byte));
    out.extend_from_slice(&address.port().to_be_bytes());
    Ok(())
}

fn read_system_address(input: &[u8], cursor: &mut usize) -> Result<Endpoint, String> {
    let version = *input
        .get(*cursor)
        .ok_or_else(|| "OpenReply2 ends before SystemAddress version".to_string())?;
    *cursor += 1;
    if version != 4 {
        return Err(format!(
            "OpenReply2 returned unsupported SystemAddress IP version {version}"
        ));
    }
    let encoded = input
        .get(*cursor..*cursor + 4)
        .ok_or_else(|| "OpenReply2 has truncated IPv4 SystemAddress".to_string())?;
    *cursor += 4;
    let port_bytes: [u8; 2] = input
        .get(*cursor..*cursor + 2)
        .ok_or_else(|| "OpenReply2 has truncated SystemAddress port".to_string())?
        .try_into()
        .unwrap();
    *cursor += 2;
    let ip = std::net::Ipv4Addr::new(!encoded[0], !encoded[1], !encoded[2], !encoded[3]);
    Ok(Endpoint {
        address: ip.to_string(),
        port: u16::from_be_bytes(port_bytes),
    })
}

/// Build version-3 RbxOpenRequest2 exactly as 2022 `sendRbxOpenRequest2`:
/// the RUPP prefix is outside the crypto view; the clear header is AEAD AAD;
/// and ciphertext is followed by a 12-byte nonce and detached 16-byte tag.
fn build_rbx_open_request2(
    mtu: u16,
    prefix: &[u8],
    target: SocketAddr,
    material: &Request2Material,
    crypto: &Request2Crypto,
) -> Result<Vec<u8>, String> {
    if mtu < 576 {
        return Err(format!("RbxOpenRequest2 MTU {mtu} is below 576"));
    }
    let mut aad = Vec::with_capacity(96);
    aad.push(RBX_OPEN_REQUEST_2);
    aad.extend_from_slice(&OFFLINE_MAGIC);
    aad.push(RBX_OPEN_REQUEST_2_VERSION);
    aad.push(0); // patched after the pre-auth blob is written
    aad.extend_from_slice(&0u16.to_be_bytes()); // patched ciphertext length
    aad.extend_from_slice(&material.early_key_version.to_be_bytes());
    aad.extend_from_slice(&crypto.client_public);
    aad.push(material.auth.auth_version);
    aad.push(material.auth.preauth_blob.len() as u8);
    aad.extend_from_slice(&material.auth.preauth_blob);
    let aad_len = u8::try_from(aad.len())
        .map_err(|_| format!("RbxOpenRequest2 AAD is too large: {} bytes", aad.len()))?;

    let mut plaintext = Vec::with_capacity(32 + material.auth.auth_blob.len());
    plaintext.extend_from_slice(&RAK_PEER_CAPABILITIES_0735_CLIENT_FLOOR.to_be_bytes());
    plaintext.extend_from_slice(&crypto.client_guid.to_be_bytes());
    plaintext.extend_from_slice(&mtu.to_be_bytes());
    plaintext.push(1); // ChaCha20-Poly1305; getSupportAes()==false path
    write_system_address(target, &mut plaintext)?;
    plaintext.push(material.auth.auth_blob.len() as u8);
    plaintext.extend_from_slice(&material.auth.auth_blob);
    let ciphertext_len = u16::try_from(plaintext.len()).map_err(|_| {
        format!(
            "RbxOpenRequest2 encrypted plaintext is too large: {} bytes",
            plaintext.len()
        )
    })?;
    aad[18] = aad_len;
    aad[19..21].copy_from_slice(&ciphertext_len.to_be_bytes());

    let nonce = application_nonce(0);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&crypto.early_client_to_server));
    let tag = cipher
        .encrypt_in_place_detached(Nonce::from_slice(&nonce), &aad, &mut plaintext)
        .map_err(|_| "ChaCha20-Poly1305 failed to encrypt OpenRequest2".to_string())?;

    let payload_len = usize::from(mtu)
        .checked_sub(IPV6_UDP_HEADER_BYTES)
        .ok_or_else(|| "RbxOpenRequest2 MTU underflow".to_string())?;
    let used = prefix.len() + aad.len() + plaintext.len() + EARLY_AEAD_OVERHEAD;
    if used > payload_len {
        return Err(format!(
            "RbxOpenRequest2 needs {used} bytes but MTU {mtu} permits {payload_len}"
        ));
    }
    let mut packet = Vec::with_capacity(payload_len);
    packet.extend_from_slice(prefix);
    packet.extend_from_slice(&aad);
    packet.extend_from_slice(&plaintext);
    packet.extend_from_slice(&nonce);
    packet.extend_from_slice(&tag);
    packet.resize(payload_len, 0);
    Ok(packet)
}

#[derive(Debug)]
struct RbxOpenReply2 {
    version: u8,
    server_capabilities: u64,
    server_guid: u64,
    mtu: u16,
    selected_encryption: u8,
    binding_address: Endpoint,
    returned_rupp_token: Option<[u8; 16]>,
    returned_rupp_token_type: Option<u8>,
    /// Exact clear RUPP header received outside OpenReply2. Current routed
    /// replies use a 23-byte token-only header, which is distinct from the
    /// client's 31-byte token-plus-private-endpoint header.
    returned_rupp_prefix: Option<Vec<u8>>,
    session_server_to_client: [u8; 32],
    session_client_to_server: [u8; 32],
    sha512_session_first_half: [u8; 32],
    sha512_session_second_half: [u8; 32],
    diagnostic_session_key_candidates: Vec<DiagnosticSessionKeyCandidate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReceivedRuppToken {
    token_type: u8,
    value: [u8; 16],
}

fn strip_optional_rupp_prefix(
    packet: &[u8],
) -> Result<(&[u8], usize, Option<ReceivedRuppToken>, Option<Vec<u8>>), String> {
    if packet.first() != Some(&RUPP_PROTOCOL_RAKNET) {
        return Ok((packet, 0, None, None));
    }
    if packet.len() < 4 {
        return Err("truncated RUPP response prefix".into());
    }
    let header_len = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if header_len < 4 || header_len > packet.len() {
        return Err(format!("invalid RUPP response header length {header_len}"));
    }

    // In 0.735 the server-advertised route token is carried by the response's
    // outer RUPP Token TLV rather than appended to the decrypted OpenReply2
    // body. Token TLV value byte zero is its lineage/subtype; the following
    // 16 bytes become eligible for the outbound header once an online packet
    // has resolved the active remote (the offline Reply2 itself does not
    // trigger native RakPeer's per-remote token updater).
    let mut returned_token = None;
    let mut cursor = 4usize;
    while cursor < header_len {
        let tlv_type = *packet
            .get(cursor)
            .ok_or_else(|| "RUPP response ends before TLV type".to_string())?;
        let tlv_len = usize::from(
            *packet
                .get(cursor + 1)
                .ok_or_else(|| "RUPP response ends before TLV length".to_string())?,
        );
        cursor += 2;
        let value = packet
            .get(cursor..cursor + tlv_len)
            .ok_or_else(|| "RUPP response has a truncated TLV value".to_string())?;
        if tlv_type == RUPP_TLV_TOKEN {
            if tlv_len != usize::from(RUPP_TOKEN_VALUE_LENGTH) {
                return Err(format!(
                    "RUPP response token TLV has length {tlv_len}, expected {RUPP_TOKEN_VALUE_LENGTH}"
                ));
            }
            returned_token = Some(ReceivedRuppToken {
                token_type: value[0],
                value: value[1..17].try_into().unwrap(),
            });
        }
        cursor += tlv_len;
    }
    let exact_prefix = packet[..header_len].to_vec();
    Ok((
        &packet[header_len..],
        header_len,
        returned_token,
        Some(exact_prefix),
    ))
}

fn describe_received_rupp_prefix(prefix: &[u8]) -> String {
    if prefix.len() < 4 {
        return format!("truncated ({} bytes)", prefix.len());
    }
    let declared = u16::from_be_bytes([prefix[2], prefix[3]]);
    let mut tlvs = Vec::new();
    let mut cursor = 4usize;
    while cursor + 2 <= prefix.len() {
        let kind = prefix[cursor];
        let length = usize::from(prefix[cursor + 1]);
        cursor += 2;
        let Some(value) = prefix.get(cursor..cursor + length) else {
            tlvs.push(format!("type {kind}(truncated length {length})"));
            break;
        };
        let description = match (kind, value) {
            (RUPP_TLV_TOKEN, [subtype, ..]) => {
                format!("token(length {length}, subtype {subtype}, value redacted)")
            }
            (RUPP_TLV_IPV4, [a, b, c, d, port_hi, port_lo]) => format!(
                "ipv4(length 6, {}:{})",
                std::net::Ipv4Addr::new(*a, *b, *c, *d),
                u16::from_be_bytes([*port_hi, *port_lo])
            ),
            (RUPP_TLV_IPV6, value) if value.len() == 18 => {
                let mut address = [0u8; 16];
                address.copy_from_slice(&value[..16]);
                format!(
                    "ipv6(length 18, [{}]:{})",
                    std::net::Ipv6Addr::from(address),
                    u16::from_be_bytes([value[16], value[17]])
                )
            }
            _ => format!("type {kind}(length {length})"),
        };
        tlvs.push(description);
        cursor += length;
    }
    format!(
        "{{protocol {}, flags 0x{:02x}, declared length {declared}, actual length {}, TLVs [{}]}}",
        prefix[0],
        prefix[1],
        prefix.len(),
        tlvs.join(", ")
    )
}

fn parse_rbx_open_reply2(
    packet: &[u8],
    crypto: &Request2Crypto,
    normal_session_seed: Option<&[u8]>,
) -> Result<RbxOpenReply2, String> {
    let (packet, stripped_rupp_len, rupp_returned_token, returned_rupp_prefix) =
        strip_optional_rupp_prefix(packet)?;
    let Some(&packet_id) = packet.first() else {
        return Err("RbxOpenReply2 is empty".into());
    };
    if packet_id != RBX_OPEN_REPLY_2 {
        if packet.len() == 25 && packet[1..17] == OFFLINE_MAGIC {
            let server_guid = u64::from_be_bytes(packet[17..25].try_into().unwrap());
            let meaning = match packet[0] {
                0x0b => "connection attempt failed (invalid/unset encryption)",
                0x12 => "already connected",
                0x14 => "no free incoming connections",
                0x1a => "IP address connected recently",
                _ => "offline connection error",
            };
            return Err(format!(
                "server returned {meaning} id 0x{:02x} (GUID {server_guid:016x})",
                packet[0]
            ));
        }
        return Err(format!(
            "expected RbxOpenReply2 id 0x{RBX_OPEN_REPLY_2:02x}, got 0x{packet_id:02x}"
        ));
    }
    if packet.len() < 21 + EARLY_AEAD_OVERHEAD {
        return Err(format!("RbxOpenReply2 is too short: {} bytes", packet.len()));
    }
    if packet[1..17] != OFFLINE_MAGIC {
        return Err("RbxOpenReply2 has invalid offline-message magic".into());
    }
    let version = packet[17];
    let aad_len = usize::from(packet[18]);
    let ciphertext_len = usize::from(u16::from_be_bytes([packet[19], packet[20]]));
    if aad_len < 21 || aad_len > packet.len() {
        return Err(format!("RbxOpenReply2 has invalid AAD length {aad_len}"));
    }
    // Studio 0.735's routed reply passes the total AAD offset (including the
    // already-stripped RUPP header) to earlyEncryptData while that helper's
    // crypto view starts after RUPP. Consequently `aadLen` extends that many
    // bytes into the logical Reply2 body. Those bytes remain clear but are
    // authenticated. The header's ciphertextLen continues to describe the
    // complete logical body, including this clear prefix.
    const OPEN_REPLY_2_HEADER_LEN: usize = 21;
    let clear_body_prefix_len = aad_len - OPEN_REPLY_2_HEADER_LEN;
    if clear_body_prefix_len > ciphertext_len {
        return Err(format!(
            "RbxOpenReply2 AAD consumes {clear_body_prefix_len} body bytes, exceeding ciphertext length {ciphertext_len}"
        ));
    }
    if clear_body_prefix_len != 0 && clear_body_prefix_len != stripped_rupp_len {
        return Err(format!(
            "RbxOpenReply2 AAD body prefix is {clear_body_prefix_len} bytes, but stripped RUPP header is {stripped_rupp_len} bytes"
        ));
    }
    let encrypted_meaningful_len = ciphertext_len - clear_body_prefix_len;
    let minimum_len = aad_len
        .checked_add(encrypted_meaningful_len)
        .and_then(|length| length.checked_add(EARLY_AEAD_OVERHEAD))
        .ok_or_else(|| "RbxOpenReply2 length overflow".to_string())?;
    if packet.len() < minimum_len {
        return Err(format!(
            "RbxOpenReply2 is truncated: header needs at least {minimum_len} bytes, datagram has {}",
            packet.len()
        ));
    }

    // Both the 2022 and 0.735 servers may reserve 28 zero bytes before calling
    // earlyEncryptData. They become authenticated ciphertext before the real
    // 12-byte nonce and 16-byte tag. Native clients decrypt the zero tail but
    // leave it unread after parsing the logical body.
    let ciphertext_end = packet.len() - EARLY_AEAD_OVERHEAD;
    let actual_encrypted_len = ciphertext_end - aad_len;
    let encrypted_zero_tail_len = actual_encrypted_len
        .checked_sub(encrypted_meaningful_len)
        .ok_or_else(|| {
            format!(
                "RbxOpenReply2 encrypted body is {actual_encrypted_len} bytes, shorter than required {encrypted_meaningful_len}"
            )
        })?;
    if encrypted_zero_tail_len != 0 && encrypted_zero_tail_len != EARLY_AEAD_OVERHEAD {
        return Err(format!(
            "RbxOpenReply2 encrypted zero tail has unexpected length {encrypted_zero_tail_len} (logical body {ciphertext_len}, clear authenticated prefix {clear_body_prefix_len}, encrypted wire {actual_encrypted_len})"
        ));
    }
    let nonce = &packet[ciphertext_end..ciphertext_end + 12];
    let tag = Tag::from_slice(&packet[ciphertext_end + 12..]);
    let mut encrypted_plaintext = packet[aad_len..ciphertext_end].to_vec();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&crypto.early_server_to_client));
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(nonce),
            &packet[..aad_len],
            &mut encrypted_plaintext,
            tag,
        )
        .map_err(|_| "RbxOpenReply2 early ChaCha20-Poly1305 authentication failed".to_string())?;

    let mut plaintext = Vec::with_capacity(clear_body_prefix_len + encrypted_plaintext.len());
    plaintext.extend_from_slice(&packet[OPEN_REPLY_2_HEADER_LEN..aad_len]);
    plaintext.extend_from_slice(&encrypted_plaintext);

    if plaintext.len() < ciphertext_len {
        return Err(format!(
            "RbxOpenReply2 reconstructed body is {} bytes, shorter than logical length {ciphertext_len}",
            plaintext.len()
        ));
    }
    let (logical_body, zero_tail) = plaintext.split_at(ciphertext_len);
    if zero_tail.len() != encrypted_zero_tail_len
        || zero_tail.iter().any(|byte| *byte != 0)
    {
        return Err(format!(
            "RbxOpenReply2 has an invalid {}-byte decrypted zero tail",
            zero_tail.len()
        ));
    }
    if logical_body.len() < 32 + 8 + 8 + 2 + 1 + 7 {
        return Err(format!(
            "RbxOpenReply2 logical body is too short: {} bytes",
            logical_body.len()
        ));
    }

    let mut cursor = 0usize;
    let server_ephemeral_key: [u8; 32] = logical_body[cursor..cursor + 32].try_into().unwrap();
    cursor += 32;
    let server_capabilities =
        u64::from_be_bytes(logical_body[cursor..cursor + 8].try_into().unwrap());
    cursor += 8;
    let server_guid = u64::from_be_bytes(logical_body[cursor..cursor + 8].try_into().unwrap());
    cursor += 8;
    let mtu = u16::from_be_bytes(logical_body[cursor..cursor + 2].try_into().unwrap());
    cursor += 2;
    let selected_encryption = logical_body[cursor];
    cursor += 1;
    let binding_address = read_system_address(logical_body, &mut cursor)?;

    // The 2022 format optionally appended a length byte and 16-byte refreshed
    // token to the encrypted body. Current 0.735 clients stop immediately after
    // SystemAddress because token refresh moved to the outer RUPP Token TLV.
    let inline_returned_token = if cursor < logical_body.len() {
        if version == 0 {
            return Err(format!(
                "RbxOpenReply2 version 0 has {} unexpected logical trailing byte(s)",
                logical_body.len() - cursor
            ));
        }
        let token_len = logical_body[cursor] as usize;
        cursor += 1;
        if token_len != 16 {
            return Err(format!(
                "RbxOpenReply2 inline RUPP token has length {token_len}, expected 16"
            ));
        }
        let token: [u8; 16] = logical_body
            .get(cursor..cursor + token_len)
            .ok_or_else(|| "RbxOpenReply2 inline RUPP token is truncated".to_string())?
            .try_into()
            .unwrap();
        cursor += token_len;
        Some(token)
    } else {
        None
    };
    if cursor != logical_body.len() {
        return Err(format!(
            "RbxOpenReply2 has {} unexpected logical trailing byte(s)",
            logical_body.len() - cursor
        ));
    }
    let returned_rupp_token = match (rupp_returned_token, inline_returned_token) {
        (Some(outer), Some(inline)) if outer.value != inline => {
            return Err("RbxOpenReply2 outer and inline RUPP tokens disagree".into());
        }
        (Some(outer), _) => Some(outer.value),
        (_, Some(inline)) => Some(inline),
        (None, None) => None,
    };
    let returned_rupp_token_type = rupp_returned_token.map(|token| token.token_type);
    if mtu < 576 {
        return Err(format!("RbxOpenReply2 returned invalid MTU {mtu}"));
    }
    if selected_encryption != 1 && selected_encryption != 2 {
        return Err(format!(
            "RbxOpenReply2 selected unknown encryption format {selected_encryption}"
        ));
    }
    let (session_server_to_client, session_client_to_server) = derive_client_session_keys(
        &crypto.client_secret,
        &crypto.client_public,
        &server_ephemeral_key,
    )?;
    let (sha512_session_first_half, sha512_session_second_half) =
        derive_client_session_keys_sha512(
            &crypto.client_secret,
            &crypto.client_public,
            &server_ephemeral_key,
        )?;
    let mut diagnostic_session_key_candidates = Vec::new();
    if let Some(seed) = normal_session_seed {
        let (suffix_first, suffix_second) = derive_client_session_keys_sha512_with_seed(
            &crypto.client_secret,
            &crypto.client_public,
            &server_ephemeral_key,
            Some(seed),
            false,
        )?;
        diagnostic_session_key_candidates.push(DiagnosticSessionKeyCandidate {
            label: "RandomSeed1 SHA-512 suffix first-half RX + AES-GCM",
            server_to_client: suffix_first,
            client_to_server: suffix_second,
            cipher: SessionCipher::Aes256Gcm,
        });
        diagnostic_session_key_candidates.push(DiagnosticSessionKeyCandidate {
            label: "RandomSeed1 SHA-512 suffix first-half TX + AES-GCM",
            server_to_client: suffix_second,
            client_to_server: suffix_first,
            cipher: SessionCipher::Aes256Gcm,
        });
        let (prefix_first, prefix_second) = derive_client_session_keys_sha512_with_seed(
            &crypto.client_secret,
            &crypto.client_public,
            &server_ephemeral_key,
            Some(seed),
            true,
        )?;
        diagnostic_session_key_candidates.push(DiagnosticSessionKeyCandidate {
            label: "RandomSeed1 SHA-512 prefix first-half RX + AES-GCM",
            server_to_client: prefix_first,
            client_to_server: prefix_second,
            cipher: SessionCipher::Aes256Gcm,
        });
        diagnostic_session_key_candidates.push(DiagnosticSessionKeyCandidate {
            label: "RandomSeed1 SHA-512 prefix first-half TX + AES-GCM",
            server_to_client: prefix_second,
            client_to_server: prefix_first,
            cipher: SessionCipher::Aes256Gcm,
        });
    }
    Ok(RbxOpenReply2 {
        version,
        server_capabilities,
        server_guid,
        mtu,
        selected_encryption,
        binding_address,
        returned_rupp_token,
        returned_rupp_token_type,
        returned_rupp_prefix,
        session_server_to_client,
        session_client_to_server,
        sha512_session_first_half,
        sha512_session_second_half,
        diagnostic_session_key_candidates,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RbxOpenReply1 {
    pub server_guid: u64,
    pub encryption_enabled: bool,
    pub mtu: u16,
}

/// Build the exact 2022 `RbxOpenRequest1` payload.
///
/// RakPeer chooses an IP MTU, subtracts the 40-byte IPv6+UDP allowance,
/// writes `[0x7b][16-byte magic][protocol 5]`, and zero-pads the UDP
/// payload to that length. The server rejects request-1 datagrams whose
/// inferred MTU is below 576 bytes.
pub fn build_rbx_open_request1(mtu: u16) -> Result<Vec<u8>, String> {
    build_rbx_open_request1_with_prefix(mtu, &[])
}

pub(crate) fn build_rbx_open_request1_with_prefix(
    mtu: u16,
    prefix: &[u8],
) -> Result<Vec<u8>, String> {
    let mtu = usize::from(mtu);
    if mtu < 576 {
        return Err(format!("RbxOpenRequest1 MTU {mtu} is below the 576-byte protocol minimum"));
    }
    let payload_len = mtu
        .checked_sub(IPV6_UDP_HEADER_BYTES)
        .ok_or_else(|| "RbxOpenRequest1 MTU underflow".to_string())?;
    if prefix.len() + 18 > payload_len {
        return Err(format!(
            "RUPP prefix of {} bytes does not fit RbxOpenRequest1 MTU {mtu}",
            prefix.len()
        ));
    }
    let mut packet = Vec::with_capacity(payload_len);
    packet.extend_from_slice(prefix);
    packet.push(RBX_OPEN_REQUEST_1);
    packet.extend_from_slice(&OFFLINE_MAGIC);
    packet.push(RBX_PROTOCOL_VERSION);
    packet.resize(payload_len, 0);
    Ok(packet)
}

/// Decode the fixed 28-byte `RbxOpenReply1` payload:
/// `[0x7e][magic][server GUID BE][encryption byte][MTU BE]`.
pub fn parse_rbx_open_reply1(packet: &[u8]) -> Result<RbxOpenReply1, String> {
    if packet.len() != 28 {
        return Err(format!(
            "RbxOpenReply1 must be exactly 28 bytes, got {}",
            packet.len()
        ));
    }
    if packet[0] != RBX_OPEN_REPLY_1 {
        return Err(format!(
            "expected RbxOpenReply1 id 0x{RBX_OPEN_REPLY_1:02x}, got 0x{:02x}",
            packet[0]
        ));
    }
    if packet[1..17] != OFFLINE_MAGIC {
        return Err("RbxOpenReply1 has invalid offline-message magic".into());
    }
    let server_guid = u64::from_be_bytes(packet[17..25].try_into().unwrap());
    let encryption_enabled = packet[25] != 0;
    let mtu = u16::from_be_bytes([packet[26], packet[27]]);
    if mtu < 576 {
        return Err(format!("RbxOpenReply1 returned invalid MTU {mtu}"));
    }
    Ok(RbxOpenReply1 {
        server_guid,
        encryption_enabled,
        mtu,
    })
}

pub(crate) fn parse_probe_reply(packet: &[u8]) -> Result<RbxOpenReply1, String> {
    if let Ok(reply) = parse_rbx_open_reply1(packet) {
        return Ok(reply);
    }
    // Normally UDMUX consumes the client RUPP prefix and returns bare RakNet.
    // Accept a prefixed reply as well so diagnostics remain correct if a fleet
    // returns the routing header to the client.
    if packet.len() >= 4 && packet[0] == RUPP_PROTOCOL_RAKNET {
        let header_len = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
        if header_len >= 4 && header_len <= packet.len() {
            return parse_rbx_open_reply1(&packet[header_len..]);
        }
    }
    parse_rbx_open_reply1(packet)
}

/// Perform the first real customized-RakNet handshake exchange. A valid
/// reply proves both UDP reachability and that the target is a compatible
/// Roblox server; unlike a generic unconnected ping, this is the request
/// Studio sends immediately before encrypted open-request-2.
pub fn probe_endpoint(endpoint: &Endpoint, timeout_ms: u64) -> Result<String, String> {
    let mut request2_attempted = false;
    probe_endpoint_with_rupp(endpoint, timeout_ms, None, None, &mut request2_attempted)
}

fn probe_endpoint_with_rupp(
    endpoint: &Endpoint,
    timeout_ms: u64,
    rupp: Option<&RuppProbeMaterial>,
    request2: Option<&Request2Material>,
    request2_attempted: &mut bool,
) -> Result<String, String> {
    const PROBE_MTUS: [u16; 3] = [1492, DEFAULT_PROBE_MTU, 576];
    const ROUNDS: usize = 2;
    const REQUEST1_CANDIDATE_WAIT_MS: u64 = 450;
    const REQUEST2_MIN_WAIT_MS: u64 = 5_000;

    // Native Time::now<2> uses a lazy process-local monotonic start sample,
    // normally established before Connect. Do the equivalent before Request1
    // so the later application request does not receive a zero timestamp.
    initialize_raknet_time();
    let target = endpoint.label();
    let started = Instant::now();
    let request1_deadline = started + Duration::from_millis(timeout_ms.max(1_200));
    let prefixes: Vec<(Option<u8>, Vec<u8>)> = if let Some(material) = rupp {
        // CloudEditConnectionModel constructs TokenType::GameService, whose
        // serialized value is 1. The server dispatcher also forwards a RUPP
        // token to processRbxOpenRequest2 only for this subtype. Subtype 2 is
        // accepted by the generic TLV parser but is not the Team Create path.
        vec![(Some(1), build_rupp_header(material, 1)?)]
    } else {
        vec![(None, Vec::new())]
    };

    let mut sent = 0usize;
    let mut invalid_replies = Vec::new();
    let mut accepted: Option<(UdpSocket, RbxOpenReply1, SocketAddr, Vec<u8>, Option<u8>)> = None;
    let mut receive_buf = [0u8; 2048];
    'rounds: for _ in 0..ROUNDS {
        for (token_type, prefix) in &prefixes {
            if Instant::now() >= request1_deadline {
                break 'rounds;
            }
            // Keep each routed candidate on its own source port, and retain
            // the exact socket that receives Reply1 for encrypted Request2.
            let socket = UdpSocket::bind(endpoint.bind_address())
                .map_err(|error| format!("{target}: bind failed: {error}"))?;
            // RakPeer cycles MTU candidates. Send one candidate group under a
            // single RUPP token subtype, then receive before trying the next.
            for mtu in PROBE_MTUS {
                let packet = build_rbx_open_request1_with_prefix(mtu, prefix)?;
                socket
                    .send_to(&packet, (endpoint.address.as_str(), endpoint.port))
                    .map_err(|error| {
                        format!("{target}: RbxOpenRequest1 send failed: {error}")
                    })?;
                sent += 1;
            }
            let candidate_deadline = std::cmp::min(
                request1_deadline,
                Instant::now() + Duration::from_millis(REQUEST1_CANDIDATE_WAIT_MS),
            );
            while let Some(remaining) = candidate_deadline.checked_duration_since(Instant::now()) {
                socket
                    .set_read_timeout(Some(remaining.max(Duration::from_millis(1))))
                    .map_err(|error| format!("{target}: socket timeout: {error}"))?;
                match socket.recv_from(&mut receive_buf) {
                    Ok((length, from)) => match parse_probe_reply(&receive_buf[..length]) {
                        Ok(reply) => {
                            accepted = Some((socket, reply, from, prefix.clone(), *token_type));
                            break 'rounds;
                        }
                        Err(reason) => invalid_replies.push(format!(
                            "{length} bytes from {from}: {reason}"
                        )),
                    },
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        break;
                    }
                    Err(error) => {
                        return Err(format!("{target}: UDP receive failed: {error}"));
                    }
                }
            }
        }
    }

    let Some((socket, reply1, from, selected_prefix, selected_token_type)) = accepted else {
        if !invalid_replies.is_empty() {
            return Err(format!(
                "{target}: received UDP after {sent} probes, but no valid 2022 RbxOpenReply1 ({})",
                invalid_replies.join("; ")
            ));
        }
        if is_internal_address(&endpoint.address) {
            return Err(format!(
                "{target}: private/internal server address is not routable from this phone; a public UDMUX endpoint is required"
            ));
        }
        if let Some(material) = rupp {
            return Err(format!(
                "{target}: no reply to {sent} RUPP-routed RbxOpenRequest1 probes in {timeout_ms} ms (RCC target {}, token TLV present)",
                material.rcc_endpoint.label()
            ));
        }
        return Err(format!(
            "{target}: no reply to {sent} bare RbxOpenRequest1 probes in {timeout_ms} ms; no usable 2022 RUPP TokenValue/private RCC route was available"
        ));
    };

    let request1_summary = format!(
        "RbxOpenReply1 {} bytes from {from}, server GUID {:016x}, MTU {}, encryption {}{}",
        28,
        reply1.server_guid,
        reply1.mtu,
        if reply1.encryption_enabled {
            "requested"
        } else {
            "deferred to request 2"
        },
        selected_token_type
            .map(|kind| format!(", RUPP token subtype {kind}"))
            .unwrap_or_default()
    );
    let Some(request2_material) = request2 else {
        return Ok(format!(
            "{target}: ✅ {request1_summary}; OpenRequest2 material unavailable"
        ));
    };

    let crypto = begin_request2_crypto(request2_material)?;
    let packet2 = build_rbx_open_request2(
        reply1.mtu,
        &selected_prefix,
        from,
        request2_material,
        &crypto,
    )?;
    let request2_aad_len = usize::from(packet2[selected_prefix.len() + 18]);
    let request2_ciphertext_len = usize::from(u16::from_be_bytes([
        packet2[selected_prefix.len() + 19],
        packet2[selected_prefix.len() + 20],
    ]));
    let request2_started = Instant::now();
    let request2_wait_ms = timeout_ms.max(REQUEST2_MIN_WAIT_MS);
    let request2_deadline = request2_started + Duration::from_millis(request2_wait_ms);
    // A valid pre-auth MAC is entered into the server's replay set before
    // early AEAD is checked. Conservatively treat even an uncertain send as
    // consuming this join config, and never try Request2 on another endpoint.
    *request2_attempted = true;
    socket
        .send_to(&packet2, from)
        .map_err(|error| format!("{target}: RbxOpenRequest2 send failed: {error}"))?;

    let mut buf = [0u8; 2048];
    let mut request2_replies = Vec::new();
    while let Some(remaining) = request2_deadline.checked_duration_since(Instant::now()) {
        socket
            .set_read_timeout(Some(remaining.max(Duration::from_millis(1))))
            .map_err(|error| format!("{target}: socket timeout: {error}"))?;
        match socket.recv_from(&mut buf) {
            Ok((length, reply_from)) => {
                // Request1 was sent at several MTUs, so its duplicate replies
                // may still be queued when Request2 is transmitted.
                if parse_probe_reply(&buf[..length]).is_ok() {
                    continue;
                }
                match parse_rbx_open_reply2(
                    &buf[..length],
                    &crypto,
                    request2_material.normal_session_seed.as_deref(),
                ) {
                    Ok(reply2) => {
                        if reply2.server_guid != reply1.server_guid {
                            return Err(format!(
                                "{target}: OpenReply2 server GUID {:016x} does not match OpenReply1 {:016x}",
                                reply2.server_guid, reply1.server_guid
                            ));
                        }
                        let session_cipher = match reply2.selected_encryption {
                            1 => SessionCipher::ChaCha20Poly1305,
                            2 => SessionCipher::Aes256Gcm,
                            _ => unreachable!(),
                        };
                        let encryption = session_cipher.label();
                        let open_reply_summary = format!(
                            "{target}: ✅ {request1_summary}\n✅ RbxOpenReply2 — {length} bytes from {reply_from}, version {}, server GUID {:016x}, MTU {}, encryption {encryption}, binding {}, capabilities 0x{:016x}, session keys derived{}, handshake elapsed {} ms",
                            reply2.version,
                            reply2.server_guid,
                            reply2.mtu,
                            reply2.binding_address.label(),
                            reply2.server_capabilities,
                            if let Some(prefix) = reply2.returned_rupp_prefix.as_ref() {
                                format!(
                                    ", server RUPP outer {} (token deferred until online receive)",
                                    describe_received_rupp_prefix(prefix)
                                )
                            } else {
                                String::new()
                            },
                            started.elapsed().as_millis()
                        );
                        // Studio's Team Create payload path can pass a
                        // separate NetStackTokenValue/NetStackPort into the
                        // connected RUPP client configuration when present;
                        // prefer that endpoint-bearing connected route. The
                        // connected session then promotes Reply2's subtype-2
                        // token and zero flags into this header before the
                        // first online datagram when Reply2 supplies them.
                        let connected_prefix = if let (Some(material), Some(token_type)) =
                            (rupp, selected_token_type)
                        {
                            build_connected_rupp_header(material, token_type).map_err(|error| {
                                format!("{target}: failed to build connected RUPP route: {error}")
                            })?
                        } else {
                            selected_prefix.clone()
                        };
                        let common_capabilities =
                            reply2.server_capabilities & RAK_PEER_CAPABILITIES_0735_CLIENT_FLOOR;
                        match establish_connected_session(
                            &socket,
                            reply_from,
                            ConnectedConfig {
                                client_guid: crypto.client_guid,
                                mtu: reply2.mtu,
                                common_capabilities,
                                session_server_to_client: reply2.session_server_to_client,
                                session_client_to_server: reply2.session_client_to_server,
                                session_cipher,
                                sha512_session_first_half: reply2.sha512_session_first_half,
                                sha512_session_second_half: reply2.sha512_session_second_half,
                                diagnostic_session_key_candidates:
                                    reply2.diagnostic_session_key_candidates,
                                rupp_prefix: connected_prefix,
                                deferred_reply2_rupp_token_type:
                                    reply2.returned_rupp_token_type,
                                deferred_reply2_rupp_token: reply2.returned_rupp_token,
                                deferred_reply2_rupp_prefix: reply2.returned_rupp_prefix,
                                timeout_ms,
                            },
                        ) {
                            Ok(accepted) => {
                                return Ok(format!(
                                    "{open_reply_summary}\n✅ ID_CONNECTION_REQUEST_ACCEPTED — {} bytes from {}, datagram {}, {:?}, session KDF {}, client address {}, system index {}, {} internal addresses, request time {}, server time {}, server epoch {} µs, request ACKed {}, retransmissions {}, RUPP refreshes {}, session nonces tx/rx {}/{}, connected in {} ms",
                                    accepted.wire_bytes,
                                    accepted.source,
                                    accepted.datagram_number,
                                    accepted.reliability,
                                    accepted.session_kdf,
                                    accepted.client_address,
                                    accepted.system_index,
                                    accepted.internal_address_count,
                                    accepted.request_time,
                                    accepted.server_time,
                                    accepted.server_epoch_time_us,
                                    accepted.request_acked,
                                    accepted.retransmissions,
                                    accepted.refreshed_rupp_tokens,
                                    accepted.tx_nonce,
                                    accepted.rx_nonce,
                                    accepted.elapsed_ms
                                ));
                            }
                            Err(reason) => {
                                return Err(format!(
                                    "{open_reply_summary}\nConnected RakNet failed after authenticated OpenReply2: {reason}"
                                ));
                            }
                        }
                    }
                    Err(reason) => request2_replies.push(format!(
                        "{length} bytes from {reply_from}: {reason}"
                    )),
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => return Err(format!("{target}: UDP receive failed: {error}")),
        }
    }
    if request2_replies.is_empty() {
        Err(format!(
            "{target}: ✅ {request1_summary}; sent {}-byte encrypted RbxOpenRequest2 (AAD {request2_aad_len}, ciphertext {request2_ciphertext_len}) and received no UDP response during its independent {request2_wait_ms} ms reply window. RUPP/OpenRequest1 is proven; the server silently rejected either the one-use pre-auth material or early AEAD. Negotiate a fresh join config before every retry",
            packet2.len()
        ))
    } else {
        Err(format!(
            "{target}: ✅ {request1_summary}; received UDP after OpenRequest2 in {} ms but could not accept OpenReply2 ({})",
            request2_started.elapsed().as_millis(),
            request2_replies.join("; ")
        ))
    }
}

/// Probe up to `max` endpoints from a join config and produce a readable
/// multi-line report using normal KeyRing production selection (`send`).
pub fn probe_join_config(config: &serde_json::Value, max: usize, timeout_ms: u64) -> String {
    probe_join_config_with_key_ring_revert(config, max, timeout_ms, false)
}

/// Same one-use handshake probe, with explicit modeling of the native
/// `DFFlag::KeyRingRevert` emergency selection. This mode must receive its own
/// fresh gamejoin config because the pre-auth material cannot be replayed.
pub fn probe_join_config_with_key_ring_revert(
    config: &serde_json::Value,
    max: usize,
    timeout_ms: u64,
    key_ring_revert: bool,
) -> String {
    let all_endpoints = parse_all_join_endpoints(config);
    let internal_count = all_endpoints
        .iter()
        .filter(|e| is_internal_address(&e.address))
        .count();
    let endpoints = prefer_public_endpoints(all_endpoints);
    if endpoints.is_empty() {
        return "No server endpoints found in the join config (unexpected shape — check the raw JSON keys)".into();
    }
    let rupp = extract_rupp_probe_material(config);
    let mut heading = format!(
        "{} usable endpoint(s) in join config; probing {}…",
        endpoints.len(),
        endpoints.len().min(max)
    );
    if internal_count > 0 && endpoints.iter().all(|e| !is_internal_address(&e.address)) {
        heading.push_str(&format!(
            " ({internal_count} private RCC address(es) used inside the RUPP route)"
        ));
    }
    match &rupp {
        Ok(material) => {
            heading.push_str(&format!(
                "\n2022 RUPP routing ready: open TokenValue + RCC {}",
                material.rcc_endpoint.label()
            ));
            if let Some(route) = &material.connected_route {
                heading.push_str(&format!(
                    "; connected NetStackTokenValue + RCC {}",
                    route.rcc_endpoint.label()
                ));
            }
        }
        Err(reason) => heading.push_str(&format!("\nRUPP routing unavailable: {reason}")),
    }
    let token_value = find_field_ci(config, "TokenValue", 0);
    let netstack_token_value = find_field_ci(config, "NetStackTokenValue", 0);
    let netstack_port = find_field_ci(config, "NetStackPort", 0);
    let token_algorithm = find_field_ci(config, "TokenGenAlgorithm", 0);
    let pepper_id = find_field_ci(config, "PepperId", 0);
    if token_value.is_some()
        || netstack_token_value.is_some()
        || netstack_port.is_some()
        || token_algorithm.is_some()
        || pepper_id.is_some()
    {
        heading.push_str(&format!(
            "\nCurrent RUPP token inputs: TokenValue {}; NetStackTokenValue {}; NetStackPort {}; algorithm {}; pepper {} (safe metadata only)",
            token_value
                .as_ref()
                .map(diagnostic_token_value_shape)
                .unwrap_or_else(|| "absent".into()),
            netstack_token_value
                .as_ref()
                .map(diagnostic_token_value_shape)
                .unwrap_or_else(|| "absent".into()),
            netstack_port
                .as_ref()
                .map(diagnostic_scalar)
                .unwrap_or_else(|| "absent".into()),
            token_algorithm
                .as_ref()
                .map(diagnostic_scalar)
                .unwrap_or_else(|| "absent".into()),
            pepper_id
                .as_ref()
                .map(diagnostic_number_with_hex)
                .unwrap_or_else(|| "absent".into())
        ));
    }
    if let Some(seed) = find_field_ci(config, "RandomSeed1", 0) {
        let shape = diagnostic_base64_secret_shape(&seed);
        heading.push_str(&format!(
            "\nCurrent normal-session seed metadata: RandomSeed1 {shape} (used only in bounded current-build seeded-KDF diagnostics when decodable; not applied to the proven 0.735 KX path)"
        ));
    }
    if let Some(rcc_version) = find_field_ci(config, "RccVersion", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
    {
        heading.push_str(&format!("\nAdvertised RCC version: {rcc_version}"));
    }
    if let Ok(rbx_transport_plan) = extract_rbx_transport_connect_plan(config) {
        heading.push_str(&rbx_transport_plan.summary());
        return heading;
    }
    let request2 = extract_request2_material_with_revert(config, key_ring_revert);
    match &request2 {
        Ok(material) => heading.push_str(&format!(
            "\nEncrypted OpenRequest2 ready: selected key version {} via {} (send {}; revert {}; hashJobId {}), auth version {}, pre-auth {} bytes, auth {} bytes",
            material.early_key_version,
            if material.early_key_uses_ephemeral_override {
                "URL-decoded EphemeralEarlyPubKey override"
            } else if material.early_key_uses_revert {
                "KeyRingRevert"
            } else {
                "ClientPublicKeyData send"
            },
            material.early_key_send_version,
            material.early_key_revert_version,
            if material.early_key_hashes_job_id {
                "applied"
            } else {
                "not set"
            },
            material.auth.auth_version,
            material.auth.preauth_blob.len(),
            material.auth.auth_blob.len()
        )),
        Err(reason) => heading.push_str(&format!("\nOpenRequest2 unavailable: {reason}")),
    }
    if let Some(ephemeral_key) = find_field_ci(config, "EphemeralEarlyPubKey", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.is_empty())
    {
        match url_percent_decode(&ephemeral_key)
            .ok()
            .and_then(|encoded| decode_base64(&encoded))
        {
            Some(decoded) => heading.push_str(&format!(
                "\nNative Team Create ephemeral override decoded successfully ({} bytes; legacy RakNet id/send/revert {})",
                decoded.len(), RAKNET_EPHEMERAL_EARLY_KEY_VERSION
            )),
            None => heading.push_str(
                "\nEphemeralEarlyPubKey failed native URL-decode/Base64 processing",
            ),
        }
    }
    let mut lines = vec![heading];
    let endpoint_limit = endpoints.len().min(max);
    let mut request2_attempted = false;
    let mut visited = 0usize;
    for endpoint in endpoints.iter().take(endpoint_limit) {
        visited += 1;
        let material = if is_internal_address(&endpoint.address) {
            None
        } else {
            rupp.as_ref().ok()
        };
        let request2_for_endpoint = if request2_attempted {
            None
        } else {
            request2.as_ref().ok()
        };
        match probe_endpoint_with_rupp(
            endpoint,
            timeout_ms,
            material,
            request2_for_endpoint,
            &mut request2_attempted,
        ) {
            Ok(line) => lines.push(line),
            Err(line) => lines.push(line),
        }
        if request2_attempted {
            break;
        }
    }
    if request2_attempted && visited < endpoint_limit {
        lines.push(format!(
            "Skipped {} additional endpoint(s): encrypted pre-auth is replay-protected and Request2 was already attempted once",
            endpoint_limit - visited
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_fixture(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()
            })
            .collect()
    }

    #[test]
    fn safe_metadata_shapes_do_not_expose_secret_material() {
        let token = serde_json::Value::String("AAECAwQFBgcICQoLDA0ODw==".into());
        let token_shape = diagnostic_token_value_shape(&token);
        assert_eq!(
            token_shape,
            "string(24 chars), resolves to redacted 16-byte token"
        );
        assert!(!token_shape.contains("AAECAw"));

        let seed = serde_json::Value::String("AAECAwQFBgcICQoLDA0ODw==".into());
        assert_eq!(
            diagnostic_base64_secret_shape(&seed),
            "string(24 chars), Base64-decodes 16 bytes"
        );

        let pepper = serde_json::json!(1790819129u64);
        assert_eq!(
            diagnostic_number_with_hex(&pepper),
            "1790819129 (0x6abdbb39)"
        );
    }

    #[test]
    fn builds_exact_rbx_open_request_1() {
        let packet = build_rbx_open_request1(1200).unwrap();
        assert_eq!(packet.len(), 1160);
        assert_eq!(packet[0], RBX_OPEN_REQUEST_1);
        assert_eq!(&packet[1..17], &OFFLINE_MAGIC);
        assert_eq!(packet[17], RBX_PROTOCOL_VERSION);
        assert!(packet[18..].iter().all(|byte| *byte == 0));
        assert!(build_rbx_open_request1(575).is_err());
    }

    #[test]
    fn builds_exact_rupp_routed_open_request_1() {
        let config = serde_json::json!({
            "settings": {
                "DirectServerReturn": false,
                "TokenValue": "AAECAwQFBgcICQoLDA0ODw==",
                "ServerConnections": [
                    { "Address": "10.32.8.208", "Port": 50704 }
                ],
                "UdmuxEndpoints": [
                    { "Address": "128.116.54.33", "Port": 50704 }
                ]
            }
        });
        let material = extract_rupp_probe_material(&config).unwrap();
        assert_eq!(material.token, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
        assert_eq!(material.rcc_endpoint.address, "10.32.8.208");
        assert_eq!(material.rcc_endpoint.port, 50704);

        let header = build_rupp_header(&material, 2).unwrap();
        assert_eq!(header.len(), 31);
        assert_eq!(&header[..7], &[1, 0, 0, 31, 1, 17, 2]);
        assert_eq!(&header[7..23], &material.token);
        assert_eq!(&header[23..], &[2, 6, 10, 32, 8, 208, 0xc6, 0x10]);

        let packet = build_rbx_open_request1_with_prefix(1200, &header).unwrap();
        assert_eq!(packet.len(), 1160);
        assert_eq!(&packet[..31], &header);
        assert_eq!(packet[31], RBX_OPEN_REQUEST_1);
        assert_eq!(&packet[32..48], &OFFLINE_MAGIC);
        assert_eq!(packet[48], RBX_PROTOCOL_VERSION);
        assert!(packet[49..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn connected_rupp_prefers_netstack_token_and_port() {
        let config = serde_json::json!({
            "settings": {
                "DirectServerReturn": true,
                "TokenValue": "AAECAwQFBgcICQoLDA0ODw==",
                "NetStackTokenValue": "ICEiIyQlJicoKSorLC0uLw==",
                "NetStackPort": 56000,
                "ServerConnections": [
                    { "Address": "10.32.8.208", "Port": 50704 }
                ],
                "UdmuxEndpoints": [
                    { "Address": "128.116.54.33", "Port": 50704 }
                ]
            }
        });
        let material = extract_rupp_probe_material(&config).unwrap();
        let connected = material.connected_route.as_ref().unwrap();
        assert_eq!(connected.token, [
            0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
            0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
        ]);
        assert_eq!(connected.rcc_endpoint.address, "10.32.8.208");
        assert_eq!(connected.rcc_endpoint.port, 56000);

        let open_header = build_rupp_header(&material, 1).unwrap();
        assert_eq!(&open_header[7..23], &material.token);
        assert_eq!(&open_header[23..], &[2, 6, 10, 32, 8, 208, 0xc6, 0x10]);

        let connected_header = build_connected_rupp_header(&material, 1).unwrap();
        assert_eq!(connected_header[1], RUPP_FLAG_DIRECT_SERVER_RETURN);
        assert_eq!(&connected_header[7..23], &connected.token);
        assert_eq!(&connected_header[23..], &[2, 6, 10, 32, 8, 208, 0xda, 0xc0]);
    }

    #[test]
    fn rbx_transport_plan_uses_netstack_and_redacts_secrets() {
        let key_ring = serde_json::json!({
            "applications": {
                "RbxTransportEphemeralEarlyPublicKey": {
                    "versions": [
                        {
                            "id": 1,
                            "value": "EyxEK+AQ+9V+cmAzKKp25x/MwVA6riGTJ9FNnJmT9HI=",
                            "allowed": true
                        }
                    ],
                    "send": 1,
                    "revert": 1
                }
            }
        });
        let config = serde_json::json!({
            "settings": {
                "DirectServerReturn": true,
                "TokenValue": "AAECAwQFBgcICQoLDA0ODw==",
                "NetStackTokenValue": "ICEiIyQlJicoKSorLC0uLw==",
                "NetStackPort": 56000,
                "GameFqdn": "gamejoin.roblox.test",
                "ClientPublicKeyData": key_ring.to_string(),
                "ServerConnections": [
                    { "Address": "10.32.8.208", "Port": 50704 }
                ],
                "UdmuxEndpoints": [
                    { "Address": "128.116.54.33", "Port": 50704 }
                ]
            }
        });

        let plan = extract_rbx_transport_connect_plan(&config).unwrap();
        assert_eq!(plan.public_endpoint.address, "128.116.54.33");
        assert_eq!(plan.public_endpoint.port, 50704);
        assert_eq!(plan.rcc_endpoint.address, "10.32.8.208");
        assert_eq!(plan.rcc_endpoint.port, 50704);
        assert_eq!(plan.rbx_transport_port, 56000);
        assert_eq!(plan.token, [
            0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
            0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
        ]);
        assert_eq!(plan.early_key.version, 1);
        assert_eq!(plan.early_key.public_key.len(), 32);

        let report = probe_join_config(&config, 3, 1);
        assert!(report.contains("selectedTransport=RbxTransport"));
        assert!(report.contains("Legacy RakNet connected packets are intentionally skipped"));
        assert!(!report.contains("ICEiIyQl"));
        assert!(!report.contains("AAECAw"));
    }

    #[test]
    fn percent_decodes_ephemeral_early_public_key_without_form_plus_conversion() {
        assert_eq!(
            url_percent_decode(
                "EyxEK%2BAQ%2B9V%2BcmAzKKp25x%2FMwVA6riGTJ9FNnJmT9HI%3D"
            )
            .unwrap(),
            "EyxEK+AQ+9V+cmAzKKp25x/MwVA6riGTJ9FNnJmT9HI="
        );
        assert_eq!(url_percent_decode("A+B").unwrap(), "A+B");
        assert!(url_percent_decode("bad%2").is_err());
        assert!(url_percent_decode("bad%XZ").is_err());
    }

    #[test]
    fn ephemeral_team_create_key_replaces_client_public_key_data() {
        let encoded_key =
            "EyxEK%2BAQ%2B9V%2BcmAzKKp25x%2FMwVA6riGTJ9FNnJmT9HI%3D";
        let config = serde_json::json!({
            "settings": {
                "EphemeralEarlyPubKey": encoded_key,
                // Native Studio does not parse this field in the nonempty
                // ephemeral branch, so deliberately make it invalid JSON.
                "ClientPublicKeyData": "must not be parsed",
                "ClientTicket": "ticket-prefix;ignored;AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=;oKGio6SlpqeoqaqrrK2urw==;6"
            }
        });

        let material = extract_request2_material(&config).unwrap();
        assert_eq!(
            material.early_key_version,
            RAKNET_EPHEMERAL_EARLY_KEY_VERSION
        );
        assert_eq!(
            material.early_key_send_version,
            RAKNET_EPHEMERAL_EARLY_KEY_VERSION
        );
        assert_eq!(
            material.early_key_revert_version,
            RAKNET_EPHEMERAL_EARLY_KEY_VERSION
        );
        assert!(material.early_key_uses_ephemeral_override);
        assert!(!material.early_key_hashes_job_id);
        assert_eq!(
            material.server_early_public_key,
            <[u8; 32]>::try_from(
                decode_base64("EyxEK+AQ+9V+cmAzKKp25x/MwVA6riGTJ9FNnJmT9HI=").unwrap()
            )
            .unwrap()
        );
    }

    #[test]
    fn extracts_2022_key_ring_and_client_ticket_early_auth() {
        let config = serde_json::json!({
            "settings": {
                "ClientPublicKeyData": "{\"creationTime\":\"fixture\",\"applications\":{\"RakNetEarlyPublicKey\":{\"versions\":[{\"id\":5,\"value\":\"EyxEK+AQ+9V+cmAzKKp25x/MwVA6riGTJ9FNnJmT9HI=\",\"allowed\":true}],\"send\":5,\"revert\":5}}}",
                "ClientTicket": "ticket-prefix;ignored;AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=;oKGio6SlpqeoqaqrrK2urw==;6"
            }
        });
        let material = extract_request2_material(&config).unwrap();
        assert_eq!(material.early_key_version, 5);
        assert_eq!(material.early_key_send_version, 5);
        assert_eq!(material.early_key_revert_version, 5);
        assert!(!material.early_key_uses_revert);
        assert!(!material.early_key_hashes_job_id);
        assert!(!material.early_key_uses_ephemeral_override);
        assert_eq!(
            material.server_early_public_key,
            <[u8; 32]>::try_from(
                decode_base64("EyxEK+AQ+9V+cmAzKKp25x/MwVA6riGTJ9FNnJmT9HI=").unwrap()
            )
            .unwrap()
        );
        assert_eq!(material.auth.auth_version, 6);
        assert_eq!(material.auth.preauth_blob, (0u8..32).collect::<Vec<_>>());
        assert_eq!(material.auth.auth_blob, (0xa0u8..0xb0).collect::<Vec<_>>());
    }

    #[test]
    fn rejects_disallowed_production_key_instead_of_using_its_json_value() {
        let config = serde_json::json!({
            "settings": {
                "ClientPublicKeyData": "{\"applications\":{\"RakNetEarlyPublicKey\":{\"versions\":[{\"id\":5,\"value\":\"EyxEK+AQ+9V+cmAzKKp25x/MwVA6riGTJ9FNnJmT9HI=\",\"allowed\":false}],\"send\":5,\"revert\":5}}}",
                "ClientTicket": "ticket-prefix;ignored;AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=;oKGio6SlpqeoqaqrrK2urw==;6"
            }
        });
        let error = extract_request2_material(&config).unwrap_err();
        assert!(error.contains("not production-allowed"));
    }

    #[test]
    fn models_emergency_revert_version_even_when_revert_is_disallowed() {
        let key_ring = serde_json::json!({
            "applications": {
                "RakNetEarlyPublicKey": {
                    "versions": [
                        {
                            "id": 5,
                            "value": "EyxEK+AQ+9V+cmAzKKp25x/MwVA6riGTJ9FNnJmT9HI=",
                            "allowed": true
                        },
                        {
                            "id": 4,
                            "value": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                            "allowed": false
                        }
                    ],
                    "send": 5,
                    "revert": 4
                }
            }
        });
        let config = serde_json::json!({
            "settings": { "ClientPublicKeyData": key_ring.to_string() }
        });
        assert_eq!(parse_server_early_key(&config).unwrap().version, 5);
        let reverted = parse_server_early_key_with_revert(&config, true).unwrap();
        assert_eq!(reverted.version, 4);
        assert_eq!(reverted.send_version, 5);
        assert_eq!(reverted.revert_version, 4);
        assert!(reverted.uses_revert);
        assert!(!reverted.allowed);
        assert_eq!(reverted.bytes, [0u8; 32]);
    }

    #[test]
    fn hashes_key_ring_value_with_exact_game_id_when_requested() {
        let key_ring = serde_json::json!({
            "applications": {
                "RakNetEarlyPublicKey": {
                    "versions": [{
                        "id": 5,
                        "value": "EyxEK+AQ+9V+cmAzKKp25x/MwVA6riGTJ9FNnJmT9HI=",
                        "allowed": true,
                        "hashJobId": true
                    }],
                    "send": 5,
                    "revert": 5
                }
            }
        });
        let config = serde_json::json!({
            "settings": {
                "GameId": "89924573-d189-4499-8702-150868c5d601",
                "ClientPublicKeyData": key_ring.to_string()
            }
        });
        let key = parse_server_early_key(&config).unwrap();
        assert_eq!(key.version, 5);
        assert_eq!(key.revert_version, 5);
        assert!(key.hashes_job_id);
        assert_eq!(
            key.bytes,
            [
                0x2a, 0xfe, 0x8e, 0xee, 0x84, 0x43, 0x4c, 0x31, 0xef, 0xe6, 0x0c, 0x20,
                0xf9, 0xb6, 0x4c, 0x9b, 0xb3, 0xdf, 0x9f, 0x2a, 0x19, 0x6a, 0x1c, 0x2d,
                0xb3, 0x1f, 0x4a, 0xc0, 0xed, 0x7b, 0xc6, 0x78,
            ]
        );
    }

    #[test]
    fn matches_libsodium_crypto_kx_and_exact_open_request_2() {
        let server_public: [u8; 32] = hex_fixture(
            "132c442be010fbd57e72603328aa76e71fccc1503aae219327d14d9c9993f472",
        )
        .try_into()
        .unwrap();
        let client_secret = StaticSecret::from(<[u8; 32]>::try_from((1u8..=32).collect::<Vec<_>>()).unwrap());
        let client_public = PublicKey::from(&client_secret).to_bytes();
        assert_eq!(
            client_public.as_slice(),
            hex_fixture("07a37cbc142093c8b755dc1b10e86cb426374ad16aa853ed0bdfc0b2b86d1c7c")
        );
        let (rx, tx) = derive_client_session_keys(
            &client_secret,
            &client_public,
            &server_public,
        )
        .unwrap();
        assert_eq!(
            rx.as_slice(),
            hex_fixture("4624d68db58f54ed39894a223ab64628bd09ff10626c053b94aea2524a80f7fc")
        );
        assert_eq!(
            tx.as_slice(),
            hex_fixture("b6ece7514f1d029f7c8078993454f4f5e911492660f907da9dd3c111a84e479d")
        );
        let (sha_first, sha_second) = derive_client_session_keys_sha512(
            &client_secret,
            &client_public,
            &server_public,
        )
        .unwrap();
        assert_eq!(
            sha_first.as_slice(),
            hex_fixture("fe25e49f9757bc61d9b1ab1ec7c70879583b996ca270b4bd003c28a8b65c0348")
        );
        assert_eq!(
            sha_second.as_slice(),
            hex_fixture("73d393eb41bb0009ebdace87c8bad7a6b6278a550c21b230c56fbdc587574615")
        );

        let material = Request2Material {
            early_key_version: 5,
            early_key_send_version: 5,
            early_key_revert_version: 5,
            early_key_uses_revert: false,
            early_key_hashes_job_id: false,
            early_key_uses_ephemeral_override: false,
            server_early_public_key: server_public,
            normal_session_seed: None,
            auth: EarlyAuthData {
                auth_version: 6,
                preauth_blob: (0u8..32).collect(),
                auth_blob: (0xa0u8..0xb0).collect(),
            },
        };
        let crypto = Request2Crypto {
            client_secret,
            client_public,
            early_server_to_client: rx,
            early_client_to_server: tx,
            client_guid: 0x0102_0304_0506_0708,
        };
        let rupp = RuppProbeMaterial {
            token: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            rcc_endpoint: Endpoint {
                address: "10.32.8.208".into(),
                port: 50_704,
            },
            direct_server_return: false,
        };
        let prefix = build_rupp_header(&rupp, 2).unwrap();
        let packet = build_rbx_open_request2(
            1200,
            &prefix,
            "128.116.54.33:61201".parse().unwrap(),
            &material,
            &crypto,
        )
        .unwrap();
        assert_eq!(packet.len(), 1160);
        assert_eq!(&packet[..31], &prefix);
        assert_eq!(packet[31], RBX_OPEN_REQUEST_2);
        assert_eq!(packet[31 + 18], 89); // AAD length excludes RUPP
        assert_eq!(&packet[31 + 19..31 + 21], &43u16.to_be_bytes());
        assert_eq!(
            &packet[120..163],
            hex_fixture("69d9d6e24ca56453ca3acadbafe460a7b36a4ab6072a8c795c7f3216410d5b874404ef264cd782fe87e171")
        );
        assert_eq!(&packet[163..175], b"UniqueNumber");
        assert_eq!(
            &packet[175..191],
            hex_fixture("9e5ce60ac2be45853017e1cbd0d82b8b")
        );
        assert!(packet[191..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn authenticates_exact_open_reply_2_and_derives_normal_session_keys() {
        let client_secret = StaticSecret::from(
            <[u8; 32]>::try_from((1u8..=32).collect::<Vec<_>>()).unwrap(),
        );
        let client_public = PublicKey::from(&client_secret).to_bytes();
        let crypto = Request2Crypto {
            client_secret,
            client_public,
            early_server_to_client: hex_fixture(
                "4624d68db58f54ed39894a223ab64628bd09ff10626c053b94aea2524a80f7fc",
            )
            .try_into()
            .unwrap(),
            early_client_to_server: [0; 32],
            client_guid: 0x0102_0304_0506_0708,
        };
        // Independently generated with Python cryptography/OpenSSL primitives.
        // It includes the 2022 server's authenticated 28-byte zero tail.
        let packet = hex_fixture(concat!(
            "7d00ffff00fefefefefdfdfdfd123456780115004b",
            "2af743d91bc605fdaf9d5169e5fad1bb984c6925827236bdd2316d30f9170b67",
            "06016b4acbd2e8cd0f08cfb1df550ef667cd0eda1614196fdba8ee6519d1c878",
            "e1667f28256f70e4dd36c8ef8195fd8f9adec60f8e243af9ccd90b834d8c4c7",
            "0a69c266da84ffd556e697175654e756d62657256c133fd6fbb218b1fc0d4af68",
            "e1bf8c"
        ));
        let reply = parse_rbx_open_reply2(&packet, &crypto, None).unwrap();
        assert_eq!(reply.version, 1);
        assert_eq!(reply.server_capabilities, RAK_PEER_CAPABILITIES_0735_CLIENT_FLOOR);
        assert_eq!(reply.server_guid, 0x1112_1314_1516_1718);
        assert_eq!(reply.mtu, 1200);
        assert_eq!(reply.selected_encryption, 1);
        assert_eq!(
            reply.binding_address,
            Endpoint {
                address: "10.0.0.5".into(),
                port: 61_201,
            }
        );
        assert_eq!(reply.returned_rupp_token, Some([
            0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7,
            0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff,
        ]));
        assert_eq!(reply.returned_rupp_prefix, None);
        assert_eq!(
            reply.session_server_to_client.as_slice(),
            hex_fixture("649940507f84e6ae006913e4ca1d7595094a7555162fba87f2c8f37383c736ef")
        );
        assert_eq!(
            reply.session_client_to_server.as_slice(),
            hex_fixture("771b077ce6ed993d92009e89ce7c956a76fdb1d319e1560f28fd3d48aeaf18e9")
        );
    }

    #[test]
    fn authenticates_0735_routed_reply_with_rupp_extended_aad() {
        let client_secret = StaticSecret::from([7u8; 32]);
        let client_public = PublicKey::from(&client_secret).to_bytes();
        let crypto = Request2Crypto {
            client_secret,
            client_public,
            early_server_to_client: [0x42; 32],
            early_client_to_server: [0; 32],
            client_guid: 1,
        };
        let server_secret = StaticSecret::from([9u8; 32]);
        let server_public = PublicKey::from(&server_secret).to_bytes();

        // The live 0.735 shape has a 23-byte RUPP prefix. The server writes an
        // AAD length of 21 + 23, leaving the first 23 logical body bytes clear
        // and authenticated, then encrypts the remaining 35 body bytes plus
        // its reserved 28-byte zero tail. That is the observed header 58 / wire
        // 63 combination in a 158-byte routed datagram.
        let mut rupp = vec![RUPP_PROTOCOL_RAKNET, 0, 0, 23, RUPP_TLV_TOKEN, 17, 2];
        rupp.extend_from_slice(&[0xa5; 16]);
        assert_eq!(rupp.len(), 23);

        let mut body = Vec::new();
        body.extend_from_slice(&server_public);
        body.extend_from_slice(&RAK_PEER_CAPABILITIES_0735_CLIENT_FLOOR.to_be_bytes());
        body.extend_from_slice(&0x1112_1314_1516_1718u64.to_be_bytes());
        body.extend_from_slice(&1200u16.to_be_bytes());
        body.push(1);
        body.push(4);
        body.extend([10u8, 0, 0, 5].map(|byte| !byte));
        body.extend_from_slice(&61_201u16.to_be_bytes());
        assert_eq!(body.len(), 58);

        let mut aad = Vec::new();
        aad.push(RBX_OPEN_REPLY_2);
        aad.extend_from_slice(&OFFLINE_MAGIC);
        aad.push(1); // token refresh is in the outer RUPP TLV, not this body
        aad.push(44); // fixed 21-byte header + stripped 23-byte RUPP length
        aad.extend_from_slice(&58u16.to_be_bytes());
        aad.extend_from_slice(&body[..23]);
        assert_eq!(aad.len(), 44);

        let mut encrypted = body[23..].to_vec();
        encrypted.extend_from_slice(&[0; EARLY_AEAD_OVERHEAD]);
        assert_eq!(encrypted.len(), 63);
        let nonce = application_nonce(0);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&crypto.early_server_to_client));
        let tag = cipher
            .encrypt_in_place_detached(Nonce::from_slice(&nonce), &aad, &mut encrypted)
            .unwrap();

        let expected_rupp = rupp.clone();
        let mut packet = rupp;
        packet.extend_from_slice(&aad);
        packet.extend_from_slice(&encrypted);
        packet.extend_from_slice(&nonce);
        packet.extend_from_slice(tag.as_slice());
        assert_eq!(packet.len(), 158);

        let reply = parse_rbx_open_reply2(&packet, &crypto, None).unwrap();
        assert_eq!(reply.version, 1);
        assert_eq!(reply.server_guid, 0x1112_1314_1516_1718);
        assert_eq!(reply.mtu, 1200);
        assert_eq!(reply.selected_encryption, 1);
        assert_eq!(
            reply.binding_address,
            Endpoint {
                address: "10.0.0.5".into(),
                port: 61_201,
            }
        );
        assert_eq!(reply.returned_rupp_token, Some([0xa5; 16]));
        assert_eq!(reply.returned_rupp_token_type, Some(2));
        assert_eq!(reply.returned_rupp_prefix, Some(expected_rupp.clone()));
        assert_eq!(
            describe_received_rupp_prefix(&expected_rupp),
            "{protocol 1, flags 0x00, declared length 23, actual length 23, TLVs [token(length 17, subtype 2, value redacted)]}"
        );
    }

    #[test]
    fn reports_raknet_offline_error_after_open_request_2() {
        let client_secret = StaticSecret::from([7u8; 32]);
        let crypto = Request2Crypto {
            client_public: PublicKey::from(&client_secret).to_bytes(),
            client_secret,
            early_server_to_client: [0; 32],
            early_client_to_server: [0; 32],
            client_guid: 1,
        };
        let mut packet = vec![0x14];
        packet.extend_from_slice(&OFFLINE_MAGIC);
        packet.extend_from_slice(&0x1112_1314_1516_1718u64.to_be_bytes());
        let error = parse_rbx_open_reply2(&packet, &crypto, None).unwrap_err();
        assert!(error.contains("no free incoming connections"));
        assert!(error.contains("1112131415161718"));
    }

    #[test]
    fn parses_exact_rbx_open_reply_1() {
        let mut packet = vec![RBX_OPEN_REPLY_1];
        packet.extend_from_slice(&OFFLINE_MAGIC);
        packet.extend_from_slice(&0x0102_0304_0506_0708_u64.to_be_bytes());
        packet.push(0);
        packet.extend_from_slice(&1200_u16.to_be_bytes());

        let reply = parse_rbx_open_reply1(&packet).unwrap();
        assert_eq!(reply.server_guid, 0x0102_0304_0506_0708);
        assert!(!reply.encryption_enabled);
        assert_eq!(reply.mtu, 1200);

        packet[0] = 0x1c;
        assert!(parse_rbx_open_reply1(&packet).is_err());
    }

    #[test]
    fn finds_endpoints_in_nested_team_create_wrappers() {
        let config = serde_json::json!({
            "status": 2,
            "joinTicket": "ticket-value",
            "settings": {
                "ServerPort": 53640,
                "UdmuxEndpoints": [
                    { "Address": "128.116.1.2", "Port": 53641 },
                    { "Address": "128.116.1.3" }
                ],
                "ServerConnections": [
                    { "Address": "10.0.0.2", "Port": 53640 }
                ]
            }
        });
        let endpoints = parse_join_config(&config);
        assert_eq!(
            endpoints,
            vec![
                Endpoint { address: "128.116.1.2".into(), port: 53641 },
                Endpoint { address: "128.116.1.3".into(), port: 53640 },
            ]
        );
        // The private RCC target is retained only when no public UDMUX target
        // exists; otherwise it cannot be reached from an Internet client.
        assert_eq!(
            parse_join_config(&serde_json::json!({
                "settings": { "MachineAddress": "10.0.0.2", "ServerPort": 53640 }
            })),
            vec![Endpoint { address: "10.0.0.2".into(), port: 53640 }]
        );
    }

    #[test]
    fn finds_endpoints_in_json_string_and_rejects_all_null_response() {
        let wrapped = serde_json::json!({
            "joinTicket": "{\"settings\":{\"Address\":\"127.0.0.1\",\"ServerPort\":5000}}"
        });
        assert_eq!(
            parse_join_config(&wrapped),
            vec![Endpoint { address: "127.0.0.1".into(), port: 5000 }]
        );

        assert!(join_response_is_all_null(&serde_json::json!({
            "joinTicket": null,
            "settings": null,
            "message": null
        })));
        assert!(join_response_is_all_null(&serde_json::json!({})));
        assert!(!join_response_is_all_null(&serde_json::json!({
            "message": "starting"
        })));
    }
}
