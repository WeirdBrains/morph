//! Per-profile bookmarks and browsing history.
//!
//! ```text
//! <home>/profiles/<id>/bookmarks.json
//! <home>/profiles/<id>/history.json
//! ```
//!
//! Both live in the profile directory, so deleting a profile deletes them and
//! no profile can read another's. Times are Unix milliseconds supplied by the
//! caller, which keeps this module deterministic under test.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profiles::{self, ProfileError};

/// Most history entries kept per profile; the oldest are dropped first.
pub const MAX_HISTORY: usize = 5000;

/// A saved page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    pub added_ms: i64,
}

/// One visit to a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visited_ms: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BookmarkFile {
    #[serde(default)]
    bookmarks: Vec<Bookmark>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile {
    #[serde(default)]
    entries: Vec<HistoryEntry>,
}

/// `<home>/profiles/<id>/bookmarks.json`.
pub fn bookmarks_path(home: impl AsRef<Path>, id: &str) -> PathBuf {
    profiles::profile_dir(home, id).join("bookmarks.json")
}

/// `<home>/profiles/<id>/history.json`.
pub fn history_path(home: impl AsRef<Path>, id: &str) -> PathBuf {
    profiles::profile_dir(home, id).join("history.json")
}

/// Pages worth remembering: real navigations, not blank or internal pages.
pub fn is_recordable(url: &str) -> bool {
    let url = url.trim();
    !(url.is_empty() || url == "about:blank" || url.starts_with("data:") || url.starts_with("javascript:"))
}

/// Bookmarks, newest first.
pub fn bookmarks(home: impl AsRef<Path>, id: &str) -> Result<Vec<Bookmark>, ProfileError> {
    profiles::require(&home, id)?;
    let mut list = read::<BookmarkFile>(&bookmarks_path(home, id))?.bookmarks;
    list.sort_by_key(|b| std::cmp::Reverse(b.added_ms));
    Ok(list)
}

/// Whether `url` is bookmarked in the profile.
pub fn is_bookmarked(home: impl AsRef<Path>, id: &str, url: &str) -> Result<bool, ProfileError> {
    Ok(bookmarks(home, id)?.iter().any(|b| same_page(&b.url, url)))
}

/// Add `url`, or update its title if it is already saved. Returns the list.
pub fn add_bookmark(home: impl AsRef<Path>, id: &str, url: &str, title: &str, now_ms: i64) -> Result<Vec<Bookmark>, ProfileError> {
    let home = home.as_ref();
    profiles::require(home, id)?;
    let path = bookmarks_path(home, id);
    let mut file = read::<BookmarkFile>(&path)?;
    if !is_recordable(url) {
        return bookmarks(home, id);
    }
    match file.bookmarks.iter_mut().find(|b| same_page(&b.url, url)) {
        Some(existing) => {
            if !title.trim().is_empty() {
                existing.title = title.trim().to_string();
            }
        }
        None => file.bookmarks.push(Bookmark { url: url.trim().to_string(), title: title.trim().to_string(), added_ms: now_ms }),
    }
    write(&path, &file)?;
    bookmarks(home, id)
}

/// Remove `url` from the profile's bookmarks. Returns the list.
pub fn remove_bookmark(home: impl AsRef<Path>, id: &str, url: &str) -> Result<Vec<Bookmark>, ProfileError> {
    let home = home.as_ref();
    profiles::require(home, id)?;
    let path = bookmarks_path(home, id);
    let mut file = read::<BookmarkFile>(&path)?;
    let before = file.bookmarks.len();
    file.bookmarks.retain(|b| !same_page(&b.url, url));
    if file.bookmarks.len() != before {
        write(&path, &file)?;
    }
    bookmarks(home, id)
}

/// Record a visit. A repeat of the newest entry only refreshes its title and
/// time, so reloads and title updates do not flood the list.
pub fn record_visit(home: impl AsRef<Path>, id: &str, url: &str, title: &str, now_ms: i64) -> Result<(), ProfileError> {
    record_visit_capped(home.as_ref(), id, url, title, now_ms, MAX_HISTORY)
}

fn record_visit_capped(home: &Path, id: &str, url: &str, title: &str, now_ms: i64, cap: usize) -> Result<(), ProfileError> {
    if !is_recordable(url) {
        return Ok(());
    }
    profiles::require(home, id)?;
    let path = history_path(home, id);
    let mut file = read::<HistoryFile>(&path)?;
    let title = title.trim();
    match file.entries.last_mut() {
        Some(last) if same_page(&last.url, url) => {
            if !title.is_empty() {
                last.title = title.to_string();
            }
            last.visited_ms = now_ms;
        }
        _ => file.entries.push(HistoryEntry { url: url.trim().to_string(), title: title.to_string(), visited_ms: now_ms }),
    }
    if file.entries.len() > cap {
        let extra = file.entries.len() - cap;
        file.entries.drain(..extra);
    }
    write(&path, &file)
}

/// History, newest first. A non-empty `query` keeps entries whose URL or title
/// contains it (case-insensitive). `limit` 0 means no limit.
pub fn history(home: impl AsRef<Path>, id: &str, query: &str, limit: usize) -> Result<Vec<HistoryEntry>, ProfileError> {
    profiles::require(&home, id)?;
    let needle = query.trim().to_lowercase();
    let mut list: Vec<HistoryEntry> = read::<HistoryFile>(&history_path(home, id))?
        .entries
        .into_iter()
        .rev()
        .filter(|e| needle.is_empty() || e.url.to_lowercase().contains(&needle) || e.title.to_lowercase().contains(&needle))
        .collect();
    if limit > 0 {
        list.truncate(limit);
    }
    Ok(list)
}

