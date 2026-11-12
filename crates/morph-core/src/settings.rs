//! Persisted Morph shell preferences, stored as `settings.json` in the Morph home.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

/// Where the tab strip sits around the web view. Default: top.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TabPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl TabPosition {
    /// Parse a stored or UI value. Unknown values fall back to [Self::Top].
    pub fn from_key(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "bottom" => Self::Bottom,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => Self::Top,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

impl<'de> Deserialize<'de> for TabPosition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Option::<String>::deserialize(deserializer)?;
        Ok(raw.as_deref().map(Self::from_key).unwrap_or_default())
    }
}

/// User preferences kept under the Morph home directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub tab_position: TabPosition,
    /// Whether the left/right tab list is collapsed to favicons. Default: false.
    #[serde(default)]
    pub sidebar_collapsed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("read or write settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse settings: {0}")]
    Parse(#[from] serde_json::Error),
}

/// `settings.json` inside [home].
pub fn settings_path(home: impl AsRef<Path>) -> PathBuf {
    home.as_ref().join("settings.json")
}

/// Load preferences from [home]. A missing file returns defaults.
///
/// This never creates the home directory.
pub fn load(home: impl AsRef<Path>) -> Result<Settings, SettingsError> {
    let path = settings_path(home);
    if !path.exists() {
        return Ok(Settings::default());
    }
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

/// Write [settings] to [home], creating the directory if needed.
pub fn save(home: impl AsRef<Path>, settings: &Settings) -> Result<(), SettingsError> {
    let home = home.as_ref();
    fs::create_dir_all(home)?;
    let path = settings_path(home);
    let tmp = home.join("settings.json.tmp");
    let body = serde_json::to_vec_pretty(settings)?;
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&body)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

/// Load, set the tab position, and persist it.
pub fn set_tab_position(home: impl AsRef<Path>, position: TabPosition) -> Result<Settings, SettingsError> {
    let mut settings = load(&home)?;
    settings.tab_position = position;
    save(&home, &settings)?;
    Ok(settings)
}

/// Load, set whether the left/right tab list is collapsed, and persist it.
pub fn set_sidebar_collapsed(home: impl AsRef<Path>, collapsed: bool) -> Result<Settings, SettingsError> {
    let mut settings = load(&home)?;
    settings.sidebar_collapsed = collapsed;
    save(&home, &settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_loads_as_top() {
        let tmp = tempdir().unwrap();
        let settings = load(tmp.path()).unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.tab_position, TabPosition::Top);
        assert!(!settings_path(tmp.path()).exists());
        assert!(tmp.path().read_dir().unwrap().next().is_none());
    }

    #[test]
    fn save_and_load_round_trips_every_position() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("morph-home");
        for position in [TabPosition::Top, TabPosition::Bottom, TabPosition::Left, TabPosition::Right] {
            let saved = set_tab_position(&home, position).unwrap();
            assert_eq!(saved.tab_position, position);
            let loaded = load(&home).unwrap();
            assert_eq!(loaded.tab_position, position);
            let raw = fs::read_to_string(settings_path(&home)).unwrap();
            assert!(raw.contains(&format!("\"tab_position\": \"{}\"", position.as_str())), "{raw}");
        }
    }

    #[test]
    fn unknown_or_empty_position_falls_back_to_top() {
        let tmp = tempdir().unwrap();
        fs::write(settings_path(tmp.path()), "{\"tab_position\":\"sideways\"}\n").unwrap();
        assert_eq!(load(tmp.path()).unwrap().tab_position, TabPosition::Top);

        fs::write(settings_path(tmp.path()), "{\"tab_position\":\"\"}\n").unwrap();
        assert_eq!(load(tmp.path()).unwrap().tab_position, TabPosition::Top);

        fs::write(settings_path(tmp.path()), "{}\n").unwrap();
        assert_eq!(load(tmp.path()).unwrap().tab_position, TabPosition::Top);
    }

    #[test]
    fn extra_fields_are_ignored() {
        let tmp = tempdir().unwrap();
        fs::write(settings_path(tmp.path()), "{\"tab_position\":\"left\",\"future_flag\":true}\n").unwrap();
        assert_eq!(load(tmp.path()).unwrap().tab_position, TabPosition::Left);
    }

    #[test]
    fn sidebar_collapsed_defaults_false_and_round_trips() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("morph-home");
        assert!(!load(&home).unwrap().sidebar_collapsed);

        let saved = set_sidebar_collapsed(&home, true).unwrap();
        assert!(saved.sidebar_collapsed);
        assert_eq!(saved.tab_position, TabPosition::Top);
        let loaded = load(&home).unwrap();
        assert!(loaded.sidebar_collapsed);
        let raw = fs::read_to_string(settings_path(&home)).unwrap();
        assert!(raw.contains("\"sidebar_collapsed\": true"), "{raw}");

        let saved = set_sidebar_collapsed(&home, false).unwrap();
        assert!(!saved.sidebar_collapsed);
        assert!(!load(&home).unwrap().sidebar_collapsed);
    }

    #[test]
    fn missing_sidebar_collapsed_field_defaults_false() {
        let tmp = tempdir().unwrap();
        fs::write(settings_path(tmp.path()), "{\"tab_position\":\"left\"}\n").unwrap();
        let settings = load(tmp.path()).unwrap();
        assert_eq!(settings.tab_position, TabPosition::Left);
        assert!(!settings.sidebar_collapsed);
    }

    #[test]
    fn from_key_parses_known_values() {
        assert_eq!(TabPosition::from_key("TOP"), TabPosition::Top);
        assert_eq!(TabPosition::from_key(" bottom "), TabPosition::Bottom);
        assert_eq!(TabPosition::from_key("Left"), TabPosition::Left);
        assert_eq!(TabPosition::from_key("right"), TabPosition::Right);
        assert_eq!(TabPosition::from_key("nope"), TabPosition::Top);
        assert_eq!(TabPosition::Top.as_str(), "top");
    }
}
