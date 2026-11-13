//! The records a library holds. These are also the shapes of the older
//! `bookmarks.json` and `history.json` files, so `morph-core` reads both with them.
use serde::{Deserialize, Serialize};

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

/// One visit to a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visited_ms: i64,
}

/// Everything in a profile's bookmark tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookmarkSet {
    pub bookmarks: Vec<Bookmark>,
    pub folders: Vec<Folder>,
    pub deleted: Vec<Tombstone>,
}
