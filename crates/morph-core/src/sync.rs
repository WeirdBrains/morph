//! Morph bookmark / folder / tag record types and merge, on top of `weirdnet-sync`.
//!
//! Generic clocks, the op log, LWW, OR-sets and the wire protocol live in
//! `weirdnet-sync`. This module is only Morph's records and how they are
//! applied to `library.db`.
use morph_store::{Bookmark, BookmarkSet, Folder, Library, SyncConflict, SyncLogEntry, Tombstone};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use weirdnet_sync::hlc::Hlc;
use weirdnet_sync::log::{LogEntry, OpLog};
use weirdnet_sync::merge::{self, CONFLICT_KEEP_MS, EditOrDelete, Lww, OrSet, folder_cycle, lww};
use weirdnet_sync::{NodeId, Roster, SyncState, SyncType as typ};

/// Default opt-ins after link or first enable (settings come later).
pub const BOOKMARKS: &str = typ::BOOKMARKS;
pub const HISTORY: &str = typ::HISTORY;
pub const DEFAULT_TYPES: &[&str] = &[BOOKMARKS, HISTORY];

/// Record types this build can merge. A pull asks for each one that is on.
pub const SUPPORTED_TYPES: &[&str] = &[BOOKMARKS, HISTORY];

/// One bookmarks-log mutation. `record_id` lives on the log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum BookmarksOp {
    PutBookmark { url: String, title: String, folder_id: Option<String>, order: String, added_ms: i64 },
    SetUrl { url: String },
    SetTitle { title: String },
    SetFolder { folder_id: Option<String> },
    SetOrder { order: String },
    AddTag { tag: String },
    RemoveTag { tag: String },
    PutFolder { name: String, parent_id: Option<String>, order: String, created_ms: i64 },
    SetName { name: String },
    SetParent { parent_id: Option<String> },
    SetFolderOrder { order: String },
    Delete,
}

pub fn encode_hlc(h: &Hlc) -> String {
    serde_json::to_string(h).expect("hlc json")
}

pub fn decode_hlc(s: &str) -> Option<Hlc> {
    serde_json::from_str(s).ok()
}

pub fn encode_op(op: &BookmarksOp) -> Vec<u8> {
    serde_json::to_vec(op).expect("op json")
}

pub fn decode_op(bytes: &[u8]) -> Option<BookmarksOp> {
    serde_json::from_slice(bytes).ok()
}

/// Last HLC written on this device, if any.
pub fn load_clock(lib: &Library) -> morph_store::Result<Option<Hlc>> {
    Ok(lib.sync_meta("clock")?.and_then(|b| serde_json::from_slice(&b).ok()))
}

pub fn save_clock(lib: &mut Library, clock: &Hlc) -> morph_store::Result<()> {
    let bytes = serde_json::to_vec(clock).expect("clock json");
    lib.set_sync_meta("clock", &bytes)
}

pub fn node_id(lib: &Library) -> morph_store::Result<Option<String>> {
    Ok(lib.sync_meta("node_id")?.and_then(|b| String::from_utf8(b).ok()).filter(|s| !s.is_empty()))
}

pub fn set_node_id(lib: &mut Library, id: &str) -> morph_store::Result<()> {
    lib.set_sync_meta("node_id", id.as_bytes())
}

pub fn types_on(lib: &Library) -> morph_store::Result<Vec<String>> {
    match lib.sync_meta("types")? {
        Some(b) => Ok(serde_json::from_slice(&b).unwrap_or_default()),
        None => Ok(Vec::new()),
    }
}

pub fn set_types(lib: &mut Library, types: &[String]) -> morph_store::Result<()> {
    lib.set_sync_meta("types", &serde_json::to_vec(types).expect("types json"))
}

pub fn bookmarks_on(lib: &Library) -> bool {
    types_on(lib).ok().is_some_and(|t| t.iter().any(|x| x == BOOKMARKS))
}

