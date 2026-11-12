//! Chrome-style user profiles exposed to the Flutter shell.

use morph_core::profiles::{self, Catalog, Profile};
use morph_identity::Paths;
use std::path::PathBuf;

/// A single profile in the catalog. Storage is keyed by [Self::id].
pub struct ProfileView {
    pub id: String,
    pub name: String,
}

impl ProfileView {
    fn from(profile: &Profile) -> Self {
        Self { id: profile.id.clone(), name: profile.name.clone() }
    }
}

/// The full profile catalog, including which profile is current.
pub struct CatalogView {
    pub current: String,
    pub profiles: Vec<ProfileView>,
}

impl CatalogView {
    fn from(catalog: &Catalog) -> Self {
        Self { current: catalog.current.clone(), profiles: catalog.profiles.iter().map(ProfileView::from).collect() }
    }
}

/// On-disk paths for one profile, for platform web views to isolate storage.
pub struct ProfilePathsView {
    pub id: String,
    pub profile_dir: String,
    pub settings_path: String,
    pub tabs_path: String,
    pub webkit_dir: String,
    pub webkit_data_dir: String,
    pub webkit_cache_dir: String,
    pub cookies_path: String,
}

fn resolve_home(home: Option<String>) -> Result<PathBuf, String> {
    let explicit = home.filter(|h| !h.is_empty()).map(PathBuf::from);
    Ok(Paths::resolve(explicit).map_err(|e| e.to_string())?.home)
}

fn load_view(home: &std::path::Path) -> Result<CatalogView, String> {
    let catalog = profiles::load_catalog(home).map_err(|e| e.to_string())?;
    Ok(CatalogView::from(&catalog))
}

/// List profiles and the current id. Creates the default catalog on first use.
pub fn list_profiles(home: Option<String>) -> Result<CatalogView, String> {
    let home = resolve_home(home)?;
    load_view(&home)
}

/// Create a new isolated profile, make it current, and return the catalog.
pub fn create_profile(home: Option<String>, name: String) -> Result<CatalogView, String> {
    let home = resolve_home(home)?;
    profiles::create(&home, &name).map_err(|e| e.to_string())?;
    load_view(&home)
}

/// Rename a profile. The id and storage directory stay the same.
pub fn rename_profile(home: Option<String>, id: String, name: String) -> Result<CatalogView, String> {
    let home = resolve_home(home)?;
    profiles::rename(&home, &id, &name).map_err(|e| e.to_string())?;
    load_view(&home)
}

/// Delete a profile and its storage. Refuses to delete the last profile.
pub fn delete_profile(home: Option<String>, id: String) -> Result<CatalogView, String> {
    let home = resolve_home(home)?;
    profiles::delete(&home, &id).map_err(|e| e.to_string())?;
    load_view(&home)
}

/// Switch the current profile. Existing web views should be recreated by the shell.
pub fn set_current_profile(home: Option<String>, id: String) -> Result<CatalogView, String> {
    let home = resolve_home(home)?;
    profiles::set_current(&home, &id).map_err(|e| e.to_string())?;
    load_view(&home)
}

/// Absolute paths the Linux (and later other) web views should use for [id].
pub fn profile_paths(home: Option<String>, id: String) -> Result<ProfilePathsView, String> {
    let home = resolve_home(home)?;
    let catalog = profiles::load_catalog(&home).map_err(|e| e.to_string())?;
    if catalog.get(&id).is_none() {
        return Err(format!("unknown profile {id}"));
    }
    Ok(ProfilePathsView {
        id: id.clone(),
        profile_dir: profiles::profile_dir(&home, &id).display().to_string(),
        settings_path: profiles::settings_path(&home, &id).display().to_string(),
        tabs_path: profiles::tabs_path(&home, &id).display().to_string(),
        webkit_dir: profiles::webkit_dir(&home, &id).display().to_string(),
        webkit_data_dir: profiles::webkit_data_dir(&home, &id).display().to_string(),
        webkit_cache_dir: profiles::webkit_cache_dir(&home, &id).display().to_string(),
        cookies_path: profiles::cookies_path(&home, &id).display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_starts_with_default_and_grows_without_a_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let first = list_profiles(Some(home.clone())).unwrap();
        assert_eq!(first.current, "default");
        assert_eq!(first.profiles.len(), 1);
        assert_eq!(first.profiles[0].name, "Default");

        let work = create_profile(Some(home.clone()), "Work".into()).unwrap();
        assert_eq!(work.current, "p1");
        assert_eq!(work.profiles.len(), 2);
        let extra = create_profile(Some(home.clone()), "Extra".into()).unwrap();
        assert_eq!(extra.current, "p2");
        assert_eq!(extra.profiles.len(), 3);

        let renamed = rename_profile(Some(home.clone()), "p1".into(), "Office".into()).unwrap();
        assert_eq!(renamed.profiles.iter().find(|p| p.id == "p1").unwrap().name, "Office");

        let switched = set_current_profile(Some(home.clone()), "default".into()).unwrap();
        assert_eq!(switched.current, "default");

        let paths = profile_paths(Some(home.clone()), "p1".into()).unwrap();
        assert!(paths.webkit_dir.contains("/profiles/p1/webkit"));
        assert!(paths.cookies_path.ends_with("cookies.sqlite"));
        let other = profile_paths(Some(home.clone()), "p2".into()).unwrap();
        assert_ne!(paths.webkit_dir, other.webkit_dir);

        let after = delete_profile(Some(home.clone()), "p1".into()).unwrap();
        assert_eq!(after.profiles.len(), 2);
        assert!(after.profiles.iter().all(|p| p.id != "p1"));
        assert!(profile_paths(Some(home), "p1".into()).is_err());
    }
}
