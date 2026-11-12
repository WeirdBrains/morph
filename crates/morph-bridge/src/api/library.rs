//! Per-profile bookmarks (folders, tags) and history for the Flutter shell.

use morph_core::{bookmarks, library};
use morph_identity::Paths;
use std::path::PathBuf;
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
}
