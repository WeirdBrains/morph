//! The password saver's storage and fill policy (no sync wiring yet).
//!
//! There is **no separate master password**. Saved logins live in
//! `library.db` (SQLCipher under the profile lock) and each password is
//! additionally sealed (XChaCha20-Poly1305) under a *saver key*:
//!
//! ```text
//! identity seed ──HKDF "morph/library-db/v1"──▶ library key
//! library key   ──HKDF "morph/password-saver/v1"──▶ saver key
//! ```
//!
//! The seed is never held in memory after unlock, so the saver key is derived
//! through the library key; both follow the seed, so every device with the same
//! identity derives the same saver key and a sealed value can sync as is. A
//! device without the identity cannot open it. Each value is bound (AAD) to its
//! row's id, origin and username, so rows cannot be swapped or repointed.
//!
//! A profile with no identity has no key: the saver is not offered. See
//! [`requirement`] for when filling needs the profile password or the OS
//! (`docs/profile-lock.md`, "Password saver").
use std::collections::BTreeMap;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use morph_identity::profile_identity::LibraryKey;
use morph_store::SavedLogin;
use serde::{Deserialize, Serialize};
use weirdnet_keys::{Key32, Sealed, Zeroizing};

use crate::profiles::{self, ProfileError};
use crate::{library_db, profile_files, profile_lock};

/// HKDF info for the saver key. Bump to rotate.
const SAVER_KEY_INFO: &[u8] = b"morph/password-saver/v1";
/// Sealed file with the saver's settings.
pub const SETTINGS_FILE: &str = "password-saver.json";

static KEYS: Mutex<BTreeMap<PathBuf, Key32>> = Mutex::new(BTreeMap::new());

fn keys() -> MutexGuard<'static, BTreeMap<PathBuf, Key32>> {
    KEYS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Derive and keep the saver key of the profile at `dir` (called on unlock).
pub(crate) fn remember_key(dir: &Path, library_key: &LibraryKey) -> Result<(), ProfileError> {
    let key = weirdnet_keys::kdf::hkdf(library_key.expose(), None, SAVER_KEY_INFO).map_err(|e| std::io::Error::other(e.to_string()))?;
    keys().insert(dir.to_path_buf(), key);
    Ok(())
}

/// Drop the saver key of the profile at `dir` (zeroed on drop).
pub(crate) fn forget_key(dir: &Path) {
    keys().remove(dir);
}

fn key_for(home: &Path, id: &str) -> Result<Key32, ProfileError> {
    keys().get(&profiles::profile_dir(home, id)).cloned().ok_or(ProfileError::Locked)
}

#[derive(Debug, thiserror::Error)]
pub enum SaverError {
    #[error(transparent)]
    Profile(#[from] ProfileError),
    #[error("the password saver is not available: {0}")]
    Unavailable(Unavailable),
    #[error("filling needs more proof first: {0}")]
    NotPermitted(&'static str),
    #[error("no such saved login")]
    NotFound,
    #[error("that saved password cannot be opened (damaged, or sealed by another identity)")]
    Unreadable,
    #[error("{0}")]
    Other(String),
}

impl From<morph_store::StoreError> for SaverError {
    fn from(e: morph_store::StoreError) -> Self {
        Self::Profile(e.into())
    }
}

/// Why the saver is not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// No Weirdnet identity: there is no key to seal under.
    NoIdentity,
    /// The profile password is off and this system has no OS authentication.
    NoOsAuth,
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoIdentity => "this profile has no identity (create one to save passwords)",
            Self::NoOsAuth => "the profile password is off and system authentication is not available here",
        })
    }
}

/// OS authentication (polkit, Touch ID, Windows Hello). **Not implemented on
/// any platform yet**: [`NoOsAuth`] is the stub every caller gets until a
/// platform backend exists, so a profile whose password is off does not offer
/// the saver.
pub trait OsAuth: Send + Sync {
    /// Whether this system can ask the user to authenticate.
    fn available(&self) -> bool;
    /// Ask the user to authenticate; `reason` is shown in the system prompt.
    fn authenticate(&self, reason: &str) -> Result<(), OsAuthError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OsAuthError {
    #[error("system authentication is not available")]
    Unavailable,
    #[error("authentication was cancelled or failed")]
    Denied,
}

/// The stub: no OS authentication.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOsAuth;

impl OsAuth for NoOsAuth {
    fn available(&self) -> bool {
        false
    }
    fn authenticate(&self, _reason: &str) -> Result<(), OsAuthError> {
        Err(OsAuthError::Unavailable)
    }
}

/// The saver's per-profile settings (a sealed profile file).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaverSettings {
    /// Re-ask the profile password before every fill (not a second password).
    #[serde(default)]
    pub ask_again: bool,
}

