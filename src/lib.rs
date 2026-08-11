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
// Android entry uses `#[bevy_main]` (android-native-activity), compatible with
// cargo-apk2 and the existing NativeActivity MainActivity.java.
// ============================================================================

mod app;
mod asset_downloader;
mod bevy_render;
mod explorer;
mod jni_bridge;
mod lua_syntax;
mod roblox_api;
mod rbxl;
mod schema;
mod settings;
mod templates;

use app::EditorApp;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin};

/// Spawn the 3D viewport camera + sun light.
fn setup_3d(mut commands: Commands) {
    bevy_render::spawn_camera_and_light(commands);
}

/// Draw the egui editor UI each frame (toolbar, tabs, panels) and steer the
/// Bevy viewport camera from the 3D tab.
fn draw_editor_ui(
    mut app: ResMut<EditorApp>,
    mut orbit: ResMut<bevy_render::OrbitCam>,
    mut contexts: EguiContexts,
) {
    if let Ok(ctx) = contexts.ctx_mut() {
        app.draw_editor(ctx, &mut orbit);
    }
}

/// (Re)build the Bevy 3D scene from the editor's current place whenever a new
/// place was opened (`needs_3d_rebuild`).
fn rebuild_scene_system(
    mut commands: Commands,
    mut app: ResMut<EditorApp>,
    mut meshes: ResMut<Assets<bevy::mesh::Mesh>>,
    mut materials: ResMut<Assets<bevy::pbr::StandardMaterial>>,
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
            ..default()
        }),
        ..default()
    }));
    bevy_app.add_plugins(EguiPlugin::default());

    bevy_app.insert_resource(editor);
    bevy_app.insert_resource(bevy_render::OrbitCam::default());
    bevy_app.insert_resource(bevy::light::AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    });
    bevy_app.insert_resource(ClearColor(Color::srgb(0.35, 0.55, 0.85)));

    bevy_app.add_systems(Startup, setup_3d);
    // draw_editor_ui runs before the camera sync so orbit updates land.
    bevy_app.add_systems(Update, (rebuild_scene_system, draw_editor_ui, bevy_render::update_camera).chain());

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
    run_editor_app(None);
}
