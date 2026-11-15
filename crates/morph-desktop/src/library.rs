//! The profile's library (bookmarks, history): opening it at launch, and the
//! state behind the library panel, kept free of GPUI so it is tested here.

use std::path::{Path, PathBuf};

use morph_core::bookmarks::{self, Bookmark, BookmarkEdit, BookmarkError, TagCount};
use morph_core::library::HistoryEntry;
use morph_core::library_db::LibraryState;
use morph_core::profiles::ProfileError;
use morph_services::SecretString;
use morph_services::library::{self, LibraryOpen};

/// Open `profile`'s library with the key kept in the system keystore, if it
/// is encrypted. Returns whether it is open; when it is not, the library
/// panel asks for the profile password, and history waits until then.
pub fn open_at_launch(home: &Path, profile: &str) -> bool {
    match library::open(home, profile) {
        Ok(LibraryOpen::Ready) => true,
        Ok(LibraryOpen::NeedsPassword { reason }) => {
            let why = reason.map(|r| format!(" ({r})")).unwrap_or_default();
            eprintln!("morph: the library of profile {profile} is locked{why}; open the library (Ctrl+H) to unlock it");
            false
        }
        Err(e) => {
            eprintln!("morph: library of profile {profile}: {e}");
            false
        }
    }
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Which list the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Bookmarks,
    History,
}

/// One row of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Folder { id: String, name: String, folders: usize, bookmarks: usize },
    Bookmark(Bookmark),
    Visit(HistoryEntry),
}

/// History rows shown at most.
const HISTORY_ROWS: usize = 500;

/// The library panel's state for one profile.
pub struct Library {
    home: PathBuf,
    profile: String,
    pub tab: Tab,
    /// Filters either list.
    query: String,
    /// The folder being browsed; `None` is the root.
    folder: Option<String>,
    /// Bookmarks with this tag only.
    tag: Option<String>,
    /// The library needs the profile password.
    locked: bool,
    /// The last failure to show (wrong password, a write that failed).
    pub error: Option<String>,
    /// A note after unlocking (the key could not be remembered).
    pub notice: Option<String>,
}

