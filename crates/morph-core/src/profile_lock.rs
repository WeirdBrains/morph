//! Whether a profile asks for its password before it opens, and the
//! wrong-password backoff.
//!
//! Only a profile with a Weirdnet identity can be locked: its library and
//! profile files are encrypted under a key derived from the identity seed,
//! which is sealed under the profile password (Argon2id, then
//! XChaCha20-Poly1305). A profile without an identity has no key, so it is
//! not encrypted and never locks.
//!
//! For a profile with an identity, the password is required by default
//! ([`LockConfig::require_password`] is on). Turning it off keeps the
//! library key in the system keystore so the profile opens without a prompt;
//! the files stay encrypted either way. An admin policy file can keep it on
//! (`"require_profile_password": true` in the same policy files as the
//! network policy: `MORPH_NETWORK_POLICY`, `<home>/network.policy.json`,
//! `/etc/morph/network.policy.json`).
//!
//! `<profile>/lock.json` holds the setting and the failed-attempt count. It
//! is plaintext because it is read before the profile is unlocked; editing it
//! gains nothing, because the key is in the keystore only when the user
//! turned the password off with the password.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profiles::{self, ProfileError};

/// Failed attempts allowed before the backoff starts.
pub const FREE_ATTEMPTS: u32 = 3;
/// The longest wait between attempts, in milliseconds.
pub const MAX_BACKOFF_MS: i64 = 5 * 60 * 1000;

/// `<profile>/lock.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockConfig {
    /// Ask for the password every time the profile opens (on by default).
    #[serde(default = "on")]
    pub require_password: bool,
    /// Wrong passwords in a row since the last unlock.
    #[serde(default)]
    pub failures: u32,
    /// When the last wrong password was tried (ms since the epoch).
    #[serde(default)]
    pub last_failure_ms: i64,
}

fn on() -> bool {
    true
}

impl Default for LockConfig {
    fn default() -> Self {
        Self { require_password: true, failures: 0, last_failure_ms: 0 }
    }
}

/// `<home>/profiles/<id>/lock.json`.
pub fn config_path(home: impl AsRef<Path>, id: &str) -> PathBuf {
    profiles::profile_dir(home, id).join("lock.json")
}

/// The profile's lock settings (defaults when there is no file or it is unreadable).
pub fn config(home: impl AsRef<Path>, id: &str) -> LockConfig {
    fs::read(config_path(home, id)).ok().and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default()
}

fn save_config(home: &Path, id: &str, config: &LockConfig) -> Result<(), ProfileError> {
    let path = config_path(home, id);
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(config)?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

/// Whether the profile has a Weirdnet identity. A damaged identity counts as
/// one, so it fails closed.
pub fn has_identity(home: impl AsRef<Path>, id: &str) -> bool {
    !matches!(morph_identity::profile_identity::load(&profiles::profile_dir(home, id)), Ok(None))
}

/// An admin policy file keeps the profile password on for every profile.
pub fn policy_requires_password(home: impl AsRef<Path>) -> bool {
    #[derive(Deserialize)]
    struct Policy {
        #[serde(default)]
        require_profile_password: Option<bool>,
    }
    crate::network::policy_files(home.as_ref()).iter().any(|path| {
        fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str::<Policy>(&raw).ok()).and_then(|p| p.require_profile_password) == Some(true)
    })
}

/// Whether opening the profile needs its password: it has an identity, and
/// the password is on (by default, or kept on by policy).
pub fn requires_password(home: impl AsRef<Path>, id: &str) -> bool {
    let home = home.as_ref();
    has_identity(home, id) && (policy_requires_password(home) || config(home, id).require_password)
}

/// Turn the password requirement on or off. Turning it off is refused while a
/// policy keeps it on. The caller checks the password and moves the key into
/// or out of the keystore.
pub fn set_require_password(home: impl AsRef<Path>, id: &str, on: bool) -> Result<(), ProfileError> {
    let home = home.as_ref();
    profiles::require(home, id)?;
    if !on && policy_requires_password(home) {
        return Err(ProfileError::PolicyLocked);
    }
    let mut config = config(home, id);
    config.require_password = on;
    save_config(home, id, &config)
}

