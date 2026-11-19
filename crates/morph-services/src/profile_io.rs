//! The companions' files under the profile lock: `companions/` (roster, policy, chats, journals,
//! settings, MCP configs, exports, the tool log) and the `mcp-*.local.json` files beside it (secret
//! values, approvals) are sealed with the profile's file key, like `settings.json` and `tabs.json`
//! (`docs/profile-lock.md`). [`file_store`], [`io_for`] and [`io_beside`] hand `morph-ai` the I/O to
//! use; nothing else reads or writes these files.
//!
//! A profile with no identity keeps plain files. While a profile is locked these files read as
//! [`StoreError::Locked`] and are never written; once it unlocks, [`seal_existing`] seals what an
//! older build or the time before the identity left in plaintext.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use morph_ai::{FileIo, FileStore, PlainIo, StoreError};
use morph_core::profile_files;
use morph_core::profiles::ProfileError;

/// Sealed (or, with no identity, plain) files of one profile.
#[derive(Debug, Clone)]
pub struct ProfileIo {
    home: PathBuf,
    id: String,
}

fn failed(e: ProfileError) -> StoreError {
    match e {
        ProfileError::Locked => StoreError::Locked,
        other => StoreError::Io(other.to_string()),
    }
}

impl FileIo for ProfileIo {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, StoreError> {
        profile_files::read(&self.home, &self.id, path).map(|b| b.map(|b| b.to_vec())).map_err(failed)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        profile_files::write(&self.home, &self.id, path, bytes).map_err(failed)
    }

    fn locked(&self) -> bool {
        profile_files::require_key(&self.home, &self.id).is_err()
    }
}

/// `(home, profile id)` for a path inside `<home>/profiles/<id>/` (the profile directory itself, or
/// the `companions` directory under it).
fn locate(dir: &Path) -> Option<(PathBuf, String)> {
    let profile = if dir.file_name().is_some_and(|n| n == "companions") { dir.parent()? } else { dir };
    let id = profile.file_name()?.to_str()?.to_string();
    let profiles = profile.parent()?;
    if profiles.file_name()? != "profiles" {
        return None;
    }
    Some((profiles.parent()?.to_path_buf(), id))
}

/// The I/O for files in the profile directory `profile_dir` or its `companions` directory. A path
/// outside the profile layout (a test's scratch directory) gets plain files.
pub fn io_for(dir: &Path) -> Arc<dyn FileIo> {
    match locate(dir) {
        Some((home, id)) => Arc::new(ProfileIo { home, id }),
        None => Arc::new(PlainIo),
    }
}

/// [`io_for`] a profile's directory (for the `mcp-*.local.json` files beside `companions/`).
pub fn io_beside(profile_dir: &Path) -> Arc<dyn FileIo> {
    io_for(profile_dir)
}

/// A per-purpose key for the profile holding `dir` ([`profile_files::derive_key`] under `context`):
/// `Ok(None)` outside the profile layout or for a profile without an identity, `Locked` while locked.
pub fn derive_key(dir: &Path, context: &[u8]) -> Result<Option<weirdnet_keys::Key32>, StoreError> {
    match locate(dir) {
        Some((home, id)) => profile_files::derive_key(&home, &id, context).map_err(failed),
        None => Ok(None),
    }
}

/// The store over a profile's `companions/` directory.
pub fn file_store(companions: &Path) -> FileStore {
    FileStore::with_io(companions, io_for(companions))
}

fn is_local_mcp(name: &str) -> bool {
    name.starts_with("mcp-") && name.ends_with(".local.json")
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Seals the plaintext these files left behind (an older build, or the time before the profile had
/// an identity). Run when a profile unlocks, after its key is loaded. Reading a plaintext file
/// seals it and removes the plaintext; a leftover `.tmp` of an interrupted write goes.
pub fn seal_existing(home: &Path, id: &str) {
    if !profile_files::is_sealed(home, id) {
        return;
    }
    let profile = morph_core::profiles::profile_dir(home, id);
    let mut files = Vec::new();
    files_under(&profile.join("companions"), &mut files);
    if let Ok(entries) = std::fs::read_dir(&profile) {
        files.extend(entries.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(is_local_mcp_or_tmp)));
    }
    for path in files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if name.ends_with(".sealed") {
            continue;
        }
        if name.ends_with(".tmp") {
            let _ = std::fs::remove_file(&path);
        } else if let Err(e) = profile_files::read(home, id, &path) {
            eprintln!("Morph: could not seal {}: {e}", path.display());
        }
    }
}

