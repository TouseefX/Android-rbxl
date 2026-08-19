//! Audio playback for cached Sound assets.
//!
//! Audio bytes (ogg/mp3) are downloaded into `asset_downloader`'s raw
//! cache by `fetch_and_cache_audio_async`. To actually play them we
//! hand off to the platform:
//!
//! * Android: write bytes to a temp file and call
//!   `MainActivity.playAudioFile(path)` through JNI, which uses
//!   `android.media.MediaPlayer`.
//! * Desktop: if the `desktop_audio` feature is enabled, decode with
//!   rodio; otherwise log that audio isn't available.

use std::io::Write;

/// Play a cached sound by `rbxassetid://<id>`. Triggers a fetch if not
/// cached (best-effort; actual playback happens once the bytes land).
pub fn play_cached_or_fetch(id: &str, cookie: Option<String>) -> Result<(), String> {
    if let Some(bytes) = crate::asset_downloader::get_cached_raw(id) {
        return play_bytes(&bytes);
    }
    // Not cached: start the fetch and report that we're waiting.
    crate::roblox_api::fetch_and_cache_audio_async(id.to_string(), cookie);
    Err("audio not cached yet; started download".into())
}

pub fn play_bytes(bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        play_android(bytes)
    }
    #[cfg(not(target_os = "android"))]
    {
        play_desktop(bytes)
    }
}

/// Stop any currently-playing audio.
pub fn stop() {
    #[cfg(target_os = "android")]
    {
        crate::jni_bridge::with_env(|env, class| {
            env.call_static_method(class, "stopAudio", "()V", &[])?;
            Ok(())
        });
    }
    #[cfg(all(not(target_os = "android"), feature = "desktop_audio"))]
    {
        // rodio's Sink stops when dropped; the desktop playback thread
        // owns its own Sink, so we can't easily interrupt it from here.
        // A future version could hold a global Weak<Sink>.
    }
}

#[cfg(target_os = "android")]
fn play_android(bytes: &[u8]) -> Result<(), String> {
    let mut path = std::env::temp_dir();
    path.push(format!("rbxl_audio_{}.bin", std::process::id()));
    {
        let mut f = std::fs::File::create(&path).map_err(|e| format!("tmp: {e}"))?;
        f.write_all(bytes).map_err(|e| format!("write: {e}"))?;
        f.sync_all().ok();
    }
    let path_str = path.to_string_lossy().to_string();
    // Delegate to with_env; it logs any JNI errors. The Java side shows
    // a Toast if playback fails.
    let _ = path_str.clone();
    crate::jni_bridge::with_env(move |env, class| {
        let jpath = env.new_string(&path_str)?;
        env.call_static_method(
            class,
            "playAudioFile",
            "(Ljava/lang/String;)V",
            &[jni::objects::JValue::Object(&jpath)],
        )?;
        Ok(())
    });
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn play_desktop(bytes: &[u8]) -> Result<(), String> {
    #[cfg(feature = "desktop_audio")]
    {
        use std::io::Cursor;
        let owned = bytes.to_vec();
        std::thread::spawn(move || {
            if let Ok((_stream, handle)) = rodio::OutputStream::try_default() {
                if let Ok(sink) = rodio::Sink::try_new(&handle) {
                    if let Ok(source) = rodio::Decoder::new(Cursor::new(owned)) {
                        sink.append(source);
                        sink.sleep_until_end();
                    }
                }
            }
        });
        Ok(())
    }
    #[cfg(not(feature = "desktop_audio"))]
    {
        eprintln!(
            "[audio] {} bytes ready; desktop playback requires the `desktop_audio` feature.",
            bytes.len()
        );
        Ok(())
    }
}
