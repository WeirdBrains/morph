//! Morph history records on top of `weirdnet-sync`.
//!
//! Visits are immutable records keyed by their stable id (a reload or title
//! refresh rewrites the same id, last writer wins). Delete-by-URL and
//! clear-all are ops too: a visit whose HLC is before a matching delete (or
//! the latest clear) stays gone; a later visit survives. `MAX_HISTORY` is a
//! local cap applied after merge and is never synced as a delete.
//!
//! History ops share the node's log stream with bookmarks and settings;
//! `weirdnet-sync` 0.2 keeps a pull position per type. Ops that 0.1-era
//! builds wrote on a separate `<node>/history` stream still merge.
use std::collections::{BTreeMap, BTreeSet};

use morph_store::{HistoryEntry, Library, Tombstone};
use serde::{Deserialize, Serialize};
use weirdnet_sync::hlc::Hlc;
use weirdnet_sync::log::LogEntry;
use weirdnet_sync::{NodeId, SyncType};

use crate::library::{MAX_HISTORY, same_page};
use crate::sync::{decode_hlc, encode_hlc, load_clock, load_log, node_id, persist_log, types_on};

pub const HISTORY: &str = SyncType::HISTORY;

/// Record id of the clear-all op (one record; the latest clear wins).
const CLEAR_ID: &str = "clear";

/// One history-log mutation. `record_id` lives on the log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum HistoryOp {
    Visit { url: String, title: String, visited_ms: i64 },
    DeleteUrl { url: String },
    Clear,
}

fn encode(op: &HistoryOp) -> Vec<u8> {
    serde_json::to_vec(op).expect("history op json")
}

fn decode(bytes: &[u8]) -> Option<HistoryOp> {
    serde_json::from_slice(bytes).ok()
}

pub fn history_on(lib: &Library) -> bool {
    types_on(lib).ok().is_some_and(|t| t.iter().any(|x| x == HISTORY))
}

