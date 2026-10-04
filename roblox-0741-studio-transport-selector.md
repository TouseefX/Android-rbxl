# Roblox Studio 0.741 transport selector evidence

Target executable: `/tmp/winstudio_uploaded/extracted/RobloxStudioBeta.exe`

Lifecycle follow-ups: [`roblox-0741-team-create-lifecycle-map.md`](roblox-0741-team-create-lifecycle-map.md) maps the static join/leave stages and typed disconnect overlay; [`roblox-0741-rbxtransport-lifecycle-map.md`](roblox-0741-rbxtransport-lifecycle-map.md) compares the native RbxTransport/BaseClient stages with the app and flags a cross-version channel-header ambiguity.

**Version chronology correction:** the 0.735 target is from **2022**, while the 0.740/0.741 Windows targets are from **2026**. The `2026-08` stamp in the focused 0.735 decompile is its analysis/artifact date, not the target Studio build date. Accordingly, 0.735 is a four-year cross-version clue, not a near-adjacent protocol match; 0.740 signatures are closer but still require byte-level confirmation in 0.741.

- Size: `219,753,936` bytes
- Extracted from tracked split archive parts: `RobloxStudioBeta.exe.7z.001`, `.002`, `.003`
- Main selector function: `0x145b74ee0..0x145b76264`
- RbxTransport connect/config function: `0x145b6a4b0..0x145b6b258`
- RakNet connect/config function: `0x145b69e60..0x145b6a1d5`

Scratch disassembly artifacts are outside the repository:

- `/tmp/winstudio_uploaded/selector_0741.asm`
- `/tmp/winstudio_uploaded/rbxtransport_lambda_0741.asm`
- `/tmp/winstudio_uploaded/raknet_ctor_0741.asm`
- `/tmp/winstudio_uploaded/flag_funcs_0741.asm`
- `/tmp/winstudio_uploaded/scan_transport.txt`
- `/tmp/winstudio_uploaded/xrefs_selected.txt`
- `/tmp/winstudio_uploaded/rbxtransport_config_funcs.asm`
- `/tmp/winstudio_uploaded/xrefs_netstack.txt`
- `/tmp/winstudio_uploaded/netstack_parser_snips.asm`
- `/tmp/winstudio_uploaded/rbxtransport_full_remap.asm`
- `/tmp/winstudio_uploaded/rbxtransport_client_funcs.asm`
- `/tmp/winstudio_uploaded/baseclient_connect_14603b920.asm`
- `/tmp/winstudio_uploaded/baseclient_wait_14603eb70.asm`
- `/tmp/winstudio_uploaded/baseclient_events_14603daf0.asm`
- `/tmp/winstudio_uploaded/baseclient_send_14603e370.asm`
- `/tmp/winstudio_uploaded/rtcio_open_send_1435f2200.asm`
- `/tmp/winstudio_uploaded/connreg_open_send_1436a5800.asm`
- `/tmp/winstudio_uploaded/handler_open_channel_1436b5e00.asm`
- `/tmp/winstudio_uploaded/connreg_write_open_unrel_1436a8700.asm`
- `/tmp/winstudio_uploaded/control_deser_1436b0f80.asm`
- `/tmp/winstudio_uploaded/control_serialize_1436b18e0.asm`
- `/tmp/winstudio_uploaded/send_early_auth_0741.asm`

## Selector conclusion

NetStack fields alone do **not** force RbxTransport/QUIC.

The exact 0.741 selector is:

```c
useEnabled = useRbxTransport(false);

if (useEnabled || RakNetConnectionFailureFallbackToRbxTransport) {
    hasValidPort = (netStack.port != 0);
    hasValidAddress = (netStack.address != nullptr);
    hasValidPubKey = (!serverPublicKey.empty());
    canUseRbxTransport = hasValidPort && hasValidAddress && hasValidPubKey;
}

if (useRbxTransport(false)) {
    selectedTransport = canUseRbxTransport ? "RbxTransport" : "RakNet";
    if (canUseRbxTransport)
        connectViaRbxTransport();
    else
        connectViaRakNet();
} else {
    selectedTransport = "RakNet";
    connectViaRakNet();
}
```

Concrete selector addresses:

