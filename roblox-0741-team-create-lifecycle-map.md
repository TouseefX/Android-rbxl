# Roblox Studio 0.741 Team Create join/leave lifecycle map

**Target binary:** `/tmp/winstudio_0741/RobloxStudioBeta.exe` (219,753,936 bytes; the supplied Studio 0.741 executable)

**Related analysis:** [`roblox-0741-studio-transport-selector.md`](roblox-0741-studio-transport-selector.md) · [RbxTransport lifecycle crosswalk](roblox-0741-rbxtransport-lifecycle-map.md) · [bounded 0.741 disassembly/CFG artifact index](analysis/roblox-0741-rbxtransport/README.md)

## Scope and evidence rules

This is a static map of the supplied Studio binary, set beside the Android app's source and its failed live transport measurements. “Ground truth” here means behavior visible in the 0.741 machine code; it does **not** mean that a static log literal ran, that Android's client is Studio, or that a native Team Create join or leave was dynamically observed.

- **[S-code]**: targeted disassembly of Studio 0.741.
- **[S-data]**: static string, RTTI, or component-factory metadata. This identifies code/data vocabulary or object composition, not execution or a call edge.
- **[D-app]**: runtime measurements from `Android-rbxl`, not from Studio.
- **[LOG]**: user-provided Studio runtime text, not independently bound to this PE by a binary hash/session capture.
- **[?]**: unresolved edge or field semantics.

There is still **no runtime evidence of a successful RbxTransport Team Create join, TeamCreateManager acceptance, or leave**. The user-provided Studio log reports an ngtcp2 handshake timeout, closure “with no response,” then a RakNet fallback that connected successfully. That qualifies the runtime picture but does not show that RbxTransport completed or that Studio's TeamCreateManager accepted the collaboration session. Separately, the latest Android app run received a fresh gamejoin config, then tried two QUIC/RbxTransport routes: seven local UDP sends per route, zero received datagrams, no socket errors, and the native 10-second handshake timeout. This was not the same Studio session and did not reach Studio's callbacks. The figures and their limits are recorded in the transport report.

## 1. Join path visible in the 0.741 binary

### Low-level path with instruction-level evidence

Function names below are behavioral labels; the executable does not provide source-level symbols for these methods.

```text
[CloudEdit / Team Create setup]
       ··· high-level call edge not recovered ···
[RbxTransportClient configure/start]  0x14603b920
       -> [connection establishment]  0x14603bb70
       -> create/replace connection object at client +0x58
       -> WaitForConnection(client, result*)  0x14603eb70
            connection-open / receive-channel ACK / close / timeout branches
       -> open client send channel (client vtable +0x68)
       -> set client +0x20 true
       ··· TeamCreateManager acceptance edge not recovered ···
```

| Step | Static evidence in Studio 0.741 | Grounded conclusion and limit |
|---|---|---|
| CloudEdit config gate | [S-data] `CloudEditConnectionModel::connectToTeamCreateSession` and adjacent strings say the DataModel must exist and be open, and report receipt of an RbxTransport token and port or that either is empty. String targets include `0x14903d2a0` (method name), `0x14903cfb0` / `0x14903d010` (empty/closed DataModel), and `0x14903d0b0` / `0x14903d130` (token/port received or empty). | Studio contains a CloudEdit/Team Create config-validation path. These are `.rdata` strings, not proof the checks ran and not a recovered call chain into the transport object. |
| Manager milestones | [S-data] The binary contains `TeamCreate-WaitForJoinConfig` (`0x1486c58b8`), `TeamCreate-HandleConnection` (`0x1486c5898`), `TeamCreateManager::onConnectionFailed` (`0x1486c57c0`), `TeamCreateManager::onConnectionAccepted` (`0x1486c5820`), and `Disconnected due to … LostConnection = …` (`0x1486c5950`). | These are real static milestones/log vocabulary. Their code xrefs and their ordering relative to the lower-level client have **not** been recovered; none is presented as a dynamic event. |
| Configure/start | [S-code] `0x14603b920..0x14603bb68` stores a 16-byte config/shared-owner pair at client `+0x68/+0x70`, rejects an empty configuration, and calls a BaseClient start method through vtable `+0x08`. | A config must be installed before connection establishment. The exact source type name is unavailable. |
| Create connection | [S-code] `0x14603bb70..0x14603c1e1` rejects an already-set connected byte at `client+0x20`, passes the mode byte at `+0x08`, config pointer at `+0x68`, and a companion object derived from `+0x60` to factory `0x1435d0a40`, then replaces the connection pointer at `+0x58`. | This is the native RbxTransport connection-creation stage. The `+0x60` companion object's exact type is unknown. |
| Wait for transport events | [S-code] Establishment calls `0x14603eb70` at `0x14603bd6b`, passing an output result buffer. The wait/event loop contains log sites for **connection opened** (`0x14603ef73`, string `0x148f72d70`), **ACK ReceiveChannelOpened** (`0x14603f0ce`, `0x148f72dc0`), **connection closed** (`0x14603f18a`, `0x148f72e30`), and **timed out** (`0x14603f363`, `0x148f72e78`). | The 0.741 binary has these branches. Static presence of a log site does not say which branch ran in Studio. |
| Open BaseClient send channel | [S-code] After a successful wait result, `0x14603bf23` calls the client object's virtual `+0x68` method (`0x14603da90`). The recovered arguments are application `1`, channel id `0`, reliability enum `2`, and extra/priority value `0`. | This is a channel-open request after the wait result; it is not proof the full CloudEdit/Team Create protocol was accepted. The wait loop's ACK log must not be re-described as a dynamically observed ACK. |
| Mark transport client connected | [S-code] The establishment path exchanges `1` into byte `client+0x20` at `0x14603bfcd` on its success path. | This is the low-level client's connected flag. It is not, by itself, proof that `TeamCreateManager::onConnectionAccepted` ran or that Studio reached a usable collaboration session. |