/// Remove every visit to `url`.
pub fn delete_history_url(home: impl AsRef<Path>, id: &str, url: &str) -> Result<(), ProfileError> {
    profiles::require(&home, id)?;
    let path = history_path(home, id);
    let mut file = read::<HistoryFile>(&path)?;
    let before = file.entries.len();
    file.entries.retain(|e| !same_page(&e.url, url));
    if file.entries.len() != before {
        write(&path, &file)?;
    }
    Ok(())
}

/// Forget the profile's whole history.
pub fn clear_history(home: impl AsRef<Path>, id: &str) -> Result<(), ProfileError> {
    profiles::require(&home, id)?;
    let path = history_path(home, id);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// URLs that differ only by a trailing slash on the path are the same page.
fn same_page(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

fn read<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ProfileError> {
    match fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => Ok(T::default()),
        Ok(raw) => Ok(serde_json::from_str(&raw)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}

fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), ProfileError> {
    let dir = path.parent().expect("profile file has a parent");
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(value)?;
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&body)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_with(names: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        for name in names {
            profiles::create(&home, name).unwrap();
        }
        (tmp, home)
    }

    #[test]
    fn bookmarks_add_update_remove_newest_first() {
        let (_t, home) = home_with(&[]);
        assert!(bookmarks(&home, "default").unwrap().is_empty());
        add_bookmark(&home, "default", "https://a.test/", "A", 1).unwrap();
        let list = add_bookmark(&home, "default", "https://b.test", "B", 2).unwrap();
        assert_eq!(list.iter().map(|b| b.url.as_str()).collect::<Vec<_>>(), ["https://b.test", "https://a.test/"]);
        let list = add_bookmark(&home, "default", "https://a.test", "A2", 3).unwrap();
        assert_eq!(list.len(), 2, "same page with or without slash");
        assert_eq!(list[1].title, "A2");
        assert!(is_bookmarked(&home, "default", "https://a.test").unwrap());
        let list = remove_bookmark(&home, "default", "https://a.test/").unwrap();
        assert_eq!(list.len(), 1);
        assert!(!is_bookmarked(&home, "default", "https://a.test").unwrap());
        assert!(add_bookmark(&home, "default", "about:blank", "", 4).unwrap().len() == 1);
    }

    #[test]
    fn history_records_dedupes_filters_and_clears() {
        let (_t, home) = home_with(&[]);
        record_visit(&home, "default", "https://a.test/", "", 1).unwrap();
        record_visit(&home, "default", "https://a.test/", "Alpha", 2).unwrap();
        record_visit(&home, "default", "about:blank", "", 3).unwrap();
        record_visit(&home, "default", "https://b.test/docs", "Beta docs", 4).unwrap();
        record_visit(&home, "default", "https://a.test/", "Alpha", 5).unwrap();
        let all = history(&home, "default", "", 0).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].visited_ms, 5);
        assert_eq!(all[2], HistoryEntry { url: "https://a.test/".into(), title: "Alpha".into(), visited_ms: 2 });
        assert_eq!(history(&home, "default", "DOCS", 0).unwrap().len(), 1);
        assert_eq!(history(&home, "default", "", 1).unwrap().len(), 1);
        delete_history_url(&home, "default", "https://a.test").unwrap();
        assert_eq!(history(&home, "default", "", 0).unwrap().len(), 1);
        clear_history(&home, "default").unwrap();
        assert!(history(&home, "default", "", 0).unwrap().is_empty());
    }

    #[test]
    fn history_is_capped() {
        let (_t, home) = home_with(&[]);
        for i in 0..13 {
            record_visit_capped(&home, "default", &format!("https://x.test/{i}"), "", i, 10).unwrap();
        }
        let all = history(&home, "default", "", 0).unwrap();
        assert_eq!(all.len(), 10);
        assert_eq!(all.last().unwrap().url, "https://x.test/3");
    }

    #[test]
    fn profiles_never_share_bookmarks_or_history() {
        let (_t, home) = home_with(&["Work", "Play"]);
        add_bookmark(&home, "p1", "https://work.test", "W", 1).unwrap();
        record_visit(&home, "p1", "https://work.test", "W", 1).unwrap();
        record_visit(&home, "p2", "https://play.test", "P", 1).unwrap();
        assert!(bookmarks(&home, "default").unwrap().is_empty());
        assert!(bookmarks(&home, "p2").unwrap().is_empty());
        assert!(history(&home, "default", "", 0).unwrap().is_empty());
        assert_eq!(history(&home, "p2", "", 0).unwrap()[0].url, "https://play.test");
        let catalog = profiles::delete(&home, "p1").unwrap();
        assert!(catalog.get("p1").is_none());
        assert!(!bookmarks_path(&home, "p1").exists());
        assert!(!history_path(&home, "p1").exists());
    }

    #[test]
    fn unknown_profiles_are_refused() {
        let (_t, home) = home_with(&[]);
        assert!(record_visit(&home, "nope", "https://a.test", "", 1).is_err());
        assert!(add_bookmark(&home, "../x", "https://a.test", "", 1).is_err());
        assert!(history(&home, "../x", "", 0).is_err());
        assert!(bookmarks(&home, "nope").is_err());
    }
}
