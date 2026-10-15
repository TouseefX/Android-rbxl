# rbxl Editor

An Android-first editor for Roblox place and model files, written in Rust. The
main application uses Bevy for the 3D viewport and egui for the editor UI; a
Kotlin `GameActivity` host supplies Android file, clipboard, keyboard, and native
script-editor integration. It edits local files and selected Roblox web APIs;
it is not Roblox Studio or a full Roblox game runtime.

## What it does

- Open, edit, and save binary or XML places (`.rbxl`, `.rbxlx`); import and
  export models (`.rbxm`, `.rbxmx`).
- Browse the DataModel, edit properties, insert instances, and inspect a 3D
  viewport. Meshes and textures are fetched when needed and available.
- Preview common `ScreenGui`, `BillboardGui`, and `SurfaceGui` content. This is
  an editor preview, not Roblox's runtime UI engine.
- Edit Luau in the egui editor or the Android Sora editor, with syntax
  assistance and project-aware completion. Scripts can also be handed off to
  an external Android editor and synchronized back into the open place.
- Run Luau snippets and compatible plugins in the embedded sandbox. A separate
  companion plugin can send Command Bar snippets to a connected Studio session.
- Use selected Creator Store, Open Cloud, and cookie-authenticated Roblox API
  operations. Features that access Roblox require network access and may require
  an API key or a saved `.ROBLOSECURITY` cookie.

Team Create join and transport support is experimental. The app can negotiate a
join and run legacy RakNet connection probes or an RbxTransport QUIC/TLS path
(with an app=4 post-handshake flow when the required fields are available). It
does not replicate a live place into the editor or provide Studio's full
connected state. A successful handshake or observed flow traffic does not prove
native BaseClient connection or Team Create acceptance.

## Studio companion plugin

The optional Live Session WebSocket listens on loopback port `41742` by default.
Run the desktop editor and Studio on the same computer. If the editor is running
on Android, create a port forward on the host before connecting:

```sh
adb forward tcp:41742 tcp:41742
```

Keep the app's port setting and the plugin's `PORT` value in sync. The editor
currently uses the bridge to run Command Bar snippets in Studio; the plugin's
selection, undo/redo, and shallow place-summary handlers are not wired into the
editor UI, and the bridge does not synchronize the Studio DataModel.

## Project layout

| Path | Purpose |
| --- | --- |
| `Cargo.toml` | Main Rust app. Builds the Android `cdylib` and the desktop runner. |
| `src/` | Editor UI, file codecs, renderer, Luau tools, Roblox APIs, and transport code. |
| `kotlin/MainActivity.kt` | Android `GameActivity`, file/clipboard bridges, and Sora script editor. |
| `kotlin/LuauLanguage.kt` | Luau language support for the Sora editor. |
| `src/android_ime.rs` | Android GameTextInput-to-egui keyboard and clipboard bridge. |
| `manifest.yaml` | xbuild Android package, SDK, permission, and Maven dependency configuration. |
| `assets/companion_plugin.lua` | Optional Roblox Studio companion plugin for the Live Session bridge. |
| `renderer/` | Standalone renderer sandbox; separate from the main editor app. |
| `.github/workflows/main.yml` | Host checks, Android target checks, and APK packaging. |

## Run the desktop editor

```sh
cargo run --bin rbxl-editor-desktop -- /path/to/place.rbxl
```

The root crate's desktop runner uses the same Bevy/egui editor as Android. The
separate `renderer/` package is an independent rendering sandbox, not a
prerequisite for running the editor.

## Build an Android APK

