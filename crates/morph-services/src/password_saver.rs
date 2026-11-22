//! The password saver for the front ends: the fill gate on top of
//! `morph_core::password_saver` (storage and policy).
//!
//! No separate master password. What filling needs depends on the profile
//! (see `docs/profile-lock.md`, "Password saver"):
//!
//! - password on: nothing extra, because the unlock was the proof; with
//!   "ask again" the profile password is checked for every fill (with the
//!   same wrong-password backoff as the lock screen);
//! - password off: OS authentication first (`morph_os::os_auth` on the
//!   desktop); where the system cannot authenticate the saver is not offered;
//! - no identity: not offered.
//!
//! Filling goes only into an https page whose origin is exactly the saved
//! one. Showing a password to the user (Settings) always takes a fresh
//! proof: the profile password, or the system's when the password is off.
use std::path::Path;

use morph_core::password_saver::{self as core, FillPermit, SaverError};
use morph_identity::profile_identity::SecretString;
use weirdnet_keys::Zeroizing;

pub use morph_core::password_saver::{Captured, LoginInfo, NoOsAuth, OsAuth, OsAuthError, Requirement, Unavailable, page_matches};

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
    /// Settings > Privacy, "Offer to save passwords".
    pub enabled: bool,
}

impl Offer {
    /// Whether pages get save and fill offers.
    pub fn active(&self) -> bool {
        self.enabled && self.requirement.is_ok()
    }
}

/// Whether the saver is offered for `profile_id` and what filling needs.
pub fn offer(home: &Path, profile_id: &str, os: &dyn OsAuth) -> Result<Offer> {
    let has_identity = morph_core::profile_lock::has_identity(home, profile_id);
    let settings = if has_identity { core::settings(home, profile_id).map_err(map)? } else { Default::default() };
    // The stored default is on, but the saver stays off in Settings and in
    // offers until the profile has an identity.
    let (ask_again, enabled) = (settings.ask_again, has_identity && settings.enabled);
    match core::requirement(home, profile_id, os) {
        Ok(r) => Ok(Offer { requirement: Ok(r), ask_again, enabled }),
        Err(SaverError::Unavailable(u)) => Ok(Offer { requirement: Err(u), ask_again, enabled }),
        Err(e) => Err(map(e)),
    }
}

/// Save (or update) a login. `page_url` is the page's address; only the origin is kept.
pub fn save(home: &Path, profile_id: &str, os: &dyn OsAuth, page_url: &str, username: &str, password: &SecretString) -> Result<String> {
    use weirdnet_keys::ExposeSecret;
    if morph_core::profile_lock::has_identity(home, profile_id) && !core::settings(home, profile_id).map_err(map)?.enabled {
        return Err(Error::failed("the password saver is off for this profile"));
    }
    core::save(home, profile_id, os, page_url, username, password.expose_secret(), now_ms()).map_err(map)
}

/// The saved logins (no passwords), for a page or all of them.
pub fn list(home: &Path, profile_id: &str, os: &dyn OsAuth, page_url: Option<&str>) -> Result<Vec<LoginInfo>> {
    core::list(home, profile_id, os, page_url).map_err(map)
}

/// How a login the user just submitted on `page_url` compares with the
/// saved one (no saved password leaves the core): offer to save, to
/// update, or nothing.
pub fn captured(home: &Path, profile_id: &str, os: &dyn OsAuth, page_url: &str, username: &str, password: &SecretString) -> Result<Captured> {
    use weirdnet_keys::ExposeSecret;
    core::captured(home, profile_id, os, page_url, username, password.expose_secret()).map_err(map)
}

/// One saved login (no password).
pub fn login(home: &Path, profile_id: &str, login_id: &str) -> Result<LoginInfo> {
    core::login(home, profile_id, login_id).map_err(map)
}

/// Delete a saved login; whether it existed.
pub fn delete(home: &Path, profile_id: &str, login_id: &str) -> Result<bool> {
    core::delete(home, profile_id, login_id).map_err(map)
}

/// Meet `requirement`: the profile password (checked with the backoff),
/// then the system's prompt. A missing proof is refused before any prompt.
fn prove(
    home: &Path,
    profile_id: &str,
    os: &dyn OsAuth,
    requirement: Requirement,
    password: Option<&SecretString>,
    reason: &str,
) -> Result<FillPermit> {
    let mut password_checked = false;
    if requirement.password {
        let password = password.ok_or_else(|| Error::failed("enter the profile password first"))?;
        library::check_password(home, profile_id, password)?;
        password_checked = true;
    }
    let mut os_authenticated = false;
    if requirement.os_auth {
        os.authenticate(reason).map_err(Error::failed)?;
        os_authenticated = true;
    }
    FillPermit::new(requirement, password_checked, os_authenticated).map_err(map)
}

