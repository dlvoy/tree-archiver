//! Preferences that outlive a session.
//!
//! Stored as `settings.json` in the app config directory. Reading is
//! deliberately forgiving: a missing, unreadable, or malformed file falls back
//! to defaults rather than failing, because a bad preferences file must never
//! stop the app from opening.

use crate::model::sort::SortKey;
use crate::plan::{FileOrder, OutputOptions};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub const SETTINGS_VERSION: u32 = 1;

/// The config directory used before the bundle identifier became
/// `pl.dzienia.treearchiver`. Read once, to carry old preferences over.
const LEGACY_IDENTIFIER: &str = "dev.treearchiver.app";

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

/// Which language the user picked. `System` resolves against the OS display
/// language in the frontend, the same way `ThemePreference::System` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LanguagePreference {
    #[default]
    System,
    En,
    Pl,
    De,
}

/// How the toolbar draws its buttons. Only the labelled groups (sources,
/// plan, sort, selection) are affected — the icon-only buttons on the right
/// (language, theme, settings, about) are icon-only regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InterfaceMode {
    #[default]
    Icons,
    Labels,
    IconsAndLabels,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default)]
    pub language: LanguagePreference,
    #[serde(default)]
    pub sort: SortKey,
    #[serde(default)]
    pub output: OutputOptions,
    /// Absent in settings files written before this existed.
    #[serde(default)]
    pub interface_mode: InterfaceMode,
    /// Absent in settings files written before this existed.
    #[serde(default)]
    pub file_order: FileOrder,
    /// User-imported ignore rulesets. The built-in presets are code-defined
    /// and never stored here, so shipping a new or refined one needs no
    /// migration.
    #[serde(default)]
    pub ignore_rulesets: Vec<crate::ignore_rules::IgnoreRuleset>,
    /// Ids the user has explicitly *unticked*, overriding a ruleset whose own
    /// `default_checked` is `true`.
    ///
    /// Deviations from default are tracked in both directions — this list and
    /// `ignore_rulesets_checked` below — rather than one "the checked set"
    /// list, because a single list can't tell "never touched, falls back to
    /// this ruleset's own default" apart from "explicitly set to the value
    /// that happens to match the default". An empty pair of lists is exactly
    /// the fresh-install state, and a built-in shipped after the user's last
    /// save reads as its own `default_checked` rather than needing a
    /// migration.
    #[serde(default)]
    pub ignore_rulesets_unchecked: Vec<String>,
    /// Ids the user has explicitly *ticked*, overriding a ruleset whose own
    /// `default_checked` is `false`. See `ignore_rulesets_unchecked`.
    #[serde(default)]
    pub ignore_rulesets_checked: Vec<String>,
    /// Whether AutoIgnore's patterns match regardless of case, so `*.bak`
    /// also catches `SOMETHING.BAK`. On by default — most filesystems this
    /// app runs on are themselves case-insensitive, so a case-sensitive
    /// miss would be the surprising outcome, not the safe one.
    #[serde(default = "default_true")]
    pub ignore_case_insensitive: bool,
}

