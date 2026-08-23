//! Preferences that outlive a session.
//!
//! Stored as `settings.json` in the app config directory. Reading is
//! deliberately forgiving: a missing, unreadable, or malformed file falls back
//! to defaults rather than failing, because a bad preferences file must never
//! stop the app from opening.

use crate::model::sort::SortKey;
use crate::plan::OutputOptions;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub const SETTINGS_VERSION: u32 = 1;

/// Which theme the user picked, as opposed to which one is showing. `System`
/// resolves against the OS setting in the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default)]
    pub sort: SortKey,
    #[serde(default)]
    pub output: OutputOptions,
}

fn default_version() -> u32 {
    SETTINGS_VERSION
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            theme: ThemePreference::default(),
            sort: SortKey::default(),
            output: OutputOptions::default(),
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("no config directory: {e}"))?;
    Ok(dir.join("settings.json"))
}

/// Never fails. Anything unreadable is reported through the returned defaults.
pub fn load(app: &AppHandle) -> Settings {
    let Ok(path) = settings_path(app) else {
        return Settings::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Settings::default();
    };
    // Every field carries a serde default, so a file written by an older
    // version still loads and only the missing keys fall back.
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create the config directory: {e}"))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("could not save settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::sort::{SortBy, SortDir};
    use crate::plan::{Compression, PathMode};

    #[test]
    fn defaults_are_the_documented_ones() {
        let s = Settings::default();
        assert_eq!(s.theme, ThemePreference::System);
        assert_eq!(s.sort.by, SortBy::Name);
        assert_eq!(s.sort.dir, SortDir::Asc);
        assert_eq!(s.output.path_mode, PathMode::FoldersOnly);
        assert_eq!(s.output.compression, Compression::None);
        assert_eq!(s.output.gzip_level, 6);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let s = Settings {
            theme: ThemePreference::Dark,
            sort: SortKey {
                by: SortBy::Size,
                dir: SortDir::Desc,
            },
            output: OutputOptions {
                compression: Compression::Gzip,
                gzip_level: 9,
                path_mode: PathMode::FullPath,
            },
            ..Settings::default()
        };

        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.theme, ThemePreference::Dark);
        assert_eq!(back.sort.by, SortBy::Size);
        assert_eq!(back.output.gzip_level, 9);
        assert_eq!(back.output.path_mode, PathMode::FullPath);
    }

    #[test]
    fn a_partial_file_fills_the_rest_from_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(s.sort.by, SortBy::Name);
        assert_eq!(s.output.path_mode, PathMode::FoldersOnly);
    }

    #[test]
    fn a_settings_file_without_a_path_mode_still_loads() {
        let s: Settings = serde_json::from_str(
            r#"{"version":1,"theme":"light","output":{"compression":"gzip","gzipLevel":3}}"#,
        )
        .unwrap();
        assert_eq!(s.output.compression, Compression::Gzip);
        assert_eq!(s.output.gzip_level, 3);
        assert_eq!(s.output.path_mode, PathMode::FoldersOnly);
    }

    #[test]
    fn malformed_json_is_rejected_so_load_can_fall_back() {
        assert!(serde_json::from_str::<Settings>("{ not json").is_err());
    }
}
