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

---

### What remains outside the decompile export

The hidden source operand at `0x1039c5c89` requires instruction-level disassembly for absolute proof. A native first-connected-datagram capture would independently verify the resulting 31-byte prefix and provide the live five-tuple, low-16 counter bytes, ciphertext, and final authentication tag. Hook values at `encryptRakDataInPlace` (`0x1039ea560`) and `SocketLayer::SendToOrDelay` (`0x1039e8c34`) would then permit the only remaining byte-for-byte comparison against the reimplementation.

Until one of those artifacts is available, the `RakPeer+3296` template is the overwhelmingly likely assignment source, but remains formally operand-unproven. No packet-shape, routing, key-direction, nonce, AEAD-framing, or reliability change is warranted by this text-only evidence.
