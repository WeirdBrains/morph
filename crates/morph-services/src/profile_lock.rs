//! The profile password gate, for every entry point (launch, switch, a new
//! window, links from the CLI or URL handler, the MCP server, sync).
//!
//! A profile without a Weirdnet identity has nothing to encrypt and never
//! locks. A profile with one asks for its password every time it opens,
//! unless the user turned that off in Settings (with the current password);
//! an admin policy can keep it on. See `docs/profile-lock.md`.

use std::path::Path;

use morph_core::library_db::{self, LibraryState};
use morph_core::profile_lock;
use morph_core::profiles::ProfileError;
use morph_identity::profile_identity::{self, SecretString};

use crate::home::profile_dir;
use crate::{Error, Result};

/// What the UI shows about a profile's lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockStatus {
    /// The profile has a Weirdnet identity. Without one it is not encrypted
    /// and not protected by a password.
    pub has_identity: bool,
    /// Opening it asks for the password (on by default once it has an identity).
    pub require_password: bool,
    /// An admin policy keeps the password on; the Settings switch is disabled.
    pub policy_locked: bool,
    /// Open in this process (no password needed right now).
    pub unlocked: bool,
}

/// The lock status of `profile_id`.
pub fn status(home: &Path, profile_id: &str) -> Result<LockStatus> {
    profile_dir(home, profile_id)?;
    let has_identity = profile_lock::has_identity(home, profile_id);
    let unlocked = !has_identity || library_db::state(home, profile_id).map_err(Error::failed)? == LibraryState::Unlocked;
    Ok(LockStatus {
        has_identity,
        require_password: profile_lock::requires_password(home, profile_id),
        policy_locked: profile_lock::policy_requires_password(home),
        unlocked,
    })
}

/// Whether the profile is locked right now: it has an identity and was not
/// unlocked in this process.
pub fn is_locked(home: &Path, profile_id: &str) -> bool {
    status(home, profile_id).map(|s| !s.unlocked).unwrap_or(true)
}

/// The gate for anything acting on a profile without a window in front of
/// the user (links from the CLI or URL handler, the MCP server, sync): refuse
/// with [`Error::Locked`] while it is locked. A profile with no identity and
/// one open in this process pass.
pub fn require_unlocked(home: &Path, profile_id: &str) -> Result<()> {
    if is_locked(home, profile_id) { Err(Error::Locked) } else { Ok(()) }
}

/// [`is_locked`] for the profile whose companions directory is `companions`
/// (`<home>/profiles/<id>/companions`), for the MCP host. Anything that is
/// not a profile's companions directory reads as locked (fails closed).
pub fn is_locked_in(companions: &Path) -> bool {
    let Some(profile) = companions.parent() else { return true };
    let (Some(id), Some(home)) = (profile.file_name().and_then(|n| n.to_str()), profile.parent().and_then(Path::parent)) else { return true };
    is_locked(home, id)
}

/// Lock `profile_id` now: its library, sync node and file key close, and it
/// asks for the password before it opens again. With the password turned off
/// the next open uses the keystore again, so locking means little there.
pub fn lock(home: &Path, profile_id: &str) {
    crate::library::lock(home, profile_id);
}

/// Turn "Require password to open" on or off, after checking the current
/// password (with the backoff). On: the key leaves the keystore. Off: the key
/// is kept in the system keystore, so the profile opens without a prompt;
/// refused while a policy keeps it on, or if the keystore cannot keep it.
/// The library and profile files stay encrypted either way.
pub fn set_require_password(home: &Path, profile_id: &str, password: &SecretString, on: bool) -> Result<()> {
    let dir = profile_dir(home, profile_id)?;
    let identity = profile_identity::load(&dir).map_err(Error::failed)?.ok_or(Error::NoIdentity)?;
    if !on && profile_lock::policy_requires_password(home) {
        return Err(Error::failed(ProfileError::PolicyLocked));
    }
    let key = crate::library::check_password(home, profile_id, password)?;
    if on {
        profile_lock::set_require_password(home, profile_id, true).map_err(Error::failed)?;
        morph_keystore::forget(&dir).map_err(Error::failed)?;
    } else {
        morph_keystore::save(&dir, &identity.user_id, &key).map_err(Error::failed)?;
        if let Err(e) = profile_lock::set_require_password(home, profile_id, false) {
            let _ = morph_keystore::forget(&dir);
            return Err(Error::failed(e));
        }
    }
    Ok(())
}

