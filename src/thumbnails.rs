//! Thumbnail image loading + egui texture cache.
//!
//! URLs are produced by `WebClient::thumbnails_batch` (the games/icons or
//! assets/batch thumbnail endpoints). To actually *show* a thumbnail we
//! need to:
//!   1. download the PNG/JPG bytes (cached on disk by the HTTP client),
//!   2. decode them to RGBA (the `image` crate is already a dependency),
//!   3. upload them to the GPU as an `egui::TextureHandle`.
//!
//! Textures are owned by the egui context, so they must be created from
//! within a frame (which is where `get_or_load` is called). The download
//! runs on a background thread and the bytes come back through an mpsc
//! channel; the texture is created on the next frame after the bytes
//! arrive.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use bevy_egui::egui::{self, ColorImage, TextureHandle, TextureOptions};

/// One pending or completed image.
enum Entry {
    /// Download/decode in flight.
    Pending,
    /// Ready to draw.
    Ready(TextureHandle),
    /// Download or decode failed (we show a placeholder instead).
    Failed,
}

struct State {
    entries: HashMap<String, Entry>,
    to_main: Receiver<(String, Option<Vec<u8>>)>,
    from_main: Sender<(String, Option<Vec<u8>>)>,
}

impl State {
    fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            entries: HashMap::new(),
            to_main: rx,
            from_main: tx,
        }
    }
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::new()))
}

/// Ask the cache to fetch `url` if it hasn't already. Safe to call every
/// frame; subsequent calls are no-ops once the download is queued.
pub fn prefetch(url: impl Into<String>) {
    let url = url.into();
    let mut st = state().lock().unwrap();
    use std::collections::hash_map::Entry as E;
    if let E::Vacant(v) = st.entries.entry(url.clone()) {
        v.insert(Entry::Pending);
        drop(st);
        std::thread::spawn(move || {
            let bytes = fetch_bytes(&url);
            let _ = state().lock().unwrap().from_main.send((url, bytes));
        });
    }
}

/// Drain any background completions, building textures for successful
/// downloads via the egui context. Call once per frame.
pub fn pump(ctx: &egui::Context) {
    let mut st = state().lock().unwrap();
    while let Ok((url, bytes)) = st.to_main.try_recv() {
        let entry = match bytes.as_deref().and_then(decode_rgba) {
            Some((w, h, rgba)) => {
                let img = ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                let handle = ctx.load_texture(
                    format!("thumb:{url}"),
                    img,
                    TextureOptions::LINEAR,
                );
                Entry::Ready(handle)
            }
            None => Entry::Failed,
        };
        st.entries.insert(url, entry);
    }
}

/// Returns the texture handle for `url` if it's ready, and triggers a
/// fetch in the background if it hasn't been queued yet. The returned
/// handle is cheap to clone (it's an Arc to the GPU texture).
pub fn get_or_load(ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
    prefetch(url);
    // If the entry became ready this frame, pump now so the first caller
    // also gets the image without waiting another frame.
    pump(ctx);
    let st = state().lock().unwrap();
    match st.entries.get(url) {
        Some(Entry::Ready(t)) => Some(t.clone()),
        _ => None,
    }
}

fn decode_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    Some((w, h, rgba.into_raw()))
}

fn fetch_bytes(url: &str) -> Option<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("rbxl-editor")
        .build()
        .ok()?;
    let resp = client.get(url).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().ok()?;
    Some(bytes.to_vec())
}
