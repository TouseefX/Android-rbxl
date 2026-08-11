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

## Bevy 3D Viewport (port of OpenRBLX's renderer)

This branch adds a GPU-accelerated 3D viewport rendered by the **Bevy engine**,
replacing the old CPU software rasterizer. It is a direct port of the
geometry/shape/decal/material logic from **OpenRBLX**
(`TornadoCookie/OpenRBLX`, a C/raylib Roblox-engine recreation) into this
Android Rust app, swapping raylib for Bevy. Bevy is now the **only** 3D
renderer in the app.

### What's included

| File | Purpose |
| --- | --- |
| `src/bevy_rbxl.rs` | Core renderer: `WeakDom` → Bevy meshes/materials. Reuses the app's proven shape tessellation, brick-colour table, `.mesh` parser and procedural texture generators. |
| `src/viewport3d_bevy.rs` | egui bridge (`BevyViewport3D`) that runs Bevy on a background thread, renders offscreen, and blits frames into the existing 3D Viewport tab. API-compatible with the old `Viewport3D`. |
| `renderer/` | **Standalone Bevy renderer crate** (desktop) — load any `.rbxl`/`.rbxmx`/`.rbxm` and render it in a normal Bevy window. This is the fastest way to *see* the port working. |
| `Cargo.toml` | Adds the Bevy dependency (the viewport's only renderer). |
| `app.rs` / `lib.rs` | Use `BevyViewport3D` as the sole viewport. |

### Coordinate handling

Roblox places are **left-handed**; Bevy is **right-handed**. Every world
vertex/normal is Z-flipped (`x,y,z → x,y,-z`) to match, and materials are
two-sided (`cull_mode: None`) so winding/backface issues never appear. This
makes the Bevy output geometrically identical to the old software rasterizer.

### Try it on desktop first (recommended)

```sh
# from the repo root
cargo run --release --manifest-path renderer/Cargo.toml -- path/to/place.rbxl
# copy an .rbxl next to the crate and just:
cargo run --release --manifest-path renderer/Cargo.toml
```

Controls: left-drag = orbit, scroll = zoom, WASD/arrows = pan, Q/E = up/down.
The crate is intentionally standalone (the app crate is a `cdylib` with an
`android_main` entry that won't build on a normal OS). It renders all part
shapes (Block/Ball/Cylinder/Wedge/CornerWedge/Truss) with correct colours,
position & rotation, per-face **decals**, and **MeshParts** (loaded from the
local `asset/` cache by id when present).

### Build the Android app

```sh
cargo apk2 build --lib
```

The 3D Viewport tab is rendered **only by the Bevy engine** (GPU, a port of
the OpenRBLX renderer). The old CPU software rasterizer has been removed from
the app — there is no fallback and no feature flag.

> **Verified compile status.** `bevy_rbxl.rs`, `viewport3d_bevy.rs` and the
> slimmed `viewport3d.rs` (shared math) were type-checked (`cargo check`)
> against **Bevy 0.17** on Linux with the exact feature set in `Cargo.toml`
> (no `x11`/`wayland`/`bevy_winit`, which also avoids a winit clash with
> eframe). They compile cleanly.
>
> **Still needs a device check.** Compiling ≠ running. Embedding Bevy inside
> eframe/egui and reading rendered frames back with Bevy's `gpu_readback` API
> must be exercised on a real Android device/GPU. The GitHub Actions workflow
> builds the single `rbxleditor.apk` (Bevy viewport) on demand.
>
> If you bump Bevy versions, the breaking spots are: `Mesh3d`/`MeshMaterial3d`,
> `Color::srgba`, `DirectionalLight.illuminance`, `AmbientLight.brightness`,
> `Image::new`, `insert_indices`, and the offscreen `bevy_render/gpu_readback`
> API (`Readback` + `ReadbackComplete` observer) in `viewport3d_bevy.rs`. Bevy
> is heavy; a `--release` build takes several minutes.
