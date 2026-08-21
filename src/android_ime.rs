//! Android soft-keyboard / IME bridge for egui (GameActivity + GameTextInput).
//!
//! # Why this file exists
//!
//! Switching the app to GameActivity was necessary but *not sufficient* for
//! text input. GameActivity gives the app a real `InputConnection` via
//! GameTextInput, but nothing in the Bevy stack consumes it:
//!
//! * `winit` 0.30's Android backend (which `bevy_winit` drives) only translates
//!   `InputEvent::KeyEvent` / `MotionEvent`. It never looks at
//!   `MainEvent::TextInputEvent`, and it builds its `KeyEvent`s with
//!   `text: None`.
//! * `Window::set_ime_allowed()` is an empty stub on Android, so nothing ever
//!   asks the system to show the soft keyboard.
//!
//! Net effect: tapping an egui `TextEdit` focuses it (caret blinks) but no
//! keyboard appears and no characters — or pasted text — ever reach egui.
//! That's exactly the symptom we have.
//!
//! # How this works
//!
//! `android-activity` exposes the GameTextInput state directly
//! (`AndroidApp::text_input_state()`), independent of the winit event loop, so
//! we can bypass winit entirely:
//!
//! 1. After each egui pass we ask egui whether it wants keyboard input
//!    ([`egui::Context::wants_keyboard_input`]). On a false -> true edge we
//!    configure the IME, seed the GameTextInput buffer and call
//!    `show_soft_input()`; on true -> false we hide the keyboard.
//! 2. At the start of every egui pass we poll the GameTextInput buffer and
//!    diff it against our mirror of it. Insertions become `egui::Event::Text`
//!    (or `Key::Enter`), deletions become `Key::Backspace`. Because Gboard's
//!    clipboard chip and long-press "Paste" both commit text through the same
//!    `InputConnection`, pasting works through this path too.
//! 3. egui's copy/cut requests (`OutputCommand::CopyText`) are forwarded to the
//!    Android clipboard through the existing JNI bridge.
//!
//! The buffer keeps a small run of leading padding ([`PAD`]) so that a
//! Backspace pressed before the user has typed anything still shows up as a
//! change we can forward; the buffer is re-seeded whenever that padding is
//! eaten or the buffer grows too large.

#[cfg(target_os = "android")]
mod imp {
    use bevy::android::android_activity::input::{
        ImeOptions, InputType, TextInputAction, TextInputState, TextSpan,
    };
    use bevy::android::android_activity::AndroidApp;
    use bevy::android::ANDROID_APP;
    use bevy_egui::egui;
    use std::sync::Mutex;

    /// Leading padding kept in the IME buffer so a Backspace typed before any
    /// other input still produces a buffer change we can detect. Spaces are
    /// used because every IME handles them and they never trigger autocorrect.
    const PAD: &str = "        ";

    /// Re-seed the buffer once it grows past this, so a long editing session
    /// doesn't hand the IME an ever-growing string.
    const MAX_BUFFER: usize = 2048;

