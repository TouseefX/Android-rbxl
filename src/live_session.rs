//! Live Session bridge.
//!
//! Roblox Studio's Team Create / RCC replication protocol is proprietary and
//! can't be spoken by a third-party client directly. Instead this module
//! implements the **companion-plugin pattern** used by every serious external
//! Studio tool (Rojo, the MCP server, etc.):
//!
//! 1. The user installs a small **Roblox plugin** (`rbxl_editor_bridge.rbxm`)
//!    into their `Plugins` folder. The plugin opens a WebSocket to this app
//!    (`ws://127.0.0.1:41742` by default).
//! 2. This app runs a tiny WebSocket server that accepts that connection.
//! 3. The plugin sends JSON-RPC style messages: `{ id, method, params }`, and
//!    the app can call back into Studio the same way.
//!
//! We deliberately keep the wire protocol JSON (not msgpack) so it's easy to
//! inspect and so the Luau side has no native dependencies.
//!
//! When a live session is connected the command bar and plugin runner can
//! choose "Execute in Studio" instead of running locally in luaur, which
//! means scripts get the *real* engine: `game`, `workspace`, `Selection`,
//! `ChangeHistoryService`, rendering, physics — everything that the local
//! sandbox can't provide.
//!
//! No actual WebSocket frames are parsed here beyond handshake + text framing
//! because we pull in a minimal framing implementation rather than a heavy
//! dependency. The server listens with a plain `std::net::TcpListener`; the
//! Bevy UI polls `try_recv()` each frame so it never blocks the render loop.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

/// Default port the companion plugin connects back to.
pub const DEFAULT_PORT: u16 = 41742;

/// A single JSON-RPC-ish message exchanged with the Studio plugin.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RpcMessage {
    /// Optional request id; omitted for notifications (no response expected).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Method name, e.g. "run_command", "get_selection", "push_dom".
    pub method: String,
    /// JSON params payload.
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// A client (the Studio plugin) just connected.
    Connected { peer: String },
    /// The client disconnected.
    Disconnected { peer: String },
    /// We received a message from the plugin.
    Message(RpcMessage),
    /// The server itself failed to start / crashed.
    ServerError(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Stopped,
    Listening,
    Connected,
}

/// All UI-visible state for the live session feature.
pub struct LiveSessionState {
    pub status: SessionStatus,
    pub port: u16,
    pub log: Vec<String>,
    /// Next request id for calls we originate.
    next_id: u64,
    /// Queue of messages to send to the connected client.
    outbox: Arc<Mutex<VecDeque<RpcMessage>>>,
    shutdown: Arc<Mutex<bool>>,
    // Wrapped in a Mutex because `mpsc::Receiver` is `!Sync` and this struct
    // lives inside a Bevy `Resource` (required to be `Send + Sync`).
    events_rx: Option<Mutex<Receiver<SessionEvent>>>,
    _server_thread: Option<thread::JoinHandle<()>>,
}

impl Default for LiveSessionState {
    fn default() -> Self {
        Self {
            status: SessionStatus::Stopped,
            port: DEFAULT_PORT,
            log: vec![format!("Live session bridge default port: {DEFAULT_PORT}")],
            next_id: 1,
            outbox: Arc::new(Mutex::new(VecDeque::new())),
            shutdown: Arc::new(Mutex::new(false)),
            events_rx: None,
            _server_thread: None,
        }
    }
}

