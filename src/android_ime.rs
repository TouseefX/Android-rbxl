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
        /// Last fully committed GameTextInput contents. Composition candidates
        /// are deliberately excluded so Samsung Keyboard can replace them.
        mirror: String,
        /// Set after WindowInsets confirms the keyboard appeared. Once this is
        /// true, a later invisible inset means the user explicitly dismissed it.
        seen_visible: bool,
        /// Whether the keyboard currently owns a composing region.
        composing: bool,
        /// Last candidate sent to egui, used to suppress duplicate preedits.
        last_preedit: String,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        active: false,
        mirror: String::new(),
        seen_visible: false,
        composing: false,
        last_preedit: String::new(),
    });

    fn app() -> Option<&'static AndroidApp> {
        ANDROID_APP.get()
    }

    /// Replace the IME buffer with just the padding, caret at the end.
    fn reseed(app: &AndroidApp, state: &mut State) {
        state.mirror = PAD.to_owned();
        state.composing = false;
        state.last_preedit.clear();
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

        // Samsung Keyboard (especially Korean/handwriting/predictive modes)
        // repeatedly replaces a composing range. Treating every replacement as
        // Backspace + Text corrupts the egui buffer, so forward it through
        // egui's native preedit/commit protocol instead.
        if let Some(region) = current.compose_region.as_ref() {
            let candidate = current.text.get(region.start..region.end)
                .filter(|_| current.text.is_char_boundary(region.start)
                    && current.text.is_char_boundary(region.end))
                .map(str::to_owned)
                .unwrap_or_else(|| diff(&state.mirror, &current.text).1);
            if !state.composing || candidate != state.last_preedit {
                ctx.input_mut(|input| {
                    input.events.push(egui::Event::Ime(egui::ImeEvent::Preedit(
                        candidate.clone(),
                    )));
                });
                state.composing = true;
                state.last_preedit = candidate;
            }
            return;
        }

        if current.text == state.mirror && !state.composing {
            return;
        }

        let (deleted, inserted) = diff(&state.mirror, &current.text);
        // Samsung Keyboard can commit CRLF even though egui uses LF internally.
        let inserted = inserted.replace("\r\n", "\n").replace('\r', "\n");
        let finishing_composition = state.composing;
        state.mirror = current.text;
        state.composing = false;
        state.last_preedit.clear();

        ctx.input_mut(|input| {
            for _ in 0..deleted {
                input.events.push(key_event(egui::Key::Backspace, true));
                input.events.push(key_event(egui::Key::Backspace, false));
            }

            if finishing_composition {
                input.events.push(egui::Event::Ime(egui::ImeEvent::Preedit(String::new())));
                if !inserted.is_empty() {
                    input.events.push(egui::Event::Ime(egui::ImeEvent::Commit(inserted)));
                }
            } else {
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

        // Android Back/the keyboard's down-arrow hides the IME without changing
        // egui's widget focus. Honor that action instead of immediately showing
        // it again and leaving the editor in a phantom editing state.
        let system_visible = crate::jni_bridge::is_ime_visible();
        if state.active && system_visible {
            state.seen_visible = true;
        }
        if state.active && state.seen_visible && !system_visible {
            if let Some(focused) = ctx.memory(|memory| memory.focused()) {
                ctx.memory_mut(|memory| memory.surrender_focus(focused));
            }
        }
        let wants_keyboard = ctx.wants_keyboard_input();

        if wants_keyboard && !state.active {
            // This is a source-code field, so disable Samsung/Gboard word
            // correction. Luau suggestions are rendered by the editor itself;
            // keyboard composition can otherwise show blue replacement text
            // and duplicate identifiers around punctuation.
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
            state.seen_visible = false;
            reseed(app, &mut state);
            ctx.input_mut(|input| {
                input.events.push(egui::Event::Ime(egui::ImeEvent::Enabled));
            });
            app.show_soft_input(true);
            log::info!("android_ime: keyboard shown (Samsung composition enabled)");
        } else if !wants_keyboard && state.active {
            if state.composing {
                ctx.input_mut(|input| {
                    input.events.push(egui::Event::Ime(egui::ImeEvent::Preedit(String::new())));
                });
            }
            ctx.input_mut(|input| {
                input.events.push(egui::Event::Ime(egui::ImeEvent::Disabled));
            });
            state.active = false;
            state.seen_visible = false;
            state.composing = false;
            state.last_preedit.clear();
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
