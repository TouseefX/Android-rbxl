use jni::objects::{GlobalRef, JByteArray, JClass, JString, JValue};
use jni::JNIEnv;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

pub enum FileEvent {
    Opened { uri: String, data: Vec<u8> },
    /// A local .rbxm/.rbxmx model file was picked for INSERTION into the
    /// currently-open place (as opposed to Opened, which replaces the whole
    /// place). Mirrors the Creator Store download path but for local files.
    ModelOpened { uri: String, data: Vec<u8> },
    /// A place file downloaded from Roblox (by place ID) is ready to open.
    PlaceBytes { uri: String, data: Vec<u8> },
    /// Downloading a place from Roblox failed.
    PlaceError { uri: String, error: String },
    /// A cookie-based publish finished.
    PublishResult { uri: String, result: Result<(), String> },
    /// Group universes finished loading (group_id, list, thumbnails id->url).
    GroupUniverses { group_id: u64, universes: Vec<crate::roblox_api::GroupUniverse>, thumbs: std::collections::HashMap<u64, String> },
    /// Places under a universe finished (universe_id, list of (place_id, name)).
    UniversePlaces { universe_id: u64, places: Vec<(u64, String)> },
    /// A browse/network error happened.
    BrowseError { message: String },
    OpenCancelled,
    Created { uri: String },
    SaveComplete(bool),
    /// Text came back from an external editor (QuickEdit, Acode etc.) for the
    /// script identified by `script_id` (see EditorApp::next_external_id).
    ExternalEditReturned { script_id: u64, text: String },
}

static FILE_EVENTS: OnceLock<(mpsc::Sender<FileEvent>, Mutex<mpsc::Receiver<FileEvent>>)> =
    OnceLock::new();

static MAIN_ACTIVITY_CLASS: OnceLock<GlobalRef> = OnceLock::new();

/// When true, the next ModelOpened event from the SAF picker should be treated
/// as a plugin install rather than a "insert into place" action. Toggled by
/// `trigger_open_model_for_plugin()` and consumed by the app.
static NEXT_IS_PLUGIN: OnceLock<Mutex<bool>> = OnceLock::new();

fn next_is_plugin_flag() -> &'static Mutex<bool> {
    NEXT_IS_PLUGIN.get_or_init(|| Mutex::new(false))
}

/// Background-thread channel for completed plugin downloads. Plugin bytes are
/// large (hundreds of KB) and we don't want them mixed with the UI event
/// stream, so they get their own queue and the app polls it.
static PLUGIN_BYTES: OnceLock<(mpsc::Sender<PluginInstall>, Mutex<mpsc::Receiver<PluginInstall>>)> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub struct PluginInstall {
    pub name_hint: String,
    pub result: Result<Vec<u8>, String>,
}

pub fn channel() -> &'static (mpsc::Sender<FileEvent>, Mutex<mpsc::Receiver<FileEvent>>) {
    FILE_EVENTS.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        (tx, Mutex::new(rx))
    })
}

/// Drains every pending event without blocking. Call once per frame.
pub fn try_recv_all() -> Vec<FileEvent> {
    let (_, rx) = channel();
    let rx = rx.lock().unwrap();
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        out.push(ev);
    }
    out
}

fn plugin_channel() -> &'static (mpsc::Sender<PluginInstall>, Mutex<mpsc::Receiver<PluginInstall>>) {
    PLUGIN_BYTES.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        (tx, Mutex::new(rx))
    })
}

/// Drain any plugin bytes produced by background download threads.
pub fn try_recv_plugins() -> Vec<PluginInstall> {
    let (_, rx) = plugin_channel();
    let rx = rx.lock().unwrap();
    let mut out = Vec::new();
    while let Ok(p) = rx.try_recv() {
        out.push(p);
    }
    out
}

/// Background threads call this when a place finishes downloading
/// from Roblox, delivered on the event thread as a `PlaceBytes` event.
pub fn queue_open_place_bytes(uri: String, data: Vec<u8>) {
    let (tx, _) = channel();
    let _ = tx.send(FileEvent::PlaceBytes { uri, data });
}

