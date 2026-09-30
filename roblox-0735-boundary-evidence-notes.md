# 0.735 — Resolution of the four remaining boundaries

Build: **0.735 Studio, Mac symbols, 2026-08**. All quotes verbatim from the export; addresses preserved.
No instruction-level operand or runtime capture can come from this text-only export. Boundaries 2–4 are algorithmically determined; Boundary 1 is reduced to one surviving compatible source but remains formally operand-unproven.

---

## Boundary 1 — RUPP object installed at 0x1039c5c89: strongest decompile resolution (operand-unproven)

### Retraction

The earlier claim that `0x1039c5c89` installs a freshly-created empty Rupp by mirroring the Request2 path is withdrawn. `RBX::make_shared<RBX::Rupp::Rupp>` occurs exactly twice in `RakPeer.c`:

| site | context |
|---|---|
| `0x1039c4fe2` | server-side `processRbxOpenRequest2` (installs at `rss+5280`, adds an empty-token TLV) |
| `0x1039d0714` | `RakPeer::setupRuppImpl` (installs at the `RakPeer+3296` template) |

Neither site falls inside `processRbxOpenReply2` (`0x1039c5404–0x1039c5e1b`). There is no `make_shared`, `addTokenTlv`, or endpoint-TLV construction adjacent to the client install.

### `setupRuppImpl` builds the client default RUPP at `RakPeer+3296`

```c
RBX::make_shared<RBX::Rupp::Rupp>(a1: &v12);                       /*0x1039d0714*/
v8 = a1 + 412;                                                     /*0x1039d0719*/  // +3296 bytes
shared_ptr<Rupp>::operator=(a1: a1 + 412, a2: &v12);               /*0x1039d0726*/
RBX::Rupp::Rupp::setFlag(a1: a1[412], a2: 0, a3: a2);              /*0x1039d0763*/
result = RBX::Rupp::Rupp::addTokenTlv(a1: *v8, a2: a3);            /*0x1039d0798*/
if (result == 0) {
  if (inet_pton(2,  serverName, &v4)  != 0) return addIpv4EndpointTlv(*v8, v4,  port); /*0x1039d07f5*/
  if (inet_pton(30, serverName, &v12) != 0) return addIpv6EndpointTlv(*v8, v12, port); /*0x1039d0820*/
  return 10;                                                        /*0x1039d0827*/
}
```

`getRuppHeader()` copies the same `shared_ptr` from `this+3296` (strong-count word at `+3304`). The public `setupRupp` wrapper at `0x1039d06ae` is reached virtually/from a thunk in the Connect machinery, so textual call-site search does not expose its caller.

### Receive path confirms `rss+5280` is the live connected RUPP

`ProcessNetworkPacket` at `0x1039c617a` copies the `shared_ptr` from `rss+5280` and mutates that RUPP from connected receive results:

```c
shared_ptr<Rupp>::operator=(a1: &v36 /* a2 dropped: rss+5280 */);    /*0x1039c62ec*/
if (*(v36+48) != 0) {
  if (Rupp::updateTokenFromDeserializationResult(v36, incomingResult, t) == 0) /*0x1039c6312*/
    RakPeer::sendClientTokenUpdateTimeTelemetry(...);                /*0x1039c6324*/
} else {
  updated = Rupp::updateTokenFromDeserializationResult(v36, incomingResult, t); /*0x1039c634c*/
  /* reportLateRuppTokenUpdate uses *(rss+5280)+64 */                 /*0x1039c65a4*/
}
```

`updateTokenFromDeserializationResult` has three call sites in the export: the two RakPeer connected-receive sites above, plus `ClientRuppGenerator::onPeerPacketReceived` at `0x1037b4dae`, which updates the generator's in-place Rupp at `this+96`. All are receive-path consumers; none occurs inside Reply2. The in-place `ClientRuppGenerator` object is not a `shared_ptr<Rupp>` operand candidate for `0x1039c5c89`.

### Elimination table for the hidden source operand

