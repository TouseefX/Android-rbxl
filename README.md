# rbxl Editor (Android, Rust)

A starter scaffold for editing scripts inside a `.rbxl` place file on
Android: parses/writes the place file with `rbx_binary`/`rbx_dom_weak`,
browses it with an Explorer-style tree in `egui`, and edits script `Source`
either inline (syntax-highlighted via `egui_code_editor`) or by handing the
script off to an external text editor app (QuickEdit etc.) and reading the
result back when you return to the app.

## Status — please read before building

This was assembled from documented crate/tool APIs (`rbx_dom_weak`,
`rbx_binary`, `eframe`'s Android backend, `cargo-apk2`'s manifest schema,
Android's SAF intents) but **has not been compiled or run**. Treat it as a
correct-by-construction scaffold, not a tested build. Likely first-build
friction:

- Exact crate versions (`eframe`/`egui`/`egui_code_editor`/`android-activity`)
  may need bumping to whatever's current when you build — version-pin
  mismatches between `eframe` and `android-activity` are the most common
  cause of Android-target build failures in this ecosystem.
- JNI method signature strings (`"(JLjava/lang/String;Ljava/lang/String;)V"`
  etc. in `jni_bridge.rs`) must match `MainActivity.java` exactly — a typo
  there fails silently or panics at the call site rather than at compile time.
- `System.loadLibrary("rbxl_editor")` must match your actual `cdylib` name;
  check `CARGO_APK2_ARTIFACT` (an env var `cargo-apk2` sets) if unsure.

Build it, see what the compiler/linker actually says, and treat this as the
first draft of a debugging session rather than a finished app.

## Build

```
cargo install cargo-apk2   # once
cargo apk2 build --release
cargo apk2 run             # build + install + launch on an attached adb device
```

## Layout

```
Cargo.toml                                    cargo-apk2 manifest config lives here
src/lib.rs                                    android_main entry point
src/app.rs                                    egui app: explorer + editor + toolbar
src/explorer.rs                               recursive instance-tree widget
src/lua_syntax.rs                             Luau keyword set for egui_code_editor
src/rbxl.rs                                   rbx_binary/rbx_dom_weak load/edit/save
src/jni_bridge.rs                             Rust <-> Java glue (file I/O, external edit)
java/.../MainActivity.java                    SAF file picker + external-edit round trip
```

## How "Edit externally" works

1. Rust calls `MainActivity.editExternally(id, name, source)`.
2. Java creates a **new SAF document** via `ACTION_CREATE_DOCUMENT` (not a
   raw file) — this matters because only a real SAF/DocumentsProvider URI
   can be granted to another app with `FLAG_GRANT_READ/WRITE_URI_PERMISSION`;
   a plain `file://` URI would throw `FileUriExposedException` on modern
   Android, and a self-written file provider would need its own manifest
   `<provider>` entry, which isn't in `cargo-apk2`'s documented manifest
   schema.
3. Java writes the script's current source into that document, then opens
   it with `ACTION_VIEW` + `Intent.createChooser(...)` so the user can pick
   QuickEdit or any installed text editor.
4. There's no result callback for that kind of intent, so `onResume()` is
   where we re-read the temp document and push the (possibly edited) text
   back into Rust via `nativeOnExternalEditReturned`.
5. `app.rs` matches the returned text to the right script via a small
   `id -> Ref` map (`pending_external_edits`), since a `rbx_dom_weak::Ref`
   has no meaningful representation on the Java side.
---

## Bevy 3D Viewer (port of OpenRBLX's renderer)

This adds a GPU-accelerated 3D renderer powered by the **Bevy engine**, a direct
port of the geometry/shape/decal/material logic from **OpenRBLX**
(`TornadoCookie/OpenRBLX`, a C/raylib Roblox-engine recreation).

### Architecture (two apps)

Trying to embed Bevy *inside* the egui editor fails on Android: Bevy can't get
its own GPU context inside eframe's window. So the renderer is a **separate
native Bevy app** that opens full-screen, which is the supported way Bevy runs
on Android (a `NativeActivity`, `android-native-activity` feature, compatible
with `cargo-apk2`).

```
┌─────────────── rbxleditor.apk ───────────────┐   ┌──────── rbxlviewer.apk ────────┐
│  egui editor (script/property editing)        │   │   Bevy GPU renderer            │
│   "🚀 View in Bevy" button ── writes ────────┼──▶│   full-screen orbit camera      │
│   current.rbxl to shared storage, then        │   │   (OpenRBLX-style rendering)   │
│   launches rbxlviewer                         │   │                                │
└───────────────────────────────────────────────┘   └────────────────────────────────┘
```

| File | Purpose |
| --- | --- |
| `viewer/` | **Bevy GPU viewer app** (Android, `rbxlviewer.apk`). Owns its own window and renders the place full-screen with orbit controls. The real deliverable. |
| `viewer/src/render.rs` | The ported OpenRBLX renderer: `WeakDom` → Bevy meshes (Block/Ball/Cylinder/Wedge/CornerWedge/Truss, colours, transparency, per-face decals, MeshParts from local assets). |
| `renderer/` | **Desktop** version of the same renderer, for validation without a phone. |
| `src/app.rs` / `src/jni_bridge.rs` / `MainActivity.java` | Editor's "🚀 View in Bevy": exports the current place to `/storage/emulated/0/rbxlviewer/current.rbxl` and launches `rbxlviewer`. |

The old CPU software rasterizer and the failed embedded-Bevy experiment are
removed from the editor.

### Coordinate handling

Roblox places are **left-handed**; Bevy is **right-handed**. Every world
vertex/normal is Z-flipped (`x,y,z → x,y,-z`) to match, and materials are
two-sided (`cull_mode: None`) so winding/backface issues never appear.

### Validate on desktop (recommended)

```sh
cargo run --release --manifest-path renderer/Cargo.toml -- path/to/place.rbxl
```

Controls: left-drag = orbit, scroll = zoom, WASD/arrows = pan, Q/E = up/down.

### Build & install (GitHub Actions — you have no PC)

Run the **Build Android APK** workflow. It produces **three** downloadable
files:

- `rbxleditor.apk` — the egui editor.
- `rbxviewer.apk` — the Bevy GPU viewer.
- **`rbxleditor-all.xapk`** — **both apps bundled into ONE file** (see below).

#### Option 1 (single file, easiest): install the XAPK
`rbxleditor-all.xapk` contains both APKs plus a manifest and icon. Install it
with a free **XAPK installer** app from the Play Store (e.g. *APKPure*,
*APKcombo*, or *XAPK Installer*):
1. Install an XAPK installer app.
2. Download `rbxleditor-all.xapk` and open it with that installer.
3. It installs both `rbxl Editor` and `rbxl Viewer`.

> **Caveat:** because these are two *separate* apps, the XAPK bundles them as
> two install targets — it is **not** a Google-Play-style split APK of one app.
> Most XAPK installers handle this fine. If your installer refuses, fall back
> to Option 2.

#### Option 2 (manual): install the two APKs separately
1. Install `rbxleditor.apk` **and** `rbxviewer.apk`.
2. Grant **both** apps "All files access" (Settings → Apps → Special access → All files access) — they need it to read/write `/storage/emulated/0/rbxlviewer/current.rbxl`.
3. Open a `.rbxl`/`.rbxmx` in the editor, tap **🚀 View in Bevy**. The place is saved and `rbxlviewer` opens it full-screen in 3D.

### Build locally

```sh
# editor
cargo apk2 build --lib
# Bevy viewer
cd viewer && cargo apk2 build --lib
```

> **Compile status.** The Bevy viewer (`viewer/`) and desktop renderer
> (`renderer/`) type-check against **Bevy 0.17** (`cargo check` passes). The
> Android *behavior* (NativeActivity + full-screen rendering on a real device)
> still needs a device test — I can't run an Android GPU here. If the viewer
> opens but shows nothing, check logcat (`adb logcat`) for the Bevy/render
> error. If you bump Bevy versions, watch: `Mesh3d`/`MeshMaterial3d`,
> `Color::srgba`, `Image::new`, `insert_indices`, `AmbientLight`, and the
> android entry (`#[bevy_main]`). Bevy is heavy; a `--release` build takes
> several minutes.
