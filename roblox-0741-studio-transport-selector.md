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

## Fallback flags

| Flag/storage | Evidence | Selector behavior |
|---|---|---|
| `RakNetConnectionFailureFallbackToRbxTransport` storage `0x14d915cb0` | registration thunk `0x145b765e0..0x145b765f4`; selector reads at `0x145b75854` and `0x145b75ebc` | If initial selection is RakNet and this flag plus `canUseRbxTransport` are true, sets up fallback-to-RbxTransport (`0x145b75ef8..0x145b75fa9`). |
| `RbxTransportConnectionFailureFallbackToRakNet` storage `0x14d915c88` | registration thunk `0x145b76640..0x145b76654`; selector reads at `0x145b75b39` | If initial selection is RbxTransport and this flag is true, sets up fallback-to-RakNet (`0x145b75b46..0x145b75c56`). |
| `RbxTransportFallbackStudioMessage` storage object `0x14c4173b8` | registration thunk `0x145b76740..0x145b76754`; message use refs in fallback handling, e.g. `0x145b720dc..0x145b72109` | Used when an RbxTransport connection closes and Studio shows/logs a fallback message; it does not decide the initial transport branch. |

## RbxTransport config handoff recovered so far

`0x145b6a4b0..0x145b6b258` is the RbxTransport path selected by the branch above. It copies the same base connection input as RakNet, then additionally consumes the NetStack/RbxTransport fields. Evidence strings:

- `0x148f04f60`: `[DFLog::NetworkClient] RbxTransport Client RuppConfig = RCC {}:{}, DSR {}, Token type {}`
- `0x148f04fc0`: `[DFLog::NetworkClient] RbxTransport Client will connect without RuppConfig. udmuxEndpoint exists: {}, rbxTransportToken exists: {}`
- `0x148f05050`: `[DFLog::NetworkClient] RbxTransport Client will connect to server {}|{}, udmux {}|{}`

This is enough to prove selection, but not enough to implement the RbxTransport/QUIC client: the next native targets would be the downstream constructor/connect routine at `0x145cc44c0` and its auth/session-crypto callees.