fn default_version() -> u32 {
    SETTINGS_VERSION
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            theme: ThemePreference::default(),
            language: LanguagePreference::default(),
            sort: SortKey::default(),
            output: OutputOptions::default(),
            interface_mode: InterfaceMode::default(),
            file_order: FileOrder::default(),
            ignore_rulesets: Vec::new(),
            ignore_rulesets_unchecked: Vec::new(),
            ignore_rulesets_checked: Vec::new(),
            ignore_case_insensitive: true,
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

/// Where preferences lived before the identifier changed. A sibling of the
/// current directory, since both hang off the same roaming app-data folder.
fn legacy_settings_path(app: &AppHandle) -> Option<PathBuf> {
    let current = settings_path(app).ok()?;
    let parent = current.parent()?.parent()?;
    Some(parent.join(LEGACY_IDENTIFIER).join("settings.json"))
}

/// Never fails. Anything unreadable is reported through the returned defaults.
///
/// Falls back to the pre-rename config directory once, so changing the bundle
/// identifier does not silently reset preferences the user already chose.
pub fn load(app: &AppHandle) -> Settings {
    let Ok(path) = settings_path(app) else {
        return Settings::default();
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            let Some(old) = legacy_settings_path(app) else {
                return Settings::default();
            };
            let Ok(t) = std::fs::read_to_string(&old) else {
                return Settings::default();
            };
            t
        }
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
    use crate::plan::{Compression, FileOrder, PathMode};

    #[test]
    fn defaults_are_the_documented_ones() {
        let s = Settings::default();
        assert_eq!(s.theme, ThemePreference::System);
        assert_eq!(s.language, LanguagePreference::System);
        assert_eq!(s.sort.by, SortBy::Name);
        assert_eq!(s.sort.dir, SortDir::Asc);
        assert_eq!(s.output.path_mode, PathMode::FoldersOnly);
        assert_eq!(s.output.compression, Compression::None);
        assert_eq!(s.output.gzip_level, 6);
        assert_eq!(s.output.sevenz_level, 6);
        assert!(!s.output.sevenz_solid);
        assert_eq!(s.interface_mode, InterfaceMode::Icons);
        assert_eq!(s.file_order, FileOrder::Optimal);
        assert!(s.ignore_rulesets.is_empty());
        assert!(s.ignore_rulesets_unchecked.is_empty());
        assert!(s.ignore_rulesets_checked.is_empty());
        assert!(s.ignore_case_insensitive);
    }

    /// Turning case-insensitive matching off has to survive a save and a
    /// reload, same as every other preference.
    #[test]
    fn case_insensitive_choice_round_trips() {
        let s = Settings {
            ignore_case_insensitive: false,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""ignoreCaseInsensitive":false"#), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(!back.ignore_case_insensitive);
    }

    /// A settings file from before this setting existed carries no key, and
    /// must load as "on" — the documented default — not "off".
    #[test]
    fn a_settings_file_from_before_case_insensitive_still_loads_as_on() {
        let s: Settings = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(s.ignore_case_insensitive);
    }

    /// A user-imported ruleset, and an unticked built-in, both have to
    /// survive a save and a reload.
    #[test]
    fn ignore_rulesets_round_trip() {
        let s = Settings {
            ignore_rulesets: vec![crate::ignore_rules::IgnoreRuleset {
                id: "custom:mine".into(),
                name: "Mine".into(),
                description: "just for me".into(),
                rules: vec!["*.tmp".into()],
                default_checked: true,
            }],
            ignore_rulesets_unchecked: vec!["builtin:logs".into()],
            // A hypothetical default-off ruleset the user turned on by hand.
            ignore_rulesets_checked: vec!["builtin:precompiled".into()],
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""id":"custom:mine""#), "{json}");
        assert!(json.contains(r#""ignoreRulesetsUnchecked":["builtin:logs"]"#), "{json}");
        assert!(json.contains(r#""ignoreRulesetsChecked":["builtin:precompiled"]"#), "{json}");

        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ignore_rulesets, s.ignore_rulesets);
        assert_eq!(back.ignore_rulesets_unchecked, vec!["builtin:logs".to_string()]);
        assert_eq!(back.ignore_rulesets_checked, vec!["builtin:precompiled".to_string()]);
    }

    /// A settings file written before AutoIgnore existed carries neither
    /// key, and an empty unchecked-list must mean "everything checked" —
    /// not "load failed, fall back to defaults".
    #[test]
    fn a_settings_file_from_before_autoignore_still_loads_with_everything_checked() {
        let s: Settings = serde_json::from_str(
            r#"{"version":1,"theme":"dark","sort":{"by":"name","dir":"asc"},
                "output":{"compression":"none","gzipLevel":6}}"#,
        )
        .unwrap();
        assert_eq!(s.theme, ThemePreference::Dark);
        assert!(s.ignore_rulesets.is_empty());
        assert!(s.ignore_rulesets_unchecked.is_empty());
        assert!(s.ignore_rulesets_checked.is_empty());
    }

    /// The interface-mode choice has to survive a save and a reload, the same
    /// as every other preference.
    #[test]
    fn an_interface_mode_choice_round_trips() {
        let s = Settings {
            interface_mode: InterfaceMode::Labels,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""interfaceMode":"labels""#), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.interface_mode, InterfaceMode::Labels);
    }

    /// Same for the archiving-order choice.
    #[test]
    fn a_file_order_choice_round_trips() {
        let s = Settings {
            file_order: FileOrder::Alphabetical,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""fileOrder":"alphabetical""#), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.file_order, FileOrder::Alphabetical);
    }

    /// A settings file written before either setting existed (the v1.3.0
    /// shape) carries neither key, and must still load rather than falling
    /// back to every default wholesale.
    #[test]
    fn a_settings_file_from_before_these_two_settings_still_loads() {
        let s: Settings = serde_json::from_str(
            r#"{"version":1,"theme":"dark","language":"pl",
                "sort":{"by":"size","dir":"desc"},
                "output":{"compression":"gzip","gzipLevel":9,"pathMode":"fullPath"}}"#,
        )
        .unwrap();
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(s.interface_mode, InterfaceMode::Icons);
        assert_eq!(s.file_order, FileOrder::Optimal);
    }

    /// A 7z choice has to survive a save and a reload, including the two
    /// settings that only 7z uses.
    #[test]
    fn a_sevenz_choice_round_trips() {
        let s = Settings {
            output: OutputOptions {
                compression: Compression::SevenZ,
                sevenz_level: 9,
                sevenz_solid: true,
                ..OutputOptions::default()
            },
            ..Settings::default()
        };

        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""compression":"7z""#), "{json}");

        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.output.compression, Compression::SevenZ);
        assert_eq!(back.output.sevenz_level, 9);
        assert!(back.output.sevenz_solid);
    }

    /// A settings file written by 1.2.1 predates 7z entirely. The missing keys
    /// must fall back rather than making the file unreadable: load() discards
    /// the whole thing on a parse failure.
    #[test]
    fn a_settings_file_from_before_sevenz_still_loads() {
        let s: Settings = serde_json::from_str(
            r#"{"version":1,"theme":"dark","language":"pl",
                "sort":{"by":"name","dir":"asc"},
                "output":{"compression":"gzip","gzipLevel":9,"pathMode":"fullPath"}}"#,
        )
        .unwrap();
        assert_eq!(s.output.compression, Compression::Gzip);
        assert_eq!(s.output.gzip_level, 9);
        assert_eq!(s.output.sevenz_level, 6);
        assert!(!s.output.sevenz_solid);
    }

    #[test]
    fn a_count_sort_choice_round_trips() {
        let s = Settings {
            sort: SortKey { by: SortBy::Count, dir: SortDir::Desc },
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""sort":{"by":"count","dir":"desc"}"#), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sort.by, SortBy::Count);
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
                ..OutputOptions::default()
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

    /// A file written before the identifier changed carries no `language`, and
    /// must still load rather than being discarded wholesale.
    #[test]
    fn a_settings_file_from_before_the_rename_still_loads() {
        let s: Settings = serde_json::from_str(
            r#"{"version":1,"theme":"dark","sort":{"by":"size","dir":"desc"},
                "output":{"compression":"none","gzipLevel":6}}"#,
        )
        .unwrap();
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(s.sort.by, SortBy::Size);
        assert_eq!(s.language, LanguagePreference::System);
    }

    #[test]
    fn a_language_choice_round_trips() {
        let s = Settings {
            language: LanguagePreference::Pl,
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""language":"pl""#), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.language, LanguagePreference::Pl);
    }
}
