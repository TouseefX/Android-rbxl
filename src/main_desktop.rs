// Desktop runner: lets you validate the Bevy + bevy_egui editor on a normal OS
// without building for Android. Usage:
//   cargo run --bin rbxl-editor-desktop -- /path/to/place.rbxl
fn main() {
    let initial = std::env::args().nth(1).and_then(|p| std::fs::read(p).ok());
    rbxl_editor::run_editor_app(initial);
}
