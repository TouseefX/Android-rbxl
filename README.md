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
