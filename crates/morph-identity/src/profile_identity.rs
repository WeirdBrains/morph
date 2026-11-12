//! One Weirdnet user identity per Morph profile.
//!
//! Each profile directory holds its own `identity/`:
//! - `user.key`: the 32-byte user-key seed, sealed under the profile's password
//!   (`weirdnet_keys::password_seal`, Argon2id then XChaCha20-Poly1305);
//! - `identity.json`: public facts only (user id, fingerprint, when, how).
//!
//! The Ed25519 user key is derived from the seed, and the seed is what the 24
//! recovery words encode (`weirdnet_keys::words`), so the words alone restore the
//! same identity in another profile or on another device. Nothing here is shared
//! between profiles, and nothing secret is written unsealed.
//!
//! The profile's library database key ([`LibraryKey`]) is derived from the same
//! seed with HKDF-SHA256, so it exists only while the identity is unlocked.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use weirdnet_keys::words::{from_words, to_words};
use weirdnet_keys::{ExposeSecret, Key32, PasswordSealed, password_open, password_seal};
use weirdnet_net::Keypair;
use weirdnet_net::identity::fingerprint;
use weirdnet_net::user::user_id;

use crate::{Error, Result, UserIdentity};

pub use weirdnet_keys::{SecretString, Zeroizing};

/// Format tag in `identity.json`.
pub const FORMAT: &str = "morph-identity/v1";
/// Shortest password accepted for sealing a profile's user key.
pub const MIN_PASSWORD_LEN: usize = 8;
const SEED_AAD: &[u8] = b"morph/identity/user-seed/v1";

/// How a profile got its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Created,
    Imported,
    /// Moved here from another device over a QR link (`device_link`).
    Linked,
}

/// What a profile's identity says about itself. Public; safe to show and log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileIdentity {
    pub format: String,
    pub user_id: String,
    pub fingerprint: String,
    pub created_ms: i64,
    pub source: Source,
}

/// HKDF info for the library database key. Bump the version to rotate keys.
const LIBRARY_KEY_INFO: &[u8] = b"morph/library-db/v1";

/// The 32-byte key of a profile's library database. Zeroed on drop; its
/// `Debug` never shows the bytes.
#[derive(Clone)]
pub struct LibraryKey(Zeroizing<[u8; 32]>);

impl LibraryKey {
    /// HKDF-SHA256 over the identity seed (no salt; the info string binds it
    /// to this use).
    fn derive(seed: &Key32) -> Self {
        let mut out = Zeroizing::new([0u8; 32]);
        hkdf::Hkdf::<sha2::Sha256>::new(None, &seed.0).expand(LIBRARY_KEY_INFO, out.as_mut()).expect("32 bytes is a valid HKDF-SHA256 length");
        Self(out)
    }

    /// The raw key bytes, for handing to SQLCipher.
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }

    /// A key from raw bytes (tests and callers that hold one already).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }
}

impl std::fmt::Debug for LibraryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LibraryKey(..)")
    }
}

/// A new identity and its recovery words. Show the words once, then drop this.
pub struct Created {
    pub identity: ProfileIdentity,
    pub words: Zeroizing<String>,
    /// The library database key, so the caller can encrypt the library now
    /// without asking for the password again.
    pub library_key: LibraryKey,
}

pub fn identity_dir(profile_dir: &Path) -> PathBuf {
    profile_dir.join("identity")
}

fn public_path(profile_dir: &Path) -> PathBuf {
    identity_dir(profile_dir).join("identity.json")
}

pub(crate) fn sealed_path(profile_dir: &Path) -> PathBuf {
    identity_dir(profile_dir).join("user.key")
}

