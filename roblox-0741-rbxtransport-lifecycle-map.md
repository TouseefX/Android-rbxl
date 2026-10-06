# Studio RbxTransport lifecycle crosswalk: 0.741 static path vs Android implementation

**Date:** 2026-10-04
**Target:** supplied Windows Studio 0.741 executable, interpreted alongside the existing Team Create and selector reports.

**Related reports:** [Team Create join/leave lifecycle](roblox-0741-team-create-lifecycle-map.md) · [0.741 transport selector and wire findings](roblox-0741-studio-transport-selector.md) · [bounded 0.741 disassembly/CFG artifact index](analysis/roblox-0741-rbxtransport/README.md)

## Evidence boundary

- **[S741]** means address-level static evidence from the supplied 0.741 Windows executable, as recorded in the bounded disassembly findings in the related reports. It is not runtime observation.
- **[S735]** means the symbolized Mac source decompile in the tracked `roblox-network-focused-25mb.zip` (`FOCUSED-SUBSET-MANIFEST.txt` identifies it as **0.735 Studio**). Per the user's chronology correction, the 0.735 target build is from **2022**; `2026-08` in generated decompile headers is an analysis/artifact date, not the target build date. Treat 0.735 as a roughly four-year-old architectural clue; do not attribute its addresses or unconfirmed wire/lifecycle behavior to 0.741.
- **[S740]** means the recovered symbol/RTTI/function-start inventory and selected disassemblies in `roblox-0740-idb-recovery.zip` for Windows Studio **0.740**. The user identifies the 0.740–0.741 target builds as **2026**. Use 0.740 only as a cross-version candidate fingerprint set, not as a byte-level 0.741 match.
- **[APP]** means the current Rust implementation in `src/team_create.rs` / `src/app.rs`.
- **[D-app]** means Android app runtime measurements only.
- **[LOG]** means user-provided Studio runtime text; the excerpt has no binary hash/session metadata that independently binds it to this PE.

A user-provided Studio log reports an `ngtcp2_conn_handle_expiry: ERR_HANDSHAKE_TIMEOUT`, a peer closed during handshake “with no response,” and a successful RakNet fallback. This is runtime evidence that an RbxTransport/QUIC attempt can time out before a response and that the fallback path can then connect; it is **not** a successful RbxTransport handshake or proof that `TeamCreateManager::onConnectionAccepted` ran. Separately, the latest user-supplied Android run (2026-10-06) received a fresh gamejoin config, selected RbxTransport, then locally sent four datagrams on each of two route variants to public UDMUX `128.116.54.33:58616`; both got zero inbound datagrams and reached the native 10-second handshake floor. Earlier seven-datagram runs remain historical measurements in the selector report. These are different observations; neither reaches TLS/RPK completion, the app’s post-handshake channel, or early-auth stage.

## 1. 0.741 native connection lifecycle (static)

The important lifecycle boundary is **not** “QUIC handshake complete = Team Create connected.” The 0.741 binary contains further BaseClient/RbxTransport state transitions after lower-level connection setup.

```text
Team Create gamejoin config
  -> transport selector and route/config construction
       selector: 0x145b74ee0..0x145b76264
       RbxTransport config handoff: 0x145b6a4b0..0x145b6b258
       (flag- and input-dependent; RakNet fallback exists)
  ··· internal QUIC/RbxTransport connection implementation ···
  -> RbxTransportClient config/start: 0x14603b920
       stores config; calls BaseClient start via vtable +0x08
  -> connection establishment: 0x14603bb70
       creates/replaces connection object at client +0x58
  -> WaitForConnection: 0x14603eb70
       handles connection-open / ReceiveChannelOpened ACK / close / timeout events
  -> successful result: request BaseClient send channel
       client vtable +0x68 -> 0x14603da90
       application=1, channelId=0, reliability=2, priority=0
  -> set RbxTransportClient +0x20 connected
       success path at 0x14603bfcd
  ··· TeamCreateManager acceptance/callback edge not recovered ···
```

**What this proves:** [S741] the native client has a wait/event stage and does not set its low-level connected flag until after its establishment path has successfully returned from the wait and called the BaseClient send-channel path. The wait routine contains branches for connection-open, a receive-channel ACK, close, and timeout; the exact caller-to-manager path and runtime event ordering are not established here. The +0x20 flag is a transport-client state, not proof that `TeamCreateManager::onConnectionAccepted` ran.

`sendEarlyAuthData` is separately mapped at `0x145b78cb0..0x145b78f68`: it requires the early-auth-present flag and an active connection, then sends the `0xA8` payload through the active connection's send slot 1. Its exact caller edge relative to the wait, channel-open, and manager-acceptance paths remains unresolved. Do not draw a proven callback edge from the method's existence alone.

### Native stop/disconnect side

[S741] the lower-level `RbxTransportClient` teardown is `0x14603e910`, reached through the client vtable `+0x10`. It takes a reason object, clears the worker/lifecycle state, invokes the active connection's separate vtable slot `+0x78`, clears pending items, and clears the client's +0x20 connected byte when it was set. The normal Studio leave-action caller and the remote-close-to-`TeamCreateManager` callback remain unresolved. Destructor cleanup is not proof of user-initiated leave.

## 2. RbxTransport is a layered channel system, not just UDP or QUIC

The following internal shape is supported by the **0.735 symbolized decompile only** and is included as a cross-version architectural clue. It must be rechecked against the corresponding 0.741 callers before treating each detail as exact for the target binary.

### Lower QUIC/UDP dispatch layer [S735]

The symbolized 0.735 files `rbx/QuicUdpChannel.c` and `rbx/QuicConnectionPacketChannel.c` show an event-driven pipeline:

