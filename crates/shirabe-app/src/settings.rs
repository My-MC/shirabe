//! Settings of the standalone application (selected source and parameter values).
//!
//! Stored as JSON in the user's configuration directory: `%APPDATA%\\Shirabe` on Windows,
//! `~/Library/Application Support/Shirabe` on macOS and `$XDG_CONFIG_HOME/shirabe` (default
//! `~/.config/shirabe`) elsewhere.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::audio::Source;
use crate::output::DeviceRef;

/// Version of the settings file layout.
const VERSION: u32 = 1;

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    /// The selected source; `None` on first start (use the default input).
    pub source: Option<Source>,
    /// Parameter values by ID (see `context::save_params`).
    pub params: BTreeMap<String, f32>,
    /// Output device used for file playback; `None` uses the default device.
    #[serde(default)]
    pub output: Option<DeviceRef>,
    /// Theme colours imported from an appearance preset; `None` uses the built-in dark theme.
    /// Older settings files do not contain this field.
    #[serde(default)]
    pub theme: Option<crate::appearance::ThemeSpec>,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    #[serde(flatten)]
    settings: Settings,
}

fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("standalone.json"))
}

/// Directory holding the settings and the default appearance preset.
pub fn config_dir() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?).join("Shirabe")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Shirabe")
    } else if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()) {
        PathBuf::from(x).join("shirabe")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config/shirabe")
    };
    Some(dir)
}

/// Default path offered for appearance export and import.
pub fn default_appearance_path() -> Option<PathBuf> {
    Some(config_dir()?.join("appearance.json"))
}

impl Settings {
    /// Loads the settings; returns defaults if the file is missing or unreadable.
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
            .filter(|f| f.version == VERSION)
            .map(|f| f.settings)
            .unwrap_or_default()
    }

    /// Saves the settings; errors are ignored (the application keeps working without them).
    pub fn save(self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = File {
            version: VERSION,
            settings: self,
        };
        if let Ok(json) = serde_json::to_vec_pretty(&file) {
            let _ = std::fs::write(p, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_saved_before_themes_existed_still_load() {
        let text = r#"{
            "version": 1,
            "source": null,
            "params": {"lay_preset": 1.0}
        }"#;
        let file: File = serde_json::from_str(text).unwrap();
        assert!(file.settings.theme.is_none());
        assert_eq!(file.settings.params.get("lay_preset"), Some(&1.0));
    }

    #[test]
    fn an_imported_theme_is_saved_and_restored() {
        let theme = crate::appearance::ThemeSpec::from_theme(&shirabe_ui::Theme::dark());
        let file = File {
            version: VERSION,
            settings: Settings {
                theme: Some(theme.clone()),
                ..Settings::default()
            },
        };
        let text = serde_json::to_string(&file).unwrap();
        let back: File = serde_json::from_str(&text).unwrap();
        assert_eq!(back.settings.theme, Some(theme));
    }
}
