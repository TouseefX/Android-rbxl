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

A user-provided Studio log reports an `ngtcp2_conn_handle_expiry: ERR_HANDSHAKE_TIMEOUT`, a peer closed during handshake “with no response,” and a successful RakNet fallback. This is runtime evidence that an RbxTransport/QUIC attempt can time out before a response and that the fallback path can then connect; it is **not** a successful RbxTransport handshake or proof that `TeamCreateManager::onConnectionAccepted` ran. Separately, the last Android app run locally sent seven UDP datagrams on each of two QUIC route hypotheses, got zero inbound datagrams, and timed out at the 10-second handshake limit. These are different observations; neither reaches the app’s post-handshake channel or early-auth stage.

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

That last finding makes the app's current naming of `0x06` as “magic” and `0x01` as “version” questionable. The seven bytes may instead be a length-prefixed open-reliable/channel header. The current Rust receive code also treats those seven bytes as an outer header and separately checks whether the remaining body begins with another open-reliable record. That could be a valid 0.741 two-layer format, or it could double-count a single header; the 0.735 source is not enough to choose between them. **The 0.741 receive-side caller and serializer must settle this before the parser is changed.**

This is a concrete decompile discrepancy to resolve, not yet a confirmed 0.741 bug. The current code's seven-byte parser may parse the same bytes successfully while assigning the wrong semantics; whether its second body-prefix check is wrong is still version-dependent.

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
  -> await Quinn QUIC/TLS handshake
  -> on handshake success, emit RbxTransportSessionEvent::Connected
       app.rs sets team_create_session_connected = true
  -> read-only stream/datagram receive loop
       early-auth and channel-control bytes remain unsent
```

Current source locations: route/session construction and Quinn connect are in `src/team_create.rs` around `run_rbx_transport_connection_async_with_session` / `attempt_rbx_transport_connection_async`; the `Connected` event is emitted on successful Quinn handshake, and `src/app.rs::pump_team_create_session_events` sets `team_create_session_connected` immediately on that event.

| Stage | Studio 0.741 static path | Android app | Comparison |
|---|---|---|---|
| Selector | Conditional FFlag gates, validity checks, and fallback paths are present. | Selects RbxTransport from config shape under the accepted flag assumption. | Not identical unless the active Studio flags and selected route are confirmed. |
| Route choice | Builds a connection config with optional RUPP/qdmux values; branch depends on config and flags. | Tries two route hypotheses sequentially. | The app is probing alternatives; no evidence says native Studio retries those exact alternatives in that order. |
| Connected milestone | Successful native wait, followed by BaseClient channel-open call, then client +0x20 is set. | Quinn handshake completion immediately emits `Connected` and sets the UI boolean. | **Confirmed lifecycle mismatch:** app state advances earlier than the native client connected state. |
| BaseClient channel | Opens app 1 / channel 0 / reliability 2 / priority 0 via native connection registration. | Does not send the channel-control or early-auth bytes; waits for more evidence. | The app cannot reach the native BaseClient/session-accepted stage yet. |
| Team Create acceptance | Manager milestone strings exist, but the direct caller edge is not recovered. | No matching manager acceptance event is implemented. | Neither a successful native manager callback nor app-level collaboration acceptance has been observed. |
| Leave | Reasoned native connection teardown exists; normal leave action caller unknown. | Local worker cancellation closes the app's Quinn endpoint. | Local stop is not equivalent to Studio's reasoned teardown/manager lifecycle. |

## 4. What the mismatch does—and does not—explain

1. The Rust `Connected` flag/status currently means **QUIC handshake complete**, not **native BaseClient connected** and not **Team Create accepted**. That is a real app lifecycle/state-label mismatch.
2. Even if Quinn completes a handshake, the current worker does not open the native send channel or send the `0xA8` BaseClient early-auth data. That is sufficient to explain why the current implementation cannot complete a usable Team Create session after transport establishment.
3. It does **not** explain the present run's zero-inbound-datagram handshake timeout: the worker never emits `Connected`, and no early-auth/channel step is reached. The current pre-response issue remains in the initial route/packet path or the network path; no specific security rejection is established.
4. A standards QUIC library is a reasonable engine, but `rust-raknet` is not a substitute for this RbxTransport channel lifecycle. RbxTransport adds a separate reliability/channel-control/NetStream layer above QUIC.

## 5. Bounded next verification

The highest-value next static check is to resolve the 0.741 receive-side channel-header path before writing or changing a channel serializer:

1. Trace the 0.741 `ChannelHeader`/receive dispatcher from the connection's incoming QUIC stream into its application/channel lookup. Confirm whether `[06 01 app channelId]` is the full length-prefixed `OpenReliableChannelControl` header or a distinct prefix followed by another control record.
2. Tie the 0.741 open-channel request to the registration handler, received ACK/state transition, `WaitForConnection` success result, and the +0x20 write. Keep the app's QUIC-handshake milestone separate from that state.
3. Recover the 0.741 caller/order for `sendEarlyAuthData` and the edge into `TeamCreateManager::onConnectionAccepted`; mark any unrecovered edge explicitly.
4. Compare the app's one-use route inputs and first outbound flight with a native Studio trace. The present app counters show successful local socket sends, not network delivery.

No app source was changed for this map; no build or tests were run. The seven-byte framing point is intentionally left as an unresolved cross-version discrepancy pending a 0.741 receive-path xref.
