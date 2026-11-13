//! Each profile's library key, kept in the operating system's keystore so the
//! encrypted library opens at launch without asking for the profile password.
//!
//! | Platform | Store |
//! |---|---|
//! | Android | Android Keystore (AES-GCM key held by the keystore; the wrapped entry lives in SharedPreferences) |
//! | macOS, iOS | Keychain |
//! | Windows | Credential Manager (DPAPI) |
//! | Linux, BSD | Secret Service over D-Bus (GNOME Keyring, KWallet, KeePassXC) |
//!
//! One entry per profile: service [`SERVICE`], account `library-key/<hash>`,
//! where the hash is of the profile directory's path, so two profiles (or the
//! same profile id in two Morph homes) never share an entry. The entry holds the
//! 32-byte [`LibraryKey`] itself. The keystore is a convenience: if it is
//! missing, locked or unreachable (no Secret Service running, say), callers ask
//! for the profile password and derive the key from the identity instead.
//! Errors never include the key.
use std::path::Path;
use std::sync::{Arc, OnceLock};

use keyring_core::{CredentialStore, Entry, Error};
use morph_identity::profile_identity::LibraryKey;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// The keystore service name for every Morph entry.
pub const SERVICE: &str = "org.weirdbrains.morph";

/// Set `MORPH_KEYSTORE=off` to never use the OS keystore (always ask for the
/// password). For debugging and isolated test runs.
pub const ENV_OFF: &str = "MORPH_KEYSTORE";

/// The keystore could not be used. The message names the cause, never a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable(pub String);

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "system keystore unavailable: {}", self.0)
    }
}

impl std::error::Error for Unavailable {}

pub type Result<T> = std::result::Result<T, Unavailable>;

static STORE: OnceLock<std::result::Result<(), String>> = OnceLock::new();

fn platform_store() -> keyring_core::Result<Arc<CredentialStore>> {
    #[cfg(any(test, feature = "mock"))]
    {
        Ok(keyring_core::mock::Store::new()?)
    }
    #[cfg(not(any(test, feature = "mock")))]
    {
        #[cfg(target_os = "android")]
        {
            Ok(android_native_keyring_store::Store::new()?)
        }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            Ok(apple_native_keyring_store::keychain::Store::new()?)
        }
        #[cfg(windows)]
        {
            Ok(windows_native_keyring_store::Store::new()?)
        }
        #[cfg(all(unix, not(any(target_os = "android", target_os = "macos", target_os = "ios"))))]
        {
            Ok(zbus_secret_service_keyring_store::Store::new()?)
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(Error::NotSupportedByStore("no keystore on this platform".into()))
        }
    }
}

/// Connect to the platform keystore once per process.
fn ready() -> Result<()> {
    STORE
        .get_or_init(|| {
            if !cfg!(any(test, feature = "mock")) && std::env::var(ENV_OFF).is_ok_and(|v| v == "off") {
                return Err(format!("turned off by {ENV_OFF}=off"));
            }
            let store = platform_store().map_err(describe)?;
            keyring_core::set_default_store(store);
            Ok(())
        })
        .clone()
        .map_err(Unavailable)
}

/// What went wrong, without echoing any stored bytes.
fn describe(e: Error) -> String {
    match e {
        Error::BadEncoding(_) | Error::BadDataFormat(..) => "the stored entry is unreadable".into(),
        Error::Ambiguous(_) => "more than one matching entry".into(),
        other => other.to_string(),
    }
}

/// The account name for a profile: a hash of its directory, so it reveals
/// nothing about the profile and is unique per Morph home.
pub fn account(profile_dir: &Path) -> String {
    let path = std::fs::canonicalize(profile_dir).unwrap_or_else(|_| profile_dir.to_path_buf());
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("library-key/{hex}")
}

fn entry(profile_dir: &Path) -> Result<Entry> {
    ready()?;
    Entry::new(SERVICE, &account(profile_dir)).map_err(|e| Unavailable(describe(e)))
}

/// Keep `key` for the profile at `profile_dir`, replacing any earlier one.
pub fn save(profile_dir: &Path, key: &LibraryKey) -> Result<()> {
    entry(profile_dir)?.set_secret(key.expose()).map_err(|e| Unavailable(describe(e)))
}

/// The profile's saved key, or `None` if there is none (or what is there is
/// not a key, which is then removed).
pub fn load(profile_dir: &Path) -> Result<Option<LibraryKey>> {
    let entry = entry(profile_dir)?;
    match entry.get_secret() {
        Ok(bytes) => {
            let bytes = Zeroizing::new(bytes);
            match <[u8; 32]>::try_from(bytes.as_slice()) {
                Ok(raw) => Ok(Some(LibraryKey::from_bytes(raw))),
                Err(_) => {
                    let _ = entry.delete_credential();
                    Ok(None)
                }
            }
        }
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(Unavailable(describe(e))),
    }
}

/// Remove the profile's saved key. Nothing saved is not an error.
pub fn forget(profile_dir: &Path) -> Result<()> {
    match entry(profile_dir)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(Unavailable(describe(e))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_round_trips_and_is_forgotten() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p1");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).unwrap().is_none());
        save(&dir, &LibraryKey::from_bytes([3; 32])).unwrap();
        assert_eq!(load(&dir).unwrap().unwrap().expose(), &[3; 32]);
        save(&dir, &LibraryKey::from_bytes([4; 32])).unwrap();
        assert_eq!(load(&dir).unwrap().unwrap().expose(), &[4; 32], "a save replaces the key");
        forget(&dir).unwrap();
        assert!(load(&dir).unwrap().is_none());
        forget(&dir).unwrap();
    }

    #[test]
    fn profiles_have_their_own_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("h1/profiles/default"), tmp.path().join("h2/profiles/default"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        assert_ne!(account(&a), account(&b), "same profile id, different homes");
        save(&a, &LibraryKey::from_bytes([1; 32])).unwrap();
        assert!(load(&b).unwrap().is_none());
        save(&b, &LibraryKey::from_bytes([2; 32])).unwrap();
        forget(&a).unwrap();
        assert_eq!(load(&b).unwrap().unwrap().expose(), &[2; 32]);
    }

    #[test]
    fn something_that_is_not_a_key_reads_as_none_and_goes() {
        let tmp = tempfile::tempdir().unwrap();
        entry(tmp.path()).unwrap().set_secret(b"short").unwrap();
        assert!(load(tmp.path()).unwrap().is_none());
        assert!(matches!(entry(tmp.path()).unwrap().get_secret(), Err(Error::NoEntry)));
    }

    #[test]
    fn the_account_does_not_show_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let name = account(&tmp.path().join("secret-profile-name"));
        assert!(name.starts_with("library-key/") && !name.contains("secret"));
        assert_eq!(name.len(), "library-key/".len() + 32);
    }
}
