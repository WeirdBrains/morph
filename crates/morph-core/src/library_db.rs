//! Which profiles' library databases are unlocked in this process.
//!
//! A profile's bookmarks and history live in one of two places:
//! - `library.db`, encrypted with the profile's [`LibraryKey`] (see `morph-store`),
//!   once the profile has a Weirdnet identity and has been unlocked;
//! - the older plaintext `bookmarks.json` and `history.json`, only while the
//!   profile has never been unlocked (before onboarding).
//!
//! [`unlock`] opens the database and moves any JSON library into it, then
//! removes the JSON files. After that the profile is encrypted for good: while
//! it is locked, library calls fail with [`ProfileError::Locked`] rather than
//! falling back to plaintext. Keys live only inside the open connections held
//! here, and [`lock`] drops them.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use morph_identity::profile_identity::LibraryKey;
use morph_store::Library;

use crate::profiles::{self, ProfileError};

static OPEN: Mutex<BTreeMap<PathBuf, Library>> = Mutex::new(BTreeMap::new());

fn open_map() -> MutexGuard<'static, BTreeMap<PathBuf, Library>> {
    OPEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where a profile's library is right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryState {
    /// Plaintext JSON (never unlocked).
    Plain,
    /// Encrypted, and the key is not loaded.
    Locked,
    /// Encrypted and open.
    Unlocked,
}

/// The state of `id`'s library.
pub fn state(home: impl AsRef<Path>, id: &str) -> Result<LibraryState, ProfileError> {
    profiles::require(&home, id)?;
    let dir = profiles::profile_dir(home, id);
    Ok(if open_map().contains_key(&dir) {
        LibraryState::Unlocked
    } else if morph_store::exists(&dir) {
        LibraryState::Locked
    } else {
        LibraryState::Plain
    })
}

/// Open `id`'s library database with its key (creating it if needed), move
/// any plaintext JSON library into it, and keep it open for this process.
/// A wrong key is an error and changes nothing.
pub fn unlock(home: impl AsRef<Path>, id: &str, key: &LibraryKey) -> Result<(), ProfileError> {
    let home = home.as_ref();
    profiles::require(home, id)?;
    let dir = profiles::profile_dir(home, id);
    let mut map = open_map();
    let mut lib = Library::open(&dir, key)?;
    import_json(home, id, &mut lib)?;
    map.insert(dir, lib);
    Ok(())
}

/// [`unlock`] for a profile that just got a new identity. A `library.db` left
/// by an earlier identity cannot open with the new key; it is renamed to
/// `library.db.unreadable-<now_ms>` (kept, in case the old recovery words turn
/// up) and a fresh library starts.
pub fn unlock_new_identity(home: impl AsRef<Path>, id: &str, key: &LibraryKey, now_ms: i64) -> Result<(), ProfileError> {
    let home = home.as_ref();
    match unlock(home, id, key) {
        Err(ProfileError::Store(e)) if e.is_wrong_key() => {
            let db = morph_store::path(&profiles::profile_dir(home, id));
            let aside = |suffix: &str| PathBuf::from(format!("{}{suffix}.unreadable-{now_ms}", db.display()));
            fs::rename(&db, aside(""))?;
            for side in ["-wal", "-shm"] {
                let p = PathBuf::from(format!("{}{side}", db.display()));
                if p.exists() {
                    fs::rename(&p, aside(side))?;
                }
            }
            unlock(home, id, key)
        }
        other => other,
    }
}

/// Close `id`'s library and forget its key. Does nothing if it is not open.
pub fn lock(home: impl AsRef<Path>, id: &str) {
    open_map().remove(&profiles::profile_dir(home, id));
}

/// Run `f` on `id`'s open database: `Ok(None)` if the library is still
/// plaintext JSON, `Err(Locked)` if it is encrypted and not unlocked.
pub(crate) fn with<T>(home: &Path, id: &str, f: impl FnOnce(&mut Library) -> morph_store::Result<T>) -> Result<Option<T>, ProfileError> {
    let dir = profiles::profile_dir(home, id);
    let mut map = open_map();
    match map.get_mut(&dir) {
        Some(lib) => Ok(Some(f(lib)?)),
        None if morph_store::exists(&dir) => Err(ProfileError::Locked),
        None => Ok(None),
    }
}

