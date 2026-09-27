# Roblox — cookie-auth upload endpoints (the "internal" ones Studio/the site use)

The public Open Cloud APIs take `x-api-key`. The endpoints below instead
authenticate with your own account's `.ROBLOSECURITY` **cookie** + an
`X-CSRF-TOKEN` header — these are the same endpoints Roblox Studio and
roblox.com itself use. They are undocumented/supported-less, so they can
change or break at any time (they do — see the status notes per endpoint).

IMPORTANT: these give full access to whoever owns the cookie. Use a
dedicated burner account, never share the cookie, and respect rate limits.

---

## 0. How cookie auth works (required for ALL endpoints below)

Every mutating request needs:

```
Cookie: .ROBLOSECURITY=<_|WARNING... your cookie ...>
X-CSRF-TOKEN: <token>
```

**Getting the CSRF token** — the standard handshake (works on every
endpoint on the list):

1. Send the POST (or `POST https://auth.roblox.com/v2/logout`) *without*
   a token.
2. Roblox answers **403** and puts a fresh token in the response header
   `x-csrf-token` (lowercase).
3. Re-send the exact same request adding
   `X-CSRF-TOKEN: <that value>` (uppercase header name).
4. Cache the token; repeat the dance any time you get a 403 again.

 ([devforum walkthrough](https://devforum.roblox.com/t/trouble-with-login-api/896186), [SO explanations](https://stackoverflow.com/questions/78474825/roblox-web-api-authentication-failure-token-validation-failed-csrf-token-pro), [devforum thread showing the exact retry loop](https://devforum.roblox.com/t/uploading-a-rbxm-to-roblox/2951640))

Sanity check the cookie first:
`GET https://users.roblox.com/v1/users/authenticated`
→ `{"id":...,"name":...}` = cookie valid.
([auth examples](https://robloxapi.fandom.com/wiki/Authentication))

Gotchas:
- If the cookie "keeps dying," your account is refreshing sessions —
  some users disable that behavior in Roblox/creator settings, then use a
  freshly re-logged-in cookie. ([thread](https://devforum.roblox.com/t/uploading-a-rbxm-to-roblox/2951640))
- A sensible `User-Agent` matters on a few endpoints (noted inline).

---

## 1. Models / .rbxm / .rbxmx / places — the classic hidden endpoint  ✅

This is the one Rojo, RAssets, noblox.js etc. all use — "the hidden API endpoint":

```
POST https://data.roblox.com/Data/Upload.ashx
    ?json=1
    &assetid={0 = new asset, or existing assetId to overwrite}
    &type=Model              (Model; Place / Plugin etc. also accepted)
    &genreTypeId=1
    &name={urlencoded}
    &description={urlencoded}
    &ispublic={true|false}   (not copy-locked)
    &allowComments={true|false}
    &groupId={optional, upload to a group catalog}

Body:         raw bytes of the .rbxm / .rbxmx file
Content-Type: application/xml   (Roblox/WinInet UA is typical)
Returns:      an assetVersionId (plain) or {"id":...} JSON with json=1
```

 Recipe example: (RAssets TS wrapper [source](https://github.com/Stefanuk12/RAssets)) ([noblox source](https://noblox.js.org/lib_asset_uploadModel.js.html)) ([rojo issue confirming rojo uses this URL](https://github.com/rojo-rbx/rojo/issues/425)) ([matthewdean's API list](https://github.com/matthewdean/roblox-web-apis/blob/master/README.md))

**Status**: alive, but quirky — when you omit the CSRF token its error
page itself crashes and you get HTTP 500 instead of 403; still returns
the `x-csrf-token` header regardless, so the retry flow in §0 works.
([devforum incident report](https://devforum.roblox.com/t/dataroblox-upload-endpoint-appears-to-no-longer-function/2510735),
[2024 thread confirming it works once CSRF is handled right](https://devforum.roblox.com/t/uploading-a-rbxm-to-roblox/2951640))

Related dead sibling (was used for images): `data.roblox.com/data/upload.json` — only appears in old threads, treat as dead/verify yourself.

---

## 2. Any asset type — the cookie variant of the Open Cloud Assets API  ✅ (recommended modern path)

Rojo's current uploader uses the **`user-auth`** path of the official
Assets API — identical request shape to Open Cloud, but authenticated
with the cookie + CSRF instead of `x-api-key`:
([Rojo source constant](https://github.com/rojo-rbx/rojo/issues/1263) / [devforum thread quoting Rojo's source](https://devforum.roblox.com/t/using-publish-api-in-javascript/809032))

```
# CREATE
POST https://apis.roblox.com/assets/user-auth/v1/assets
# UPDATE (version control)
PATCH https://apis.roblox.com/assets/user-auth/v1/assets/{assetId}
# POLL the returned long-running operation
GET https://apis.roblox.com/assets/user-auth/v1/operations/{operationId}
```

Multipart form, exactly like Open Cloud:

```
request    = {"assetType":"Decal" | "Image" | "Model" | "Audio" |
              "Animation" | "Video" | "Mesh",
              "displayName":"...","description":"...",
              "creationContext":{"creator":{"userId":N}} }   # or "groupId":N
fileContent = (binary)
```

Formats/limits per type (from the Open Cloud docs — same pipeline):
Model .fbx/.gltf/.glb/.rbxm/.rbxmx; Image/Decal png/jpg/bmp/tga
(<8000×8000); Audio mp3/ogg/wav/flac ≤7 min (upload quotas: 100/month
ID-verified, 10 otherwise; **audio + video + image updates are NOT
supported by the API** — create-only); Video mp4/mov ≤5 min ≤4K
(20/day, 13+ & ID-verified); Animation .rbxm/.rbxmx with a
KeyframeSequence. ([open cloud assets docs](https://create.roblox.com/docs/cloud/guides/usage-assets))

Response to create/update: `{"path":"operations/{operationId}"}` — poll
§-3 URL until `{"done":true,"response":{"assetId":...}}`.

This is the best cookie path for **images/decals, audio, video, models**
today. Note that on the public docs only the API-key flavor is
mentioned; the `user-auth` variant is undocumented.

---

## 3. Animations — simple one-shot cookie endpoints  ✅

Exact URLs from noblox.js source:

```
# New animation
POST https://www.roblox.com/ide/publish/uploadnewanimation
    ?AllID=1&assetTypeName=Animation&genreTypeId=1
    &name={...}&description={...}&ispublic={t|f}
    &allowComments={t|f}&groupId={optional}

# Overwrite an existing animation
POST https://www.roblox.com/ide/publish/uploadexistinganimation
    ?assetID={assetId}&isGamesAsset=False

Body:         the .rbxm file containing the KeyframeSequence
Content-Type: application/xml
User-Agent:   RobloxStudio/WinInet RobloxApp/0.483.1.425021 (GlobalDist; RobloxDirectDownload)
Returns:      the new assetId as a raw number in the body (for the "new" flavor)
```

Then set metadata afterwards with §8 (configureItem).
([noblox uploadAnimation source](https://noblox.js.org/lib_asset_uploadModel.js.html) — same repo family; function verified verbatim)

---

## 4. Places (publish/save a .rbxl or .rbxlx)

- **Open Cloud** officially requires an **API key** — NOT cookie:
  `POST https://apis.roblox.com/universes/v1/{universeId}/places/{placeId}/versions?versionType=Published`
  ([docs](https://create.roblox.com/docs/cloud/guides/usage-place-publishing))
- **Cookie alternatives that historically worked**:
  - `https://placepublishing.roblox.com/v1/universes/{u}/places/{p}/versions`
    (the same route behind the old web gateway; cookie+CSRF used to work —
     now mostly redirects to the API-key flavor)
  - `Data/Upload.ashx?assetid={placeId}&type=Place` — older bots treat a
    place as an asset and push the .rbxl through §1. Rojo's `rojo upload`
    worked this way for years.
  So for places today the cookie path is **⚠️ legacy/intermittent** — if
  you must automate publishing, the Open Cloud API key is the reliable
  answer; cookie works for model/place-type asset **saves** in many
  pipelines still.

---

## 5. Game icons & media (screenshots)  ✅ (community-verified; see swagger `games.roblox.com/docs`)

```
POST https://games.roblox.com/v1/games/{universeId}/icon
    multipart: file=<png/jpg 512×512>

POST https://games.roblox.com/v1/games/media
    form: universeId={u}, mediaType={Image|Video}, mediaName={...}, file=<binary>
```

cookie + X-CSRF. Used by dashboard bots for icon/thumbnail automation.

---

## 6. Badges (incl. icon upload)  ✅ (swagger: badges.roblox.com/docs)

```
POST  https://badges.roblox.com/v1/universes/{universeId}/badges
      multipart: name, description, paymentSourceType={Free|Robux},
                 badgeIconFile=<png>
PATCH https://badges.roblox.com/v1/badges/{badgeId}
      multipart: name?, description?, badgeIconFile?
```

cookie + X-CSRF, costs Robux per creation when paid.

---

## 7. Game passes

- Create: old cookie flow was multipart to
  `https://develop.roblox.com/v1/assets?assetTypeId=34`
  (icon file + name/description) — **⚠️ legacy, frequently restricted**
  (`UserDoesNotHavePermissionToUpload` style errors). Old form-flavored
  endpoint `https://www.roblox.com/gamepass/sendrequest` (noblox's
  updateGamePass) is also legacy/dying.
- The **current** supported config/update flow (`game-passes/v1/...`)
  is Open Cloud **API-key only**. If creating via cookie fails, create
  the pass once in the dashboard and only automate what's cookie-capable.

---

## 8. Companion "configure" endpoints you'll need after uploading  ✅

```
PATCH https://develop.roblox.com/v1/assets/{assetId}
      JSON: {"name":"...","description":"...","enableComments":false,"sellForRobux":false}
PATCH https://develop.roblox.com/v1/assets/{assetId}/permissions
      (set copy/comment permissions)
itemconfiguration.roblox.com  (price / on-sale / off-sale, swagger: itemconfiguration.roblox.com/docs)
```
and for groups: pass `groupId` on §1/§3, or `"creator":{"groupId":N}` on §2.

---

## 9. Quick reference

| Upload target | Cookie endpoint | Status |
|---|---|---|
| Model / place-as-asset / any .rbxm(x) | `data.roblox.com/Data/Upload.ashx` | ✅ (CSRF quirk, 500-on-error-page) |
| Decal/Image/Audio/Video/Model/Animation/Mesh (modern) | `apis.roblox.com/assets/user-auth/v1/assets` (+ /operations) | ✅ recommended |
| Animation | `www.roblox.com/ide/publish/uploadnewanimation` / `uploadexistinganimation` | ✅ |
| Place publish | cookie paths legacy; Open Cloud key instead | ⚠️ |
| Game icon / media | `games.roblox.com/v1/games/{u}/icon`, `/v1/games/media` | ✅ community |
| Badge (+icon) | `badges.roblox.com/v1/...` | ✅ |
| Game pass | `develop.roblox.com/v1/assets?assetTypeId=34` | ⚠️ legacy |
| Decals (old publish API) | `publish.roblox.com/v1/assets/upload` | ⚠️ often 403-permission |
| Old audio upload page | `www.roblox.com/manage/uploadaudio` | ❌ dead — use §2 |
| Old images json upload | `data.roblox.com/data/upload.json` | ❌/⚠️ ancient |

## 10. Working recipes (curl)

```bash
COOKIE=".ROBLOSECURITY=<_|WARNING-your-cookie>"

# 1) get a CSRF token
TOKEN=$(curl -s -o /dev/null -D - -X POST \
  -H "Cookie: $COOKIE" https://auth.roblox.com/v2/logout \
  | grep -i '^x-csrf-token' | tr -d '\r' | cut -d' ' -f2)

# 2) overwrite a model through the hidden endpoint
curl -s -X POST \
  "https://data.roblox.com/Data/Upload.ashx?json=1&assetid=1818&type=Model" \
  -H "Cookie: $COOKIE" -H "X-CSRF-TOKEN: $TOKEN" \
  -H "Content-Type: application/xml" -H "User-Agent: Roblox/WinInet" \
  --data-binary @model.rbxm

# 3) upload a decal through the user-auth Assets API
curl -s -X POST https://apis.roblox.com/assets/user-auth/v1/assets \
  -H "Cookie: $COOKIE" -H "X-CSRF-TOKEN: $TOKEN" \
  -F 'request={"assetType":"Decal","displayName":"my decal","description":"via cookie","creationContext":{"creator":{"userId":123}}}' \
  -F 'fileContent=@decal.png;type=image/png'
# → {"path":"operations/abc..."} then:
curl -s "https://apis.roblox.com/assets/user-auth/v1/operations/abc..." \
  -H "Cookie: $COOKIE" -H "X-CSRF-TOKEN: $TOKEN"

# 4) upload an animation
curl -s -X POST \
  "https://www.roblox.com/ide/publish/uploadnewanimation?AllID=1&assetTypeName=Animation&genreTypeId=1&name=myanim&description=&ispublic=False&allowComments=True&groupId=" \
  -H "Cookie: $COOKIE" -H "X-CSRF-TOKEN: $TOKEN" \
  -H "Content-Type: application/xml" \
  -H "User-Agent: RobloxStudio/WinInet RobloxApp/0.483.1.425021 (GlobalDist; RobloxDirectDownload)" \
  --data-binary @anim.rbxm   # → plain numeric assetId
```

## 11. Hard rules / safety

- None of this is an **officially supported** surface. Roblox can and
  does break these (the 2023 Upload.ashx error-page incident is a good
  example). Build the retry-on-403 flow so your tool self-heals.
- 429s = back off. Hammering upload endpoints can get the account
  rate-limited or flagged.
- Never share `.ROBLOSECURITY`; anyone with it IS you. Everything here
  is meant for YOUR account.
- Where a cookie path has an Open Cloud equivalent with first-class
  support (place publishing, game-pass config), prefer the official one
  for anything production-critical.
