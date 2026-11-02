//! What the player remembers between runs, as JSON in the app's support folder.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// ~/Library/Application Support/WebSkin Player (or $WEBSKIN_HOME, for tests and the corpus runner).
pub fn support_dir() -> PathBuf {
    std::env::var_os("WEBSKIN_HOME").map(PathBuf::from).unwrap_or_else(|| dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("WebSkin Player"))
}

/// The visualization last shown in one skin.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct SkinEffect {
    pub effect: String,
    pub preset: usize,
}

#[derive(Serialize, Deserialize, Debug)]
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
    /// The visualization last shown in each skin (key: its file name, lower case). Full mode's choice goes to the
    /// last skin, so it is the one that skin shows when you go back.
    pub skin_effects: HashMap<String, SkinEffect>,
    /// Colours chosen per visualization (key: its id), e.g. "palette:Fire"; see [`crate::vis::colors`].
    pub vis_colors: HashMap<String, String>,
    /// Player windows (full mode and every skin window) stay above other windows.
    pub always_on_top: bool,
    /// The mode the player was in when it quit: "skin", "full" or "menubar".
    pub start_mode: String,
    /// The menu-bar player stays in the menu bar in every mode, not only when the player is tucked into it.
    pub menu_bar_always: bool,
    /// The player was showing a skin (not full mode) when it was tucked into the menu bar.
    pub tucked_from_skin: bool,
    /// Full mode's window frame (x, y, width, height in Cocoa screen coordinates).
    pub full_mode_frame: Option<(f64, f64, f64, f64)>,
    /// The visualizer screen saver: random full-screen visualizations once the Mac has been idle this long.
    pub saver_enabled: bool,
    pub saver_idle_minutes: u32,
    pub saver_change_seconds: u32,
    /// Lock the Mac when the screen saver is dismissed.
    pub saver_locks: bool,
    #[serde(skip)]
    disabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            last_skin: None,
            skin_preferences: HashMap::new(),
            skin_windows: HashMap::new(),
            media_center: HashMap::new(),
            vis_effect: String::new(),
            vis_preset: 0,
            skin_effects: HashMap::new(),
            vis_colors: HashMap::new(),
            always_on_top: false,
            start_mode: String::new(),
            menu_bar_always: false,
            tucked_from_skin: false,
            full_mode_frame: None,
            saver_enabled: false,
            saver_idle_minutes: 5,
            saver_change_seconds: 30,
            saver_locks: false,
            disabled: false,
        }
    }
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

    /// Never written: previews, the corpus runner, and test runs.
    pub fn is_scratch(&self) -> bool {
        self.disabled
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

    /// The screen saver's idle time, within sensible bounds.
    pub fn saver_idle(&self) -> u32 {
        self.saver_idle_minutes.clamp(1, 120)
    }

    /// How long the screen saver shows each visualization, in seconds.
    pub fn saver_change(&self) -> u32 {
        self.saver_change_seconds.clamp(5, 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_settings_files_load_with_the_new_defaults() {
        let old = r#"{ "last_skin": "/tmp/Compact.wmz", "vis_effect": "Battery", "vis_preset": 2 }"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.vis_effect, "Battery");
        assert_eq!(s.saver_idle(), 5);
        assert_eq!(s.saver_change(), 30);
        assert!(!s.always_on_top);
        assert!(s.skin_effects.is_empty());
    }
}
