//! What the player remembers between runs, as JSON in the app's support folder.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// ~/Library/Application Support/WebSkin Player (or $WEBSKIN_HOME, for tests and the corpus runner).
pub fn support_dir() -> PathBuf {
    std::env::var_os("WEBSKIN_HOME").map(PathBuf::from).unwrap_or_else(|| dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("WebSkin Player"))
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Settings {
    /// The skin open when the player last quit.
    pub last_skin: Option<PathBuf>,
    /// theme.savePreference values, per skin file name.
    pub skin_preferences: HashMap<String, HashMap<String, String>>,
    /// Where each skin view's window was, by "skin.wmz|viewid".
    pub skin_windows: HashMap<String, (f64, f64)>,
    /// mediacenter.* values skins stored, as "value|type".
    pub media_center: HashMap<String, String>,
    /// The visualization last chosen (mediacenter.effectType), by id, and its preset.
    pub vis_effect: String,
    pub vis_preset: usize,
    #[serde(skip)]
    disabled: bool,
}

impl Settings {
    fn path() -> PathBuf {
        support_dir().join("settings.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Settings that are never written (skin previews and the corpus runner).
    pub fn scratch() -> Self {
        Settings { disabled: true, ..Default::default() }
    }

    pub fn save(&self) {
        if self.disabled {
            return;
        }
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(tmp, path);
            }
        }
    }
}