| candidate | status | evidence |
|---|---|---|
| Fresh `make_shared<Rupp>` | **Disproven** | No `make_shared` occurs inside Reply2. |
| `RequestedConnectionStruct`-carried Rupp | **Disproven** | Reply2 has no RCS Rupp-field access. |
| Rebuilt from the reply's parsed RUPP | **Disproven** | Reply2 consumes no `DeserializationResult` and performs no token/endpoint construction. |
| `RakPeer+3296` default template | **Sole surviving compatible candidate** | The RakPeer default at `+3296` is the only identified client-side `shared_ptr<Rupp>` compatible with this assignment. |

The default-template conclusion is also consistent with the reimplementation's constructed/emitted 31-byte prefix—base header, subtype-1 Token TLV, and IPv4 endpoint TLV—from its own diagnostic output, not a native Studio first-connected datagram. The source remains formally operand-unproven until instruction-level disassembly or a native capture is available.

## Boundary 2 — first connected route prefix: connected online path confirmed

Connected reliability datagrams use `ReliabilityLayer::startOnlineBitStream` at `0x1039e1158`, not the `startOfflineBitStream` family:

```c
BitStream::Reset(out);
if (rupp) {
  ruppLen = Rupp::getHeaderTotalByteLength(rupp);                    /*0x1039e1199*/
  BitStream::AddBitsAndReallocate(out, 8 * ruppLen);
  ReliabilityLayer::reportRuppMissingTlvs(this, rupp);               /*0x1039e11c6*/
  Rupp::serialize(status, rupp, bitstream_base, ruppLen, timeUS);     /*0x1039e120b*/
  /* reportRuppSerializationFailure if status != 0 */                /*0x1039e122e*/
  BitStream::SetWriteOffset(out, 8 * ruppLen, false);                 /*0x1039e1247*/
  if (*(rupp+6) != 0) RakPeer::reportLateRuppTokenUpdate(...);        /*0x1039e1269*/
}
```

Its callers cover data (`sendDatagrams`, `0x1039e31e3`), ACK (`SendACKs`, `0x1039e3a9b`), and NAK (`checkSendNak`, `0x1039e166f`) datagrams. The caller supplies the RUPP dereferenced from the per-connection `shared_ptr`, so the serialized clear prefix begins at byte zero for each connected datagram class.

`reportRuppMissingTlvs` at `0x1039e127e` checks:

| check | meaning | condition |
|---|---|---|
| `rupp->hasTlv(1)` | Token TLV | Missing is reported. |
| `rupp->hasTlv(2)` | IPv4 endpoint TLV | Required unless the `RakPeer+3728` mode suppresses it. |
| `rupp->hasTlv(6)` | Reverse/relay endpoint TLV | Checked when a relay address is configured. |

These checks corroborate the endpoint-bearing `setupRuppImpl` model and contradict a token-only normal client prefix. Combined with Boundary 1, the evidence-backed initial connected prefix remains the 31-byte subtype-1 Token plus private IPv4 endpoint form.

## Boundary 3 — session-key activation + `encryptRakDataInPlace` keys: RESOLVED (algorithm-complete)

`RakPeerCrypto::clientInitEphemeralSessionKeys` @ **0x1039ab798** (RakPeerCrypto.c:103) — takes the **32-byte** server ephemeral public key (hard guard `a3 != 32 → fail`; this also tells you the dropped second arg at the 0x1039c5900–0x1039c5935 call site is a pointer to the 32-byte key inside the decrypted Reply2 body):

```c
crypto_kx_client_session_keys( rx = this+96  (server→client key),
                               tx = this+64  (client→server key),
                               client_pk = kxKeys+0, client_sk = kxKeys+32, server_pk = a2 )
on success: this+42 = 1     // isSessionReady — the ONLY activation flag the encryptor tests
```

`SessionCrypto::encryptRakDataInPlace` @ **0x1039ea560** (SessionCrypto.c:536, your proposed hook address — full body):

