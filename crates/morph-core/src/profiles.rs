//! Chrome-style browser profiles with fully isolated storage.
//!
//! There is no cap on how many profiles a Morph home can hold. Each profile
//! has a stable filesystem-safe `id` and a user-visible `name`. Data lives
//! under `<home>/profiles/<id>/` and is never shared across profiles:
//!
//! ```text
//! <home>/profiles.json
//! <home>/profiles/<id>/settings.json
//! <home>/profiles/<id>/tabs.json
//! <home>/profiles/<id>/webkit/cookies.sqlite
//! <home>/profiles/<id>/webkit/data/      // localStorage, IndexedDB, service workers, WebSQL, HSTS
//! <home>/profiles/<id>/webkit/cache/     // HTTP cache and Cache Storage
//! ```
//!
//! The first profile is always `default`. Existing `<home>/settings.json` and
//! the earlier `<home>/profile/default/` WebKit tree are migrated into it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Identifier of the profile created on first run and used for migration.
pub const DEFAULT_PROFILE_ID: &str = "default";
/// Display name of [DEFAULT_PROFILE_ID] until the user renames it.
pub const DEFAULT_PROFILE_NAME: &str = "Default";

/// A Chrome-style user profile. Storage is keyed by [Self::id], not [Self::name].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
}

/// On-disk catalog of profiles and which one is current.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub current: String,
    pub profiles: Vec<Profile>,
}

