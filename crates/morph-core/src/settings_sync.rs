//! Morph's cross-device settings on top of `weirdnet-sync`.
//!
//! Only `search_engine`, `home_page` and `theme` sync; layout and paths
//! (`tab_position`, sidebar, `downloads_dir`) stay on the device. Each field
//! is one last-writer-wins record. `profiles/<id>/settings.json` stays the
//! file the shell reads; sync writes those three fields into it.
//!
//! Settings ops share the node's log stream; `weirdnet-sync` 0.2 keeps a
//! pull position per type. Ops on 0.1-era `<node>/settings` streams still merge.
use std::collections::BTreeMap;
use std::path::Path;

use morph_store::Library;
use serde::{Deserialize, Serialize};
use weirdnet_sync::hlc::Hlc;
use weirdnet_sync::{NodeId, SyncType};

use crate::settings::{Settings, ThemeChoice};
use crate::sync::{load_clock, load_log, node_id, persist_log, types_on};

pub const SETTINGS: &str = SyncType::SETTINGS;

/// The fields that sync, by record id.
pub const FIELDS: &[&str] = &["search_engine", "home_page", "theme"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum SettingsOp {
    Set { value: String },
}

fn encode(op: &SettingsOp) -> Vec<u8> {
    serde_json::to_vec(op).expect("settings op json")
}

fn decode(bytes: &[u8]) -> Option<SettingsOp> {
    serde_json::from_slice(bytes).ok()
}

pub fn settings_on(lib: &Library) -> bool {
    types_on(lib).ok().is_some_and(|t| t.iter().any(|x| x == SETTINGS))
}

fn get(s: &Settings, field: &str) -> String {
    match field {
        "search_engine" => s.search_engine.clone(),
        "home_page" => s.home_page.clone(),
        "theme" => s.theme.as_str().to_string(),
        _ => String::new(),
    }
}

fn put(s: &mut Settings, field: &str, value: &str) {
    match field {
        "search_engine" if crate::search::is_known(value) => s.search_engine = value.to_string(),
        "home_page" => s.home_page = value.to_string(),
        "theme" => s.theme = ThemeChoice::from_key(value),
        _ => {}
    }
}

/// Winning value and HLC per field, from every settings op in the log.
fn merged(lib: &Library) -> morph_store::Result<BTreeMap<String, (Hlc, String)>> {
    let log = load_log(lib)?;
    let mut nodes: std::collections::BTreeSet<String> = log.cursors().keys().map(|n| n.0.clone()).collect();
    for e in lib.sync_log()? {
        nodes.insert(e.node_id);
    }
    let revoked = crate::sync::revoked_writers(&crate::sync::load_roster(lib)?);
    let mut out: BTreeMap<String, (Hlc, String)> = BTreeMap::new();
    for n in nodes {
        for e in log.iter_node(&NodeId::new(n)).filter(|e| e.typ == SETTINGS && !crate::sync::late_from_revoked(&revoked, e)) {
            let Some(SettingsOp::Set { value }) = decode(&e.op) else { continue };
            if !FIELDS.contains(&e.record_id.as_str()) {
                continue;
            }
            let newer = out.get(&e.record_id).is_none_or(|(h, _)| e.hlc.cmp_total(h).is_gt());
            if newer {
                out.insert(e.record_id.clone(), (e.hlc.clone(), value));
            }
        }
    }
    Ok(out)
}

fn append(lib: &mut Library, ops: &[(&str, String, Option<Hlc>)], now_ms: i64) -> morph_store::Result<()> {
    let Some(our) = node_id(lib)? else { return Ok(()) };
    if ops.is_empty() {
        return Ok(());
    }
    let node = NodeId::new(&our);
    let mut clock = load_clock(lib)?;
    let mut log = load_log(lib)?;
    for (field, value, at) in ops {
        let hlc = match at {
            Some(h) => Hlc::new(h.physical_ms, h.counter, node.clone()),
            None => {
                let h = Hlc::tick(clock.as_ref(), now_ms.max(0) as u64, node.clone());
                clock = Some(h.clone());
                h
            }
        };
        log.append(node.clone(), hlc, SETTINGS, *field, encode(&SettingsOp::Set { value: value.clone() }));
    }
    persist_log(lib, &log, clock.as_ref())
}

/// Log the synced fields that differ between `prev` and `next` (a local save).
pub fn record_local_changes(lib: &mut Library, prev: &Settings, next: &Settings, now_ms: i64) -> morph_store::Result<()> {
    if !settings_on(lib) {
        return Ok(());
    }
    let ops: Vec<(&str, String, Option<Hlc>)> = FIELDS.iter().filter(|f| get(prev, f) != get(next, f)).map(|f| (*f, get(next, f), None)).collect();
    append(lib, &ops, now_ms)
}

/// Put file values that the log does not have yet into our stream: a first
/// enable, or a change saved while the library was locked. They are stamped
/// with the file's modification time, not now, so a newer change from
/// another device still wins and a fresh device's defaults never override a
/// linked device's choices.
pub fn backfill(lib: &mut Library, home: &Path, profile_id: &str) -> morph_store::Result<()> {
    if !settings_on(lib) || node_id(lib)?.is_none() {
        return Ok(());
    }
    let path = crate::profiles::settings_path(home, profile_id);
    let Some(file) = read(&path) else { return Ok(()) };
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let defaults = Settings::default();
    let have = merged(lib)?;
    let mut ops = Vec::new();
    for f in FIELDS {
        let value = get(&file, f);
        let at = Hlc::new(mtime, 0, NodeId::new(""));
        match have.get(*f) {
            None if value == get(&defaults, f) => {}
            None => ops.push((*f, value, Some(at))),
            Some((_, v)) if *v == value => {}
            Some((h, _)) if mtime > h.physical_ms => ops.push((*f, value, Some(at))),
            Some(_) => {}
        }
    }
    append(lib, &ops, 0)
}

fn read(path: &Path) -> Option<Settings> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Settings::default()),
        Err(_) => None,
    }
}