    struct State {
        /// egui currently has keyboard focus.
        active: bool,
        /// Our copy of what GameTextInput last reported.
        mirror: String,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        active: false,
        mirror: String::new(),
    });

    fn app() -> Option<&'static AndroidApp> {
        ANDROID_APP.get()
    }

    /// Replace the IME buffer with just the padding, caret at the end.
    fn reseed(app: &AndroidApp, state: &mut State) {
        state.mirror = PAD.to_owned();
        let end = state.mirror.len();
        app.set_text_input_state(TextInputState {
            text: state.mirror.clone(),
            selection: TextSpan { start: end, end },
            compose_region: None,
        });
    }

    /// Number of trailing chars deleted and the run of chars inserted between
    /// `old` and `new`, comparing by common prefix / common suffix.
    fn diff(old: &str, new: &str) -> (usize, String) {
        let o: Vec<char> = old.chars().collect();
        let n: Vec<char> = new.chars().collect();

        let mut prefix = 0;
        while prefix < o.len() && prefix < n.len() && o[prefix] == n[prefix] {
            prefix += 1;
        }

        let mut suffix = 0;
        while suffix < o.len() - prefix
            && suffix < n.len() - prefix
            && o[o.len() - 1 - suffix] == n[n.len() - 1 - suffix]
        {
            suffix += 1;
        }

        let deleted = o.len() - prefix - suffix;
        let inserted: String = n[prefix..n.len() - suffix].iter().collect();
        (deleted, inserted)
    }

    fn key_event(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Poll GameTextInput and inject anything new into egui.
    ///
    /// MUST run at the start of the egui pass, before any widget is built, so
    /// the events are seen by the focused `TextEdit` in the same frame.
    pub fn begin_frame(ctx: &egui::Context) {
        let Some(app) = app() else { return };

        // Ctrl+V from a hardware/bluetooth keyboard: egui only pastes when it
        // is handed an `Event::Paste`, and nothing on Android produces one.
        let ctrl_v = ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::V,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.command
                )
            })
        });
        if ctrl_v {
            paste_from_clipboard(ctx);
        }

        let mut state = STATE.lock().unwrap();
        if !state.active {
            return;
        }

        let current = app.text_input_state();
        if current.text == state.mirror {
            return;
        }

        let (deleted, inserted) = diff(&state.mirror, &current.text);
        state.mirror = current.text;

        ctx.input_mut(|input| {
            for _ in 0..deleted {
                input.events.push(key_event(egui::Key::Backspace, true));
                input.events.push(key_event(egui::Key::Backspace, false));
            }

            // egui wants Enter as a key press, never as Text("\n").
            let mut first = true;
            for line in inserted.split('\n') {
                if !first {
                    input.events.push(key_event(egui::Key::Enter, true));
                    input.events.push(key_event(egui::Key::Enter, false));
                }
                first = false;
                if !line.is_empty() {
                    input.events.push(egui::Event::Text(line.to_owned()));
                }
            }
        });

        if !state.mirror.starts_with(PAD) || state.mirror.len() > MAX_BUFFER {
            reseed(app, &mut state);
        }
    }

    /// Show/hide the soft keyboard to match egui's focus, and forward egui's
    /// clipboard writes to Android. MUST run at the end of the egui pass.
    pub fn end_frame(ctx: &egui::Context) {
        let Some(app) = app() else { return };

        let wants_keyboard = ctx.wants_keyboard_input();
        let mut state = STATE.lock().unwrap();

        if wants_keyboard && !state.active {
            // Multi-line + no autocorrect: the editor is used for Luau source,
            // where suggestions/autocapitalisation do more harm than good.
            // NO_FULLSCREEN keeps the IME from covering the app with its own
            // "extracted text" editor in landscape.
            app.set_ime_editor_info(
                InputType::TYPE_CLASS_TEXT
                    | InputType::TYPE_TEXT_FLAG_MULTI_LINE
                    | InputType::TYPE_TEXT_FLAG_NO_SUGGESTIONS,
                TextInputAction::None,
                ImeOptions::IME_FLAG_NO_FULLSCREEN,
            );
            state.active = true;
            reseed(app, &mut state);
            app.show_soft_input(true);
            log::info!("android_ime: keyboard shown");
        } else if !wants_keyboard && state.active {
            state.active = false;
            app.hide_soft_input(false);
            log::info!("android_ime: keyboard hidden");
        }

        // egui asked for something to be copied -> put it on the Android
        // clipboard (bevy_egui's own clipboard support is compiled out).
        ctx.output_mut(|out| {
            out.commands.retain(|command| match command {
                egui::OutputCommand::CopyText(text) => {
                    if !text.is_empty() {
                        crate::jni_bridge::trigger_copy_to_clipboard(text);
                    }
                    false
                }
                _ => true,
            });
        });
    }

    /// Push the current Android clipboard contents into egui as a paste event.
    #[allow(dead_code)]
    /// Wire this to a "Paste" button for users whose keyboard has no clipboard
    /// chip.
    pub fn paste_from_clipboard(ctx: &egui::Context) {
        let text = crate::jni_bridge::get_clipboard_text();
        if !text.is_empty() {
            ctx.input_mut(|input| input.events.push(egui::Event::Paste(text)));
        }
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    use bevy_egui::egui;

    pub fn begin_frame(_ctx: &egui::Context) {}
    pub fn end_frame(_ctx: &egui::Context) {}
    #[allow(dead_code)]
    pub fn paste_from_clipboard(_ctx: &egui::Context) {}
}

pub use imp::{begin_frame, end_frame, paste_from_clipboard};