pub fn queue_open_place_error(uri: String, error: String) {
    let (tx, _) = channel();
    let _ = tx.send(FileEvent::PlaceError { uri, error });
}

/// Background thread reports the result of a cookie-based publish.
pub fn queue_publish_result(uri: String, result: Result<(), String>) {
    let (tx, _) = channel();
    let _ = tx.send(FileEvent::PublishResult { uri, result });
}

/// Background thread reports a group's universe list + thumbnails.
pub fn queue_group_universes(
    group_id: u64,
    universes: Vec<crate::roblox_api::GroupUniverse>,
    thumbs: std::collections::HashMap<u64, String>,
) {
    let (tx, _) = channel();
    let _ = tx.send(FileEvent::GroupUniverses { group_id, universes, thumbs });
}

pub fn queue_universe_places(universe_id: u64, places: Vec<(u64, String)>) {
    let (tx, _) = channel();
    let _ = tx.send(FileEvent::UniversePlaces { universe_id, places });
}

pub fn queue_browse_error(message: String) {
    let (tx, _) = channel();
    let _ = tx.send(FileEvent::BrowseError { message });
}


/// Background threads call this when a plugin finishes downloading.
pub fn queue_plugin_bytes(name_hint: String, data: Vec<u8>) {
    let (tx, _) = plugin_channel();
    let _ = tx.send(PluginInstall { name_hint, result: Ok(data) });
}

pub fn queue_plugin_error(name_hint: String, err: String) {
    let (tx, _) = plugin_channel();
    let _ = tx.send(PluginInstall { name_hint, result: Err(err) });
}

/// Open the model picker but tag the result as a plugin install, not a
/// place-insert.
pub fn trigger_open_model_for_plugin() {
    *next_is_plugin_flag().lock().unwrap() = true;
    trigger_open_model_document();
}

/// Read and reset the "next picker result is a plugin" flag.
pub fn take_next_is_plugin() -> bool {
    std::mem::take(&mut *next_is_plugin_flag().lock().unwrap())
}

#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) -> jni::sys::jint {
    if let Ok(mut env) = vm.get_env() {
        if let Ok(class) = env.find_class("com/yourname/rbxleditor/MainActivity") {
            if let Ok(global) = env.new_global_ref(class) {
                let _ = MAIN_ACTIVITY_CLASS.set(global);
                log::info!("JNI_OnLoad: successfully cached MainActivity class");
            }
        }
    }
    jni::sys::JNI_VERSION_1_6
}

pub fn with_env(f: impl FnOnce(&mut JNIEnv, &JClass) -> Result<(), jni::errors::Error>) {
    let ctx = ndk_context::android_context();
    let vm = match unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(vm) => vm,
        Err(e) => {
            log::error!("Failed to get JavaVM: {e:?}");
            return;
        }
    };
    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(e) => {
            log::error!("Failed to attach current thread: {e:?}");
            return;
        }
    };

    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }

    let class_res = if let Some(global) = MAIN_ACTIVITY_CLASS.get() {
        Ok(unsafe { JClass::from_raw(global.as_raw() as jni::sys::jclass) })
    } else {
        env.find_class("com/yourname/rbxleditor/MainActivity")
    };

    let class = match class_res {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to find MainActivity class: {e:?}");
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            return;
        }
    };

    let result = f(&mut env, &class);

    if env.exception_check().unwrap_or(false) {
        log::error!("JNI call raised an exception");
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }

    if let Err(e) = result {
        log::error!("JNI call error: {e:?}");
    }
}

pub fn trigger_open_document() {
    with_env(|env, class| {
        let _ = env.call_static_method(class, "openDocumentStatic", "()V", &[])?;
        Ok(())
    });
}

/// Launch the Android file picker for a local .rbxm/.rbxmx MODEL file. The
/// picked bytes come back as a [`FileEvent::ModelOpened`] and are inserted as
/// a subtree into the active place, unlike `trigger_open_document` which
/// replaces the whole place.
pub fn trigger_open_model_document() {
    with_env(|env, class| {
        let _ = env.call_static_method(class, "openModelDocumentStatic", "()V", &[])?;
        Ok(())
    });
}

