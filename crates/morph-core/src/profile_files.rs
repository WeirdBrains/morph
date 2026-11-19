//! Morph's own small per-profile files (`settings.json`, `tabs.json`),
//! encrypted at rest once the profile has a Weirdnet identity.
//!
//! The file key is derived from the profile's [`LibraryKey`] with HKDF
//! (info `morph/profile-files/v1`), so it exists only while the library is
//! unlocked in this process ([`crate::library_db::unlock`] keeps it,
//! [`crate::library_db::lock`] drops it). Each file is sealed with
//! XChaCha20-Poly1305 (`weirdnet_keys::seal`), bound to its file name, and
//! written next to where the plaintext was as `<name>.sealed`.
//!
//! A profile without an identity keeps plaintext files: it has no key.
//! A plaintext file left from before the identity is sealed at the next
//! unlock ([`seal_existing`]) or the next read, and removed only after its
//! sealed copy is on disk, so a crash at any point loses nothing.
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use morph_identity::profile_identity::LibraryKey;
use serde::{Deserialize, Serialize};
use weirdnet_keys::{Key32, Sealed, Zeroizing};

use crate::profiles::{self, ProfileError};

/// HKDF info for the file key. Bump to rotate.
const FILE_KEY_INFO: &[u8] = b"morph/profile-files/v1";
/// Format tag inside a sealed file.
pub const FORMAT: &str = "morph-sealed/v1";
/// Morph's own files under the profile directory that are sealed.
pub const SEALED_FILES: &[&str] = &["settings.json", "tabs.json"];

static KEYS: Mutex<BTreeMap<PathBuf, Key32>> = Mutex::new(BTreeMap::new());

fn keys() -> MutexGuard<'static, BTreeMap<PathBuf, Key32>> {
    KEYS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Derive and keep the file key of the profile at `dir` (called on unlock).
pub(crate) fn remember_key(dir: &Path, library_key: &LibraryKey) -> Result<(), ProfileError> {
    let key = weirdnet_keys::kdf::hkdf(library_key.expose(), None, FILE_KEY_INFO).map_err(|e| std::io::Error::other(e.to_string()))?;
    keys().insert(dir.to_path_buf(), key);
    Ok(())
}

/// Drop the file key of the profile at `dir` (zeroed on drop).
pub(crate) fn forget_key(dir: &Path) {
    keys().remove(dir);
}

fn key_for(dir: &Path) -> Option<Key32> {
    keys().get(dir).cloned()
}

#[derive(Serialize, Deserialize)]
struct SealedFile {
    format: String,
    #[serde(flatten)]
    sealed: Sealed,
}

/// `<path>.sealed`.
pub fn sealed_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sealed", path.display()))
}

fn aad(path: &Path) -> Vec<u8> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    [b"morph/profile-file/v1/".as_slice(), name.as_bytes()].concat()
}

/// Whether `id`'s files are sealed (it has an identity, so it has a key).
pub fn is_sealed(home: &Path, id: &str) -> bool {
    crate::profile_lock::has_identity(home, id)
}

