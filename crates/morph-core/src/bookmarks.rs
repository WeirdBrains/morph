//! Per-profile bookmarks: nested folders, keyword tags and search.
//!
//! ```text
//! <home>/profiles/<id>/bookmarks.json
//! ```
//!
//! Every bookmark and folder has a stable id, creation and update times and a
//! fractional `order` key among its siblings, and deletes leave tombstones, so
//! a later Weirdnet sync can merge two devices' trees record by record (see
//! docs/identity-recovery.md). Folders and bookmarks share one ordering inside
//! a parent. Files from before folders existed load as a flat root list.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::library::{bookmarks_path, is_recordable, read, same_page, write};
use crate::profiles::{self, ProfileError};

/// Tombstones older than this are forgotten.
pub const TOMBSTONE_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// Longest tag kept, in characters.
pub const MAX_TAG_LEN: usize = 40;

/// A saved page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    #[serde(default)]
    pub id: String,
    pub url: String,
    pub title: String,
    pub added_ms: i64,
    #[serde(default)]
    pub updated_ms: i64,
    /// Containing folder; `None` is the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub order: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// A bookmark folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub order: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}

/// A deleted bookmark or folder, kept so sync can tell delete from absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    pub id: String,
    pub deleted_ms: i64,
}

/// Folders and bookmarks, each sorted by parent then `order`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    pub folders: Vec<Folder>,
    pub bookmarks: Vec<Bookmark>,
}

/// A tag and how many bookmarks carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum BookmarkError {
    #[error(transparent)]
    Profile(#[from] ProfileError),
    #[error("no bookmark or folder {0}")]
    UnknownItem(String),
    #[error("no folder {0}")]
    UnknownFolder(String),
    #[error("a folder cannot move into itself or a folder inside it")]
    Cycle,
    #[error("folder name must not be empty")]
    EmptyName,
    #[error("not a web address: {0}")]
    BadUrl(String),
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    bookmarks: Vec<Bookmark>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    folders: Vec<Folder>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deleted: Vec<Tombstone>,
}

const VERSION: u32 = 2;

// --- Fractional order keys ----------------------------------------------
//
// Keys are base-62 fractions in (0, 1) with no trailing '0', compared as
// strings. `key_between` always finds a key strictly between two others, so
// a move rewrites only the moved record (what makes concurrent reorders on
// two devices merge cleanly).

const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn digit_index(c: u8) -> usize {
    DIGITS.iter().position(|&d| d == c).unwrap_or(0)
}

fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(b'0') == b[n] {
            n += 1;
        }
        if n > 0 {
            let mut out = b[..n].to_vec();
            out.extend(midpoint(a.get(n..).unwrap_or(&[]), Some(&b[n..])));
            return out;
        }
    }
    let da = a.first().map(|&c| digit_index(c)).unwrap_or(0);
    let db = b.map(|b| digit_index(b[0])).unwrap_or(DIGITS.len());
    if db - da > 1 {
        return vec![DIGITS[(da + db).div_ceil(2)]];
    }
    if let Some(b) = b
        && b.len() > 1
    {
        return b[..1].to_vec();
    }
    let mut out = vec![DIGITS[da]];
    out.extend(midpoint(a.get(1..).unwrap_or(&[]), None));
    out
}

/// A key strictly between `a` and `b` (either may be open).
pub fn key_between(a: Option<&str>, b: Option<&str>) -> String {
    let a = a.unwrap_or("");
    debug_assert!(b.is_none_or(|b| a < b), "keys out of order: {a:?} {b:?}");
    String::from_utf8(midpoint(a.as_bytes(), b.map(str::as_bytes))).expect("ascii digits")
}

/// `count` ascending keys, for giving an old flat list an order.
fn spread_keys(count: usize) -> Vec<String> {
    let mut keys = Vec::with_capacity(count);
    let mut last: Option<String> = None;
    for _ in 0..count {
        let next = key_between(last.as_deref(), None);
        keys.push(next.clone());
        last = Some(next);
    }
    keys
}

// --- Ids ------------------------------------------------------------------

