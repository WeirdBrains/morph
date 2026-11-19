//! Morph's companion records on top of `weirdnet-sync`: companions, grants, chats, MCP
//! configs and journals follow the user across devices.
//!
//! Record types (each its own sync type, so each has its own key and pull position):
//!
//! | type               | records                                                         |
//! |--------------------|-----------------------------------------------------------------|
//! | `morph-companions` | `c/<id>` companion (OR-set), `default` and `settings` (LWW)     |
//! | `morph-grants`     | `site/<agent>\u{1f}<site>` level (LWW), `allow/<key>` standing allow and `ext/<agent>` external approval (OR-set) |
//! | `morph-chats`      | `t/<id>` chat turn (OR-set; long answers clipped)               |
//! | `morph-mcp`        | `s/<id>` server config without secret values (OR-set)           |
//! | `morph-journal`    | `j/<id>` journal and `e/<id>` entry (OR-set)                    |
//!
//! The files the app reads stay where they were (the companions directory, through
//! [`AiStore`]). Like settings, sync works by difference: [`backfill`] logs what changed in
//! the files since the last pass, and [`materialize`] writes the merged result back. The last
//! merged state is kept in the encrypted library (`sync_meta`), never in the files.
//!
//! Revoked devices: everything a revoked device granted is void. Its site grants above Off, its
//! standing allows and its external approvals are dropped (its removals and Off levels stand),
//! and Node companions that ran on it leave the roster along with their grants. This is
//! checked against the sync roster on every pass, and right after a revoke on this device.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use morph_ai::{AgentSettings, AiStore, ChatLog, FileStore, Journals, McpConfigs, Roster as Companions, StoreError};
use morph_store::Library;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weirdnet_agent::{AgentKind, CompanionScope, Policy};
use weirdnet_sync::NodeId;
use weirdnet_sync::hlc::Hlc;
use weirdnet_sync::merge::OrSet;

use crate::sync::{load_clock, load_log, load_roster, node_id, persist_log, types_on};

pub const COMPANIONS: &str = "morph-companions";
pub const GRANTS: &str = "morph-grants";
pub const CHATS: &str = "morph-chats";
pub const MCP: &str = "morph-mcp";
pub const JOURNAL: &str = "morph-journal";
pub const TYPES: &[&str] = &[COMPANIONS, GRANTS, CHATS, MCP, JOURNAL];

const META_SNAPSHOT: &str = "companion-sync";
const SEP: char = '\u{1f}';

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum RecOp {
    Put { value: Value },
    Remove,
}