impl Catalog {
    fn fresh_default() -> Self {
        Self {
            current: DEFAULT_PROFILE_ID.to_string(),
            profiles: vec![Profile { id: DEFAULT_PROFILE_ID.to_string(), name: DEFAULT_PROFILE_NAME.to_string() }],
        }
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub fn current(&self) -> &Profile {
        self.get(&self.current).or_else(|| self.profiles.first()).expect("catalog always has a profile")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("read or write profiles: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse profiles: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("unknown profile {0}")]
    Unknown(String),
    #[error("invalid profile id {0}")]
    InvalidId(String),
    #[error("profile name must not be empty")]
    EmptyName,
    #[error("cannot delete the last profile")]
    LastProfile,
}

/// `<home>/profiles.json`.
pub fn catalog_path(home: impl AsRef<Path>) -> PathBuf {
    home.as_ref().join("profiles.json")
}

/// `<home>/profiles/<id>`.
pub fn profile_dir(home: impl AsRef<Path>, id: &str) -> PathBuf {
    home.as_ref().join("profiles").join(id)
}

/// `<home>/profiles/<id>/settings.json`.
pub fn settings_path(home: impl AsRef<Path>, id: &str) -> PathBuf {
    profile_dir(home, id).join("settings.json")
}

/// `<home>/profiles/<id>/tabs.json`. Reserved for per-profile tab persistence.
pub fn tabs_path(home: impl AsRef<Path>, id: &str) -> PathBuf {
    profile_dir(home, id).join("tabs.json")
}

/// `<home>/profiles/<id>/webkit`.
pub fn webkit_dir(home: impl AsRef<Path>, id: &str) -> PathBuf {
    profile_dir(home, id).join("webkit")
}

/// `<home>/profiles/<id>/webkit/data`.
pub fn webkit_data_dir(home: impl AsRef<Path>, id: &str) -> PathBuf {
    webkit_dir(home, id).join("data")
}

/// `<home>/profiles/<id>/webkit/cache`.
pub fn webkit_cache_dir(home: impl AsRef<Path>, id: &str) -> PathBuf {
    webkit_dir(home, id).join("cache")
}

/// `<home>/profiles/<id>/webkit/cookies.sqlite`.
pub fn cookies_path(home: impl AsRef<Path>, id: &str) -> PathBuf {
    webkit_dir(home, id).join("cookies.sqlite")
}

/// A profile id is a short filesystem-safe token, never a path.
pub fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if id.len() > 64 || !first.is_ascii_alphanumeric() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Load the catalog, creating `default` and migrating leftover data if needed.
///
/// This may create `<home>/profiles.json` and move files that already exist
/// under [home]. It is idempotent.
pub fn load_catalog(home: impl AsRef<Path>) -> Result<Catalog, ProfileError> {
    let home = home.as_ref();
    migrate(home)?;
    let path = catalog_path(home);
    let raw = fs::read_to_string(path)?;
    let catalog: Catalog = serde_json::from_str(&raw)?;
    if catalog.profiles.is_empty() || catalog.get(&catalog.current).is_none() {
        let mut catalog = catalog;
        if catalog.profiles.is_empty() {
            catalog = Catalog::fresh_default();
        } else if catalog.get(&catalog.current).is_none() {
            catalog.current = catalog.profiles[0].id.clone();
        }
        save_catalog(home, &catalog)?;
        return Ok(catalog);
    }
    Ok(catalog)
}

/// Profiles in catalog order.
pub fn list(home: impl AsRef<Path>) -> Result<Vec<Profile>, ProfileError> {
    Ok(load_catalog(home)?.profiles)
}

/// The current profile.
pub fn current(home: impl AsRef<Path>) -> Result<Profile, ProfileError> {
    Ok(load_catalog(home)?.current().clone())
}

/// Switch the current profile. The id must already exist.
pub fn set_current(home: impl AsRef<Path>, id: &str) -> Result<Profile, ProfileError> {
    validate_id(id)?;
    let home = home.as_ref();
    let mut catalog = load_catalog(home)?;
    let profile = catalog.get(id).cloned().ok_or_else(|| ProfileError::Unknown(id.to_string()))?;
    catalog.current = profile.id.clone();
    save_catalog(home, &catalog)?;
    Ok(profile)
}

/// Create a new isolated profile and make it current. The id is generated and
/// is not derived from [name], so a later rename does not move storage.
pub fn create(home: impl AsRef<Path>, name: &str) -> Result<Profile, ProfileError> {
    let name = normalize_name(name)?;
    let home = home.as_ref();
    let mut catalog = load_catalog(home)?;
    let profile = Profile { id: next_id(&catalog), name };
    ensure_profile_dirs(home, &profile.id)?;
    catalog.profiles.push(profile.clone());
    catalog.current = profile.id.clone();
    save_catalog(home, &catalog)?;
    Ok(profile)
}

/// Change the display name of an existing profile. The id and directory stay put.
pub fn rename(home: impl AsRef<Path>, id: &str, name: &str) -> Result<Profile, ProfileError> {
    validate_id(id)?;
    let name = normalize_name(name)?;
    let home = home.as_ref();
    let mut catalog = load_catalog(home)?;
    let profile = catalog.profiles.iter_mut().find(|profile| profile.id == id).ok_or_else(|| ProfileError::Unknown(id.to_string()))?;
    profile.name = name;
    let result = profile.clone();
    save_catalog(home, &catalog)?;
    Ok(result)
}

/// Delete a profile and its on-disk storage. Refuses to delete the last one.
/// If it was current, the first remaining profile (preferring `default`) becomes current.
pub fn delete(home: impl AsRef<Path>, id: &str) -> Result<Catalog, ProfileError> {
    validate_id(id)?;
    let home = home.as_ref();
    let mut catalog = load_catalog(home)?;
    if catalog.get(id).is_none() {
        return Err(ProfileError::Unknown(id.to_string()));
    }
    if catalog.profiles.len() == 1 {
        return Err(ProfileError::LastProfile);
    }
    catalog.profiles.retain(|profile| profile.id != id);
    if catalog.current == id {
        catalog.current = preferred_current(&catalog.profiles);
    }
    save_catalog(home, &catalog)?;
    let dir = profile_dir(home, id);
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(catalog)
}

/// Move leftover pre-profile files into `profiles/default/` and ensure the catalog exists.
pub fn migrate(home: impl AsRef<Path>) -> Result<Catalog, ProfileError> {
    let home = home.as_ref();
    let path = catalog_path(home);
    let catalog = if path.exists() { serde_json::from_str(&fs::read_to_string(&path)?)? } else { Catalog::fresh_default() };
    let catalog = if catalog.profiles.is_empty() { Catalog::fresh_default() } else { catalog };

    // Move leftover data before creating empty webkit placeholders, so a
    // just-created data/cache pair does not block the rename.
    fs::create_dir_all(profile_dir(home, DEFAULT_PROFILE_ID))?;
    let root_settings = home.join("settings.json");
    let default_settings = settings_path(home, DEFAULT_PROFILE_ID);
    if root_settings.exists() && !default_settings.exists() {
        fs::rename(&root_settings, &default_settings)?;
    }
    migrate_legacy_webkit(home.join("profile").join(DEFAULT_PROFILE_ID), profile_dir(home, DEFAULT_PROFILE_ID))?;

    ensure_profile_dirs(home, DEFAULT_PROFILE_ID)?;
    for profile in &catalog.profiles {
        if is_valid_id(&profile.id) {
            ensure_profile_dirs(home, &profile.id)?;
        }
    }

    // Older cookie-fix layout stored WebKit files under profile/default/webkit
    // or, if someone already created profile/default as the webkit root, leave it
    // after the rename above.

    save_catalog(home, &catalog)?;
    Ok(catalog)
}

fn migrate_legacy_webkit(old_dir: PathBuf, dest_profile_dir: PathBuf) -> Result<(), ProfileError> {
    if !old_dir.exists() {
        return Ok(());
    }
    // Cookie-fix path: <home>/profile/default/webkit or <home>/profile/default/{data,cache,cookies.sqlite}
    let old_webkit = if old_dir.join("webkit").is_dir() { old_dir.join("webkit") } else { old_dir.clone() };
    let dest_webkit = dest_profile_dir.join("webkit");
    if old_webkit.exists() && !dest_has_webkit_data(&dest_webkit) {
        if dest_webkit.exists() {
            // Empty placeholder: replace it.
            if dest_webkit.read_dir()?.next().is_none() {
                fs::remove_dir(&dest_webkit)?;
                fs::rename(&old_webkit, &dest_webkit)?;
            }
        } else {
            fs::create_dir_all(&dest_profile_dir)?;
            fs::rename(&old_webkit, &dest_webkit)?;
        }
    }
    // Drop <home>/profile if it is now empty (and the default folder too).
    let _ = remove_if_empty(&old_dir);
    if let Some(parent) = old_dir.parent() {
        let _ = remove_if_empty(parent);
    }
    Ok(())
}

fn dest_has_webkit_data(webkit: &Path) -> bool {
    webkit.join("cookies.sqlite").exists()
        || webkit.join("data").exists() && dir_nonempty(&webkit.join("data"))
        || webkit.join("cache").exists() && dir_nonempty(&webkit.join("cache"))
}

fn dir_nonempty(path: &Path) -> bool {
    path.read_dir().map(|mut entries| entries.next().is_some()).unwrap_or(false)
}

fn remove_if_empty(path: &Path) -> std::io::Result<()> {
    if path.is_dir() && !dir_nonempty(path) {
        fs::remove_dir(path)?;
    }
    Ok(())
}

fn ensure_profile_dirs(home: &Path, id: &str) -> Result<(), ProfileError> {
    fs::create_dir_all(webkit_data_dir(home, id))?;
    fs::create_dir_all(webkit_cache_dir(home, id))?;
    Ok(())
}

fn save_catalog(home: &Path, catalog: &Catalog) -> Result<(), ProfileError> {
    fs::create_dir_all(home)?;
    let path = catalog_path(home);
    let tmp = home.join("profiles.json.tmp");
    let body = serde_json::to_vec_pretty(catalog)?;
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&body)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

fn next_id(catalog: &Catalog) -> String {
    let mut n = 1u64;
    loop {
        let id = format!("p{n}");
        if catalog.get(&id).is_none() {
            return id;
        }
        n += 1;
    }
}

fn preferred_current(profiles: &[Profile]) -> String {
    profiles
        .iter()
        .find(|profile| profile.id == DEFAULT_PROFILE_ID)
        .or_else(|| profiles.first())
        .map(|profile| profile.id.clone())
        .expect("at least one profile")
}

fn validate_id(id: &str) -> Result<(), ProfileError> {
    if is_valid_id(id) { Ok(()) } else { Err(ProfileError::InvalidId(id.to_string())) }
}

fn normalize_name(name: &str) -> Result<String, ProfileError> {
    let name = name.trim();
    if name.is_empty() { Err(ProfileError::EmptyName) } else { Ok(name.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn first_load_creates_default_without_a_cap() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("morph");
        let catalog = load_catalog(&home).unwrap();
        assert_eq!(catalog.current, DEFAULT_PROFILE_ID);
        assert_eq!(catalog.profiles.len(), 1);
        assert_eq!(catalog.profiles[0].name, DEFAULT_PROFILE_NAME);
        assert!(webkit_data_dir(&home, DEFAULT_PROFILE_ID).is_dir());
        assert!(catalog_path(&home).exists());
    }

    #[test]
    fn create_rename_delete_and_switch_unlimited_profiles() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("morph");
        let work = create(&home, "Work").unwrap();
        let school = create(&home, "  School ").unwrap();
        let extra = create(&home, "Extra").unwrap();
        assert_eq!(work.id, "p1");
        assert_eq!(school.id, "p2");
        assert_eq!(extra.id, "p3");
        assert_eq!(list(&home).unwrap().len(), 4);
        assert_eq!(current(&home).unwrap().id, "p3");

        let renamed = rename(&home, "p1", "Office").unwrap();
        assert_eq!(renamed.id, "p1");
        assert_eq!(renamed.name, "Office");
        assert_eq!(set_current(&home, "p1").unwrap().name, "Office");
        assert_eq!(current(&home).unwrap().id, "p1");

        delete(&home, "p3").unwrap();
        let ids: Vec<_> = list(&home).unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec!["default".to_string(), "p1".to_string(), "p2".to_string()]);

        // Deleting the current profile falls back to default.
        set_current(&home, "p2").unwrap();
        delete(&home, "p2").unwrap();
        assert_eq!(current(&home).unwrap().id, DEFAULT_PROFILE_ID);
        assert!(!profile_dir(&home, "p2").exists());
    }

    #[test]
    fn cannot_delete_the_last_profile_or_use_bad_ids() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("morph");
        load_catalog(&home).unwrap();
        assert!(matches!(delete(&home, DEFAULT_PROFILE_ID), Err(ProfileError::LastProfile)));
        assert!(matches!(set_current(&home, "missing"), Err(ProfileError::Unknown(_))));
        assert!(matches!(rename(&home, "../etc", "x"), Err(ProfileError::InvalidId(_))));
        assert!(matches!(create(&home, "  "), Err(ProfileError::EmptyName)));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("../etc"));
        assert!(!is_valid_id("a/b"));
        assert!(!is_valid_id("has space"));
        assert!(is_valid_id("default"));
        assert!(is_valid_id("p-12_b"));
    }