/// The password to fill for `login_id` into the page at `page_url`, after
/// what the profile needs: OS authentication when the password is off, and
/// the profile password (`password`, checked with the backoff) when "ask
/// again" is set. Refused before any prompt when the page is not https or
/// not exactly the login's origin, or the saver is off.
pub fn fill(
    home: &Path,
    profile_id: &str,
    os: &dyn OsAuth,
    login_id: &str,
    page_url: &str,
    password: Option<&SecretString>,
) -> Result<Zeroizing<String>> {
    let requirement = core::requirement(home, profile_id, os).map_err(map)?;
    if !core::settings(home, profile_id).map_err(map)?.enabled {
        return Err(Error::failed("the password saver is off for this profile"));
    }
    let login = core::login(home, profile_id, login_id).map_err(map)?;
    if !page_matches(&login.origin, page_url) {
        return Err(Error::failed("this login is saved for another site"));
    }
    let permit = prove(home, profile_id, os, requirement, password, "Fill a saved password")?;
    core::reveal(home, profile_id, login_id, &permit).map_err(map)
}

/// Show a saved password to the user (Settings): always after a fresh
/// proof, the profile password (`password`) or, with the password off, the
/// system's prompt.
pub fn reveal(home: &Path, profile_id: &str, os: &dyn OsAuth, login_id: &str, password: Option<&SecretString>) -> Result<Zeroizing<String>> {
    let requirement = core::reveal_requirement(home, profile_id, os).map_err(map)?;
    let permit = prove(home, profile_id, os, requirement, password, "Show a saved password")?;
    core::reveal(home, profile_id, login_id, &permit).map_err(map)
}

/// What [`reveal`] will ask for (Settings shows the password field or not).
pub fn reveal_requirement(home: &Path, profile_id: &str, os: &dyn OsAuth) -> Result<Requirement> {
    core::reveal_requirement(home, profile_id, os).map_err(map)
}