impl Library {
    pub fn new(home: &Path, profile: &str) -> Self {
        let mut lib = Self {
            home: home.to_path_buf(),
            profile: profile.to_string(),
            tab: Tab::Bookmarks,
            query: String::new(),
            folder: None,
            tag: None,
            locked: false,
            error: None,
            notice: None,
        };
        lib.refresh_lock();
        lib
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    fn refresh_lock(&mut self) {
        self.locked = matches!(library::state(&self.home, &self.profile), Ok(LibraryState::Locked));
    }

    /// Note a failure: a locked library turns into the password prompt.
    fn fail(&mut self, e: impl Into<Failure>) {
        match e.into() {
            Failure::Locked => self.locked = true,
            Failure::Other(message) => self.error = Some(message),
        }
    }

    /// Unlock with the profile password.
    pub fn unlock(&mut self, password: &str) -> bool {
        let secret = SecretString::from(password.to_string());
        match library::unlock(&self.home, &self.profile, &secret) {
            Ok(unlocked) => {
                self.locked = false;
                self.error = None;
                self.notice = (!unlocked.remembered).then(|| {
                    let why = unlocked.reason.map(|r| format!(" ({r})")).unwrap_or_default();
                    format!("Unlocked for this session; the system keystore could not keep the key{why}.")
                });
                true
            }
            Err(_) => {
                // Never echo what came back: it may describe the secret.
                self.error = Some("That password did not unlock the library.".into());
                false
            }
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = query.trim().to_string();
    }

    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    /// Filter by `tag`, or stop filtering when it is the current one.
    pub fn toggle_tag(&mut self, tag: &str) {
        self.tag = if self.tag.as_deref() == Some(tag) { None } else { Some(tag.to_string()) };
    }

    pub fn enter(&mut self, folder: &str) {
        self.folder = Some(folder.to_string());
    }

    /// Up one folder.
    pub fn up(&mut self) {
        let Some(current) = self.folder.take() else {
            return;
        };
        if let Ok(tree) = bookmarks::tree(&self.home, &self.profile) {
            self.folder = tree.folders.iter().find(|f| f.id == current).and_then(|f| f.parent_id.clone());
        }
    }

    /// Names from the root to the current folder.
    pub fn path(&self) -> Vec<String> {
        let Some(mut id) = self.folder.clone() else {
            return Vec::new();
        };
        let Ok(tree) = bookmarks::tree(&self.home, &self.profile) else {
            return Vec::new();
        };
        let mut names = Vec::new();
        while let Some(folder) = tree.folders.iter().find(|f| f.id == id) {
            names.push(folder.name.clone());
            match &folder.parent_id {
                Some(parent) if names.len() < 64 => id = parent.clone(),
                _ => break,
            }
        }
        names.reverse();
        names
    }

    pub fn tags(&mut self) -> Vec<TagCount> {
        if self.locked {
            return Vec::new();
        }
        match bookmarks::tags(&self.home, &self.profile) {
            Ok(tags) => tags,
            Err(e) => {
                self.fail(e);
                Vec::new()
            }
        }
    }

    /// What the panel lists now.
    pub fn rows(&mut self) -> Vec<Row> {
        if self.locked {
            return Vec::new();
        }
        let rows = match self.tab {
            Tab::History => morph_core::library::history(&self.home, &self.profile, &self.query, HISTORY_ROWS)
                .map(|list| list.into_iter().map(Row::Visit).collect())
                .map_err(Failure::from),
            Tab::Bookmarks if !self.query.is_empty() || self.tag.is_some() => {
                bookmarks::search(&self.home, &self.profile, &self.query, self.tag.as_deref())
                    .map(|list| list.into_iter().map(Row::Bookmark).collect())
                    .map_err(Failure::from)
            }
            Tab::Bookmarks => self.folder_rows(),
        };
        match rows {
            Ok(rows) => rows,
            Err(e) => {
                self.fail(e);
                Vec::new()
            }
        }
    }

    fn folder_rows(&self) -> Result<Vec<Row>, Failure> {
        let tree = bookmarks::tree(&self.home, &self.profile)?;
        let here = self.folder.as_deref();
        let mut rows: Vec<Row> = tree
            .folders
            .iter()
            .filter(|f| f.parent_id.as_deref() == here)
            .map(|f| Row::Folder {
                id: f.id.clone(),
                name: f.name.clone(),
                folders: tree.folders.iter().filter(|c| c.parent_id.as_deref() == Some(&f.id)).count(),
                bookmarks: tree.bookmarks.iter().filter(|b| b.folder_id.as_deref() == Some(&f.id)).count(),
            })
            .collect();
        rows.extend(tree.bookmarks.into_iter().filter(|b| b.folder_id.as_deref() == here).map(Row::Bookmark));
        Ok(rows)
    }

    /// Whether `url` is bookmarked; false while locked.
    pub fn is_bookmarked(&self, url: &str) -> bool {
        !self.locked && bookmarks::is_bookmarked(&self.home, &self.profile, url).unwrap_or(false)
    }

    /// Bookmark the page, or remove its bookmark (Ctrl+D, the star).
    /// Returns whether it is bookmarked now.
    pub fn toggle_bookmark(&mut self, url: &str, title: &str) -> bool {
        if self.locked || url.is_empty() {
            return false;
        }
        let result = if self.is_bookmarked(url) {
            bookmarks::remove_bookmark(&self.home, &self.profile, url, now_ms())
        } else {
            bookmarks::add_bookmark(&self.home, &self.profile, url, title, now_ms())
        };
        if let Err(e) = result {
            self.fail(e);
        }
        self.is_bookmarked(url)
    }

    fn apply<T>(&mut self, result: Result<T, impl Into<Failure>>) {
        match result {
            Ok(_) => self.error = None,
            Err(e) => self.fail(e),
        }
    }

    pub fn delete_bookmark(&mut self, id: &str) {
        let r = bookmarks::delete_bookmark(&self.home, &self.profile, id, now_ms());
        self.apply(r);
    }

    /// Set a bookmark's tags from comma- or space-separated text.
    pub fn set_tags(&mut self, id: &str, text: &str) {
        let tags: Vec<&str> = text.split([',', ' ']).filter(|t| !t.trim().is_empty()).collect();
        let edit = BookmarkEdit { tags: Some(bookmarks::normalize_tags(&tags)), ..BookmarkEdit::default() };
        let r = bookmarks::update_bookmark(&self.home, &self.profile, id, edit, now_ms());
        self.apply(r);
    }

    pub fn rename_bookmark(&mut self, id: &str, title: &str) {
        let edit = BookmarkEdit { title: Some(title.trim().to_string()), ..BookmarkEdit::default() };
        let r = bookmarks::update_bookmark(&self.home, &self.profile, id, edit, now_ms());
        self.apply(r);
    }

    /// Move a bookmark or folder into `folder` (`None` is the root), last.
    pub fn move_to(&mut self, item: &str, folder: Option<&str>) {
        let r = bookmarks::move_item(&self.home, &self.profile, item, folder, usize::MAX, now_ms());
        self.apply(r);
    }

    /// Folders a bookmark can move to, as (id, path name), root first.
    pub fn folders(&self) -> Vec<(Option<String>, String)> {
        let mut out = vec![(None, "Bookmarks".to_string())];
        if let Ok(tree) = bookmarks::tree(&self.home, &self.profile) {
            let name = |mut id: String| {
                let mut parts = Vec::new();
                while let Some(f) = tree.folders.iter().find(|f| f.id == id) {
                    parts.push(f.name.clone());
                    match &f.parent_id {
                        Some(p) if parts.len() < 64 => id = p.clone(),
                        _ => break,
                    }
                }
                parts.reverse();
                parts.join(" / ")
            };
            let mut named: Vec<(Option<String>, String)> = tree.folders.iter().map(|f| (Some(f.id.clone()), name(f.id.clone()))).collect();
            named.sort_by_key(|a| a.1.to_lowercase());
            out.extend(named);
        }
        out
    }

    /// A new folder in the current one.
    pub fn new_folder(&mut self, name: &str) {
        let r = bookmarks::create_folder(&self.home, &self.profile, name.trim(), self.folder.as_deref(), now_ms());
        self.apply(r);
    }

    pub fn rename_folder(&mut self, id: &str, name: &str) {
        let r = bookmarks::rename_folder(&self.home, &self.profile, id, name.trim(), now_ms());
        self.apply(r);
    }

    /// Delete a folder and everything in it.
    pub fn delete_folder(&mut self, id: &str) {
        let r = bookmarks::delete_folder(&self.home, &self.profile, id, now_ms());
        self.apply(r);
    }

    pub fn delete_visit(&mut self, url: &str) {
        let r = morph_core::library::delete_history_url(&self.home, &self.profile, url);
        self.apply(r);
    }

    pub fn clear_history(&mut self) {
        let r = morph_core::library::clear_history(&self.home, &self.profile);
        self.apply(r);
    }
}

/// Why a library call failed, as the panel cares.
enum Failure {
    Locked,
    Other(String),
}

impl From<ProfileError> for Failure {
    fn from(e: ProfileError) -> Self {
        match e {
            ProfileError::Locked => Failure::Locked,
            other => Failure::Other(other.to_string()),
        }
    }
}

impl From<BookmarkError> for Failure {
    fn from(e: BookmarkError) -> Self {
        match e {
            BookmarkError::Profile(p) => p.into(),
            other => Failure::Other(other.to_string()),
        }
    }
}

/// When something happened, as a short phrase ("3 min ago", "yesterday").
pub fn ago(then_ms: i64, now_ms: i64) -> String {
    let secs = (now_ms - then_ms).max(0) / 1000;
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        86_400..172_800 => "yesterday".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::create(&home, "Work").unwrap();
        (tmp, home)
    }

    #[test]
    fn history_follows_whether_the_library_opens() {
        let (_tmp, home) = home();
        assert!(open_at_launch(&home, "p1"), "plaintext library");

        morph_services::identity::create(&home, "p1", &SecretString::from("long enough".to_string())).unwrap();
        library::lock(&home, "p1");
        assert!(open_at_launch(&home, "p1"), "the saved key opens it");

        library::lock(&home, "p1");
        morph_keystore::forget(&morph_core::profiles::profile_dir(&home, "p1")).unwrap();
        assert!(!open_at_launch(&home, "p1"), "locked without a key");
        assert!(!open_at_launch(&home, "nope"), "unknown profile");
    }

    #[test]
    fn the_star_bookmarks_and_unbookmarks_a_page() {
        let (_tmp, home) = home();
        let mut lib = Library::new(&home, "p1");
        assert!(!lib.is_bookmarked("https://a.test/"));
        assert!(lib.toggle_bookmark("https://a.test/", "A"));
        assert!(lib.is_bookmarked("https://a.test/"));
        assert!(matches!(&lib.rows()[..], [Row::Bookmark(b)] if b.title == "A"));
        assert!(!lib.toggle_bookmark("https://a.test/", "A"));
        assert!(lib.rows().is_empty());
        assert!(!lib.toggle_bookmark("", ""), "a blank tab has nothing to bookmark");
    }

    #[test]
    fn folders_tags_and_search() {
        let (_tmp, home) = home();
        let mut lib = Library::new(&home, "p1");
        lib.toggle_bookmark("https://rust-lang.org/", "Rust");
        lib.toggle_bookmark("https://zed.dev/", "Zed");
        lib.new_folder("Reading");
        let rows = lib.rows();
        let folder = match &rows[0] {
            Row::Folder { id, name, .. } => {
                assert_eq!(name, "Reading");
                id.clone()
            }
            other => panic!("folders come first: {other:?}"),
        };
        assert_eq!(rows.len(), 3);
        let zed = rows.iter().find_map(|r| match r {
            Row::Bookmark(b) if b.title == "Zed" => Some(b.id.clone()),
            _ => None,
        });
        let zed = zed.unwrap();
        lib.move_to(&zed, Some(&folder));
        assert!(matches!(&lib.rows()[..], [Row::Folder { bookmarks: 1, .. }, Row::Bookmark(b)] if b.title == "Rust"));
        lib.enter(&folder);
        assert_eq!(lib.path(), vec!["Reading".to_string()]);
        assert!(matches!(&lib.rows()[..], [Row::Bookmark(b)] if b.title == "Zed"));
        assert_eq!(lib.folders().len(), 2);
        lib.up();
        assert_eq!(lib.folder(), None);

        lib.set_tags(&zed, "Editor, rust  gpui");
        let tags: Vec<String> = lib.tags().into_iter().map(|t| t.name).collect();
        assert_eq!(tags, ["editor", "gpui", "rust"]);
        lib.toggle_tag("editor");
        assert!(matches!(&lib.rows()[..], [Row::Bookmark(b)] if b.title == "Zed"), "a tag filters across folders");
        lib.toggle_tag("editor");
        lib.set_query("rust");
        assert_eq!(lib.rows().len(), 2, "title, URL or tag");
        lib.set_query("");

        lib.rename_folder(&folder, "Later");
        assert!(matches!(&lib.rows()[0], Row::Folder { name, .. } if name == "Later"));
        lib.rename_bookmark(&zed, "Zed editor");
        lib.delete_folder(&folder);
        assert!(matches!(&lib.rows()[..], [Row::Bookmark(b)] if b.title == "Rust"));
        lib.new_folder("   ");
        assert!(lib.error.is_some(), "an empty name is refused");
    }

    #[test]
    fn history_lists_newest_first_and_can_be_cleared() {
        let (_tmp, home) = home();
        morph_core::library::record_visit(&home, "p1", "https://a.test/", "A", 1_000).unwrap();
        morph_core::library::record_visit(&home, "p1", "https://b.test/", "B", 2_000).unwrap();
        let mut lib = Library::new(&home, "p1");
        lib.tab = Tab::History;
        let urls: Vec<String> = lib
            .rows()
            .into_iter()
            .map(|r| match r {
                Row::Visit(v) => v.url,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(urls, ["https://b.test/", "https://a.test/"]);
        lib.set_query("a.test");
        assert_eq!(lib.rows().len(), 1);
        lib.delete_visit("https://a.test/");
        assert!(lib.rows().is_empty());
        lib.set_query("");
        lib.clear_history();
        assert!(lib.rows().is_empty());
    }

    #[test]
    fn a_locked_library_asks_for_the_password() {
        let (_tmp, home) = home();
        morph_services::identity::create(&home, "p1", &SecretString::from("long enough".to_string())).unwrap();
        library::lock(&home, "p1");
        let mut lib = Library::new(&home, "p1");
        assert!(lib.is_locked());
        assert!(lib.rows().is_empty());
        assert!(!lib.toggle_bookmark("https://a.test/", "A"));
        assert!(!lib.unlock("wrong password"));
        assert_eq!(lib.error.as_deref(), Some("That password did not unlock the library."));
        assert!(lib.is_locked());
        assert!(lib.unlock("long enough"));
        assert!(!lib.is_locked());
        assert!(lib.error.is_none());
        assert!(lib.toggle_bookmark("https://a.test/", "A"));
        // Locked again behind the panel's back: the next read notices.
        library::lock(&home, "p1");
        assert!(lib.rows().is_empty());
        assert!(lib.is_locked());
    }

    #[test]
    fn times_read_as_phrases() {
        assert_eq!(ago(0, 30_000), "just now");
        assert_eq!(ago(0, 5 * 60_000), "5 min ago");
        assert_eq!(ago(0, 3 * 3_600_000), "3 h ago");
        assert_eq!(ago(0, 30 * 3_600_000), "yesterday");
        assert_eq!(ago(0, 72 * 3_600_000), "3 days ago");
        assert_eq!(ago(10, 0), "just now");
    }
}
