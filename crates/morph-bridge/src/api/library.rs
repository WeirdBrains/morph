//! Per-profile bookmarks (folders, tags) and history for the Flutter shell.

use morph_core::profiles::ProfileError;
use morph_core::{bookmarks, library, library_db};
use morph_identity::Paths;
use morph_identity::profile_identity;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A saved page.
pub struct BookmarkView {
    pub id: String,
    pub url: String,
    pub title: String,
    pub added_ms: i64,
    pub updated_ms: i64,
    /// Containing folder; `None` is the root.
    pub folder_id: Option<String>,
    /// Sort key among the folder's items (folders and bookmarks together).
    pub order: String,
    pub tags: Vec<String>,
}

/// A bookmark folder.
pub struct BookmarkFolderView {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub order: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// Every folder and bookmark of a profile, sorted by parent then order.
pub struct BookmarkTreeView {
    pub folders: Vec<BookmarkFolderView>,
    pub bookmarks: Vec<BookmarkView>,
}

/// A tag and how many bookmarks carry it.
pub struct BookmarkTagView {
    pub name: String,
    pub count: u32,
}

/// One visit to a page.
pub struct HistoryEntryView {
    pub url: String,
    pub title: String,
    pub visited_ms: i64,
}

fn resolve_home(home: Option<String>) -> Result<PathBuf, String> {
    let explicit = home.filter(|h| !h.is_empty()).map(PathBuf::from);
    Ok(Paths::resolve(explicit).map_err(|e| e.to_string())?.home)
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn view(b: bookmarks::Bookmark) -> BookmarkView {
    BookmarkView {
        id: b.id,
        url: b.url,
        title: b.title,
        added_ms: b.added_ms,
        updated_ms: b.updated_ms,
        folder_id: b.folder_id,
        order: b.order,
        tags: b.tags,
    }
}

fn views(list: Vec<bookmarks::Bookmark>) -> Vec<BookmarkView> {
    list.into_iter().map(view).collect()
}

fn tree_view(tree: bookmarks::Tree) -> BookmarkTreeView {
    BookmarkTreeView {
        folders: tree
            .folders
            .into_iter()
            .map(|f| BookmarkFolderView {
                id: f.id,
                name: f.name,
                parent_id: f.parent_id,
                order: f.order,
                created_ms: f.created_ms,
                updated_ms: f.updated_ms,
            })
            .collect(),
        bookmarks: views(tree.bookmarks),
    }
}

/// The profile's bookmark tree.
pub fn bookmark_tree(home: Option<String>, profile_id: String) -> Result<BookmarkTreeView, String> {
    let home = resolve_home(home)?;
    bookmarks::tree(&home, &profile_id).map(tree_view).map_err(|e| e.to_string())
}

/// Make a folder at the end of `parent_id` (`None` = top level).
pub fn create_bookmark_folder(home: Option<String>, profile_id: String, name: String, parent_id: Option<String>) -> Result<BookmarkTreeView, String> {
    let home = resolve_home(home)?;
    bookmarks::create_folder(&home, &profile_id, &name, parent_id.as_deref(), now_ms()).map(|(_, t)| tree_view(t)).map_err(|e| e.to_string())
}

/// Rename a folder.
pub fn rename_bookmark_folder(home: Option<String>, profile_id: String, folder_id: String, name: String) -> Result<BookmarkTreeView, String> {
    let home = resolve_home(home)?;
    bookmarks::rename_folder(&home, &profile_id, &folder_id, &name, now_ms()).map(tree_view).map_err(|e| e.to_string())
}

/// Delete a folder and everything in it.
pub fn delete_bookmark_folder(home: Option<String>, profile_id: String, folder_id: String) -> Result<BookmarkTreeView, String> {
    let home = resolve_home(home)?;
    bookmarks::delete_folder(&home, &profile_id, &folder_id, now_ms()).map(tree_view).map_err(|e| e.to_string())
}

/// Move a bookmark or folder into `parent_id` at slot `index`.
pub fn move_bookmark_item(
    home: Option<String>,
    profile_id: String,
    item_id: String,
    parent_id: Option<String>,
    index: u32,
) -> Result<BookmarkTreeView, String> {
    let home = resolve_home(home)?;
    bookmarks::move_item(&home, &profile_id, &item_id, parent_id.as_deref(), index as usize, now_ms()).map(tree_view).map_err(|e| e.to_string())
}

/// Edit a bookmark; `None` leaves a field as it is.
pub fn update_bookmark(
    home: Option<String>,
    profile_id: String,
    bookmark_id: String,
    title: Option<String>,
    url: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<BookmarkTreeView, String> {
    let home = resolve_home(home)?;
    bookmarks::update_bookmark(&home, &profile_id, &bookmark_id, bookmarks::BookmarkEdit { title, url, tags }, now_ms())
        .map(tree_view)
        .map_err(|e| e.to_string())
}

/// Delete one bookmark by id.
pub fn delete_bookmark(home: Option<String>, profile_id: String, bookmark_id: String) -> Result<BookmarkTreeView, String> {
    let home = resolve_home(home)?;
    bookmarks::delete_bookmark(&home, &profile_id, &bookmark_id, now_ms()).map(tree_view).map_err(|e| e.to_string())
}

/// Bookmarks matching every word of `query` in title, URL or tags, and `tag`.
pub fn search_bookmarks(home: Option<String>, profile_id: String, query: String, tag: Option<String>) -> Result<Vec<BookmarkView>, String> {
    let home = resolve_home(home)?;
    bookmarks::search(&home, &profile_id, &query, tag.as_deref()).map(views).map_err(|e| e.to_string())
}

/// Every tag in use, by name.
pub fn bookmark_tags(home: Option<String>, profile_id: String) -> Result<Vec<BookmarkTagView>, String> {
    let home = resolve_home(home)?;
    bookmarks::tags(&home, &profile_id)
        .map(|list| list.into_iter().map(|t| BookmarkTagView { name: t.name, count: t.count as u32 }).collect())
        .map_err(|e| e.to_string())
}

/// Bookmarks for `profile_id`, newest first.
pub fn list_bookmarks(home: Option<String>, profile_id: String) -> Result<Vec<BookmarkView>, String> {
    let home = resolve_home(home)?;
    library::bookmarks(&home, &profile_id).map(views).map_err(|e| e.to_string())
}

/// Save `url` in `profile_id`'s bookmarks (or retitle it). Returns the list.
pub fn add_bookmark(home: Option<String>, profile_id: String, url: String, title: String) -> Result<Vec<BookmarkView>, String> {
    let home = resolve_home(home)?;
    library::add_bookmark(&home, &profile_id, &url, &title, now_ms()).map(views).map_err(|e| e.to_string())
}

/// Remove `url` from `profile_id`'s bookmarks. Returns the list.
pub fn remove_bookmark(home: Option<String>, profile_id: String, url: String) -> Result<Vec<BookmarkView>, String> {
    let home = resolve_home(home)?;
    library::remove_bookmark(&home, &profile_id, &url, now_ms()).map(views).map_err(|e| e.to_string())
}

/// Record a visit to `url` in `profile_id`'s history.
pub fn record_visit(home: Option<String>, profile_id: String, url: String, title: String) -> Result<(), String> {
    let home = resolve_home(home)?;
    library::record_visit(&home, &profile_id, &url, &title, now_ms()).map_err(|e| e.to_string())
}

/// History for `profile_id`, newest first, filtered by `query`. `limit` 0 is unlimited.
pub fn list_history(home: Option<String>, profile_id: String, query: String, limit: u32) -> Result<Vec<HistoryEntryView>, String> {
    let home = resolve_home(home)?;
    library::history(&home, &profile_id, &query, limit as usize)
        .map(|list| list.into_iter().map(|e| HistoryEntryView { url: e.url, title: e.title, visited_ms: e.visited_ms }).collect())
        .map_err(|e| e.to_string())
}

/// Remove every visit to `url` from `profile_id`'s history.
pub fn delete_history_url(home: Option<String>, profile_id: String, url: String) -> Result<(), String> {
    let home = resolve_home(home)?;
    library::delete_history_url(&home, &profile_id, &url).map_err(|e| e.to_string())
}

/// Forget all of `profile_id`'s history.
pub fn clear_history(home: Option<String>, profile_id: String) -> Result<(), String> {
    let home = resolve_home(home)?;
    library::clear_history(&home, &profile_id).map_err(|e| e.to_string())
}

/// Where a profile's bookmarks and history are kept right now.
pub enum LibraryStateView {
    /// Plaintext files, before the profile was first unlocked.
    Plain,
    /// Encrypted, waiting for the profile password.
    Locked,
    /// Encrypted and open.
    Unlocked,
}

/// Whether `profile_id`'s library is plaintext, locked or unlocked.
pub fn library_state(home: Option<String>, profile_id: String) -> Result<LibraryStateView, String> {
    let home = resolve_home(home)?;
    Ok(match library_db::state(&home, &profile_id).map_err(|e| e.to_string())? {
        library_db::LibraryState::Plain => LibraryStateView::Plain,
        library_db::LibraryState::Locked => LibraryStateView::Locked,
        library_db::LibraryState::Unlocked => LibraryStateView::Unlocked,
    })
}

/// What opening a profile's library at launch found.
pub struct LibraryOpenView {
    /// The library is open, or there is nothing to unlock (no identity yet).
    pub ready: bool,
    /// The system keystore had no usable key: ask for the profile password.
    pub needs_password: bool,
    /// Why the keystore could not help, for the prompt. Never secret.
    pub reason: Option<String>,
}

impl LibraryOpenView {
    fn ready() -> Self {
        Self { ready: true, needs_password: false, reason: None }
    }

    fn needs_password(reason: Option<String>) -> Self {
        Self { ready: false, needs_password: true, reason }
    }
}

/// At launch or on a profile switch: open `profile_id`'s encrypted library
/// with the key kept in the system keystore, without a prompt. If there is no
/// usable key there, say so, and the shell asks for the password
/// ([`unlock_library`]).
pub fn open_library(home: Option<String>, profile_id: String) -> Result<LibraryOpenView, String> {
    let dir = crate::api::identity::profile_dir(home.clone(), &profile_id)?;
    let home = resolve_home(home)?;
    let Some(identity) = profile_identity::load(&dir).map_err(|e| e.to_string())? else {
        return Ok(LibraryOpenView::ready());
    };
    if library_db::state(&home, &profile_id).map_err(|e| e.to_string())? == library_db::LibraryState::Unlocked {
        return Ok(LibraryOpenView::ready());
    }
    match morph_keystore::load(&dir, &identity.user_id) {
        Ok(Some(key)) => match library_db::unlock(&home, &profile_id, &key) {
            Ok(()) => Ok(LibraryOpenView::ready()),
            Err(ProfileError::Store(e)) if e.is_wrong_key() => {
                let _ = morph_keystore::forget(&dir);
                Ok(LibraryOpenView::needs_password(Some("the saved key no longer opens this library".into())))
            }
            Err(e) => Err(e.to_string()),
        },
        Ok(None) => Ok(LibraryOpenView::needs_password(None)),
        Err(e) => Ok(LibraryOpenView::needs_password(Some(e.to_string()))),
    }
}

/// The outcome of unlocking with the password.
pub struct LibraryUnlockView {
    /// The key is now in the system keystore, so the next launch needs no password.
    pub remembered: bool,
    /// Why it could not be kept, if it was not. Never secret.
    pub reason: Option<String>,
}

/// Unlock `profile_id`'s encrypted library with the profile password (its key
/// comes from the profile's Weirdnet identity and never leaves Rust), then keep
/// the key in the system keystore. The first unlock moves the plaintext
/// library into the encrypted database.
pub fn unlock_library(home: Option<String>, profile_id: String, password: String) -> Result<LibraryUnlockView, String> {
    let dir = crate::api::identity::profile_dir(home.clone(), &profile_id)?;
    let identity = profile_identity::load(&dir).map_err(|e| e.to_string())?.ok_or("this profile has no identity yet")?;
    let password = profile_identity::SecretString::from(password);
    let key = profile_identity::library_key(&dir, &password).map_err(|e| e.to_string())?;
    library_db::unlock(resolve_home(home)?, &profile_id, &key).map_err(|e| e.to_string())?;
    Ok(match morph_keystore::save(&dir, &identity.user_id, &key) {
        Ok(()) => LibraryUnlockView { remembered: true, reason: None },
        Err(e) => LibraryUnlockView { remembered: false, reason: Some(e.to_string()) },
    })
}

/// A profile just got an identity (created, imported or linked) and its key is
/// in hand: encrypt the library now and keep the key in the keystore. Neither
/// failing undoes the identity; the next launch asks for the password instead.
pub(crate) fn adopt_new_identity(home: &Path, profile_id: &str, dir: &Path, user_id: &str, key: &profile_identity::LibraryKey) {
    if let Err(e) = library_db::unlock_new_identity(home, profile_id, key, now_ms()) {
        eprintln!("Morph: could not open the library of profile {profile_id}: {e}");
        return;
    }
    if let Err(e) = morph_keystore::save(dir, user_id, key) {
        eprintln!("Morph: library key not kept for profile {profile_id}: {e}");
    }
}

/// Close `profile_id`'s library and forget its key until the next unlock.
pub fn lock_library(home: Option<String>, profile_id: String) -> Result<(), String> {
    library_db::lock(resolve_home(home)?, &profile_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_and_history_round_trip_per_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        crate::api::profiles::create_profile(Some(home.clone()), "Work".into()).unwrap();
        let list = add_bookmark(Some(home.clone()), "p1".into(), "https://w.test".into(), "W".into()).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list_bookmarks(Some(home.clone()), "default".into()).unwrap().is_empty());
        record_visit(Some(home.clone()), "p1".into(), "https://w.test".into(), "W".into()).unwrap();
        let hist = list_history(Some(home.clone()), "p1".into(), String::new(), 0).unwrap();
        assert_eq!(hist[0].url, "https://w.test");
        assert!(hist[0].visited_ms > 0);
        delete_history_url(Some(home.clone()), "p1".into(), "https://w.test".into()).unwrap();
        assert!(list_history(Some(home.clone()), "p1".into(), String::new(), 0).unwrap().is_empty());
        record_visit(Some(home.clone()), "p1".into(), "https://w.test".into(), "W".into()).unwrap();
        clear_history(Some(home.clone()), "p1".into()).unwrap();
        assert!(list_history(Some(home.clone()), "p1".into(), String::new(), 0).unwrap().is_empty());
        assert!(remove_bookmark(Some(home.clone()), "p1".into(), "https://w.test".into()).unwrap().is_empty());
        assert!(record_visit(Some(home), "missing".into(), "https://x.test".into(), String::new()).is_err());
    }

    #[test]
    fn folders_tags_and_search_cross_the_bridge() {
        let tmp = tempfile::tempdir().unwrap();
        let home = Some(tmp.path().join("h").display().to_string());
        add_bookmark(home.clone(), "default".into(), "https://r.test".into(), "Rust".into()).unwrap();
        let tree = create_bookmark_folder(home.clone(), "default".into(), "Lang".into(), None).unwrap();
        let folder = tree.folders[0].id.clone();
        let id = tree.bookmarks[0].id.clone();
        let tree = move_bookmark_item(home.clone(), "default".into(), id.clone(), Some(folder.clone()), 0).unwrap();
        assert_eq!(tree.bookmarks[0].folder_id.as_deref(), Some(folder.as_str()));
        update_bookmark(home.clone(), "default".into(), id.clone(), None, None, Some(vec!["Systems".into()])).unwrap();
        assert_eq!(bookmark_tags(home.clone(), "default".into()).unwrap()[0].name, "systems");
        assert_eq!(search_bookmarks(home.clone(), "default".into(), "rust".into(), Some("systems".into())).unwrap().len(), 1);
        rename_bookmark_folder(home.clone(), "default".into(), folder.clone(), "Code".into()).unwrap();
        assert_eq!(bookmark_tree(home.clone(), "default".into()).unwrap().folders[0].name, "Code");
        assert!(delete_bookmark_folder(home.clone(), "default".into(), folder).unwrap().bookmarks.is_empty());
        add_bookmark(home.clone(), "default".into(), "https://x.test".into(), "X".into()).unwrap();
        let x = bookmark_tree(home.clone(), "default".into()).unwrap().bookmarks[0].id.clone();
        assert!(delete_bookmark(home, "default".into(), x).unwrap().bookmarks.is_empty());
    }

    fn profile_dir(home: &str, id: &str) -> PathBuf {
        morph_core::profiles::profile_dir(home, id)
    }

    fn user_id(home: &str, id: &str) -> String {
        profile_identity::load(&profile_dir(home, id)).unwrap().unwrap().user_id
    }

    #[test]
    fn a_new_identity_encrypts_the_library_and_keeps_its_key() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let h = || Some(home.clone());
        crate::api::profiles::create_profile(h(), "Work".into()).unwrap();
        add_bookmark(h(), "p1".into(), "https://w.test".into(), "W".into()).unwrap();
        let open = open_library(h(), "p1".into()).unwrap();
        assert!(open.ready && !open.needs_password, "no identity: nothing to unlock");
        assert!(matches!(library_state(h(), "p1".into()).unwrap(), LibraryStateView::Plain));

        crate::api::identity::create_profile_identity(h(), "p1".into(), "long enough".into()).unwrap();
        assert!(matches!(library_state(h(), "p1".into()).unwrap(), LibraryStateView::Unlocked));
        assert_eq!(list_bookmarks(h(), "p1".into()).unwrap().len(), 1, "moved in");
        assert!(morph_keystore::load(&profile_dir(&home, "p1"), &user_id(&home, "p1")).unwrap().is_some());

        // Next launch: the keystore opens it with no password.
        lock_library(h(), "p1".into()).unwrap();
        let open = open_library(h(), "p1".into()).unwrap();
        assert!(open.ready && !open.needs_password);
        assert_eq!(list_bookmarks(h(), "p1".into()).unwrap().len(), 1);
    }

    #[test]
    fn without_a_saved_key_the_password_unlocks_and_the_key_is_kept_again() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let h = || Some(home.clone());
        crate::api::profiles::create_profile(h(), "Work".into()).unwrap();
        crate::api::identity::create_profile_identity(h(), "p1".into(), "long enough".into()).unwrap();
        add_bookmark(h(), "p1".into(), "https://w.test".into(), "W".into()).unwrap();
        lock_library(h(), "p1".into()).unwrap();
        morph_keystore::forget(&profile_dir(&home, "p1")).unwrap();

        let open = open_library(h(), "p1".into()).unwrap();
        assert!(!open.ready && open.needs_password);
        let err = list_bookmarks(h(), "p1".into()).err().expect("a locked library refuses");
        assert!(err.contains("locked"), "{err}");
        assert!(unlock_library(h(), "p1".into(), "wrong password".into()).is_err());
        assert!(matches!(library_state(h(), "p1".into()).unwrap(), LibraryStateView::Locked));

        let unlocked = unlock_library(h(), "p1".into(), "long enough".into()).unwrap();
        assert!(unlocked.remembered && unlocked.reason.is_none());
        assert_eq!(list_bookmarks(h(), "p1".into()).unwrap().len(), 1);
        lock_library(h(), "p1".into()).unwrap();
        assert!(open_library(h(), "p1".into()).unwrap().ready, "kept again");
    }

