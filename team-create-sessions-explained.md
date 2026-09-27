# Team Create sessions — how the protocol works, and how to recreate a client

Source for everything citing a function/address: the 2022 decompile
("2022 Studio + full PDB" per SOURCE.md). REST endpoints are also plain-
text strings found in that build. Items marked (inferred) are reasonable
deductions, not decompiled facts.

--------------------------------------------------------------------------------
## 0. TL;DR

A Team Create session is, from the client's point of view, **a normal
Roblox network join into a special cloud server** ("cloud edit" server).
Studio does three things:

1. REST control plane (cookie auth) to check/hold the session
   (`/v1/universes/{u}/teamcreate`, `/v1/places/{p}/teamcreate/active_session/members`, …)
2. Game-join handshake (cookie auth) to get the server address
   (`POST https://{game-join-host}/v1/team-create`, and
   `/v1/team-create-preemptive` to spin the server up early)
3. A UDP RakNet-style replication channel (the same one any game client
   uses) over which the *entire place* is streamed down and all edits
   flow live in both directions.

So "recreating the Team Create client" = recreating **the Roblox
replication client**, plus a thin REST wrapper. The replication protocol
is the hard part; everything else is a day of work.

--------------------------------------------------------------------------------
## 1. The three planes (as verified in the decompile)

### 1a. REST control plane (host: develop/api, cookie+CSRF)

Class: `ApiTeamCreateUrlConstruction` (`_external/_a/ApiTeamCreateUrlConstruction.c`).
Each function composes a full URL from a host getter + a literal path:

| Function (2022 Studio + full PDB) | Host | Path literal |
|---|---|---|
| `constructTeamCreateStatusUrl` @ 0x141789cb0 | develop host | `/v1/universes/%lld/teamcreate` |
| `constructTeamCreateActiveSessionMembersUrl` @ 0x141789a00 | develop host | `/v1/places/%s/teamcreate/active_session/members` |
| `constructMultiGetTeamCreateStatusesUrl` @ 0x141789740 | develop host | `/v1/universes/multiget/teamcreate?{}` |
| `constructChangeTeamTestStatusUrl` @ 0x141789130 | develop host | `/v2/teamtest/{:d}` |
| `constructGetRunningGamesUrl` @ 0x141789490 | api host | (running TC servers listing) |
| `DEPRECATED_constructStopTeamCreateSessionUrl` @ 0x141788d10 | api host | deprecated stop-session |

Sister path found in the same area:
`/v1/universes/%lld/teamcreate/memberships` (who may edit).
In 2022 "develop host" = develop.roblox.com, "api host" = api.roblox.com;
today the fleet equivalents expose these under games.roblox.com (same shape).

Also exists: `TeamCreateService::sendUnarchiveUniverseRequestAsync`
(`_external/_t/TeamCreateService.c`).

### 1b. Game-join plane (gamejoin host, cookie+CSRF)

Class: `ApiGameJoinUrlConstruction` (`_external/_a/ApiGameJoinUrlConstruction.c`),
built from `getRobloxGameJoinHost(...)` (gamejoin.roblox.com):

| Function | Address | Path |
|---|---|---|
| `constructJoinTeamCreateUrl` | 0x141eae9c0 | `POST /v1/team-create` |
| `constructPreemptiveStartTeamCreateUrl` | 0x141eaeb10 | `POST /v1/team-create-preemptive` |
| `constructTeleportUrl` (unrelated) | — | `/v1/teleport` |

The request body is JSON built by
`CloudEditConnectionModel::constructTeamCreateGameJoinRequest` @ 0x142fb1130:
it serializes a map containing at least `placeId` and
`gameJoinAttemptId` (a UUID generated per attempt; also used when
`isPreemptive=true`, which swaps the URL to the `-preemptive` variant).

The response JSON is the join **config**. Keys directly confirmed in
`CloudEditConnectionModel::fillServerConnectionsArrayFromConfig` @ 0x142fb2240
and its caller:

- `Address` / `Port` — direct server endpoint entries, or
- `ServerPort`, `UdmuxEndpoints` — a list of UDMUX multiplexed endpoints
  (same structure modern gamejoin returns for games).

