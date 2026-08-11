// Desktop entry point for the Bevy viewer. Lets you validate the renderer on a
// normal OS before/without building for Android:
//   cargo run --manifest-path viewer/Cargo.toml -- /path/to/place.rbxl
fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "test.rbxl".to_string());
    match std::fs::read(&path) {
        Ok(bytes) => rbxl_viewer::render::run_game(&bytes),
        Err(e) => rbxl_viewer::render::run_empty(format!("cannot read {path}: {e}")),
    }
}
