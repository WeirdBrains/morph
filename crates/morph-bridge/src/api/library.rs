//! Per-profile bookmarks and history for the Flutter shell.

use morph_core::library;
use morph_identity::Paths;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A saved page.
pub struct BookmarkView {
    pub url: String,
    pub title: String,
    pub added_ms: i64,
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

fn views(list: Vec<library::Bookmark>) -> Vec<BookmarkView> {
    list.into_iter().map(|b| BookmarkView { url: b.url, title: b.title, added_ms: b.added_ms }).collect()
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
    library::remove_bookmark(&home, &profile_id, &url).map(views).map_err(|e| e.to_string())
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
}