/// How long to wait after `failures` wrong passwords: nothing for the first
/// [`FREE_ATTEMPTS`], then 1 s doubling up to [`MAX_BACKOFF_MS`].
pub fn backoff_ms(failures: u32) -> i64 {
    if failures < FREE_ATTEMPTS {
        return 0;
    }
    let step = (failures - FREE_ATTEMPTS).min(20);
    (1000i64 << step).min(MAX_BACKOFF_MS)
}

/// Milliseconds until the next attempt is allowed (0: now).
pub fn wait_ms(home: impl AsRef<Path>, id: &str, now_ms: i64) -> i64 {
    let config = config(home, id);
    let until = config.last_failure_ms.saturating_add(backoff_ms(config.failures));
    (until - now_ms).max(0)
}

/// Count a wrong password.
pub fn record_failure(home: impl AsRef<Path>, id: &str, now_ms: i64) -> Result<(), ProfileError> {
    let home = home.as_ref();
    let mut config = config(home, id);
    config.failures = config.failures.saturating_add(1);
    config.last_failure_ms = now_ms;
    save_config(home, id, &config)
}

/// A right password clears the count.
pub fn record_success(home: impl AsRef<Path>, id: &str) -> Result<(), ProfileError> {
    let home = home.as_ref();
    let mut config = config(home, id);
    if config.failures == 0 && config.last_failure_ms == 0 && config_path(home, id).exists() {
        return Ok(());
    }
    config.failures = 0;
    config.last_failure_ms = 0;
    save_config(home, id, &config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        (tmp, home)
    }

    #[test]
    fn a_profile_without_an_identity_never_needs_a_password() {
        let (_tmp, home) = home();
        assert!(config(&home, "default").require_password, "on by default");
        assert!(!has_identity(&home, "default"));
        assert!(!requires_password(&home, "default"));
        fs::write(crate::network::policy_path(&home), "{\"require_profile_password\": true}").unwrap();
        assert!(!requires_password(&home, "default"), "policy cannot lock a profile with nothing to encrypt");
    }

    #[test]
    fn an_identity_turns_the_lock_on_and_policy_keeps_it_on() {
        let (_tmp, home) = home();
        let dir = profiles::profile_dir(&home, "default");
        morph_identity::profile_identity::create(&dir, &"a long password".to_string().into(), 1).unwrap();
        assert!(requires_password(&home, "default"));
        set_require_password(&home, "default", false).unwrap();
        assert!(!requires_password(&home, "default"));
        fs::write(crate::network::policy_path(&home), "{\"require_profile_password\": true}").unwrap();
        assert!(requires_password(&home, "default"), "policy wins over the setting");
        assert!(matches!(set_require_password(&home, "default", false), Err(ProfileError::PolicyLocked)));
        set_require_password(&home, "default", true).unwrap();
    }

    #[test]
    fn a_damaged_identity_fails_closed() {
        let (_tmp, home) = home();
        let dir = profiles::profile_dir(&home, "default").join("identity");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("identity.json"), "{}").unwrap();
        assert!(has_identity(&home, "default"));
    }

    #[test]
    fn backoff_grows_and_a_success_clears_it() {
        let (_tmp, home) = home();
        assert_eq!((backoff_ms(0), backoff_ms(2), backoff_ms(3), backoff_ms(4)), (0, 0, 1000, 2000));
        assert_eq!(backoff_ms(1000), MAX_BACKOFF_MS);
        for _ in 0..FREE_ATTEMPTS {
            record_failure(&home, "default", 10_000).unwrap();
        }
        assert_eq!(wait_ms(&home, "default", 10_000), 1000);
        assert_eq!(wait_ms(&home, "default", 10_600), 400);
        assert_eq!(wait_ms(&home, "default", 11_000), 0);
        record_failure(&home, "default", 11_000).unwrap();
        assert_eq!(wait_ms(&home, "default", 11_000), 2000);
        record_success(&home, "default").unwrap();
        assert_eq!(wait_ms(&home, "default", 11_000), 0);
        assert!(config(&home, "default").require_password, "the count never touches the setting");
    }
}