These are deserialized into `RBX::Network::Client::ServerConnection`
objects — i.e. "where you open the UDP socket."

### 1c. Replication plane (the real protocol — network/*.c)

All the remaining Team Create behavior is just the standard
client-server replication stack, at `network/` in the decompile.

Connection-side classes: `Client` (`network/Client.c`),
`ClientReplicator`, `ConcurrentRakPeer`, `Compressor`,
`CachedBitStream`, `ItemQueue`/`ItemSender`/`DeserializePacketsJob`,
`GuidRegistryService`, `REID` network IDs (`GuidDataValueGetter/Setter`).

**Wire item types confirmable from the decompile (network/ files):**

| Purpose | Item classes |
|---|---|
| Initial join payload | `JoinDataItem`, `JoinDataItemV2` (+ `DeserializedJoinDataItem`, `DeserializingJoinDataItemV2`) |
| Property/attribute edits | `ChangePropertyItem`, `ChangeAttributeItem` (+ Deserialized variants) |
| Instance create/remove | `NewInstanceItem`, `DeleteInstanceItem`, `DestroyInstanceItem`, `ResolveNewInstanceAtomicTreeItem`, `InstanceRemovalItem`, `InstanceItem` |
| Replicated events | `EventInvocationItem` |
| Streaming (region) replication | `ClusterSerializer`, `ClusterReplicationData`, `ClusterPacketCache`, `ChunkClumpMap`, `Chunk`, `StreamDataItem`, `StreamDataInfoItem`, `StreamPrefetchDataInfoItem`, `StreamPrefetchRequestItem`, `MegaReplicationData`, `Master` |
| QoS / keepalive / stats | `PingItem`, `MarkerItem`, `StatsItem`, `ClientStatsItem`, `TimeoutInfoRequestItem`, `ClientCapacityUpdateItem`, `ClientStreamCapacityUpdateItem`, `ClientVisualEffectItem`, `MemoryChallengeItem`, `PmcConfigItem`, `CcRequestItem`, `TouchItemS2` |
| Schema bootstrap | `ClassInfo`, `EnumDesc`, `EventDesc`, `TmpClassInfo` — the protocol's compact class/enum/event-name dictionaries streamed at join so later packets use indices |

So the describing sequence (for a new client implementing this) is:

1. UDP (RakNet-derived) connect to `{Address}:{Port}` (or one of the
   `UdmuxEndpoints`), Roblox-layered handshake with your join
   ticket/session.
2. Receive `ClassInfo` tables + `JoinDataItem(V2)` → reconstruct the
   whole place (or start receiving `Stream*` items if StreamingEnabled).
3. From then on: apply `NewInstance`/`ChangeProperty`/`DeleteInstance`/
   `Event` items as they arrive; keep capacity/stats/whathaveyou
   flowing so the server doesn't drop you.