fn settings_path(home: &Path, id: &str) -> PathBuf {
    profiles::profile_dir(home, id).join(SETTINGS_FILE)
}

/// The profile's saver settings (defaults if none or unreadable).
pub fn settings(home: &Path, id: &str) -> Result<SaverSettings, SaverError> {
    let raw = profile_files::read(home, id, &settings_path(home, id))?;
    Ok(raw.and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default())
}

/// Store the saver settings. Callers that *loosen* them (ask again off) check
/// the profile password first (`morph_services::password_saver`).
pub fn save_settings(home: &Path, id: &str, settings: &SaverSettings) -> Result<(), SaverError> {
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| SaverError::Other(e.to_string()))?;
    Ok(profile_files::write(home, id, &settings_path(home, id), &bytes)?)
}

/// What must happen before a saved password is handed to a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requirement {
    /// The user must authenticate with the OS (profile password off).
    pub os_auth: bool,
    /// The user must re-enter the profile password ("ask again").
    pub password: bool,
}

/// Whether the saver is offered and what filling needs.
///
/// - no identity: [`Unavailable::NoIdentity`];
/// - password on: nothing extra (the unlock was the proof), plus the profile
///   password again if "ask again" is set;
/// - password off: OS authentication (and the password again if "ask again"
///   is set), or [`Unavailable::NoOsAuth`] where the OS cannot authenticate.
pub fn requirement(home: &Path, id: &str, os: &dyn OsAuth) -> Result<Requirement, SaverError> {
    if !profile_lock::has_identity(home, id) {
        return Err(SaverError::Unavailable(Unavailable::NoIdentity));
    }
    let ask_again = settings(home, id)?.ask_again;
    if profile_lock::requires_password(home, id) {
        return Ok(Requirement { os_auth: false, password: ask_again });
    }
    if !os.available() {
        return Err(SaverError::Unavailable(Unavailable::NoOsAuth));
    }
    Ok(Requirement { os_auth: true, password: ask_again })
}

/// Proof that a [`Requirement`] was met, needed by [`reveal`]. Built by the
/// service layer after it checked the password and/or asked the OS; this is a
/// seam against calling `reveal` by mistake, not a boundary against code in
/// the same process (the saver key is in memory while the profile is open).
#[derive(Debug)]
pub struct FillPermit(());

impl FillPermit {
    /// `password_checked`: the profile password was verified for this fill.
    /// `os_authenticated`: the OS accepted the user for this fill.
    pub fn new(requirement: Requirement, password_checked: bool, os_authenticated: bool) -> Result<Self, SaverError> {
        if requirement.password && !password_checked {
            return Err(SaverError::NotPermitted("profile password"));
        }
        if requirement.os_auth && !os_authenticated {
            return Err(SaverError::NotPermitted("system authentication"));
        }
        Ok(Self(()))
    }
}

/// A saved login without its password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginInfo {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}

impl From<&SavedLogin> for LoginInfo {
    fn from(l: &SavedLogin) -> Self {
        Self { id: l.id.clone(), origin: l.origin.clone(), username: l.username.clone(), created_ms: l.created_ms, updated_ms: l.updated_ms }
    }
}

fn aad(id: &str, origin: &str, username: &str) -> Vec<u8> {
    [b"morph/saved-login/v1".as_slice(), id.as_bytes(), b"\0", origin.as_bytes(), b"\0", username.as_bytes()].concat()
}

/// Normalize a page address to its origin (`scheme://host[:port]`, lowercase).
pub fn origin_of(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h).to_ascii_lowercase();
    let host = match (scheme.as_str(), host.rsplit_once(':')) {
        ("https", Some((h, "443"))) | ("http", Some((h, "80"))) => h.to_string(),
        _ => host,
    };
    (!host.is_empty()).then(|| format!("{scheme}://{host}"))
}

fn with_lib<T>(home: &Path, id: &str, f: impl FnOnce(&mut morph_store::Library) -> morph_store::Result<T>) -> Result<T, SaverError> {
    library_db::with(home, id, f)?.ok_or(SaverError::Profile(ProfileError::Locked))
}

fn gate(home: &Path, id: &str, os: &dyn OsAuth) -> Result<(), SaverError> {
    requirement(home, id, os).map(|_| ())
}