```c
if ( a3 < 0x12 )            return 1;   // needs room for the 18-byte trailer
if ( this+43 != 0 )         return 2;   // isError / disabled
if ( this+42 != 1 )         return 3;   // isSessionReady — set by clientInitEphemeralSessionKeys
v5 = a3 - 18;                           // plaintext length
v6 = 32 * *(uint8*)(this+40);           // isServer → picks key slot
v7 = a2 + a3 - 18;                      // trailer start
qmemcpy(v7, "UniqueNumbeR", 12);        // nonce template
*v7 = _InterlockedExchangeAdd64(this+16, 1);   // first 8 trailer bytes = monotonic counter
key = this + v6 + 64;                   // +64 client→server | +96 server→client
nonce = a2 + a3 - 18;                   // 12 bytes: 8-byte LE counter + ASCII "mbeR"
if ( this+45 & 2 ) crypto_aead_aes256gcm_encrypt_detached(out=a2, mac=a2+a3-16, &mlen, in=a2, v5, ad=0, adlen=0, nsec=0, nonce, key);
else               crypto_aead_chacha20poly1305_ietf_encrypt_detached( same args );
```

**The on-wire 18-byte connected trailer** (nonce buffer overlaps the tag by design — the tag is written over nonce bytes 10..25, so only these survive):

```
offset 0..1    packet counter, low 2 bytes (LE)
offset 2..17   16-byte Poly1305 tag (or AES-256-GCM tag if this+45 bit1)
```

Receiver-side nonce reconstruction: 8-byte LE counter (tracked, wire syncs low-16 bits) + literal `"mbeR"` (bytes 8–11 of the `"UniqueNumbeR"` template after the counter overwrite). **No AAD.** Cipher selection: chacha20poly1305_ietf default, AES-256-GCM if `SessionCrypto+45 & 2` (matches the kCryptoSessionSelectedCounter telemetry `"cipher": "chacha-poly"/"aes-gcm"` in processRbxOpenRequest2 @ 0x1039c4f22–0x1039c4f64). Return codes: 0 = success, 1 = too short, 2 = disabled/error, 3 = keys not ready, 5 = libsodium failure.

Server analog (same file family): `serverInitEarlySessionKeys_DualPair` @ **0x1039ab7f6** — **two** `crypto_kx_server_session_keys` (primary at kxKeys+192/+224 from keys at +64/+96, secondary at +256/+288 from +128/+160, both against the same 32-byte client ephemeral pubkey), stores the client pk at kxKeys+320/+336, sets `this+41` (isEarlySessionReady). This is the dual-pair behavior you already inferred.

## Boundary 4 — final buffer at `SocketLayer::SendToOrDelay`: composition fully determined

Your hook address 0x1039e8c34 sits at the entry of `SocketLayer::SendToOrDelay` (SocketLayer.c — shipped in the P1 zip). Pseudocode determines the buffer; a capture would only yield byte values, not structure:

```
[ ruppLen bytes : serialized RUPP, CLEARTEXT                              ]  ← ReliabilityLayer::startOnlineBitStream
[ N bytes       : RakNet DatagramHeader + payload, AEAD ciphertext        ]  ← encryptRakDataInPlace over [ruppLen, ruppLen+N)
[ 2 bytes       : packet counter low-16 LE                                ]
[ 16 bytes      : Poly1305 / GCM tag                                      ]
```

call chain: `SendImmediate/SendBitStream` → (RakPeer::) `SendToOrDelay` wrapper @ 0x1039e1c20 → `SocketLayer::SendToOrDelay` @ 0x1039e8c34 → `RakPeer::trackSocketSendResult` @ 0x1039e1c59. Destination = the SystemAddress from the RSS/calling site; the socket is the shared_ptr<RakNetSocket> threaded through every send signature.

## Connected request correction — Cloud Edit password resolved

The targeted current-build follow-up export resolves `RBX::Network::versionB` without relying on the older source tree. `initWithCloudEditSecurity` @ **0x106d750b2** first assigns the empty string, appends `"^"` @ **0x106d750d8**, and pushes decimal `17` @ **0x106d750e9**. The resulting byte string is exactly:

