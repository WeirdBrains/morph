//! The password saver for the front ends: the fill gate on top of
//! `morph_core::password_saver` (storage and policy).
//!
//! No separate master password. What filling needs depends on the profile
//! (see `docs/profile-lock.md`, "Password saver"):
//!
//! - password on: nothing extra, because the unlock was the proof; with
//!   "ask again" the profile password is checked for every fill (with the
//!   same wrong-password backoff as the lock screen);
//! - password off: OS authentication first (a stub today: the saver is not
//!   offered until a platform backend implements [`OsAuth`]);
//! - no identity: not offered.
use std::path::Path;

use morph_core::password_saver::{self as core, FillPermit, SaverError, SaverSettings};
use morph_identity::profile_identity::SecretString;
use weirdnet_keys::Zeroizing;

pub use morph_core::password_saver::{LoginInfo, NoOsAuth, OsAuth, Requirement, Unavailable};

use crate::home::now_ms;
use crate::{Error, Result, library};

fn map(e: SaverError) -> Error {
    match e {
        SaverError::Profile(morph_core::profiles::ProfileError::Locked) => Error::Locked,
        other => Error::failed(other),
    }
}

/// What the UI shows: whether the saver is offered, what a fill needs, and the setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// `Err` is why it is not offered (shown to the user).
    pub requirement: std::result::Result<Requirement, Unavailable>,
    pub ask_again: bool,
}

/// Whether the saver is offered for `profile_id` and what filling needs.
pub fn offer(home: &Path, profile_id: &str, os: &dyn OsAuth) -> Result<Offer> {
    let ask_again =
        if morph_core::profile_lock::has_identity(home, profile_id) { core::settings(home, profile_id).map_err(map)?.ask_again } else { false };
    match core::requirement(home, profile_id, os) {
        Ok(r) => Ok(Offer { requirement: Ok(r), ask_again }),
        Err(SaverError::Unavailable(u)) => Ok(Offer { requirement: Err(u), ask_again }),
        Err(e) => Err(map(e)),
    }
}

/// Save (or update) a login. `page_url` is the page's address; only the origin is kept.
pub fn save(home: &Path, profile_id: &str, os: &dyn OsAuth, page_url: &str, username: &str, password: &SecretString) -> Result<String> {
    use weirdnet_keys::ExposeSecret;
    core::save(home, profile_id, os, page_url, username, password.expose_secret(), now_ms()).map_err(map)
}

/// The saved logins (no passwords), for a page or all of them.
pub fn list(home: &Path, profile_id: &str, os: &dyn OsAuth, page_url: Option<&str>) -> Result<Vec<LoginInfo>> {
    core::list(home, profile_id, os, page_url).map_err(map)
}

/// Delete a saved login; whether it existed.
pub fn delete(home: &Path, profile_id: &str, login_id: &str) -> Result<bool> {
    core::delete(home, profile_id, login_id).map_err(map)
}

/// The password to fill for `login_id`, after what the profile needs:
/// OS authentication when the password is off, and the profile password
/// (`password`, checked with the backoff) when "ask again" is set. A missing
/// proof is refused before anything is opened.
pub fn fill(home: &Path, profile_id: &str, os: &dyn OsAuth, login_id: &str, password: Option<&SecretString>) -> Result<Zeroizing<String>> {
    let requirement = core::requirement(home, profile_id, os).map_err(map)?;
    let mut password_checked = false;
    if requirement.password {
        let password = password.ok_or_else(|| Error::failed("enter the profile password to fill this login"))?;
        library::check_password(home, profile_id, password)?;
        password_checked = true;
    }
    let mut os_authenticated = false;
    if requirement.os_auth {
        os.authenticate("Fill a saved password").map_err(Error::failed)?;
        os_authenticated = true;
    }
    let permit = FillPermit::new(requirement, password_checked, os_authenticated).map_err(map)?;
    core::reveal(home, profile_id, login_id, &permit).map_err(map)
}

