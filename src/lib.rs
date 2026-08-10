mod app;
mod asset_downloader;
mod explorer;
mod jni_bridge;
mod lua_syntax;
mod roblox_api;
mod rbxl;
mod schema;
mod settings;
mod templates;
mod viewport3d;

// Pull AndroidApp from winit's re-export, not a direct android-activity
// dependency -- this keeps the type identical to whatever version
// winit/eframe use internally (see the comment in Cargo.toml).
use winit::platform::android::activity::AndroidApp;

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