| Evidence | Address |
|---|---:|
| Zeroes selector booleans (`hasValidPort`, `hasValidAddress`, `hasValidPubKey`, `canUseRbxTransport`) | `0x145b7583c..0x145b75845` |
| First `useRbxTransport(false)` call used to decide whether to compute/log RbxTransport fields | `0x145b75847..0x145b75858` |
| `hasValidPort = netStack.port != 0` | `0x145b7585e..0x145b75868` |
| `hasValidAddress = netStack.address != nullptr` | `0x145b7586c..0x145b75873` |
| `hasValidPubKey = pubkey_begin != pubkey_end` | `0x145b75877..0x145b75888` |
| `canUseRbxTransport = port && address && pubkey` | `0x145b7588c..0x145b7589a` |
| Event field `useRbxTransportEnabled` | `0x145b7589e..0x145b758d1` and false path `0x145b75d76..0x145b75d9c` |
| Event field `rbxTransportHasValidPort` | `0x145b758d6..0x145b75904` |
| Event field `rbxTransportHasValidAddress` | `0x145b75909..0x145b75946` |
| Event field `rbxTransportHasValidPubKey` | `0x145b7594b..0x145b75988` |
| Event field `canUseRbxTransport` | `0x145b7598d..0x145b759ca` |
| Decisive second `useRbxTransport(false)` call | `0x145b759cf..0x145b759df` |
| If enabled: `selectedTransport = canUse ? "RbxTransport" : "RakNet"` | `0x145b759e5..0x145b75a25` |
| Enabled-path full telemetry string | `0x145b75a45` → `0x148f051e0` |
| If enabled but `canUseRbxTransport == false`: jump to RakNet constructor | `0x145b75b25..0x145b75d65` |
| If enabled and `canUseRbxTransport == true`: call RbxTransport constructor | `0x145b75b25..0x145b75b34` |
| If disabled: force `selectedTransport = "RakNet"` | `0x145b75d76..0x145b75dd2` |
| Disabled-path telemetry string | `0x145b75df2` → `0x148f052d0` |
| Disabled-path RakNet constructor call | `0x145b75d65..0x145b75d6c` |

## `useRbxTransport(false)` gate

Function: `0x144a012f0..0x144a0132a`.

Relevant storage and registration thunks:

| Flag/storage | Evidence |
|---|---|
| `UseRbxTransportClient` storage `0x14d8e49e0` | registration thunk `0x144a007a0..0x144a007b4`; checked at `0x144a01301` |
| `UseRbxTransportServer` storage `0x14d8e4a78` | registration thunk `0x144a00890..0x144a008a4`; only considered when the caller passes `cl != 0`; selector passes zero |
| `RefactorIngressFlow` storage `0x14d8e49b8` | registration thunk `0x144a00720..0x144a00734`; checked at `0x144a01313` |
| `SendsGoThroughConnection` storage `0x14cd99970` | accessor `0x1427bd4c0` and registration thunk `0x1427be240..0x1427be254`; called at `0x144a0130a` |

Effective logic for the selector's call (`cl = 0`):

```c
bool useRbxTransport(bool serverPath)
{
    if (!(UseRbxTransportServer && serverPath)) {
        if (!UseRbxTransportClient)
            return false;
    }

    if (!SendsGoThroughConnection())
        return false;

    if (!RefactorIngressFlow)
        return false;

    return true;
}
```

Important runtime implication: `UseRbxTransportClient` and `RefactorIngressFlow` storage addresses are in the `.data` zero-fill region in this uploaded binary, so the compiled default is false unless runtime/client settings enable them. Current public `PCStudioApp.json` from `MaximumADHD/Roblox-FFlag-Tracker` contains only:

- `DFFlagDebugDisableRbxTransportQuicAddressValidation = True`
- `DFFlagRbxTransportDisableIoEventLoopPerfScope = True`

It does not contain `FFlagUseRbxTransportClient`, `FFlagRefactorIngressFlow`, or the fallback-enable flags. That means public Studio settings do not prove RbxTransport-first Team Create. Private/runtime flags could still alter the decision.

Runtime correction from the current investigation: treat Studio's `FFlagUseRbxTransport` and `FFlagStudioClientServerMDI2` as enabled by default for the active Team Create target. With those runtime flags accepted, configs that have a nonzero NetStack/RbxTransport port, an address, and a 32-byte RbxTransport early public key should be mapped to `selectedTransport=RbxTransport` before any legacy RakNet probe is attempted.

## Fallback flags

| Flag/storage | Evidence | Selector behavior |
|---|---|---|
| `RakNetConnectionFailureFallbackToRbxTransport` storage `0x14d915cb0` | registration thunk `0x145b765e0..0x145b765f4`; selector reads at `0x145b75854` and `0x145b75ebc` | If initial selection is RakNet and this flag plus `canUseRbxTransport` are true, sets up fallback-to-RbxTransport (`0x145b75ef8..0x145b75fa9`). |
| `RbxTransportConnectionFailureFallbackToRakNet` storage `0x14d915c88` | registration thunk `0x145b76640..0x145b76654`; selector reads at `0x145b75b39` | If initial selection is RbxTransport and this flag is true, sets up fallback-to-RakNet (`0x145b75b46..0x145b75c56`). |
| `RbxTransportFallbackStudioMessage` storage object `0x14c4173b8` | registration thunk `0x145b76740..0x145b76754`; message use refs in fallback handling, e.g. `0x145b720dc..0x145b72109` | Used when an RbxTransport connection closes and Studio shows/logs a fallback message; it does not decide the initial transport branch. |

## Join-config parser fields relevant to RbxTransport

The general Studio test/join config parser is `0x141d44ef0..0x141d457f7`. Relevant output assignments recovered from `/tmp/winstudio_uploaded/rbxtransport_config_funcs.asm`:

| Config key | Native output |
|---|---:|
| `ServerPort` | `+0x00` |
| `ServerName` | string at `+0x10` |
| `RbxTransportPort` | `+0x04` |
| `RbxTransportToken` | string at `+0x30` |
| `NumTestServerPlayersOnStartup` | `+0x08` |
| `ExecuteTestService` | byte `+0x70` |
| `IsInStudioTestServiceMode` | byte `+0x71` |

