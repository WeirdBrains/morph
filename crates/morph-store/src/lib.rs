//! A profile's library database: bookmarks, folders, tags and history in one
//! SQLCipher file, encrypted at rest.
//!
//! ```text
//! <home>/profiles/<id>/library.db
//! ```
//!
//! The key is the profile's [`LibraryKey`], derived with HKDF from its Weirdnet
//! identity seed (see `morph_identity::profile_identity`), so the file opens only
//! while that identity is unlocked, and only for that profile. SQLCipher and
//! OpenSSL are built from source through `weirdnet-storage`, so the same code
//! runs on Android, Linux, macOS and Windows with no system libraries.
//!
//! This crate stores records and nothing more. Rules about what to keep (same
//! page, history cap, tidying bookmark trees, importing the old JSON files) live
//! in `morph-core`.
use std::path::{Path, PathBuf};

use morph_identity::profile_identity::LibraryKey;
use weirdnet_storage::rusqlite::{self, Connection, params};

mod model;

pub use model::{Bookmark, BookmarkSet, Folder, HistoryEntry, Tombstone};

/// File name inside the profile directory.
pub const FILE_NAME: &str = "library.db";

/// Name used in error messages.
const WHAT: &str = "library";

/// Schema migrations, oldest first. Never edit a shipped entry; append.
const MIGRATIONS: &[&str] = &["
CREATE TABLE folders (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    parent_id TEXT,
    sort_key TEXT NOT NULL,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL
);
CREATE TABLE bookmarks (
    id TEXT PRIMARY KEY NOT NULL,
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    folder_id TEXT,
    sort_key TEXT NOT NULL,
    added_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL
);
CREATE TABLE bookmark_tags (
    bookmark_id TEXT NOT NULL REFERENCES bookmarks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY (bookmark_id, position)
);
CREATE TABLE tombstones (
    id TEXT PRIMARY KEY NOT NULL,
    deleted_ms INTEGER NOT NULL
);
CREATE TABLE history (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    visited_ms INTEGER NOT NULL
);
"];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Storage(#[from] weirdnet_storage::StorageError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// `<profile_dir>/library.db`.
pub fn path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(FILE_NAME)
}

/// Whether the profile has a library database yet.
pub fn exists(profile_dir: &Path) -> bool {
    path(profile_dir).exists()
}

/// An open, unlocked library database.
pub struct Library {
    conn: Connection,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Library(..)")
    }
}

impl Library {
    /// Open the profile's database with its key, creating it (private to the
    /// user) and bringing its schema up to date. A wrong key is an error and
    /// leaves the file alone.
    pub fn open(profile_dir: &Path, key: &LibraryKey) -> Result<Self> {
        let mut conn = weirdnet_storage::open(&path(profile_dir), Some(key.expose()), WHAT)?;
        weirdnet_storage::migrate::<StoreError>(&mut conn, MIGRATIONS, MIGRATIONS.len(), |_, _| Ok(()))?;
        Ok(Self { conn })
    }

    /// The schema version (the number of applied migrations).
    pub fn schema_version(&self) -> Result<i64> {
        Ok(weirdnet_storage::schema_version(&self.conn)?)
    }

    // --- Bookmarks -------------------------------------------------------