/// Turn the saver's offers on or off (Settings > Privacy). Saved logins stay.
pub fn set_enabled(home: &Path, profile_id: &str, on: bool) -> Result<()> {
    if !morph_core::profile_lock::has_identity(home, profile_id) {
        return Err(Error::NoIdentity);
    }
    let mut settings = core::settings(home, profile_id).map_err(map)?;
    settings.enabled = on;
    core::save_settings(home, profile_id, &settings).map_err(map)
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
    let mut settings = core::settings(home, profile_id).map_err(map)?;
    settings.ask_again = on;
    core::save_settings(home, profile_id, &settings).map_err(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_lock;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    struct Os {
        ok: bool,
        asked: std::sync::atomic::AtomicUsize,
        deny: bool,
    }
    impl Os {
        fn new(ok: bool) -> Self {
            Self { ok, asked: 0.into(), deny: false }
        }
    }
    impl OsAuth for Os {
        fn available(&self) -> bool {
            self.ok
        }
        fn authenticate(&self, _reason: &str) -> std::result::Result<(), OsAuthError> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.deny { Err(OsAuthError::Denied) } else { Ok(()) }
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
        assert_eq!(fill(&home, "default", &os, &id, "https://example.com/login", None).unwrap().as_str(), "saved secret");

        set_ask_again(&home, "default", true, None).unwrap();
        assert!(offer(&home, "default", &os).unwrap().ask_again);
        assert!(fill(&home, "default", &os, &id, "https://example.com/login", None).is_err(), "no password, no fill");
        assert!(matches!(fill(&home, "default", &os, &id, "https://example.com/login", Some(&pw("wrong wrong"))), Err(Error::WrongPassword)));
    }

    #[test]
    fn ask_again_takes_the_profile_password_and_turning_it_off_needs_it_too() {
        let (_tmp, home) = profile();
        let os = NoOsAuth;
        let id = save(&home, "default", &os, "https://example.com/", "amy", &pw("s3cret")).unwrap();
        set_ask_again(&home, "default", true, None).unwrap();
        assert_eq!(fill(&home, "default", &os, &id, "https://example.com/login", Some(&pw("profile password"))).unwrap().as_str(), "s3cret");
        assert!(set_ask_again(&home, "default", false, None).is_err());
        set_ask_again(&home, "default", false, Some(&pw("profile password"))).unwrap();
        assert_eq!(fill(&home, "default", &os, &id, "https://example.com/login", None).unwrap().as_str(), "s3cret");
    }

    #[test]
    fn with_the_password_off_filling_asks_the_os_or_the_saver_is_not_offered() {
        let (_tmp, home) = profile();
        let id = save(&home, "default", &NoOsAuth, "https://example.com/", "amy", &pw("s3cret")).unwrap();
        profile_lock::set_require_password(&home, "default", &pw("profile password"), false).unwrap();

        let none = NoOsAuth;
        assert_eq!(offer(&home, "default", &none).unwrap().requirement, Err(Unavailable::NoOsAuth));
        assert!(fill(&home, "default", &none, &id, "https://example.com/login", None).is_err());
        assert!(save(&home, "default", &none, "https://example.com/", "bob", &pw("x")).is_err());

        let os = Os::new(true);
        assert_eq!(offer(&home, "default", &os).unwrap().requirement, Ok(Requirement { os_auth: true, password: false }));
        assert_eq!(fill(&home, "default", &os, &id, "https://example.com/login", None).unwrap().as_str(), "s3cret");
        assert_eq!(os.asked.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn filling_goes_only_to_the_exact_https_origin_and_stops_when_the_saver_is_off() {
        let (_tmp, home) = profile();
        let os = NoOsAuth;
        let id = save(&home, "default", &os, "https://example.com/login", "amy", &pw("s3cret")).unwrap();
        for page in ["http://example.com/login", "https://www.example.com/", "https://example.com:8443/", "https://evil.test/"] {
            assert!(fill(&home, "default", &os, &id, page, None).is_err(), "{page}");
        }
        assert!(offer(&home, "default", &os).unwrap().active());
        set_enabled(&home, "default", false).unwrap();
        let offer_now = offer(&home, "default", &os).unwrap();
        assert!(!offer_now.enabled && !offer_now.active());
        assert!(fill(&home, "default", &os, &id, "https://example.com/", None).is_err());
        assert!(save(&home, "default", &os, "https://example.com/", "bob", &pw("x")).is_err(), "off also refuses a new save");
        assert_eq!(list(&home, "default", &os, None).unwrap().len(), 1, "turning it off keeps the saved logins");
        set_ask_again(&home, "default", true, None).unwrap();
        set_enabled(&home, "default", true).unwrap();
        assert!(offer(&home, "default", &os).unwrap().ask_again, "one setting does not reset the other");
    }

    #[test]
    fn showing_a_password_always_takes_a_fresh_proof() {
        let (_tmp, home) = profile();
        let id = save(&home, "default", &NoOsAuth, "https://example.com/", "amy", &pw("s3cret")).unwrap();
        assert!(reveal(&home, "default", &NoOsAuth, &id, None).is_err(), "even without ask again");
        assert!(matches!(reveal(&home, "default", &NoOsAuth, &id, Some(&pw("nope nope"))), Err(Error::WrongPassword)));
        assert_eq!(reveal(&home, "default", &NoOsAuth, &id, Some(&pw("profile password"))).unwrap().as_str(), "s3cret");
        profile_lock::set_require_password(&home, "default", &pw("profile password"), false).unwrap();
        let os = Os::new(true);
        assert_eq!(reveal_requirement(&home, "default", &os).unwrap(), Requirement { os_auth: true, password: false });
        assert_eq!(reveal(&home, "default", &os, &id, None).unwrap().as_str(), "s3cret");
        assert_eq!(os.asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        let denied = Os { ok: true, asked: 0.into(), deny: true };
        assert!(reveal(&home, "default", &denied, &id, None).is_err(), "a cancelled system prompt shows nothing");
        assert!(fill(&home, "default", &denied, &id, "https://example.com/", None).is_err());
    }

    #[test]
    fn a_submitted_login_is_new_changed_or_the_same() {
        let (_tmp, home) = profile();
        let os = NoOsAuth;
        assert_eq!(captured(&home, "default", &os, "https://example.com/", "amy", &pw("one")).unwrap(), Captured::New);
        let id = save(&home, "default", &os, "https://example.com/", "amy", &pw("one")).unwrap();
        assert_eq!(captured(&home, "default", &os, "https://example.com/", "amy", &pw("one")).unwrap(), Captured::Same);
        assert_eq!(captured(&home, "default", &os, "https://example.com/", "amy", &pw("two")).unwrap(), Captured::Changed(id.clone()));
        assert_eq!(login(&home, "default", &id).unwrap().username, "amy");
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
        assert!(matches!(fill(&home, "default", &NoOsAuth, &id, "https://example.com/login", None), Err(Error::Locked)));
        assert!(matches!(list(&home, "default", &NoOsAuth, None), Err(Error::Locked)));
        library::unlock(&home, "default", &pw("profile password")).unwrap();
        assert_eq!(
            fill(&home, "default", &NoOsAuth, &id, "https://example.com/login", None).unwrap().as_str(),
            "s3cret",
            "the saved password survives lock and unlock"
        );
    }

    #[test]
    fn the_saver_is_on_as_soon_as_a_profile_gets_an_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        let os = NoOsAuth;
        let none = offer(&home, "default", &os).unwrap();
        assert_eq!(none.requirement, Err(Unavailable::NoIdentity));
        assert!(!none.enabled && !none.active(), "off until the profile has an identity");
        assert!(matches!(set_enabled(&home, "default", true), Err(Error::NoIdentity)));

        let created = crate::identity::create(&home, "default", &pw("profile password")).unwrap();
        let on = offer(&home, "default", &os).unwrap();
        assert!(on.enabled && on.active(), "on by default the moment the profile has an identity");

        // Restore onto a fresh profile: same default.
        let other = tmp.path().join("other");
        morph_core::profiles::load_catalog(&other).unwrap();
        crate::identity::import(&other, "default", &created.words, &pw("profile password")).unwrap();
        let restored = offer(&other, "default", &os).unwrap();
        assert!(restored.enabled && restored.active(), "a restored identity also turns the saver on");

        set_enabled(&home, "default", false).unwrap();
        assert!(!offer(&home, "default", &os).unwrap().active());
        assert!(offer(&other, "default", &os).unwrap().active(), "one profile's switch does not change another's");
    }
}
