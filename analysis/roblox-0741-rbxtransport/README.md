# Studio 0.741 RbxTransport bounded analysis

**Target:** supplied `RobloxStudioBeta.exe` (219,753,936 bytes), SHA-256 `b065d5ef7ce099eddbc0a2dc60e455eb8cdb946424ed2bf6b05a4ad3bd5f51d5`.

**Tool:** Rizin `0.9.1 @ linux-x86-64`, SHA-256 `ab5614cf137608cffc5a3f4f471a91c995cfa7c0c35465bfb73876e5b95561ba`; executable restored from the user-fetched `tools/` repo state under `/tmp/rizin-unpacked/`. The target PE and tools are temporary files outside this repository.

All hexadecimal code/data addresses in these artifacts are VAs from the supplied 0.741 PE. `*.asm` is targeted `pdf` or raw disassembly, `*.cfg.json` is Rizin `afbj` output, and `*.info.txt` is `afi` output. No whole-image Rizin reference scan was repeated.

## Main result: one concrete 0.741 RtcIo event-loop worker path

- `0x1435d40e0` (`RtcIoRna::Impl` constructor path) reads the `RbxTransportRtcIoUseMultipleEventLoopThreads` byte from `0x14d735648` and writes it to `this+0x08`. It constructs the I/O manager at `this+0x10` with count `1`, calls the manager start routine `0x143447b40`, and handles a startup error with the log string `Failed to start IO event loop thread(s): {}`.
- `0x143447b40` uses the configured count as the loop bound. Only a zero count falls back to `_Thrd_hardware_concurrency`; that fallback is not taken for the observed count of one.
- `0x143447a70` creates each OS worker with `_beginthreadex`, using entry `0x143446d90`. The worker formats the name `RBX IoEvLoop {}`, runs `0x1434464d0`, and broadcasts at thread exit. The loop calls the event-loop virtual slots at `+0x08` and `+0x28` through its active and drain phases.
- `0x143447550` first signals each manager entry through `0x143446160`, then checks thread IDs and joins non-current workers through the MSVC `_Thrd_join` wrapper. The exact poller wake primitive remains unresolved.
- This proves **one dedicated I/O worker for this inspected manager instance if startup succeeds**. It is not a process-wide Studio thread count and does not include the separate BaseClient task scheduler.

## Feature flags and backend selection

- The name `RbxTransportRtcIoUseMultipleEventLoopThreads` at `0x148aa7280` is passed with data address `0x14d735648` and type id `2` by the stub at `0x1435db340`. The shim at `0x143862120` rearranges the arguments and tail-jumps through `0x143896a70` into `0x14388ed50`; this is registration plumbing, not by itself proof of runtime use. The PE byte at `0x14d735648` is initially zero.
- Runtime use is separately visible: constructor `0x1435d411d` loads `[0x14d735648]` into `this+0x08`; at `0x1435d4373`, a nonzero value chooses event-loop vector index `0` (`0x1434479f0`), while zero chooses a PRNG-derived index (`0x1434479b0`, computing `0x1438696e0() % vector_count`). The chosen loop pointer is passed into registry setup (`0x1435d3a60`), and the flag later selects the registry member at `+0x298` or `+0x210`. The flag affects loop/registry selection in this constructor; it does **not** set the manager count here. Since the manager vector has one entry, both selection paths resolve to that same worker.
- The adjacent `RbxTransportRtcIoEventLoopThreadCount` literal is at `0x148aa72b0`. A direct executable-section `/r` scan over `0x143000000–0x144000000` produced no reference lines. This is a scoped negative only; no binary-wide absence or runtime consumer is claimed. The count passed by the constructor above is the immediate constant `1`.
- `0x143446fb0` recognizes explicit backend strings `"libuv"` and `"sys"`, dispatching to `0x143465e90` and `0x14344ba20`, respectively. With no explicit backend name it checks `[0x14d4a6cc8]`; the PE image byte is zero and that static default reaches the `"sys"` helper. A live runtime override/selected backend is not known. 0.741 RTTI contains `SysEventLoop`, `LibuvEventLoop`, and `RtcIoRna` class names.

## Separate BaseClient task path

`0x14603e7f0` wraps callback `0x14603b3d0`, submits it through generic scheduler helper `0x1427c7fc0` with task name `BaseClient::ClientConnectThread`, and stores the returned task object at client offset `+0x78`. This factory does not itself call `_beginthreadex`; the task scheduler's OS-thread affinity/count was not followed to a thread-creation site. Do not count this named task as an additional dedicated RtcIo worker without that evidence.

The native Studio log supplied by the user reports an ngtcp2 handshake timeout, closure “with no response,” and a successful RakNet fallback. It corroborates a failed RbxTransport handshake followed by fallback, but does not establish the active I/O backend, event-loop count, or TeamCreateManager acceptance.

## Version/signature boundaries

The 0.740 IDB names candidate classes (`RtcIoRna`, `SysEventLoop`, `LibuvEventLoop`) and 0.741 has matching RTTI class names and a direct factory CFG. This is class-family corroboration, not a byte-for-byte 0.740-to-0.741 function signature match; keep the 0.740 VAs in the report labeled as candidates. Studio 0.735 remains a 2022 architectural lead only; do not present its addresses as 0.741 matches. The 0.740/0.741 Windows builds are 2026 targets.

## Artifact map

- `client/` — 0.741 RbxTransportClient establishment, receive/wait, teardown, and connect-task factory.
- `client/async-scheduler/` — bounded generic scheduler helper disassemblies and CFGs.
- `client/thread-primitives/` — task/thread primitives and scheduler dispatch candidates.
- `event-loop/` — manager factory/start/worker/shutdown and event-loop CFGs.
- `event-loop/backend/` — `sys` and `libuv` constructor helper disassemblies.
- `event-loop/flag-branch/` — flag consumer/index selection, PRNG helper, registration stub, RTTI strings, and scoped ThreadCount reference-search record.

## Caveats / do not repeat

- `af @ 0x1435db34d` misidentified a tiny registration thunk with an implausible ~2.86 MB function size. Use the saved raw `pd` around `0x1435db340` and the constructor load at `0x1435d411d`; do not rely on the inflated function boundary.
- `af @ 0x143862120` likewise reports an implausible ~215 KB function for a 48-byte shim, and `pdf` fails with a linear-size/bbsum mismatch. Use the raw snippet in `event-loop/flag-branch/registrar-helper-raw-pd.txt`.
- The ThreadCount search result only covers executable code in `0x143000000–0x144000000`; it is not a binary-wide no-reference result.
- An unscoped full-image reference search previously ran to the six-minute limit. Do not repeat it. The bounded range queries completed and are recorded under `event-loop/flag-branch/`.
- The current analysis pass changed documentation/artifacts only; no app source changes, build, or tests were performed for this binary investigation.
