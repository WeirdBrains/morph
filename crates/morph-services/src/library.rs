//! Opening and unlocking a profile's encrypted library (bookmarks, history).
//!
//! A profile's library is plaintext until the profile gets a Weirdnet
//! identity; from then on it is an encrypted database whose key comes from
//! the identity and the profile password. The key is kept in the system
//! keystore so later launches open the library without a prompt.

use std::path::Path;

use morph_core::library_db::{self, LibraryState};
use morph_core::profiles::ProfileError;
use morph_identity::profile_identity::{self, LibraryKey, SecretString};

use crate::home::{now_ms, profile_dir};
use crate::{Error, Result};

/// What opening a profile's library found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryOpen {
    /// The library is open, or there is nothing to unlock (no identity yet).
    Ready,
    /// The keystore had no usable key: ask for the profile password and call
    /// [`unlock`]. `reason` says why the keystore could not help (never secret).
    NeedsPassword { reason: Option<String> },
}

/// The outcome of unlocking with the password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unlocked {
    /// The key is in the system keystore, so the next launch needs no password.
    pub remembered: bool,
    /// Why it could not be kept, if it was not. Never secret.
    pub reason: Option<String>,
}

/// Where `profile_id`'s library is kept right now.
pub fn state(home: &Path, profile_id: &str) -> Result<LibraryState> {
    library_db::state(home, profile_id).map_err(Error::failed)
}

/// At launch or on a profile switch: open `profile_id`'s encrypted library
/// with the key kept in the system keystore, without a prompt. A saved key
/// that no longer opens the library is forgotten.
pub fn open(home: &Path, profile_id: &str) -> Result<LibraryOpen> {
    let dir = profile_dir(home, profile_id)?;
    let Some(identity) = profile_identity::load(&dir).map_err(Error::failed)? else {
        return Ok(LibraryOpen::Ready);
    };
    if state(home, profile_id)? == LibraryState::Unlocked {
        return Ok(LibraryOpen::Ready);
    }
    match morph_keystore::load(&dir, &identity.user_id) {
        Ok(Some(key)) => match library_db::unlock(home, profile_id, &key) {
            Ok(()) => Ok(LibraryOpen::Ready),
            Err(ProfileError::Store(e)) if e.is_wrong_key() => {
                let _ = morph_keystore::forget(&dir);
                Ok(LibraryOpen::NeedsPassword { reason: Some("the saved key no longer opens this library".into()) })
            }
            Err(e) => Err(Error::failed(e)),
        },
        Ok(None) => Ok(LibraryOpen::NeedsPassword { reason: None }),
        Err(e) => Ok(LibraryOpen::NeedsPassword { reason: Some(e.to_string()) }),
    }
}

/// Unlock `profile_id`'s library with the profile password (the key is
/// derived from its identity and never leaves Rust), then keep the key in the
/// system keystore. The first unlock moves a plaintext library in.
pub fn unlock(home: &Path, profile_id: &str, password: &SecretString) -> Result<Unlocked> {
    let dir = profile_dir(home, profile_id)?;
    let identity = profile_identity::load(&dir).map_err(Error::failed)?.ok_or(Error::NoIdentity)?;
    let key = profile_identity::library_key(&dir, password).map_err(Error::failed)?;
    library_db::unlock(home, profile_id, &key).map_err(Error::failed)?;
    Ok(match morph_keystore::save(&dir, &identity.user_id, &key) {
        Ok(()) => Unlocked { remembered: true, reason: None },
        Err(e) => Unlocked { remembered: false, reason: Some(e.to_string()) },
    })
}

/// A profile just got an identity (created, imported, restored or linked)
/// and its key is in hand: encrypt the library now and keep the key in the
/// keystore. Neither failing undoes the identity; the next launch asks for
/// the password instead.
pub fn adopt_new_identity(home: &Path, profile_id: &str, user_id: &str, key: &LibraryKey) {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    if let Err(e) = library_db::unlock_new_identity(home, profile_id, key, now_ms()) {
        eprintln!("Morph: could not open the library of profile {profile_id}: {e}");
        return;
    }
    if let Err(e) = morph_keystore::save(&dir, user_id, key) {
        eprintln!("Morph: library key not kept for profile {profile_id}: {e}");
    }
}