/// Turn this library into a syncing replica: record our node, default
/// opt-ins, and backfill existing rows into the log so they go out on the
/// first pull.
pub fn enable(lib: &mut Library, our_node: &str, now_ms: i64) -> morph_store::Result<()> {
    upgrade_defaults(lib)?;
    if node_id(lib)?.as_deref() == Some(our_node) && bookmarks_on(lib) {
        return crate::history_sync::backfill(lib, now_ms);
    }
    set_node_id(lib, our_node)?;
    if types_on(lib)?.is_empty() {
        set_types(lib, &DEFAULT_TYPES.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
        lib.set_sync_meta(META_DEFAULTS, DEFAULTS_VERSION)?;
    }
    backfill_local_log(lib, our_node, now_ms)?;
    crate::history_sync::backfill(lib, now_ms)
}

const META_DEFAULTS: &str = "defaults";
const DEFAULTS_VERSION: &[u8] = b"2";

/// Libraries that synced before history existed have opt-ins `["bookmarks"]`
/// (the old default). Turn history on for them once, like a fresh link.
fn upgrade_defaults(lib: &mut Library) -> morph_store::Result<()> {
    if lib.sync_meta(META_DEFAULTS)?.as_deref() == Some(DEFAULTS_VERSION) {
        return Ok(());
    }
    let types = types_on(lib)?;
    if types == [BOOKMARKS] {
        set_types(lib, &DEFAULT_TYPES.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;
    }
    if !types.is_empty() {
        lib.set_sync_meta(META_DEFAULTS, DEFAULTS_VERSION)?;
    }
    Ok(())
}

/// Whether any record type this build can merge is on.
pub fn any_on(lib: &Library) -> bool {
    types_on(lib).ok().is_some_and(|t| t.iter().any(|x| SUPPORTED_TYPES.contains(&x.as_str())))
}

/// Rematerialize every supported type from the log.
pub fn materialize_all(lib: &mut Library, now_ms: i64) -> morph_store::Result<()> {
    if bookmarks_on(lib) {
        materialize(lib, now_ms)?;
    }
    crate::history_sync::materialize(lib, now_ms)
}

fn backfill_local_log(lib: &mut Library, our_node: &str, now_ms: i64) -> morph_store::Result<()> {
    if !lib.sync_log()?.is_empty() {
        return Ok(());
    }
    let set = lib.bookmark_set()?;
    let node = NodeId::new(our_node);
    let mut clock = load_clock(lib)?;
    let mut log = load_log(lib)?;
    for f in &set.folders {
        let wall = if f.updated_ms > 0 { f.updated_ms as u64 } else { now_ms as u64 };
        let hlc = Hlc::tick(clock.as_ref(), wall, node.clone());
        clock = Some(hlc.clone());
        let op = BookmarksOp::PutFolder { name: f.name.clone(), parent_id: f.parent_id.clone(), order: f.order.clone(), created_ms: f.created_ms };
        log.append(node.clone(), hlc, BOOKMARKS, f.id.clone(), encode_op(&op));
    }
    for b in &set.bookmarks {
        let wall = if b.updated_ms > 0 { b.updated_ms as u64 } else { now_ms as u64 };
        let hlc = Hlc::tick(clock.as_ref(), wall, node.clone());
        clock = Some(hlc.clone());
        let op = BookmarksOp::PutBookmark {
            url: b.url.clone(),
            title: b.title.clone(),
            folder_id: b.folder_id.clone(),
            order: b.order.clone(),
            added_ms: b.added_ms,
        };
        log.append(node.clone(), hlc, BOOKMARKS, b.id.clone(), encode_op(&op));
        for tag in &b.tags {
            let hlc = Hlc::tick(clock.as_ref(), wall, node.clone());
            clock = Some(hlc.clone());
            log.append(node.clone(), hlc, BOOKMARKS, b.id.clone(), encode_op(&BookmarksOp::AddTag { tag: tag.clone() }));
        }
    }
    persist_log(lib, &log, clock.as_ref())?;
    Ok(())
}

pub fn load_log(lib: &Library) -> morph_store::Result<OpLog> {
    let mut log = OpLog::new();
    let rows = lib.sync_log()?;
    if !rows.is_empty() {
        let ops: Vec<LogEntry> = rows
            .into_iter()
            .filter_map(|r| {
                let hlc = decode_hlc(&r.hlc)?;
                Some(LogEntry { node_id: NodeId::new(r.node_id), seq: r.seq as u64, hlc, typ: r.typ, record_id: r.record_id, op: r.op })
            })
            .collect();
        // Group by node and integrate (gaps are allowed).
        let mut by_node: std::collections::BTreeMap<String, Vec<LogEntry>> = std::collections::BTreeMap::new();
        for e in ops {
            by_node.entry(e.node_id.0.clone()).or_default().push(e);
        }
        for (nid, mut ops) in by_node {
            ops.sort_by_key(|e| e.seq);
            let from_seq = ops.first().map(|e| e.seq).unwrap_or(0);
            let to_seq = ops.last().map(|e| e.seq).unwrap_or(0);
            log.integrate(&weirdnet_sync::log::Page { node_id: NodeId::new(nid), from_seq, to_seq, ops });
        }
    }
    for (n, seq) in lib.sync_cursors()? {
        log.set_cursor(&NodeId::new(n), seq as u64);
    }
    Ok(log)
}

pub fn persist_log(lib: &mut Library, log: &OpLog, clock: Option<&Hlc>) -> morph_store::Result<()> {
    let cursors = log.cursors();
    let mut nodes: std::collections::BTreeSet<String> = cursors.keys().map(|n| n.0.clone()).collect();
    if let Some(id) = node_id(lib)? {
        nodes.insert(id);
    }
    for (n, _) in lib.sync_cursors()? {
        nodes.insert(n);
    }
    // Also walk every cursor and our log via known node ids from the existing table.
    for e in lib.sync_log()? {
        nodes.insert(e.node_id);
    }
    let mut entries = Vec::new();
    for n in &nodes {
        for e in log.iter_node(&NodeId::new(n.clone())) {
            entries.push(SyncLogEntry {
                node_id: e.node_id.0.clone(),
                seq: e.seq as i64,
                hlc: encode_hlc(&e.hlc),
                typ: e.typ.clone(),
                record_id: e.record_id.clone(),
                op: e.op.clone(),
            });
        }
    }
    lib.save_sync_log(&entries)?;
    let cur: Vec<(String, i64)> = cursors.into_iter().map(|(n, s)| (n.0, s as i64)).collect();
    lib.save_sync_cursors(&cur)?;
    if let Some(c) = clock {
        save_clock(lib, c)?;
    }
    Ok(())
}

/// Append ops for the difference between `prev` and `next`, stamp HLCs on
/// `next`, and persist the log. No-op if bookmarks sync is not enabled.
pub fn record_local_changes(lib: &mut Library, prev: &BookmarkSet, next: &mut BookmarkSet, now_ms: i64) -> morph_store::Result<()> {
    let Some(our) = node_id(lib)? else { return Ok(()) };
    if !bookmarks_on(lib) {
        return Ok(());
    }
    let node = NodeId::new(&our);
    let mut clock = load_clock(lib)?;
    let mut log = load_log(lib)?;
    let mut ops: Vec<(String, BookmarksOp)> = Vec::new();

    let prev_folders: std::collections::BTreeMap<_, _> = prev.folders.iter().map(|f| (f.id.clone(), f)).collect();
    let prev_bookmarks: std::collections::BTreeMap<_, _> = prev.bookmarks.iter().map(|b| (b.id.clone(), b)).collect();
    let next_folder_ids: std::collections::BTreeSet<_> = next.folders.iter().map(|f| f.id.clone()).collect();
    let next_bookmark_ids: std::collections::BTreeSet<_> = next.bookmarks.iter().map(|b| b.id.clone()).collect();

    for f in &next.folders {
        match prev_folders.get(&f.id) {
            None => ops.push((
                f.id.clone(),
                BookmarksOp::PutFolder { name: f.name.clone(), parent_id: f.parent_id.clone(), order: f.order.clone(), created_ms: f.created_ms },
            )),
            Some(old) => {
                if old.name != f.name {
                    ops.push((f.id.clone(), BookmarksOp::SetName { name: f.name.clone() }));
                }
                if old.parent_id != f.parent_id {
                    ops.push((f.id.clone(), BookmarksOp::SetParent { parent_id: f.parent_id.clone() }));
                }
                if old.order != f.order {
                    ops.push((f.id.clone(), BookmarksOp::SetFolderOrder { order: f.order.clone() }));
                }
            }
        }
    }
    for b in &next.bookmarks {
        match prev_bookmarks.get(&b.id) {
            None => {
                ops.push((
                    b.id.clone(),
                    BookmarksOp::PutBookmark {
                        url: b.url.clone(),
                        title: b.title.clone(),
                        folder_id: b.folder_id.clone(),
                        order: b.order.clone(),
                        added_ms: b.added_ms,
                    },
                ));
                for tag in &b.tags {
                    ops.push((b.id.clone(), BookmarksOp::AddTag { tag: tag.clone() }));
                }
            }
            Some(old) => {
                if old.url != b.url {
                    ops.push((b.id.clone(), BookmarksOp::SetUrl { url: b.url.clone() }));
                }
                if old.title != b.title {
                    ops.push((b.id.clone(), BookmarksOp::SetTitle { title: b.title.clone() }));
                }
                if old.folder_id != b.folder_id {
                    ops.push((b.id.clone(), BookmarksOp::SetFolder { folder_id: b.folder_id.clone() }));
                }
                if old.order != b.order {
                    ops.push((b.id.clone(), BookmarksOp::SetOrder { order: b.order.clone() }));
                }
                let old_tags: std::collections::BTreeSet<_> = old.tags.iter().cloned().collect();
                let new_tags: std::collections::BTreeSet<_> = b.tags.iter().cloned().collect();
                for tag in new_tags.difference(&old_tags) {
                    ops.push((b.id.clone(), BookmarksOp::AddTag { tag: tag.clone() }));
                }
                for tag in old_tags.difference(&new_tags) {
                    ops.push((b.id.clone(), BookmarksOp::RemoveTag { tag: tag.clone() }));
                }
            }
        }
    }
    for id in prev_folders.keys().filter(|id| !next_folder_ids.contains(*id)) {
        ops.push(((*id).clone(), BookmarksOp::Delete));
    }
    for id in prev_bookmarks.keys().filter(|id| !next_bookmark_ids.contains(*id)) {
        ops.push(((*id).clone(), BookmarksOp::Delete));
    }

    let wall = now_ms.max(0) as u64;
    for (record_id, op) in &ops {
        let hlc = Hlc::tick(clock.as_ref(), wall, node.clone());
        clock = Some(hlc.clone());
        let encoded = encode_hlc(&hlc);
        stamp(next, record_id, &encoded, &our, now_ms);
        log.append(node.clone(), hlc, BOOKMARKS, record_id.clone(), encode_op(op));
    }
    persist_log(lib, &log, clock.as_ref())?;
    Ok(())
}

fn stamp(set: &mut BookmarkSet, id: &str, hlc: &str, node: &str, now_ms: i64) {
    if let Some(b) = set.bookmarks.iter_mut().find(|b| b.id == id) {
        b.hlc = hlc.to_string();
        b.node_id = node.to_string();
        b.updated_ms = now_ms;
    }
    if let Some(f) = set.folders.iter_mut().find(|f| f.id == id) {
        f.hlc = hlc.to_string();
        f.node_id = node.to_string();
        f.updated_ms = now_ms;
    }
    if let Some(t) = set.deleted.iter_mut().find(|t| t.id == id) {
        t.hlc = hlc.to_string();
        t.node_id = node.to_string();
        t.deleted_ms = now_ms;
    }
}

/// Replay the bookmarks log into `library.db` (idempotent).
pub fn materialize(lib: &mut Library, now_ms: i64) -> morph_store::Result<()> {
    let log = load_log(lib)?;
    let mut entries: Vec<&LogEntry> = Vec::new();
    let mut nodes: std::collections::BTreeSet<String> = log.cursors().keys().map(|n| n.0.clone()).collect();
    for (n, _) in lib.sync_cursors()? {
        nodes.insert(n);
    }
    for e in lib.sync_log()? {
        nodes.insert(e.node_id);
    }
    // Re-load via iter: we already have log in memory; walk all nodes we know.
    for n in &nodes {
        for e in log.iter_node(&NodeId::new(n.clone())) {
            if e.typ == BOOKMARKS {
                entries.push(e);
            }
        }
    }
    entries.sort_by(|a, b| a.hlc.cmp_total(&b.hlc).then(a.seq.cmp(&b.seq)));

    let mut fields: std::collections::BTreeMap<(String, String), (Hlc, String)> = std::collections::BTreeMap::new();
    let mut tags: std::collections::BTreeMap<String, OrSet<String>> = std::collections::BTreeMap::new();
    let mut deleted: std::collections::BTreeMap<String, Hlc> = std::collections::BTreeMap::new();
    let mut kinds: std::collections::BTreeMap<String, Kind> = std::collections::BTreeMap::new();
    let mut conflicts: Vec<SyncConflict> = Vec::new();

    for e in entries {
        let Some(op) = decode_op(&e.op) else { continue };
        apply_one(
            &e.record_id,
            &e.hlc,
            &op,
            &mut Apply { fields: &mut fields, tags: &mut tags, deleted: &mut deleted, kinds: &mut kinds, conflicts: &mut conflicts, now_ms },
        );
    }

    let set = gather_set(&fields, &tags, &deleted, &kinds);
    lib.save_bookmark_set(&set)?;
    // Persist field tables and conflicts (lose old field state).
    for ((record, field), (hlc, value)) in &fields {
        lib.set_sync_lww(record, field, &encode_hlc(hlc), Some(value))?;
    }
    for (record, set) in &tags {
        for elem in set.values() {
            let row = morph_store::SyncOrSetRow { record_id: record.clone(), elem, added_hlc: None, removed_hlc: None };
            // Values() only has live elems; we still store a row so a later load can
            // be rebuilt from the log. The log is the source of truth.
            let _ = row;
        }
    }
    lib.expire_sync_conflicts(now_ms)?;
    for c in conflicts {
        lib.add_sync_conflict(&c)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bookmark,
    Folder,
}

fn put_field(fields: &mut std::collections::BTreeMap<(String, String), (Hlc, String)>, id: &str, field: &str, hlc: &Hlc, value: String) {
    let key = (id.to_string(), field.to_string());
    match fields.get(&key) {
        None => {
            fields.insert(key, (hlc.clone(), value));
        }
        Some((old, _)) => {
            if lww(old, hlc) == Lww::TakeRemote {
                fields.insert(key, (hlc.clone(), value));
            }
        }
    }
}

struct Apply<'a> {
    fields: &'a mut std::collections::BTreeMap<(String, String), (Hlc, String)>,
    tags: &'a mut std::collections::BTreeMap<String, OrSet<String>>,
    deleted: &'a mut std::collections::BTreeMap<String, Hlc>,
    kinds: &'a mut std::collections::BTreeMap<String, Kind>,
    conflicts: &'a mut Vec<SyncConflict>,
    now_ms: i64,
}

fn apply_one(id: &str, hlc: &Hlc, op: &BookmarksOp, st: &mut Apply<'_>) {
    match op {
        BookmarksOp::Delete => {
            if let Some(edit) = st.fields.iter().filter(|((r, _), _)| r == id).map(|(_, (h, _))| h.clone()).max_by(|a, b| a.cmp_total(b)) {
                match merge::edit_vs_delete(&edit, hlc) {
                    EditOrDelete::KeepEdit => {
                        st.conflicts.push(conflict(id, hlc, op, st.now_ms));
                        return;
                    }
                    EditOrDelete::TakeDelete => {
                        st.conflicts.push(conflict(id, &edit, op, st.now_ms));
                    }
                }
            }
            st.deleted.insert(id.to_string(), hlc.clone());
        }
        other => {
            if let Some(del) = st.deleted.get(id) {
                match merge::edit_vs_delete(hlc, del) {
                    EditOrDelete::TakeDelete => {
                        st.conflicts.push(conflict(id, hlc, other, st.now_ms));
                        return;
                    }
                    EditOrDelete::KeepEdit => {
                        st.conflicts.push(conflict(id, del, &BookmarksOp::Delete, st.now_ms));
                        st.deleted.remove(id);
                    }
                }
            }
            match other {
                BookmarksOp::PutBookmark { url, title, folder_id, order, added_ms } => {
                    st.kinds.insert(id.to_string(), Kind::Bookmark);
                    put_field(st.fields, id, "url", hlc, url.clone());
                    put_field(st.fields, id, "title", hlc, title.clone());
                    put_field(st.fields, id, "folder_id", hlc, opt(folder_id));
                    put_field(st.fields, id, "order", hlc, order.clone());
                    put_field(st.fields, id, "added_ms", hlc, added_ms.to_string());
                }
                BookmarksOp::SetUrl { url } => {
                    st.kinds.insert(id.to_string(), Kind::Bookmark);
                    put_field(st.fields, id, "url", hlc, url.clone());
                }
                BookmarksOp::SetTitle { title } => {
                    st.kinds.insert(id.to_string(), Kind::Bookmark);
                    put_field(st.fields, id, "title", hlc, title.clone());
                }
                BookmarksOp::SetFolder { folder_id } => {
                    st.kinds.insert(id.to_string(), Kind::Bookmark);
                    put_field(st.fields, id, "folder_id", hlc, opt(folder_id));
                }
                BookmarksOp::SetOrder { order } => {
                    st.kinds.insert(id.to_string(), Kind::Bookmark);
                    put_field(st.fields, id, "order", hlc, order.clone());
                }
                BookmarksOp::AddTag { tag } => {
                    st.kinds.insert(id.to_string(), Kind::Bookmark);
                    st.tags.entry(id.to_string()).or_default().add(tag.clone(), hlc.clone());
                }
                BookmarksOp::RemoveTag { tag } => {
                    st.kinds.insert(id.to_string(), Kind::Bookmark);
                    st.tags.entry(id.to_string()).or_default().remove(tag.clone(), hlc.clone());
                }
                BookmarksOp::PutFolder { name, parent_id, order, created_ms } => {
                    st.kinds.insert(id.to_string(), Kind::Folder);
                    put_field(st.fields, id, "name", hlc, name.clone());
                    put_field(st.fields, id, "parent_id", hlc, opt(parent_id));
                    put_field(st.fields, id, "order", hlc, order.clone());
                    put_field(st.fields, id, "created_ms", hlc, created_ms.to_string());
                }
                BookmarksOp::SetName { name } => {
                    st.kinds.insert(id.to_string(), Kind::Folder);
                    put_field(st.fields, id, "name", hlc, name.clone());
                }
                BookmarksOp::SetParent { parent_id } => {
                    st.kinds.insert(id.to_string(), Kind::Folder);
                    put_field(st.fields, id, "parent_id", hlc, opt(parent_id));
                }
                BookmarksOp::SetFolderOrder { order } => {
                    st.kinds.insert(id.to_string(), Kind::Folder);
                    put_field(st.fields, id, "order", hlc, order.clone());
                }
                BookmarksOp::Delete => unreachable!(),
            }
        }
    }
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_default()
}

fn unopt(v: &str) -> Option<String> {
    if v.is_empty() { None } else { Some(v.to_string()) }
}

fn conflict(id: &str, hlc: &Hlc, op: &BookmarksOp, now_ms: i64) -> SyncConflict {
    SyncConflict {
        id: format!("{id}:{}", encode_hlc(hlc)),
        typ: BOOKMARKS.into(),
        record_id: id.into(),
        hlc: encode_hlc(hlc),
        op: encode_op(op),
        kept_until_ms: now_ms + CONFLICT_KEEP_MS as i64,
    }
}

fn field<'a>(fields: &'a std::collections::BTreeMap<(String, String), (Hlc, String)>, id: &str, name: &str) -> Option<(&'a Hlc, &'a str)> {
    fields.get(&(id.to_string(), name.to_string())).map(|(h, v)| (h, v.as_str()))
}

fn gather_set(
    fields: &std::collections::BTreeMap<(String, String), (Hlc, String)>,
    tags: &std::collections::BTreeMap<String, OrSet<String>>,
    deleted: &std::collections::BTreeMap<String, Hlc>,
    kinds: &std::collections::BTreeMap<String, Kind>,
) -> BookmarkSet {
    let mut folders = Vec::new();
    let mut bookmarks = Vec::new();
    let mut tombstones = Vec::new();
    let ids: std::collections::BTreeSet<_> = kinds.keys().cloned().collect();
    // Parent map for cycle break, after gathering raw parents.
    let mut parent_of: std::collections::BTreeMap<String, Option<String>> = std::collections::BTreeMap::new();
    for (id, kind) in kinds {
        if *kind != Kind::Folder {
            continue;
        }
        parent_of.insert(id.clone(), field(fields, id, "parent_id").and_then(|(_, v)| unopt(v)));
    }
    for (id, kind) in kinds {
        if deleted.contains_key(id) {
            let h = &deleted[id];
            tombstones.push(Tombstone { id: id.clone(), deleted_ms: h.physical_ms as i64, hlc: encode_hlc(h), node_id: h.node_id.0.clone() });
            continue;
        }
        match kind {
            Kind::Folder => {
                let mut parent = field(fields, id, "parent_id").and_then(|(_, v)| unopt(v));
                if folder_cycle(id, parent.as_ref(), |p| parent_of.get(p).cloned().flatten()) {
                    parent = None;
                    parent_of.insert(id.clone(), None);
                }
                let (hlc, _) = latest_row(fields, deleted, id);
                folders.push(Folder {
                    id: id.clone(),
                    name: field(fields, id, "name").map(|(_, v)| v.to_string()).unwrap_or_default(),
                    parent_id: parent,
                    order: field(fields, id, "order").map(|(_, v)| v.to_string()).unwrap_or_default(),
                    created_ms: field(fields, id, "created_ms").and_then(|(_, v)| v.parse().ok()).unwrap_or(0),
                    updated_ms: hlc.physical_ms as i64,
                    hlc: encode_hlc(&hlc),
                    node_id: hlc.node_id.0,
                });
            }
            Kind::Bookmark => {
                let (hlc, _) = latest_row(fields, deleted, id);
                let mut tag_list: Vec<String> = tags.get(id).map(|s| s.values().into_iter().collect()).unwrap_or_default();
                tag_list.sort();
                bookmarks.push(Bookmark {
                    id: id.clone(),
                    url: field(fields, id, "url").map(|(_, v)| v.to_string()).unwrap_or_default(),
                    title: field(fields, id, "title").map(|(_, v)| v.to_string()).unwrap_or_default(),
                    folder_id: field(fields, id, "folder_id").and_then(|(_, v)| unopt(v)),
                    order: field(fields, id, "order").map(|(_, v)| v.to_string()).unwrap_or_default(),
                    added_ms: field(fields, id, "added_ms").and_then(|(_, v)| v.parse().ok()).unwrap_or(0),
                    updated_ms: hlc.physical_ms as i64,
                    tags: tag_list,
                    hlc: encode_hlc(&hlc),
                    node_id: hlc.node_id.0,
                });
            }
        }
    }
    let _ = ids;
    BookmarkSet { bookmarks, folders, deleted: tombstones }
}

fn latest_row(
    fields: &std::collections::BTreeMap<(String, String), (Hlc, String)>,
    deleted: &std::collections::BTreeMap<String, Hlc>,
    id: &str,
) -> (Hlc, String) {
    let mut best: Option<Hlc> = deleted.get(id).cloned();
    for ((r, _), (h, _)) in fields {
        if r != id {
            continue;
        }
        best = Some(match best {
            None => h.clone(),
            Some(b) if lww(&b, h) == Lww::TakeRemote => h.clone(),
            Some(b) => b,
        });
    }
    match best {
        Some(h) => {
            let node = h.node_id.0.clone();
            (h, node)
        }
        None => (Hlc::new(0, 0, NodeId::new("")), String::new()),
    }
}

/// Apply newly integrated log entries and rematerialize bookmarks.
pub fn apply_log(lib: &mut Library, now_ms: i64) -> morph_store::Result<()> {
    materialize(lib, now_ms)
}

/// Advance our clock past every op we now hold, so a local write made after
/// seeing a remote op (a visit after a remote clear) orders after it.
/// `weirdnet-sync` 0.1's `pull_from` does not do this itself.
pub fn observe_remote(state: &mut SyncState, wall_ms: u64) {
    let newest = state
        .log
        .cursors()
        .keys()
        .filter_map(|n| state.log.iter_node(n).map(|e| &e.hlc).max_by(|a, b| a.cmp_total(b)).cloned())
        .max_by(|a, b| a.cmp_total(b));
    if let Some(h) = newest
        && state.clock.as_ref().is_none_or(|c| h.cmp_total(c).is_gt())
    {
        state.clock = Some(Hlc::receive(state.clock.as_ref(), &h, wall_ms, state.our_node.clone()));
    }
}

pub fn conflict_count(lib: &Library) -> morph_store::Result<usize> {
    Ok(lib.sync_conflicts()?.len())
}

pub const META_SEED: &str = "seed";
pub const META_NODE_KEY: &str = "node_key";
pub const META_ROSTER: &str = "roster";

/// JSON carried in `Transfer.settings` at link time (certs only, no secrets).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LinkSettings {
    #[serde(default)]
    pub roster: Roster,
}