/// Read one of `id`'s files: `None` if it does not exist. Fails with
/// [`ProfileError::Locked`] when the profile's files are sealed and its key
/// is not loaded, rather than ever falling back to a stale plaintext copy.
pub fn read(home: &Path, id: &str, path: &Path) -> Result<Option<Zeroizing<Vec<u8>>>, ProfileError> {
    if !is_sealed(home, id) {
        return match fs::read(path) {
            Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        };
    }
    let key = key_for(&profiles::profile_dir(home, id)).ok_or(ProfileError::Locked)?;
    let sealed = sealed_path(path);
    match fs::read(&sealed) {
        Ok(raw) => {
            let file: SealedFile = serde_json::from_slice(&raw)?;
            if file.format != FORMAT {
                return Err(std::io::Error::other(format!("{}: unsupported format", sealed.display())).into());
            }
            let plain = weirdnet_keys::open(&key, &file.sealed, &aad(path))
                .map_err(|_| std::io::Error::other(format!("{} does not open with this profile's key", sealed.display())))?;
            // A plaintext copy left by a crash during sealing goes now.
            remove_if_present(path)?;
            Ok(Some(plain))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::read(path) {
            Ok(bytes) => {
                let bytes = Zeroizing::new(bytes);
                write_sealed(&key, path, &bytes)?;
                remove_if_present(path)?;
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        },
        Err(e) => Err(e.into()),
    }
}

/// Write one of `id`'s files (atomic replace): sealed when the profile has an
/// identity (refused with [`ProfileError::Locked`] while it is locked), plain
/// otherwise.
pub fn write(home: &Path, id: &str, path: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !is_sealed(home, id) {
        return write_atomic(path, bytes);
    }
    let key = key_for(&profiles::profile_dir(home, id)).ok_or(ProfileError::Locked)?;
    write_sealed(&key, path, bytes)?;
    remove_if_present(path)
}

/// Refuse with [`ProfileError::Locked`] while `id`'s files are sealed and
/// its key is not loaded: nothing may replace or remove them then.
pub fn require_key(home: &Path, id: &str) -> Result<(), ProfileError> {
    if is_sealed(home, id) && key_for(&profiles::profile_dir(home, id)).is_none() {
        return Err(ProfileError::Locked);
    }
    Ok(())
}

/// When one of `id`'s files last changed (sealed or plain), if it exists.
pub fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(sealed_path(path)).or_else(|_| fs::metadata(path)).and_then(|m| m.modified()).ok()
}

/// Seal every plaintext file in [`SEALED_FILES`] of an unlocked profile with
/// an identity. Called on unlock, so a profile that just got an identity, or
/// one upgraded from a build that kept them plain, is encrypted right away.
pub fn seal_existing(home: &Path, id: &str) -> Result<(), ProfileError> {
    if !is_sealed(home, id) {
        return Ok(());
    }
    let dir = profiles::profile_dir(home, id);
    for name in SEALED_FILES {
        let path = dir.join(name);
        if path.exists() {
            read(home, id, &path)?;
        }
    }
    Ok(())
}

fn write_sealed(key: &Key32, path: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    let sealed = weirdnet_keys::seal(key, bytes, &aad(path)).map_err(|e| std::io::Error::other(e.to_string()))?;
    let body = serde_json::to_vec(&SealedFile { format: FORMAT.into(), sealed })?;
    write_atomic(&sealed_path(path), &body)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProfileError> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
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
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), ProfileError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_with_identity() -> (tempfile::TempDir, PathBuf, LibraryKey) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        let made =
            morph_identity::profile_identity::create(&profiles::profile_dir(&home, "default"), &"a long password".to_string().into(), 1).unwrap();
        (tmp, home, made.library_key)
    }

    #[test]
    fn a_profile_without_an_identity_stays_plain() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        let path = profiles::settings_path(&home, "default");
        write(&home, "default", &path, b"{\"theme\":\"dark\"}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"theme\":\"dark\"}");
        assert!(!sealed_path(&path).exists());
        assert_eq!(read(&home, "default", &path).unwrap().unwrap().as_slice(), b"{\"theme\":\"dark\"}");
    }

    #[test]
    fn sealed_files_are_not_plaintext_and_need_the_key() {
        let (_tmp, home, key) = home_with_identity();
        let dir = profiles::profile_dir(&home, "default");
        let path = profiles::tabs_path(&home, "default");
        assert!(matches!(write(&home, "default", &path, b"https://secret.example/"), Err(ProfileError::Locked)), "no key, no write");
        remember_key(&dir, &key).unwrap();
        write(&home, "default", &path, b"https://secret.example/").unwrap();
        assert!(!path.exists());
        let raw = fs::read(sealed_path(&path)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("secret.example"));
        assert_eq!(read(&home, "default", &path).unwrap().unwrap().as_slice(), b"https://secret.example/");
        forget_key(&dir);
        assert!(matches!(read(&home, "default", &path), Err(ProfileError::Locked)));
        // A file sealed for one name does not open as another.
        remember_key(&dir, &key).unwrap();
        let other = profiles::settings_path(&home, "default");
        fs::copy(sealed_path(&path), sealed_path(&other)).unwrap();
        assert!(read(&home, "default", &other).is_err());
        // Nor with another profile's key.
        remember_key(&dir, &LibraryKey::from_bytes([9; 32])).unwrap();
        assert!(read(&home, "default", &path).is_err());
        forget_key(&dir);
    }

    #[test]
    fn plaintext_left_from_before_is_sealed_on_unlock() {
        let (_tmp, home, key) = home_with_identity();
        let dir = profiles::profile_dir(&home, "default");
        let settings = profiles::settings_path(&home, "default");
        let tabs = profiles::tabs_path(&home, "default");
        fs::write(&settings, b"{\"home_page\":\"https://plain.example/\"}").unwrap();
        fs::write(&tabs, b"tabs").unwrap();
        remember_key(&dir, &key).unwrap();
        seal_existing(&home, "default").unwrap();
        assert!(!settings.exists() && !tabs.exists());
        assert!(sealed_path(&settings).exists() && sealed_path(&tabs).exists());
        assert_eq!(read(&home, "default", &settings).unwrap().unwrap().as_slice(), b"{\"home_page\":\"https://plain.example/\"}");
        // A crash after sealing but before the plaintext went: the sealed copy wins and the plain one goes.
        fs::write(&tabs, b"stale").unwrap();
        assert_eq!(read(&home, "default", &tabs).unwrap().unwrap().as_slice(), b"tabs");
        assert!(!tabs.exists());
        forget_key(&dir);
    }
}
