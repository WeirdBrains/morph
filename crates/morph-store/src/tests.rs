use super::*;

fn key(b: u8) -> LibraryKey {
    LibraryKey::from_bytes([b; 32])
}

fn visit(url: &str, ms: i64) -> HistoryEntry {
    HistoryEntry { url: url.into(), title: format!("t{ms}"), visited_ms: ms, ..Default::default() }
}

fn sample() -> BookmarkSet {
    BookmarkSet {
        folders: vec![Folder {
            id: "f1".into(),
            name: "Work".into(),
            parent_id: None,
            order: "V".into(),
            created_ms: 1,
            updated_ms: 2,
            ..Default::default()
        }],
        bookmarks: vec![
            Bookmark {
                id: "b2".into(),
                url: "https://b.example/".into(),
                title: "B".into(),
                added_ms: 3,
                updated_ms: 4,
                folder_id: Some("f1".into()),
                order: "a".into(),
                tags: vec!["zeta".into(), "alpha".into()],
                ..Default::default()
            },
            Bookmark {
                id: "b1".into(),
                url: "https://a.example/".into(),
                title: "A".into(),
                added_ms: 5,
                updated_ms: 5,
                folder_id: None,
                order: "k".into(),
                tags: vec![],
                ..Default::default()
            },
        ],
        deleted: vec![Tombstone { id: "gone".into(), deleted_ms: 9, ..Default::default() }],
    }
}

#[test]
fn bookmarks_round_trip_in_saved_order_with_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(tmp.path(), &key(1)).unwrap();
    assert_eq!(lib.bookmark_set().unwrap(), BookmarkSet::default());
    lib.save_bookmark_set(&sample()).unwrap();
    drop(lib);
    let mut lib = Library::open(tmp.path(), &key(1)).unwrap();
    assert_eq!(lib.bookmark_set().unwrap(), sample());
    let mut smaller = sample();
    smaller.bookmarks.remove(0);
    lib.save_bookmark_set(&smaller).unwrap();
    assert_eq!(lib.bookmark_set().unwrap(), smaller, "a save replaces everything, tags too");
}

#[test]
fn history_appends_caps_replaces_and_deletes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(tmp.path(), &key(1)).unwrap();
    assert_eq!(lib.newest_visit().unwrap(), None);
    lib.append_visits(&[visit("a", 1), visit("b", 2), visit("c", 3)], 0).unwrap();
    lib.append_visits(&[visit("d", 4)], 3).unwrap();
    let got = lib.history().unwrap();
    assert_eq!(
        got.iter().map(|e| (e.url.as_str(), e.visited_ms)).collect::<Vec<_>>(),
        [("d", 4), ("c", 3), ("b", 2)],
        "newest first, oldest dropped"
    );
    assert!(got.iter().all(|e| e.id.starts_with("h-")), "each visit gets a stable id");
    assert_eq!(lib.history_tombstones().unwrap().len(), 1, "the capped visit is tombstoned");
    lib.replace_newest_visit(&HistoryEntry { url: "d".into(), title: "D".into(), visited_ms: 5, ..Default::default() }).unwrap();
    assert_eq!(lib.newest_visit().unwrap().unwrap().title, "D");
    assert_eq!(lib.delete_visits(|e| e.url == "c").unwrap(), 1);
    assert_eq!(lib.history().unwrap().len(), 2);
    lib.clear_history().unwrap();
    assert!(lib.history().unwrap().is_empty());
    assert!(lib.history_tombstones().unwrap().len() >= 3, "deletes leave history tombstones");
}

#[test]
fn the_file_is_encrypted_and_needs_its_key() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(tmp.path(), &key(1)).unwrap();
    lib.save_bookmark_set(&sample()).unwrap();
    lib.append_visits(&[visit("https://secret-history.example/", 1)], 0).unwrap();
    drop(lib);
    let mut raw = std::fs::read(path(tmp.path())).unwrap();
    if let Ok(wal) = std::fs::read(tmp.path().join("library.db-wal")) {
        raw.extend(wal);
    }
    assert!(!raw.starts_with(b"SQLite format 3"));
    for needle in [&b"secret-history"[..], b"a.example", b"zeta", b"Work"] {
        assert!(!raw.windows(needle.len()).any(|w| w == needle), "{} is readable on disk", String::from_utf8_lossy(needle));
    }
    let err = Library::open(tmp.path(), &key(2)).unwrap_err();
    assert!(err.to_string().contains("wrong key"), "{err}");
    assert!(err.is_wrong_key());
    assert_eq!(Library::open(tmp.path(), &key(1)).unwrap().history().unwrap().len(), 1, "a wrong key leaves the file alone");
}

#[test]
fn profiles_with_different_keys_cannot_read_each_other() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    Library::open(&a, &key(1)).unwrap().append_visits(&[visit("x", 1)], 0).unwrap();
    Library::open(&b, &key(2)).unwrap();
    std::fs::copy(path(&a), path(&b)).unwrap();
    assert!(Library::open(&b, &key(2)).is_err());
}

#[test]
fn opening_creates_the_schema_once() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!exists(tmp.path()));
    assert_eq!(Library::open(tmp.path(), &key(1)).unwrap().schema_version().unwrap(), MIGRATIONS.len() as i64);
    assert!(exists(tmp.path()));
    assert_eq!(Library::open(tmp.path(), &key(1)).unwrap().schema_version().unwrap(), MIGRATIONS.len() as i64);
}

#[cfg(unix)]
#[test]
fn the_file_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    Library::open(tmp.path(), &key(1)).unwrap();
    assert_eq!(std::fs::metadata(path(tmp.path())).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn import_fills_only_an_empty_database_all_at_once() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(tmp.path(), &key(1)).unwrap();
    assert!(lib.is_empty().unwrap());
    let visits = [visit("a", 1), visit("b", 2), visit("c", 3)];
    assert!(lib.import(&sample(), &visits, 2).unwrap());
    assert_eq!(lib.bookmark_set().unwrap(), sample());
    let history = lib.history().unwrap();
    assert_eq!(history.iter().map(|e| (e.url.as_str(), e.visited_ms)).collect::<Vec<_>>(), [("c", 3), ("b", 2)]);
    assert!(history.iter().all(|e| !e.id.is_empty()));
    assert!(!lib.is_empty().unwrap());
    assert!(!lib.import(&BookmarkSet::default(), &[visit("z", 9)], 0).unwrap(), "never over existing records");
    assert_eq!(lib.history().unwrap().len(), 2);
}

#[test]
fn sync_log_and_meta_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(tmp.path(), &key(1)).unwrap();
    assert_eq!(lib.schema_version().unwrap(), 3);
    lib.set_sync_meta("node_id", b"peer-a").unwrap();
    assert_eq!(lib.sync_meta("node_id").unwrap(), Some(b"peer-a".to_vec()));
    let e = SyncLogEntry { node_id: "peer-a".into(), seq: 1, hlc: "h".into(), typ: "bookmarks".into(), record_id: "b1".into(), op: b"{}".to_vec() };
    lib.insert_sync_log(&e).unwrap();
    lib.set_sync_cursor("peer-a", 1).unwrap();
    assert_eq!(lib.sync_log().unwrap(), vec![e]);
    assert_eq!(lib.sync_cursors().unwrap(), vec![("peer-a".into(), 1)]);
}