```
5e 11
```

`Network::Server::setupToReceiveStudioClients` calls that initializer @ **0x10687cdcb**, reads the resulting pointer and length, and passes both to the RakPeer incoming-password virtual call. On the client side, `RakNetClientConnection::connect` reads the same global string and passes its pointer/length to RakPeer `Connect` @ **0x106724dc3**.

Finally, `RakPeer::sendApplicationConnectionRequest` writes ID `0x09`, GUID, `GetTime`, and the false security byte, then appends the requested-connection password from `a2+58` with its length at `a2+314` @ **0x1039d00f3–0x1039d0103**. The Cloud Edit application payload is therefore byte-exact:

```
09 || GUID_BE || GetTime(false)_BE || 00 || 5e 11
```

The password belongs inside the encrypted reliable application payload; it does not alter the clear RUPP prefix or SessionCrypto trailer. With the previously observed 31-byte prefix and otherwise identical framing, adding these two bytes raises the initial UDP payload from 77 to 79 bytes.

## Current-build `GetTime(false)` clock — resolved

The second targeted export includes the previously missing free-function definitions. `RakNet::GetTime(bool)` @ **0x1039b48c3** takes the false branch used by the application request, calls `RBX::Time::now((SampleMethod)2)`, scales its seconds by `1,000,000`, and divides the resulting microseconds by `1,000` to produce milliseconds. The decompile renders the intermediate conversion as `(unsigned int)(int)(now * 1000000.0)`; that narrowing is recorded literally here rather than extrapolated into a new transport requirement.

`Time::now<2>` @ **0x1041b6780** forwards directly to `RBX::nowPrecise` @ **0x1041b678a**. `nowPrecise` has two flag-selected implementations, but both have the same clock origin and unit:

1. Read `mach_absolute_time()`.
2. Lazily establish the process-local `RBX::startTime` from a `mach_absolute_time()` sample (`nowPrecise` does so directly on the `FasterPreciseTime` path; the other path calls `Time::getStart`).
3. Subtract that start tick from the current tick.
4. Multiply by seconds per tick.

`RBX::tick_resolution` @ **0x1041b68e8** and the old-path `tick_frequency_helper` @ **0x1041b6e3d** compute the same scale:

```
seconds_per_tick = (mach_timebase_info.numer / mach_timebase_info.denom) * 1e-9
```

Both fall back to `1e-9` if `mach_timebase_info` fails. Thus the request timestamp is elapsed monotonic milliseconds since a lazy process-local start sample. It is neither Unix time nor raw `mach_absolute_time`, and it has no server-shared absolute origin. The reimplementation's app-start-seeded `Instant` preserves the protocol-relevant origin and monotonic elapsed-time semantics; no further wire-format correction follows from the completed clock trace.

## Live 0.741 routed outcome after the password correction

A fresh no-UI-delay session against RCC `0.741.0.7411056` authenticated OpenReply2 and emitted the corrected 79-byte packet: 31-byte subtype-1 client RUPP, 30-byte plaintext, and 18-byte SessionCrypto trailer. The plaintext was byte-structurally correct, including the 20-byte application request ending in `00 5e 11`, but neither it nor subsequent endpoint-bearing subtype-2 and bare diagnostics received an ACK.

That run did not actually test the exact RUPP returned with OpenReply2. The routed 158-byte Reply2 shape carries a **23-byte token-only outer header**, while the old “Reply2-token” diagnostic replaced the token inside the client's **31-byte token-plus-private-endpoint header**. The implementation now preserves the exact returned header and sends it once as a late diagnostic, separately from both the native 31-byte subtype-1 form and the receive-updater-style 31-byte subtype-2 form. Native-first behavior remains unchanged.

A second fresh run tested all three forms and still received no ACK. That removes the specific 23-vs-31-byte diagnostic gap and makes the 0.735-to-0.741 normal-session crypto boundary the strongest remaining candidate.

## Current-build normal-session KDF boundary

