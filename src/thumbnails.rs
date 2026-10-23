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

/// Poison-tolerant lock: a single panicking download thread must not
/// take down every subsequent frame via `lock().unwrap()`.
fn lock_state() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Hard cap on cached thumbnails (each holds a GPU texture). The map
/// previously grew forever while browsing the catalog/toolbox.
const MAX_ENTRIES: usize = 256;

fn evict_overflow(st: &mut State) {
    while st.entries.len() > MAX_ENTRIES {
        // Prefer evicting finished entries; never evict a download in
        // flight if a completed one can go instead.
        let victim = st
            .entries
            .iter()
            .find(|(_, e)| !matches!(e, Entry::Pending))
            .map(|(k, _)| k.clone())
            .or_else(|| st.entries.keys().next().cloned());
        let Some(victim) = victim else { break };
        st.entries.remove(&victim);
    }
}

/// One shared blocking client for all thumbnail fetches. Building a
/// fresh `reqwest::blocking::Client` (new connection pool + TLS config)
/// for *every thumbnail* was a major catalog-scroll cost.
fn http_client() -> Option<&'static reqwest::blocking::Client> {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    // `get_or_try_init` is unstable; build outside and race-set instead.
    if let Some(c) = CLIENT.get() {
        return Some(c);
    }
    let built = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("rbxl-editor")
        .build()
        .ok()?;
    let _ = CLIENT.set(built);
    CLIENT.get()
}

/// Ask the cache to fetch `url` if it hasn't already. Safe to call every
/// frame; subsequent calls are no-ops once the download is queued.
pub fn prefetch(url: impl Into<String>) {
    let url = url.into();
    let mut st = lock_state();
    use std::collections::hash_map::Entry as E;
    if let E::Vacant(v) = st.entries.entry(url.clone()) {
        v.insert(Entry::Pending);
        evict_overflow(&mut st);
        drop(st);
        std::thread::spawn(move || {
            let bytes = fetch_bytes(&url);
            let _ = lock_state().from_main.send((url, bytes));
        });
    }
}

/// Drain any background completions, building textures for successful
/// downloads via the egui context. Call once per frame.
pub fn pump(ctx: &egui::Context) {
    let mut st = lock_state();
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
    evict_overflow(&mut st);
}

/// Returns the texture handle for `url` if it's ready, and triggers a
/// fetch in the background if it hasn't been queued yet. The returned
/// handle is cheap to clone (it's an Arc to the GPU texture).
pub fn get_or_load(ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
    prefetch(url);
    // If the entry became ready this frame, pump now so the first caller
    // also gets the image without waiting another frame.
    pump(ctx);
    let st = lock_state();
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
    let resp = http_client()?.get(url).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().ok()?;
    Some(bytes.to_vec())
}