/// Save (or update) the login for `origin` and `username`; returns its id.
/// Needs the saver to be offered (an identity, and the password on or OS
/// authentication available) and the profile unlocked.
pub fn save(home: &Path, id: &str, os: &dyn OsAuth, page_url: &str, username: &str, password: &str, now_ms: i64) -> Result<String, SaverError> {
    gate(home, id, os)?;
    let origin = origin_of(page_url).ok_or_else(|| SaverError::Other("only http and https pages can have saved passwords".into()))?;
    let key = key_for(home, id)?;
    let existing = with_lib(home, id, |lib| Ok(lib.logins(Some(&origin))?.into_iter().find(|l| l.username == username)))?;
    let (login_id, created_ms) = match &existing {
        Some(l) => (l.id.clone(), l.created_ms),
        None => (uuid::Uuid::now_v7().to_string(), now_ms),
    };
    let sealed = weirdnet_keys::seal(&key, password.as_bytes(), &aad(&login_id, &origin, username)).map_err(|e| SaverError::Other(e.to_string()))?;
    let sealed = serde_json::to_vec(&sealed).map_err(|e| SaverError::Other(e.to_string()))?;
    let login =
        SavedLogin { id: login_id.clone(), origin, username: username.to_string(), sealed, created_ms, updated_ms: now_ms, ..Default::default() };
    with_lib(home, id, |lib| lib.put_login(&login))?;
    Ok(login_id)
}

/// The saved logins (no passwords), optionally for the page at `page_url`.
pub fn list(home: &Path, id: &str, os: &dyn OsAuth, page_url: Option<&str>) -> Result<Vec<LoginInfo>, SaverError> {
    gate(home, id, os)?;
    let origin = match page_url {
        Some(u) => match origin_of(u) {
            Some(o) => Some(o),
            None => return Ok(Vec::new()),
        },
        None => None,
    };
    with_lib(home, id, |lib| Ok(lib.logins(origin.as_deref())?.iter().map(LoginInfo::from).collect()))
}

/// Delete a saved login; whether it existed.
pub fn delete(home: &Path, id: &str, login_id: &str) -> Result<bool, SaverError> {
    with_lib(home, id, |lib| lib.delete_login(login_id))
}