```text
AsyncUdpSocket
  -> QuicUdpChannel::startRecv / multishot receive
  -> QuicUdpChannel::onRecv
       -> packet-prefix processor and registered/default dispatch handler
  -> QuicConnectionPacketChannel
       -> Tx queue and Rx queues
       -> packet matching/demultiplexing by ConnectionId
       -> asynchronous receive/send-readiness callbacks and timers
  -> QUIC connection implementation
```

Relevant 0.735 Mac-symbol addresses include `QuicUdpChannel::startRecv` at `0x1037a390c`, `QuicUdpChannel::onRecv` at `0x1037a3a7c`, `QuicConnectionPacketChannel::trySend` at `0x10377cde6`, `takePacketsForCid` at `0x10377d19a`, `matchPacket` at `0x10377d3a2`, and `processUdpPacket` at `0x10377d606`. These show the native abstraction has prefix handling, dispatch, queues, and CID association around the QUIC engine. Quinn can supply an equivalent QUIC engine, but the RbxTransport-specific routing and upper channel protocol still have to match.

### System channels and control framing [S735]

The same decompile includes `BaseChannelContext`, `SendChannel`, `ReliableSendChannel`, `ReliableReceiveChannel`, `UnreliableSendChannel`, `UnreliableReceiveChannel`, `WireChannelId`, `NetStream`, and open/close control serializers. It shows a protocol layer above QUIC:

- Send channels are keyed by application and internal channel id and carry a reliability mode. Reliable sending tracks pending streams/bytes and ACKs; reliable receiving reconstructs streams. This is not achieved merely by opening an arbitrary QUIC stream.
- `WireChannelId::createReliable(id)` returns the positive id; `createUnreliable(id)` encodes the id as negative. The unreliable-open control carries a runtime wire id as well as the application and internal channel id.
- `OpenReliableChannelControl` serializes a six-byte body: control type `1`, application byte, and big-endian channel id. `OpenUnreliableChannelControl` serializes a ten-byte body: type `2`, application, channel id, and wire id. Type `3` is a close-unreliable control in this decompile.
- **Important framing ambiguity:** in `rbx/ChannelHeader.c`, the seven-byte sequence `[0x06, 0x01, app, channelId-u32be]` is produced as a one-byte payload length (`0x06`) followed by the six-byte `OpenReliableChannelControl` (whose type is `0x01`). The matching deserializer validates that length and control type before returning the app/channel id. The 0.735 Mac-symbol addresses are `ChannelHeader::serialize` `0x1036ff9d4`, `ChannelHeader::deserialize` `0x1036ffad2`, `OpenReliableChannelControl::serialize` `0x1037d9934`, and its size check `0x1037d99b0`.

The 44,029-line readable Player session contains direct nested IDA results that strengthen—and refine—the cross-version clue. `sub_213AF60` constructs an `OpenReliableChannelControl` object with type byte `1`, the application byte, and the 32-bit channel id; `sub_2289660` writes those fields as one byte, one byte, then a big-endian dword. The caller prepends the one-byte serialized length, which is six. Independently, `sub_213CC90` checks the first byte against the stored `0x06` constant, checks the next byte is `1`, reads the app byte and a big-endian dword; the recorded final check reads the constant as `0x06`. Android Player `sub_63D1A1C` independently parses the same seven-byte shape. This supports the Player interpretation `[length=6][type=1][app][channelId:u32be]` much more directly than the 0.735 source alone.

The later assistant summary table in the session describes an OpenReliable control as `[app][channelId:u8][streamId:u32be]`, but the recorded writer/reader pseudocode does not support that field mapping for these seven bytes: `sub_2133F30` stores/uses the QUIC stream id separately, while the channel header's final dword is parsed as the channel id. Treat the direct recorded pseudocode as stronger than that inconsistent summary, and keep both findings on the Player path—not as proof of Studio 0.741's exact receive contract.

The Rust parser now uses neutral prefix-byte names while preserving accepted bytes and extraction. Its test records byte equivalence only, not target-version semantics. The app no longer probes application payload for a second type-1 header; after consuming the seven-byte prefix it counts remaining bytes as opaque. This avoids interpreting arbitrary payload as a duplicate channel-open record. Studio 0.741's post-header framing remains unresolved, so no new payload parsing or send behavior is inferred.

### Async I/O, thread ownership, and signature/CFG pass

The direct 0.741 evidence now establishes one concrete `RtcIoRna::Impl` worker-manager path: it is event-loop-driven and starts an OS worker per configured event loop, with this inspected constructor passing a count of **one**. This is not a process-wide thread census, and the separate `BaseClient::ClientConnectThread` task still has unresolved OS-thread affinity.

| Evidence | What it supports | Limit |
|---|---|---|
| `[S741]` 0.741 `RtcIoRna::Impl` and native client | `0x1435d40e0` initializes the I/O manager with count `1`; `0x143447b40` creates/starts that many event-loop workers, `0x143447a70` calls `_beginthreadex`, and worker entry `0x143446d90` runs the event loop. The client also has a task object at `+0x78`, a wait/event routine at `0x14603eb70`, and mutex-protected teardown at `0x14603e910`. | One worker is established for this manager construction, not the whole Studio process. The separate `BaseClient::ClientConnectThread` is queued through a generic scheduler; its OS-thread affinity is unresolved. |
| `[S740]` `roblox-0740-idb-recovery.zip` | Candidate Windows vtable fingerprints include `SysEventLoop` `0x146c96de0`, `SysPollerMswin` `0x146c97400`, `LibuvEventLoop` `0x146c977e0`, `QuicPacketReaderMultishot` `0x146c97b70`, `QuicConnectionPacketChannel` `0x146c97b40`, `ConcurrentPacketSender` `0x146c98210`, `BasePacketSender<SPSCQueue<TxItem>>` `0x146c98200`, and `NetStreamWireReliable/Unreliable` `0x146c97fd0` / `0x146c97fb0`; RTTI also names `AsyncUdpSocket`. | These are 0.740 symbols/type fingerprints, not byte-level 0.741 matches. Do not transfer addresses or declare function identity from names alone. |
| `[S735]` symbolized Mac decompile | `QuicUdpChannel::startRecv` (`0x1037a390c`) guards duplicate starts and arms `AsyncUdpSocket::recvMultishot`; `onRecv` (`0x1037a3a7c`) processes receive batches and routes packets through prefix/dispatch handlers. The error path retries receive when not closing. `QuicConnectionPacketChannel::receiveAsync` (`0x10377c9dc`) uses `Pollable::dispatch`; its `trySend` (`0x10377cde6`) enqueues a `TxItem` on an SPSC queue and invokes a send-side vtable `+0x28` callback. The channel also posts close work and uses timers. | Cross-version architectural evidence only. `SPSC` identifies a queue contract, not by itself two different OS threads or the exact 0.741 scheduler. |
| `[APP]` `src/app.rs`, `src/team_create.rs` | A session gets a `std::thread::spawn` worker; that worker drives a Tokio `new_current_thread()` runtime with Quinn's Tokio runtime. Its QUIC tasks/readiness work are cooperatively scheduled on that session thread; UI updates cross a standard mpsc channel and cancellation uses an atomic flag. | This is already event-driven, not synchronous socket polling. It does not reproduce native callback boundaries, queue ownership, or the native BaseClient event/ACK sequencing. |