    #[test]
    fn directories_are_isolated_for_three_profiles() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("morph");
        load_catalog(&home).unwrap();
        let a = DEFAULT_PROFILE_ID;
        let b = create(&home, "B").unwrap().id;
        let c = create(&home, "C").unwrap().id;
        fs::write(webkit_data_dir(&home, a).join("marker.txt"), "A").unwrap();
        fs::write(webkit_data_dir(&home, &b).join("marker.txt"), "B").unwrap();
        fs::write(webkit_data_dir(&home, &c).join("marker.txt"), "C").unwrap();
        assert_ne!(webkit_data_dir(&home, a), webkit_data_dir(&home, &b));
        assert_ne!(webkit_dir(&home, &b), webkit_dir(&home, &c));
        assert_eq!(fs::read_to_string(webkit_data_dir(&home, a).join("marker.txt")).unwrap(), "A");
        assert_eq!(fs::read_to_string(webkit_data_dir(&home, &b).join("marker.txt")).unwrap(), "B");
        assert_eq!(fs::read_to_string(webkit_data_dir(&home, &c).join("marker.txt")).unwrap(), "C");
        assert!(!webkit_data_dir(&home, a).join("marker.txt").starts_with(profile_dir(&home, &b)));
    }

    #[test]
    fn migrates_legacy_settings_and_webkit_tree() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("morph");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("settings.json"), "{\"tab_position\":\"left\"}\n").unwrap();
        let old_webkit = home.join("profile").join("default").join("webkit");
        fs::create_dir_all(old_webkit.join("data")).unwrap();
        fs::write(old_webkit.join("cookies.sqlite"), b"cookies").unwrap();
        fs::write(old_webkit.join("data").join("localstorage"), b"ls").unwrap();

        let catalog = load_catalog(&home).unwrap();
        assert_eq!(catalog.current, DEFAULT_PROFILE_ID);
        assert!(!home.join("settings.json").exists());
        assert_eq!(fs::read_to_string(settings_path(&home, DEFAULT_PROFILE_ID)).unwrap(), "{\"tab_position\":\"left\"}\n");
        assert_eq!(fs::read(cookies_path(&home, DEFAULT_PROFILE_ID)).unwrap(), b"cookies");
        assert_eq!(fs::read(webkit_data_dir(&home, DEFAULT_PROFILE_ID).join("localstorage")).unwrap(), b"ls");
        assert!(!home.join("profile").exists());
        // Second load is a no-op.
        load_catalog(&home).unwrap();
        assert_eq!(fs::read(cookies_path(&home, DEFAULT_PROFILE_ID)).unwrap(), b"cookies");
    }

    #[test]
    fn next_ids_are_unbounded() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("morph");
        for i in 1..=8 {
            let created = create(&home, &format!("P{i}")).unwrap();
            assert_eq!(created.id, format!("p{i}"));
        }
        assert_eq!(list(&home).unwrap().len(), 9);
    }
}