The Team Create/join parser functions now known to consume the same native payload family are:

| Function range | Relevant field refs |
|---|---|
| `0x145b845e0..0x145b8c1fd` | `RandomSeed1`, `TokenValue`, `UdmuxEndpoints`, `NetStackTokenValue`, `NetStackPort`, `GameFqdn` |
| `0x14674c120..0x14674df77` | NetStack/join-config consumer |
| `0x1467525e0..0x14675486d` | NetStack/join-config consumer |
| `0x146f71f90..0x146f72ccb` | NetStack/join-config consumer |

The useful snippet in `0x145b8b3ac..0x145b8b67a` reads, in order, `ServerConnections`, `UdmuxEndpoints`, `DirectServerReturn`, `TokenValue`, optional `MachineAddress`/`ServerPort`, `NetStackTokenValue`, `NetStackPort`, and `GameFqdn`. Token and seed bytes remain secrets; diagnostics should only report shapes/lengths.

`RbxTransportEphemeralEarlyPublicKey` has static string xrefs at `0x1404788e1`, `0x1467586e1`, and `0x14675fc21`; `RbxTransportEphemeralEarlySecretKey` has a paired xref at `0x140478951`. The emitted KeyRing application uses id/send/revert `1` for `RbxTransportEphemeralEarlyPublicKey`, separate from the legacy RakNet early-key application id `5`.

## RbxTransport config handoff recovered so far

`0x145b6a4b0..0x145b6b258` is the RbxTransport path selected by the branch above. It copies the same base connection input as RakNet, then additionally consumes the NetStack/RbxTransport fields. Evidence strings:

- `0x148f04f60`: `[DFLog::NetworkClient] RbxTransport Client RuppConfig = RCC {}:{}, DSR {}, Token type {}`
- `0x148f04fc0`: `[DFLog::NetworkClient] RbxTransport Client will connect without RuppConfig. udmuxEndpoint exists: {}, rbxTransportToken exists: {}`
- `0x148f05050`: `[DFLog::NetworkClient] RbxTransport Client will connect to server {}|{}, udmux {}|{}`

Additional structure recovered from the full `0x145b6a4b0` body in `/tmp/winstudio_uploaded/rbxtransport_full_remap.asm`:

| Native source offset in the selector handoff struct | Meaning recovered so far |
|---:|---|
| `+0x08` string, `+0x28` word/dword | RCC/server address and port. These are parsed with `0x143449960`; failure goes to `RbxTransport Client rccAddr parse error: %s`. |
| `+0x78` optional string/endpoint, `+0x98` dword | Public/RbxTransport endpoint copied to the outgoing connect configuration at stack `+0x118/+0x138`. Follow-up PlayerConfigurer evidence shows `MachineAddress`/`ServerPort` remain the logical server endpoint while the advertised UDMUX/public endpoint remains the UDP socket target. The broader NetworkClient map later showed `ClientRuppConfiguration` and qdmux routing fields are filled from the RCC/server endpoint port, not `NetStackPort`; `NetStackPort` remains separate selector/config metadata. |
| `+0x122` byte | DirectServerReturn/RUPP flag copied into the RUPP config blob and logged as `DSR`. |
| `+0x123` byte | RUPP token type logged by `RuppConfig = RCC {}:{}, DSR {}, Token type {}`. |
| `+0x124..+0x133` bytes | 16-byte RbxTransport/RUPP token payload. Do not print. |
| `+0x134` byte | Token-present flag; RUPP config is omitted if the UDMUX optional is empty or this byte is false. |
| `+0x138` string optional | Game/FQDN-related string copied into the connect configuration when present. |
| `+0x1f0` onward | Existing connection/base-client subobject copied into the outgoing RbxTransport configuration before network-emulation setup. |

`0x145cc44c0` is no longer considered the QUIC handshake/auth routine. In the RbxTransport path it is reached after the connect configuration is assembled, receives the client `+0x1248` object plus the network-emulation output buffer, and populates packet-loss/latency/jitter fields (`sendPacketLossRatio`, `recvPacketLossRatio`, `sendLatencyMs`, `sendJitterMs`, `recvLatencyMs`, `recvJitterMs`).

This is enough to prove selection and to keep the app on the RbxTransport branch, but not enough to implement the RbxTransport/QUIC client. The remaining native target is the actual BaseClient/QUIC connect-and-channel path that consumes the assembled configuration.

## RbxTransport client/BaseClient path recovered so far

`/tmp/winstudio_uploaded/rbxtransport_client_funcs.asm` maps the public client wrapper functions that own connection state and channel opening:

| Function range | Role recovered so far |
|---|---|
| `0x14603b920..0x14603bb68` | `RbxTransportClient::connect`-style setter/start routine. It stores the connect configuration at client offsets `+0x68/+0x70`, rejects an empty configuration, calls a BaseClient virtual start method (`vtable +0x08`), and logs `Failed to start the BaseClient with configuration {}` when that start call fails. |
| `0x14603bb70..0x14603c1e1` | Connection establishment path. It rejects `client+0x20` already-connected, creates an RbxTransport connection with `0x1435d0a40` using `client+0x60`, `client+0x68`, and `client+0x08`, stores it at `client+0x58`, waits via `0x14603eb70`, opens the client channel through connection vtable `+0x68`, then sets `client+0x20 = true`. |
| `0x14603eb70..0x14603f4d5` | Wait/event loop for connection-open, receive-channel ACK, close, and timeout. Logs `connection opened`, `ACK ReceiveChannelOpened IN WaitForConnection`, `connection closed`, and `timed out`. |
| `0x14603daf0..0x14603e26b` | Receive/event dispatch for ACK/unknown receive channel state. |
| `0x14603e4e0..0x14603e7a5` and `0x14603e370..0x14603e4d5` | Message send/queue paths before and after the receive channel is open. |

Important implication for implementation: RbxTransport does not start with Roblox's RakNet `RbxOpenRequest1/2` packets. The native client creates a QUIC/RbxTransport connection first, waits for a connection-open event, then opens/ACKs channels before game data is sent.

### BaseClient open-send channel and control framing

The first BaseClient send channel now has exact 0.741 argument mapping. `BaseClient::connect` calls BaseClient vtable `+0x68` at `0x14603bf1d..0x14603bf23` after `0x14603eb70` reports the connection-open event. That vmethod is `0x14603da90..0x14603dae5`; it locks the BaseClient connection mutex and calls the active connection vtable `+0x70` with:

| Argument | Native evidence | Value | Meaning |
|---|---|---:|---|
| `dl` | `0x14603dac4` (`lea edx, [r9-1]` after `r9d=2`) | `1` | application id |
| `r8d` | `0x14603dac1` | `0` | send `ChannelId` |
| `r9d` | `0x14603dabb` | `2` | reliability enum value |
| stack byte/dword | `0x14603dab6` | `0` | priority/extra open option seen by the registration layer |

The RtcIo/RnaExp wrapper at `0x1435f24f0..0x1435f2901` validates the channel id (`r8d`, accepting `"ctrl"` or `< 0x21`) and packages `application/channelId/reliability/priority` for the connection registration. `ConnectionRegistration::openSendChannel` at `0x1436a5aa0..0x1436a5c34` confirms the same mapping: it uses `dl` to look up the per-application registration via `0x1436a4af0`, forwards `r8d` as the channel id, forwards `r9d` as the reliability enum, and forwards the stack value as the extra open parameter into `0x1436b5eb0`.

Exact channel-control payload serializers are also recovered:

| Control | Serializer | Deserializer | Payload after QUIC/RbxTransport channel header | Length |
|---|---:|---:|---|---:|
| `OpenReliableChannelControl` | `0x1436b1980` | `0x1436b1060` | `type=0x01`, `application u8`, `channelId u32be` | 6 bytes |
| `OpenUnreliableChannelControl` | `0x1436b1a40` | `0x1436b12a0` | `type=0x02`, `application u8`, `channelId u32be`, `wireChannelId u32be` | 10 bytes |

The dword writes go through NetStream helper `0x147393b20`, which calls the host-to-network conversion before appending the four stored bytes. `OpenUnreliableChannelControl` serializes the absolute value of the object field at `+0x18`; that field is a runtime wire-channel id allocated by the transport/channel handler and should not be guessed from the join config. The Rust code can decode/test the 6-byte OpenReliable and 10-byte OpenUnreliable bodies, but it does **not** emit the BaseClient channel-control or early-auth bytes. A separate 0.735 symbolized decompile maps the seven-byte `06 01 app channelId` sequence as a length-6 prefix plus OpenReliable type 1, not a magic/version header; whether 0.741 uses a distinct outer stream prefix as well remains unresolved. See the [RbxTransport lifecycle crosswalk](roblox-0741-rbxtransport-lifecycle-map.md) before treating the app's current header naming or two-layer parser as verified.

Exact 0.741 `sendEarlyAuthData` is mapped at `0x145b78cb0..0x145b78f68` (`/tmp/winstudio_uploaded/send_early_auth_0741.asm`). It checks the early-auth-present byte at client `+0x1210`, checks the active connection pointer (`+0xfe0` then subobject `+0x138`, or fallback `+0xfe8`), builds a `NetworkStream`, and writes the BaseClient early-auth application payload:

| Write site | Payload component |
|---|---|
| `0x145b78df8..0x145b78e0a` | byte `0xA8` |
| `0x145b78e0f..0x145b78e27` | one-byte auth version from client `+0x1208` |
| `0x145b78e2c..0x145b78e44` | one-byte pre-auth length from `+0x11d8` |
| `0x145b78e49..0x145b78e65` | pre-auth blob from string/storage at `+0x11c8`, length `+0x11d8` |
| `0x145b78e6a..0x145b78e82` | one-byte auth length from `+0x11f8` |
| `0x145b78e87..0x145b78ea3` | auth blob from string/storage at `+0x11e8`, length `+0x11f8` |
| `0x145b78ef1..0x145b78f3a` | send through the active connection vtable `+0x30` with send-slot argument `1` |