**Rust/native comparison:** the app's single-thread Tokio runtime can numerically resemble the one worker requested by this native manager instance, but the ownership boundary differs: Studio's `RtcIo` manager owns the event-loop thread, while the named BaseClient connect task is dispatched separately through a generic scheduler. No evidence here says those native layers collapse into one thread or one queue, and no Rust thread-model code was changed.

#### 0.741 event-loop manager, flag, and backend CFG

The bounded 0.741 path is now:

```text
RtcIoRna::Impl constructor 0x1435d40e0
  -> read RbxTransportRtcIoUseMultipleEventLoopThreads storage 0x14d735648
  -> initialize IoLibContext at this+0x10 with count = 1
       0x143446fb0: explicit "libuv" / "sys" name dispatch;
                    otherwise consult byte 0x14d4a6cc8
  -> start manager 0x143447b40
       if count == 0: substitute _Thrd_hardware_concurrency()
       create N event-loop objects (0x143447830)
       start N workers (0x143447a70 -> _beginthreadex)
            entry 0x143446d90 -> event-loop loop 0x1434464d0
  -> initialize RtcIo registry members with a selected loop
  -> shutdown 0x143447550: signal each loop, then join worker records
```

Supporting details and qualifications:

- At `0x1435d4127`, the constructor writes `1` into the manager configuration before calling `0x143446fb0`; it calls `0x143447b40` at `0x1435d41f7`. That start helper uses the manager's count as a loop bound. Its `_Thrd_hardware_concurrency` fallback applies only if that count is zero, not to this `1`.
- `0x143447a70` calls the Windows CRT `_beginthreadex` with entry `0x143446d90`. The entry formats the worker name `"RBX IoEvLoop {}"`, then calls `0x1434464d0`; that worker loop invokes event-loop virtual slots `+0x08` and `+0x28` until its stop/drain conditions finish. Thus this manager creates a dedicated OS event-loop worker per configured loop; the inspected `RtcIoRna` instance requests one.
- `0x143447550` first calls `0x143446160` for each manager entry, then checks thread IDs and calls the MSVC `_Thrd_join` wrapper for non-current workers before releasing records. This is a stop-then-join lifecycle. The precise poller wake primitive is still not named from this CFG alone.
- The name `RbxTransportRtcIoUseMultipleEventLoopThreads` is not merely a registration-only string. Its stub at `0x1435db340` passes the name at `0x148aa7280`, data address `0x14d735648`, and type id `2` to generic registrar `0x143862120`. The `RtcIoRna` constructor loads that byte at `0x1435d411d` into `this+0x08` and branches on it at `0x1435d4373`: nonzero selects manager vector element `0` via `0x1434479f0`; zero selects a PRNG-derived vector index via `0x1434479b0` (`0x1438696e0() % vector_count`). The chosen event-loop pointer is passed to registry setup (`0x1435d3a60`); a later branch selects the `RtcIoRna` registry member at `+0x298` or `+0x210`. This is a runtime consumer affecting event-loop/registry selection, **not** a count setter in the inspected constructor. Since its vector has one entry here, both selection paths resolve to that sole worker.
- The adjacent `RbxTransportRtcIoEventLoopThreadCount` literal is at `0x148aa72b0`, but a bounded executable-section reference scan over `0x143000000–0x144000000` returned no direct xrefs. That scoped negative does not prove the literal is unused elsewhere. No registration/read path for this count flag has been established; the observed manager count in this constructor is the immediate constant `1`.
- `0x143446fb0` compares an explicit backend string against `"libuv"` and `"sys"`, dispatching to `0x143465e90` and `0x14344ba20`, respectively. When no explicit name is supplied, it checks `0x14d4a6cc8`; its image-initialized byte is zero and the zero branch reaches the `"sys"` helper. The runtime-selected value/backend is not established. The 0.741 PE has RTTI strings for `SysEventLoop`, `LibuvEventLoop`, and `RtcIoRna`; 0.740 has corresponding class/vtable names, but those older addresses are only class-family candidates, not 0.741 function addresses.
- The bounded cross-version name match is: 0.740 `RtcIoRna` vtable `0x146c94c20` ↔ 0.741 RTTI string at `0x14c924d48` plus constructor `0x1435d40e0`; 0.740 `SysEventLoop` vtable `0x146c96de0` ↔ 0.741 RTTI string at `0x14c916b78` plus the factory's `"sys"` branch; 0.740 `LibuvEventLoop` vtable `0x146c977e0` ↔ 0.741 RTTI string at `0x14c917c78` plus the factory's `"libuv"` branch. This is a class-name/factory-CFG match only—not a byte-signature or per-method match. The 0.740 addresses are never used as 0.741 addresses.