The log and callback targets above are static data. In particular, there is no recovered direct xref here from `CloudEditConnectionModel` to `RbxTransportClient`, or from the low-level connected bit to `TeamCreateManager::onConnectionAccepted`.

### Higher-level component structure (static metadata only)

The binary contains MSVC RTTI names for `Studio::ITeamCreateManager`, `Studio::TeamCreateManager`, `TeamCreateReconnectManager`, `TeamCreateEvictionManager`, `TeamCreateTeamTestDisconnectionHandler`, `TeamCreateWorkflowManager`, and `CloudEditConnectionModel`. Demangled component-factory type strings expose dependencies including:

- `TeamCreateManager`: `IPlaceManager`, `IJoinConfigFetcher`, `IAssetDataModelManager`, place-session state/context, `TeamCreateReconnectManager`, `ICancellableDialogManager`, optional `TeamCreateTeamTestDisconnectionHandler`, and `ICloudEditController`.
- `TeamCreateReconnectManager`: `IPlaceManager`, `SleepStateTracker`, `IMainWindow`, and optional `IPlaceReconnectState`.
- `TeamCreateTeamTestDisconnectionHandler`: local-save bridge, place manager/opener/session context, IDE document, team-test mode checker, sleep-state tracker, reconnect manager, main window, and—in one factory signature—an optional eviction manager.
- `CloudEditPlaceListener`: CloudEdit controller/model dependencies and an optional Team Create test-disconnection handler.

This supports a component map, not an execution graph. Related static logs include “Disconnected during place open,” autosave/reconnect failure, reconnect-dialog state, and safety-eviction dialog actions. They show that Studio has separate reconnect, place-open recovery, and eviction concerns; they do not prove any one handler ran in this investigation.

## 2. Leave and disconnect path visible in the 0.741 binary

The user/menu action that starts a normal native “leave Team Create” operation, and the remote-close-to-manager callback edge, remain unverified. The lower-level RbxTransportClient teardown itself is much better grounded:

1. **Async/lifecycle method.** The client vtable installed by the constructor is at `0x148f729c0`. Its `+0x08` entry is `0x14603e7b0`. That method clears the byte at `client+0x80`, constructs a task/callable, queues it through the object at `client+0x78`, and returns `AL=1`. A suitable behavioral label is *schedule lifecycle work*; its source name and exact user-facing meaning are unknown.
2. **Reasoned disconnect.** The same vtable's `+0x10` entry is `0x14603e910`. Microsoft x64 argument flow is `RCX=this`, `RDX=DisconnectReason*`. The method raises the lifecycle byte at `+0x80`, clears the worker byte at `+0x09`, locks the state lock at `+0x50`, resets flags at `+0x81/+0x82`, and, if a connection pointer exists at `+0x58`, invokes that connection's **separate** vtable slot `+0x78` with `EDX=1` and `R8=reason`. It clears the pending-item range beginning at `+0x88` and ending at `+0x90` before releasing the lock.
3. **Connected state and reason.** If `client+0x20` was set, the routine clears it, copies the two 32-bit reason fields and the reason string into the saved reason at `client+0x28`, and can emit the static log “RbxTransportClient disconnected from server for reason: {}” (string `0x148f73130`). The not-connected diagnostic uses a separate static string at `0x148f73190` (its code branch is inside `0x14603e910`); it does not show that a disconnect happened in this run.
4. **Destructor cleanup.** `0x14603b5f0` constructs a default reason `{0, 0, ""}` through helper `0x1427c28c0` and calls `0x14603e910`; deleting destructor `0x14603b7d0` reaches it. This proves a destructor cleanup route, not the ordinary UI leave route.
5. **Remote close/timeout reporting.** The wait loop has closed and timeout branches and fills a result buffer. The binary strings also name Team Create failure/reconnect/eviction handlers, but the call edges from those branches into the managers were not established.

