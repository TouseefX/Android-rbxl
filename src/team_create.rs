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
//! - the first exact packet exchange of Roblox's customized 2022 RakNet
//!   handshake: `RbxOpenRequest1` (0x7b) / `RbxOpenReply1` (0x7e).
//!
//! The remaining encrypted open-request-2 exchange, RakNet reliability
//! layer, and JoinData → live change-item application make up the rest of
//! the Stage 1 engine.

use std::net::{IpAddr, UdpSocket};
use std::time::{Duration, Instant};

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
pub const RBX_PROTOCOL_VERSION: u8 = 5;
pub const DEFAULT_PROBE_MTU: u16 = 1200;
const IPV6_UDP_HEADER_BYTES: usize = 40;

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
    let mtu = usize::from(mtu);
    if mtu < 576 {
        return Err(format!("RbxOpenRequest1 MTU {mtu} is below the 576-byte protocol minimum"));
    }
    let payload_len = mtu
        .checked_sub(IPV6_UDP_HEADER_BYTES)
        .ok_or_else(|| "RbxOpenRequest1 MTU underflow".to_string())?;
    let mut packet = vec![0_u8; payload_len];
    packet[0] = RBX_OPEN_REQUEST_1;
    packet[1..17].copy_from_slice(&OFFLINE_MAGIC);
    packet[17] = RBX_PROTOCOL_VERSION;
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

/// Perform the first real customized-RakNet handshake exchange. A valid
/// reply proves both UDP reachability and that the target is a compatible
/// Roblox server; unlike a generic unconnected ping, this is the request
/// Studio sends immediately before encrypted open-request-2.
pub fn probe_endpoint(endpoint: &Endpoint, timeout_ms: u64) -> Result<String, String> {
    const PROBE_MTUS: [u16; 3] = [1492, DEFAULT_PROBE_MTU, 576];
    const ROUNDS: usize = 2;

    let target = endpoint.label();
    let socket = UdpSocket::bind(endpoint.bind_address())
        .map_err(|e| format!("{target}: bind failed: {e}"))?;
    let started = Instant::now();
    let deadline = started + Duration::from_millis(timeout_ms.max(300));

    // RakPeer normally tries several MTU candidates and retransmits its UDP
    // probe. A single 1200-byte datagram was too easy to lose and could turn
    // ordinary UDP loss into a false negative.
    let mut sent = 0usize;
    for _ in 0..ROUNDS {
        for mtu in PROBE_MTUS {
            let packet = build_rbx_open_request1(mtu)?;
            socket
                .send_to(&packet, (endpoint.address.as_str(), endpoint.port))
                .map_err(|e| format!("{target}: RbxOpenRequest1 send failed: {e}"))?;
            sent += 1;
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    let mut buf = [0u8; 2048];
    let mut invalid_replies = Vec::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        socket
            .set_read_timeout(Some(remaining.max(Duration::from_millis(1))))
            .map_err(|e| format!("{target}: socket timeout: {e}"))?;
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => match parse_rbx_open_reply1(&buf[..n]) {
                Ok(reply) => {
                    return Ok(format!(
                        "{target}: ✅ RbxOpenReply1 — {n} bytes from {from}, server GUID {:016x}, MTU {}, encryption {}, RTT {} ms",
                        reply.server_guid,
                        reply.mtu,
                        if reply.encryption_enabled { "requested" } else { "deferred to request 2" },
                        started.elapsed().as_millis()
                    ));
                }
                Err(reason) => invalid_replies.push(format!("{n} bytes from {from}: {reason}")),
            },
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(e) => return Err(format!("{target}: UDP receive failed: {e}")),
        }
    }

    if !invalid_replies.is_empty() {
        return Err(format!(
            "{target}: received UDP after {sent} probes, but no valid 2022 RbxOpenReply1 ({})",
            invalid_replies.join("; ")
        ));
    }
    if is_internal_address(&endpoint.address) {
        Err(format!(
            "{target}: private/internal server address is not routable from this phone; a public UDMUX endpoint is required"
        ))
    } else {
        Err(format!(
            "{target}: endpoint found, but no reply to {sent} bare 2022 RbxOpenRequest1 probes in {timeout_ms} ms. This public UDMUX path likely requires its session/token/crypto outer framing before it will pass the RakNet packet; endpoint detection itself succeeded"
        ))
    }
}

/// Probe up to `max` endpoints from a join config and produce a readable
/// multi-line report.
pub fn probe_join_config(config: &serde_json::Value, max: usize, timeout_ms: u64) -> String {
    let all_endpoints = parse_all_join_endpoints(config);
    let internal_count = all_endpoints
        .iter()
        .filter(|e| is_internal_address(&e.address))
        .count();
    let endpoints = prefer_public_endpoints(all_endpoints);
    if endpoints.is_empty() {
        return "No server endpoints found in the join config (unexpected shape — check the raw JSON keys)".into();
    }
    let mut heading = format!(
        "{} usable endpoint(s) in join config; probing {}…",
        endpoints.len(),
        endpoints.len().min(max)
    );
    if internal_count > 0 && endpoints.iter().all(|e| !is_internal_address(&e.address)) {
        heading.push_str(&format!(
            " ({internal_count} private RCC address(es) ignored as non-routable)"
        ));
    }
    let mut lines = vec![heading];
    for endpoint in endpoints.iter().take(max) {
        match probe_endpoint(endpoint, timeout_ms) {
            Ok(line) => lines.push(line),
            Err(line) => lines.push(line),
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

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
