# Roblox Studio 0.741 transport selector evidence

Target executable: `/tmp/winstudio_uploaded/extracted/RobloxStudioBeta.exe`

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
| `+0x78` optional string/endpoint, `+0x98` dword | Public/RbxTransport endpoint copied to the outgoing connect configuration at stack `+0x118/+0x138`; this is the QUIC target. The app reports it as public address plus `NetStackPort` when the Team Create payload supplies a separate NetStack port. It is also used as the UDMUX side of the `will connect to server {}|{}, udmux {}|{}` log. |
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

The dword writes go through NetStream helper `0x147393b20`, which calls the host-to-network conversion before appending the four stored bytes. `OpenUnreliableChannelControl` serializes the absolute value of the object field at `+0x18`; that field is a runtime wire-channel id allocated by the transport/channel handler and should not be guessed from the join config. The app now uses the exact OpenReliable shape after its QUIC/RPK/RUPP connection attempt; OpenUnreliable remains reported only until the runtime wire-channel id can be allocated from connected traffic.

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

Current reporting distinguishes the advertised public/UDMUX endpoint from the QUIC UDP target formed from the public address plus `NetStackPort`; one earlier live output therefore reported `128.116.54.33:52054` as the QUIC UDP target while retaining `128.116.54.33:51996` as the advertised UDMUX endpoint/open-RUPP peer port.

Latest live validation adds an immediate-response case with no legacy RakNet wait:

- QUIC UDP target: `128.116.50.33:58490` (`Address` plus `NetStackPort=58490`).
- Advertised UDMUX endpoint/open peer: `128.116.50.33:62638`.
- RCC/RUPP endpoint: `10.20.0.12:62638`.
- RCC version: `0.741.0.7411056`.
- `TokenGenAlgorithm=1`; `PepperId=1790880922 (0x6abeac9a)`; `RandomSeed1` is only reported by decoded length (`64` bytes); `TokenValue`/`NetStackTokenValue` remain redacted but shape-checked.
- RbxTransport early public key override: version `1`, length `32` bytes.
- BaseClient early-auth metadata: tag `0xA8`, auth version `17`, pre-auth `33` bytes, auth `66` bytes, total payload `103` bytes.

The app handed the fresh gamejoin config directly to the selected RbxTransport path and produced the selector/channel/open-auth report immediately; the missing former ~5 second UI stall is useful branch proof that the legacy RakNet probe is no longer being attempted first. The implementation now advances past reporting into a QUIC connection attempt using the public address plus `NetStackPort`, an RFC 7250 raw-public-key verifier for the 32-byte RbxTransport key, and a RUPP prefix built from `NetStackTokenValue` plus the RCC address remapped to `NetStackPort`. Follow-up decompilation of both the Studio Team Create payload path and the PlayerConfigurer path showed native constructs the NetStack `TokenTlv::Token` with subtype `1` directly; `TokenGenAlgorithm` remains safe legacy-token metadata and is no longer allowed to change the RbxTransport prefix subtype. The QUIC attempt also now uses Studio's native `RbxTransportQuicHandshakeTimeoutMs=10000` floor instead of the shorter UDP probe timeout.

A live run after the native subtype/native-timeout patch still timed out during QUIC handshake, before early-auth/channel-control traffic. The next decompile pass found the likely envelope mismatch in `/tmp/rbx_net/rbx_ClientRuppGenerator.c`: native `ClientRuppGenerator::generateHeader` constructs `Rupp(protocol=1)`, adds the token TLV, then calls `addIpv4EndpointTlv` / `addIpv6EndpointTlv` (TLV types `2`/`3`). The legacy RakNet routed-open path is the one that uses reverse-endpoint TLVs `6`/`7`. The app therefore now switches only the RbxTransport/QUIC prefix to native endpoint TLVs `2`/`3`, while preserving the existing RakNet reverse-endpoint prefix shape. Inbound Team Create traffic is still the success gate: if QUIC handshakes and no stream/datagram arrives after the early-auth write, the next remaining suspects are the recovered NetStream framing and the custom TLS capability extension.

## Roblox Studio profile/client-status presence

The public `presence.roblox.com` documentation exposes the read-only `POST /v1/presence/users` query endpoint, whose enum maps `InStudio` to value `3`. No public `presence.roblox.com` setter/register endpoint was found. Static 0.741 Studio strings instead show the `UseMatchmakingApiClientStatus` path and the documented Beta endpoint `POST https://apis.roblox.com/matchmaking-api/v1/client-status`, with the exact JSON shape `{"browserTrackerId":…, "status":"…"}`. Recovered native status strings include `AppStarted`, `JoiningGame`, `InGame`, and `LeftGame`; the startup path sends `AppStarted`.

`src/roblox_api.rs` mirrors that safe Studio startup heartbeat when the app opens with a saved `.ROBLOSECURITY` cookie. The first attempt only used the newer Matchmaking API, which can be accepted without changing the visible profile presence. The app now resolves a non-zero BrowserTrackerId from a pasted full cookie header, authenticated app-launch-info, the `www.roblox.com` Set-Cookie bootstrap, or a generated per-launch fallback, then sends `status="AppStarted"` through the native legacy `www.roblox.com/client-status/set` POST/GET forms and the newer `POST https://apis.roblox.com/matchmaking-api/v1/client-status` form. It then queries `POST /v1/presence/users` for the authenticated user and reports whether Roblox currently returns `InStudio=3`. The app does not claim success if Roblox still reports Online; full InStudio presence may still depend on the later Team Create connected session.

## App behavior after the RbxTransport pivot

`src/team_create.rs` now performs a static-selector remap before sending any legacy RakNet datagrams. When a fresh Team Create config contains:

- a usable public UDP/UDMUX endpoint,
- an RCC/server endpoint,
- `NetStackPort` or `RbxTransportPort`,
- `NetStackTokenValue` or `RbxTransportToken` decoding to 16 bytes, and
- a 32-byte RbxTransport early public key from either `EphemeralEarlyPubKey` or the `ClientPublicKeyData` application `RbxTransportEphemeralEarlyPublicKey`,

it reports `selectedTransport=RbxTransport`, preserves only safe token/key metadata in the UI report, intentionally skips the RakNet connected probe, then attempts the RbxTransport path: Quinn QUIC over a RUPP-prefixed UDP socket, ALPN `RbxTransport`, RFC 7250 raw-public-key verification against the 32-byte early key, native NetStack RUPP token subtype `1`, native RbxTransport endpoint TLVs `2`/`3` in the QUIC prefix, the native 10 second QUIC handshake budget, the BaseClient early-auth payload, and the recovered OpenReliable channel-control payload. This avoids burning a one-use Team Create config on the wrong transport now that the runtime flags are treated as RbxTransport-first. A live connected-client fix should only be claimed once the report observes inbound RbxTransport stream/datagram traffic; `0x145cc44c0` is now identified as the network-emulation configuration helper reached near the end of the RbxTransport path, not the QUIC handshake itself.