The public `kingdudely/Roblox-RakNet-Decompilation-Project` provenance updated on 2026-09-25 targets client build `9.4.260915.1d72b8c0`. Its current static trace reports **SHA-512**, not libsodium BLAKE2b, over:

```text
X25519 shared secret || local public key || peer public key
```

It does not yet establish the directional interpretation of the two 32-byte digest halves. This does not invalidate the live early channel: authenticated OpenReply2 directly proves that its separate early-key path works with the implemented BLAKE2b derivation. It does, however, justify two late normal-session diagnostics after the directly proven 0.735 BLAKE2b path fails: SHA-512 with the first half as client RX, and SHA-512 with the first half as client TX. Each candidate owns an independent nonce stream beginning at `UniqueNu`; neither changes the native-first packet.

Source: `https://github.com/kingdudely/Roblox-RakNet-Decompilation-Project/blob/main/docs/PROVENANCE.md` (current-build static evidence; directional split explicitly unresolved).

The first SHA-512 live diagnostic also received no ACK, but it used the original subtype-1/flags-1 client RUPP. The same current project's measured framing identifies established client-to-server headers as 31 bytes with a subtype-2 token and flags zero, while established server-to-client headers are the 23-byte token-only form. Therefore the next diagnostic is the bounded cross-product that had not yet been tested: a 31-byte header combining Reply2's flags-zero subtype-2 token with the original client endpoint TLV, encrypted once under BLAKE2b and under each SHA-512 digest-half orientation. This does not replace native-first behavior and does not expose the token.

### Fresh 0.741 no-delay live result

Commit `4776872` built successfully in Actions and was live-tested with a fresh gamejoin config handed directly to the UDP handshake. The safe config shape report was:

- `TokenGenAlgorithm = 1`
- `PepperId = 1790786389`
- `RandomSeed1 = string(88 chars)`
- advertised RCC version `0.741.0.7411056`
- auth version `17`, pre-auth `33` bytes, auth `66` bytes

The authenticated offline path is still solid: routed Request1 received Reply1 from `128.116.54.33:64830`; OpenRequest2 selected key version 5 through the URL-decoded `EphemeralEarlyPubKey` override; Reply2 was 158 bytes, version 1, ChaCha20-Poly1305, MTU 1200, capabilities `0x0000020350b70892`, and carried a 23-byte outer RUPP header with a subtype-2 token. The first connected plaintext was the corrected 30-byte reliable `ID_CONNECTION_REQUEST` under a 31-byte subtype-1/private-RCC RUPP header: data header `81 00 00 00`, reliable header `40 00 a0 00 00 00`, and payload `09 || client_guid_be || request_time_be || 00 || 5e 11`.

No connected ACK or `ID_CONNECTION_REQUEST_ACCEPTED` arrived for any candidate in that build: native subtype-1/BLAKE2b retries, original subtype-1 SHA-512 half orientations, established subtype-2/flags-0 endpoint-bearing BLAKE2b and SHA-512 half orientations, exact 23-byte Reply2 header, refreshed subtype-2 endpoint-bearing header, no-RUPP flow-affinity send, or one final native retry. This shifts suspicion away from UI delay, Request2 key selection, password, basic reliability encoding, and the specific 23-vs-31 Reply2-token gap.

One important diagnostic caveat remained: the established-header/KDF matrix was emitted after several native retransmissions, so those matrix packets used later datagram numbers and later transmit nonce counters. If earlier packets were being dropped before the server's connected crypto consumed them, a valid alternate route/KDF could still require a fresh datagram-0 packet with the initial `UniqueNu` nonce. Commit `8a67cb6` added a bounded fresh-first matrix for subtype-1/flags-0 and established subtype-2/flags-0 prefixes across BLAKE2b and both SHA-512 half orientations.

### Fresh 0.740 no-delay live result after the fresh-first matrix

A fresh no-UI-delay session against advertised RCC `0.740.487.7400001` authenticated the full offline path again. The safe metadata changed only in per-ticket values:

