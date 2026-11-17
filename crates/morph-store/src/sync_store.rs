//! Persistence for the library op log, cursors, merge clocks and conflicts.
use super::{Library, Result};
use weirdnet_storage::rusqlite;
use weirdnet_storage::rusqlite::params;

/// One row of `sync_log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncLogEntry {
    pub node_id: String,
    pub seq: i64,
    pub hlc: String,
    pub typ: String,
    pub record_id: String,
    pub op: Vec<u8>,
}

/// A losing version kept for the UI (30 days).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConflict {
    pub id: String,
    pub typ: String,
    pub record_id: String,
    pub hlc: String,
    pub op: Vec<u8>,
    pub kept_until_ms: i64,
}

/// Last-writer-wins field clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncLwwRow {
    pub record_id: String,
    pub field: String,
    pub hlc: String,
    pub value: Option<String>,
}

/// Observed-remove set element (tags).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOrSetRow {
    pub record_id: String,
    pub elem: String,
    pub added_hlc: Option<String>,
    pub removed_hlc: Option<String>,
}

impl Library {
    pub fn sync_log(&self) -> Result<Vec<SyncLogEntry>> {
        let mut q = self.conn.prepare("SELECT node_id, seq, hlc, typ, record_id, op FROM sync_log ORDER BY node_id, seq")?;
        let rows = q
            .query_map([], |r| {
                Ok(SyncLogEntry { node_id: r.get(0)?, seq: r.get(1)?, hlc: r.get(2)?, typ: r.get(3)?, record_id: r.get(4)?, op: r.get(5)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn save_sync_log(&mut self, entries: &[SyncLogEntry]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM sync_log", [])?;
        let mut ins = tx.prepare("INSERT INTO sync_log (node_id, seq, hlc, typ, record_id, op) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
        for e in entries {
            ins.execute(params![e.node_id, e.seq, e.hlc, e.typ, e.record_id, e.op])?;
        }
        drop(ins);
        tx.commit()?;
        Ok(())
    }

    pub fn insert_sync_log(&mut self, e: &SyncLogEntry) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_log (node_id, seq, hlc, typ, record_id, op) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![e.node_id, e.seq, e.hlc, e.typ, e.record_id, e.op],
        )?;
        Ok(())
    }

    pub fn sync_cursors(&self) -> Result<Vec<(String, i64)>> {
        let mut q = self.conn.prepare("SELECT node_id, last_seq FROM sync_cursor ORDER BY node_id")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_sync_cursor(&mut self, node_id: &str, last_seq: i64) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO sync_cursor (node_id, last_seq) VALUES (?1, ?2)", params![node_id, last_seq])?;
        Ok(())
    }

    pub fn save_sync_cursors(&mut self, cursors: &[(String, i64)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM sync_cursor", [])?;
        let mut ins = tx.prepare("INSERT INTO sync_cursor (node_id, last_seq) VALUES (?1, ?2)")?;
        for (n, s) in cursors {
            ins.execute(params![n, s])?;
        }
        drop(ins);
        tx.commit()?;
        Ok(())
    }

    pub fn sync_meta(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut q = self.conn.prepare("SELECT value FROM sync_meta WHERE key = ?1")?;
        let mut rows = q.query(params![key])?;
        match rows.next()? {
            Some(r) => Ok(Some(r.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn set_sync_meta(&mut self, key: &str, value: &[u8]) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO sync_meta (key, value) VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    pub fn sync_lww(&self) -> Result<Vec<SyncLwwRow>> {
        let mut q = self.conn.prepare("SELECT record_id, field, hlc, value FROM sync_lww")?;
        let rows = q
            .query_map([], |r| Ok(SyncLwwRow { record_id: r.get(0)?, field: r.get(1)?, hlc: r.get(2)?, value: r.get(3)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_sync_lww(&mut self, record_id: &str, field: &str, hlc: &str, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_lww (record_id, field, hlc, value) VALUES (?1, ?2, ?3, ?4)",
            params![record_id, field, hlc, value],
        )?;
        Ok(())
    }

    pub fn sync_orset(&self) -> Result<Vec<SyncOrSetRow>> {
        let mut q = self.conn.prepare("SELECT record_id, elem, added_hlc, removed_hlc FROM sync_orset")?;
        let rows = q
            .query_map([], |r| Ok(SyncOrSetRow { record_id: r.get(0)?, elem: r.get(1)?, added_hlc: r.get(2)?, removed_hlc: r.get(3)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_sync_orset(&mut self, row: &SyncOrSetRow) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_orset (record_id, elem, added_hlc, removed_hlc) VALUES (?1, ?2, ?3, ?4)",
            params![&row.record_id, &row.elem, &row.added_hlc, &row.removed_hlc],
        )?;
        Ok(())
    }

    pub fn sync_deleted(&self) -> Result<Vec<(String, String)>> {
        let mut q = self.conn.prepare("SELECT record_id, hlc FROM sync_deleted")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_sync_deleted(&mut self, record_id: &str, hlc: &str) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO sync_deleted (record_id, hlc) VALUES (?1, ?2)", params![record_id, hlc])?;
        Ok(())
    }

    pub fn clear_sync_deleted(&mut self, record_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM sync_deleted WHERE record_id = ?1", params![record_id])?;
        Ok(())
    }

    pub fn sync_conflicts(&self) -> Result<Vec<SyncConflict>> {
        let mut q = self.conn.prepare("SELECT id, typ, record_id, hlc, op, kept_until_ms FROM sync_conflict ORDER BY record_id")?;
        let rows = q
            .query_map([], |r| {
                Ok(SyncConflict { id: r.get(0)?, typ: r.get(1)?, record_id: r.get(2)?, hlc: r.get(3)?, op: r.get(4)?, kept_until_ms: r.get(5)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn add_sync_conflict(&mut self, c: &SyncConflict) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_conflict (id, typ, record_id, hlc, op, kept_until_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![c.id, c.typ, c.record_id, c.hlc, c.op, c.kept_until_ms],
        )?;
        Ok(())
    }

    pub fn expire_sync_conflicts(&mut self, now_ms: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sync_conflict WHERE kept_until_ms <= ?1", params![now_ms])?;
        Ok(())
    }
}