pub fn trigger_create_document(suggested_name: &str) {
    with_env(|env, class| {
        let name = env.new_string(suggested_name)?;
        let _ = env.call_static_method(
            class,
            "createDocumentStatic",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&name)],
        )?;
        Ok(())
    });
}

pub fn trigger_save(data: &[u8]) {
    with_env(|env, class| {
        let arr = env.byte_array_from_slice(data)?;
        let _ = env.call_static_method(
            class,
            "saveToCurrentDocumentStatic",
            "([B)V",
            &[JValue::Object(&arr)],
        )?;
        Ok(())
    });
}

pub fn trigger_edit_externally(script_id: u64, name: &str, source: &str) {
    with_env(|env, class| {
        let jname = env.new_string(name)?;
        let jsource = env.new_string(source)?;
        let _ = env.call_static_method(
            class,
            "editExternallyStatic",
            "(JLjava/lang/String;Ljava/lang/String;)V",
            &[
                JValue::Long(script_id as i64),
                JValue::Object(&jname),
                JValue::Object(&jsource),
            ],
        )?;
        Ok(())
    });
}

pub fn trigger_sync_external_edits() {
    with_env(|env, class| {
        let _ = env.call_static_method(class, "syncExternalEditsStatic", "()V", &[])?;
        Ok(())
    });
}

pub fn trigger_finish_external_edit() {
    with_env(|env, class| {
        let _ = env.call_static_method(class, "finishExternalEditStatic", "()V", &[])?;
        Ok(())
    });
}

// Soft-keyboard input forwarded from Java's RbxInputConnection.
// These are drained each frame in EditorApp::drain_ime_events and turned
// into egui Events so Gboard typing and (especially) paste land in
// egui's self-rendered text fields.
#[derive(Debug, Clone)]
pub enum ImeInput {
    Commit(String),
    DeleteSurrounding { before: usize, after: usize },
    KeyDown { keycode: i32, unicode: i32, shift: bool },
    KeyUp { keycode: i32 },
    EditorAction(i32),
    FinishComposing,
    ShowIme,
    HideIme,
}

static IME_QUEUE: OnceLock<Mutex<Vec<ImeInput>>> = OnceLock::new();
fn ime_queue() -> &'static Mutex<Vec<ImeInput>> {
    IME_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn drain_ime() -> Vec<ImeInput> {
    if let Ok(mut q) = ime_queue().lock() {
        std::mem::take(&mut *q)
    } else {
        Vec::new()
    }
}

pub fn request_show_ime() {
    // Best-effort: tell Java to show the soft keyboard.
    let _ = with_env(|env, class| {
        env.call_static_method(class, "showImeStatic", "()V", &[])?;
        Ok(())
    });
}

