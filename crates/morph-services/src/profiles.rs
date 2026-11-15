//! Profile changes with side effects outside `morph_core::profiles`.

use std::path::Path;

use morph_core::library_db::{self, LibraryState};
use morph_core::profiles::{self, Catalog};

use crate::{Error, Result};

/// Delete profile `id` and its storage, and drop its library key from the
/// system keystore first (the entry is found by the profile's directory).
/// Refuses to delete the last profile. Returns the catalog after.
pub fn delete(home: &Path, id: &str) -> Result<Catalog> {
    forget_library_key(home, id);
    profiles::delete(home, id).map_err(Error::failed)?;
    profiles::load_catalog(home).map_err(Error::failed)
}

/// Only profiles that ever had an identity or an encrypted library can have
/// a saved key.
fn forget_library_key(home: &Path, id: &str) {
    if profiles::require(home, id).is_err() {
        return;
    }
    let dir = profiles::profile_dir(home, id);
    let encrypted = library_db::state(home, id).is_ok_and(|s| s != LibraryState::Plain);
    let had_identity = morph_identity::profile_identity::identity_dir(&dir).exists();
    if (had_identity || encrypted)
        && let Err(e) = morph_keystore::forget(&dir)
    {
        eprintln!("Morph: library key of deleted profile {id} not removed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretString;
    use morph_identity::profile_identity;

    fn user_id(home: &Path, id: &str) -> String {
        profile_identity::load(&profiles::profile_dir(home, id)).unwrap().unwrap().user_id
    }

    #[test]
    fn deleting_a_profile_removes_its_saved_key_only() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::create(&home, "Work").unwrap();
        crate::identity::create(&home, "p1", &SecretString::from("long enough".to_string())).unwrap();
        crate::identity::create(&home, "default", &SecretString::from("other enough".to_string())).unwrap();
        let (work, default) = (profiles::profile_dir(&home, "p1"), profiles::profile_dir(&home, "default"));
        let (work_user, default_user) = (user_id(&home, "p1"), user_id(&home, "default"));
        let after = delete(&home, "p1").unwrap();
        assert!(after.get("p1").is_none());
        assert!(!work.exists());
        std::fs::create_dir_all(&work).unwrap();
        assert!(morph_keystore::load(&work, &work_user).unwrap().is_none());
        assert!(morph_keystore::load(&default, &default_user).unwrap().is_some());
    }

    #[test]
    fn the_last_profile_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        assert!(delete(&home, "default").is_err());
        assert!(delete(&home, "nope").is_err());
    }
}
