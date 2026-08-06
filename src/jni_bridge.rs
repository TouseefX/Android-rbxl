use jni::objects::{JByteArray, JClass, JString};
use jni::JNIEnv;
use std::sync::mpsc;
use std::sync::OnceLock;

pub enum FileEvent {
    Opened { uri: String, data: Vec<u8> },
    OpenCancelled,
    Created { uri: String },
    SaveComplete(bool),
    /// Text came back from an external editor (QuickEdit etc.) for the
    /// script identified by `script_id` (see EditorApp::request_id below).
    ExternalEditReturned { script_id: u64, text: String },
}

static FILE_EVENTS: OnceLock<(mpsc::Sender<FileEvent>, mpsc::Receiver<FileEvent>)> = OnceLock::new();

pub fn channel() -> &'static (mpsc::Sender<FileEvent>, mpsc::Receiver<FileEvent>) {
    FILE_EVENTS.get_or_init(mpsc::channel)
}

fn env_and_activity() -> (jni::AttachGuard<'static>, jni::objects::JObject<'static>) {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    // Leak-free in practice: JNIEnv borrows are per-call, this pattern is the
    // standard android-activity + jni idiom for one-off calls into Java.
    let env = Box::leak(Box::new(vm)).attach_current_thread().unwrap();
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    (env, activity)
}

pub fn trigger_open_document() {
    let (mut env, activity) = env_and_activity();
    let _ = env.call_method(&activity, "openDocument", "()V", &[]);
}

pub fn trigger_create_document(suggested_name: &str) {
    let (mut env, activity) = env_and_activity();
    let name = env.new_string(suggested_name).unwrap();
    let _ = env.call_method(
        &activity,
        "createDocument",
        "(Ljava/lang/String;)V",
        &[(&name).into()],
    );
}

pub fn trigger_save(data: &[u8]) {
    let (mut env, activity) = env_and_activity();
    let arr = env.byte_array_from_slice(data).unwrap();
    let _ = env.call_method(&activity, "saveToCurrentDocument", "([B)V", &[(&arr).into()]);
}

/// Ask Java to create a standalone temp document for `script_id`, write
/// `source` into it, then launch a chooser so the user can pick QuickEdit
/// or any other text-editor app installed.
pub fn trigger_edit_externally(script_id: u64, name: &str, source: &str) {
    let (mut env, activity) = env_and_activity();
    let jname = env.new_string(format!("{name}.lua")).unwrap();
    let jsource = env.new_string(source).unwrap();
    let _ = env.call_method(
        &activity,
        "editExternally",
        "(JLjava/lang/String;Ljava/lang/String;)V",
        &[script_id.into(), (&jname).into(), (&jsource).into()],
    );
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yourname_rbxleditor_MainActivity_nativeOnDocumentOpened(
    mut env: JNIEnv,
    _class: JClass,
    uri: JString,
    data: JByteArray,
) {
    let (tx, _) = channel();
    if uri.is_null() {
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
    let text_str: String = env.get_string(&text).map(|s| s.into()).unwrap_or_default();
    let _ = tx.send(FileEvent::ExternalEditReturned {
        script_id: script_id as u64,
        text: text_str,
    });
}