4. Your **local edits** are serialized as the SAME change items sent
   client→server (Team Create grants the editor write authority; the
   server rebroadcasts them — last-write-wins per property/instance;
   there's no OT/CRDT).

Client-side join wiring is explicit in
`CloudEditConnectionModel::connectToTeamCreateSession` @ 0x142fae700 /
0x142fb0880 (the latter logs `"[FLog::Output] Joining game %s place %lld
at [%s]:%d"`), with signal hooks `onConnectionAccepted`,
`connectionAcceptedSignal`, `connectionFailedSignal`, and retry logic in
`getIsRetryNeededAndMaybeSetupTeamCreateConfig` @ 0x142fb26d0
(timeout/error codes logged "unable to initialize Team Create server").

### 1d. Script-editing plane (drafts)

`rbx/DraftsService.c` — the Team Create script draft system. Functions:
`getDrafts`, `getDraftStatus`, `getEditors`, `commitEdits`,
`discardEdits`, `updateToLatestVersion`, `restoreScripts`,
`showDiffsAgainstBase`, `showDiffsAgainstServer`, plus draft
added/removed/status-changed signals. In other words: script source is
NOT synced as plain property edits — each collaborator edits a private
draft copy and commits; the server tracks editors and merges by base/
server/own three-way diff. Companion classes:
`studio/CrossDMScriptChangeListener.c` (keeps script content in sync
between the visible and hidden datamodel), `ScriptCloneWatcher/Tracker`,
`reflection/DraftStatusCode_.c`, `VersionControlService.c` (mentions
Team Create).

### 1e. Presence / collaboration UX

CloudEditModel (`_external/_c/CloudEditModel.c`): `isCloudEditSession`,
`setIsCloudEditSession`, `isChatEnabledForUser`,
`maybeMakePlayersDataManager`.
CloudEditController: presence is the same as a game — collaborators
appear as replicating Player instances in the Players service; the REST
`active_session/members` endpoint is the listing for the widget.
`CloudEditUserData`/`CloudEditAdornable` handle the per-user colorings.
`requestStartTeamCreateSessionsPreemptively` @ 0x142faaca0 fires the
`-preemptive` join call (faster open).

--------------------------------------------------------------------------------
## 2. Session lifecycle, end to end

1. User opens place → Studio checks
   `GET https://{develop}/v1/universes/{u}/teamcreate`
   (is TC on? membership?) and, under FFlags, fires
   `POST https://{gamejoin}/v1/team-create-preemptive` to warm the server.
2. Studio posts `POST https://{gamejoin}/v1/team-create` with
   `{placeId, gameJoinAttemptId}` (each retry gets a new attempt ID).
3. Response = server config JSON (Address/Port or UdmuxEndpoints).
4. Studio opens UDP to that server with its join ticket, runs the RakNet
   handshake, accepts the class-info/join-data stream (the entire place),
   and considers itself "in session" once
   `onPlayerReceivedFromServerInCloudEdit` fires (the local editor's
   Player instance landed).
5. Live phase: every DataModel change propagates as replicator items
   (edit something locally → items emit upward; others' edits arrive as
   items you apply; scripts via drafts §1d).
   `PlaceholderPlayerCreationPolicy` / team-create settings start the
   document in a "cloud edit" DataModel marked by `maybeMakePlayersDataManager`.
6. Leave/crash: heartbeat failure → server tears down; Studio shows the
   disconnect UI (`TeamCreateDisconnectInfo`, popups, incident reporting).
   Client pipes keepalive (`PingItem`, capacity updates) to avoid that.
7. Stopping/kicking: control plane members endpoint; legacy stop-session
   is deprecated (the server side now owns shutdown;

## 3. Team *test* (LAN) — the other "team session"

Different thing despite the name: `studio/StartTeamTestVerb.c`,
`StartServerAndPlayerVerb.c`, `CloudEditController::isServerModeTeamTest`.
Studio launches a real `Network::Server` locally plus 1..N local client
processes auto-connecting to it (regular game replication, no drafts,
no cloud server). The cloud variant (remote, server-server players)
updates status through `POST {develop}/v2/teamtest/{id}`.
If your recreation needs local multiplayer test, mirroring
StartServerAndPlayerVerb = run your own replication server + spawn child
clients that join it — you do NOT need the Team Create path at all.

--------------------------------------------------------------------------------
## 4. Recreating the client in Rust for Android — engineering plan

### Must-implement, in dependency order

1. **HTTP w/ cookie + X-CSRF** (see `roblox-cookie-upload-endpoints.md` 
   for that bootstrap; it's identical here). TLS/reqwest.
2. **Join negotiation**: POST `/v1/team-create`, parse the config JSON
   (`Address`/`Port` or `UdmuxEndpoints`), pick a socket target,
   keep the `gameJoinAttemptId` for retries.
3. **RakNet-compatible UDP layer** (the biggest non-protocol task):
   connection handshake, reliable ordered channels, splits/ACKs,
   keepalive. Rework `ConcurrentRakPeer.c` faithfully to this build's
   vintages (it's a customized RakNet, not stock).
4. **Bitstream codec** (`CachedBitStream.c`): Roblox serializes
   everything Rak-style bit-packed (unaligned fields, endian order
   per field type), plus compression (`Compressor.c`).
5. **Class dictionary bootstrap**: on join the server sends
   `ClassInfo`/`EnumDesc`/`EventDesc` tables; all subsequent packets
   refer to classes/events/enum values *by those indices*. Without
   implementing this you cannot decode anything. (This also makes the
   protocol forward-compatible-ish, but pins the schema to the server's
   build.)
6. **Replicator items**: the set from §1c. Minimum viable editor:
   JoinDataItemV2 (place load) → NewInstance/ChangeProperty/ChangeAttribute/
   Delete/Destroy/ResolveTree (live state) → EventInvocation (if you
   want scripts/presence) → Ping/Capacity/Stats (so you aren't dropped).
7. **The place/object model layer your edits bang on**: reflection
   (class table, properties, enums), Instance tree keyed by REID
   (network identity issued by `GuidRegistryService`), the
   topological-resolve rules for `ResolveNewInstanceAtomicTreeItem`
   (instances arrive in dependency chunks).
8. **Edit API** for your own UI: locally mutate the tree → serialize
   the same change items upward. The SERVER is authoritative: it can
   refuse/override (last-write-wins), it is the thing that rebroadcasts
   to other editors. You do NOT do OT or CRDT anywhere; TC's conflict
   model is simply "server order wins".
9. Scripts: if you want collaborative script editing, Drafts (§1d) is a
   separate REST service layer on top (three-way diff commit).
   Everything else (parts, properties) is raw replication.

### Suggested staging

- **Stage 0 (1–2 days)**: REST control-plane only — query session
  status/members; prove cookie+CSRF works from Android.
- **Stage 1**: pure network observer. Join, ack everything, decode
  class-info + join data into your own tree; never send edits. Verify
  by watching live changes from a real Studio session.
- **Stage 2**: presence — get your Player accepted
  (`connectPlayerReceivedFromServerInCloudEdit` semantics), show user
  list.
- **Stage 3**: simple edits upward (move a part, rename an instance).
- **Stage 4+**: scripts/drafts, streaming-enabled places, everything else.

### Key classes in the decompile to follow (2022 Studio + full PDB)

| Concern | File | Anchor function |
|---|---|---|
| session connect | `_external/_c/CloudEditConnectionModel.c` | `connectToTeamCreateSession` @ 0x142fae700 / 0x142fb0880 |
| join request JSON | same | `constructTeamCreateGameJoinRequest` @ 0x142fb1130 |
| config → sockets | same | `fillServerConnectionsArrayFromConfig` @ 0x142fb2240 |
| retry/backoff | same | `getIsRetryNeededAndMaybeSetupTeamCreateConfig` @ 0x142fb26d0 |
| join URLs | `_external/_a/ApiGameJoinUrlConstruction.c` | `constructJoinTeamCreateUrl` @ 0x141eae9c0, `constructPreemptiveStartTeamCreateUrl` @ 0x141eaeb10 |
| control URLs | `_external/_a/ApiTeamCreateUrlConstruction.c` | (6 functions, addresses in §1a) |
| UDP/RakNet | `network/Client.c`, `network/ConcurrentRakPeer.c` | connect/ack machinery |
| codec | `network/CachedBitStream.c`, `network/Compressor.c` | bit packing + compression |
| item decode | `network/DeserializePacketsJob.c` + `network/Deserialized*Item.c` | packet → item |
| schema bootstrap | `network/ClassInfo.c`, `network/EnumDesc.c`, `network/EventDesc.c` | class dictionaries |
| join payload | `network/JoinDataItemV2.c`, `network/DeserializingJoinDataItemV2.c` | initial place |
| identity | `network/GuidRegistryService.c`, `network/GuidDataValueGetter.c` | REID/GUID mapping |
| scripts in TC | `rbx/DraftsService.c`, `studio/CrossDMScriptChangeListener.c` | drafts + sync |

### Realistic caveats

- This build's stock is 2022; today's servers/streaming features may
  differ (StreamingEnabled cluster replication, UDMUX defaults). Decode
  defensively: accept unknown item indices without dying.
- The dict bootstrap + compression details are version-pinned — derive
  them only by reading these classes, not from memory.
- Full replication implementation is the "engine" part of your Studio
  recreation (multi-month solo); consider gating it: many Studio-like
  tools ship REST-only (status/presence/asset ops) first.
- ToS: automated access with an account cookie is the gray area you
  already know from the upload endpoints. A *broken* implementation can
  also corrupt a TC place — the server is authoritative but applies
  well-formed edits verbatim; test against throwaway universes.