`BaseClient::ClientConnectThread` is a **separate scheduling layer**, not the `_beginthreadex` call above. `0x14603e7f0` wraps callback `0x14603b3d0`, passes the task name `"BaseClient::ClientConnectThread"` to generic scheduler helper `0x1427c7fc0`, and stores the returned task at client `+0x78`. That factory itself does not create a Windows thread; the scheduler's worker affinity/count has not been followed to an OS creation site. Do not add this named task as another dedicated `RtcIo` thread without that proof.

The raw disassembly and `afbj` CFG artifacts for these paths are in `analysis/roblox-0741-rbxtransport/event-loop/`, `analysis/roblox-0741-rbxtransport/event-loop/flag-branch/`, `analysis/roblox-0741-rbxtransport/event-loop/backend/`, and `analysis/roblox-0741-rbxtransport/client/`.

The best-supported 0.741 CFG skeleton is:

```text
connect/establish (0x14603bb70)
  -> connected-state guard
  -> create connection object; store at client +0x58
  -> WaitForConnection (0x14603eb70)
       -> event/wait paths: connection-open, ReceiveChannelOpened ACK,
          connection-closed, timeout
  -> on successful result: BaseClient open-send-channel call
       (vtable +0x68; app=1, channel=0, reliability=2, priority=0)
  -> set client +0x20 connected
```

That is a **bounded interprocedural CFG summary**, not a complete basic-block graph: branch destinations and thread ownership are not all available in the workspace. It does show that native connected state is downstream of event processing and channel setup; the Rust session currently reports the earlier Quinn handshake milestone.

**Signature-matching status:** the direct function/dataflow/CFG evidence above is from the supplied 0.741 PE. The 0.740 IDB provides candidate Windows class/vtable names (`RtcIoRna`, `SysEventLoop`, `LibuvEventLoop`) and 0.741 carries matching RTTI class names plus direct `"sys"`/`"libuv"` factory branches. This is useful class-family corroboration, not a byte-for-byte 0.740-to-0.741 function signature match; do not transfer 0.740 addresses. The 0.735 Mac decompile remains an architectural lead only, never a 0.741 address or function match. Rizin 0.9.1 and 7-Zip were restored from the user-fetched repo tools into `/tmp`, and the supplied PE was analyzed directly. The direct 0.741 worker-manager path is now recovered, but the whole-image reference scan was deliberately not repeated: bounded searches were used after the unscoped scan exceeded the six-minute limit. No protocol code should be changed solely from the 0.735/0.740 clues.

For implementation, the recovered count of one applies only to this `RtcIoRna::Impl` manager instance; it is not a reason to copy a process-wide thread count or to add another worker blindly. The Rust session's `std::thread` plus current-thread Tokio runtime may be a viable engine, but equivalence still depends on callback affinity, queue ownership, shutdown wake/drain order, and the separate scheduler task. Mirror the handoff and lifecycle semantics only after those boundaries are traced.

### BitStream vs RbxTransport NetStream

I checked the symbolized 0.735 sources rather than assuming that every packet serializer shares the RakNet writer:

| Evidence | Finding |
|---|---|
| `[S735]` `rbx/NetStream.c`, `rbx/NetStreamBuffer.c`, and `rbx/Open*ChannelControl.c` | RbxTransport's channel/control serializers call `RBX::RbxTransport::NetStream::{createReliable,write,writeUInt32}`. `writeUInt32` swaps to network byte order; `NetStream` is byte-oriented and supports buffer/cursor/prepend-header operations. |
| `[S735]` `_external/raknet/BitStream.c` | This is the separate `RakNet::BitStream` implementation, with bit-positioned `ReadBits`/`WriteBits`, aligned-byte operations, and RakNet read/write pointers. I found no `BitStream` references in the extracted `rbx/` RbxTransport implementation files. |
| `[S741]` 0.741 Windows report | The recovered channel-control dword path calls NetStream's host-to-network helper at `0x147393b20`; the early-auth path is described as a `NetworkStream` writer. That is consistent with distinct RbxTransport/BaseClient stream writers, not evidence that this flow uses RakNet `BitStream`. |
| `[APP]` `src/team_create.rs` | The RbxTransport RUPP/control/auth payload shapes are assembled directly into `Vec<u8>`; fixed dwords use `to_be_bytes()`. There is no `RakNet::BitStream` object in the Rust RbxTransport path. The `write_system_address()` helper mentioning `BitStream::Write<SystemAddress>` is used by the legacy RakNet OpenRequest2 serializer, not RbxTransport. |

For the recovered byte-aligned RUPP and channel-control fields, a `Vec<u8>` serializer with explicit big-endian writes can match the bytes that `NetStream` would produce; it does **not** reproduce the full RbxTransport `NetStreamWireReliable`/`NetStreamWireUnreliable` machinery. The confirmed lifecycle gap remains that the app does not route those prepared payloads through a native-equivalent channel writer, and does not open the BaseClient channel or send early auth. A wholesale RakNet BitStream port would target the wrong serializer unless a 0.741 call trace shows an actual BitStream call on this path.

## 3. Current app lifecycle [APP] beside native

```text
POST /v1/team-create -> fresh join config
  -> Rust selector treats the accepted RbxTransport flags as enabled
  -> builds two route hypotheses (pure qdmux and RUPP-prefixed)
  -> Quinn endpoint + custom RUPP UDP wrapper
  -> await Quinn/ngtcp2 QUIC/TLS handshake
  -> on handshake success, emit RbxTransportSessionEvent::QuicHandshakeComplete
       app.rs sets team_create_session_quic_handshake_complete = true
       UI reports “Stop QUIC Receive Session,” not “Stop Connected Session”
  -> read-only stream/datagram receive loop
       early-auth and channel-control bytes remain unsent
```