/// Every record by type and id, plus the local ids given to synced standing allows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct State {
    recs: BTreeMap<String, BTreeMap<String, Value>>,
    #[serde(default)]
    allow_ids: BTreeMap<String, u64>,
    #[serde(default)]
    next_allow_id: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum CompanionSyncError {
    #[error(transparent)]
    Store(#[from] morph_store::StoreError),
    #[error(transparent)]
    Files(#[from] StoreError),
}

pub type Result<T> = std::result::Result<T, CompanionSyncError>;

/// The companions directory of a profile, where the panel and Settings read these files.
pub fn store(profile_dir: &Path) -> FileStore {
    FileStore::new(profile_dir.join("companions"))
}

fn on(lib: &Library) -> Vec<&'static str> {
    let t = types_on(lib).unwrap_or_default();
    TYPES.iter().copied().filter(|x| t.iter().any(|y| y == x)).collect()
}

fn is_collection(record: &str) -> bool {
    !(record == "default" || record == "settings" || record.starts_with("site/"))
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("record json")
}

/// Records from the files. Allows get a sync key: the one they were synced under, else
/// `<our node>/<local id>`.
fn read_files(store: &dyn AiStore, our: &str, base: &State) -> Result<State> {
    let mut st = State { allow_ids: BTreeMap::new(), next_allow_id: base.next_allow_id, recs: BTreeMap::new() };
    let mut c = BTreeMap::new();
    if let Some(r) = store.load_roster()? {
        for p in r.companions() {
            c.insert(format!("c/{}", p.id()), to_value(p));
        }
        if let Some(d) = to_value(&r).get("default").filter(|d| !d.is_null()) {
            c.insert("default".into(), d.clone());
        }
    }
    if let Some(s) = store.load_settings()? {
        c.insert("settings".into(), to_value(&s));
    }
    st.recs.insert(COMPANIONS.into(), c);

    let mut g = BTreeMap::new();
    if let Some(p) = store.load_policy()? {
        let v = to_value(&p);
        if let Some(grants) = v.get("grants").and_then(Value::as_object) {
            for (agent, sites) in grants {
                for (site, level) in sites.as_object().into_iter().flatten() {
                    g.insert(format!("site/{agent}{SEP}{site}"), level.clone());
                }
            }
        }
        let by_local: BTreeMap<u64, &String> = base.allow_ids.iter().map(|(k, id)| (*id, k)).collect();
        for a in p.allows() {
            let key = by_local.get(&a.id).map(|k| (*k).clone()).unwrap_or_else(|| format!("{our}/{}", a.id));
            st.allow_ids.insert(key.clone(), a.id);
            let mut val = to_value(a);
            val.as_object_mut().expect("allow object").remove("id");
            g.insert(format!("allow/{key}"), val);
        }
        for agent in v.get("approved_external").and_then(Value::as_array).into_iter().flatten() {
            if let Some(a) = agent.as_str() {
                g.insert(format!("ext/{a}"), Value::Bool(true));
            }
        }
        st.next_allow_id = st.next_allow_id.max(v.get("next_allow_id").and_then(Value::as_u64).unwrap_or(0));
    }
    st.recs.insert(GRANTS.into(), g);

    let mut t = BTreeMap::new();
    for turn in store.load_chats()?.unwrap_or_default().turns {
        t.insert(format!("t/{}", turn.id), to_value(&turn.for_sync()));
    }
    st.recs.insert(CHATS.into(), t);

    let mut m = BTreeMap::new();
    for s in store.load_mcp()?.unwrap_or_default().for_sync().servers {
        m.insert(format!("s/{}", s.id), to_value(&s));
    }
    st.recs.insert(MCP.into(), m);

    let mut j = BTreeMap::new();
    let journals = store.load_journals()?.unwrap_or_default();
    for x in &journals.journals {
        j.insert(format!("j/{}", x.id), to_value(x));
    }
    for e in &journals.entries {
        j.insert(format!("e/{}", e.id), to_value(e));
    }
    st.recs.insert(JOURNAL.into(), j);
    Ok(st)
}

/// What a fresh install has before the user touched anything: stamped at time zero so it
/// never overrides a linked device's choices.
fn first_run_value(typ: &str, record: &str, value: &Value) -> bool {
    if typ != COMPANIONS {
        return false;
    }
    match record {
        "c/local" => [morph_ai::ModelChoice::Gemma4E4B, morph_ai::ModelChoice::Gemma4E2B]
            .into_iter()
            .any(|m| Companions::first_run(m).companions().first().map(to_value).as_ref() == Some(value)),
        "settings" => *value == to_value(&AgentSettings::default()),
        _ => false,
    }
}

fn load_snapshot(lib: &Library) -> Result<Option<State>> {
    Ok(lib.sync_meta(META_SNAPSHOT)?.and_then(|b| serde_json::from_slice(&b).ok()))
}

fn save_snapshot(lib: &mut Library, st: &State) -> Result<()> {
    lib.set_sync_meta(META_SNAPSHOT, &serde_json::to_vec(st).expect("snapshot json"))?;
    Ok(())
}

/// Log what changed in the files since the last pass (all of it on the first pass). Run before
/// a pull so other devices can fetch it.
pub fn backfill(lib: &mut Library, store: &dyn AiStore, now_ms: i64) -> Result<()> {
    let types = on(lib);
    let Some(our) = node_id(lib)? else { return Ok(()) };
    if types.is_empty() {
        return Ok(());
    }
    let base = load_snapshot(lib)?;
    let first = base.is_none();
    let base = base.unwrap_or_default();
    let mut file = read_files(store, &our, &base)?;
    let node = NodeId::new(&our);
    let mut clock = load_clock(lib)?;
    let mut log = load_log(lib)?;
    let mut wrote = false;
    for typ in &types {
        let empty = BTreeMap::new();
        let before = base.recs.get(*typ).unwrap_or(&empty);
        let now = file.recs.get(*typ).cloned().unwrap_or_default();
        let mut ops: Vec<(String, RecOp, bool)> = Vec::new();
        for (id, v) in &now {
            if before.get(id) != Some(v) {
                ops.push((id.clone(), RecOp::Put { value: v.clone() }, first && first_run_value(typ, id, v)));
            }
        }
        for id in before.keys().filter(|k| !now.contains_key(*k)) {
            ops.push((id.clone(), RecOp::Remove, false));
        }
        for (id, op, at_zero) in ops {
            let hlc = if at_zero {
                Hlc::new(0, 0, node.clone())
            } else {
                let h = Hlc::tick(clock.as_ref(), now_ms.max(0) as u64, node.clone());
                clock = Some(h.clone());
                h
            };
            log.append(node.clone(), hlc, *typ, id, serde_json::to_vec(&op).expect("op json"));
            wrote = true;
        }
    }
    if wrote {
        persist_log(lib, &log, clock.as_ref())?;
    }
    // Types that are off keep their last merged records, so turning one on later diffs correctly.
    for (typ, recs) in &base.recs {
        if !types.contains(&typ.as_str()) {
            file.recs.insert(typ.clone(), recs.clone());
        }
    }
    for (k, id) in &base.allow_ids {
        file.allow_ids.entry(k.clone()).or_insert(*id);
    }
    save_snapshot(lib, &file)
}

/// Grant-widening ops from a revoked device are void; its removals and Off levels stand.
fn voided(typ: &str, record: &str, op: &RecOp) -> bool {
    if typ != GRANTS {
        return false;
    }
    match op {
        RecOp::Put { value } if record.starts_with("site/") => value.as_str() != Some("off"),
        RecOp::Put { .. } => true,
        // A removed site level means "back to Ask", which loosens an Off.
        RecOp::Remove => record.starts_with("site/"),
    }
}

/// Merge one type's ops from every node: OR-sets for collections, LWW for single values.
fn merge_type(lib: &Library, log: &weirdnet_sync::log::OpLog, typ: &str, revoked: &BTreeSet<String>) -> Result<BTreeMap<String, Value>> {
    let mut nodes: BTreeSet<String> = log.cursors().keys().map(|n| n.0.clone()).collect();
    for e in lib.sync_log()? {
        nodes.insert(e.node_id);
    }
    let mut set: OrSet<String> = OrSet::new();
    let mut latest: BTreeMap<String, (Hlc, Option<Value>)> = BTreeMap::new();
    for n in nodes {
        let gone = revoked.contains(&n);
        for e in log.iter_node(&NodeId::new(n.clone())).filter(|e| e.typ == typ) {
            let Ok(op) = serde_json::from_slice::<RecOp>(&e.op) else { continue };
            if gone && voided(typ, &e.record_id, &op) {
                continue;
            }
            let value = match &op {
                RecOp::Put { value } => Some(value.clone()),
                RecOp::Remove => None,
            };
            if is_collection(&e.record_id) {
                match value {
                    Some(v) => {
                        set.add(e.record_id.clone(), e.hlc.clone());
                        let newer = latest.get(&e.record_id).is_none_or(|(h, _)| e.hlc.cmp_total(h).is_gt());
                        if newer {
                            latest.insert(e.record_id.clone(), (e.hlc.clone(), Some(v)));
                        }
                    }
                    None => set.remove(e.record_id.clone(), e.hlc.clone()),
                }
            } else {
                let newer = latest.get(&e.record_id).is_none_or(|(h, _)| e.hlc.cmp_total(h).is_gt());
                if newer {
                    latest.insert(e.record_id.clone(), (e.hlc.clone(), value));
                }
            }
        }
    }
    Ok(latest.into_iter().filter(|(id, _)| !is_collection(id) || set.contains(id)).filter_map(|(id, (_, v))| v.map(|v| (id, v))).collect())
}

/// Drop Node companions hosted on revoked devices, and every grant naming them.
fn drop_revoked_hosts(st: &mut State, revoked: &BTreeSet<String>) {
    let Some(c) = st.recs.get_mut(COMPANIONS) else { return };
    let gone: BTreeSet<String> = c
        .iter()
        .filter_map(|(id, v)| {
            let p: morph_ai::CompanionProfile = serde_json::from_value(v.clone()).ok()?;
            (p.identity.kind == AgentKind::Node && p.host.as_ref().is_some_and(|h| revoked.contains(h)))
                .then(|| id.trim_start_matches("c/").to_string())
        })
        .collect();
    if gone.is_empty() {
        return;
    }
    c.retain(|id, _| !gone.contains(id.trim_start_matches("c/")));
    if c.get("default").and_then(Value::as_str).is_some_and(|d| gone.contains(d)) {
        c.remove("default");
    }
    if let Some(g) = st.recs.get_mut(GRANTS) {
        g.retain(|id, v| {
            if let Some(rest) = id.strip_prefix("site/") {
                return !rest.split(SEP).next().is_some_and(|a| gone.contains(a));
            }
            if let Some(a) = id.strip_prefix("ext/") {
                return !gone.contains(a);
            }
            match v.get("companion").cloned().map(serde_json::from_value::<CompanionScope>) {
                Some(Ok(CompanionScope::One(a))) => !gone.contains(a.as_str()),
                _ => true,
            }
        });
    }
}

fn write_if_changed<T: PartialEq>(current: Option<T>, next: T, save: impl FnOnce(&T) -> std::result::Result<(), StoreError>) -> Result<bool> {
    if current.as_ref() == Some(&next) {
        return Ok(false);
    }
    save(&next)?;
    Ok(true)
}

fn write_files(store: &dyn AiStore, st: &mut State, types: &[&str]) -> Result<bool> {
    let mut changed = false;
    let recs = |t: &str| st.recs.get(t).cloned().unwrap_or_default();
    if types.contains(&COMPANIONS) {
        let c = recs(COMPANIONS);
        let mut list: Vec<Value> = c.iter().filter(|(k, _)| k.starts_with("c/")).map(|(_, v)| v.clone()).collect();
        // The first-run companion stays first (it is the fallback default), then by name.
        let key = |v: &Value| {
            let id = v.pointer("/identity/id").and_then(Value::as_str).unwrap_or_default().to_string();
            let name = v.pointer("/identity/name").and_then(Value::as_str).unwrap_or_default().to_lowercase();
            (id != "local", name, id)
        };
        list.sort_by_key(key);
        let default = c.get("default").cloned().filter(|d| list.iter().any(|p| p.pointer("/identity/id") == Some(d)));
        if let Ok(roster) = serde_json::from_value::<Companions>(json!({ "companions": list, "default": default })) {
            changed |= write_if_changed(store.load_roster()?, roster, |r| store.save_roster(r))?;
        }
        if let Some(s) = c.get("settings").and_then(|v| serde_json::from_value::<AgentSettings>(v.clone()).ok()) {
            changed |= write_if_changed(store.load_settings()?, s, |s| store.save_settings(s))?;
        }
    }
    if types.contains(&GRANTS) {
        let g = recs(GRANTS);
        let mut grants: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
        let mut allows = Vec::new();
        let mut external = Vec::new();
        let mut next = st.next_allow_id.max(st.allow_ids.values().max().map_or(0, |m| m + 1));
        let mut ordered: Vec<(&String, &Value)> = g.iter().filter(|(k, _)| k.starts_with("allow/")).collect();
        ordered.sort_by_key(|(k, v)| (v.get("created_unix_ms").and_then(Value::as_u64).unwrap_or(0), (*k).clone()));
        let mut allow_ids = BTreeMap::new();
        for (k, v) in ordered {
            let key = k.trim_start_matches("allow/").to_string();
            let id = match st.allow_ids.get(&key) {
                Some(id) => *id,
                None => {
                    let id = next;
                    next += 1;
                    id
                }
            };
            allow_ids.insert(key, id);
            let mut v = v.clone();
            if let Some(o) = v.as_object_mut() {
                o.insert("id".into(), json!(id));
            }
            allows.push(v);
        }
        for (k, v) in &g {
            if let Some(rest) = k.strip_prefix("site/")
                && let Some((agent, site)) = rest.split_once(SEP)
            {
                grants.entry(agent.to_string()).or_default().insert(site.to_string(), v.clone());
            } else if let Some(a) = k.strip_prefix("ext/") {
                external.push(a.to_string());
            }
        }
        let next_allow_id = next.max(allow_ids.values().max().map_or(0, |m| m + 1));
        st.allow_ids = allow_ids;
        st.next_allow_id = next_allow_id;
        if let Ok(policy) = serde_json::from_value::<Policy>(
            json!({ "grants": grants, "allows": allows, "next_allow_id": next_allow_id, "approved_external": external }),
        ) {
            changed |= write_if_changed(store.load_policy()?, policy, |p| store.save_policy(p))?;
        }
    }
    if types.contains(&CHATS) {
        let mut log = ChatLog { turns: recs(CHATS).into_values().filter_map(|v| serde_json::from_value(v).ok()).collect() };
        log.sort();
        // Keep this device's unclipped answers when the synced copy is only their clipped form.
        let current = store.load_chats()?;
        if let Some(cur) = &current {
            for t in log.turns.iter_mut() {
                if let Some(full) = cur.turns.iter().find(|c| c.id == t.id && c.for_sync() == *t) {
                    *t = full.clone();
                }
            }
        }
        changed |= write_if_changed(current, log, |l| store.save_chats(l))?;
    }
    if types.contains(&MCP) {
        let mut cfg = McpConfigs::default();
        for s in recs(MCP).into_values().filter_map(|v| serde_json::from_value(v).ok()) {
            cfg.upsert(s);
        }
        changed |= write_if_changed(store.load_mcp()?.map(|m| m.for_sync()), cfg, |c| store.save_mcp(c))?;
    }
    if types.contains(&JOURNAL) {
        let j = recs(JOURNAL);
        let mut journals = Journals {
            journals: j.iter().filter(|(k, _)| k.starts_with("j/")).filter_map(|(_, v)| serde_json::from_value(v.clone()).ok()).collect(),
            entries: j.iter().filter(|(k, _)| k.starts_with("e/")).filter_map(|(_, v)| serde_json::from_value(v.clone()).ok()).collect(),
        };
        journals.journals.sort_by_key(|j| (j.id != morph_ai::NOTES_ID, j.name.to_lowercase()));
        journals.entries.sort_by(|a, b| a.at_ms.cmp(&b.at_ms).then_with(|| a.id.cmp(&b.id)));
        changed |= write_if_changed(store.load_journals()?, journals, |j| store.save_journals(j))?;
    }
    Ok(changed)
}

/// Write the merged records into the files. Returns whether any file changed (the panel and
/// Settings then reload). Does nothing until [`backfill`] has run once, so a pass can never
/// replace local files with an empty log.
pub fn materialize(lib: &mut Library, store: &dyn AiStore) -> Result<bool> {
    let types = on(lib);
    if types.is_empty() {
        return Ok(false);
    }
    let Some(base) = load_snapshot(lib)? else { return Ok(false) };
    let log = load_log(lib)?;
    let revoked: BTreeSet<String> = load_roster(lib)?.certs.iter().filter(|c| !c.active()).map(|c| c.peer_id.clone()).collect();
    let mut st = State { recs: base.recs.clone(), allow_ids: base.allow_ids.clone(), next_allow_id: base.next_allow_id };
    for typ in &types {
        st.recs.insert(typ.to_string(), merge_type(lib, &log, typ, &revoked)?);
    }
    drop_revoked_hosts(&mut st, &revoked);
    let changed = write_files(store, &mut st, &types)?;
    save_snapshot(lib, &st)?;
    Ok(changed)
}

/// A device was revoked on this device: void its grants now, not on the next pass.
pub fn after_revoke(lib: &mut Library, store: &dyn AiStore, now_ms: i64) -> Result<bool> {
    backfill(lib, store, now_ms)?;
    materialize(lib, store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{enable, save_roster};
    use morph_ai::{ChatTurn, CompanionProfile, Journal, JournalEntry, JournalKind, McpServer, McpTransport, McpValue, MemoryStore, ModelChoice};
    use morph_identity::profile_identity::LibraryKey;
    use weirdnet_agent::{AgentId, AllowRule, SiteLevel};

    fn lib() -> (tempfile::TempDir, Library) {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(&tmp.path().join("lib"), &LibraryKey::from_bytes([7; 32])).unwrap();
        (tmp, lib)
    }

    /// An op from another device, as a pull would integrate it.
    fn remote(lib: &mut Library, node: &str, ms: u64, typ: &str, record: &str, op: RecOp) {
        let mut log = load_log(lib).unwrap();
        let n = NodeId::new(node);
        log.append(n.clone(), Hlc::new(ms, 0, n), typ, record, serde_json::to_vec(&op).unwrap());
        persist_log(lib, &log, None).unwrap();
    }

    fn put<T: Serialize>(v: &T) -> RecOp {
        RecOp::Put { value: to_value(v) }
    }

    fn ours(lib: &Library, typ: &str) -> Vec<(String, RecOp)> {
        load_log(lib)
            .unwrap()
            .iter_node(&NodeId::new("a"))
            .filter(|e| e.typ == typ)
            .map(|e| (e.record_id.clone(), serde_json::from_slice(&e.op).unwrap()))
            .collect()
    }

    fn revoke(lib: &mut Library, peer: &str) {
        let cert = weirdnet_net::user::NodeCert {
            user_id: "u".into(),
            peer_id: peer.into(),
            label: peer.into(),
            cert: vec![],
            issued_at: 1,
            revoked_at: Some(2),
            revocation: None,
        };
        let mut r = weirdnet_sync::Roster::new();
        r.upsert(cert);
        save_roster(lib, &r).unwrap();
    }

    fn setup() -> (tempfile::TempDir, Library, MemoryStore) {
        let (t, mut l) = lib();
        enable(&mut l, "a", 10).unwrap();
        (t, l, MemoryStore::default())
    }

    #[test]
    fn companion_types_are_default_opt_ins() {
        let (_t, l, _s) = setup();
        assert_eq!(on(&l), TYPES);
    }

    #[test]
    fn libraries_on_the_previous_defaults_pick_the_companion_types_up_once() {
        use crate::sync::{BOOKMARKS, HISTORY, SETTINGS, set_types};
        let (_t, mut l) = lib();
        enable(&mut l, "a", 10).unwrap();
        set_types(&mut l, &[BOOKMARKS.to_string(), HISTORY.to_string(), SETTINGS.to_string()]).unwrap();
        l.set_sync_meta("defaults", b"3").unwrap();
        enable(&mut l, "a", 11).unwrap();
        assert_eq!(on(&l), TYPES);
        // A later opt-out sticks.
        set_types(&mut l, &[BOOKMARKS.to_string()]).unwrap();
        enable(&mut l, "a", 12).unwrap();
        assert!(on(&l).is_empty());
    }

    #[test]
    fn local_changes_go_out_by_difference_and_first_run_defaults_go_out_at_time_zero() {
        let (_t, mut l, s) = setup();
        s.save_roster(&Companions::first_run(ModelChoice::Gemma4E4B)).unwrap();
        backfill(&mut l, &s, 100).unwrap();
        let first = ours(&l, COMPANIONS);
        assert_eq!(first.len(), 1);
        let e = load_log(&l).unwrap().iter_node(&NodeId::new("a")).find(|e| e.typ == COMPANIONS).unwrap().hlc.clone();
        assert_eq!(e.physical_ms, 0, "the untouched first-run companion never wins over a linked device");
        backfill(&mut l, &s, 101).unwrap();
        assert_eq!(ours(&l, COMPANIONS).len(), 1, "nothing changed, nothing logged");

        let mut r = s.load_roster().unwrap().unwrap();
        r.add(CompanionProfile::local("atlas", "Atlas", ModelChoice::Gemma4E2B)).unwrap();
        r.set_default(&AgentId::new("atlas")).unwrap();
        s.save_roster(&r).unwrap();
        backfill(&mut l, &s, 102).unwrap();
        let ops = ours(&l, COMPANIONS);
        assert_eq!(ops[1..].iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["c/atlas", "default"]);
        r.remove(&AgentId::new("atlas")).unwrap();
        s.save_roster(&r).unwrap();
        backfill(&mut l, &s, 103).unwrap();
        let ops = ours(&l, COMPANIONS);
        assert_eq!(ops.last().unwrap(), &("default".to_string(), RecOp::Remove));
        assert!(ops.contains(&("c/atlas".to_string(), RecOp::Remove)));
    }

    #[test]
    fn remote_companions_settings_and_defaults_merge_into_the_files() {
        let (_t, mut l, s) = setup();
        s.save_roster(&Companions::first_run(ModelChoice::Gemma4E4B)).unwrap();
        backfill(&mut l, &s, 100).unwrap();
        let renamed = {
            let mut r = Companions::first_run(ModelChoice::Gemma4E4B);
            r.rename(&AgentId::new("local"), "Desk").unwrap();
            r.companions()[0].clone()
        };
        remote(&mut l, "b", 50, COMPANIONS, "c/local", put(&renamed));
        let atlas = CompanionProfile::on_node("atlas", "Atlas", "b", ModelChoice::Gemma4E4B);
        remote(&mut l, "b", 51, COMPANIONS, "c/atlas", put(&atlas));
        remote(&mut l, "b", 52, COMPANIONS, "default", RecOp::Put { value: json!("atlas") });
        let settings = AgentSettings { search_provider: morph_ai::SearchProvider::Google, ..Default::default() };
        remote(&mut l, "b", 53, COMPANIONS, "settings", put(&settings));
        assert!(materialize(&mut l, &s).unwrap());
        let r = s.load_roster().unwrap().unwrap();
        assert_eq!(
            r.companions().iter().map(|c| c.name()).collect::<Vec<_>>(),
            ["Desk", "Atlas"],
            "the linked device's rename beat the fresh default"
        );
        assert_eq!(r.default_companion().unwrap().name(), "Atlas");
        assert_eq!(s.load_settings().unwrap().unwrap(), settings);
        assert!(!materialize(&mut l, &s).unwrap(), "idempotent");
        backfill(&mut l, &s, 200).unwrap();
        assert_eq!(ours(&l, COMPANIONS).len(), 1, "materialized records are not echoed back");
    }

    #[test]
    fn a_concurrent_add_beats_a_remove_and_a_later_remove_wins() {
        let (_t, mut l, s) = setup();
        backfill(&mut l, &s, 100).unwrap();
        let turn = ChatTurn::new(AgentId::new("local"), "q", "a", 5);
        let rec = format!("t/{}", turn.id);
        remote(&mut l, "b", 50, CHATS, &rec, put(&turn));
        remote(&mut l, "c", 50, CHATS, &rec, RecOp::Remove);
        materialize(&mut l, &s).unwrap();
        assert_eq!(s.load_chats().unwrap().unwrap().turns.len(), 1, "concurrent add wins");
        remote(&mut l, "c", 60, CHATS, &rec, RecOp::Remove);
        materialize(&mut l, &s).unwrap();
        assert!(s.load_chats().unwrap().unwrap().turns.is_empty());
    }

    #[test]
    fn grants_sync_with_stable_local_allow_ids() {
        let (_t, mut l, s) = setup();
        let mut p = Policy::new();
        p.set_site_level(&AgentId::new("local"), "a.com", SiteLevel::Act);
        let id = p.add_allow(CompanionScope::All, AllowRule::Research { provider: None }, 7);
        s.save_policy(&p).unwrap();
        backfill(&mut l, &s, 100).unwrap();
        let keys: Vec<String> = ours(&l, GRANTS).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, [format!("allow/a/{id}"), format!("site/local{SEP}a.com")]);

        // Another device adds an allow and turns a site off.
        let theirs = json!({"companion": {"one": "local"}, "rule": {"tier": "navigate", "site": "tokio.rs"}, "created_unix_ms": 9});
        let theirs = serde_json::from_value::<weirdnet_agent::StandingAllow>(
            json!({"id": 0, "companion": theirs["companion"], "rule": theirs["rule"], "created_unix_ms": 9}),
        );
        let mut v = to_value(&theirs.unwrap());
        v.as_object_mut().unwrap().remove("id");
        remote(&mut l, "b", 50, GRANTS, "allow/b/0", RecOp::Put { value: v });
        remote(&mut l, "b", 51, GRANTS, &format!("site/scout{SEP}x.org"), RecOp::Put { value: json!("off") });
        materialize(&mut l, &s).unwrap();
        let got = s.load_policy().unwrap().unwrap();
        assert_eq!(got.allows().len(), 2);
        assert_eq!(got.allows()[0].id, id, "our allow keeps its id, so a Stop on its notice still finds it");
        let new_id = got.allows()[1].id;
        assert_ne!(new_id, id);
        assert_eq!(got.site_level(&AgentId::new("scout"), "x.org"), SiteLevel::Off);
        assert_eq!(got.site_level(&AgentId::new("local"), "a.com"), SiteLevel::Act);
        materialize(&mut l, &s).unwrap();
        assert_eq!(s.load_policy().unwrap().unwrap().allows()[1].id, new_id, "ids are stable across passes");

        // Stop here revokes it everywhere.
        let mut p = s.load_policy().unwrap().unwrap();
        p.revoke(new_id);
        s.save_policy(&p).unwrap();
        backfill(&mut l, &s, 300).unwrap();
        assert!(ours(&l, GRANTS).contains(&("allow/b/0".to_string(), RecOp::Remove)));
        materialize(&mut l, &s).unwrap();
        assert_eq!(s.load_policy().unwrap().unwrap().allows().len(), 1);
    }

    #[test]
    fn revoking_a_device_voids_its_grants_and_its_node_companions() {
        let (_t, mut l, s) = setup();
        s.save_roster(&Companions::first_run(ModelChoice::Gemma4E4B)).unwrap();
        let mut p = Policy::new();
        p.set_site_level(&AgentId::new("local"), "bank.com", SiteLevel::Off);
        p.set_site_level(&AgentId::new("local"), "news.com", SiteLevel::Act);
        s.save_policy(&p).unwrap();
        backfill(&mut l, &s, 100).unwrap();

        // The phone ("b") widened grants, tightened one, and hosts a Node companion.
        remote(&mut l, "b", 200, GRANTS, &format!("site/local{SEP}bank.com"), RecOp::Put { value: json!("act") });
        remote(&mut l, "b", 201, GRANTS, &format!("site/local{SEP}news.com"), RecOp::Put { value: json!("off") });
        let allow = json!({"companion": "all", "rule": {"tier": "research", "provider": null}, "created_unix_ms": 3});
        remote(&mut l, "b", 202, GRANTS, "allow/b/0", RecOp::Put { value: allow });
        remote(&mut l, "b", 203, GRANTS, "ext/claude", RecOp::Put { value: json!(true) });
        let atlas = CompanionProfile::on_node("atlas", "Atlas", "b", ModelChoice::Gemma4E2B);
        remote(&mut l, "b", 204, COMPANIONS, "c/atlas", put(&atlas));
        remote(&mut l, "c", 205, GRANTS, &format!("site/atlas{SEP}a.org"), RecOp::Put { value: json!("act") });
        materialize(&mut l, &s).unwrap();
        let p = s.load_policy().unwrap().unwrap();
        assert_eq!(p.site_level(&AgentId::new("local"), "bank.com"), SiteLevel::Act);
        assert_eq!(p.allows().len(), 1);
        assert!(p.is_external_approved(&AgentId::new("claude")));
        assert!(s.load_roster().unwrap().unwrap().get(&AgentId::new("atlas")).is_some());

        revoke(&mut l, "b");
        assert!(after_revoke(&mut l, &s, 300).unwrap());
        let p = s.load_policy().unwrap().unwrap();
        assert_eq!(p.site_level(&AgentId::new("local"), "bank.com"), SiteLevel::Off, "its widening is void; ours stands");
        assert_eq!(p.site_level(&AgentId::new("local"), "news.com"), SiteLevel::Off, "its tightening stands");
        assert!(p.allows().is_empty(), "its standing allows are gone");
        assert!(!p.is_external_approved(&AgentId::new("claude")));
        assert!(s.load_roster().unwrap().unwrap().get(&AgentId::new("atlas")).is_none(), "its Node companion is gone");
        assert_eq!(p.site_level(&AgentId::new("atlas"), "a.org"), SiteLevel::Ask, "and grants for it from other devices");
        // A later local pass does not resurrect anything by logging removals as local edits.
        backfill(&mut l, &s, 301).unwrap();
        materialize(&mut l, &s).unwrap();
        assert!(s.load_policy().unwrap().unwrap().allows().is_empty());
    }

    #[test]
    fn mcp_configs_sync_without_secrets_and_journals_sync() {
        let (_t, mut l, s) = setup();
        let mut env = BTreeMap::new();
        env.insert("API_TOKEN".to_string(), McpValue::Plain { value: "tok-123".into() });
        let mut cfg = McpConfigs::default();
        cfg.upsert(McpServer::new("S", McpTransport::Stdio { command: "s".into(), args: vec![], env }));
        s.save_mcp(&cfg).unwrap();
        let mut j = Journals::default();
        j.save_to_notes(JournalEntry::from_answer(AgentId::new("local"), "T", "# T".into(), vec![], 1, None));
        s.save_journals(&j).unwrap();
        backfill(&mut l, &s, 100).unwrap();
        let mcp_ops = serde_json::to_string(&ours(&l, MCP)).unwrap();
        assert!(!mcp_ops.contains("tok-123"), "{mcp_ops}");
        assert!(mcp_ops.contains("API_TOKEN"));
        assert_eq!(ours(&l, JOURNAL).len(), 2, "Notes and its entry");

        let proj = Journal { id: "journal-b".into(), name: "Research".into(), kind: JournalKind::Project };
        remote(&mut l, "b", 50, JOURNAL, "j/journal-b", put(&proj));
        materialize(&mut l, &s).unwrap();
        let names: Vec<String> = s.load_journals().unwrap().unwrap().journals.into_iter().map(|j| j.name).collect();
        assert_eq!(names, ["Notes", "Research"]);
    }

    #[test]
    fn long_answers_stay_whole_on_the_device_that_has_them() {
        let (_t, mut l, s) = setup();
        let long = ChatTurn::new(AgentId::new("local"), "q", "x".repeat(morph_ai::chats::MAX_SYNCED_ANSWER + 10), 1);
        let mut log = ChatLog::default();
        log.push(long.clone());
        s.save_chats(&log).unwrap();
        backfill(&mut l, &s, 100).unwrap();
        materialize(&mut l, &s).unwrap();
        assert_eq!(s.load_chats().unwrap().unwrap().turns[0], long);
    }

    #[test]
    fn nothing_is_written_before_the_first_backfill() {
        let (_t, mut l, s) = setup();
        s.save_roster(&Companions::first_run(ModelChoice::Gemma4E4B)).unwrap();
        assert!(!materialize(&mut l, &s).unwrap());
        assert!(s.load_roster().unwrap().is_some());
    }
}
