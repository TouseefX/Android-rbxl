# Roblox Studio 0.741 place-publishing investigation

Target: `/tmp/winstudio_0741/RobloxStudioBeta.exe` (219,753,936 bytes), extracted from the supplied split 7z archive. Inspection used GNU `strings`, targeted GNU `objdump` xrefs/disassembly, and the locally built Rizin CLI without full-executable auto-analysis.

## Findings supported by the executable

### Place-version rollout flags exist, but do not identify the selected route

The binary contains these FFlag names:

| Name | File offset | Image VA | Registration storage |
|---|---:|---:|---:|
| `placesCreatePlaceVersionApiKeyTelemetryHundredthsPercent` | `0x8eb6238` | `0x148eb8238` | — |
| `placesCreatePlaceVersionApiKey` | `0x8eb6278` | `0x148eb8278` | `0x14d908188` |
| `placesCreatePlaceVersionUserAuthTelemetryHundredthsPercent` | `0x8eb6298` | `0x148eb8298` | — |
| `placesCreatePlaceVersionUserAuth` | `0x8eb62d8` | `0x148eb82d8` | `0x14d9081b0` |

The two name-to-storage entries are visible in the registration table at file offsets `0xc40ee50` and `0xc40ee68` (image VAs `0x14c411c50` and `0x14c411c68`). Their registration thunks at `0x145681b60` and `0x145681b80` call the generic flag-registration helper. These markers establish that API-key and user-auth rollout alternatives exist. They do **not** establish which flag was active, the request endpoint, or which branch a particular Studio process used.

### A cookie-authenticated *place creation* route is directly referenced

The executable contains `universes/v1/user/universes/{universeId}/places` at file offset `0x8eb64c8` (VA `0x148eb84c8`), referenced by code at `0x14568055f`. Nearby it contains `RBX::OpenApi::PlacesApi::placesCreatePlaceUserAuth` at `0x8eb64f8` (VA `0x148eb84f8`), referenced at `0x145681467`.

This is evidence for a user-authenticated **create-place** operation, not proof of the separate create-place-version/publish operation. It does confirm that Studio 0.741 contains a keyless user-auth path somewhere in the Places API implementation.

### Asset-version read/check routes are not the publish POST

The binary contains `/assets/user-auth/v1/assets/{assetId}/versions` at file offset `0xa6f68b0`. Code at `0x1474a7a64` builds that path with the `apis` host and the literal HTTP method `GET`; the nearby code substitutes `{assetId}`. This is a read/list request, not a version upload.

A second string, `assets/user-auth/v1/assets/{}/versions`, appears at file offset `0x86c7e48` beside `checkPlaceVersionRequest`, `PlaceVersionCheck`, and response/error strings for an `assetVersions` table. That association points to version checking, not the place-publish POST.

The executable also includes `/assets/user-auth/v1/assets/{assetId}:multipartUpload` and several `/ide/publish/Upload...` asset routes. Those strings and their direct xrefs land in generic asset-upload code; no call chain tying them to `StudioPublishService::publishPlace` was recovered. The binary contains legacy `/Data/Upload.ashx` strings as well, but their presence alone does not show that Studio uses that route for place publishing.

### Generic HTTP strings do not establish the publish request

`X-CSRF-TOKEN` occurs at file offset `0x87d8638`, with code references in generic HTTP/auth code. `Content-Type: application/octet-stream` occurs at `0x8b513d0` and `0x8bd5488`, with references in generic HTTP/upload code. These strings have not been linked to the place-publish request.

The printable-string scan did not find a `versionType` literal or the documented Place Versions route template. That absence is not conclusive by itself because Studio could assemble a URL dynamically. The `placesCreatePlaceVersionApiKey` / `UserAuth` FFlags are also not enough to infer the route, query, body, selected auth branch, or CSRF challenge flow.

## Current conclusion on exact Studio request

**Still unverified:** Studio 0.741's exact place-version publishing hostname/path, HTTP method, query parameters, request body/content type, API-key versus cookie branch, and CSRF-token acquisition/retry behavior.

The public Creator Hub reference is a valid candidate for this app's implementation:

```text
POST https://apis.roblox.com/universes/v1/{universeId}/places/{placeId}/versions?versionType=Published|Saved
```

Its publishing guide documents raw place-file bytes (`application/octet-stream`) and API-key use; the API reference lists Cookie authentication too. Those docs establish a public API capability, not that Studio 0.741 uses this exact request. The app's `publish_place_with_cookie` path is likewise an implementation of that documented candidate, not evidence of Studio's internal flow.

References:

- https://create.roblox.com/docs/cloud/reference/domains/apis
- https://create.roblox.com/docs/cloud/reference/features/places
- https://create.roblox.com/docs/cloud/guides/usage-place-publishing
- https://devforum.roblox.com/t/official-list-of-deprecated-web-endpoints/62889/62

## Team Create connected-stage and transport checkpoint

The separate transport findings remain in [`roblox-0741-studio-transport-selector.md`](roblox-0741-studio-transport-selector.md). Latest recorded run: both recovered RbxTransport/QUIC route variants sent seven local UDP datagrams, but received **zero** return datagrams and had no socket errors before the native 10-second handshake timeout. That is the observed blocker: no return traffic / incomplete QUIC handshake. It does not yet reach TLS/RPK verification, BaseClient early-auth, channel open, the peer's channel ACK, or connected state; those are later requirements, not the current failure explanation.

The static 0.741 selector is conditional. With the accepted runtime flags enabled and valid RbxTransport port/address/32-byte key inputs, it chooses RbxTransport; if the selector flag is off or required inputs are invalid, it chooses RakNet (subject to the recovered fallback flags). Join-config fields alone are not proof of the runtime branch. The Android app's `selectedTransport=RbxTransport` is evidence of the app's own selector, not Studio's runtime telemetry. No switch to legacy RakNet is justified by the current no-return-traffic result.

## Bounded `publishPlace` trace (2026-10-04)

A function-scoped `objdump` pass around the known telemetry xref now covers the body at `0x146c51e30..0x146c532f3` (visible prologue and return). At `0x146c52d30`, the code references the static string `StudioPublishService::publishPlace` at VA `0x1490b75e8` (file offset `0x90b55e8`) and copies it into a local string/context object. This is a static operation label, not an endpoint or proof that a publish request ran.

The immediate internal call sites after that label are `0x146c52d73 -> 0x1461d0050`, `0x146c52e2f -> 0x1461d2770`, `0x146c52e61 -> 0x1461d1c50`, and `0x146c52e69 -> 0x1461d11a0`. They lead into callback/context handling, but this pass did **not** resolve them as the HTTP request mediator and did not recover a hostname, URL path, method, body, or auth branch. A separate branch at `0x146c52f43` calls `0x146c507a0`; the existing trace ties that branch to `StudioPublishService::addNewPlace`, so it is the place-creation path, not evidence for place-version publishing.

**Still unresolved:** the exact place-version request and whether `placesCreatePlaceVersionApiKey` or `placesCreatePlaceVersionUserAuth` controls the Studio request. The flags, generic CSRF strings, and this operation label are not yet linked to a concrete HTTP call. The next bounded step is to resolve the callback/mediator targets and follow their selected request path; do not rerun broad Rizin `-A` auto-analysis. If the auth flags cannot be bound statically, a Studio HTTP trace or explicit runtime route/branch log is needed to close the endpoint and CSRF questions.

No tests or compilation were run for this static-analysis/documentation pass.