The blobs come from `ClientTicket` fields 2 and 3, and the auth version is the final semicolon field. `src/team_create.rs` now reports only this frame's lengths and version, keeps the ticket contents redacted, and uses those bytes only after a QUIC/RPK/RUPP connection has been established.

## Live validation after the RbxTransport pivot

Fresh 0.741 Team Create joins run through the committed transport selector resolve immediately to `selectedTransport=RbxTransport` with no UI delay. Live payloads have supplied:

- public UDMUX address/endpoint such as `128.116.54.33:61938`, `128.116.50.33:65161`, and `128.116.54.33:51996`,
- RCC/RUPP endpoints such as `10.32.1.158:61938`, `10.20.7.153:65161`, and `10.32.0.205:51996`,
- separate `NetStackPort` values such as `58659`, `54020`, and `52054`,
- a 16-byte `NetStackTokenValue` (redacted in the app output),
- an `EphemeralEarlyPubKey` override decoded as version `1`, length `32` bytes, and
- BaseClient early-auth examples such as auth version `17`, pre-auth `33` bytes, auth `66` bytes, payload `103` bytes.

Current reporting originally distinguished the advertised public/UDMUX endpoint from a guessed QUIC UDP target formed from the public address plus `NetStackPort`; later PlayerConfigurer review corrected that guess. Native passes the advertised public/UDMUX endpoint as the UDP socket destination, carries `MachineAddress`/`ServerPort` as the logical RCC/server endpoint, and the wider NetworkClient/RUPP map shows qdmux plus ClientRupp endpoint TLVs use that RCC/server endpoint port rather than `NetStackPort`.

Latest live validation adds an immediate-response case with no legacy RakNet wait:

- Earlier guessed QUIC UDP target: `128.116.50.33:58490` (`Address` plus `NetStackPort=58490`); this has since been corrected to use the advertised public/UDMUX endpoint as the socket destination.
- Advertised UDMUX endpoint/open peer: `128.116.50.33:62638`.
- RCC/RUPP endpoint: `10.20.0.12:62638`.
- RCC version: `0.741.0.7411056`.
- `TokenGenAlgorithm=1`; `PepperId=1790880922 (0x6abeac9a)`; `RandomSeed1` is only reported by decoded length (`64` bytes); `TokenValue`/`NetStackTokenValue` remain redacted but shape-checked.
- RbxTransport early public key override: version `1`, length `32` bytes.
- BaseClient early-auth metadata: tag `0xA8`, auth version `17`, pre-auth `33` bytes, auth `66` bytes, total payload `103` bytes.

The app handed the fresh gamejoin config directly to the selected RbxTransport path and produced the selector/channel/open-auth report immediately; the missing former ~5 second UI stall is useful branch proof that the legacy RakNet probe is no longer being attempted first. The implementation now advances past reporting into a QUIC connection attempt using the public UDMUX endpoint as the socket destination, an RFC 7250 raw-public-key verifier for the 32-byte RbxTransport key, and RUPP/qdmux routing metadata built from `NetStackTokenValue`, the RCC address, and the RCC/server endpoint port. Follow-up decompilation of both the Studio Team Create payload path and the PlayerConfigurer path showed native constructs the NetStack `TokenTlv::Token` with subtype `1` directly; `TokenGenAlgorithm` remains safe legacy-token metadata and is no longer allowed to change the RbxTransport prefix subtype. The QUIC attempt also now uses Studio's native `RbxTransportQuicHandshakeTimeoutMs=10000` floor instead of the shorter UDP probe timeout.

A live run after the native subtype/native-timeout patch still timed out during QUIC handshake, before early-auth/channel-control traffic. The next decompile pass found the likely envelope mismatch in `/tmp/rbx_net/rbx_ClientRuppGenerator.c`: native `ClientRuppGenerator::generateHeader` constructs `Rupp(protocol=1)`, adds the token TLV, then calls `addIpv4EndpointTlv` / `addIpv6EndpointTlv` (TLV types `2`/`3`). The legacy RakNet routed-open path is the one that uses reverse-endpoint TLVs `6`/`7`. The app therefore now switches only the RbxTransport/QUIC prefix to native endpoint TLVs `2`/`3`, while preserving the existing RakNet reverse-endpoint prefix shape. Inbound Team Create traffic is still the success gate: if QUIC handshakes and no stream/datagram arrives after the early-auth write, the next remaining suspects are the recovered NetStream framing and the custom TLS capability extension.

A later live run with the native endpoint TLVs still timed out at the native 10 second QUIC handshake budget. The next Windows/Mac evidence pass recovered Studio's qdmux SNI formatter and server-side parser: `parseQuicSni` consumes `{token}-{rccIpv4}-{rccPort}.{qdmuxVip}.qdmux.roblox.com`, and the Windows `DebugRbxTransportGenerateGameFqdn` branch formats exactly `"{}-{}-{}.{}.qdmux.roblox.com"` before clearing the RUPP config for a pure-QUIC route. The app now derives that redacted qdmux GameFqdn shape from the NetStack token, RCC IPv4, the RCC/server endpoint port, and qdmux VIP when the join config omits `GameFqdn`; a native-style `QdmuxVip` / `DebugRbxTransportQdmuxVip` field is preferred when present, otherwise the advertised public UDMUX IPv4 remains the fallback VIP. Generated qdmux names contain the Team Create token and must never be logged; the UI reports only that the redacted token/ip/port SNI shape was used.