fn is_local_mcp_or_tmp(name: &str) -> bool {
    is_local_mcp(name) || (name.starts_with("mcp-") && name.ends_with(".local.tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{library, profile_lock};
    use morph_ai::{AgentSettings, AiStore, FileSecrets, McpApprovals, McpSecrets};
    use morph_identity::profile_identity::SecretString;

    const PASSWORD: &str = "profile password";
    const SECRET: &str = "tok-SECRET-4242";
    const ENV_VALUE: &str = "ENV-VALUE-7777";
    const APPROVAL: &str = "APPROVAL-FINGERPRINT-9999";

    fn home() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        (tmp, home)
    }

    fn pw() -> SecretString {
        SecretString::from(PASSWORD.to_string())
    }

    fn dirs(home: &Path) -> (PathBuf, PathBuf) {
        let profile = morph_core::profiles::profile_dir(home, "default");
        (profile.clone(), profile.join("companions"))
    }

    /// Every file under the profile that still holds `needle` in the clear.
    fn leaks(home: &Path, needle: &str) -> Vec<PathBuf> {
        let mut all = Vec::new();
        files_under(&dirs(home).0, &mut all);
        // The library database is encrypted whole, so its bytes are random: a short needle
        // such as a three-byte tag turns up in them by chance (roughly once in 16 MB), which
        // made this test fail now and then. A long needle (a digest or a secret) cannot.
        let random = |p: &Path| needle.len() < 16 && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("library.db"));
        all.into_iter().filter(|p| !random(p) && String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).contains(needle)).collect()
    }

    /// What an older build left: plaintext companion files and MCP local files.
    fn plant_plaintext(home: &Path) {
        let (profile, companions) = dirs(home);
        std::fs::create_dir_all(companions.join("exports")).unwrap();
        let put = |p: PathBuf, s: String| std::fs::write(p, s).unwrap();
        put(companions.join("mcp.json"), format!(r#"{{"servers":[{{"id":"srv","env":"{ENV_VALUE}"}}]}}"#));
        put(companions.join("policy.json"), r#"{"grants":"policy-marker"}"#.into());
        put(companions.join("roster.json"), r#"{"companions":[]}"#.into());
        put(companions.join("exports/t1.md"), format!("answer mentioning {SECRET}"));
        put(companions.join("tool-log.jsonl"), format!("{{\"trace\":\"{SECRET}\"}}\n"));
        put(profile.join("mcp-secrets.local.json"), format!(r#"{{"srv":{{"TOKEN":"{SECRET}"}}}}"#));
        put(profile.join("mcp-approved.local.json"), format!(r#"{{"approved":{{"srv":"{APPROVAL}"}}}}"#));
        put(profile.join("mcp-extra.local.json"), format!("{{\"x\":\"{SECRET}\"}}"));
        put(profile.join("mcp-secrets.local.tmp"), format!("{{\"x\":\"{SECRET}\"}}"));
    }

    #[test]
    fn approvals_are_keyed_by_the_profile_and_hold_no_plain_digest() {
        use morph_ai::{APPROVAL_KEY_CONTEXT, McpServer, McpTransport, McpValue};
        let (_tmp, home) = home();
        let (profile, companions) = dirs(&home);
        assert!(derive_key(&companions, APPROVAL_KEY_CONTEXT).unwrap().is_none(), "no identity, no key");
        assert!(derive_key(Path::new("/tmp/elsewhere"), APPROVAL_KEY_CONTEXT).unwrap().is_none());
        crate::identity::create(&home, "default", &pw()).unwrap();
        let key = derive_key(&companions, APPROVAL_KEY_CONTEXT).unwrap().unwrap();
        assert_eq!(key.0, derive_key(&profile, APPROVAL_KEY_CONTEXT).unwrap().unwrap().0, "the same key from either directory");
        assert_ne!(key.0, derive_key(&profile, b"morph/other/v1").unwrap().unwrap().0, "one key per purpose");

        let secrets = FileSecrets::open_with(profile.join("mcp-secrets.local.json"), io_beside(&profile));
        secrets.set("srv", "TOKEN", SECRET);
        let env = std::collections::BTreeMap::from([("TOKEN".to_string(), McpValue::Secret { name: "TOKEN".into() })]);
        let mut server = McpServer::new("S", McpTransport::Stdio { command: "x".into(), args: vec![], env });
        server.id = "srv".into();
        let plain = McpApprovals::fingerprint(&server, &secrets);
        let path = profile.join("mcp-approved.local.json");
        let mut a = McpApprovals::load_with(&path, io_beside(&profile).as_ref()).with_key(key.0);
        a.approve(&server, &secrets);
        a.save_with(&path, io_beside(&profile).as_ref()).unwrap();
        let inside = String::from_utf8(io_beside(&profile).read(&path).unwrap().unwrap()).unwrap();
        assert!(inside.contains("k1:") && !inside.contains(&plain) && !inside.contains(SECRET), "{inside}");
        let (by_digest, by_tag) = (leaks(&home, &plain), leaks(&home, "k1:"));
        assert!(by_digest.is_empty() && by_tag.is_empty(), "sealed on disk too: {by_digest:?} {by_tag:?}");

        profile_lock::lock(&home, "default");
        assert!(matches!(derive_key(&companions, APPROVAL_KEY_CONTEXT), Err(StoreError::Locked)), "no key while locked");
        library::unlock(&home, "default", &pw()).unwrap();
        let again = derive_key(&companions, APPROVAL_KEY_CONTEXT).unwrap().unwrap();
        assert_eq!(again.0, key.0, "stable across a lock");
        assert!(McpApprovals::load_with(&path, io_beside(&profile).as_ref()).with_key(again.0).is_approved(&server, &secrets));
    }

    #[test]
    fn files_round_trip_sealed_and_never_plain() {
        let (_tmp, home) = home();
        crate::identity::create(&home, "default", &pw()).unwrap();
        let (profile, companions) = dirs(&home);
        let store = file_store(&companions);
        let settings = AgentSettings::default();
        store.save_settings(&settings).unwrap();
        store.write_file("exports/t.md", SECRET.as_bytes()).unwrap();
        store.append_line("tool-log.jsonl", "one").unwrap();
        store.append_line("tool-log.jsonl", "two").unwrap();
        assert_eq!(store.load_settings().unwrap(), Some(settings));
        assert_eq!(store.read_file("tool-log.jsonl").unwrap().unwrap(), b"one\ntwo\n");
        assert_eq!(store.read_file("exports/t.md").unwrap().unwrap(), SECRET.as_bytes());

        let secrets = FileSecrets::open_with(profile.join("mcp-secrets.local.json"), io_beside(&profile));
        secrets.set("srv", "TOKEN", SECRET);
        assert_eq!(FileSecrets::open_with(profile.join("mcp-secrets.local.json"), io_beside(&profile)).get("srv", "TOKEN").as_deref(), Some(SECRET));
        let approvals: McpApprovals = serde_json::from_str(&format!(r#"{{"approved":{{"srv":"{APPROVAL}"}}}}"#)).unwrap();
        approvals.save_with(&profile.join("mcp-approved.local.json"), io_beside(&profile).as_ref()).unwrap();
        assert_eq!(McpApprovals::load_with(&profile.join("mcp-approved.local.json"), io_beside(&profile).as_ref()), approvals);

        for name in
            ["companions/settings.json", "companions/exports/t.md", "companions/tool-log.jsonl", "mcp-secrets.local.json", "mcp-approved.local.json"]
        {
            let plain = profile.join(name);
            assert!(!plain.exists(), "{name} is not plain");
            assert!(morph_core::profile_files::sealed_path(&plain).exists(), "{name} is sealed");
        }
        assert!(leaks(&home, SECRET).is_empty() && leaks(&home, APPROVAL).is_empty());
    }

    #[test]
    fn plaintext_is_sealed_on_the_first_unlock_and_nothing_is_left() {
        let (_tmp, home) = home();
        plant_plaintext(&home);
        assert!(!leaks(&home, SECRET).is_empty(), "the setup is plaintext");
        // An identity arrives (or, below, an older build's files are found at the next unlock).
        crate::identity::create(&home, "default", &pw()).unwrap();
        for needle in [SECRET, ENV_VALUE, APPROVAL, "policy-marker"] {
            assert!(leaks(&home, needle).is_empty(), "{needle} left on disk: {:?}", leaks(&home, needle));
        }
        let (profile, companions) = dirs(&home);
        assert!(!profile.join("mcp-secrets.local.tmp").exists(), "an interrupted write's leftover goes");
        let store = file_store(&companions);
        assert!(String::from_utf8(store.read_file("mcp.json").unwrap().unwrap()).unwrap().contains(ENV_VALUE));
        assert!(String::from_utf8(store.read_file("exports/t1.md").unwrap().unwrap()).unwrap().contains(SECRET));
        let secrets = FileSecrets::open_with(profile.join("mcp-secrets.local.json"), io_beside(&profile));
        assert_eq!(secrets.get("srv", "TOKEN").as_deref(), Some(SECRET));
        let extra = io_beside(&profile).read(&profile.join("mcp-extra.local.json")).unwrap().unwrap();
        assert!(String::from_utf8(extra).unwrap().contains(SECRET), "every mcp-*.local.json is sealed, not just the two known ones");
        let approvals = McpApprovals::load_with(&profile.join("mcp-approved.local.json"), io_beside(&profile).as_ref());
        assert_eq!(approvals, serde_json::from_str(&format!(r#"{{"approved":{{"srv":"{APPROVAL}"}}}}"#)).unwrap());

        // Files an older build writes while the profile is locked are sealed at the next unlock.
        profile_lock::lock(&home, "default");
        plant_plaintext(&home);
        assert!(store.read_file("mcp.json").is_err(), "locked: the plaintext is not served either");
        library::unlock(&home, "default", &pw()).unwrap();
        for needle in [SECRET, ENV_VALUE, APPROVAL, "policy-marker"] {
            assert!(leaks(&home, needle).is_empty(), "{needle} left after the second unlock");
        }
        assert!(store.read_file("policy.json").unwrap().is_some());
    }

    #[test]
    fn a_locked_profile_reads_nothing_writes_nothing_and_does_not_panic() {
        let (_tmp, home) = home();
        crate::identity::create(&home, "default", &pw()).unwrap();
        let (profile, companions) = dirs(&home);
        let store = file_store(&companions);
        store.save_settings(&AgentSettings::default()).unwrap();
        let secrets_path = profile.join("mcp-secrets.local.json");
        let live = FileSecrets::open_with(&secrets_path, io_beside(&profile));
        live.set("srv", "TOKEN", SECRET);
        let approvals_path = profile.join("mcp-approved.local.json");
        let approvals: McpApprovals = serde_json::from_str(&format!(r#"{{"approved":{{"srv":"{APPROVAL}"}}}}"#)).unwrap();
        approvals.save_with(&approvals_path, io_beside(&profile).as_ref()).unwrap();
        let sealed = |p: &Path| std::fs::read(morph_core::profile_files::sealed_path(p)).unwrap();
        let before = (sealed(&secrets_path), sealed(&approvals_path), sealed(&companions.join("settings.json")));

        profile_lock::lock(&home, "default");
        assert!(matches!(store.load_settings(), Err(StoreError::Locked)));
        assert!(matches!(store.save_settings(&AgentSettings::default()), Err(StoreError::Locked)));
        assert!(matches!(store.write_file("exports/x.md", b"x"), Err(StoreError::Locked)));
        assert!(matches!(store.append_line("tool-log.jsonl", "x"), Err(StoreError::Locked)));
        let locked = FileSecrets::open_with(&secrets_path, io_beside(&profile));
        assert_eq!(locked.get("srv", "TOKEN"), None, "locked secrets read as empty");
        locked.set("srv", "OTHER", "x");
        locked.remove_server("srv");
        assert_eq!(live.get("srv", "TOKEN"), None, "an instance opened before the lock sees nothing either, and writes nothing");
        assert_eq!(
            McpApprovals::load_with(&approvals_path, io_beside(&profile).as_ref()),
            McpApprovals::default(),
            "nothing is approved while locked"
        );
        assert!(approvals.save_with(&approvals_path, io_beside(&profile).as_ref()).is_err());
        assert_eq!(before, (sealed(&secrets_path), sealed(&approvals_path), sealed(&companions.join("settings.json"))), "no sealed file changed");

        library::unlock(&home, "default", &pw()).unwrap();
        assert_eq!(locked.get("srv", "TOKEN").as_deref(), Some(SECRET), "the secrets come back after the unlock, with no reopen");
        assert_eq!(live.get("srv", "TOKEN").as_deref(), Some(SECRET));
        assert!(store.load_settings().unwrap().is_some());
        locked.set("srv", "OTHER", "y");
        assert_eq!(FileSecrets::open_with(&secrets_path, io_beside(&profile)).get("srv", "OTHER").as_deref(), Some("y"));
    }

    #[test]
    fn a_profile_with_no_identity_and_a_stray_path_keep_plain_files() {
        let (_tmp, home) = home();
        let (profile, companions) = dirs(&home);
        let store = file_store(&companions);
        store.save_settings(&AgentSettings::default()).unwrap();
        assert!(companions.join("settings.json").exists());
        let outside = tempfile::tempdir().unwrap();
        let stray = file_store(outside.path());
        stray.write_file("a.json", b"{}").unwrap();
        assert!(outside.path().join("a.json").exists());
        assert!(io_beside(&profile).read(&profile.join("mcp-secrets.local.json")).unwrap().is_none());
    }
}
