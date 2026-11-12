//! Persisted shell preferences stored in the Morph home.

use morph_core::settings::{self, TabPosition};
use morph_identity::Paths;
use std::path::PathBuf;

/// Snapshot of `settings.json` for the Flutter shell.
pub struct SettingsView {
    pub tab_position: String,
}

fn resolve_home(home: Option<String>) -> Result<PathBuf, String> {
    let explicit = home.filter(|h| !h.is_empty()).map(PathBuf::from);
    Ok(Paths::resolve(explicit).map_err(|e| e.to_string())?.home)
}

/// Load preferences from the Morph home. A missing file returns defaults.
pub fn load_settings(home: Option<String>) -> Result<SettingsView, String> {
    let home = resolve_home(home)?;
    let settings = settings::load(&home).map_err(|e| e.to_string())?;
    Ok(SettingsView { tab_position: settings.tab_position.as_str().to_string() })
}

/// Persist the tab-strip position (`top`, `bottom`, `left`, or `right`).
pub fn set_tab_position(home: Option<String>, position: String) -> Result<SettingsView, String> {
    let home = resolve_home(home)?;
    let settings = settings::set_tab_position(&home, TabPosition::from_key(&position)).map_err(|e| e.to_string())?;
    Ok(SettingsView { tab_position: settings.tab_position.as_str().to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_defaults_and_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let loaded = load_settings(Some(home.clone())).unwrap();
        assert_eq!(loaded.tab_position, "top");
        assert!(!std::path::Path::new(&home).exists());

        let saved = set_tab_position(Some(home.clone()), "LEFT".into()).unwrap();
        assert_eq!(saved.tab_position, "left");
        let again = load_settings(Some(home)).unwrap();
        assert_eq!(again.tab_position, "left");
    }
}