The qdmux-SNI route by itself was not enough while the socket was still aimed at the guessed `NetStackPort`. The subsequent PE pass recovered the optional native packet-protector path: the RbxTransport UDP send path computes any RUPP prefix length (`0x143443590`), reserves an additional `0x12` bytes, and can call the native QUIC packet protector (`0x14343f520`) over the datagram slice after the RUPP prefix. That helper writes the `UniqueNumbeR` nonce marker, ChaCha20-Poly1305 protects the QUIC datagram with Studio's built-in fallback 32-byte protection key (`0x143427b30`), and leaves a transmitted 18-byte RUPP/QUIC CID trailer: two nonce bytes plus a 16-byte tag. The key/trailer/tag bytes are never logged.

The first trailer implementation still timed out because the crypto primitive was misidentified. Rechecking `0x1473bce40` showed the `expand 32-byte k` ChaCha core and Poly1305-style tag flow used by the default generator state; the app switched from AES-GCM-SIV to `ChaCha20Poly1305`, but a live run with that corrected forced trailer still timed out.

PlayerConfigurer then corrected the socket destination: native passes `MachineAddress` and `ServerPort` as the main `playerConnect` server host/port arguments while the advertised UDMUX/public endpoint remains the UDP socket target. A later broader NetworkClient map (`ClientRuppConfiguration` construction around the `"RbxTransport Client will connect to server {}|{}, udmux {}|{}"` log and `ClientRuppGenerator::generateHeader`) corrected the remaining port split: the RUPP endpoint TLV and qdmux SNI/CID fields use the RCC/server endpoint port, not `NetStackPort`. The app now connects the UDP socket to the advertised public/UDMUX endpoint port and uses the RCC/server endpoint port consistently for native RUPP/qdmux routing fields. A live run with the public/UDMUX port plus forced 18-byte trailer still timed out. The next native enabler correction is therefore that `0x14343f520` is not unconditional: the send path checks the state flag through `0x14343f640`, and the helper itself rejects packets unless the `+0x2d`/`+0x2a` state says protection is enabled. The no-forced-trailer route then timed out too, proving that visible qdmux SNI and correct public/UDMUX destination were still missing another native qdmux routing field.

The follow-up Windows Studio PE pass found the missing initial destination-CID shape in the RUPP/QUIC generator rather than another trailer mode. `QuicConnectionIdGenerator::generate` at `0x14757f850` writes a 20-byte CID as `0xd1 || rccIpv4 || rccServerPort || 13 bytes of native inner-CID material`, and the caller at `0x143618fac..0x143618fe5` stores the generated output with length `0x14`. The exact tail mode depends on whether `ServerRuppConfiguration +0x10` is empty: pure qdmux uses eight bytes from the inner QUIC CID generator followed by the low 40 bits of the wrapper-initialized counter `1` as a five-byte big-endian suffix, while the RUPP-configured route uses thirteen inner-generator bytes. The app now configures the recovered 20-byte initial DCID and SCID through ngtcp2's `ConnBuilder`, then generates subsequent local/source CIDs through its custom connection-ID callback, matching the native `getNewConnectionIdCb` family. The RCC IPv4 and RCC/server endpoint port remain visible qdmux routing fields, while all tail bytes are generated locally and never logged. Generated-qdmux configs now try (1) pure QUIC with generated qdmux SNI plus native `0xd1` initial/local CIDs in counter-tail mode and (2) ClientRuppGenerator RUPP prefix without SNI plus native `0xd1` initial/local CIDs in inner-tail mode; the known-failed no-CID/no-SNI fallback is no longer retried for that live shape.

## Roblox Studio profile/client-status presence

The public `presence.roblox.com` documentation exposes the read-only `POST /v1/presence/users` query endpoint, whose enum maps `InStudio` to value `3`. No public `presence.roblox.com` setter/register endpoint was found. Static 0.741 Studio strings instead show the `UseMatchmakingApiClientStatus` path and the documented Beta endpoint `POST https://apis.roblox.com/matchmaking-api/v1/client-status`, with the exact JSON shape `{"browserTrackerId":…, "status":"…"}`. Recovered native status strings include `AppStarted`, `JoiningGame`, `InGame`, and `LeftGame`; the startup path sends `AppStarted`.

`src/roblox_api.rs` mirrors that safe Studio startup heartbeat when the app opens with a saved `.ROBLOSECURITY` cookie. The first attempt only used the newer Matchmaking API, which can be accepted without changing the visible profile presence. The app now resolves a non-zero BrowserTrackerId from a pasted full cookie header, authenticated app-launch-info, the `www.roblox.com` Set-Cookie bootstrap, or a generated per-launch fallback, then sends `status="AppStarted"` by trying the native legacy `www.roblox.com/client-status/set` GET form first, falling back to the legacy JSON POST form and finally `POST https://apis.roblox.com/matchmaking-api/v1/client-status`. The Settings UI uses short per-request deadlines, returns after the first accepted status write, and has a local 45-second watchdog so Android networking cannot leave the panel stuck on `Sending…`. It then queries `POST /v1/presence/users` for the authenticated user and reports whether Roblox currently returns `InStudio=3`. The app does not claim success if Roblox still reports Online; full InStudio presence may still depend on the later Team Create connected session.