Therefore the precise static claim is: **Studio 0.741 contains a reasoned RbxTransportClient teardown routine that closes its underlying connection, clears local queued state, and clears its internal connected byte.** This does not establish that Studio executed it, that a user left, that the peer received a close, or that the Team Create UI transitioned to a disconnected state.

## 3. Type-propagation overlay for the decompile

The following names are an annotation overlay derived from register flow, field accesses, constructor behavior, and helper calls. They are not recovered source declarations. The most useful type propagation is the reason object: constructor helper `0x1427c28c0` writes two `uint32_t` fields and moves/copies a 32-byte MSVC x64 `std::string`; `0x14603e910` reads those same fields and the string at `reason+8`.

```cpp
struct NativeDisconnectReason_0741 {
    uint32_t code;          // +0x00; meaning not recovered
    uint32_t detail;        // +0x04; meaning not recovered
    std::string message;    // +0x08; MSVC x64 std::string is 0x20 bytes
};                          // inferred size 0x28

struct WaitForConnectionResult_0741 {
    uint8_t success;        // +0x00; result byte; exact semantic scope unresolved
    uint8_t reserved[7];
    uint32_t code;          // +0x08
    uint32_t detail;        // +0x0c
    std::string message;    // +0x10
};                          // inferred size 0x30

struct OpaquePendingItem_0741 {
    std::byte raw[0x20];    // queue walk advances by 0x20; member meanings unknown
};
```

Observed client fields (partial layout only):

| Offset | Typed annotation | Evidence / confidence |
|---:|---|---|
| `+0x00` | vtable pointer | Constructor stores `0x148f729c0`; high confidence. |
| `+0x08` | mode/flags byte | Constructor argument is stored here and passed to the connection factory; exact enum unknown. |
| `+0x09` | worker-active/loop byte | Initialized to zero and tested by the worker loop at `0x14603b3d0`; semantic label inferred. |
| `+0x10..+0x1f` | opaque 16-byte aggregate | Zeroed by constructor; no reliable member names. |
| `+0x20` | `bool connected` | Checked before establishment and cleared/set on teardown/success. |
| `+0x28..+0x4f` | `NativeDisconnectReason_0741 lastDisconnect` | Confirmed by reason constructor and copy offsets. |
| `+0x50` | lock/mutex storage | Constructor initializes it; lock/unlock helpers operate on it. |
| `+0x58` | active connection interface pointer | Replaced during connection creation; vtable methods are called through it. |
| `+0x60` | opaque factory/runtime context | Its `+0x18` member is passed to the connection factory; exact type unresolved. |
| `+0x68/+0x70` | shared config pair | Config setter copies both words here; factory consumes `+0x68`. |
| `+0x78` | opaque async dispatch/task slot | Used to enqueue work; exact wrapper type unresolved. |
| `+0x80..+0x82` | lifecycle/state bytes | Written by the scheduling/disconnect paths; individual flag semantics remain provisional. |
| `+0x88/+0x90/+0x98` | vector begin/end/capacity | Constructor zeroes the triple; cleanup uses begin/end; entries advance by `0x20`. |

A compact type-propagated pseudocode view of `0x14603e910` is:

```cpp
void fn_14603e910(RbxTransportClient_0741* self,
                  const NativeDisconnectReason_0741* reason) {
    self->lifecycle[0] = 1;          // observed write to +0x80
    self->workerActive = false;      // observed write to +0x09; name inferred
    wake_or_reset_dispatch(self);    // +0x78 helper path; exact type unresolved

    lock(self->stateLock);           // +0x50
    self->lifecycle[1] = 0;          // +0x81
    self->lifecycle[2] = 0;          // +0x82
    if (self->connection) {
        self->connection->vfunc_0x78(/* flag */ 1, reason);
    }
    clear_opaque_0x20_byte_items(self->pending);
    unlock(self->stateLock);

    if (exchange(self->connected, false)) {
        self->lastDisconnect = *reason;
        log_static_disconnect_reason(reason);
    } else if (debug_log_enabled) {
        log_static_not_connected();
    }
}
```

