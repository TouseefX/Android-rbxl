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
//! - the exact offline exchanges of Roblox's customized 2022 RakNet
//!   handshake: RUPP-routed `RbxOpenRequest1`/`RbxOpenReply1`, followed on
//!   the same UDP socket by authenticated and encrypted
//!   `RbxOpenRequest2`/`RbxOpenReply2`.
//!
//! Connected RakNet reliability and JoinData → live change-item application
//! make up the rest of the Stage 1 engine.

use blake2::{
    digest::{consts::U32, Mac},
    Blake2b512, Blake2bMac, Digest,
};
use chacha20poly1305::{
    aead::{AeadInPlace, KeyInit},
    ChaCha20Poly1305, Key, Nonce, Tag,
};
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
// Studio 0.735 and PlayerConfigurer generate the legacy RakNet application
// with the URL-decoded Team Create key at id/send/revert 5. This remains
// distinct from RbxTransportEphemeralEarlyPublicKey, whose generated id is 1.
const RAKNET_EPHEMERAL_EARLY_KEY_VERSION: u16 = 5;
pub const DEFAULT_PROBE_MTU: u16 = 1200;
const IPV6_UDP_HEADER_BYTES: usize = 40;
const EARLY_AEAD_OVERHEAD: usize = 28; // 12-byte nonce + 16-byte detached tag
const RAK_PEER_CAPABILITIES_2022_BASE: u64 = 0x0000_0002_321e_7e1e;

// Exact 2022 RUPP values from Rupp::{serialize,TokenTlv,Ipv4Tlv,Ipv6Tlv}.
const RUPP_PROTOCOL_RAKNET: u8 = 1;
const RUPP_FLAG_DIRECT_SERVER_RETURN: u8 = 1;
const RUPP_TLV_TOKEN: u8 = 1;
const RUPP_TLV_IPV4: u8 = 2;
const RUPP_TLV_IPV6: u8 = 3;
const RUPP_TOKEN_VALUE_LENGTH: u8 = 17; // subtype byte + 16-byte token

#[derive(Clone, Debug)]
struct RuppProbeMaterial {
    token: [u8; 16],
    rcc_endpoint: Endpoint,
    direct_server_return: bool,
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
    Ok(RuppProbeMaterial {
        token,
        rcc_endpoint,
        direct_server_return,
    })
}