## App behavior after the RbxTransport pivot

`src/team_create.rs` now performs a static-selector remap before sending any legacy RakNet datagrams. When a fresh Team Create config contains:

- a usable public UDP/UDMUX endpoint,
- an RCC/server endpoint,
- `NetStackPort` or `RbxTransportPort`,
- `NetStackTokenValue` or `RbxTransportToken` decoding to 16 bytes, and
- a 32-byte RbxTransport early public key from either `EphemeralEarlyPubKey` or the `ClientPublicKeyData` application `RbxTransportEphemeralEarlyPublicKey`,

it reports `selectedTransport=RbxTransport`, preserves only safe token/key metadata in the UI report, intentionally skips the RakNet connected probe, then attempts the RbxTransport path: ngtcp2 QUIC with Rustls TLS, ALPN `RbxTransport`, RFC 7250 raw-public-key verification against the 32-byte early key, native NetStack RUPP token subtype `1`, native RbxTransport endpoint TLVs `2`/`3` carrying the RCC/server endpoint port when the RUPP prefix route is used, a generated redacted qdmux GameFqDN/SNI route when the join config omits `GameFqdn`, the recovered native `0xd1`/RCC-IP/RCC-server-port/inner-CID 20-byte qdmux initial destination CID via ngtcp2's `ConnBuilder` DCID/SCID configuration and custom connection-ID callback (including the native counter-tail mode for pure qdmux), the native `+0x2d`-gated 18-byte ChaCha20-Poly1305 RUPP/QUIC CID trailer implementation only when a route explicitly enables it, and the native 10 second QUIC handshake budget. The app also parses/builds candidate BaseClient early-auth and OpenReliable-control bytes for diagnostics, but still does **not** emit them on the unresolved path. This avoids burning a one-use Team Create config on the wrong transport now that the runtime flags are treated as RbxTransport-first. A live connected-client fix should only be claimed once the report observes inbound RbxTransport stream/datagram traffic; `0x145cc44c0` is now identified as the network-emulation configuration helper reached near the end of the RbxTransport path, not the QUIC handshake itself.

## Session worker and latest handshake report (2026-10-03)

A fresh live gamejoin report tried both recovered routes (generated qdmux GameFqdn/SNI with the native `0xd1` initial DCID, then the ClientRuppGenerator prefix without SNI) and both timed out at the native 10,000 ms QUIC handshake budget. The timeout occurs before TLS/RPK completion and before any inbound Team Create stream/datagram is accepted; early auth and channel-control bytes are still intentionally unsent. This report therefore does not exercise the receive dispatcher yet.

### 2026-10-04: user-supplied Studio runtime fallback trace

The user-provided Studio log reports `ngtcp2_conn_handle_expiry: ERR_HANDSHAKE_TIMEOUT`, a peer closed during handshake “with no response,” then a RakNet fallback that connected successfully. This is consistent with the 0.741 selector's RbxTransport-failure-to-RakNet fallback path and confirms that the observed Studio attempt reached the ngtcp2/RbxTransport handshake before falling back. It does **not** prove a successful RbxTransport handshake, identify why no response arrived, or establish that TeamCreateManager accepted the session. It also does not reveal whether the native I/O backend was `sys` or `libuv`, nor the event-loop thread count. This Studio log is separate from the Android app run below, which recorded zero inbound UDP datagrams.

The report's `NetStackPort=51433` versus public UDMUX/RCC server port `58526` split is not treated as a route bug. The `connected NetStackTokenValue + RCC:51433` diagnostic is the legacy RakNet connected route (reverse-endpoint TLVs 6/7); the selected 0.741 RbxTransport route uses the advertised public UDMUX endpoint as the UDP destination and the RCC/server port for qdmux fields and ClientRuppGenerator endpoint TLVs 2/3. The code now labels that distinction explicitly and has a regression fixture with different values for the two ports.

Handshake errors now include per-route UDP transmit/receive counts, bytes, stripped RUPP envelopes, inbound QUIC long-/short-header versus other prefix counts, and socket errors. This distinguishes an outbound-only timeout from a case where UDP replies arrive but ngtcp2/Rustls cannot advance the handshake. The app's `Start Transport Session` action runs gamejoin/transport work on a background worker; after a successful QUIC handshake it retains the ngtcp2 connection and receive/dispatch loop until the user stops the session or the peer closes it. Stopping the app also signals the worker to cancel. This is a long-lived receive baseline, not yet a complete Team Create join: the reliability-2 channel/wire-ID writer, application-level ACK contract, and early-auth send remain gated on native evidence.

### Follow-up UDP return-path measurement

