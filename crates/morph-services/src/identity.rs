//! A profile's Weirdnet identity: onboarding (create, import from words,
//! restore from a backup), recovery words and backups. Every way a profile
//! gets an identity also encrypts its library and keeps the key
//! ([`crate::library::adopt_new_identity`]).

use std::path::Path;

use morph_identity::profile_identity::{self, ProfileIdentity, SecretString, Zeroizing};

use crate::home::{now_ms, profile_dir};
use crate::library::adopt_new_identity;
use crate::{Error, Result};

/// A new identity and the 24 recovery words to show the user once.
pub struct NewIdentity {
    pub identity: ProfileIdentity,
    pub words: Zeroizing<String>,
}

/// The identity of `profile_id`, or `None` before its onboarding.
pub fn load(home: &Path, profile_id: &str) -> Result<Option<ProfileIdentity>> {
    let dir = profile_dir(home, profile_id)?;
    profile_identity::load(&dir).map_err(Error::failed)
}

/// Create `profile_id`'s identity, sealed under `password`.
pub fn create(home: &Path, profile_id: &str, password: &SecretString) -> Result<NewIdentity> {
    let dir = profile_dir(home, profile_id)?;
    let created = profile_identity::create(&dir, password, now_ms()).map_err(Error::failed)?;
    adopt_new_identity(home, profile_id, &created.identity.user_id, &created.library_key);
    Ok(NewIdentity { identity: created.identity, words: created.words })
}

/// Restore an identity from its recovery `words` into `profile_id`.
pub fn import(home: &Path, profile_id: &str, words: &str, password: &SecretString) -> Result<ProfileIdentity> {
    let dir = profile_dir(home, profile_id)?;
    let (identity, key) = profile_identity::import_unlocked(&dir, words, password, now_ms()).map_err(Error::failed)?;
    adopt_new_identity(home, profile_id, &identity.user_id, &key);
    Ok(identity)
}

/// Restore the identity in a backup or recovery-phrase file into
/// `profile_id`, sealed under its new `password`. A sealed backup needs
/// `backup_password`.
pub fn restore_backup(
    home: &Path,
    profile_id: &str,
    contents: &[u8],
    backup_password: Option<&SecretString>,
    password: &SecretString,
) -> Result<ProfileIdentity> {
    let dir = profile_dir(home, profile_id)?;
    let (identity, key) = morph_identity::backup::restore(&dir, contents, backup_password, password, now_ms()).map_err(Error::failed)?;
    adopt_new_identity(home, profile_id, &identity.user_id, &key);
    Ok(identity)
}

/// The recovery words of `profile_id`'s identity, after checking its password.
pub fn recovery_words(home: &Path, profile_id: &str, password: &SecretString) -> Result<Zeroizing<String>> {
    let dir = profile_dir(home, profile_id)?;
    profile_identity::recovery_words(&dir, password).map_err(Error::failed)
}

/// A backup of `profile_id`'s identity sealed under `backup_password` (an
/// armored age file of its recovery words), after checking the profile password.
pub fn export_backup(home: &Path, profile_id: &str, password: &SecretString, backup_password: &SecretString) -> Result<Zeroizing<String>> {
    let dir = profile_dir(home, profile_id)?;
    morph_identity::backup::export(&dir, password, backup_password).map_err(Error::failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_core::library_db::LibraryState;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn home_with_work() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::create(&home, "Work").unwrap();
        (tmp, home)
    }

    #[test]
    fn every_onboarding_path_encrypts_the_library() {
        let (_tmp, home) = home_with_work();
        morph_core::profiles::create(&home, "Spare").unwrap();
        assert!(load(&home, "p1").unwrap().is_none());
        let made = create(&home, "p1", &pw("long enough")).unwrap();
        assert_eq!(made.words.split_whitespace().count(), 24);
        assert_eq!(load(&home, "p1").unwrap().unwrap().user_id, made.identity.user_id);
        assert_eq!(*recovery_words(&home, "p1", &pw("long enough")).unwrap(), *made.words);
        assert!(recovery_words(&home, "p1", &pw("wrong one")).is_err());

        let imported = import(&home, "default", &made.words, &pw("other pass")).unwrap();
        assert_eq!(imported.user_id, made.identity.user_id);
        assert_eq!(crate::library::state(&home, "default").unwrap(), LibraryState::Unlocked);

        let backup = export_backup(&home, "p1", &pw("long enough"), &pw("backup pass")).unwrap();
        assert!(export_backup(&home, "p1", &pw("wrong one"), &pw("backup pass")).is_err());
        assert!(restore_backup(&home, "p2", backup.as_bytes(), Some(&pw("nope nope")), &pw("third pass")).is_err());
        assert!(load(&home, "p2").unwrap().is_none());
        let restored = restore_backup(&home, "p2", backup.as_bytes(), Some(&pw("backup pass")), &pw("third pass")).unwrap();
        assert_eq!(restored.user_id, made.identity.user_id);
        assert_eq!(crate::library::state(&home, "p2").unwrap(), LibraryState::Unlocked);
    }

    #[test]
    fn unknown_profiles_are_refused() {
        let (_tmp, home) = home_with_work();
        assert!(load(&home, "nope").is_err());
        assert!(create(&home, "nope", &pw("long enough")).is_err());
    }
}