/// The password of a saved login. Needs a [`FillPermit`] for the profile's
/// current [`requirement`].
pub fn reveal(home: &Path, id: &str, login_id: &str, _permit: &FillPermit) -> Result<Zeroizing<String>, SaverError> {
    let key = key_for(home, id)?;
    let login = with_lib(home, id, |lib| lib.login(login_id))?.ok_or(SaverError::NotFound)?;
    let sealed: Sealed = serde_json::from_slice(&login.sealed).map_err(|_| SaverError::Unreadable)?;
    let plain = weirdnet_keys::open(&key, &sealed, &aad(&login.id, &login.origin, &login.username)).map_err(|_| SaverError::Unreadable)?;
    String::from_utf8(plain.to_vec()).map(Zeroizing::new).map_err(|_| SaverError::Unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Os(bool);
    impl OsAuth for Os {
        fn available(&self) -> bool {
            self.0
        }
        fn authenticate(&self, _reason: &str) -> Result<(), OsAuthError> {
            if self.0 { Ok(()) } else { Err(OsAuthError::Unavailable) }
        }
    }

    fn unlocked(identity: bool) -> (tempfile::TempDir, PathBuf, Option<LibraryKey>) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        let key = identity.then(|| {
            let made =
                morph_identity::profile_identity::create(&profiles::profile_dir(&home, "default"), &"a long password".to_string().into(), 1).unwrap();
            library_db::unlock(&home, "default", &made.library_key).unwrap();
            made.library_key
        });
        (tmp, home, key)
    }

    fn permit(home: &Path, os: &dyn OsAuth) -> FillPermit {
        FillPermit::new(requirement(home, "default", os).unwrap(), true, true).unwrap()
    }

    #[test]
    fn origins_are_normalized() {
        assert_eq!(origin_of("HTTPS://Example.com:443/a?b#c").as_deref(), Some("https://example.com"));
        assert_eq!(origin_of("http://u:p@a.example:8080/x").as_deref(), Some("http://a.example:8080"));
        assert_eq!(origin_of("file:///etc/passwd"), None);
        assert_eq!(origin_of("not a url"), None);
    }

    #[test]
    fn a_saved_password_is_sealed_in_the_database_and_comes_back() {
        let (_tmp, home, _) = unlocked(true);
        let os = NoOsAuth;
        let a = save(&home, "default", &os, "https://example.com/login", "amy", "hunter2-correct", 10).unwrap();
        let again = save(&home, "default", &os, "https://example.com/other", "amy", "hunter3-newer", 20).unwrap();
        assert_eq!(a, again, "same origin and username update in place");
        save(&home, "default", &os, "https://other.example/", "amy", "p", 30).unwrap();
        assert_eq!(list(&home, "default", &os, Some("https://example.com/x")).unwrap().len(), 1);
        assert_eq!(list(&home, "default", &os, None).unwrap().len(), 2);
        assert_eq!(list(&home, "default", &os, Some("about:blank")).unwrap().len(), 0);
        let stored = library_db::with(&home, "default", |lib| lib.login(&a)).unwrap().unwrap().unwrap();
        assert!(!String::from_utf8_lossy(&stored.sealed).contains("hunter3"), "the row holds ciphertext, not the password");
        assert_eq!((stored.created_ms, stored.updated_ms), (10, 20));
        assert_eq!(reveal(&home, "default", &a, &permit(&home, &os)).unwrap().as_str(), "hunter3-newer");
        assert!(delete(&home, "default", &a).unwrap());
        assert!(matches!(reveal(&home, "default", &a, &permit(&home, &os)), Err(SaverError::NotFound)));
    }

    #[test]
    fn a_row_cannot_be_repointed_at_another_site_or_user() {
        let (_tmp, home, _) = unlocked(true);
        let id = save(&home, "default", &NoOsAuth, "https://bank.example/", "amy", "secret-one", 1).unwrap();
        library_db::with(&home, "default", |lib| {
            let mut l = lib.login(&id)?.unwrap();
            l.origin = "https://evil.example".into();
            lib.put_login(&l)
        })
        .unwrap();
        assert!(matches!(reveal(&home, "default", &id, &permit(&home, &NoOsAuth)), Err(SaverError::Unreadable)));
    }

    #[test]
    fn the_saver_key_follows_the_identity_and_goes_on_lock() {
        let (_tmp, home, key) = unlocked(true);
        let id = save(&home, "default", &NoOsAuth, "https://example.com/", "amy", "pw-1", 1).unwrap();
        // Same identity on another device: same saver key, so the sealed value opens there.
        let dir = profiles::profile_dir(&home, "default");
        let sealed = library_db::with(&home, "default", |lib| lib.login(&id)).unwrap().unwrap().unwrap();
        forget_key(&dir);
        remember_key(&dir, &key.unwrap()).unwrap();
        assert_eq!(reveal(&home, "default", &id, &permit(&home, &NoOsAuth)).unwrap().as_str(), "pw-1");
        library_db::lock(&home, "default");
        assert!(matches!(list(&home, "default", &NoOsAuth, None), Err(SaverError::Profile(ProfileError::Locked))));
        assert!(matches!(key_for(&home, "default"), Err(ProfileError::Locked)));
        // A different identity derives a different key and cannot open it.
        let other = LibraryKey::from_bytes([9; 32]);
        remember_key(&dir, &other).unwrap();
        let k = key_for(&home, "default").unwrap();
        let s: Sealed = serde_json::from_slice(&sealed.sealed).unwrap();
        assert!(weirdnet_keys::open(&k, &s, &aad(&sealed.id, &sealed.origin, &sealed.username)).is_err());
    }

    #[test]
    fn no_identity_means_no_saver() {
        let (_tmp, home, _) = unlocked(false);
        assert!(matches!(requirement(&home, "default", &Os(true)), Err(SaverError::Unavailable(Unavailable::NoIdentity))));
        assert!(matches!(
            save(&home, "default", &Os(true), "https://a.example/", "u", "p", 1),
            Err(SaverError::Unavailable(Unavailable::NoIdentity))
        ));
    }

    #[test]
    fn password_off_needs_os_auth_or_no_saver() {
        let (_tmp, home, _) = unlocked(true);
        assert_eq!(requirement(&home, "default", &NoOsAuth).unwrap(), Requirement { os_auth: false, password: false });
        profile_lock::set_require_password(&home, "default", false).unwrap();
        assert!(matches!(requirement(&home, "default", &NoOsAuth), Err(SaverError::Unavailable(Unavailable::NoOsAuth))));
        assert!(matches!(save(&home, "default", &NoOsAuth, "https://a.example/", "u", "p", 1), Err(SaverError::Unavailable(Unavailable::NoOsAuth))));
        assert_eq!(requirement(&home, "default", &Os(true)).unwrap(), Requirement { os_auth: true, password: false });
    }

    #[test]
    fn ask_again_adds_the_profile_password_and_a_permit_must_cover_it() {
        let (_tmp, home, _) = unlocked(true);
        assert!(!settings(&home, "default").unwrap().ask_again);
        save_settings(&home, "default", &SaverSettings { ask_again: true }).unwrap();
        assert!(settings(&home, "default").unwrap().ask_again);
        let req = requirement(&home, "default", &NoOsAuth).unwrap();
        assert_eq!(req, Requirement { os_auth: false, password: true });
        assert!(matches!(FillPermit::new(req, false, true), Err(SaverError::NotPermitted(_))));
        assert!(FillPermit::new(req, true, false).is_ok());
        let both = Requirement { os_auth: true, password: true };
        assert!(FillPermit::new(both, true, false).is_err());
        assert!(FillPermit::new(both, false, true).is_err());
        assert!(FillPermit::new(both, true, true).is_ok());
        let raw = fs::read(profile_files::sealed_path(&settings_path(&home, "default"))).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("ask_again"), "settings are sealed");
    }
}
