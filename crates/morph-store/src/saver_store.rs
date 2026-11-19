//! Persistence for the password saver: saved logins whose password is already
//! sealed by morph-core. This crate never sees a plaintext password.
use super::{Library, Result};
use weirdnet_storage::rusqlite::{self, OptionalExtension, params};

/// One row of `saved_logins`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SavedLogin {
    pub id: String,
    /// Scheme, host and port of the page (`https://example.com`).
    pub origin: String,
    pub username: String,
    /// The password sealed under the saver key (opaque here).
    pub sealed: Vec<u8>,
    pub created_ms: i64,
    pub updated_ms: i64,
    /// Sync clock; empty until password sync is wired.
    pub hlc: String,
    pub node_id: String,
}

impl std::fmt::Debug for SavedLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedLogin").field("id", &self.id).field("origin", &self.origin).field("username", &self.username).finish_non_exhaustive()
    }
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SavedLogin> {
    Ok(SavedLogin {
        id: r.get(0)?,
        origin: r.get(1)?,
        username: r.get(2)?,
        sealed: r.get(3)?,
        created_ms: r.get(4)?,
        updated_ms: r.get(5)?,
        hlc: r.get(6)?,
        node_id: r.get(7)?,
    })
}

const COLUMNS: &str = "id, origin, username, sealed, created_ms, updated_ms, hlc, node_id";

impl Library {
    /// Insert or replace a saved login (by id).
    pub fn put_login(&mut self, login: &SavedLogin) -> Result<()> {
        self.conn.execute(
            "INSERT INTO saved_logins (id, origin, username, sealed, created_ms, updated_ms, hlc, node_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET origin = ?2, username = ?3, sealed = ?4, updated_ms = ?6, hlc = ?7, node_id = ?8",
            params![login.id, login.origin, login.username, login.sealed, login.created_ms, login.updated_ms, login.hlc, login.node_id],
        )?;
        Ok(())
    }

    /// Every saved login, by origin then username. With `origin`, only that origin's.
    pub fn logins(&self, origin: Option<&str>) -> Result<Vec<SavedLogin>> {
        let sql = format!("SELECT {COLUMNS} FROM saved_logins WHERE (?1 IS NULL OR origin = ?1) ORDER BY origin, username, id");
        let mut stmt = self.conn.prepare(&sql)?;
        Ok(stmt.query_map(params![origin], row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// One saved login by id.
    pub fn login(&self, id: &str) -> Result<Option<SavedLogin>> {
        let sql = format!("SELECT {COLUMNS} FROM saved_logins WHERE id = ?1");
        Ok(self.conn.query_row(&sql, params![id], row).optional()?)
    }

    /// Delete a saved login; whether it existed.
    pub fn delete_login(&mut self, id: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM saved_logins WHERE id = ?1", params![id])? > 0)
    }
}