The checked-in CI configuration uses Rust nightly for the app build, Android
NDK r27c, Java 17, Gradle 8.11.1, AGP 8.7.3, Kotlin Gradle plugin 2.1.21, and a
pinned xbuild revision. Install those tools (including the Android SDK build
tools) before building. On a Linux x86-64 host, set `ANDROID_NDK_HOME` to the
installed NDK directory and install the pinned xbuild revision. Native
Android dependency builds use API 26 (the app's `minSdk`); the generated Gradle
project targets SDK level 34 from the manifest:

```sh
rustup toolchain install stable --profile minimal
rustup toolchain install nightly --profile minimal
rustup target add aarch64-linux-android --toolchain nightly
# The pinned xbuild starts `cargo` by name and inherits this Rustup selection.
export RUSTUP_TOOLCHAIN=nightly

cargo +stable install --locked \
  --git https://github.com/rust-mobile/xbuild \
  --rev e67b501cbe0e9ca5436c223aa7ec0fe5e27544d1 xbuild

export ANDROID_SDK_ROOT="$HOME/Android/Sdk"
export ANDROID_HOME="$ANDROID_SDK_ROOT"
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/27.2.12479018"
export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"
export ANDROID_PLATFORM=android-26
export ANDROID_API_LEVEL=26
NDK_HOST="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"
mkdir -p "$HOME/.cache/x"
# If Android.ndk exists as a real directory, inspect it and move it aside first.
ln -sfnT "$NDK_HOST/sysroot" "$HOME/.cache/x/Android.ndk"
export PATH="$NDK_HOST/bin:$PATH"
```

For another host OS, use the matching NDK `prebuilt/<host>` directory. xbuild's
generated Gradle project needs the plugin versions used by CI. Create the
`~/.gradle/init.d` directory if needed, then save the following as
`~/.gradle/init.d/xbuild-overrides.gradle`:

```groovy
settingsEvaluated { settings ->
    settings.pluginManagement.resolutionStrategy.eachPlugin { details ->
        def id = details.requested.id.id
        if (id == 'com.android.application' || id == 'com.android.library') {
            details.useModule('com.android.tools.build:gradle:8.7.3')
        } else if (id == 'org.jetbrains.kotlin.android') {
            details.useModule('org.jetbrains.kotlin:kotlin-gradle-plugin:2.1.21')
        }
    }
}
```

Then check the local toolchain and build:

```sh
x doctor
x build --release --platform android --arch arm64 --format apk
```

The local APK is written to
`target/x/release/android/rbxl-editor.apk` and is unsigned. The command above
uses Rustls by default: that backend verifies the expected raw public key but
does not add the 0xFF00 extension and does not send SNI for the configured IP
address. `ossl-tls` switches to OpenSSL, registers the RbxTransport 0xFF00
ClientHello extension, and sets SNI to the configured IP literal; it requires
an Android OpenSSL build at version 3.5.0 or later. The current OpenSSL configuration disables
peer verification, so a successful handshake is not authenticated; do not use
that feature for trusted sessions until verification is re-enabled. The CI
workflow compiles OpenSSL 3.5.9, builds with `--features ossl-tls`, and signs its
test artifact with a per-run key; use your own persistent signing key for an
app you intend to distribute or update. To
install and run on a connected device, use
`x run --device adb:<device-id>`.

### Android activity version pairing

`android-activity` is pinned to `0.6.1` in `Cargo.toml`, while
`manifest.yaml` declares `androidx.games:games-activity:4.4.0`. Keep these
versions in sync when upgrading: Bevy's GameActivity glue and the Android AAR
must expose matching JNI entry points.

## Android text input

GameActivity provides the Android `InputConnection`, but Bevy/winit does not
currently translate all GameTextInput state into egui events. `src/android_ime.rs`
therefore polls GameTextInput directly: it forwards committed text, backspace,
newlines, and composing text to egui, and routes copy/paste through Android's
clipboard. `MainActivity` applies the keyboard's bottom inset to the content
root; the manifest uses `adjustNothing` because resizing the native surface is
not reliable on every device.

## CI and tests

The workflow runs host checks and selected regression tests, checks the Android
target without packaging an APK, and reports the full library test suite
separately. APK packaging runs on manual dispatch and on pushes to `arena/**`;
it is skipped for ordinary pushes to `master`.

## Third-party components

The Android native script editor uses the Sora editor AAR declared in
`manifest.yaml`, distributed under LGPL v2.1. The AAR is resolved at build time
and bundled into the APK; this repository does not vendor its source. Anyone
redistributing a built APK should review the upstream license and ensure the
distribution meets its requirements. Roblox Builder fonts and their license
notes are documented in `content/fonts/README.md`.