/// A new time-ordered id (UUIDv7), unique across devices.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// The id an old, id-less bookmark gets: derived from its URL and time so it
/// is the same on every read until the file is rewritten.
fn legacy_id(url: &str, added_ms: i64) -> String {
    // FNV-1a, 64 bits, twice with different seeds for 128 bits.
    let hash = |seed: u64| {
        let mut h = seed;
        for byte in url.bytes().chain(added_ms.to_le_bytes()) {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    };
    format!("legacy-{:016x}{:016x}", hash(0xcbf2_9ce4_8422_2325), hash(0x8422_2325_cbf2_9ce4))
}

// --- Load and save ----------------------------------------------------------

fn load(home: &Path, id: &str) -> Result<File, BookmarkError> {
    profiles::require(home, id)?;
    let mut file = read::<File>(&bookmarks_path(home, id))?;
    normalize(&mut file);
    Ok(file)
}

fn save(home: &Path, id: &str, file: &mut File, now_ms: i64) -> Result<(), BookmarkError> {
    file.version = VERSION;
    file.deleted.retain(|t| now_ms - t.deleted_ms < TOMBSTONE_MS);
    write(&bookmarks_path(home, id), file)?;
    Ok(())
}

/// Fills in what older files lack: ids, update times, order keys, and moves
/// anything whose folder is gone to the root.
fn normalize(file: &mut File) {
    let folder_ids: HashSet<String> = file.folders.iter().map(|f| f.id.clone()).collect();
    for folder in &mut file.folders {
        if folder.parent_id.as_ref().is_some_and(|p| !folder_ids.contains(p) || *p == folder.id) {
            folder.parent_id = None;
        }
    }
    // Break any parent cycles a bad merge could leave.
    let parents: BTreeMap<String, Option<String>> = file.folders.iter().map(|f| (f.id.clone(), f.parent_id.clone())).collect();
    for folder in &mut file.folders {
        let mut seen = HashSet::new();
        let mut at = folder.parent_id.clone();
        while let Some(p) = at {
            if !seen.insert(p.clone()) || p == folder.id {
                folder.parent_id = None;
                break;
            }
            at = parents.get(&p).cloned().flatten();
        }
    }
    for bookmark in &mut file.bookmarks {
        if bookmark.id.is_empty() {
            bookmark.id = legacy_id(&bookmark.url, bookmark.added_ms);
        }
        if bookmark.updated_ms == 0 {
            bookmark.updated_ms = bookmark.added_ms;
        }
        if bookmark.folder_id.as_ref().is_some_and(|f| !folder_ids.contains(f)) {
            bookmark.folder_id = None;
        }
    }
    // Old files had no order: keep their newest-first look.
    let mut unordered: Vec<usize> = (0..file.bookmarks.len()).filter(|&i| file.bookmarks[i].order.is_empty()).collect();
    if !unordered.is_empty() {
        unordered.sort_by_key(|&i| std::cmp::Reverse(file.bookmarks[i].added_ms));
        let first = file
            .bookmarks
            .iter()
            .filter(|b| !b.order.is_empty() && b.folder_id.is_none())
            .map(|b| b.order.clone())
            .chain(file.folders.iter().filter(|f| f.parent_id.is_none()).map(|f| f.order.clone()))
            .min();
        let keys = match first {
            None => spread_keys(unordered.len()),
            Some(first) => {
                let mut keys = Vec::new();
                let mut last: Option<String> = None;
                for _ in 0..unordered.len() {
                    let k = key_between(last.as_deref(), Some(&first));
                    last = Some(k.clone());
                    keys.push(k);
                }
                keys
            }
        };
        for (i, key) in unordered.into_iter().zip(keys) {
            file.bookmarks[i].order = key;
            file.bookmarks[i].folder_id = None;
        }
    }
}

fn sorted(file: &File) -> Tree {
    let mut folders = file.folders.clone();
    folders.sort_by(|a, b| (a.parent_id.as_deref(), &a.order, &a.id).cmp(&(b.parent_id.as_deref(), &b.order, &b.id)));
    let mut bookmarks = file.bookmarks.clone();
    bookmarks.sort_by(|a, b| (a.folder_id.as_deref(), &a.order, &a.id).cmp(&(b.folder_id.as_deref(), &b.order, &b.id)));
    Tree { folders, bookmarks }
}

/// Order keys of everything directly in `parent`, ascending, skipping `except`.
fn sibling_keys(file: &File, parent: Option<&str>, except: &str) -> Vec<String> {
    let mut keys: Vec<String> = file
        .folders
        .iter()
        .filter(|f| f.parent_id.as_deref() == parent && f.id != except)
        .map(|f| f.order.clone())
        .chain(file.bookmarks.iter().filter(|b| b.folder_id.as_deref() == parent && b.id != except).map(|b| b.order.clone()))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// The key for slot `index` among `keys` (0 = first, past the end = last).
fn key_at(keys: &[String], index: usize) -> String {
    let index = index.min(keys.len());
    let before = index.checked_sub(1).map(|i| keys[i].as_str());
    let after = keys.get(index).map(String::as_str);
    key_between(before, after)
}

fn require_folder(file: &File, folder: Option<&str>) -> Result<(), BookmarkError> {
    match folder {
        Some(f) if !file.folders.iter().any(|x| x.id == f) => Err(BookmarkError::UnknownFolder(f.to_string())),
        _ => Ok(()),
    }
}

/// Clean tags: trimmed, lower case, inner whitespace as single spaces,
/// capped, no duplicates, original order kept.
pub fn normalize_tags<S: AsRef<str>>(tags: &[S]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tag in tags {
        let clean: String = tag.as_ref().split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        let clean: String = clean.trim_start_matches('#').chars().take(MAX_TAG_LEN).collect();
        let clean = clean.trim().to_string();
        if !clean.is_empty() && seen.insert(clean.clone()) {
            out.push(clean);
        }
    }
    out
}

// --- Reading ----------------------------------------------------------------

/// The whole tree.
pub fn tree(home: impl AsRef<Path>, id: &str) -> Result<Tree, BookmarkError> {
    Ok(sorted(&load(home.as_ref(), id)?))
}

/// Every bookmark in any folder, newest first.
pub fn bookmarks(home: impl AsRef<Path>, id: &str) -> Result<Vec<Bookmark>, BookmarkError> {
    let mut list = load(home.as_ref(), id)?.bookmarks;
    list.sort_by_key(|b| std::cmp::Reverse(b.added_ms));
    Ok(list)
}

/// Whether `url` is bookmarked anywhere in the profile.
pub fn is_bookmarked(home: impl AsRef<Path>, id: &str, url: &str) -> Result<bool, BookmarkError> {
    Ok(load(home.as_ref(), id)?.bookmarks.iter().any(|b| same_page(&b.url, url)))
}

/// Bookmarks whose title, URL or a tag contains every word of `query`
/// (case-insensitive), and that carry `tag` when one is given. Tree order.
pub fn search(home: impl AsRef<Path>, id: &str, query: &str, tag: Option<&str>) -> Result<Vec<Bookmark>, BookmarkError> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let tag = tag.map(|t| normalize_tags(&[t])).and_then(|t| t.into_iter().next());
    let tree = sorted(&load(home.as_ref(), id)?);
    Ok(tree
        .bookmarks
        .into_iter()
        .filter(|b| tag.as_ref().is_none_or(|t| b.tags.contains(t)))
        .filter(|b| {
            let haystack = format!("{}\n{}\n{}", b.title.to_lowercase(), b.url.to_lowercase(), b.tags.join("\n"));
            words.iter().all(|w| haystack.contains(w.as_str()))
        })
        .collect())
}

/// Every tag in use, by name.
pub fn tags(home: impl AsRef<Path>, id: &str) -> Result<Vec<TagCount>, BookmarkError> {
    let mut counts = BTreeMap::<String, usize>::new();
    for b in load(home.as_ref(), id)?.bookmarks {
        for t in b.tags {
            *counts.entry(t).or_default() += 1;
        }
    }
    Ok(counts.into_iter().map(|(name, count)| TagCount { name, count }).collect())
}

// --- Changing ---------------------------------------------------------------

/// Save `url` at the top of the root, or retitle it if it is already saved
/// (wherever it is). Returns every bookmark, newest first.
pub fn add_bookmark(home: impl AsRef<Path>, id: &str, url: &str, title: &str, now_ms: i64) -> Result<Vec<Bookmark>, BookmarkError> {
    let home = home.as_ref();
    let mut file = load(home, id)?;
    if !is_recordable(url) {
        return bookmarks(home, id);
    }
    match file.bookmarks.iter_mut().find(|b| same_page(&b.url, url)) {
        Some(existing) => {
            if !title.trim().is_empty() && existing.title != title.trim() {
                existing.title = title.trim().to_string();
                existing.updated_ms = now_ms;
            }
        }
        None => {
            let order = key_at(&sibling_keys(&file, None, ""), 0);
            file.bookmarks.push(Bookmark {
                id: new_id(),
                url: url.trim().to_string(),
                title: title.trim().to_string(),
                added_ms: now_ms,
                updated_ms: now_ms,
                folder_id: None,
                order,
                tags: Vec::new(),
            });
        }
    }
    save(home, id, &mut file, now_ms)?;
    bookmarks(home, id)
}

/// Remove every bookmark of `url`. Returns every bookmark, newest first.
pub fn remove_bookmark(home: impl AsRef<Path>, id: &str, url: &str, now_ms: i64) -> Result<Vec<Bookmark>, BookmarkError> {
    let home = home.as_ref();
    let mut file = load(home, id)?;
    let gone: Vec<String> = file.bookmarks.iter().filter(|b| same_page(&b.url, url)).map(|b| b.id.clone()).collect();
    if !gone.is_empty() {
        file.bookmarks.retain(|b| !gone.contains(&b.id));
        file.deleted.extend(gone.into_iter().map(|id| Tombstone { id, deleted_ms: now_ms }));
        save(home, id, &mut file, now_ms)?;
    }
    bookmarks(home, id)
}

/// What [`update_bookmark`] changes; `None` leaves a field alone.
#[derive(Debug, Default, Clone)]
pub struct BookmarkEdit {
    pub title: Option<String>,
    pub url: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Edit a bookmark's title, URL or tags.
pub fn update_bookmark(home: impl AsRef<Path>, id: &str, item: &str, edit: BookmarkEdit, now_ms: i64) -> Result<Tree, BookmarkError> {
    let home = home.as_ref();
    let mut file = load(home, id)?;
    let b = file.bookmarks.iter_mut().find(|b| b.id == item).ok_or_else(|| BookmarkError::UnknownItem(item.to_string()))?;
    let mut changed = false;
    if let Some(title) = edit.title {
        let title = title.trim().to_string();
        changed |= b.title != title;
        b.title = title;
    }
    if let Some(url) = edit.url {
        let url = url.trim().to_string();
        if !is_recordable(&url) {
            return Err(BookmarkError::BadUrl(url));
        }
        changed |= b.url != url;
        b.url = url;
    }
    if let Some(tags) = edit.tags {
        let tags = normalize_tags(&tags);
        changed |= b.tags != tags;
        b.tags = tags;
    }
    if changed {
        b.updated_ms = now_ms;
        save(home, id, &mut file, now_ms)?;
    }
    Ok(sorted(&file))
}

/// Delete one bookmark by id.
pub fn delete_bookmark(home: impl AsRef<Path>, id: &str, item: &str, now_ms: i64) -> Result<Tree, BookmarkError> {
    let home = home.as_ref();
    let mut file = load(home, id)?;
    let before = file.bookmarks.len();
    file.bookmarks.retain(|b| b.id != item);
    if file.bookmarks.len() == before {
        return Err(BookmarkError::UnknownItem(item.to_string()));
    }
    file.deleted.push(Tombstone { id: item.to_string(), deleted_ms: now_ms });
    save(home, id, &mut file, now_ms)?;
    Ok(sorted(&file))
}

/// Make a folder at the end of `parent` (`None` = root). Returns its id.
pub fn create_folder(home: impl AsRef<Path>, id: &str, name: &str, parent: Option<&str>, now_ms: i64) -> Result<(String, Tree), BookmarkError> {
    let home = home.as_ref();
    let name = name.trim();
    if name.is_empty() {
        return Err(BookmarkError::EmptyName);
    }
    let mut file = load(home, id)?;
    require_folder(&file, parent)?;
    let keys = sibling_keys(&file, parent, "");
    let folder = Folder {
        id: new_id(),
        name: name.to_string(),
        parent_id: parent.map(str::to_string),
        order: key_at(&keys, keys.len()),
        created_ms: now_ms,
        updated_ms: now_ms,
    };
    let folder_id = folder.id.clone();
    file.folders.push(folder);
    save(home, id, &mut file, now_ms)?;
    Ok((folder_id, sorted(&file)))
}

/// Rename a folder.
pub fn rename_folder(home: impl AsRef<Path>, id: &str, folder: &str, name: &str, now_ms: i64) -> Result<Tree, BookmarkError> {
    let home = home.as_ref();
    let name = name.trim();
    if name.is_empty() {
        return Err(BookmarkError::EmptyName);
    }
    let mut file = load(home, id)?;
    let f = file.folders.iter_mut().find(|f| f.id == folder).ok_or_else(|| BookmarkError::UnknownFolder(folder.to_string()))?;
    if f.name != name {
        f.name = name.to_string();
        f.updated_ms = now_ms;
        save(home, id, &mut file, now_ms)?;
    }
    Ok(sorted(&file))
}

/// Ids of `folder` and every folder inside it.
fn subtree(file: &File, folder: &str) -> HashSet<String> {
    let mut out = HashSet::from([folder.to_string()]);
    loop {
        let more: Vec<String> = file
            .folders
            .iter()
            .filter(|f| f.parent_id.as_ref().is_some_and(|p| out.contains(p)) && !out.contains(&f.id))
            .map(|f| f.id.clone())
            .collect();
        if more.is_empty() {
            return out;
        }
        out.extend(more);
    }
}

/// How many folders (including itself) and bookmarks deleting `folder` removes.
pub fn folder_contents(home: impl AsRef<Path>, id: &str, folder: &str) -> Result<(usize, usize), BookmarkError> {
    let file = load(home.as_ref(), id)?;
    require_folder(&file, Some(folder))?;
    let ids = subtree(&file, folder);
    let bookmarks = file.bookmarks.iter().filter(|b| b.folder_id.as_ref().is_some_and(|f| ids.contains(f))).count();
    Ok((ids.len(), bookmarks))
}

/// Delete a folder with everything in it.
pub fn delete_folder(home: impl AsRef<Path>, id: &str, folder: &str, now_ms: i64) -> Result<Tree, BookmarkError> {
    let home = home.as_ref();
    let mut file = load(home, id)?;
    require_folder(&file, Some(folder))?;
    let ids = subtree(&file, folder);
    let mut gone: Vec<String> = ids.iter().cloned().collect();
    gone.extend(file.bookmarks.iter().filter(|b| b.folder_id.as_ref().is_some_and(|f| ids.contains(f))).map(|b| b.id.clone()));
    file.folders.retain(|f| !ids.contains(&f.id));
    file.bookmarks.retain(|b| !b.folder_id.as_ref().is_some_and(|f| ids.contains(f)));
    gone.sort();
    file.deleted.extend(gone.into_iter().map(|id| Tombstone { id, deleted_ms: now_ms }));
    save(home, id, &mut file, now_ms)?;
    Ok(sorted(&file))
}

/// Move a bookmark or folder into `parent` (`None` = root) at slot `index`
/// among what is already there (0 = first; past the end = last). Only the
/// moved item's record changes.
pub fn move_item(home: impl AsRef<Path>, id: &str, item: &str, parent: Option<&str>, index: usize, now_ms: i64) -> Result<Tree, BookmarkError> {
    let home = home.as_ref();
    let mut file = load(home, id)?;
    require_folder(&file, parent)?;
    let is_folder = file.folders.iter().any(|f| f.id == item);
    if !is_folder && !file.bookmarks.iter().any(|b| b.id == item) {
        return Err(BookmarkError::UnknownItem(item.to_string()));
    }
    if is_folder && parent.is_some_and(|p| subtree(&file, item).contains(p)) {
        return Err(BookmarkError::Cycle);
    }
    let order = key_at(&sibling_keys(&file, parent, item), index);
    let parent = parent.map(str::to_string);
    if is_folder {
        let f = file.folders.iter_mut().find(|f| f.id == item).expect("checked");
        f.parent_id = parent;
        f.order = order;
        f.updated_ms = now_ms;
    } else {
        let b = file.bookmarks.iter_mut().find(|b| b.id == item).expect("checked");
        b.folder_id = parent;
        b.order = order;
        b.updated_ms = now_ms;
    }
    save(home, id, &mut file, now_ms)?;
    Ok(sorted(&file))
}

/// Tombstones still kept (for sync and tests).
pub fn tombstones(home: impl AsRef<Path>, id: &str) -> Result<Vec<Tombstone>, BookmarkError> {
    Ok(load(home.as_ref(), id)?.deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn home() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        profiles::create(&home, "Work").unwrap();
        (tmp, home)
    }

    fn names(tree: &Tree, parent: Option<&str>) -> Vec<String> {
        let mut items: Vec<(String, String)> = tree
            .folders
            .iter()
            .filter(|f| f.parent_id.as_deref() == parent)
            .map(|f| (f.order.clone(), format!("[{}]", f.name)))
            .chain(tree.bookmarks.iter().filter(|b| b.folder_id.as_deref() == parent).map(|b| (b.order.clone(), b.title.clone())))
            .collect();
        items.sort();
        items.into_iter().map(|(_, n)| n).collect()
    }

    fn by_title<'a>(tree: &'a Tree, title: &str) -> &'a Bookmark {
        tree.bookmarks.iter().find(|b| b.title == title).unwrap()
    }

    #[test]
    fn order_keys_always_fit_between() {
        let mut keys = vec![key_between(None, None)];
        // Always insert right after the first key: the worst case for growth.
        for i in 0..300 {
            let next = key_between(Some(&keys[0]), keys.get(1).map(String::as_str));
            assert!(keys[0] < next, "{i}");
            if let Some(after) = keys.get(1) {
                assert!(&next < after, "{i}");
            }
            assert!(!next.ends_with('0'));
            keys.insert(1, next);
        }
        let mut front = keys[0].clone();
        for _ in 0..300 {
            let k = key_between(None, Some(&front));
            assert!(k < front && !k.is_empty());
            front = k;
        }
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
        assert!(keys.iter().all(|k| k.len() < 80));
    }

    #[test]
    fn old_flat_files_load_newest_first_with_stable_ids() {
        let (_t, home) = home();
        let path = bookmarks_path(&home, "default");
        std::fs::write(
            &path,
            r#"{"bookmarks":[{"url":"https://old.test","title":"Old","added_ms":1},{"url":"https://new.test","title":"New","added_ms":2}]}"#,
        )
        .unwrap();
        let first = tree(&home, "default").unwrap();
        assert_eq!(names(&first, None), ["New", "Old"]);
        assert!(first.bookmarks.iter().all(|b| !b.id.is_empty() && b.updated_ms == b.added_ms));
        let again = tree(&home, "default").unwrap();
        assert_eq!(first, again, "ids stay the same before any write");
        // A write keeps them and records the new version.
        add_bookmark(&home, "default", "https://third.test", "Third", 3).unwrap();
        let after = tree(&home, "default").unwrap();
        assert_eq!(by_title(&after, "Old").id, by_title(&first, "Old").id);
        assert_eq!(names(&after, None), ["Third", "New", "Old"]);
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"version\": 2"));
    }

    #[test]
    fn folders_nest_rename_move_and_delete_with_tombstones() {
        let (_t, home) = home();
        for (i, t) in ["A", "B", "C"].iter().enumerate() {
            add_bookmark(&home, "p1", &format!("https://{t}.test"), t, i as i64 + 1).unwrap();
        }
        let (work, _) = create_folder(&home, "p1", "Work", None, 10).unwrap();
        let (docs, tree1) = create_folder(&home, "p1", "Docs", Some(&work), 11).unwrap();
        assert_eq!(names(&tree1, None), ["C", "B", "A", "[Work]"]);
        assert!(create_folder(&home, "p1", "  ", None, 12).is_err());
        assert!(create_folder(&home, "p1", "X", Some("nope"), 12).is_err());

        let a = by_title(&tree1, "A").id.clone();
        let c = by_title(&tree1, "C").id.clone();
        let t = move_item(&home, "p1", &a, Some(&docs), 0, 20).unwrap();
        assert_eq!(names(&t, Some(&docs)), ["A"]);
        assert_eq!(by_title(&t, "A").updated_ms, 20);
        assert_eq!(by_title(&t, "B").updated_ms, 2, "only the moved record changes");
        // Reorder in the root: C to the end, then to between B and Work.
        let t = move_item(&home, "p1", &c, None, 99, 21).unwrap();
        assert_eq!(names(&t, None), ["B", "[Work]", "C"]);
        let t = move_item(&home, "p1", &c, None, 1, 22).unwrap();
        assert_eq!(names(&t, None), ["B", "C", "[Work]"]);
        // Folders move too, but never into themselves.
        let t = move_item(&home, "p1", &docs, None, 0, 23).unwrap();
        assert_eq!(names(&t, None), ["[Docs]", "B", "C", "[Work]"]);
        let t = move_item(&home, "p1", &work, Some(&docs), 0, 24).unwrap();
        assert_eq!(names(&t, Some(&docs)), ["[Work]", "A"]);
        assert!(matches!(move_item(&home, "p1", &docs, Some(&work), 0, 25), Err(BookmarkError::Cycle)));
        assert!(matches!(move_item(&home, "p1", &docs, Some(&docs), 0, 25), Err(BookmarkError::Cycle)));
        assert!(matches!(move_item(&home, "p1", "ghost", None, 0, 25), Err(BookmarkError::UnknownItem(_))));

        let t = rename_folder(&home, "p1", &docs, "Reading", 30).unwrap();
        assert_eq!(names(&t, None)[0], "[Reading]");
        assert_eq!(t.folders.iter().find(|f| f.id == docs).unwrap().updated_ms, 30);

        assert_eq!(folder_contents(&home, "p1", &docs).unwrap(), (2, 1));
        let t = delete_folder(&home, "p1", &docs, 40).unwrap();
        assert_eq!(names(&t, None), ["B", "C"]);
        assert!(t.folders.is_empty());
        let mut dead: Vec<String> = tombstones(&home, "p1").unwrap().into_iter().map(|t| t.id).collect();
        dead.sort();
        let mut want = vec![docs.clone(), work.clone(), a.clone()];
        want.sort();
        assert_eq!(dead, want);
        // Re-adding A gives a new record; its old id stays dead.
        let list = add_bookmark(&home, "p1", "https://A.test", "A", 50).unwrap();
        assert_ne!(list.iter().find(|b| b.title == "A").unwrap().id, a);
        // Tombstones expire.
        add_bookmark(&home, "p1", "https://later.test", "L", 40 + TOMBSTONE_MS).unwrap();
        assert!(tombstones(&home, "p1").unwrap().is_empty());
    }

    #[test]
    fn tags_are_cleaned_counted_and_searchable() {
        let (_t, home) = home();
        add_bookmark(&home, "default", "https://rust-lang.org", "Rust", 1).unwrap();
        add_bookmark(&home, "default", "https://flutter.dev/docs", "Flutter docs", 2).unwrap();
        add_bookmark(&home, "default", "https://news.test", "News", 3).unwrap();
        let t = tree(&home, "default").unwrap();
        let rust = by_title(&t, "Rust").id.clone();
        let flutter = by_title(&t, "Flutter docs").id.clone();
        let edit = |tags: &[&str]| BookmarkEdit { tags: Some(tags.iter().map(|s| s.to_string()).collect()), ..Default::default() };
        let t = update_bookmark(&home, "default", &rust, edit(&[" Lang ", "#Systems", "lang", "", "  sys   prog "]), 5).unwrap();
        assert_eq!(by_title(&t, "Rust").tags, ["lang", "systems", "sys prog"]);
        assert_eq!(by_title(&t, "Rust").updated_ms, 5);
        update_bookmark(&home, "default", &flutter, edit(&["lang", "ui"]), 6).unwrap();

        let tag_list = tags(&home, "default").unwrap();
        assert_eq!(
            tag_list.iter().map(|t| (t.name.as_str(), t.count)).collect::<Vec<_>>(),
            [("lang", 2), ("sys prog", 1), ("systems", 1), ("ui", 1)]
        );
        let titles = |list: Vec<Bookmark>| list.into_iter().map(|b| b.title).collect::<Vec<_>>();
        assert_eq!(titles(search(&home, "default", "", Some("LANG")).unwrap()), ["Flutter docs", "Rust"]);
        assert_eq!(titles(search(&home, "default", "docs", None).unwrap()), ["Flutter docs"]);
        assert_eq!(titles(search(&home, "default", "systems", None).unwrap()), ["Rust"], "tags are searched");
        assert_eq!(titles(search(&home, "default", "RUST-LANG", None).unwrap()), ["Rust"], "urls are searched");
        assert_eq!(titles(search(&home, "default", "flutter ui", None).unwrap()), ["Flutter docs"], "every word must match");
        assert!(search(&home, "default", "news", Some("lang")).unwrap().is_empty());
        assert_eq!(search(&home, "default", "", None).unwrap().len(), 3);
        // Untouched edits do not bump the time.
        let t = update_bookmark(&home, "default", &rust, edit(&["lang", "systems", "sys prog"]), 9).unwrap();
        assert_eq!(by_title(&t, "Rust").updated_ms, 5);
    }

    #[test]
    fn edits_and_deletes_by_id() {
        let (_t, home) = home();
        add_bookmark(&home, "default", "https://a.test", "A", 1).unwrap();
        let id = tree(&home, "default").unwrap().bookmarks[0].id.clone();
        let t = update_bookmark(
            &home,
            "default",
            &id,
            BookmarkEdit { title: Some(" Alpha ".into()), url: Some("https://alpha.test".into()), tags: None },
            2,
        )
        .unwrap();
        assert_eq!((t.bookmarks[0].title.as_str(), t.bookmarks[0].url.as_str()), ("Alpha", "https://alpha.test"));
        assert!(matches!(
            update_bookmark(&home, "default", &id, BookmarkEdit { url: Some("about:blank".into()), ..Default::default() }, 3),
            Err(BookmarkError::BadUrl(_))
        ));
        assert!(is_bookmarked(&home, "default", "https://alpha.test/").unwrap());
        assert!(delete_bookmark(&home, "default", &id, 4).unwrap().bookmarks.is_empty());
        assert!(delete_bookmark(&home, "default", &id, 5).is_err());
        assert_eq!(tombstones(&home, "default").unwrap(), [Tombstone { id, deleted_ms: 4 }]);
    }

    #[test]
    fn a_damaged_tree_recovers_to_the_root() {
        let (_t, home) = home();
        std::fs::write(
            bookmarks_path(&home, "default"),
            r#"{"version":2,
               "folders":[{"id":"f1","name":"Loop1","parent_id":"f2","order":"V","created_ms":1,"updated_ms":1},
                          {"id":"f2","name":"Loop2","parent_id":"f1","order":"W","created_ms":1,"updated_ms":1}],
               "bookmarks":[{"id":"b1","url":"https://x.test","title":"X","added_ms":1,"folder_id":"gone","order":"a"}]}"#,
        )
        .unwrap();
        let t = tree(&home, "default").unwrap();
        assert!(t.bookmarks[0].folder_id.is_none());
        assert!(t.folders.iter().any(|f| f.parent_id.is_none()), "cycle broken");
    }

    #[test]
    fn profiles_keep_their_own_trees() {
        let (_t, home) = home();
        create_folder(&home, "p1", "Work only", None, 1).unwrap();
        assert!(tree(&home, "default").unwrap().folders.is_empty());
        assert!(tree(&home, "nope").is_err());
        assert!(create_folder(&home, "../x", "F", None, 1).is_err());
    }
}
