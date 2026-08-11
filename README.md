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

## Bevy 3D Viewport (single Bevy app + bevy_egui)

The whole editor is now **one Bevy app** with a GPU-accelerated 3D viewport, a
direct port of the geometry/shape/decal/material logic from **OpenRBLX**
(`TornadoCookie/OpenRBLX`, a C/raylib Roblox-engine recreation).

### Why this architecture

The previous attempts failed because of a fundamental Android constraint:

- Embedding Bevy *inside* an **eframe/egui** window failed — Bevy can't get its
  own GPU context inside a window eframe already owns.
- A **two-app** split (editor + separate Bevy viewer) added install/launch
  friction.

The fix: **Bevy owns the whole app/window** (so it gets a GPU context and
renders the 3D viewport natively on Android), and the **egui editor UI runs
inside Bevy** via the `bevy_egui` crate, drawn over the 3D scene. One app.

```
┌────────────────────── rbxleditor.apk (ONE Bevy app) ──────────────────────┐
│  bevy_egui editor UI (toolbar · tabs · explorer · properties · scripts)    │
│  ─────────────── 3D viewport rendered natively by Bevy (GPU) ─────────────  │
└────────────────────────────────────────────────────────────────────────────┘
```

### What's where

| File | Purpose |
| --- | --- |
| `src/bevy_render.rs` | The ported OpenRBLX renderer: `WeakDom` → Bevy meshes (Block/Ball/Cylinder/Wedge/CornerWedge/Truss, colours, transparency, MeshParts from local assets) + orbit camera. |
| `src/app.rs` | `EditorApp` (a Bevy `Resource`) with the full egui editor UI. `draw_editor(ctx, orbit)` lays out the panels; the 3D tab is a transparent drag area that steers the Bevy camera. |
| `src/lib.rs` | Bevy app setup (`DefaultPlugins` + `EguiPlugin` + systems) and `#[bevy_main]` Android entry. |
| `src/main_desktop.rs` | Desktop runner to validate the whole editor on a normal OS. |
| `renderer/` | Minimal standalone Bevy renderer (kept for quick headless checks). |

`bevy_egui 0.38` + `bevy 0.17` (egui 0.33). `egui` is pinned to the same
version bevy_egui uses so the editor's egui code resolves to identical types.

### Coordinate handling

Roblox places are **left-handed**; Bevy is **right-handed**. Every world
vertex/normal is Z-flipped (`x,y,z → x,y,-z`) to match, and materials are
two-sided (`cull_mode: None`) so winding/backface issues never appear.

### Validate on desktop

```sh
cargo run --bin rbxl-editor-desktop -- path/to/place.rbxl
```

Opens the full editor (Bezy 3D + egui panels) in a desktop window.

### Build & install (GitHub Actions — you have no PC)

Run the **Build Android APK** workflow. It produces **one** file:
`rbxleditor.apk` (the single Bevy app). Install it normally.

1. Install `rbxleditor.apk`.
2. Grant it "All files access" (Settings → Apps → Special access → All files access) if you want to read/write places outside the SAF picker.
3. Open the app, tap **📂 Open .rbxl** (uses Android's document picker), choose a `.rbxl`/`.rbxmx`.
4. The **🌍 3D Viewport** tab renders the place live with the Bevy engine (drag to orbit, scroll/pinch to zoom).

### Build locally

```sh
cargo apk2 build --lib
```

> **Compile status.** The full editor (Bevy 0.17 + bevy_egui 0.38 + egui 0.33)
> type-checks on desktop via `cargo check --bin rbxl-editor-desktop` (passes).
> The Android *behavior* (NativeActivity + on-device GPU) still needs a device
> test — I can't run an Android GPU here. If it opens but the viewport is
> blank, check logcat (`adb logcat`) for Bevy/render errors. If you bump Bevy
> versions, watch: `Mesh3d`/`MeshMaterial3d`, `Color::srgba`, `Image::new`,
> `insert_indices`, `AmbientLight`, the `bevy_egui` pairing, and the android
> entry (`#[bevy_main]`). Bevy is heavy; a `--release` build takes several
> minutes.
