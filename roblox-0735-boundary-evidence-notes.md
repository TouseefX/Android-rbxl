# 0.735 — Resolution of the four remaining boundaries

Build: **0.735 Studio, Mac symbols, 2026-08**. All quotes verbatim from the export; addresses preserved.
Still true: no assembly/runtime capture can come from this text-only export — but each boundary is now closed or reduced to pure byte-value confirmation.

---

## Boundary 1 — RUPP object installed at 0x1039c5c89: RESOLVED

The client Reply2 install is an exact mirror of the Request2 (server-side) install at 0x1039c4fe2–0x1039c5027, where Hex-Rays' argument recovery **survived** (RakPeer.c:8863–8870):

```c
RBX::make_shared<RBX::Rupp::Rupp>(a1: v104);          /*0x1039c4fe2*/  // fresh, EMPTY Rupp
v68 = v91;                                            /*0x1039c4fe7*/
v69 = (Rupp**)((char*)v91 + 5280);                    /*0x1039c4fee*/
shared_ptr<Rupp>::operator=(a1: v91+5280, a2: v104);  /*0x1039c5002*/  // ← installed object identified
shared_ptr<Rupp>::~shared_ptr(a1: v104);              /*0x1039c500a*/
*(_OWORD *)v101 = 0; v102 = 0;                        //               // empty token blob
Rupp::addTokenTlv(a1: *((_QWORD*)v68 + 660), a2: v101); /*0x1039c5027*/ // (!) +660 QWORDs == +5280 bytes — 4th independent confirmation of the offset
```

**Answer: the installed object is a newly-created empty `RBX::Rupp::Rupp` (default flags), to which a Token TLV is appended immediately after installation.** The client's 0x1039c5c89 sequence is the same shape (same helper calls immediately before/after, same +5280). The "hidden source operand" is the local `make_shared<Rupp>` result. Token content thereafter is maintained by the generator machinery shipped in the P1 zip (`updateTokenTlv` 0x1037b6072, `autoUpdateToken` 0x1037b5f7a, `requestImmediateTokenRegeneration` 0x1037b67ae, Client/ServerRuppGenerator).

## Boundary 2 — route prefix of the first connected packet: RESOLVED

`RakPeer::startOfflineBitStream(BitStream& out, const RemoteSystemStruct* rss, …)` @ **0x1039c2eac** (RakPeer.c:7590+):

```c
  *(_OWORD *)v18 = 0;
  if ( rss != nullptr )
    shared_ptr<Rupp>::operator=(a1: v18 /* a2 dropped: &rss->rupp (+5280) */);   /*0x1039c2eed*/
  ... (else branch: vtable+816 ... -- covered below)
  if ( v18[0] != nullptr ) {
    ruppLen = Rupp::getHeaderTotalByteLength(v18[0]);                 /*0x1039c2f0f*/
    BitStream::AddBitsAndReallocate(out, 8*ruppLen);                  /*0x1039c2f29*/
    log "[DFLog::Rupp] Offline ruppLength: {}"
    v10 = BitStream data base;                                        /*0x1039c2f93*/
    Rupp::serialize(a1: status, a2: rupp, a3: v10 /*buf@0*/, a4: ruppLen, a5: a4 /*startOffset*/); /*0x1039c2fa6*/
    BitStream::SetWriteOffset(out, 8*ruppLen, false);                 /*0x1039c3086*/  // RakNet header follows after
  }
```

So **every** connected outgoing packet (first = `sendApplicationConnectionRequest` right after Reply2, called at 0x1039c5c3c) is built as: reset BitStream → serialize the **per-connection rss+5280 RUPP at byte offset 0** (cleartext) → append RakNet DatagramHeader+payload after it. **Route prefix = exactly `Rupp::serialize`'s output of the installed Rupp** — for a freshly-installed client Rupp that is a base header + Token TLV; nothing else gets prepended.

Reverse-direction variant (`startOfflineBitStreamReverse` @ **0x1039ce1dc**, RakPeer.c:14190+): constructs `Rupp(1,1)` + `addIpv4ReverseEndpointTlv(srcAddr+24, srcPort @ +70)` and serializes the same way — this is the server-reply route prefix.

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
[ ruppLen bytes : serialized RUPP, CLEARTEXT                              ]  ← startOfflineBitStream
[ N bytes       : RakNet DatagramHeader + payload, AEAD ciphertext        ]  ← encryptRakDataInPlace over [ruppLen, ruppLen+N)
[ 2 bytes       : packet counter low-16 LE                                ]
[ 16 bytes      : Poly1305 / GCM tag                                      ]
```

call chain: `SendImmediate/SendBitStream` → (RakPeer::) `SendToOrDelay` wrapper @ 0x1039e1c20 → `SocketLayer::SendToOrDelay` @ 0x1039e8c34 → `RakPeer::trackSocketSendResult` @ 0x1039e1c59. Destination = the SystemAddress from the RSS/calling site; the socket is the shared_ptr<RakNetSocket> threaded through every send signature.

---

### What still genuinely needs runtime evidence

Only byte *values*, not structure: (a) the actual 5-tuple/low-16 counter wiring on pathological reconnects, (b) the live session key bytes by definition, (c) final Tag bytes — all three are entropy, not logic. If your emulator reproduces the layout above and the server still rejects, the delta is cryptographic (key schedule / nonce counter base / trailer order), not routing or packet shape.