Current source locations: route/session construction and QUIC connect are in `src/team_create.rs` around `run_rbx_transport_connection_async_with_session` / `attempt_rbx_transport_connection_async`; successful QUIC/TLS handshake emits `RbxTransportSessionEvent::QuicHandshakeComplete`, and `src/app.rs::pump_team_create_session_events` sets only `team_create_session_quic_handshake_complete`. The event and UI deliberately do not claim BaseClient-connected or Team Create-accepted state.

| Stage | Studio 0.741 static path | Android app | Comparison |
|---|---|---|---|
| Selector | Conditional FFlag gates, validity checks, and fallback paths are present. | Selects RbxTransport from config shape under the accepted flag assumption. | Not identical unless the active Studio flags and selected route are confirmed. |
| Route choice | Builds a connection config with optional RUPP/qdmux values; branch depends on config and flags. | Tries two route hypotheses sequentially. | The app is probing alternatives; no evidence says native Studio retries those exact alternatives in that order. |
| Connected milestone | Successful native wait, followed by BaseClient channel-open call, then client +0x20 is set. | QUIC/TLS handshake completion emits `QuicHandshakeComplete`; the UI records only that milestone and labels the active loop as a QUIC session. | The implementation no longer conflates a handshake with native connected state; native channel-open/acceptance is still not implemented. |
| BaseClient channel | Opens app 1 / channel 0 / reliability 2 / priority 0 via native connection registration. | Does not send the channel-control or early-auth bytes; waits for more evidence. | The app cannot reach the native BaseClient/session-accepted stage yet. |
| Team Create acceptance | Manager milestone strings exist, but the direct caller edge is not recovered. | No matching manager acceptance event is implemented. | Neither a successful native manager callback nor app-level collaboration acceptance has been observed. |
| Leave | Reasoned native connection teardown exists; normal leave action caller unknown. | Local worker cancellation closes the app's Quinn endpoint. | Local stop is not equivalent to Studio's reasoned teardown/manager lifecycle. |

## 4. What the mismatch does—and does not—explain

1. The Rust event/flag now means **QUIC/TLS handshake complete**, not **native BaseClient connected** and not **Team Create accepted**. The status/UI-label conflation is fixed; the underlying native connected milestone is still not reached by this receive-only implementation.
2. Even if Quinn completes a handshake, the current worker does not open the native send channel or send the `0xA8` BaseClient early-auth data. That is sufficient to explain why the current implementation cannot complete a usable Team Create session after transport establishment.
3. It does **not** explain the present run's zero-inbound-datagram handshake timeout: the worker never emits `Connected`, and no early-auth/channel step is reached. The current pre-response issue remains in the initial route/packet path or the network path; no specific security rejection is established.
4. A standards QUIC library is a reasonable engine, but `rust-raknet` is not a substitute for this RbxTransport channel lifecycle. RbxTransport adds a separate reliability/channel-control/NetStream layer above QUIC.

## 5. Bounded next verification

The highest-value next static check is to resolve the 0.741 receive-side channel-header path before writing or changing a channel serializer:

1. Trace the 0.741 `ChannelHeader`/receive dispatcher from the connection's incoming QUIC stream into its application/channel lookup. Confirm whether `[06 01 app channelId]` is the full length-prefixed `OpenReliableChannelControl` header or a distinct prefix followed by another control record.
2. Tie the 0.741 open-channel request to the registration handler, received ACK/state transition, `WaitForConnection` success result, and the +0x20 write. Keep the app's QUIC-handshake milestone separate from that state.
3. Recover the 0.741 caller/order for `sendEarlyAuthData` and the edge into `TeamCreateManager::onConnectionAccepted`; mark any unrecovered edge explicitly.
4. Compare the app's one-use route inputs and first outbound flight with a native Studio trace. The present app counters show successful local socket sends, not network delivery.

## 6. Focused follow-up search: RBXSIG, SCK, heartbeat, and other gaps

A bounded text search covered the saved 0.741 disassembly/CFG subset, the Team Create and RbxTransport maps, `src/team_create.rs`, `src/ngtcp2_rustls.rs`, and `vendor/ngnet-quic/src`, using RBXSIG/signature, SCK, heartbeat, ping/pong, keepalive, idle-timeout, and session/server-key terms.