    #[test]
    fn an_onboarded_profile_from_before_encryption_moves_in_on_first_unlock() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let h = || Some(home.clone());
        crate::api::profiles::create_profile(h(), "Work".into()).unwrap();
        // An identity made by an older Morph: no library.db, no keystore entry.
        profile_identity::create(&profile_dir(&home, "p1"), &profile_identity::SecretString::from("long enough".to_string()), 1).unwrap();
        add_bookmark(h(), "p1".into(), "https://w.test".into(), "W".into()).unwrap();
        assert!(open_library(h(), "p1".into()).unwrap().needs_password);
        unlock_library(h(), "p1".into(), "long enough".into()).unwrap();
        assert!(matches!(library_state(h(), "p1".into()).unwrap(), LibraryStateView::Unlocked));
        assert_eq!(list_bookmarks(h(), "p1".into()).unwrap().len(), 1);
    }

    #[test]
    fn deleting_a_profile_removes_its_saved_key_only() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let h = || Some(home.clone());
        crate::api::profiles::create_profile(h(), "Work".into()).unwrap();
        crate::api::identity::create_profile_identity(h(), "p1".into(), "long enough".into()).unwrap();
        crate::api::identity::create_profile_identity(h(), "default".into(), "other enough".into()).unwrap();
        let (work, default) = (profile_dir(&home, "p1"), profile_dir(&home, "default"));
        let (work_user, default_user) = (user_id(&home, "p1"), user_id(&home, "default"));
        assert_ne!(morph_keystore::account(&work), morph_keystore::account(&default));
        crate::api::profiles::delete_profile(h(), "p1".into()).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        assert!(morph_keystore::load(&work, &work_user).unwrap().is_none());
        assert!(morph_keystore::load(&default, &default_user).unwrap().is_some());
    }
}