This is pseudocode, not reconstructed source: helper names describe observed operations, while reason-code meaning, dispatch type, and the underlying vtable method's source name remain unknown. A decompiler can now label the key flows as `DisconnectReason*`, `WaitForConnectionResult*`, a connection-interface pointer, and `std::vector<OpaquePendingItem_0741>`, while leaving `+0x60`, `+0x78`, and reason-code meanings opaque. This is safer than naming every byte from a guessed C++ class. No binary, Ghidra project, Rizin database, or generated decompile was rewritten; these annotations are supplied here as a typed overlay.

### Reusable propagation steps

1. Apply the Microsoft x64 ABI at the function boundary (`RCX=this`, then `RDX`, `R8`, `R9` arguments).
2. Follow the same pointer through the caller and callee. At `0x14603e910`, `RDX` is preserved as `R14`; reads at `[R14]`, `[R14+4]`, and a string operation at `[R14+8]` establish the reason shape.
3. Use the constructor/helper as a second constraint: `0x1427c28c0` constructs the same two integers plus string at the client field `+0x28`.
4. Propagate the output-buffer layout from `WaitForConnection` into its caller before naming branches; keep the strings as log-site labels, not runtime facts.
5. Only name a field when at least two independent uses agree. Otherwise retain `opaque_*` and note the evidence.

## 4. Side-by-side static versus dynamic layout

| Lifecycle stage | Studio 0.741 static binary | Android app source/runtime | What can be asserted |
|---|---|---|---|
| Join control plane | Static Team Create/CloudEdit names and progress strings exist; this pass did not close Studio's HTTP call chain. | `src/roblox_api.rs` implements cookie-auth `POST gamejoin.roblox.com/v1/team-create`; the latest app run received a fresh join config. | The app's control-plane exchange succeeded. It is not proof of Studio's exact HTTP implementation or of a data-plane join. |
| Transport choice | The existing selector report maps the 0.741 RbxTransport gates and fallback logic. There is no native runtime flag/telemetry capture here. | App selected its RbxTransport path from the returned config and tried the two recovered route variants. | The app's selection is measured; Studio's actual selected branch is not dynamically confirmed by this trace. |
| Connection open | Static `WaitForConnection` contains open, receive-channel-ACK, closed, and timeout branches; send-channel open follows a successful result. | Each route made seven local UDP sends: 8,400 bytes on pure QUIC and 8,617 bytes with the RUPP prefix. Both received zero datagrams and hit the 10,000 ms handshake timeout, with no socket errors. | No inbound packet means the app did not reach TLS/RPK completion, early auth, channel traffic, or a native acceptance callback. Local send counts do not prove packets reached the peer. |
| Team Create acceptance | `onConnectionAccepted`, `onConnectionFailed`, and disconnected-reason strings exist, but their code xrefs and execution are unverified. | The app did not emit its `Connected` session event; it deliberately keeps early-auth/channel-control bytes gated. | No successful Studio or app Team Create session is demonstrated. A static callback string is not dynamic behavior. |
| Leave/disconnect | The native binary contains `0x14603e7b0` lifecycle scheduling, `0x14603e910` reasoned disconnect, destructor cleanup, and close/timeout result paths. The high-level leave-button edge is unresolved. | The UI stop action sets a cancellation atomic (`src/app.rs`); the worker closes its local Quinn endpoint in cancellation/session-end paths (`src/team_create.rs`). No connected run has exercised a server-observed leave. | The app's local cancellation/close code is not native Studio behavior and does not prove the peer received a close. No dynamic leave was captured. |

## 5. Remaining gaps

- Recover the direct call edges from `CloudEditConnectionModel` through the Team Create manager to the BaseClient/RbxTransportClient methods.
- Recover the caller of the normal user-facing Studio leave action and the connection-closed callback chain into `TeamCreateManager`, reconnect, and eviction handling.
- Obtain a native Studio 0.741 log/trace or packet capture of one successful join followed by a leave before calling any of these static branches dynamically observed.
- The place-version publishing endpoint/auth path is still unresolved in [`roblox-0741-place-publishing-investigation.md`](roblox-0741-place-publishing-investigation.md); this lifecycle report does not claim to solve it.

No app source changes, build, or tests were made for this report-only analysis.