/// Turn "ask again before filling" on or off. Turning it **off** needs the
/// current profile password (checked with the backoff); on needs nothing.
pub fn set_ask_again(home: &Path, profile_id: &str, on: bool, password: Option<&SecretString>) -> Result<()> {
    if !morph_core::profile_lock::has_identity(home, profile_id) {
        return Err(Error::NoIdentity);
    }
    if !on {
        let password = password.ok_or(Error::WrongPassword)?;
        library::check_password(home, profile_id, password)?;
    }
    core::save_settings(home, profile_id, &SaverSettings { ask_again: on }).map_err(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_lock;
    use morph_core::password_saver::OsAuthError;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    struct Os {
        ok: bool,
        asked: std::sync::atomic::AtomicUsize,
    }
    impl Os {
        fn new(ok: bool) -> Self {
            Self { ok, asked: 0.into() }
        }
    }
    impl OsAuth for Os {
        fn available(&self) -> bool {
            self.ok
        }
        fn authenticate(&self, _reason: &str) -> std::result::Result<(), OsAuthError> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    fn profile() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        crate::identity::create(&home, "default", &pw("profile password")).unwrap();
        (tmp, home)
    }

    #[test]
    fn with_the_password_on_filling_needs_nothing_more_until_ask_again() {
        let (_tmp, home) = profile();
        let os = NoOsAuth;
        assert_eq!(offer(&home, "default", &os).unwrap().requirement, Ok(Requirement { os_auth: false, password: false }));
        let id = save(&home, "default", &os, "https://example.com/login", "amy", &pw("saved secret")).unwrap();
        assert_eq!(fill(&home, "default", &os, &id, None).unwrap().as_str(), "saved secret");

        set_ask_again(&home, "default", true, None).unwrap();
        assert!(offer(&home, "default", &os).unwrap().ask_again);
        assert!(fill(&home, "default", &os, &id, None).is_err(), "no password, no fill");
        assert!(matches!(fill(&home, "default", &os, &id, Some(&pw("wrong wrong"))), Err(Error::WrongPassword)));
    }

    #[test]
    fn ask_again_takes_the_profile_password_and_turning_it_off_needs_it_too() {
        let (_tmp, home) = profile();
        let os = NoOsAuth;
        let id = save(&home, "default", &os, "https://example.com/", "amy", &pw("s3cret")).unwrap();
        set_ask_again(&home, "default", true, None).unwrap();
        assert_eq!(fill(&home, "default", &os, &id, Some(&pw("profile password"))).unwrap().as_str(), "s3cret");
        assert!(set_ask_again(&home, "default", false, None).is_err());
        set_ask_again(&home, "default", false, Some(&pw("profile password"))).unwrap();
        assert_eq!(fill(&home, "default", &os, &id, None).unwrap().as_str(), "s3cret");
    }

    #[test]
    fn with_the_password_off_filling_asks_the_os_or_the_saver_is_not_offered() {
        let (_tmp, home) = profile();
        let id = save(&home, "default", &NoOsAuth, "https://example.com/", "amy", &pw("s3cret")).unwrap();
        profile_lock::set_require_password(&home, "default", &pw("profile password"), false).unwrap();

        let none = NoOsAuth;
        assert_eq!(offer(&home, "default", &none).unwrap().requirement, Err(Unavailable::NoOsAuth));
        assert!(fill(&home, "default", &none, &id, None).is_err());
        assert!(save(&home, "default", &none, "https://example.com/", "bob", &pw("x")).is_err());

        let os = Os::new(true);
        assert_eq!(offer(&home, "default", &os).unwrap().requirement, Ok(Requirement { os_auth: true, password: false }));
        assert_eq!(fill(&home, "default", &os, &id, None).unwrap().as_str(), "s3cret");
        assert_eq!(os.asked.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_profile_without_an_identity_and_a_locked_one_do_not_fill() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        let os = Os::new(true);
        assert_eq!(offer(&home, "default", &os).unwrap().requirement, Err(Unavailable::NoIdentity));
        assert!(save(&home, "default", &os, "https://a.example/", "u", &pw("p")).is_err());
        assert!(matches!(set_ask_again(&home, "default", true, None), Err(Error::NoIdentity)));

        let (_tmp2, home) = profile();
        let id = save(&home, "default", &NoOsAuth, "https://example.com/", "amy", &pw("s3cret")).unwrap();
        profile_lock::lock(&home, "default");
        assert!(matches!(fill(&home, "default", &NoOsAuth, &id, None), Err(Error::Locked)));
        assert!(matches!(list(&home, "default", &NoOsAuth, None), Err(Error::Locked)));
        library::unlock(&home, "default", &pw("profile password")).unwrap();
        assert_eq!(fill(&home, "default", &NoOsAuth, &id, None).unwrap().as_str(), "s3cret", "the saved password survives lock and unlock");
    }
}