pub fn request_hide_ime() {
    let _ = with_env(|env, class| {
        env.call_static_method(class, "hideImeStatic", "()V", &[])?;
        Ok(())
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_RbxInputConnection_nativeCommitText(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
) {
    let s: String = env.get_string(&text).map(|s| s.into()).unwrap_or_default();
    if let Ok(mut q) = ime_queue().lock() {
        q.push(ImeInput::Commit(s));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_RbxInputConnection_nativeDeleteSurrounding(
    _env: JNIEnv,
    _class: JClass,
    before: i32,
    after: i32,
) {
    if let Ok(mut q) = ime_queue().lock() {
        q.push(ImeInput::DeleteSurrounding {
            before: before.max(0) as usize,
            after: after.max(0) as usize,
        });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_RbxInputConnection_nativeKeyDown(
    mut env: JNIEnv,
    _class: JClass,
    keycode: i32,
    unicode: i32,
    shift: jni::sys::jboolean,
) {
    if let Ok(mut q) = ime_queue().lock() {
        q.push(ImeInput::KeyDown {
            keycode,
            unicode,
            shift: shift != 0,
        });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_RbxInputConnection_nativeKeyUp(
    _env: JNIEnv,
    _class: JClass,
    keycode: i32,
) {
    if let Ok(mut q) = ime_queue().lock() {
        q.push(ImeInput::KeyUp { keycode });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_RbxInputConnection_nativeEditorAction(
    _env: JNIEnv,
    _class: JClass,
    action: i32,
) {
    if let Ok(mut q) = ime_queue().lock() {
        q.push(ImeInput::EditorAction(action));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_RbxInputConnection_nativeFinishComposing(
    _env: JNIEnv,
    _class: JClass,
) {
    if let Ok(mut q) = ime_queue().lock() {
        q.push(ImeInput::FinishComposing);
    }
}

pub fn trigger_copy_to_clipboard(text: &str) {
    with_env(|env, class| {
        let jtext = env.new_string(text)?;
        let _ = env.call_static_method(
            class,
            "copyToClipboardStatic",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&jtext)],
        )?;
        Ok(())
    });
}

pub fn get_clipboard_text() -> String {
    let mut text_out = String::new();
    with_env(|env, class| {
        let jval = env.call_static_method(class, "getClipboardTextStatic", "()Ljava/lang/String;", &[])?;
        if let Ok(jstr_obj) = jval.l() {
            if !jstr_obj.is_null() {
                let jstr = JString::from(jstr_obj);
                if let Ok(s) = env.get_string(&jstr) {
                    text_out = s.into();
                }
            }
        }
        Ok(())
    });
    text_out
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_MainActivity_nativeOnDocumentOpened(
    mut env: JNIEnv,
    _class: JClass,
    uri: JString,
    data: JByteArray,
) {
    let (tx, _) = channel();
    if uri.is_null() || data.is_null() {
        let _ = tx.send(FileEvent::OpenCancelled);
        return;
    }
    let uri_str: String = env.get_string(&uri).map(|s| s.into()).unwrap_or_default();
    let bytes: Vec<u8> = env.convert_byte_array(data).unwrap_or_default();
    let _ = tx.send(FileEvent::Opened { uri: uri_str, data: bytes });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_MainActivity_nativeOnModelOpened(
    mut env: JNIEnv,
    _class: JClass,
    uri: JString,
    data: JByteArray,
) {
    let (tx, _) = channel();
    if uri.is_null() || data.is_null() {
        let _ = tx.send(FileEvent::OpenCancelled);
        return;
    }
    let uri_str: String = env.get_string(&uri).map(|s| s.into()).unwrap_or_default();
    let bytes: Vec<u8> = env.convert_byte_array(data).unwrap_or_default();
    let _ = tx.send(FileEvent::ModelOpened { uri: uri_str, data: bytes });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_MainActivity_nativeOnDocumentCreated(
    mut env: JNIEnv,
    _class: JClass,
    uri: JString,
) {
    let (tx, _) = channel();
    if uri.is_null() {
        return;
    }
    let uri_str: String = env.get_string(&uri).map(|s| s.into()).unwrap_or_default();
    let _ = tx.send(FileEvent::Created { uri: uri_str });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_MainActivity_nativeOnSaveComplete(
    _env: JNIEnv,
    _class: JClass,
    success: jni::sys::jboolean,
) {
    let (tx, _) = channel();
    let _ = tx.send(FileEvent::SaveComplete(success != 0));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_MainActivity_nativeOnExternalEditReturned(
    mut env: JNIEnv,
    _class: JClass,
    script_id: jni::sys::jlong,
    text: JString,
) {
    let (tx, _) = channel();
    if text.is_null() {
        return;
    }
    let text_str: String = env.get_string(&text).map(|s| s.into()).unwrap_or_default();
    let _ = tx.send(FileEvent::ExternalEditReturned {
        script_id: script_id as u64,
        text: text_str,
    });
}

#[cfg(target_os = "android")]
/// Get the app's internal files directory (always writable on Android, no
/// permissions needed). This is where settings should be persisted.
pub fn files_dir() -> Option<String> {
    let mut out = None;
    with_env(|env, class| {
        let jval = env.call_static_method(class, "getFilesDirStatic", "()Ljava/lang/String;", &[])?;
        if let Ok(obj) = jval.l() {
            if !obj.is_null() {
                let jstr = jni::objects::JString::from(obj);
                if let Ok(s) = env.get_string(&jstr) {
                    out = Some(s.into());
                }
            }
        }
        Ok(())
    });
    out
}
