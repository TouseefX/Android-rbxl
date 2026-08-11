// ============================================================================
// rbxl-viewer  —  standalone Bevy GPU viewer for Roblox places (Android)
// ----------------------------------------------------------------------------
// A full-screen Bevy app that renders a .rbxl / .rbxmx / .rbxm place with the
// GPU. This is the "Option A" architecture: instead of trying to embed Bevy
// inside the egui editor (which fails on Android because Bevy can't get a GPU
// context inside eframe's window), the viewer is its own native Android
// activity that owns a window and renders on the GPU.
//
// The editor app writes the current place to
//   /storage/emulated/0/rbxlviewer/current.rbxl
// and then launches this app (see the editor's "View in Bevy" button). This
// viewer reads that file on startup and renders it full-screen with an orbit
// camera.
//
// Build (Android, via GitHub Actions / cargo-apk2):
//   cargo apk2 build --lib -p ...   (or from the viewer/ directory)
// Build & run (desktop, for validation):
//   cargo run --manifest-path viewer/Cargo.toml -- /path/to/place.rbxl
// ============================================================================

pub mod render;

use bevy::prelude::*;

/// Path the editor writes the current place to before launching this viewer.
/// Chosen in shared external storage so both apps can access it (both declare
/// MANAGE_EXTERNAL_STORAGE).
pub const PLACE_PATH: &str = "/storage/emulated/0/rbxlviewer/current.rbxl";

/// Read and render the place at `path`.
fn render_file(path: &str) {
    match std::fs::read(path) {
        Ok(bytes) => {
            log::info!("rbxl-viewer: rendering {}", path);
            render::run_game(&bytes);
        }
        Err(e) => {
            log::error!("rbxl-viewer: cannot read {path}: {e}");
            // Show a minimal Bevy window with an error note, so the user isn't
            // left staring at a black screen.
            render::run_empty(e.to_string());
        }
    }
}

/// Android entry point. `#[bevy_main]` expands to the native `android_main`
/// that Bevy needs (with the `android-native-activity` feature), passing the
/// Android app context to Bevy internally. We just read the place file and run.
#[bevy_main]
fn main() {
    #[cfg(target_os = "android")]
    {
        android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
        render_file(PLACE_PATH);
    }
    #[cfg(not(target_os = "android"))]
    {
        // Desktop: take a path from the command line (or default to test.rbxl).
        env_logger_dummy();
        let path = std::env::args().nth(1).unwrap_or_else(|| "test.rbxl".to_string());
        render_file(&path);
    }
}

#[cfg(not(target_os = "android"))]
fn env_logger_dummy() {
    // Bevy's DefaultPlugins installs a logger on desktop; nothing to do here.
}