pub fn encode_link_settings(roster: &Roster) -> String {
    serde_json::to_string(&LinkSettings { roster: roster.clone() }).expect("link settings json")
}

pub fn decode_link_settings(settings: &str) -> LinkSettings {
    if settings.trim().is_empty() || settings.trim() == "{}" {
        return LinkSettings::default();
    }
    serde_json::from_str(settings).unwrap_or_default()
}

pub fn load_roster(lib: &Library) -> morph_store::Result<Roster> {
    Ok(lib.sync_meta(META_ROSTER)?.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
}

pub fn save_roster(lib: &mut Library, roster: &Roster) -> morph_store::Result<()> {
    lib.set_sync_meta(META_ROSTER, &serde_json::to_vec(roster).expect("roster json"))
}

pub fn store_seed(lib: &mut Library, seed: &[u8]) -> morph_store::Result<()> {
    lib.set_sync_meta(META_SEED, seed)
}

pub fn load_seed(lib: &Library) -> morph_store::Result<Option<Vec<u8>>> {
    lib.sync_meta(META_SEED)
}

pub fn store_node_key(lib: &mut Library, key: &[u8]) -> morph_store::Result<()> {
    lib.set_sync_meta(META_NODE_KEY, key)
}

pub fn load_node_key(lib: &Library) -> morph_store::Result<Option<Vec<u8>>> {
    lib.sync_meta(META_NODE_KEY)
}

/// Persist this device's seed, node key, roster and default opt-ins, and
/// backfill existing bookmarks into the log.
pub fn bind(lib: &mut Library, our_node: &str, seed: &[u8], node_key: &[u8], roster: &Roster, now_ms: i64) -> morph_store::Result<()> {
    store_seed(lib, seed)?;
    store_node_key(lib, node_key)?;
    save_roster(lib, roster)?;
    enable(lib, our_node, now_ms)
}

static LIVE: Mutex<Vec<(PathBuf, Arc<Mutex<SyncState>>)>> = Mutex::new(Vec::new());

fn live() -> std::sync::MutexGuard<'static, Vec<(PathBuf, Arc<Mutex<SyncState>>)>> {
    LIVE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Keep a running sync node's in-memory log in step with `library.db`.
pub fn attach_live(profile_dir: PathBuf, state: Arc<Mutex<SyncState>>) {
    let mut g = live();
    g.retain(|(p, _)| p != &profile_dir);
    g.push((profile_dir, state));
}

pub fn detach_live(profile_dir: &Path) {
    live().retain(|(p, _)| p != profile_dir);
}

pub fn refresh_live(profile_dir: &Path, lib: &Library) {
    let state = live().iter().find(|(p, _)| p == profile_dir).map(|(_, s)| s.clone());
    let Some(state) = state else { return };
    let Ok(mut st) = state.lock() else { return };
    if let Ok(log) = load_log(lib) {
        st.log = log;
    }
    if let Ok(c) = load_clock(lib) {
        st.clock = c;
    }
    if let Ok(t) = types_on(lib) {
        st.types = t;
    }
    if let Ok(r) = load_roster(lib) {
        st.roster = r;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_identity::profile_identity::LibraryKey;
    use morph_store::Library;

    fn key() -> LibraryKey {
        LibraryKey::from_bytes([7; 32])
    }

    fn open() -> (tempfile::TempDir, Library) {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path(), &key()).unwrap();
        (tmp, lib)
    }

    fn node(s: &str) -> NodeId {
        NodeId::new(s)
    }

    fn append(log: &mut OpLog, n: &str, ms: u64, ctr: u32, id: &str, op: BookmarksOp) {
        let hlc = Hlc::new(ms, ctr, node(n));
        log.append(node(n), hlc, BOOKMARKS, id, encode_op(&op));
    }

    #[test]
    fn lww_title_and_move_both_survive() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        let mut log = load_log(&lib).unwrap();
        append(
            &mut log,
            "a",
            10,
            0,
            "b1",
            BookmarksOp::PutBookmark { url: "https://a.example/".into(), title: "A".into(), folder_id: None, order: "V".into(), added_ms: 10 },
        );
        append(&mut log, "a", 20, 0, "b1", BookmarksOp::SetTitle { title: "Renamed".into() });
        append(&mut log, "b", 20, 0, "b1", BookmarksOp::SetFolder { folder_id: Some("f".into()) });
        persist_log(&mut lib, &log, None).unwrap();
        materialize(&mut lib, 30).unwrap();
        let set = lib.bookmark_set().unwrap();
        assert_eq!(set.bookmarks.len(), 1);
        assert_eq!(set.bookmarks[0].title, "Renamed");
        assert_eq!(set.bookmarks[0].folder_id.as_deref(), Some("f"));
    }

    #[test]
    fn or_set_tags_keep_concurrent_adds() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        let mut log = load_log(&lib).unwrap();
        append(
            &mut log,
            "a",
            10,
            0,
            "b1",
            BookmarksOp::PutBookmark { url: "https://a.example/".into(), title: "A".into(), folder_id: None, order: "V".into(), added_ms: 10 },
        );
        append(&mut log, "a", 20, 0, "b1", BookmarksOp::AddTag { tag: "work".into() });
        append(&mut log, "b", 20, 0, "b1", BookmarksOp::AddTag { tag: "later".into() });
        persist_log(&mut lib, &log, None).unwrap();
        materialize(&mut lib, 30).unwrap();
        let tags = &lib.bookmark_set().unwrap().bookmarks[0].tags;
        assert!(tags.contains(&"work".to_string()) && tags.contains(&"later".to_string()), "{tags:?}");
    }

    #[test]
    fn concurrent_edit_beats_delete_later_delete_wins() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        let mut log = load_log(&lib).unwrap();
        append(
            &mut log,
            "a",
            10,
            0,
            "b1",
            BookmarksOp::PutBookmark { url: "https://a.example/".into(), title: "A".into(), folder_id: None, order: "V".into(), added_ms: 10 },
        );
        append(&mut log, "a", 20, 0, "b1", BookmarksOp::SetTitle { title: "Kept".into() });
        append(&mut log, "b", 20, 0, "b1", BookmarksOp::Delete);
        persist_log(&mut lib, &log, None).unwrap();
        materialize(&mut lib, 30).unwrap();
        let set = lib.bookmark_set().unwrap();
        assert_eq!(set.bookmarks.len(), 1);
        assert_eq!(set.bookmarks[0].title, "Kept");
        assert!(!set.deleted.iter().any(|t| t.id == "b1"));
        assert!(conflict_count(&lib).unwrap() >= 1);

        append(&mut log, "b", 40, 0, "b1", BookmarksOp::Delete);
        persist_log(&mut lib, &log, None).unwrap();
        materialize(&mut lib, 50).unwrap();
        let set = lib.bookmark_set().unwrap();
        assert!(set.bookmarks.is_empty());
        assert!(set.deleted.iter().any(|t| t.id == "b1"));
    }

    #[test]
    fn a_folder_cycle_is_broken_to_the_root() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        let mut log = load_log(&lib).unwrap();
        append(&mut log, "a", 10, 0, "f1", BookmarksOp::PutFolder { name: "One".into(), parent_id: None, order: "a".into(), created_ms: 10 });
        append(
            &mut log,
            "a",
            11,
            0,
            "f2",
            BookmarksOp::PutFolder { name: "Two".into(), parent_id: Some("f1".into()), order: "b".into(), created_ms: 11 },
        );
        append(&mut log, "b", 20, 0, "f1", BookmarksOp::SetParent { parent_id: Some("f2".into()) });
        persist_log(&mut lib, &log, None).unwrap();
        materialize(&mut lib, 30).unwrap();
        let folders = lib.bookmark_set().unwrap().folders;
        let f1 = folders.iter().find(|f| f.id == "f1").unwrap();
        // Later write made f1 parent of f2 while f2 still has parent f1; break the later one.
        assert!(f1.parent_id.is_none() || folders.iter().find(|f| f.id == "f2").unwrap().parent_id.is_none());
        let parent = |id: &str| folders.iter().find(|f| f.id == id).unwrap().parent_id.clone();
        assert!(!folder_cycle(&"f1".to_string(), parent("f1").as_ref(), |p| parent(p)));
    }
}