/// Close `profile_id`'s library and drop its key from memory until the next unlock.
pub fn lock(home: &Path, profile_id: &str) {
    library_db::lock(home, profile_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity;
    use morph_core::library;

    fn home_with_work() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::create(&home, "Work").unwrap();
        (tmp, home)
    }

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn user_id(home: &Path, id: &str) -> String {
        profile_identity::load(&morph_core::profiles::profile_dir(home, id)).unwrap().unwrap().user_id
    }

    #[test]
    fn without_an_identity_there_is_nothing_to_unlock() {
        let (_tmp, home) = home_with_work();
        library::add_bookmark(&home, "p1", "https://w.test", "W", 1).unwrap();
        assert_eq!(open(&home, "p1").unwrap(), LibraryOpen::Ready);
        assert_eq!(state(&home, "p1").unwrap(), LibraryState::Plain);
        assert!(matches!(unlock(&home, "p1", &pw("long enough")), Err(Error::NoIdentity)));
        assert!(open(&home, "nope").is_err());
    }

    #[test]
    fn a_new_identity_encrypts_and_the_keystore_reopens_it() {
        let (_tmp, home) = home_with_work();
        library::add_bookmark(&home, "p1", "https://w.test", "W", 1).unwrap();
        identity::create(&home, "p1", &pw("long enough")).unwrap();
        assert_eq!(state(&home, "p1").unwrap(), LibraryState::Unlocked);
        assert_eq!(library::bookmarks(&home, "p1").unwrap().len(), 1, "moved in");
        lock(&home, "p1");
        assert_eq!(state(&home, "p1").unwrap(), LibraryState::Locked);
        assert_eq!(open(&home, "p1").unwrap(), LibraryOpen::Ready);
        assert_eq!(library::bookmarks(&home, "p1").unwrap().len(), 1);
    }

    #[test]
    fn without_a_saved_key_the_password_unlocks_and_keeps_it_again() {
        let (_tmp, home) = home_with_work();
        identity::create(&home, "p1", &pw("long enough")).unwrap();
        lock(&home, "p1");
        let dir = morph_core::profiles::profile_dir(&home, "p1");
        morph_keystore::forget(&dir).unwrap();
        assert_eq!(open(&home, "p1").unwrap(), LibraryOpen::NeedsPassword { reason: None });
        assert!(unlock(&home, "p1", &pw("wrong password")).is_err());
        assert_eq!(state(&home, "p1").unwrap(), LibraryState::Locked);
        let unlocked = unlock(&home, "p1", &pw("long enough")).unwrap();
        assert_eq!(unlocked, Unlocked { remembered: true, reason: None });
        assert!(morph_keystore::load(&dir, &user_id(&home, "p1")).unwrap().is_some());
        lock(&home, "p1");
        assert_eq!(open(&home, "p1").unwrap(), LibraryOpen::Ready, "kept again");
    }

    #[test]
    fn a_saved_key_that_no_longer_fits_is_forgotten() {
        let (_tmp, home) = home_with_work();
        identity::create(&home, "p1", &pw("long enough")).unwrap();
        lock(&home, "p1");
        let dir = morph_core::profiles::profile_dir(&home, "p1");
        // A key from another identity, saved under this profile's owner.
        let tmp2 = tempfile::tempdir().unwrap();
        let other = profile_identity::create(tmp2.path(), &pw("other enough"), 1).unwrap();
        morph_keystore::save(&dir, &user_id(&home, "p1"), &other.library_key).unwrap();
        match open(&home, "p1").unwrap() {
            LibraryOpen::NeedsPassword { reason: Some(why) } => assert!(why.contains("no longer opens"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(morph_keystore::load(&dir, &user_id(&home, "p1")).unwrap().is_none());
    }
}