/// Move the JSON library into `lib` (only if `lib` is empty, so a crash after
/// the import but before the files went cannot duplicate anything), then
/// remove the JSON files.
fn import_json(home: &Path, id: &str, lib: &mut Library) -> Result<(), ProfileError> {
    let bookmarks = crate::library::bookmarks_path(home, id);
    let history = crate::library::history_path(home, id);
    if !bookmarks.exists() && !history.exists() {
        return Ok(());
    }
    let set = crate::bookmarks::json_set(home, id)?;
    let visits = crate::library::json_history(home, id)?;
    lib.import(&set, &visits, crate::library::MAX_HISTORY)?;
    for path in [&bookmarks, &history] {
        for p in [path.clone(), path.with_extension("json.tmp")] {
            match fs::remove_file(&p) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bookmarks, library};

    fn home_with_profile() -> (tempfile::TempDir, PathBuf, String) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        let id = profiles::create(&home, "Work").unwrap().id;
        (tmp, home, id)
    }

    fn key(b: u8) -> LibraryKey {
        LibraryKey::from_bytes([b; 32])
    }

    #[test]
    fn unlocking_moves_the_json_library_into_the_encrypted_database() {
        let (_tmp, home, id) = home_with_profile();
        bookmarks::add_bookmark(&home, &id, "https://kept.example/", "Kept", 1).unwrap();
        let (folder, _) = bookmarks::create_folder(&home, &id, "Reading", None, 2).unwrap();
        let mine = bookmarks::bookmarks(&home, &id).unwrap()[0].id.clone();
        bookmarks::update_bookmark(&home, &id, &mine, bookmarks::BookmarkEdit { title: None, url: None, tags: Some(vec!["later".into()]) }, 3)
            .unwrap();
        bookmarks::move_item(&home, &id, &mine, Some(&folder), 0, 4).unwrap();
        library::record_visit(&home, &id, "https://a.example/", "A", 5).unwrap();
        library::record_visit(&home, &id, "https://b.example/", "B", 6).unwrap();
        let tree = bookmarks::tree(&home, &id).unwrap();
        let history = library::history(&home, &id, "", 0).unwrap();
        assert_eq!(state(&home, &id).unwrap(), LibraryState::Plain);

        unlock(&home, &id, &key(1)).unwrap();
        assert_eq!(state(&home, &id).unwrap(), LibraryState::Unlocked);
        assert!(!library::bookmarks_path(&home, &id).exists(), "plaintext bookmarks are gone");
        assert!(!library::history_path(&home, &id).exists(), "plaintext history is gone");
        assert_eq!(bookmarks::tree(&home, &id).unwrap(), tree);
        assert_eq!(library::history(&home, &id, "", 0).unwrap(), history);
        assert_eq!(bookmarks::tags(&home, &id).unwrap()[0].name, "later");

        let raw = fs::read(morph_store::path(&profiles::profile_dir(&home, &id))).unwrap();
        assert!(!raw.windows(12).any(|w| w == b"kept.example"));
    }

    #[test]
    fn a_locked_library_refuses_instead_of_writing_plaintext() {
        let (_tmp, home, id) = home_with_profile();
        unlock(&home, &id, &key(1)).unwrap();
        bookmarks::add_bookmark(&home, &id, "https://x.example/", "X", 1).unwrap();
        lock(&home, &id);
        assert_eq!(state(&home, &id).unwrap(), LibraryState::Locked);
        assert!(matches!(library::record_visit(&home, &id, "https://y.example/", "Y", 2), Err(ProfileError::Locked)));
        assert!(matches!(bookmarks::bookmarks(&home, &id), Err(bookmarks::BookmarkError::Profile(ProfileError::Locked))));
        assert!(matches!(library::history(&home, &id, "", 0), Err(ProfileError::Locked)));
        assert!(!library::bookmarks_path(&home, &id).exists() && !library::history_path(&home, &id).exists());

        assert!(unlock(&home, &id, &key(2)).is_err(), "wrong key");
        assert_eq!(state(&home, &id).unwrap(), LibraryState::Locked);
        unlock(&home, &id, &key(1)).unwrap();
        assert_eq!(bookmarks::bookmarks(&home, &id).unwrap().len(), 1);
    }

    #[test]
    fn library_calls_behave_the_same_on_the_database() {
        let (_tmp, home, id) = home_with_profile();
        unlock(&home, &id, &key(1)).unwrap();
        library::record_visit(&home, &id, "https://a.example/", "", 1).unwrap();
        library::record_visit(&home, &id, "https://a.example", "Alpha", 2).unwrap();
        library::record_visit(&home, &id, "about:blank", "", 3).unwrap();
        library::record_visit(&home, &id, "https://b.example/", "Beta", 4).unwrap();
        let all = library::history(&home, &id, "", 0).unwrap();
        assert_eq!(all.len(), 2, "a repeat of the newest page refreshes it");
        assert_eq!(all[1].title, "Alpha");
        assert_eq!(all[1].visited_ms, 2);
        assert_eq!(library::history(&home, &id, "ALPHA", 0).unwrap().len(), 1);
        assert_eq!(library::history(&home, &id, "", 1).unwrap()[0].url, "https://b.example/");
        library::delete_history_url(&home, &id, "https://a.example").unwrap();
        assert_eq!(library::history(&home, &id, "", 0).unwrap().len(), 1);
        library::clear_history(&home, &id).unwrap();
        assert!(library::history(&home, &id, "", 0).unwrap().is_empty());
        assert_eq!(state(&home, &id).unwrap(), LibraryState::Unlocked, "clearing history keeps the database");

        bookmarks::add_bookmark(&home, &id, "https://c.example/", "C", 5).unwrap();
        assert!(bookmarks::is_bookmarked(&home, &id, "https://c.example").unwrap());
        bookmarks::remove_bookmark(&home, &id, "https://c.example/", 6).unwrap();
        assert!(bookmarks::bookmarks(&home, &id).unwrap().is_empty());
        assert_eq!(bookmarks::tombstones(&home, &id).unwrap().len(), 1);
    }

    #[test]
    fn a_new_identity_sets_an_unreadable_library_aside() {
        let (_tmp, home, id) = home_with_profile();
        unlock(&home, &id, &key(1)).unwrap();
        bookmarks::add_bookmark(&home, &id, "https://old.example/", "Old", 1).unwrap();
        lock(&home, &id);
        unlock_new_identity(&home, &id, &key(2), 77).unwrap();
        assert!(bookmarks::bookmarks(&home, &id).unwrap().is_empty());
        let dir = profiles::profile_dir(&home, &id);
        assert!(dir.join("library.db.unreadable-77").exists(), "the old file is kept");
        lock(&home, &id);
        unlock_new_identity(&home, &id, &key(2), 78).unwrap();
        assert!(!dir.join("library.db.unreadable-78").exists(), "a readable library stays");
    }

    #[test]
    fn deleting_a_profile_closes_its_library() {
        let (_tmp, home, id) = home_with_profile();
        unlock(&home, &id, &key(1)).unwrap();
        profiles::delete(&home, &id).unwrap();
        assert!(!profiles::profile_dir(&home, &id).exists());
        assert!(!open_map().contains_key(&profiles::profile_dir(&home, &id)));
    }
}