fn url_key(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// Append one op to our history stream. Returns the stamp (encoded HLC and
/// stream id) for the row, or `None` when history sync is off.
fn record(lib: &mut Library, record_id: &str, op: &HistoryOp, now_ms: i64) -> morph_store::Result<Option<(String, String)>> {
    let Some(our) = node_id(lib)? else { return Ok(None) };
    if !history_on(lib) {
        return Ok(None);
    }
    let node = NodeId::new(&our);
    let clock = load_clock(lib)?;
    let mut log = load_log(lib)?;
    let hlc = Hlc::tick(clock.as_ref(), now_ms.max(0) as u64, node.clone());
    log.append(node.clone(), hlc.clone(), HISTORY, record_id, encode(op));
    persist_log(lib, &log, Some(&hlc))?;
    Ok(Some((encode_hlc(&hlc), node.0)))
}

/// Log a local visit (new, or a refresh of the same id) and stamp `entry`.
pub fn local_visit(lib: &mut Library, entry: &mut HistoryEntry, now_ms: i64) -> morph_store::Result<()> {
    if entry.id.is_empty() {
        entry.id = morph_store::new_history_id();
    }
    let op = HistoryOp::Visit { url: entry.url.clone(), title: entry.title.clone(), visited_ms: entry.visited_ms };
    if let Some((hlc, node)) = record(lib, &entry.id.clone(), &op, now_ms)? {
        entry.hlc = hlc;
        entry.node_id = node;
    }
    Ok(())
}

/// Log a local delete of every visit to `url`.
pub fn local_delete_url(lib: &mut Library, url: &str, now_ms: i64) -> morph_store::Result<()> {
    record(lib, &format!("url:{}", url_key(url)), &HistoryOp::DeleteUrl { url: url.trim().to_string() }, now_ms).map(|_| ())
}

/// Log a local clear-all.
pub fn local_clear(lib: &mut Library, now_ms: i64) -> morph_store::Result<()> {
    record(lib, CLEAR_ID, &HistoryOp::Clear, now_ms).map(|_| ())
}

fn history_entries(log: &weirdnet_sync::OpLog, lib: &Library) -> morph_store::Result<Vec<LogEntry>> {
    let mut nodes: BTreeSet<String> = log.cursors().keys().map(|n| n.0.clone()).collect();
    for (n, _) in lib.sync_cursors()? {
        nodes.insert(n);
    }
    for e in lib.sync_log()? {
        nodes.insert(e.node_id);
    }
    let mut out = Vec::new();
    for n in &nodes {
        out.extend(log.iter_node(&NodeId::new(n.clone())).filter(|e| e.typ == HISTORY).cloned());
    }
    Ok(out)
}

/// Put local visits that are not in the log yet into our stream (first
/// enable, or history sync turned on later), so they go out on the next pull.
pub fn backfill(lib: &mut Library, now_ms: i64) -> morph_store::Result<()> {
    let Some(our) = node_id(lib)? else { return Ok(()) };
    if !history_on(lib) {
        return Ok(());
    }
    let mut log = load_log(lib)?;
    let logged: BTreeSet<String> = history_entries(&log, lib)?.into_iter().map(|e| e.record_id).collect();
    let mut missing: Vec<HistoryEntry> = lib.history()?.into_iter().filter(|e| !e.id.is_empty() && !logged.contains(&e.id)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    missing.reverse();
    let node = NodeId::new(&our);
    let mut clock = load_clock(lib)?;
    for e in &missing {
        let wall = if e.visited_ms > 0 { e.visited_ms } else { now_ms };
        let hlc = Hlc::tick(clock.as_ref(), wall.max(0) as u64, node.clone());
        clock = Some(hlc.clone());
        let op = HistoryOp::Visit { url: e.url.clone(), title: e.title.clone(), visited_ms: e.visited_ms };
        log.append(node.clone(), hlc, HISTORY, e.id.clone(), encode(&op));
    }
    persist_log(lib, &log, clock.as_ref())
}

/// Rebuild `history` from every history op in the log (idempotent). No-op
/// when history sync is off, so an opted-out library keeps its rows.
pub fn materialize(lib: &mut Library, now_ms: i64) -> morph_store::Result<()> {
    if !history_on(lib) || node_id(lib)?.is_none() {
        return Ok(());
    }
    backfill(lib, now_ms)?;
    let log = load_log(lib)?;
    let mut entries = history_entries(&log, lib)?;
    entries.sort_by(|a, b| a.hlc.cmp_total(&b.hlc).then(a.seq.cmp(&b.seq)));

    let mut visits: BTreeMap<String, (Hlc, String, String, i64)> = BTreeMap::new();
    let mut url_deletes: BTreeMap<String, (Hlc, String)> = BTreeMap::new();
    let mut clear: Option<Hlc> = None;
    for e in &entries {
        let Some(op) = decode(&e.op) else { continue };
        match op {
            HistoryOp::Visit { url, title, visited_ms } => {
                // Sorted by HLC, so a later write of the same id replaces it.
                visits.insert(e.record_id.clone(), (e.hlc.clone(), url, title, visited_ms));
            }
            HistoryOp::DeleteUrl { url } => {
                url_deletes.insert(url_key(&url), (e.hlc.clone(), url));
            }
            HistoryOp::Clear => clear = Some(e.hlc.clone()),
        }
    }

    let mut live = Vec::new();
    let mut stones = Vec::new();
    for (id, (hlc, url, title, visited_ms)) in visits {
        let cleared = clear.as_ref().filter(|c| c.cmp_total(&hlc).is_gt());
        let by_url =
            url_deletes.values().filter(|(d, u)| d.cmp_total(&hlc).is_gt() && same_page(u, &url)).map(|(d, _)| d).max_by(|a, b| a.cmp_total(b));
        let killer = match (cleared, by_url) {
            (Some(a), Some(b)) => Some(if a.cmp_total(b).is_gt() { a } else { b }),
            (a, b) => a.or(b),
        };
        if let Some(k) = killer {
            stones.push(Tombstone { id, deleted_ms: k.physical_ms as i64, hlc: encode_hlc(k), node_id: k.node_id.0.clone() });
            continue;
        }
        live.push(HistoryEntry { id, url, title, visited_ms, hlc: encode_hlc(&hlc), node_id: hlc.node_id.0.clone() });
    }
    live.sort_by(|a, b| {
        a.visited_ms.cmp(&b.visited_ms).then_with(|| match (decode_hlc(&a.hlc), decode_hlc(&b.hlc)) {
            (Some(x), Some(y)) => x.cmp_total(&y),
            _ => a.id.cmp(&b.id),
        })
    });
    if live.len() > MAX_HISTORY {
        live.drain(..live.len() - MAX_HISTORY);
    }
    lib.replace_history(&live, &stones)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{BOOKMARKS, enable, set_types};
    use morph_identity::profile_identity::LibraryKey;

    fn open() -> (tempfile::TempDir, Library) {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path(), &LibraryKey::from_bytes([7; 32])).unwrap();
        (tmp, lib)
    }

    /// A remote device's op on its own history stream.
    fn remote(lib: &mut Library, n: &str, ms: u64, id: &str, op: HistoryOp) {
        let mut log = load_log(lib).unwrap();
        let node = NodeId::new(n);
        log.append(node.clone(), Hlc::new(ms, 0, node), HISTORY, id, encode(&op));
        persist_log(lib, &log, None).unwrap();
    }

    fn visit(url: &str, ms: i64) -> HistoryOp {
        HistoryOp::Visit { url: url.into(), title: url.into(), visited_ms: ms }
    }

    fn urls(lib: &Library) -> Vec<String> {
        lib.history().unwrap().into_iter().map(|e| e.url).collect()
    }

    #[test]
    fn local_rows_backfill_and_remote_visits_merge_newest_first() {
        let (_t, mut lib) = open();
        lib.append_visits(&[HistoryEntry { url: "https://local.example/".into(), visited_ms: 5, ..Default::default() }], 0).unwrap();
        enable(&mut lib, "a", 10).unwrap();
        assert!(history_on(&lib), "history is a default opt-in");
        let ours: Vec<_> = load_log(&lib).unwrap().iter_node(&NodeId::new("a")).cloned().collect();
        assert_eq!(ours.len(), 1, "the existing visit is backfilled on our history stream");
        remote(&mut lib, "b", 20, "h-b1", visit("https://remote.example/", 20));
        materialize(&mut lib, 30).unwrap();
        assert_eq!(urls(&lib), ["https://remote.example/", "https://local.example/"]);
        // Idempotent.
        materialize(&mut lib, 31).unwrap();
        assert_eq!(urls(&lib).len(), 2);
    }

    #[test]
    fn a_refresh_of_the_same_visit_is_last_writer_wins() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        remote(&mut lib, "b", 20, "h-1", visit("https://x.example/", 20));
        remote(&mut lib, "b", 25, "h-1", HistoryOp::Visit { url: "https://x.example/".into(), title: "Later".into(), visited_ms: 25 });
        materialize(&mut lib, 30).unwrap();
        let h = lib.history().unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!((h[0].title.as_str(), h[0].visited_ms), ("Later", 25));
    }

    #[test]
    fn delete_by_url_removes_earlier_visits_but_keeps_later_ones() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        remote(&mut lib, "b", 20, "h-1", visit("https://x.example/", 20));
        remote(&mut lib, "b", 21, "h-2", visit("https://y.example/", 21));
        remote(&mut lib, "c", 30, "url:https://x.example", HistoryOp::DeleteUrl { url: "https://x.example".into() });
        remote(&mut lib, "b", 40, "h-3", visit("https://x.example/", 40));
        materialize(&mut lib, 50).unwrap();
        let h = lib.history().unwrap();
        assert_eq!(h.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["h-3", "h-2"]);
        let stones = lib.history_tombstones().unwrap();
        assert!(stones.iter().any(|t| t.id == "h-1" && !t.hlc.is_empty()), "{stones:?}");
    }

    #[test]
    fn clear_all_drops_everything_before_it_and_a_later_visit_survives() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        remote(&mut lib, "b", 20, "h-1", visit("https://x.example/", 20));
        remote(&mut lib, "b", 21, "h-2", visit("https://y.example/", 21));
        remote(&mut lib, "c", 30, CLEAR_ID, HistoryOp::Clear);
        remote(&mut lib, "b", 40, "h-3", visit("https://z.example/", 40));
        materialize(&mut lib, 50).unwrap();
        assert_eq!(urls(&lib), ["https://z.example/"]);
    }

    #[test]
    fn local_ops_are_stamped_and_logged_only_when_history_is_on() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        let mut e = HistoryEntry { url: "https://x.example/".into(), visited_ms: 11, ..Default::default() };
        local_visit(&mut lib, &mut e, 11).unwrap();
        assert!(e.id.starts_with("h-") && !e.hlc.is_empty() && e.node_id == "a");
        local_delete_url(&mut lib, "https://x.example", 12).unwrap();
        local_clear(&mut lib, 13).unwrap();
        let log = load_log(&lib).unwrap();
        assert_eq!(log.iter_node(&NodeId::new("a")).filter(|x| x.typ == HISTORY).count(), 3, "history shares the node's stream");

        set_types(&mut lib, &[BOOKMARKS.to_string()]).unwrap();
        let mut off = HistoryEntry { url: "https://y.example/".into(), visited_ms: 14, ..Default::default() };
        local_visit(&mut lib, &mut off, 14).unwrap();
        assert!(off.hlc.is_empty());
        lib.append_visits(&[off], 0).unwrap();
        materialize(&mut lib, 15).unwrap();
        assert_eq!(urls(&lib), ["https://y.example/"], "an opted-out library keeps its rows");
    }

    #[test]
    fn a_bookmarks_only_library_gets_history_once() {
        let (_t, mut lib) = open();
        set_types(&mut lib, &[BOOKMARKS.to_string()]).unwrap();
        enable(&mut lib, "a", 10).unwrap();
        assert!(history_on(&lib));
        set_types(&mut lib, &[BOOKMARKS.to_string()]).unwrap();
        enable(&mut lib, "a", 11).unwrap();
        assert!(!history_on(&lib), "a later opt-out sticks");
    }

    #[test]
    fn the_cap_applies_after_merge_without_tombstones() {
        let (_t, mut lib) = open();
        enable(&mut lib, "a", 10).unwrap();
        let mut log = load_log(&lib).unwrap();
        let node = NodeId::new("b");
        for i in 0..(MAX_HISTORY as u64 + 3) {
            let op = visit(&format!("https://x.example/{i}"), i as i64 + 1);
            log.append(node.clone(), Hlc::new(i + 1, 0, node.clone()), HISTORY, format!("h-{i}"), encode(&op));
        }
        persist_log(&mut lib, &log, None).unwrap();
        materialize(&mut lib, 1).unwrap();
        let h = lib.history().unwrap();
        assert_eq!(h.len(), MAX_HISTORY);
        assert_eq!(h.last().unwrap().id, "h-3", "the oldest go first");
        assert!(lib.history_tombstones().unwrap().is_empty());
    }
}
