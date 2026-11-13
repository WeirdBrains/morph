//! Identity facts that are safe to show in the UI. Secrets never cross this boundary.

use morph_identity::profile_identity::{self, ProfileIdentity, SecretString, Source};
use morph_identity::{Paths, UserIdentity, identity_status};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Public identity status for a Morph home. Never creates the directory.
pub struct IdentityStatusView {
    pub home: String,
    pub home_exists: bool,
    pub profile: String,
}

/// Resolve the data dir (`home`, else `MORPH_HOME`, else `~/.morph`) without creating it.
pub fn identity_dir_info(home: Option<String>) -> Result<IdentityStatusView, String> {
    let path = home.filter(|h| !h.is_empty());
    let status = identity_status(path.as_deref().map(std::path::Path::new)).map_err(|e| e.to_string())?;
    Ok(IdentityStatusView { home: status.home, home_exists: status.home_exists, profile: status.profile })
}

/// An ephemeral user identity for the M1 wiring check. The key never leaves Rust.
pub struct EphemeralUser {
    pub user_id: String,
    pub fingerprint: String,
}

/// Generate a new user key in memory and return only its public id and fingerprint.
pub fn new_ephemeral_user() -> Result<EphemeralUser, String> {
    let user = UserIdentity::generate().map_err(|e| e.to_string())?;
    let fingerprint = user.fingerprint();
    Ok(EphemeralUser { user_id: user.user_id, fingerprint })
}

/// A profile's Weirdnet identity, public facts only.
pub struct ProfileIdentityView {
    pub user_id: String,
    pub fingerprint: String,
    pub created_ms: i64,
    pub imported: bool,
    /// Moved here from another device over a QR link.
    pub linked: bool,
}

impl ProfileIdentityView {
    pub(crate) fn from(identity: ProfileIdentity) -> Self {
        Self {
            user_id: identity.user_id,
            fingerprint: identity.fingerprint,
            created_ms: identity.created_ms,
            imported: identity.source == Source::Imported,
            linked: identity.source == Source::Linked,
        }
    }
}

/// A new identity and the 24 recovery words to show the user once.
pub struct CreatedIdentityView {
    pub identity: ProfileIdentityView,
    pub words: String,
}

pub(crate) fn profile_dir(home: Option<String>, profile_id: &str) -> Result<PathBuf, String> {
    let explicit = home.filter(|h| !h.is_empty()).map(PathBuf::from);
    let home = Paths::resolve(explicit).map_err(|e| e.to_string())?.home;
    morph_core::profiles::require(&home, profile_id).map_err(|e| e.to_string())?;
    Ok(morph_core::profiles::profile_dir(&home, profile_id))
}

pub(crate) fn resolve_home(home: Option<String>) -> Result<PathBuf, String> {
    let explicit = home.filter(|h| !h.is_empty()).map(PathBuf::from);
    Ok(Paths::resolve(explicit).map_err(|e| e.to_string())?.home)
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The identity of `profile_id`, or `None` before its onboarding.
pub fn profile_identity(home: Option<String>, profile_id: String) -> Result<Option<ProfileIdentityView>, String> {
    let dir = profile_dir(home, &profile_id)?;
    Ok(profile_identity::load(&dir).map_err(|e| e.to_string())?.map(ProfileIdentityView::from))
}

/// Create `profile_id`'s identity, sealed under `password`.
pub fn create_profile_identity(home: Option<String>, profile_id: String, password: String) -> Result<CreatedIdentityView, String> {
    let dir = profile_dir(home.clone(), &profile_id)?;
    let password = SecretString::from(password);
    let created = profile_identity::create(&dir, &password, now_ms()).map_err(|e| e.to_string())?;
    crate::api::library::adopt_new_identity(&resolve_home(home)?, &profile_id, &dir, &created.identity.user_id, &created.library_key);
    Ok(CreatedIdentityView { identity: ProfileIdentityView::from(created.identity), words: created.words.to_string() })
}

/// Restore an identity from its recovery words into `profile_id`.
pub fn import_profile_identity(home: Option<String>, profile_id: String, words: String, password: String) -> Result<ProfileIdentityView, String> {
    let dir = profile_dir(home.clone(), &profile_id)?;
    let words = morph_identity::profile_identity::Zeroizing::new(words);
    let password = SecretString::from(password);
    let (identity, key) = profile_identity::import_unlocked(&dir, &words, &password, now_ms()).map_err(|e| e.to_string())?;
    crate::api::library::adopt_new_identity(&resolve_home(home)?, &profile_id, &dir, &identity.user_id, &key);
    Ok(ProfileIdentityView::from(identity))
}

/// The recovery words of `profile_id`'s identity, after checking its password.
pub fn profile_recovery_words(home: Option<String>, profile_id: String, password: String) -> Result<String, String> {
    let dir = profile_dir(home, &profile_id)?;
    let password = SecretString::from(password);
    profile_identity::recovery_words(&dir, &password).map(|w| w.to_string()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn dir_info_does_not_create() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let info = super::identity_dir_info(Some(home.clone())).unwrap();
        assert!(!info.home_exists);
        assert!(!std::path::Path::new(&home).exists());
        assert_eq!(info.profile, "weirdnet/keys/v1");
    }

    #[test]
    fn ephemeral_user_has_a_public_id() {
        let a = super::new_ephemeral_user().unwrap();
        let b = super::new_ephemeral_user().unwrap();
        assert_eq!(a.user_id.len(), 32);
        assert_ne!(a.user_id, b.user_id);
    }

    fn home_with_profile() -> (tempfile::TempDir, String, String) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().display().to_string();
        let catalog = super::super::profiles::create_profile(Some(home.clone()), "Work".into()).unwrap();
        (tmp, home, catalog.current)
    }

    #[test]
    fn profile_identity_onboarding_round_trip() {
        let (_tmp, home, work) = home_with_profile();
        assert!(super::profile_identity(Some(home.clone()), work.clone()).unwrap().is_none());
        let made = super::create_profile_identity(Some(home.clone()), work.clone(), "long enough".into()).unwrap();
        assert_eq!(made.words.split_whitespace().count(), 24);
        assert!(!made.identity.imported);
        let loaded = super::profile_identity(Some(home.clone()), work.clone()).unwrap().unwrap();
        assert_eq!(loaded.user_id, made.identity.user_id);
        assert!(super::profile_identity(Some(home.clone()), "default".into()).unwrap().is_none());
        let words = super::profile_recovery_words(Some(home.clone()), work.clone(), "long enough".into()).unwrap();
        assert_eq!(words, made.words);
        assert!(super::profile_recovery_words(Some(home.clone()), work, "wrong one".into()).is_err());

        let imported = super::import_profile_identity(Some(home.clone()), "default".into(), made.words, "other pass".into()).unwrap();
        assert!(imported.imported);
        assert_eq!(imported.user_id, made.identity.user_id);
    }

    #[test]
    fn unknown_profiles_are_refused() {
        let (_tmp, home, _) = home_with_profile();
        assert!(super::profile_identity(Some(home.clone()), "nope".into()).is_err());
        assert!(super::create_profile_identity(Some(home), "nope".into(), "long enough".into()).is_err());
    }
}