/// The profile's identity, or `None` before onboarding.
pub fn load(profile_dir: &Path) -> Result<Option<ProfileIdentity>> {
    let public = match fs::read(public_path(profile_dir)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !sealed_path(profile_dir).exists() {
        return Err(Error::Message("identity key is missing".into()));
    }
    let identity: ProfileIdentity = serde_json::from_slice(&public).map_err(|e| Error::Message(format!("identity.json: {e}")))?;
    if identity.format != FORMAT {
        return Err(Error::Message(format!("unsupported identity format {:?}", identity.format)));
    }
    Ok(Some(identity))
}

/// Give the profile a new identity sealed under `password`.
pub fn create(profile_dir: &Path, password: &SecretString, now_ms: i64) -> Result<Created> {
    let seed = Key32::random()?;
    let identity = store(profile_dir, &seed, password, Source::Created, now_ms)?;
    Ok(Created { identity, words: to_words(&seed)?, library_key: LibraryKey::derive(&seed) })
}

/// Restore an identity from its 24 recovery words into this profile.
pub fn import(profile_dir: &Path, words: &str, password: &SecretString, now_ms: i64) -> Result<ProfileIdentity> {
    Ok(import_unlocked(profile_dir, words, password, now_ms)?.0)
}

/// [`import`], also returning the library database key.
pub fn import_unlocked(profile_dir: &Path, words: &str, password: &SecretString, now_ms: i64) -> Result<(ProfileIdentity, LibraryKey)> {
    let seed = from_words(words)?;
    let identity = store(profile_dir, &seed, password, Source::Imported, now_ms)?;
    Ok((identity, LibraryKey::derive(&seed)))
}

/// Open the profile's sealed seed with its password and derive the library
/// database key from it.
pub fn library_key(profile_dir: &Path, password: &SecretString) -> Result<LibraryKey> {
    Ok(LibraryKey::derive(&open_seed(profile_dir, password)?))
}

/// Open the profile's user key with its password.
pub fn unlock(profile_dir: &Path, password: &SecretString) -> Result<UserIdentity> {
    user_from_seed(&open_seed(profile_dir, password)?)
}

/// The recovery words for the profile's identity, after checking the password.
pub fn recovery_words(profile_dir: &Path, password: &SecretString) -> Result<Zeroizing<String>> {
    Ok(to_words(&open_seed(profile_dir, password)?)?)
}

pub(crate) fn open_seed(profile_dir: &Path, password: &SecretString) -> Result<Key32> {
    let identity = load(profile_dir)?.ok_or_else(|| Error::Message("this profile has no identity yet".into()))?;
    let raw = fs::read(sealed_path(profile_dir))?;
    let sealed: PasswordSealed = serde_json::from_slice(&raw).map_err(|e| Error::Message(format!("user.key: {e}")))?;
    let seed = Key32::from_slice(&password_open(password, &sealed, SEED_AAD)?)?;
    if user_from_seed(&seed)?.user_id != identity.user_id {
        return Err(Error::Message("identity key does not match identity.json".into()));
    }
    Ok(seed)
}

fn user_from_seed(seed: &Key32) -> Result<UserIdentity> {
    let mut bytes = Zeroizing::new(seed.0);
    let keypair = Keypair::ed25519_from_bytes(bytes.as_mut()).map_err(|e| Error::Message(format!("user key: {e}")))?;
    let user_id = user_id(&keypair.public());
    Ok(UserIdentity { keypair, user_id })
}

pub(crate) fn check_password(password: &SecretString) -> Result<()> {
    if password.expose_secret().chars().count() < MIN_PASSWORD_LEN {
        return Err(Error::Message(format!("password must be at least {MIN_PASSWORD_LEN} characters")));
    }
    Ok(())
}

pub(crate) fn store(profile_dir: &Path, seed: &Key32, password: &SecretString, source: Source, now_ms: i64) -> Result<ProfileIdentity> {
    check_password(password)?;
    if public_path(profile_dir).exists() || sealed_path(profile_dir).exists() {
        return Err(Error::Message("this profile already has an identity".into()));
    }
    let user = user_from_seed(seed)?;
    let identity = ProfileIdentity {
        format: FORMAT.to_string(),
        fingerprint: fingerprint(&user.public_key()),
        user_id: user.user_id,
        created_ms: now_ms,
        source,
    };
    let sealed = password_seal(password, &seed.0, SEED_AAD)?;
    let sealed = serde_json::to_vec_pretty(&sealed).map_err(|e| Error::Message(e.to_string()))?;
    let public = serde_json::to_vec_pretty(&identity).map_err(|e| Error::Message(e.to_string()))?;
    let dir = identity_dir(profile_dir);
    fs::create_dir_all(&dir)?;
    private_dir(&dir)?;
    // The sealed key goes first: identity.json without user.key reads as damaged,
    // never as an identity that silently has no key.
    write_private(&sealed_path(profile_dir), &sealed)?;
    if let Err(e) = write_private(&public_path(profile_dir), &public) {
        let _ = fs::remove_file(sealed_path(profile_dir));
        return Err(e);
    }
    Ok(identity)
}

#[cfg(unix)]
fn private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn private_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    #[test]
    fn create_then_load_and_unlock() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("work");
        assert!(load(&dir).unwrap().is_none());
        let made = create(&dir, &pw("correct horse"), 42).unwrap();
        assert_eq!(made.words.split_whitespace().count(), 24);
        assert_eq!(made.identity.source, Source::Created);
        assert_eq!(made.identity.created_ms, 42);
        assert_eq!(load(&dir).unwrap().unwrap(), made.identity);
        let user = unlock(&dir, &pw("correct horse")).unwrap();
        assert_eq!(user.user_id, made.identity.user_id);
        assert_eq!(user.fingerprint(), made.identity.fingerprint);
        assert_eq!(recovery_words(&dir, &pw("correct horse")).unwrap().as_str(), made.words.as_str());
    }

    #[test]
    fn nothing_secret_is_written_in_the_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        let made = create(&dir, &pw("correct horse"), 1).unwrap();
        let seed = from_words(&made.words).unwrap();
        for entry in fs::read_dir(identity_dir(&dir)).unwrap() {
            let body = fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(!body.contains(seed.hex().as_str()));
            assert!(!body.contains(made.words.as_str()));
            assert!(!body.contains("correct horse"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(sealed_path(&dir)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let mode = fs::metadata(identity_dir(&dir)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    #[test]
    fn wrong_password_does_not_unlock() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        create(&dir, &pw("correct horse"), 1).unwrap();
        assert!(unlock(&dir, &pw("wrong horse")).is_err());
        assert!(recovery_words(&dir, &pw("wrong horse")).is_err());
    }

    #[test]
    fn words_restore_the_same_identity_elsewhere() {
        let tmp = tempfile::tempdir().unwrap();
        let made = create(&tmp.path().join("a"), &pw("password-a"), 1).unwrap();
        let shouty = made.words.to_uppercase();
        let restored = import(&tmp.path().join("b"), &shouty, &pw("password-b"), 2).unwrap();
        assert_eq!(restored.user_id, made.identity.user_id);
        assert_eq!(restored.fingerprint, made.identity.fingerprint);
        assert_eq!(restored.source, Source::Imported);
        assert!(unlock(&tmp.path().join("b"), &pw("password-a")).is_err());
        assert!(unlock(&tmp.path().join("b"), &pw("password-b")).is_ok());
    }

    #[test]
    fn profiles_get_different_identities() {
        let tmp = tempfile::tempdir().unwrap();
        let a = create(&tmp.path().join("a"), &pw("password"), 1).unwrap();
        let b = create(&tmp.path().join("b"), &pw("password"), 1).unwrap();
        assert_ne!(a.identity.user_id, b.identity.user_id);
    }

    #[test]
    fn refuses_short_passwords_bad_words_and_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        assert!(create(&dir, &pw("short"), 1).is_err());
        assert!(!identity_dir(&dir).exists());
        assert!(import(&dir, "abandon abandon", &pw("long enough"), 1).is_err());
        assert!(!identity_dir(&dir).exists());
        let first = create(&dir, &pw("long enough"), 1).unwrap();
        assert!(create(&dir, &pw("long enough"), 2).is_err());
        assert!(import(&dir, &first.words, &pw("long enough"), 2).is_err());
        assert_eq!(load(&dir).unwrap().unwrap(), first.identity);
    }

    #[test]
    fn the_library_key_follows_the_seed_not_the_password() {
        let tmp = tempfile::tempdir().unwrap();
        let made = create(&tmp.path().join("a"), &pw("password-a"), 1).unwrap();
        let again = library_key(&tmp.path().join("a"), &pw("password-a")).unwrap();
        assert_eq!(again.expose(), made.library_key.expose());
        assert!(library_key(&tmp.path().join("a"), &pw("wrong pass")).is_err());
        let (_, restored) = import_unlocked(&tmp.path().join("b"), &made.words, &pw("password-b"), 2).unwrap();
        assert_eq!(restored.expose(), made.library_key.expose(), "same words, same library key");
        let other = create(&tmp.path().join("c"), &pw("password-a"), 1).unwrap();
        assert_ne!(other.library_key.expose(), made.library_key.expose());
        // Not the seed itself, and never printed.
        let seed = from_words(&made.words).unwrap();
        assert_ne!(made.library_key.expose(), &seed.0);
        assert_eq!(format!("{:?}", made.library_key), "LibraryKey(..)");
    }

    #[test]
    fn a_public_file_without_its_key_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        create(&dir, &pw("long enough"), 1).unwrap();
        fs::remove_file(sealed_path(&dir)).unwrap();
        assert!(load(&dir).is_err());
    }
}