- `TokenGenAlgorithm = 1`
- `PepperId = 1790790878`
- `RandomSeed1 = string(88 chars)`
- public UDMUX `128.116.50.33:60356` routed to private RCC `10.20.3.206:60356`
- Reply1 GUID `1962642e45076f7c`, MTU `1200`
- Reply2 was 158 bytes, version 1, selected byte mapped by the legacy code to ChaCha20-Poly1305, capabilities `0x0000020350b70892`, and carried the same 23-byte token-only subtype-2 outer RUPP shape

The connected request was still structurally correct: 31-byte subtype-1/private-RCC RUPP, data header `81 00 00 00`, reliable header `40 00 a0 00 00 00`, payload `09 || client_guid_be || request_time_be || 00 || 5e 11`, first nonce suffix `[55 6e]`, final UDP payload 79 bytes. No ACK or accept arrived for any of the 19 packets, including all six fresh-first datagram-0/initial-nonce route/KDF candidates. This rules out the specific delayed-datagram/advanced-nonce caveat.

The next bounded diagnostic was cipher mapping rather than another reliability-layout change. The legacy 0.735 Reply2 byte interpretation selects ChaCha for value `1`, but an independent current-build packet corpus reports AES-256-GCM connected payloads with the same 18-byte SessionCrypto trailer. Commit `9a0ce26` kept negotiated-cipher behavior native-first, supported selected AES-GCM if a server advertises it, and added a late AES-GCM fresh-first matrix for subtype-1/flags-0 and established subtype-2/flags-0 RUPP prefixes across BLAKE2b and both SHA-512 half orientations.

### Fresh 0.741 no-delay live result after AES-GCM cipher diagnostics

A fresh no-UI-delay session against RCC `0.741.0.7411056` authenticated Reply1/Reply2 again with safe metadata `TokenGenAlgorithm = 1`, `PepperId = 1790795958`, and `RandomSeed1 = string(88 chars)`. Route was public UDMUX `128.116.54.33:57806` to private RCC `10.32.2.16:57806`; Reply2 was the same 158-byte/version-1/23-byte-subtype-2-token shape and reported selected ChaCha20-Poly1305.

No connected ACK or accept arrived for any of the 25 datagrams, including all six AES-GCM fresh-first route/KDF candidates. This removes simple selected-cipher mis-mapping as the explanation. The remaining current-build crypto gap is narrower: `RandomSeed1` is present in every fresh config and a public current-build headless-flow note treats it as normal-session KDF input, while the directly recovered 0.735 path does not. The implementation therefore now keeps the proven 0.735 KX native-first but adds bounded AES-GCM seeded-KDF diagnostics derived from `SHA512(seed || shared || client_pub || server_pub)` and `SHA512(shared || client_pub || server_pub || seed)`, both digest-half orientations, across subtype-1/flags-0 and established subtype-2/flags-0 fresh-first routes. The seed remains internal and is still reported only by shape.

### Fresh 0.741 no-delay live result after RandomSeed1 seeded-KDF diagnostics

A fresh no-UI-delay session against RCC `0.741.0.7411056` again authenticated Reply1/Reply2. Safe metadata was `TokenGenAlgorithm = 1`, `PepperId = 1790809599`, and `RandomSeed1 = string(88 chars)`; route was public UDMUX `128.116.97.33:63353` to private RCC `10.182.1.207:63353`. The connected plaintext remained the same corrected 30-byte request under a 31-byte subtype-1/private-RCC RUPP header.

No connected ACK or accept arrived for any of the 33 datagrams, including all eight seeded-KDF diagnostics. This removes the tested `RandomSeed1` placements, simple selected-cipher mis-mapping, route flags, 23-vs-31 RUPP framing, first nonce/datagram state, password, basic reliable-header encoding, and UI/ticket timing as primary explanations.