/// Serialize the exact RUPP prefix built by 2022 RakPeer::setupRupp. Studio
/// adds the token TLV first and the private RCC endpoint TLV second, then
/// prepends this header to even the offline RbxOpenRequest1 packet.
fn build_rupp_header(
    material: &RuppProbeMaterial,
    token_type: u8,
) -> Result<Vec<u8>, String> {
    if !(1..=2).contains(&token_type) {
        return Err(format!("unsupported 2022 RUPP token subtype {token_type}"));
    }
    let mut tlvs = Vec::with_capacity(27);
    tlvs.extend_from_slice(&[RUPP_TLV_TOKEN, RUPP_TOKEN_VALUE_LENGTH, token_type]);
    tlvs.extend_from_slice(&material.token);
    match material.rcc_endpoint.address.parse::<IpAddr>() {
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
                material.rcc_endpoint.address
            ));
        }
    }
    tlvs.extend_from_slice(&material.rcc_endpoint.port.to_be_bytes());
    let header_len = 4usize
        .checked_add(tlvs.len())
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(|| "RUPP header is too long".to_string())?;
    let mut header = Vec::with_capacity(usize::from(header_len));
    header.push(RUPP_PROTOCOL_RAKNET);
    header.push(if material.direct_server_return {
        RUPP_FLAG_DIRECT_SERVER_RETURN
    } else {
        0
    });
    header.extend_from_slice(&header_len.to_be_bytes());
    header.extend_from_slice(&tlvs);
    Ok(header)
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
/// id/send/revert 5, and does not parse `ClientPublicKeyData`. Only when the
/// ephemeral value is absent or empty does Studio parse the supplied KeyRing.
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
    Ok(Request2Material {
        early_key_version: early_key.version,
        early_key_send_version: early_key.send_version,
        early_key_revert_version: early_key.revert_version,
        early_key_uses_revert: early_key.uses_revert,
        early_key_hashes_job_id: early_key.hashes_job_id,
        early_key_uses_ephemeral_override: early_key.uses_ephemeral_override,
        server_early_public_key: early_key.bytes,
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
    plaintext.extend_from_slice(&RAK_PEER_CAPABILITIES_2022_BASE.to_be_bytes());
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

struct RbxOpenReply2 {
    version: u8,
    server_capabilities: u64,
    server_guid: u64,
    mtu: u16,
    selected_encryption: u8,
    binding_address: Endpoint,
    returned_rupp_token: Option<[u8; 16]>,
    _session_server_to_client: [u8; 32],
    _session_client_to_server: [u8; 32],
}

fn strip_optional_rupp_prefix(packet: &[u8]) -> Result<&[u8], String> {
    if packet.first() != Some(&RUPP_PROTOCOL_RAKNET) {
        return Ok(packet);
    }
    if packet.len() < 4 {
        return Err("truncated RUPP response prefix".into());
    }
    let header_len = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if header_len < 4 || header_len > packet.len() {
        return Err(format!("invalid RUPP response header length {header_len}"));
    }
    Ok(&packet[header_len..])
}

fn parse_rbx_open_reply2(
    packet: &[u8],
    crypto: &Request2Crypto,
) -> Result<RbxOpenReply2, String> {
    let packet = strip_optional_rupp_prefix(packet)?;
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
    let minimum_len = aad_len
        .checked_add(ciphertext_len)
        .and_then(|length| length.checked_add(EARLY_AEAD_OVERHEAD))
        .ok_or_else(|| "RbxOpenReply2 length overflow".to_string())?;
    if packet.len() < minimum_len {
        return Err(format!(
            "RbxOpenReply2 is truncated: header needs at least {minimum_len} bytes, datagram has {}",
            packet.len()
        ));
    }
    // The 2022 server reserves 28 zero bytes before calling earlyEncryptData.
    // Those bytes become additional authenticated ciphertext, after which
    // earlyEncryptData appends the real nonce and tag. The header's
    // ciphertextLen still describes only the meaningful body. Roblox's client
    // therefore decrypts every byte between AAD and the final 28-byte suffix
    // and simply leaves this zero tail unread.
    let ciphertext_end = packet.len() - EARLY_AEAD_OVERHEAD;
    let actual_ciphertext_len = ciphertext_end - aad_len;
    if actual_ciphertext_len != ciphertext_len
        && actual_ciphertext_len != ciphertext_len + EARLY_AEAD_OVERHEAD
    {
        return Err(format!(
            "RbxOpenReply2 ciphertext length mismatch: header says {ciphertext_len}, wire has {actual_ciphertext_len}"
        ));
    }
    let nonce = &packet[ciphertext_end..ciphertext_end + 12];
    let tag = Tag::from_slice(&packet[ciphertext_end + 12..]);
    let mut plaintext = packet[aad_len..ciphertext_end].to_vec();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&crypto.early_server_to_client));
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(nonce),
            &packet[..aad_len],
            &mut plaintext,
            tag,
        )
        .map_err(|_| "RbxOpenReply2 early ChaCha20-Poly1305 authentication failed".to_string())?;

    if plaintext.len() < 32 + 8 + 8 + 2 + 1 + 7 {
        return Err(format!(
            "RbxOpenReply2 decrypted body is too short: {} bytes",
            plaintext.len()
        ));
    }
    let mut cursor = 0usize;
    let server_ephemeral_key: [u8; 32] = plaintext[cursor..cursor + 32].try_into().unwrap();
    cursor += 32;
    let server_capabilities = u64::from_be_bytes(plaintext[cursor..cursor + 8].try_into().unwrap());
    cursor += 8;
    let server_guid = u64::from_be_bytes(plaintext[cursor..cursor + 8].try_into().unwrap());
    cursor += 8;
    let mtu = u16::from_be_bytes(plaintext[cursor..cursor + 2].try_into().unwrap());
    cursor += 2;
    let selected_encryption = plaintext[cursor];
    cursor += 1;
    let binding_address = read_system_address(&plaintext, &mut cursor)?;
    let returned_rupp_token = if version != 0 {
        let token_len = *plaintext
            .get(cursor)
            .ok_or_else(|| "RbxOpenReply2 has no returned RUPP token length".to_string())?
            as usize;
        cursor += 1;
        if token_len != 16 {
            return Err(format!(
                "RbxOpenReply2 returned RUPP token has length {token_len}, expected 16"
            ));
        }
        let token: [u8; 16] = plaintext
            .get(cursor..cursor + token_len)
            .ok_or_else(|| "RbxOpenReply2 returned RUPP token is truncated".to_string())?
            .try_into()
            .unwrap();
        cursor += token_len;
        Some(token)
    } else {
        None
    };
    let trailing = &plaintext[cursor..];
    if !trailing.is_empty()
        && (trailing.len() != EARLY_AEAD_OVERHEAD || trailing.iter().any(|byte| *byte != 0))
    {
        return Err(format!(
            "RbxOpenReply2 has {} unexpected decrypted trailing byte(s)",
            trailing.len()
        ));
    }
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
    Ok(RbxOpenReply2 {
        version,
        server_capabilities,
        server_guid,
        mtu,
        selected_encryption,
        binding_address,
        returned_rupp_token,
        _session_server_to_client: session_server_to_client,
        _session_client_to_server: session_client_to_server,
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

fn build_rbx_open_request1_with_prefix(mtu: u16, prefix: &[u8]) -> Result<Vec<u8>, String> {
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

fn parse_probe_reply(packet: &[u8]) -> Result<RbxOpenReply1, String> {
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
                match parse_rbx_open_reply2(&buf[..length], &crypto) {
                    Ok(reply2) => {
                        if reply2.server_guid != reply1.server_guid {
                            return Err(format!(
                                "{target}: OpenReply2 server GUID {:016x} does not match OpenReply1 {:016x}",
                                reply2.server_guid, reply1.server_guid
                            ));
                        }
                        let encryption = match reply2.selected_encryption {
                            1 => "ChaCha20-Poly1305",
                            2 => "AES-256-GCM",
                            _ => unreachable!(),
                        };
                        return Ok(format!(
                            "{target}: ✅ {request1_summary}\n✅ RbxOpenReply2 — {length} bytes from {reply_from}, version {}, server GUID {:016x}, MTU {}, encryption {encryption}, binding {}, capabilities 0x{:016x}, session keys derived{}, handshake elapsed {} ms",
                            reply2.version,
                            reply2.server_guid,
                            reply2.mtu,
                            reply2.binding_address.label(),
                            reply2.server_capabilities,
                            if reply2.returned_rupp_token.is_some() {
                                ", refreshed RUPP token received"
                            } else {
                                ""
                            },
                            started.elapsed().as_millis()
                        ));
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
        Ok(material) => heading.push_str(&format!(
            "\n2022 RUPP routing ready: token TLV + RCC {}",
            material.rcc_endpoint.label()
        )),
        Err(reason) => heading.push_str(&format!("\nRUPP routing unavailable: {reason}")),
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
    if let Some(rcc_version) = find_field_ci(config, "RccVersion", 0)
        .and_then(|value| value.as_str().map(str::to_owned))
    {
        heading.push_str(&format!("\nAdvertised RCC version: {rcc_version}"));
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

        let material = Request2Material {
            early_key_version: 5,
            early_key_send_version: 5,
            early_key_revert_version: 5,
            early_key_uses_revert: false,
            early_key_hashes_job_id: false,
            early_key_uses_ephemeral_override: false,
            server_early_public_key: server_public,
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
        let reply = parse_rbx_open_reply2(&packet, &crypto).unwrap();
        assert_eq!(reply.version, 1);
        assert_eq!(reply.server_capabilities, RAK_PEER_CAPABILITIES_2022_BASE);
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
        assert_eq!(
            reply._session_server_to_client.as_slice(),
            hex_fixture("649940507f84e6ae006913e4ca1d7595094a7555162fba87f2c8f37383c736ef")
        );
        assert_eq!(
            reply._session_client_to_server.as_slice(),
            hex_fixture("771b077ce6ed993d92009e89ce7c956a76fdb1d319e1560f28fd3d48aeaf18e9")
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
        let error = parse_rbx_open_reply2(&packet, &crypto).unwrap_err();
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