    /// Every folder, bookmark (with its tags) and tombstone, in the order they
    /// were saved.
    pub fn bookmark_set(&self) -> Result<BookmarkSet> {
        let folders = self
            .conn
            .prepare("SELECT id, name, parent_id, sort_key, created_ms, updated_ms FROM folders ORDER BY rowid")?
            .query_map([], |r| {
                Ok(Folder { id: r.get(0)?, name: r.get(1)?, parent_id: r.get(2)?, order: r.get(3)?, created_ms: r.get(4)?, updated_ms: r.get(5)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tags = self.conn.prepare("SELECT tag FROM bookmark_tags WHERE bookmark_id = ?1 ORDER BY position")?;
        let mut bookmarks = self
            .conn
            .prepare("SELECT id, url, title, folder_id, sort_key, added_ms, updated_ms FROM bookmarks ORDER BY rowid")?
            .query_map([], |r| {
                Ok(Bookmark {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    title: r.get(2)?,
                    folder_id: r.get(3)?,
                    order: r.get(4)?,
                    added_ms: r.get(5)?,
                    updated_ms: r.get(6)?,
                    tags: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for b in &mut bookmarks {
            b.tags = tags.query_map([&b.id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        }
        let deleted = self
            .conn
            .prepare("SELECT id, deleted_ms FROM tombstones ORDER BY rowid")?
            .query_map([], |r| Ok(Tombstone { id: r.get(0)?, deleted_ms: r.get(1)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(BookmarkSet { bookmarks, folders, deleted })
    }

    /// Replace every folder, bookmark and tombstone with `set`, in one
    /// transaction. Ids must be unique within each list.
    pub fn save_bookmark_set(&mut self, set: &BookmarkSet) -> Result<()> {
        let tx = self.conn.transaction()?;
        replace_bookmarks(&tx, set)?;
        tx.commit()?;
        Ok(())
    }

    /// Whether the database holds no bookmarks, folders, tombstones or history.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT NOT EXISTS (SELECT 1 FROM bookmarks) AND NOT EXISTS (SELECT 1 FROM folders)
                AND NOT EXISTS (SELECT 1 FROM tombstones) AND NOT EXISTS (SELECT 1 FROM history)",
            [],
            |r| r.get(0),
        )?)
    }

    /// Bring in a library kept elsewhere (the old JSON files), all or nothing,
    /// and only into an empty database. Returns whether anything was written:
    /// `false` means the database already had records and was left alone.
    pub fn import(&mut self, set: &BookmarkSet, visits: &[HistoryEntry], cap: usize) -> Result<bool> {
        if !self.is_empty()? {
            return Ok(false);
        }
        let tx = self.conn.transaction()?;
        replace_bookmarks(&tx, set)?;
        append_visits(&tx, visits, cap)?;
        tx.commit()?;
        Ok(true)
    }

    // --- History ---------------------------------------------------------

    /// Every visit, newest first.
    pub fn history(&self) -> Result<Vec<HistoryEntry>> {
        Ok(self
            .conn
            .prepare("SELECT url, title, visited_ms FROM history ORDER BY seq DESC")?
            .query_map([], |r| Ok(HistoryEntry { url: r.get(0)?, title: r.get(1)?, visited_ms: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The newest visit, if any.
    pub fn newest_visit(&self) -> Result<Option<HistoryEntry>> {
        let mut q = self.conn.prepare("SELECT url, title, visited_ms FROM history ORDER BY seq DESC LIMIT 1")?;
        let mut rows = q.query_map([], |r| Ok(HistoryEntry { url: r.get(0)?, title: r.get(1)?, visited_ms: r.get(2)? }))?;
        Ok(rows.next().transpose()?)
    }

    /// Replace the newest visit with `entry` (a reload or title update).
    pub fn replace_newest_visit(&self, entry: &HistoryEntry) -> Result<()> {
        self.conn.execute(
            "UPDATE history SET url = ?1, title = ?2, visited_ms = ?3 WHERE seq = (SELECT MAX(seq) FROM history)",
            params![entry.url, entry.title, entry.visited_ms],
        )?;
        Ok(())
    }

    /// Append visits, oldest first, then keep only the newest `cap` (0 keeps all).
    pub fn append_visits(&mut self, entries: &[HistoryEntry], cap: usize) -> Result<()> {
        let tx = self.conn.transaction()?;
        append_visits(&tx, entries, cap)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove the visits `remove` picks. Returns how many went.
    pub fn delete_visits(&mut self, mut remove: impl FnMut(&HistoryEntry) -> bool) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let picked: Vec<i64> = {
            let mut q = tx.prepare("SELECT seq, url, title, visited_ms FROM history")?;
            let rows = q.query_map([], |r| Ok((r.get::<_, i64>(0)?, HistoryEntry { url: r.get(1)?, title: r.get(2)?, visited_ms: r.get(3)? })))?;
            let mut picked = Vec::new();
            for row in rows {
                let (seq, e) = row?;
                if remove(&e) {
                    picked.push(seq);
                }
            }
            picked
        };
        {
            let mut del = tx.prepare("DELETE FROM history WHERE seq = ?1")?;
            for seq in &picked {
                del.execute([seq])?;
            }
        }
        tx.commit()?;
        Ok(picked.len())
    }

    /// Forget all history.
    pub fn clear_history(&self) -> Result<()> {
        self.conn.execute("DELETE FROM history", [])?;
        Ok(())
    }
}

fn replace_bookmarks(tx: &rusqlite::Transaction, set: &BookmarkSet) -> Result<()> {
    tx.execute_batch("DELETE FROM bookmark_tags; DELETE FROM bookmarks; DELETE FROM folders; DELETE FROM tombstones;")?;
    let mut f = tx.prepare("INSERT INTO folders (id, name, parent_id, sort_key, created_ms, updated_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
    for x in &set.folders {
        f.execute(params![x.id, x.name, x.parent_id, x.order, x.created_ms, x.updated_ms])?;
    }
    let mut b =
        tx.prepare("INSERT INTO bookmarks (id, url, title, folder_id, sort_key, added_ms, updated_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
    let mut t = tx.prepare("INSERT INTO bookmark_tags (bookmark_id, position, tag) VALUES (?1, ?2, ?3)")?;
    for x in &set.bookmarks {
        b.execute(params![x.id, x.url, x.title, x.folder_id, x.order, x.added_ms, x.updated_ms])?;
        for (i, tag) in x.tags.iter().enumerate() {
            t.execute(params![x.id, i as i64, tag])?;
        }
    }
    let mut d = tx.prepare("INSERT INTO tombstones (id, deleted_ms) VALUES (?1, ?2)")?;
    for x in &set.deleted {
        d.execute(params![x.id, x.deleted_ms])?;
    }
    Ok(())
}

fn append_visits(tx: &rusqlite::Transaction, entries: &[HistoryEntry], cap: usize) -> Result<()> {
    let mut add = tx.prepare("INSERT INTO history (url, title, visited_ms) VALUES (?1, ?2, ?3)")?;
    for e in entries {
        add.execute(params![e.url, e.title, e.visited_ms])?;
    }
    if cap > 0 {
        tx.execute("DELETE FROM history WHERE seq NOT IN (SELECT seq FROM history ORDER BY seq DESC LIMIT ?1)", [cap as i64])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