/// Change the profile password: `current` must be right (with the backoff).
/// The identity seed, the library key and the encrypted files stay the same;
/// only the sealed user and node keys are resealed (crash-safe).
pub fn change_password(home: &Path, profile_id: &str, current: &SecretString, new: &SecretString) -> Result<()> {
    let dir = profile_dir(home, profile_id)?;
    crate::library::check_password(home, profile_id, current)?;
    profile_identity::change_password(&dir, current, new).map_err(Error::failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{self, LibraryOpen};
    use morph_core::profile_files;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn home() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        (tmp, home)
    }

    /// A profile with an identity, its library open (as right after onboarding).
    fn with_identity(home: &Path, id: &str) {
        crate::identity::create(home, id, &pw("profile password")).unwrap();
    }

    #[test]
    fn a_profile_without_an_identity_opens_freely() {
        let (_tmp, home) = home();
        let s = status(&home, "default").unwrap();
        assert!(!s.has_identity && !s.require_password && s.unlocked);
        assert_eq!(library::open(&home, "default").unwrap(), LibraryOpen::Ready);
        require_unlocked(&home, "default").unwrap();
        assert!(!is_locked_in(&morph_core::profiles::profile_dir(&home, "default").join("companions")));
        // Its settings stay plaintext: there is no key to seal them with.
        morph_core::settings::save(&home, &Default::default()).unwrap();
        assert!(morph_core::profiles::settings_path(&home, "default").exists());
    }

    #[test]
    fn adding_an_identity_turns_the_lock_on() {
        let (_tmp, home) = home();
        morph_core::settings::save(&home, &Default::default()).unwrap();
        with_identity(&home, "default");
        let s = status(&home, "default").unwrap();
        assert!(s.has_identity && s.require_password && s.unlocked);
        // The plaintext settings from before were sealed when the identity came.
        let settings = morph_core::profiles::settings_path(&home, "default");
        assert!(!settings.exists() && profile_files::sealed_path(&settings).exists());
        let dir = morph_core::profiles::profile_dir(&home, "default");
        assert!(
            morph_keystore::load(&dir, &crate::identity::load(&home, "default").unwrap().unwrap().user_id).unwrap().is_none(),
            "the key is not kept"
        );
    }

    #[test]
    fn a_locked_profile_needs_its_password_and_refuses_mcp_and_sync() {
        let (_tmp, home) = home();
        with_identity(&home, "default");
        use morph_core::session::{self, Session, WindowSession};
        let tabs = Session::new(vec![WindowSession { tabs: vec!["https://private.example/".into()], active: 0 }]);
        session::save(&home, "default", &tabs).unwrap();
        let tabs_file = profile_files::sealed_path(&morph_core::profiles::tabs_path(&home, "default"));
        assert!(!String::from_utf8_lossy(&std::fs::read(&tabs_file).unwrap()).contains("private.example"), "tabs are not plaintext");
        lock(&home, "default");
        assert!(is_locked(&home, "default"));
        assert_eq!(session::load(&home, "default"), Session::default(), "a locked profile shows no tabs");
        assert!(session::save(&home, "default", &tabs).is_err(), "nor saves any");
        assert!(session::save(&home, "default", &Session::default()).is_err(), "nor removes them");
        assert!(tabs_file.exists());
        assert_eq!(library::open(&home, "default").unwrap(), LibraryOpen::NeedsPassword { reason: None });
        assert!(matches!(require_unlocked(&home, "default"), Err(Error::Locked)));
        assert!(!crate::sync::mcp_may_start(&home, "default", "any-server"), "MCP fails closed while locked");
        let companions = morph_core::profiles::profile_dir(&home, "default").join("companions");
        assert!(is_locked_in(&companions), "the MCP host runs no servers for it");
        let refused = crate::device_calls::DeviceCaller::for_profile(&home, "default").err().expect("no device calls while locked");
        assert_eq!(refused.to_string(), "profile locked");
        assert!(require_unlocked(&home, "default").unwrap_err().to_string().starts_with("profile locked"));
        assert!(matches!(crate::sync::start(&home, "default"), Err(Error::Locked)), "no sync node while locked");
        assert!(!crate::sync::is_running(&home, "default"));
        assert!(morph_core::settings::load(&home).is_err(), "settings stay sealed");
        assert!(morph_core::bookmarks::tree(&home, "default").is_err(), "the library stays closed");
        let unlocked = library::unlock(&home, "default", &pw("profile password")).unwrap();
        assert!(unlocked.password_required && !unlocked.remembered);
        assert!(!is_locked(&home, "default"));
        morph_core::settings::load(&home).unwrap();
        assert_eq!(session::load(&home, "default"), tabs, "the tabs come back after the unlock");
    }

    #[test]
    fn a_key_an_older_build_kept_is_dropped_and_the_password_asked() {
        let (_tmp, home) = home();
        with_identity(&home, "default");
        let dir = morph_core::profiles::profile_dir(&home, "default");
        let user = crate::identity::load(&home, "default").unwrap().unwrap().user_id;
        let key = profile_identity::library_key(&dir, &pw("profile password")).unwrap();
        morph_keystore::save(&dir, &user, &key).unwrap();
        lock(&home, "default");
        assert_eq!(library::open(&home, "default").unwrap(), LibraryOpen::NeedsPassword { reason: None });
        assert!(morph_keystore::load(&dir, &user).unwrap().is_none());
    }

    #[test]
    fn a_wrong_password_is_refused_with_backoff() {
        let (_tmp, home) = home();
        with_identity(&home, "default");
        lock(&home, "default");
        for _ in 0..profile_lock::FREE_ATTEMPTS {
            assert!(matches!(library::unlock(&home, "default", &pw("wrong password")), Err(Error::WrongPassword)));
        }
        // Now even the right password waits: the attempt is refused without trying it.
        assert!(matches!(library::unlock(&home, "default", &pw("profile password")), Err(Error::TooSoon { wait_ms }) if wait_ms > 0));
        assert!(is_locked(&home, "default"));
        // After the wait the right one works and clears the count.
        let cfg = profile_lock::config(&home, "default");
        let mut earlier = cfg.clone();
        earlier.last_failure_ms -= profile_lock::backoff_ms(cfg.failures);
        std::fs::write(profile_lock::config_path(&home, "default"), serde_json::to_vec(&earlier).unwrap()).unwrap();
        library::unlock(&home, "default", &pw("profile password")).unwrap();
        assert_eq!(profile_lock::config(&home, "default").failures, 0);
    }

    #[test]
    fn turning_the_password_off_needs_it_and_keeps_the_files_encrypted() {
        let (_tmp, home) = home();
        with_identity(&home, "default");
        morph_core::settings::save(&home, &Default::default()).unwrap();
        assert!(matches!(set_require_password(&home, "default", &pw("wrong password"), false), Err(Error::WrongPassword)));
        set_require_password(&home, "default", &pw("profile password"), false).unwrap();
        let s = status(&home, "default").unwrap();
        assert!(!s.require_password);
        lock(&home, "default");
        assert_eq!(library::open(&home, "default").unwrap(), LibraryOpen::Ready, "opens from the keystore");
        let settings = morph_core::profiles::settings_path(&home, "default");
        assert!(!settings.exists() && profile_files::sealed_path(&settings).exists(), "still encrypted at rest");
        // On again: the key leaves the keystore.
        set_require_password(&home, "default", &pw("profile password"), true).unwrap();
        lock(&home, "default");
        assert_eq!(library::open(&home, "default").unwrap(), LibraryOpen::NeedsPassword { reason: None });
    }

    #[test]
    fn policy_keeps_the_password_on() {
        let (_tmp, home) = home();
        with_identity(&home, "default");
        std::fs::write(morph_core::network::policy_path(&home), "{\"require_profile_password\": true}").unwrap();
        assert!(status(&home, "default").unwrap().policy_locked);
        assert!(set_require_password(&home, "default", &pw("profile password"), false).is_err());
        assert!(status(&home, "default").unwrap().require_password);
    }

    #[test]
    fn a_password_change_needs_the_current_one_and_keeps_the_data() {
        let (_tmp, home) = home();
        with_identity(&home, "default");
        morph_core::bookmarks::add_bookmark(&home, "default", "https://kept.example/", "Kept", 1).unwrap();
        assert!(matches!(change_password(&home, "default", &pw("wrong password"), &pw("new password")), Err(Error::WrongPassword)));
        change_password(&home, "default", &pw("profile password"), &pw("new password")).unwrap();
        lock(&home, "default");
        assert!(matches!(library::unlock(&home, "default", &pw("profile password")), Err(Error::WrongPassword)));
        library::unlock(&home, "default", &pw("new password")).unwrap();
        morph_core::settings::load(&home).unwrap();
        assert!(morph_core::bookmarks::tree(&home, "default").unwrap().bookmarks.iter().any(|b| b.url == "https://kept.example/"));
    }
}
