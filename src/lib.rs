// ============================================================================
// rbxl-editor — a Roblox place editor rendered as a single Bevy app
// ----------------------------------------------------------------------------
// Architecture: ONE app, owned by Bevy.
//   - Bevy owns the window, so it gets a GPU context and renders the 3D
//     viewport natively on Android (this is what makes the GPU viewport work,
//     unlike embedding Bevy inside an eframe window).
//   - The egui editor UI runs INSIDE Bevy via `bevy_egui`, drawn over the 3D.
//   - Opening a place rebuilds the Bevy meshes; the 3D tab shows them live.
//
// Android entry uses `#[bevy_main]`, which expands to the `android_main` entry
// point of `android-activity`'s GameActivity backend (Bevy feature
// `android-game-activity`). The APK is built by xbuild (`manifest.yaml`), which
// drives a real Gradle project so `kotlin/MainActivity.kt` can extend
// `com.google.androidgamesdk.GameActivity` from the games-activity AAR — that
// is what gives us a proper InputConnection / soft keyboard (paste works).
// ============================================================================

mod android_ime;
mod app;
mod asset_downloader;
mod audio;
mod bevy_render;
mod explorer;
mod jni_bridge;
mod lua_syntax;
mod roblox_api;
mod roblox_domains;
mod rbxl;
mod schema;
mod live_session;
mod lua_runtime;
mod plugins;
mod settings;
mod templates;
mod thumbnails;

use app::EditorApp;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};

/// Spawn the 3D viewport camera + sun light.
fn setup_3d(
    mut commands: Commands,
    mut meshes: ResMut<Assets<bevy::mesh::Mesh>>,
    mut sky_materials: ResMut<Assets<bevy_render::SkyMaterial>>,
) {
    bevy_render::spawn_camera_and_light(&mut commands);
    bevy_render::spawn_sky_dome(&mut commands, &mut meshes, &mut sky_materials);
}



/// Draw the egui editor UI each frame (toolbar, tabs, panels) and steer the
/// Bevy viewport camera from the 3D tab.
fn draw_editor_ui(
    mut app: ResMut<EditorApp>,
    mut orbit: ResMut<bevy_render::OrbitCam>,
    mut contexts: EguiContexts,
) {
    if let Ok(ctx) = contexts.ctx_mut() {
        // Feed anything the Android IME produced since last frame into egui
        // BEFORE the widgets are built, so the focused TextEdit sees it.
        android_ime::begin_frame(ctx);
        app.draw_editor(ctx, &mut orbit);
        // Show/hide the soft keyboard to match egui's focus and flush egui's
        // clipboard writes to Android.
        android_ime::end_frame(ctx);
    }
}

/// (Re)build the Bevy 3D scene from the editor's current place whenever a new
/// place was opened (`needs_3d_rebuild`).
fn rebuild_scene_system(
    mut commands: Commands,
    mut app: ResMut<EditorApp>,
    mut meshes: ResMut<Assets<bevy::mesh::Mesh>>,
    mut materials: ResMut<Assets<bevy_render::FlatMaterial>>,
    mut images: ResMut<Assets<bevy::image::Image>>,
    old_scene: Query<Entity, With<bevy_render::RbxSceneRoot>>,
) {
    if !app.take_3d_rebuild() {
        return;
    }
    for e in &old_scene {
        commands.entity(e).despawn();
    }
    if let Some(dom) = app.dom() {
        bevy_render::rebuild_scene(&mut commands, &mut meshes, &mut materials, &mut images, dom);
    }
}

/// Build and run the Bevy editor app. `initial_bytes` (a raw place file) is
/// loaded at startup if present — used by the desktop runner for validation.
pub fn run_editor_app(initial_bytes: Option<Vec<u8>>) {
    let mut editor = EditorApp::default();
    if let Some(bytes) = initial_bytes {
        editor.load_from_bytes(bytes);
    }

    let mut bevy_app = App::new();
    bevy_app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "rbxl Editor".into(),
            // Enable winit's IME so the soft keyboard / Gboard can
            // commit text (and paste) into egui text fields on Android.
            ime_enabled: true,
            ..default()
        }),
        ..default()
    }).set(bevy::render::RenderPlugin {
        // Force the Vulkan backend. On Adreno GPUs (Galaxy S20 Ultra), Bevy
        // can pick GLES and its shaders fail over GLES, which renders every
        // mesh magenta. Vulkan is well-supported on Adreno 660 and fixes the
        // magenta. (If Vulkan isn't available the adapter init will log it.)
        render_creation: bevy::render::settings::WgpuSettings {
            backends: Some(bevy::render::settings::Backends::VULKAN),
            ..default()
        }
        .into(),
        ..default()
    }));
    bevy_app.add_plugins(EguiPlugin::default());
    // Register the custom flat-colour material (bypasses StandardMaterial,
    // which renders magenta on this device's Adreno GPU).
    bevy_app.add_plugins(bevy_render::FlatMaterialPlugin);

    bevy_app.insert_resource(editor);
    bevy_app.insert_resource(bevy_render::OrbitCam::default());
    bevy_app.insert_resource(bevy::light::AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    });
    // Roblox-style light blue sky (matches the OpenRBLX reference scene).
    bevy_app.insert_resource(ClearColor(Color::srgb(0.56, 0.84, 0.97)));

    bevy_app.add_systems(Startup, setup_3d);
    // Scene rebuild + camera sync run on the main Update schedule.
    bevy_app.add_systems(Update, (rebuild_scene_system, bevy_render::update_camera, bevy_render::update_sky_dome));
    // IMPORTANT: the egui UI must run in bevy_egui's `EguiPrimaryContextPass`
    // schedule, NOT `Update`. bevy_egui loads its fonts when it begins its
    // frame; running the UI in `Update` (before begin-pass) panics with
    // "No fonts available until first call to Context::run()".
    bevy_app.add_systems(EguiPrimaryContextPass, draw_editor_ui);

    // Make sure the event channel exists before any JNI callback could fire.
    let _ = jni_bridge::channel();

    bevy_app.run();
}

/// Android entry point. `#[bevy_main]` generates the native `android_main`
/// with the app context Bevy needs.
#[cfg(target_os = "android")]
#[bevy_main]
fn main() {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    // Log any Rust panic (including ones that then abort from a C callback,
    // like the android-activity resize panic) so the real message shows in
    // logcat instead of just a silent SIGABRT.
    std::panic::set_hook(Box::new(|info| {
        log::error!("===== PANIC =====");
        log::error!("{}", info);
        log::error!("location: {:?}", info.location());
        // Also dump the message alone, which is the most useful bit.
        if let Some(s) = info.payload().downcast_ref::<&str>() {
            log::error!("payload: {s}");
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            log::error!("payload: {s}");
        }
    }));

    run_editor_app(None);
}
