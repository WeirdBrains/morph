//! The password saver for the Flutter shell (`docs/profile-lock.md`).
//!
//! On by default for a profile that has a Weirdnet identity; off and not
//! offered without one. Password values cross this boundary only for a fill
//! or when the user asks to see a saved password, and are never logged.

use crate::home::resolve as resolve_home;
use morph_identity::profile_identity::SecretString;
use morph_services::password_saver::{self as saver, NoOsAuth, Unavailable};

fn map(e: morph_services::Error) -> String {
    e.to_string()
}

fn os() -> NoOsAuth {
    NoOsAuth
}

/// What the Settings page shows for the current profile.
pub struct SaverOfferView {
    /// Settings > Privacy, "Offer to save passwords".
    pub enabled: bool,
    /// Offers to save and fill are live (enabled, and the profile can use them).
    pub active: bool,
    /// The profile can use the saver (it has an identity, and a way to prove it).
    pub available: bool,
    pub ask_again: bool,
    /// Why it is not offered, for the UI; empty when it is.
    pub unavailable: String,
}

fn offer_view(offer: saver::Offer) -> SaverOfferView {
    SaverOfferView {
        enabled: offer.enabled,
        active: offer.active(),
        available: offer.requirement.is_ok(),
        ask_again: offer.ask_again,
        unavailable: match offer.requirement {
            Err(Unavailable::NoIdentity) => {
                "This profile has no identity, so it cannot save passwords. Create or import one in Identity.".into()
            }
            Err(Unavailable::NoOsAuth) => {
                "The profile password is off and this device cannot confirm it's you, so saved passwords are not filled. Turn the profile password on to use them.".into()
            }
            Ok(_) => String::new(),
        },
    }
}

/// One saved login, with no password.
pub struct SavedLoginView {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}

fn login_view(login: saver::LoginInfo) -> SavedLoginView {
    SavedLoginView { id: login.id, origin: login.origin, username: login.username, created_ms: login.created_ms, updated_ms: login.updated_ms }
}

/// Whether the saver is offered for `profile_id` and what filling needs.
pub fn password_saver_offer(home: Option<String>, profile_id: String) -> Result<SaverOfferView, String> {
    let home = resolve_home(home)?;
    saver::offer(&home, &profile_id, &os()).map(offer_view).map_err(map)
}

/// Turn offers to save and fill on or off. Saved logins stay.
pub fn set_password_saver_enabled(home: Option<String>, profile_id: String, on: bool) -> Result<SaverOfferView, String> {
    let home = resolve_home(home)?;
    saver::set_enabled(&home, &profile_id, on).map_err(map)?;
    saver::offer(&home, &profile_id, &os()).map(offer_view).map_err(map)
}

/// Turn "ask again before filling" on or off. Turning it off needs the
/// current profile password.
pub fn set_password_saver_ask_again(home: Option<String>, profile_id: String, on: bool, password: Option<String>) -> Result<SaverOfferView, String> {
    let home = resolve_home(home)?;
    let password = password.map(SecretString::from);
    saver::set_ask_again(&home, &profile_id, on, password.as_ref()).map_err(map)?;
    saver::offer(&home, &profile_id, &os()).map(offer_view).map_err(map)
}

/// The saved logins (no passwords), optionally only for `page_url`.
pub fn list_saved_logins(home: Option<String>, profile_id: String, page_url: Option<String>) -> Result<Vec<SavedLoginView>, String> {
    let home = resolve_home(home)?;
    saver::list(&home, &profile_id, &os(), page_url.as_deref()).map(|list| list.into_iter().map(login_view).collect()).map_err(map)
}

/// Delete a saved login; whether it existed.
pub fn delete_saved_login(home: Option<String>, profile_id: String, login_id: String) -> Result<bool, String> {
    let home = resolve_home(home)?;
    saver::delete(&home, &profile_id, &login_id).map_err(map)
}

/// Show a saved password after a fresh proof (the profile password, or,
/// with the profile password off, the system's prompt — not yet on the phone).
pub fn reveal_saved_password(home: Option<String>, profile_id: String, login_id: String, password: Option<String>) -> Result<String, String> {
    let home = resolve_home(home)?;
    let password = password.map(SecretString::from);
    saver::reveal(&home, &profile_id, &os(), &login_id, password.as_ref()).map(|s| s.to_string()).map_err(map)
}

/// Save (or update) a login from the page at `page_url`.
pub fn save_login(home: Option<String>, profile_id: String, page_url: String, username: String, password: String) -> Result<String, String> {
    let home = resolve_home(home)?;
    saver::save(&home, &profile_id, &os(), &page_url, &username, &SecretString::from(password)).map_err(map)
}

/// The password to fill for `login_id` into the page at `page_url`.
pub fn fill_saved_login(
    home: Option<String>,
    profile_id: String,
    login_id: String,
    page_url: String,
    password: Option<String>,
) -> Result<String, String> {
    let home = resolve_home(home)?;
    let password = password.map(SecretString::from);
    saver::fill(&home, &profile_id, &os(), &login_id, &page_url, password.as_ref()).map(|s| s.to_string()).map_err(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::identity::create_profile_identity;
    use crate::api::profiles::list_profiles;

    fn home() -> (tempfile::TempDir, String) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        list_profiles(Some(home.clone())).unwrap();
        (tmp, home)
    }

    #[test]
    fn off_without_an_identity_and_on_once_it_has_one() {
        let (_tmp, home) = home();
        let none = password_saver_offer(Some(home.clone()), "default".into()).unwrap();
        assert!(!none.available && !none.active);
        assert!(none.unavailable.contains("no identity"));
        assert!(set_password_saver_enabled(Some(home.clone()), "default".into(), true).is_err());

        create_profile_identity(Some(home.clone()), "default".into(), "profile password".into()).unwrap();
        let on = password_saver_offer(Some(home.clone()), "default".into()).unwrap();
        assert!(on.enabled && on.active && on.available, "on by default once the profile has an identity");

        let off = set_password_saver_enabled(Some(home.clone()), "default".into(), false).unwrap();
        assert!(!off.enabled && !off.active && off.available);

        assert!(
            save_login(Some(home.clone()), "default".into(), "https://example.com/login".into(), "amy".into(), "s3cret".into()).is_err(),
            "saves are refused while the saver is off"
        );

        set_password_saver_enabled(Some(home.clone()), "default".into(), true).unwrap();
        let id = save_login(Some(home.clone()), "default".into(), "https://example.com/login".into(), "amy".into(), "s3cret".into()).unwrap();
        let list = list_saved_logins(Some(home.clone()), "default".into(), None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].username, "amy");
        assert_eq!(list[0].origin, "https://example.com");
        assert_eq!(reveal_saved_password(Some(home.clone()), "default".into(), id.clone(), Some("profile password".into())).unwrap(), "s3cret");
        assert!(reveal_saved_password(Some(home.clone()), "default".into(), id.clone(), None).is_err());
        assert_eq!(fill_saved_login(Some(home.clone()), "default".into(), id.clone(), "https://example.com/login".into(), None).unwrap(), "s3cret");
        assert!(delete_saved_login(Some(home.clone()), "default".into(), id).unwrap());
        assert!(list_saved_logins(Some(home.clone()), "default".into(), None).unwrap().is_empty());
    }
}
