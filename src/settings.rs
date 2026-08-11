use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
        // Try Android external media / scripts folder first, then home dir
        let android_media = Path::new("/Android/media/com.yourname.rbxleditor/scripts/settings.json");
        if android_media.parent().map(|p| p.exists()).unwrap_or(false) {
            return android_media.to_path_buf();
        }

        let home_settings = Path::new("/home/user/settings.json");
        home_settings.to_path_buf()
    }

    pub fn load() -> Self {
        let path = Self::get_settings_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<Self>(&data) {
                return settings;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::get_settings_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {e}"))?;

        std::fs::write(&path, json)
            .map_err(|e| format!("Failed to write settings to {:?}: {e}", path))?;

        Ok(())
    }
}