/// Write the merged synced fields into the profile's `settings.json`,
/// leaving every other field alone. Returns whether the file changed.
pub fn materialize(lib: &Library, home: &Path, profile_id: &str) -> Result<bool, crate::settings::SettingsError> {
    if !settings_on(lib) {
        return Ok(false);
    }
    let have = merged(lib).map_err(|e| std::io::Error::other(e.to_string()))?;
    if have.is_empty() {
        return Ok(false);
    }
    let path = crate::profiles::settings_path(home, profile_id);
    let Some(mut file) = read(&path) else { return Ok(false) };
    let before = file.clone();
    for (field, (_, value)) in &have {
        put(&mut file, field, value);
    }
    if file == before {
        return Ok(false);
    }
    crate::settings::write_profile(home, profile_id, &file)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{BOOKMARKS, HISTORY, enable, set_types};
    use morph_identity::profile_identity::LibraryKey;

    fn setup() -> (tempfile::TempDir, std::path::PathBuf, Library) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        crate::profiles::create(&home, "Work").unwrap();
        let lib = Library::open(&tmp.path().join("lib"), &LibraryKey::from_bytes([7; 32])).unwrap();
        (tmp, home, lib)
    }

    fn remote(lib: &mut Library, n: &str, ms: u64, field: &str, value: &str) {
        let mut log = load_log(lib).unwrap();
        let node = NodeId::new(n);
        log.append(node.clone(), Hlc::new(ms, 0, node), SETTINGS, field, encode(&SettingsOp::Set { value: value.into() }));
        persist_log(lib, &log, None).unwrap();
    }

    fn file(home: &Path) -> Settings {
        read(&crate::profiles::settings_path(home, "p1")).unwrap()
    }

    #[test]
    fn only_the_three_fields_sync_and_the_rest_stay_local() {
        let (_t, home, mut lib) = setup();
        enable(&mut lib, "a", 10).unwrap();
        assert!(settings_on(&lib), "settings is a default opt-in");
        let local = Settings { sidebar_width: 333, downloads_dir: "/dl".into(), ..Settings::default() };
        crate::settings::write_profile(&home, "p1", &local).unwrap();
        remote(&mut lib, "b", 20, "theme", "light");
        remote(&mut lib, "b", 21, "home_page", "https://home.example/");
        remote(&mut lib, "b", 22, "search_engine", "duckduckgo");
        remote(&mut lib, "b", 23, "downloads_dir", "/elsewhere");
        assert!(materialize(&lib, &home, "p1").unwrap());
        let s = file(&home);
        assert_eq!((s.theme, s.home_page.as_str(), s.search_engine.as_str()), (ThemeChoice::Light, "https://home.example/", "duckduckgo"));
        assert_eq!((s.sidebar_width, s.downloads_dir.as_str()), (333, "/dl"));
        assert!(!materialize(&lib, &home, "p1").unwrap(), "idempotent");
    }

    #[test]
    fn a_revoked_devices_late_ops_are_void_and_its_earlier_ones_stand() {
        let (_t, home, mut lib) = setup();
        enable(&mut lib, "a", 10).unwrap();
        remote(&mut lib, "b", 30, "theme", "light");
        remote(&mut lib, "b", 300, "home_page", "https://late.example/");
        let mut r = weirdnet_sync::Roster::new();
        r.upsert(weirdnet_net::user::NodeCert {
            user_id: "u".into(),
            peer_id: "b".into(),
            label: "b".into(),
            cert: vec![],
            issued_at: 1,
            revoked_at: Some(100),
            revocation: None,
        });
        crate::sync::save_roster(&mut lib, &r).unwrap();
        assert!(crate::sync::revocations_unapplied(&lib).unwrap());
        materialize(&lib, &home, "p1").unwrap();
        let s = file(&home);
        assert_eq!(s.theme, ThemeChoice::Light, "written before the revocation");
        assert_eq!(s.home_page, Settings::default().home_page, "written after it, relayed: void");
        crate::sync::mark_revocations_applied(&mut lib).unwrap();
        assert!(!crate::sync::revocations_unapplied(&lib).unwrap());
    }

    #[test]
    fn last_writer_wins_per_field_and_unknown_engines_are_ignored() {
        let (_t, home, mut lib) = setup();
        enable(&mut lib, "a", 10).unwrap();
        remote(&mut lib, "b", 30, "theme", "light");
        remote(&mut lib, "c", 20, "theme", "dark");
        remote(&mut lib, "c", 40, "search_engine", "not-an-engine");
        materialize(&lib, &home, "p1").unwrap();
        let s = file(&home);
        assert_eq!(s.theme, ThemeChoice::Light);
        assert_eq!(s.search_engine, Settings::default().search_engine);
    }

    #[test]
    fn local_changes_log_only_synced_fields() {
        let (_t, _home, mut lib) = setup();
        enable(&mut lib, "a", 10).unwrap();
        let prev = Settings::default();
        let next = Settings { theme: ThemeChoice::Light, sidebar_width: 400, ..Settings::default() };
        record_local_changes(&mut lib, &prev, &next, 11).unwrap();
        let log = load_log(&lib).unwrap();
        let ours: Vec<_> = log.iter_node(&NodeId::new("a")).collect();
        assert_eq!(ours.len(), 1);
        assert_eq!(ours[0].record_id, "theme");
        set_types(&mut lib, &[BOOKMARKS.to_string(), HISTORY.to_string()]).unwrap();
        record_local_changes(&mut lib, &next, &prev, 12).unwrap();
        assert_eq!(load_log(&lib).unwrap().iter_node(&NodeId::new("a")).count(), 1, "off means not logged");
    }

    #[test]
    fn backfill_logs_customized_values_at_file_time_and_skips_defaults() {
        let (_t, home, mut lib) = setup();
        enable(&mut lib, "a", 10).unwrap();
        crate::settings::write_profile(&home, "p1", &Settings::default()).unwrap();
        backfill(&mut lib, &home, "p1").unwrap();
        assert_eq!(load_log(&lib).unwrap().iter_node(&NodeId::new("a")).count(), 0, "defaults are not pushed");

        crate::settings::write_profile(&home, "p1", &Settings { home_page: "https://mine.example/".into(), ..Settings::default() }).unwrap();
        backfill(&mut lib, &home, "p1").unwrap();
        let log = load_log(&lib).unwrap();
        let ours: Vec<_> = log.iter_node(&NodeId::new("a")).cloned().collect();
        assert_eq!(ours.len(), 1);
        assert!(ours[0].hlc.physical_ms > 1_000_000, "stamped with the file time");
        backfill(&mut lib, &home, "p1").unwrap();
        assert_eq!(load_log(&lib).unwrap().iter_node(&NodeId::new("a")).count(), 1, "nothing new");

        // A newer remote value beats an older file.
        remote(&mut lib, "b", u64::MAX / 2, "home_page", "https://theirs.example/");
        backfill(&mut lib, &home, "p1").unwrap();
        materialize(&lib, &home, "p1").unwrap();
        assert_eq!(file(&home).home_page, "https://theirs.example/");
    }
}
