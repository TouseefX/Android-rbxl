use jni::objects::{GlobalRef, JByteArray, JClass, JString, JValue};
use jni::JNIEnv;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

pub enum FileEvent {
    Opened { uri: String, data: Vec<u8> },
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

fn with_env(f: impl FnOnce(&mut JNIEnv, &JClass) -> Result<(), jni::errors::Error>) {
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
