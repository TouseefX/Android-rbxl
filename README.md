# rbxl Editor (Android, Rust)

A Roblox place (`.rbxl`) editor for Android, written in Rust: it parses/writes
place files with `rbx_binary`/`rbx_dom_weak`, renders the 3D viewport with
**Bevy**, and draws the editor UI with **egui** (via `bevy_egui`) on top of it.
Scripts can be edited inline or handed off to an external editor app
(QuickEdit, Acode, …) and synced back on return.

## Build system: xbuild + GameActivity

The APK is built with [`xbuild`](https://github.com/rust-mobile/xbuild)
(`x build`), **not** `cargo-apk2`.

* The app's Activity extends `com.google.androidgamesdk.GameActivity` from the
  `androidx.games:games-activity` AAR. GameActivity is what provides a real
  `InputConnection` (GameTextInput), so Gboard/Samsung keyboard input — and
  paste — work in egui text fields. `NativeActivity` cannot do that.
* `cargo-apk2` compiles Java by shelling out to `javac` with only `android.jar`
  on the classpath. It has no notion of Maven/AAR dependencies, so the
  GameActivity class was never on the classpath and the build failed with 37
  `cannot find symbol` errors for every inherited method.
* `xbuild` with `gradle: true` generates a real Gradle/AGP project, so AAR
  dependencies resolve from Google's Maven repo and the Activity compiles like
  in any native Android app.

### Layout

| Path | What it is |
| --- | --- |
| `manifest.yaml` | xbuild project config: package id, SDK levels, permissions, activity, Maven deps. Replaces the old `[package.metadata.android]` tables. |
| `kotlin/MainActivity.kt` | The `GameActivity` subclass + the JNI surface used by `src/jni_bridge.rs`. xbuild copies every file in `kotlin/` into the generated Gradle project (flat — no package sub-directories). |
| `src/` | The Rust app (`cdylib` `librbxl_editor.so` + an rlib for the desktop runner). |
| `.github/workflows/main.yml` | CI: cross-compiles, generates the Gradle project, builds, zipaligns and signs the APK. |

### Building an APK locally

```sh
cargo install --locked --git https://github.com/rust-mobile/xbuild xbuild
rustup target add aarch64-linux-android

# xbuild compiles against ~/.cache/x/Android.ndk. Its bundled sysroot is from
# 2022 and has no API-34 stubs, so point it at a real NDK (r27+) instead:
ln -sfn "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot" ~/.cache/x/Android.ndk
export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"

x doctor                       # checks clang/llvm, java, gradle, adb
x build --release --platform android --arch arm64 --format apk
# -> target/x/release/android/rbxl-editor.apk   (unsigned; see CI for signing)
```

Note that xbuild's Gradle template pins AGP 7.3.0 / Kotlin 1.7.20, which is too
old for `compileSdk 34` and current AndroidX artifacts. CI overrides those
plugin versions with a Gradle init script (see
`.github/workflows/main.yml`); do the same locally if the Gradle step fails.

`x run --device adb:<id>` builds, installs and streams logcat to a connected
phone.

### Desktop runner

```sh
cargo run --bin rbxl-editor-desktop -- /path/to/place.rbxl
```

### Version pinning

`android-activity` (the GameActivity glue Bevy links through its
`android-game-activity` feature) is pinned to `0.6.1` in `Cargo.toml`, and
`manifest.yaml` requests `androidx.games:games-activity:4.4.0` — the AAR
version that release expects. **Bump both together**, otherwise you get an
`UnsatisfiedLinkError` for `GameActivity.initializeNativeCode` at startup.

## Text input (soft keyboard, IME, paste)

GameActivity is necessary for text input but it is **not sufficient**: nothing
in the Bevy stack consumes GameTextInput.

* `winit` 0.30's Android backend only translates `InputEvent::KeyEvent` /
  `MotionEvent`; it ignores `MainEvent::TextInputEvent` and always sets
  `KeyEvent::text = None`.
* `Window::set_ime_allowed()` is an empty stub on Android, so nothing ever asks
  the system to raise the soft keyboard.

So a focused egui `TextEdit` would blink a caret and receive nothing.
`src/android_ime.rs` closes that gap by talking to `android-activity` directly,
bypassing winit:

* **End of each egui pass** — if `ctx.wants_keyboard_input()` just became true,
  configure the IME (`set_ime_editor_info`: multi-line, no autocorrect,
  `IME_FLAG_NO_FULLSCREEN`), seed the GameTextInput buffer and call
  `show_soft_input()`. When focus is lost, `hide_soft_input()`.
* **Start of each egui pass** — poll `AndroidApp::text_input_state()` and diff
  it against our mirror of the buffer. Inserted text becomes
  `egui::Event::Text` (newlines become `Key::Enter`), removed characters become
  `Key::Backspace`. Gboard's clipboard chip commits through the same
  `InputConnection`, so **paste works through this path**.
* egui's copy/cut (`OutputCommand::CopyText`) is forwarded to Android's
  `ClipboardManager` via the existing JNI bridge, and Ctrl+V on a hardware
  keyboard pushes `egui::Event::Paste` from the system clipboard.

The buffer keeps a small run of leading spaces as padding so a Backspace
pressed before anything has been typed still registers, and it is re-seeded
when that padding is consumed.

Debugging: `adb logcat -s rbxl_editor` shows `android_ime: keyboard shown` /
`hidden` on every focus change.

**Known limitation:** the activity is fullscreen with the system bars hidden,
so `windowSoftInputMode=adjustResize` cannot shrink the window — the keyboard
overlays the bottom of the screen instead of pushing the UI up. If a field ends
up underneath it, the fix is to read `AndroidApp::content_rect()` while the IME
is up and reserve that much space at the bottom of the egui layout.