impl LiveSessionState {
    /// Start listening. Non-blocking: spawns a background thread that accepts
    /// connections and pumps events back through a channel.
    pub fn start(&mut self) {
        if self.status != SessionStatus::Stopped {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let port = self.port;
        let outbox = self.outbox.clone();
        let shutdown = self.shutdown.clone();

        let handle = thread::Builder::new()
            .name("live-session".into())
            .spawn(move || run_server(port, tx, outbox, shutdown))
            .ok();

        if handle.is_none() {
            self.log.push("Failed to spawn live-session thread".into());
            return;
        }
        self._server_thread = handle;
        self.events_rx = Some(Mutex::new(rx));
        *self.shutdown.lock().unwrap() = false;
        self.status = SessionStatus::Listening;
        self.log.push(format!("Listening on ws://127.0.0.1:{port} — install the companion plugin in Studio to connect."));
    }

    /// Signal the server thread to stop. We don't join the thread (it may be
    /// blocked in accept()) but the next incoming connection / timeout will
    /// see the flag and exit.
    pub fn stop(&mut self) {
        *self.shutdown.lock().unwrap() = true;
        self.status = SessionStatus::Stopped;
        self.log.push("Live session stopped.".into());
    }

    /// Drain any events queued by the server thread. Called once per UI frame.
    pub fn poll_events(&mut self) -> Vec<SessionEvent> {
        let Some(rx) = &self.events_rx else { return Vec::new() };
        let rx = rx.lock().unwrap();
        let mut events = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(ev) => events.push(ev),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.status = SessionStatus::Stopped;
                    break;
                }
            }
        }
        drop(rx);
        for ev in &events {
            match ev {
                SessionEvent::Connected { peer } => {
                    self.status = SessionStatus::Connected;
                    self.log.push(format!("Studio connected from {peer}"));
                }
                SessionEvent::Disconnected { peer } => {
                    self.status = SessionStatus::Listening;
                    self.log.push(format!("Studio disconnected ({peer})"));
                }
                SessionEvent::Message(m) => {
                    self.log.push(format!("← {} {}", m.method, m.params));
                }
                SessionEvent::ServerError(e) => {
                    self.log.push(format!("server error: {e}"));
                }
            }
        }
        events
    }

    /// Queue a message to be sent to the connected plugin, if any. Returns
    /// the assigned request id so callers can correlate the response.
    pub fn send(&mut self, method: &str, params: serde_json::Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let msg = RpcMessage {
            id: Some(id),
            method: method.to_string(),
            params,
        };
        self.outbox.lock().unwrap().push_back(msg);
        id
    }

    /// Ask the connected Studio to run a Luau snippet and return its result.
    pub fn run_in_studio(&mut self, source: &str) {
        self.send(
            "run_command",
            serde_json::json!({ "source": source }),
        );
        self.log.push("→ sent run_command to Studio".into());
    }
}

// ---------------------------------------------------------------------------
// Minimal WebSocket server (RFC 6455, server-side text frames only). Enough to
// talk to a Luau WebSocket client without a heavy dependency.
// ---------------------------------------------------------------------------

fn run_server(
    port: u16,
    tx: Sender<SessionEvent>,
    outbox: Arc<Mutex<VecDeque<RpcMessage>>>,
    shutdown: Arc<Mutex<bool>>,
) {
    let bind = format!("127.0.0.1:{port}");
    let listener = match TcpListener::bind(&bind) {
        Ok(l) => l,
        Err(e) => {
            let _ = tx.send(SessionEvent::ServerError(format!("bind {bind}: {e}")));
            return;
        }
    };
    // Non-blocking accept so we can honor the shutdown flag.
    let _ = listener.set_nonblocking(true);

    while !*shutdown.lock().unwrap() {
        match listener.accept() {
            Ok((stream, peer)) => {
                let peer = peer.to_string();
                if do_handshake(&stream).is_ok() {
                    let _ = tx.send(SessionEvent::Connected { peer: peer.clone() });
                    // Pump this single connection (blocking for its lifetime).
                    serve_connection(stream, &tx, &outbox, &shutdown);
                    let _ = tx.send(SessionEvent::Disconnected { peer });
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_millis(150));
            }
            Err(e) => {
                let _ = tx.send(SessionEvent::ServerError(format!("accept: {e}")));
                thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}

/// Perform the RFC 6455 opening handshake. Reads the client's Upgrade headers
/// and writes the `101 Switching Protocols` response.
fn do_handshake(stream: &TcpStream) -> std::io::Result<()> {
    let mut reader = stream.try_clone()?;
    reader.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 1];
    loop {
        let n = reader.read(&mut tmp)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof in handshake"));
        }
        buf.push(tmp[0]);
        // Wait for the header terminator \r\n\r\n.
        if buf.len() > 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        if buf.len() > 16 * 1024 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "handshake too large"));
        }
    }

    let req = String::from_utf8_lossy(&buf);
    let key = req
        .lines()
        .find_map(|l| {
            let mut parts = l.splitn(2, ':');
            let name = parts.next()?.trim().to_ascii_lowercase();
            let val = parts.next()?.trim();
            (name == "sec-websocket-key").then(|| val.to_string())
        })
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no Sec-WebSocket-Key"))?;

    // Server accept = base64(SHA1(key + GUID)).
    let mut sha = Sha1::new();
    sha.update(key.as_bytes());
    sha.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept = base64_encode(&sha.finalize());

    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    let mut writer = stream.try_clone()?;
    writer.write_all(resp.as_bytes())?;
    writer.flush()?;
    Ok(())
}

