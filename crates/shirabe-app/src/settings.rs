//! Settings of the standalone application (selected source and parameter values).
//!
//! Stored as JSON in the user's configuration directory: `%APPDATA%\\Shirabe` on Windows,
//! `~/Library/Application Support/Shirabe` on macOS and `$XDG_CONFIG_HOME/shirabe` (default
//! `~/.config/shirabe`) elsewhere.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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
    #[serde(default, deserialize_with = "deserialize_theme")]
    pub theme: Option<crate::appearance::ThemeSpec>,
}

fn deserialize_theme<'de, D>(de: D) -> Result<Option<crate::appearance::ThemeSpec>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(mut theme) = Option::<serde_json::Value>::deserialize(de)? else {
        return Ok(None);
    };
    // Only settings (not preset files) accept the prerelease theme shape. The
    // renderer used the first and last colours and never used these thresholds.
    if let Some(fields) = theme.as_object_mut() {
        if let Some(zones) = fields.remove("vu_zones") {
            let pair = |zone: &serde_json::Value| serde_json::json!({"lit": zone["lit"], "unlit": zone["unlit"]});
            if let Some((first, last)) = zones.as_array().and_then(|z| z.first().zip(z.last())) {
                fields.entry("level_bar").or_insert_with(
                    || serde_json::json!({"normal": pair(first), "over": pair(last)}),
                );
            }
        }
    }
    // An invalid theme must not discard saved audio and metering settings.
    Ok(serde_json::from_value(theme).ok())
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

    /// Saves the settings and reports errors to the caller.
    pub fn save(self) -> Result<()> {
        let path = path().context("configuration directory is unavailable")?;
        self.save_to(&path)
    }

    fn save_to(self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let file = File {
            version: VERSION,
            settings: self,
        };
        let json = serde_json::to_vec_pretty(&file).context("cannot encode settings")?;
        std::fs::write(path, json).with_context(|| format!("cannot write {}", path.display()))
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
    fn settings_with_legacy_vu_zones_keep_params_and_the_rendered_colours() {
        let mut theme = serde_json::to_value(crate::appearance::ThemeSpec::from_theme(
            &shirabe_ui::Theme::dark(),
        ))
        .unwrap();
        theme.as_object_mut().unwrap().remove("level_bar");
        theme["vu_zones"] = serde_json::json!([
            {"lit": "#123456", "unlit": "#000000"},
            {"from_db": -5, "lit": "#ffff00", "unlit": "#010000"},
            {"from_db": 3, "lit": "#ff0000", "unlit": "#000001"}
        ]);
        let text = serde_json::json!({"version": 1, "source": "None", "params": {"lay_preset": 1.0}, "theme": theme});
        let file: File = serde_json::from_value(text).unwrap();
        assert_eq!(file.settings.params.get("lay_preset"), Some(&1.0));
        let theme = file.settings.theme.unwrap().to_theme().unwrap();
        assert_eq!(
            theme.vu_zones[0].lit,
            shirabe_ui::egui::Color32::from_rgb(0x12, 0x34, 0x56)
        );
        assert_eq!(theme.vu_zones[1].lit, shirabe_ui::egui::Color32::RED);
    }

    #[test]
    fn a_failed_settings_write_reports_why_persistence_failed() {
        let dir = crate::appearance::tests::temp_dir("settings-save");
        let path = dir.join("standalone.json");
        std::fs::create_dir(&path).unwrap();
        let error = Settings::default().save_to(&path).unwrap_err();
        assert!(error.to_string().contains("standalone.json"));
        assert!(path.is_dir());
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