One bounded RUPP-token gap remained in the diagnostics: all subtype-2 endpoint-bearing sends used Reply2's 16-byte token value, while all original `TokenValue` sends kept subtype 1. Current captures show subtype 2 in established client-to-server headers, so the implementation now tests the missing shape separately: original GameService token bytes and private RCC endpoint, but flags zero and token subtype promoted to 2. It sends that promoted-original-token shape fresh-first across the existing KDF/cipher candidates, including the seeded AES-GCM variants, without printing token material.

If the promoted-original-token matrix is also silent, the strongest remaining blockers are the concrete current-build RUPP token generator (`TokenGenAlgorithm=1`, `PepperId`) and/or matching-build SessionCrypto key installation/epoch rekey logic.

## 0.740 Windows Player recovery corrections

The parsed 0.740 archive targets Windows Player `0.740.0.7400927`, not the exact `0.741.19.7411056` Studio build. Its broad disassembly range `fn_0x000142897340.asm` was initially labelled only from later string xrefs. The first real body in that range, **`0x142897340–0x142897555`**, is instead the Windows `RakNet::RakPeer::generateUdmuxToken` implementation:

- its Microsoft x64 ABI is `(RakPeer *this, uint8_t *out, SystemAddress const *remote, uint32_t value, lineage byte on stack)`;
- it reads the processor pointer at `RakPeer+0xEA0`;
- lineage `6` forces the alternate-token boolean, while other values visible in this body clear it;
- it builds local and optional remote endpoint context;
- the ordinary path dispatches `RuppTokenProcessor` virtual `+0x18`, and the algorithm-telemetry path dispatches virtual `+0x20` with an additional algorithm-result output.

This matches `lambdaBaa9::_Do_call` at `0x1428A4FB0`, which writes subtype `2`, loads the same five arguments, and invokes RakPeer vtable offset `+0x340` (slot **104**). Consequently the slot-104 concrete RakPeer target is `0x142897340`; the archive README's 80-slot export was truncated and its earlier classification of this broad disassembly as only a reply serializer was incorrect.

This recovery still does **not** expose the token primitive. The Windows `RBX::Rupp::RuppTokenProcessor` vtable at `0x146C989B8` contains one deleting destructor followed by four pure-virtual entries. The Windows offsets correspond to the older Mac interface's `verify_DEPRECATED`, `verify`, `generate_DEPRECATED`, and `generate`; MSVC's one destructor slot shifts the two generation methods to `+0x18/+0x20`. `RakPeer::generateUdmuxToken` therefore only marshals endpoint/lineage inputs into a processor supplied by the server environment.

The same Player binary's `setServerRuppStemma` path at `0x14289DFA0` allocates only the 16-byte base processor and installs that pure interface. That is consistent with dormant server code in a Player executable, not an embedded concrete RCC token backend. Recovering `TokenGenAlgorithm`, `PepperId`, and `RandomSeed1` semantics requires the concrete processor from a matching RCC/game-server module or a safe live configuration shape report; they cannot be inferred from this Player target.

This boundary is independent of connected SessionCrypto. The 0.735 Studio `RakPeerCrypto::clientInitEphemeralSessionKeys` still directly proves normal-session installation through `crypto_kx_client_session_keys` into `SessionCrypto+96` (server-to-client) and `SessionCrypto+64` (client-to-server). The unresolved 0.740/0.741 question is whether the newer Reply2 path changed that KDF or key installation; the RUPP generator supplies no evidence either way.

---

### What remains outside the decompile export

The hidden source operand at `0x1039c5c89` requires instruction-level disassembly for absolute proof. A native first-connected-datagram capture would independently verify the resulting 31-byte prefix and provide the live five-tuple, low-16 counter bytes, ciphertext, and final authentication tag. Hook values at `encryptRakDataInPlace` (`0x1039ea560`) and `SocketLayer::SendToOrDelay` (`0x1039e8c34`) would then permit the only remaining byte-for-byte comparison against the reimplementation.

Until one of those artifacts is available, the `RakPeer+3296` template is the overwhelmingly likely assignment source, but remains formally operand-unproven. No packet-shape, routing, key-direction, nonce, AEAD-framing, or reliability change is warranted by this text-only evidence.
