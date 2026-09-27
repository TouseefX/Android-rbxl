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
//! - a raw UDP reachability probe of those endpoints (a RakNet-style
//!   unconnected ping), which proves the device can actually reach the
//!   cloud-edit server's socket before any protocol work begins.
//!
//! The full replication client (custom RakNet handshake, bitstream codec,
//! ClassInfo dictionaries, JoinDataItemV2 → live change items) is the
//! doc's Stage 1+ engine work and needs the `network/*.c` decompile
//! sources for the exact wire formats.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub address: String,
    pub port: u16,
}

impl Endpoint {
    pub fn label(&self) -> String {
        format!("{}:{}", self.address, self.port)
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

    if let (Some(address), Some(port)) = (
        get_ci(v, "Address").and_then(as_addr),
        get_ci(v, "Port").and_then(as_port),
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

/// Extract every server endpoint we can find in a team-create join config.
/// Checks the top level plus the common nesting levels (`joinScript`,
/// `serverConnections` array entries).
pub fn parse_join_config(v: &serde_json::Value) -> Vec<Endpoint> {
    let mut out = Vec::new();
    collect_level(v, &mut out);
    if let Some(js) = get_ci(v, "joinScript") {
        collect_level(js, &mut out);
    }
    for key in ["serverConnections", "ServerConnections"] {
        if let Some(arr) = get_ci(v, key).and_then(|c| c.as_array()) {
            for e in arr {
                collect_level(e, &mut out);
            }
        }
    }
    out
}

/// RakNet offline-message magic (stock RakNet, which Roblox's fork keeps
/// for the unconnected/offline packet family).
const OFFLINE_MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56,
    0x78,
];

/// Send one RakNet-style unconnected ping (packet id 0x01) to an endpoint
/// and wait briefly for ANY reply. This is a REACHABILITY probe, not a
/// protocol handshake: a reply (usually 0x1c unconnected pong) proves UDP
/// egress + the server socket is live; silence can mean either a filtered
/// path or a server that ignores unconnected pings — both worth knowing
/// before building the real connection layer.
pub fn probe_endpoint(endpoint: &Endpoint, timeout_ms: u64) -> Result<String, String> {
    let target = endpoint.label();
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("{target}: bind failed: {e}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(timeout_ms.max(100))))
        .map_err(|e| format!("{target}: socket timeout: {e}"))?;

    // [0x01][u64 send-time BE][16B offline magic][u64 client GUID]
    let mut packet = Vec::with_capacity(33);
    packet.push(0x01);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    packet.extend_from_slice(&now_ms.to_be_bytes());
    packet.extend_from_slice(&OFFLINE_MAGIC);
    packet.extend_from_slice(&now_ms.wrapping_mul(0x9E3779B97F4A7C15).to_be_bytes());

    let started = Instant::now();
    socket
        .send_to(&packet, target.as_str())
        .map_err(|e| format!("{target}: send failed: {e}"))?;

    let mut buf = [0u8; 2048];
    match socket.recv_from(&mut buf) {
        Ok((n, from)) => Ok(format!(
            "{target}: ✅ reply — {n} bytes from {from}, packet id 0x{:02x}, RTT {} ms",
            buf[0],
            started.elapsed().as_millis()
        )),
        Err(_) => Err(format!(
            "{target}: no reply in {timeout_ms} ms (path filtered, or server ignores unconnected pings)"
        )),
    }
}

/// Probe up to `max` endpoints from a join config and produce a readable
/// multi-line report.
pub fn probe_join_config(config: &serde_json::Value, max: usize, timeout_ms: u64) -> String {
    let endpoints = parse_join_config(config);
    if endpoints.is_empty() {
        return "No server endpoints found in the join config (unexpected shape — check the raw JSON keys)".into();
    }
    let mut lines = vec![format!(
        "{} endpoint(s) in join config; probing {}…",
        endpoints.len(),
        endpoints.len().min(max)
    )];
    for endpoint in endpoints.iter().take(max) {
        match probe_endpoint(endpoint, timeout_ms) {
            Ok(line) => lines.push(line),
            Err(line) => lines.push(line),
        }
    }
    lines.join("\n")
}
