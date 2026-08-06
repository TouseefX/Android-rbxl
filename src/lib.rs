mod app;
mod explorer;
mod jni_bridge;
mod lua_syntax;
mod rbxl;

use android_activity::AndroidApp;

#[unsafe(no_mangle)]
fn android_main(android_app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    // Make sure the event channel exists before any JNI callback could fire.
    let _ = jni_bridge::channel();

    let options = eframe::NativeOptions {
        android_app: Some(android_app),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "rbxl Editor",
        options,
        Box::new(|_cc| Ok(Box::new(app::EditorApp::default()))),
    ) {
        log::error!("eframe::run_native failed: {e:?}");
    }
}