- **RBXSIG:** no join-payload verifier appears in the saved 0.741 subset. The Rust source contains the separate Ed25519 TLS Raw Public Key handshake verifier; `roblox-0741-team-create-lifecycle-map.md` records the historical join-signature evidence and why the exact Studio 0.741 verifier/key/format remains unresolved. This is a scoped non-hit, not proof Studio lacks a check.
- **SCK:** no literal `SCK` hit appeared in those local artifacts or the targeted public code searches. The acronym's expansion is not established, so do not silently equate it with the RPK key, RakNet session key, or another key field.
- **Heartbeat/liveness:** no RbxTransport app-level heartbeat or explicit QUIC keepalive/PING call was found in the Rust connection path. The app sets a 10-second handshake deadline; ngnet-quic's transport-parameter default idle timeout is 30 seconds. That QUIC idle timeout is not evidence of a Studio heartbeat interval or frame rule. The app's `AppStarted` client-status heartbeat and Luau `RunService.Heartbeat` hits are unrelated; the legacy RakNet unconnected-ping helper is also a different path.
- **Public search:** a recent Developer Forum post reports `ConnectionHandler.handle` microprofiler entries grouped under RbxTransport, but its suggestion that these might be RakNet calls is explicitly speculation; it supplies no SCK, signature, heartbeat, or wire-format rule ([thread](https://devforum.roblox.com/t/studio-microprofiler-flooded-with-threads-of-rbxtransport/4834270)). Targeted web/GitHub searches did not surface a protocol specification for these items.
- **Other material gaps remain:** the BaseClient reliability-2 to wire-channel assignment; the 0.741 receive-side channel header and ACK/state transition; early-auth send ordering; and the direct edge into TeamCreateManager acceptance and normal leave are still unresolved, as detailed above. So the known work is not limited to three names.

This was a search of the existing bounded artifacts and source, not a fresh whole-image disassembly. In the current work area the supplied PE remains in multipart 7z archives and `7z`/`rizin` are not available on `PATH`; the previously timed-out whole-image reference search was not repeated. At the time of that search, no app source had yet been changed and no build or tests had been run. The seven-byte framing point remains an unresolved cross-version discrepancy pending a 0.741 receive-path xref; subsequent conservative implementation and validation are recorded in Section 8.

## 7. External lead review: `kingdudely/RbxTransport`

The linked repository contains one file, `session.json`, an exported reverse-engineering session. At parent commit `8b57e7922be3d948e0f3ab548b9dd588de08c26b` it was 5,073,038 bytes / 552 messages; at updated `main` commit `a0355614b630b5b942d5b4eb18cb75630663da2b` it is 12,380,577 bytes / 1,162 messages. A parsed comparison confirms the older 552-message array is an exact prefix of the newer one, with 610 messages appended. The size increase therefore reflects an appended session, not by itself a protocol finding. The export records IDA Pro 9.4 SP1 tool calls and results—including function metadata, decompilation, and xrefs—so it is useful evidence, not merely an AI summary. It does not include the underlying IDA database or a maintained implementation ([repository](https://github.com/kingdudely/RbxTransport), [session file](https://github.com/kingdudely/RbxTransport/blob/main/session.json)). Its title is “Roblox WebTransport /wt path reverse engineering”; it starts from a Windows Hyperion-decrypted Player dump, later opens Android `libroblox.so` for comparison, and investigates the Player's `/wt` route and regular `/v1/join-game` flow. That makes it a useful adjacent RbxTransport source, but not the Studio 0.741 CloudEdit `/v1/team-create` call chain or proof of the same TLS/authentication route.

The export is more than a narrative: it contains recorded IDA inputs and results, including decompilation/disassembly and xrefs. It actually follows **two Player-side IDA databases**—a Windows x64 executable (`sub_214…` family) and a later Android `libroblox.so` (`sub_63…`/`sub_64…` family)—and includes a Windows↔Android comparison. It does not provide stable executable hashes/build IDs or the IDBs, so those results remain Player-path evidence, not proof about the supplied Studio 0.741 Team Create binary. The relevant transcript sections are the Windows connection report at lines 21935–22105, the wire-format decode at 26768–26917 and 28650–28770, the cross-platform comparison at 35541–35642, the server capability-extraction result at 36787–36853, and the final Android bypass/certificate pass at 40467–44026.

### Additional Player-path findings in the session

| Topic | Recorded IDA evidence / session result | Applicability boundary |
|---|---|---|
| Join route | Windows `sub_4C36170` builds `/v1/join-game` by default; the `/v2/join-game` branch is gated by `EnableReactiveGameJoin` plus allowlist/rollout. | This is normal Player game join, **not** Studio CloudEdit `/v1/team-create`. |
| WT request | Windows `sub_2143030`/`sub_212F500` build an HTTP/3 extended `CONNECT` to `/wt?X-Rbx-Capabilities=<16 lowercase hex>` and send the same value in the request header. The value comes from the 64-bit `serverCapabilities` join config. The response handler checks the returned capabilities header. | This describes the Player WebTransport `/wt` path, not the direct Studio 0.741 Team Create QUIC path. |
| Android parity | The session maps Android `sub_63CEC10`, `sub_6A5AD6E`, `sub_641D1D8`, `sub_6463182`, and `sub_63D1A1C` to Windows response, varint, datagram, varint-reader, and stream-header counterparts; its sampled protocol comparison reports no divergence. | Useful cross-platform corroboration for those Player routines only; no target build hash is attached. |
| RUPP bypass | Android `sub_63CFB42` has a branch on `RbxTransportPureQuicSupport`: for a non-empty packet whose first byte has bit `0x40` set, it marks per-connection pure-QUIC state and returns without stripping a RUPP prefix. The session also records a contemporaneous `PCDesktopClient` settings snapshot with that flag `True`, alongside `RbxTransportUseRtcioRna` and `RbxTransportListenerWtEnabled`. Its synthesis says the client can omit the prefix when no `RuppConfig` is initialized. | This is strong evidence that the inspected Android code has a pure-QUIC receive mode, but the settings snapshot is mutable/desktop-scoped and is not the Studio 0.741 PE's runtime flag state. It does not by itself prove the target Team Create server accepts our route or that an end-to-end session succeeds. |
| Capabilities on accept | Android server-side `sub_63B580C`, called by `sub_63B5144`, checks the `X-Rbx-Capabilities` header and falls back to the same query parameter in `:path`; the handler ANDs the client mask with server capabilities. The client code sends both copies. | A query-only request appears accepted by this inspected extractor; the session did not empirically validate an Internet server or establish that the TLS extension is unnecessary under every fail-closed configuration. |
| Certificate material | Android `sub_646EEAB` parses a `certHashes` array in `NetStackConfig`; `sub_642186B` parses an X.509 PEM and computes a 32-byte SHA-256 digest. The session's final synthesis connects these as the production pinning path. | The export does not close every caller/consumer edge from the parsed list through the verifier, and the result is not a Studio 0.741 verifier trace. Treat the end-to-end pinning claim as a well-supported lead, not a cross-target guarantee. |

The Android `RUPP` prefix processor is in the QUIC packet I/O path, not a record inside a WebTransport stream. The session first considered it a browser blocker, then found the pure-QUIC branch above; those claims are not contradictory—one is the normal prefixed mode, the other is a flag-/connection-state-dependent alternate. The appended session later records a Python QUIC handshake over a RUPP-prefixed Player route, but it does not validate the separate pure-QUIC bypass or the browser `h3` `/wt` path. The latter has its own failed HTTP/3 CONNECT attempt in the added transcript. Do not transpose the Player's `webtransport-h3` `/wt` endpoint, join reply, `certHashes`, or dynamic flag snapshot into Studio Team Create without a target-specific call chain.

A recursive search of the session export found no `rbxsig`, `rbxsign`, or `SCK` mentions. Its lone `heartbeat` hit is the `RCCHeartbeatFPS` metadata name, not a transport timer or frame rule. The two `keepalive` mentions are a speculative thought that a zero-length payload after RUPP stripping might be a control/keepalive (immediately questioned in the same discussion) and a proposed app logging hook; neither establishes a heartbeat packet, interval, or timeout rule.

### Further nested IDA output reviewed: message names, fragments, and backpressure

The additional transcript pass covered the remaining relevant output blocks, not just the final assistant report. This is still Player `/wt` evidence:

- Recorded IDA strings name `RbxTransport/RtcIo/Remote/{AppFin,OpenUnreliableChannel,ConnClose,AppControl,FlowControl,Loss,StreamClose,Ack,Handshake,StreamAccepted}` (for example the `Remote/Ack` string at `0x6ef5fc8`) and the related accept/connection records. These are **message-family names only**; the transcript does not include the `Remote/Ack` serializer/deserializer or a byte layout.
- The `sub_21FEB80` decompile excerpt (message 327) directly shows an expiry branch logging `Discarding expired fragment datagram channel {} datagram id {}` and an illegal-fragment path; the same routine's recorded string output includes `Could not reassemble unreliable NetStream`. The excerpt is partial (the earlier query returns a 656-line function but only prints selected slices), so it does not establish every fragment field or ACK response.
- Recorded strings/types include queue backpressure (`recv queue is full — pausing stream`, `resumeStreamRecv`), `RbxTransportSystemLayerFlowWindowSizeBytes`, and `HandlerRestoreFlowCredit`. This supports flow-control/backpressure machinery, not an application heartbeat interval.
- `RbxTransportDummyClient*` ping/time-sync names belong to the diagnostic DummyClient and are not evidence of Team Create session heartbeat rules.

This extra pass reinforces the safe implementation boundary: parse/count what has a known layout, keep the other message families opaque, and do not synthesize an ACK from a class name or an overflow log.

### ACK evidence: three different layers, one unresolved frame

The nested IDA outputs support ACK-related behavior in the Player RbxTransport path, but **do not decode an application ACK's wire layout**:

- **RbxTransport datagram/send-ID bookkeeping:** the IDA string results contain `RbxTransport/RtcIo/Remote/Ack` (`0x6ef5fc8`), `ackedSendIdQueue overflow: dropping ACK for sendId` (`0x6ef6e30`), and tracker messages `Received ACK for unknown fragment id` / `Received ACK for unknown SendId` (`0x6f00ab0` / `0x6f00b00`). These establish an ACK/send-ID concept and queue/tracker paths; the export does not show the `Remote/Ack` serializer/deserializer or enough payload bytes to implement it.
- **QUIC transport acknowledgments:** `WtSession::setOnDatagramAcked`, `setOnAckedStreamData`, the `ackDatagramCb`/`ackedStreamDataOffsetCb` logs (`0x6eff300` / `0x6eff140`), and `sub_21390C0`'s `QUIC stream acked data` callback are lower-level QUIC delivery/loss notifications. They are not RakNet ACK/NACK packets and are handled by QUIC/ngtcp2 in our recreation.
- **Channel-open acknowledgment/state:** the Player log `Received data through locally opened stream (channel ack)` describes an incoming stream tied to a locally opened stream; Studio 0.741 separately has the `ACK ReceiveChannelOpened` wait/event branch. Neither string alone gives a RakNet ACK frame or proves both builds use the same channel-control bytes.
- **Legacy RakNet reliability:** the export also contains distinct `ReliabilityLayerLog` messages about serializing RakNet ACKs and RakNet ACK/NACK statistics. These are the legacy RakNet subsystem, not evidence that the RbxTransport `/wt` path carries RakNet's reliability ACK format.

So the session adds useful evidence that RbxTransport has its own send-ID ACK bookkeeping in addition to QUIC's built-in ACKs, but it is **not enough to add a RakNet ACK frame** (or a guessed RbxTransport `Remote/Ack` frame) to the Rust Team Create client. The observed app run received no UDP datagrams and timed out before any post-handshake ACK path could run. Keep RakNet ACK/NACK, QUIC ACKs, RbxTransport send-ID ACKs, and channel-open acknowledgment states distinct.

The author's separate [RakNet decompilation project](https://github.com/kingdudely/Roblox-RakNet-Decompilation-Project) is explicitly about the legacy RakNet/SessionCrypto pre-auth path; its README says Request2 is not emitted. It may help with the fallback path, but it does not fill the RbxTransport SCK or heartbeat gaps.

### 2026-10-07 update: appended Player transcript reviewed (`a0355614`)

The appended 610 messages add substantial nested IDA output and runtime experiments. The evidence below is split by source; it remains Android/Windows **Player** evidence, not Studio 0.741 Team Create evidence.

#### Direct nested IDA output

- **NetStream channel-control writers:** Android `sub_63D1924` calls `sub_63E8824` to serialize a six-byte type-1 body (`type`, application byte, channel ID as big-endian `u32`), then prepends a one-byte body length. The captured seven-byte form `06 01 ...` is consistent with that writer. `sub_63DF38E` logs `writeOpenUnreliableChannelControlNetStream` and calls `sub_63E8B06`, whose decompile writes a ten-byte body `[02][application][channelId u32 BE][wireChannelId u32 BE]`. This is direct support for those **Player channel-control** writers using `NetStream`; it does not prove every RbxTransport record, or the Studio 0.741 path, uses the same framing. These calls provide no basis to substitute `BitStream` or treat the two abstractions as interchangeable.
- **Channel-open result is not a decoded ACK:** `sub_63E4DAE` is a channel-registration/result handler. On success it logs `tx channel confirmed`, records the wire-channel ID, and may emit the corresponding type-1 or type-2 channel control. The output does not show a distinct `Remote/Ack` serializer or a RakNet ACK frame. Keep this state transition separate from QUIC transport ACKs and legacy RakNet reliability ACK/NACK.
- **Fourteen-app plan and raw reliability values:** `sub_369379C` loops over app indices 0–13 and passes the packed result of `sub_36932AB`. The direct decompile returns these raw groups (low word = channel index; high word = raw value): apps `0,1,2,4,7,8,12,13` → channel 0/value 3; app `3` → channel 1/value 3; app `5,11` → channel 1/value 2; app `6` → channel 1/value 3, or value 2 when `byte_750AC88` is set; app `9` → channel 0/value 4; app `10` → channel 0/value 2. Separately, `sub_63CE41A` labels its own raw enum values `0=Reliable`, `1=Unreliable`, other values invalid. The session assistant calls 2/3/4 RakNet-style reliability values, but this output does not connect that interpretation to `sub_63CE41A`; preserve the raw values and do not transplant the Player app plan into Studio.
- **Port/token mapping remains open:** the checked nested item `msg_1115c6d95001BV9RgHqbl1m5pr` decompiles `sub_646E34C` and searches `sub_646E6FE`; its shell call edits/prints test-matrix combinations, not a raw packet capture or the Python invocation. Other IDA output establishes that `NetStackPort` is parsed as a field, but does not by itself show which parsed field supplies the UDP destination or RUPP TLV2. The transcript's older findings and later assistant narration disagree on `ServerPort` versus `NetStackPort` (including a narrated `64183`/`57054` split); the checked block does not substantiate that mapping. Likewise, the 16-byte copy at `RbxTransport_Connect` config offset `+243` and subtype at `+259` have not been traced to `UdmuxToken` or `NetStackTokenValue`. Treat both questions as unresolved, not as repaired by the session's summaries.

#### Recorded runtime/tool output (not static IDA proof)

- The Python probe reports a QUIC handshake with `ALPN=RbxTransport`, a 103-byte early-auth write, 42 received stream bytes, no datagrams, and server-opened control streams. This is real handshake/channel-control progress in that probe; the output does **not** explicitly say early auth was accepted or show a completed game/world join.
- A separate native mocktail run with `FFlagUseRbxTransportClient3=true` reports RbxTransport selection, Replicator creation, peer-ID assignment, and a Join snapshot timer. This is stronger native Player-path evidence, but it is a forced-flag run and does not establish the default route or a Studio route. Its endpoint log does not identify whether the port came from `ServerPort` or `NetStackPort`.
- The browser experiment is distinct: it received a `/v2/join-game` response with an inline `joinScript`, then its HTTP/3 WebTransport `/wt` attempt ended with `Opening handshake failed`. That does not negate the separate native `RbxTransport`-ALPN probe, nor does it establish that Studio Team Create uses `/v2/join-game`. The session's DummyClient ping/time-sync output is diagnostic-client behavior, not a Team Create heartbeat rule.

#### Impact on this repository's findings

The new IDA output strengthens the **Player-only** NetStream channel-control crosswalk and explains why a one-byte length followed by a six-byte control is plausible in that code. It does not close the 0.741 receive-side or acceptance edges, decode `Remote/Ack`, settle BitStream use elsewhere, or establish the server-port/token-field mapping. The Studio 0.741 zero-response attempt therefore remains scoped to its recorded UDMUX `:58616` route; the external session does not justify silently switching that test to `NetStackPort`. No Rust wire behavior or Studio/gamejoin conclusion is changed on this evidence alone; a target-specific trace or reproducible capture is still required.

## 8. Focused implementation after the evidence review

The Rust client now makes the highest-confidence lifecycle and interpretation corrections without crossing the unresolved protocol boundary:

- `RbxTransportSessionEvent::Connected` is now `QuicHandshakeComplete`; the app state field, status text, stop-button label, and elapsed-time report identify the QUIC/TLS handshake milestone, not Team Create acceptance. The native BaseClient channel-open and early-auth steps remain intentionally unimplemented.
- The seven-byte stream prefix parser still validates the same bytes and extracts the same fields, but labels the first two bytes neutrally rather than “magic/version.” A unit test records that `[0x06] + OpenReliable(type 1, app, channel)` is byte-equivalent to the parsed sequence; the test explicitly does not claim 0.741 uses that framing interpretation.
- A bounded, receive-only application-stream observer now buffers enough initial bytes to report a bare type-1/OpenReliable-shaped candidate, records its app/channel alongside the outer header, and counts any remainder as opaque payload. It never routes data, emits control bytes, or treats the candidate as connection acceptance. The newly reviewed Player writer uses a one-byte NetStream length prefix for its own six-byte type-1 control; the 0.741 observer does not interpret that wrapper or claim such a record belongs in the application body. This remains a diagnostic candidate check, not a second-header protocol implementation.
- No RakNet ACK/NACK or guessed RbxTransport `Remote/Ack` serializer was added. The IDA session proves send-ID ACK bookkeeping exists, but does not provide its application-level wire layout; QUIC ACKs remain ngtcp2's responsibility.

The focused source/document diff passes `git diff --check`. A full staged-tree check also reports preserved trailing whitespace/CRLF in the verbatim session export and upstream vendored sources. Build, unit tests, and rustfmt could not be run in this environment because `cargo`, `rustc`, and `rustfmt` are not installed; the new byte-equivalence test is therefore added but not executed here.
