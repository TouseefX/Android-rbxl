use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[cfg(target_os = "android")]
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {
    pub roblosecurity_cookie: String,
    pub open_cloud_api_key: String,
    pub open_cloud_universe_id: String,
    pub open_cloud_place_id: String,
    pub auto_download_meshes: bool,
    pub show_skybox: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            roblosecurity_cookie: String::new(),
            open_cloud_api_key: String::new(),
            open_cloud_universe_id: String::new(),
            open_cloud_place_id: String::new(),
            auto_download_meshes: true,
            show_skybox: true,
        }
    }
}

impl EditorSettings {
    pub fn get_settings_path() -> PathBuf {
        // On Android use the app's internal files dir, which is always writable
        // (no permissions needed). On desktop use the home dir.
        #[cfg(target_os = "android")]
        {
            if let Some(dir) = crate::jni_bridge::files_dir() {
                return Path::new(&dir).join("settings.json");
            }
        }
        let home_settings = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
        home_settings.join("rbxl_editor_settings.json")
    }

    pub fn load() -> Self {
        let path = Self::get_settings_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<Self>(&data) {
                log::info!("Loaded editor settings from {:?}", path);
                return settings;
            }
        }
        log::warn!(
            "No editor settings found at {:?}; using defaults",
            path
        );
        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_settings_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {e}"))?;

        log::info!("Saving editor settings to {:?}", path);
        std::fs::write(&path, json)
            .map_err(|e| format!("Failed to write settings to {:?}: {e}", path))?;

        Ok(())
    }
}