A later run used public UDMUX `128.116.54.33:51416`, RCC `10.32.4.14:51416`, and separate `NetStackPort=50499`. Both route variants again timed out with no receive datagrams and no socket errors. The pure-QUIC route reported 7 successful local UDP sends / 8,400 bytes (seven 1,200-byte datagrams); the RUPP route reported 7 / 8,617 bytes, exactly 31 additional prefix bytes per datagram. This confirms the local send wrapper is applying the configured RUPP prefix on each send, but it does not prove packets reached the network or server. With zero inbound datagrams, no TLS/RPK, early-auth, stream framing, or channel-ACK stage was reached. The result does not distinguish upstream egress/return filtering from a silent server/qdmux drop due to route metadata; it is not evidence to substitute `NetStackPort` for the advertised UDMUX/RCC server port.

## 2026-10-03: Security gates and the current pre-session failure

The evidence shows multiple gates, not one “Team Create security check”:

1. **Control-plane authentication/authorization.** `RobloxApiClient::team_create_join` (`src/roblox_api.rs:899–963`) posts to `gamejoin.roblox.com/v1/team-create` with the `.ROBLOSECURITY` cookie and `{placeId, gameJoinAttemptId}`. It obtains a CSRF token through the logout challenge helper, includes `X-CSRF-TOKEN`, and retries with a challenge token if the response supplies one. The live run that produced the join config passed this exchange. That establishes only that Roblox issued join configuration; it does not prove the later UDP/data-plane session was accepted.
2. **Transport routing and peer authentication.** The returned config supplies route material (including the 16-byte NetStack token, endpoints, and 32-byte early key). The client uses those fields for qdmux/RUPP routing and configures TLS 1.3 with ALPN `RbxTransport` and a raw-public-key verifier (`src/team_create.rs:2446–2593`). That verifier can reject a presented key/signature only after a server handshake packet arrives. The current captures contain no inbound datagrams, so this check has not produced either a pass or a rejection. The token/SNI/CID/RUPP values may also be checked by qdmux before a QUIC response; a silent drop there remains possible, but is not proven.
3. **BaseClient early authentication and channel setup.** `ClientTicket` is parsed into the native `0xA8` early-auth payload (`src/team_create.rs:1212+`), and Studio’s `sendEarlyAuthData` path requires an active connection (static trace above). This app deliberately stages, but does not send, that frame or channel-open control while the reliability-2 wire-channel route is unresolved. These later checks therefore cannot explain the current *QUIC-handshake* timeout; they could matter after transport establishment, before a full Team Create session becomes usable.

**What the latest symptom means:** each of the two recovered route variants locally sent seven UDP datagrams, with zero received and no socket errors, then hit the 10-second QUIC timeout. There was no TLS alert, RPK mismatch, explicit Roblox rejection, or inbound packet to inspect. The HTTP join/config gate passed; the transport gate is where progress stops, before the app’s RPK verifier and before `ClientTicket` early-auth/channel handling. So a security-related qdmux route-token rejection is one possible silent-drop explanation, but the evidence does not support calling this an account/permission denial. Network egress/return filtering, a server/qdmux drop, and a still-wrong first-flight route remain indistinguishable without inbound traffic or a packet capture at the network boundary.

## 2026-10-03: EditPlace CLI and Team Create server-list interpretation

The supplied 0.741 executable contains the `-task` option string, distinct `TeamCreateEdit`, `TeamCreatePlayClient`, and `TeamCreatePlayServer` task names, and TeamCreateManager milestones such as `TeamCreate-WaitForJoinConfig`, `TeamCreate-HandleConnection`, and `onConnectionAccepted`. This supports separate edit and play/session stages; strings alone do not establish that `EditPlace` runs without a UI or bypasses any authentication.

The current Creator Hub CLI reference describes `EditPlace` as opening the latest published place for editing and lists `--placeId`, `--universeId`, and `--task` as required arguments. It documents a command-line launch workflow, not a headless Team Create protocol client. The given legacy single-dash form may be accepted by 0.741, but the version-specific behavior should be confirmed on Windows. If the place is Team Create-enabled, opening it through Studio is a useful native test of the CloudEdit workflow; it still relies on Studio’s signed-in identity and the applicable collaboration permissions. The command is not a direct “join this listed server ID” operation.

A visible Team Create/server-list entry is consistent with `POST /v1/team-create`: listing/discovery shows a session exists, while the authenticated gamejoin request obtains the per-attempt join configuration needed to connect. The control-plane pattern is game-join-like, but the transport is not interchangeable with an ordinary gameplay join: this 0.741 path selects RbxTransport/CloudEdit and later BaseClient auth/channel stages. The repository’s ordinary gamejoin URL helper points to `/v1/join-game`; Team Create uses its dedicated `/v1/team-create` route.

The 0.741 binary also contains Team Create safety/eviction UI and feature names for age verification, parent permission, trusted connections, terms of use, and locked experiences. These show additional collaboration-policy states exist, but the static strings do not establish when they are checked or that any caused this run. No such denial was returned in the observed gamejoin/UDP attempt.

Reference: [Roblox Studio command-line interface](https://create.roblox.com/docs/studio/command-line-interface).