fn serve_connection(
    mut stream: TcpStream,
    tx: &Sender<SessionEvent>,
    outbox: &Arc<Mutex<VecDeque<RpcMessage>>>,
    shutdown: &Arc<Mutex<bool>>,
) {
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(200)))
        .ok();
    let mut writer = stream.try_clone().ok();

    loop {
        if *shutdown.lock().unwrap() {
            return;
        }

        // Drain outbox → client.
        if let Some(w) = &mut writer {
            while let Some(msg) = outbox.lock().unwrap().pop_front() {
                if let Ok(json) = serde_json::to_string(&msg) {
                    if send_text_frame(w, &json).is_err() {
                        return;
                    }
                }
            }
        }

        // Read one incoming frame.
        match read_frame(&mut stream) {
            Ok(Frame::Text(text)) => {
                if let Ok(msg) = serde_json::from_str::<RpcMessage>(&text) {
                    let _ = tx.send(SessionEvent::Message(msg));
                }
            }
            Ok(Frame::Binary(_)) => { /* ignore */ }
            Ok(Frame::Close) => return,
            Ok(Frame::Ping(data)) => {
                if let Some(w) = &mut writer {
                    let _ = send_pong(w, &data);
                }
            }
            Ok(Frame::Pong(_)) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                // idle; loop back to drain outbox / check shutdown
            }
            Err(_) => return,
        }
    }
}

enum Frame {
    Text(String),
    Binary(Vec<u8>),
    Close,
    Ping(Vec<u8>),
    Pong(Vec<u8>),
}

fn read_frame(stream: &mut TcpStream) -> std::io::Result<Frame> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header)?;
    let fin = header[0] & 0x80 != 0;
    let opcode = header[0] & 0x0F;
    let masked = header[1] & 0x80 != 0;
    let mut len = (header[1] & 0x7F) as u64;
    if len == 126 {
        let mut b = [0u8; 2];
        stream.read_exact(&mut b)?;
        len = u16::from_be_bytes(b) as u64;
    } else if len == 127 {
        let mut b = [0u8; 8];
        stream.read_exact(&mut b)?;
        len = u64::from_be_bytes(b);
    }
    // Server-side frames MUST be masked, but be lenient if they aren't.
    let mask = if masked {
        let mut m = [0u8; 4];
        stream.read_exact(&mut m)?;
        Some(m)
    } else {
        None
    };
    // Cap at 16 MiB to prevent OOM on bad data.
    if len > 16 * 1024 * 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload)?;
    if let Some(m) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= m[i % 4];
        }
    }
    let _ = fin;
    Ok(match opcode {
        0x1 => Frame::Text(String::from_utf8_lossy(&payload).into_owned()),
        0x2 => Frame::Binary(payload),
        0x8 => Frame::Close,
        0x9 => Frame::Ping(payload),
        0xA => Frame::Pong(payload),
        _ => Frame::Binary(payload),
    })
}

fn send_text_frame(stream: &mut TcpStream, text: &str) -> std::io::Result<()> {
    let payload = text.as_bytes();
    let mut header = Vec::with_capacity(10);
    header.push(0x81); // FIN + text
    let len = payload.len() as u64;
    if len < 126 {
        header.push(len as u8);
    } else if len <= u16::MAX as u64 {
        header.push(126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&len.to_be_bytes());
    }
    stream.write_all(&header)?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

fn send_pong(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    stream.write_all(&[0x8A, data.len() as u8])?;
    stream.write_all(data)?;
    stream.flush()
}

// ---------------------------------------------------------------------------
// Tiny SHA-1 + base64 — only used once per handshake; avoids a dep.
// ---------------------------------------------------------------------------

struct Sha1 {
    h: [u32; 5],
    buf: Vec<u8>,
    total: u64,
}
impl Sha1 {
    fn new() -> Self {
        Self {
            h: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            buf: Vec::new(),
            total: 0,
        }
    }
    fn update(&mut self, data: &[u8]) {
        self.total += data.len() as u64;
        self.buf.extend_from_slice(data);
        while self.buf.len() >= 64 {
            let block: [u8; 64] = self.buf[..64].try_into().unwrap();
            self.process_block(&block);
            self.buf.drain(..64);
        }
    }
    fn finalize(mut self) -> [u8; 20] {
        let bit_len = self.total * 8;
        self.buf.push(0x80);
        while self.buf.len() % 64 != 56 {
            self.buf.push(0);
        }
        self.buf.extend_from_slice(&bit_len.to_be_bytes());
        let blocks: Vec<[u8; 64]> = self
            .buf
            .chunks(64)
            .map(|c| c.try_into().unwrap())
            .collect();
        for block in &blocks {
            self.process_block(block);
        }
        let mut out = [0u8; 20];
        for (i, word) in self.h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
    fn process_block(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (self.h[0], self.h[1], self.h[2], self.h[3], self.h[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
    }
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// The Luau source for the companion Studio plugin. The app can write this to
/// the user's Plugins folder via a SAF document for one-click install.
pub const COMPANION_PLUGIN_SOURCE: &str = include_str!("../assets/companion_plugin.lua");
