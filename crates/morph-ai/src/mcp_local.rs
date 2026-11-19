//! What stays on one machine for MCP servers: secret values and the user's OK to start a local
//! command. Neither is ever part of [`crate::mcp::McpConfigs`], so neither syncs.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::file_io::{FileIo, PlainIo};
use crate::mcp::{McpSecrets, McpServer, McpTransport, McpValue};
use crate::store::StoreError;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Writes `bytes` atomically, readable by the user only.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

/// Secrets in memory (tests, and the fallback when nothing can be saved).
#[derive(Default)]
pub struct MemorySecrets(Mutex<BTreeMap<(String, String), String>>);

impl McpSecrets for MemorySecrets {
    fn get(&self, server: &str, name: &str) -> Option<String> {
        lock(&self.0).get(&(server.to_string(), name.to_string())).cloned()
    }
    fn set(&self, server: &str, name: &str, value: &str) {
        lock(&self.0).insert((server.to_string(), name.to_string()), value.to_string());
    }
    fn remove_server(&self, server: &str) {
        lock(&self.0).retain(|(s, _), _| s != server);
    }
}

/// Secrets in a JSON file readable only by the user (`0600`), written atomically. Debug output
/// never shows a value.
pub struct FileSecrets {
    path: PathBuf,
    io: Arc<dyn FileIo>,
    cache: Mutex<Loaded>,
}

/// The secrets read so far. `ready` stays false while the file could not be opened (a locked
/// profile): nothing is written until it has been read, so a locked start never wipes it.
#[derive(Default)]
struct Loaded {
    ready: bool,
    map: BTreeMap<String, BTreeMap<String, String>>,
}

impl Loaded {
    fn load(&mut self, io: &dyn FileIo, path: &Path) {
        if self.ready {
            return;
        }
        match io.read(path) {
            Ok(bytes) => {
                self.map = bytes.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                self.ready = true;
            }
            Err(_) => {}
        }
    }
}

impl std::fmt::Debug for FileSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSecrets").field("path", &self.path).finish_non_exhaustive()
    }
}

impl FileSecrets {
    /// Opens (reading it now if it exists) the plain file at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self::open_with(path, Arc::new(PlainIo))
    }

    /// Opens the file at `path` through `io`. A file that cannot be opened yet (locked) reads as
    /// empty and is read again on the next use; nothing is saved until then.
    pub fn open_with(path: impl Into<PathBuf>, io: Arc<dyn FileIo>) -> Self {
        let mut cache = Loaded::default();
        let path = path.into();
        cache.load(io.as_ref(), &path);
        Self { path, io, cache: Mutex::new(cache) }
    }

    fn save(&self, cache: &Loaded) {
        if cache.ready {
            if let Ok(bytes) = serde_json::to_vec_pretty(&cache.map) {
                let _ = self.io.write(&self.path, &bytes);
            }
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Loaded> {
        let mut c = lock(&self.cache);
        c.load(self.io.as_ref(), &self.path);
        c
    }

    /// Whether anything is kept for `server` (names only, never values).
    pub fn names(&self, server: &str) -> Vec<String> {
        self.cache().map.get(server).map(|m| m.keys().cloned().collect()).unwrap_or_default()
    }
}

impl McpSecrets for FileSecrets {
    fn get(&self, server: &str, name: &str) -> Option<String> {
        self.cache().map.get(server).and_then(|m| m.get(name)).cloned()
    }
    fn set(&self, server: &str, name: &str, value: &str) {
        let mut c = self.cache();
        if !c.ready {
            return;
        }
        c.map.entry(server.to_string()).or_default().insert(name.to_string(), value.to_string());
        self.save(&c);
    }
    fn remove_server(&self, server: &str) {
        let mut c = self.cache();
        if c.ready && c.map.remove(server).is_some() {
            self.save(&c);
        }
    }
}

/// What the user agreed to start on this machine: a local-command server runs only after the user
/// has seen its warning, and again if anything about it changes. Saved per machine.
#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct McpApprovals {
    /// Server id to the fingerprint that was approved.
    #[serde(default)]
    approved: BTreeMap<String, String>,
}

impl McpApprovals {
    /// Reads the file; a missing or unreadable file approves nothing.
    pub fn load(path: &Path) -> Self {
        Self::load_with(path, &PlainIo)
    }

    /// [`load`](Self::load) through `io`: a locked profile approves nothing.
    pub fn load_with(path: &Path, io: &dyn FileIo) -> Self {
        io.read(path).ok().flatten().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Writes the file (readable by the user only).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        write_private(path, &serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)
    }

    /// [`save`](Self::save) through `io`; refused while the profile is locked.
    pub fn save_with(&self, path: &Path, io: &dyn FileIo) -> Result<(), StoreError> {
        io.write(path, &serde_json::to_vec_pretty(self).map_err(|e| StoreError::Io(e.to_string()))?)
    }

    /// What a server is approved as: a SHA-256 over everything that decides what runs or where
    /// requests go (the program, arguments and every environment value for a local command; the
    /// URL and every header value for an HTTP server). Secret values are read from `secrets` and
    /// hashed in, so a changed token asks again too; only the digest is ever stored. A secret this
    /// device does not hold counts as missing, which is a state of its own.
    pub fn fingerprint(server: &McpServer, secrets: &dyn McpSecrets) -> String {
        use sha2::{Digest, Sha256};
        fn field(h: &mut Sha256, tag: &str, bytes: &[u8]) {
            h.update(tag.as_bytes());
            h.update((bytes.len() as u64).to_le_bytes());
            h.update(bytes);
        }
        fn values(h: &mut Sha256, server: &McpServer, map: &BTreeMap<String, McpValue>, secrets: &dyn McpSecrets) {
            for (name, v) in map {
                field(h, "n", name.as_bytes());
                match v {
                    McpValue::Plain { value } => field(h, "p", value.as_bytes()),
                    McpValue::Secret { name: secret } => {
                        field(h, "r", secret.as_bytes());
                        match secrets.get(&server.id, secret) {
                            Some(value) => field(h, "s", value.as_bytes()),
                            None => field(h, "m", b""),
                        }
                    }
                }
            }
        }
        let mut h = Sha256::new();
        match &server.transport {
            McpTransport::Stdio { command, args, env } => {
                field(&mut h, "kind", b"stdio");
                field(&mut h, "command", command.as_bytes());
                for a in args {
                    field(&mut h, "arg", a.as_bytes());
                }
                values(&mut h, server, env, secrets);
            }
            McpTransport::Http { url, headers } => {
                field(&mut h, "kind", b"http");
                field(&mut h, "url", url.as_bytes());
                values(&mut h, server, headers, secrets);
            }
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Whether `server` may start on this machine: the user approved exactly this server.
    pub fn is_approved(&self, server: &McpServer, secrets: &dyn McpSecrets) -> bool {
        self.approved.get(&server.id) == Some(&Self::fingerprint(server, secrets))
    }

    /// Records the user's OK for this server as it is now.
    pub fn approve(&mut self, server: &McpServer, secrets: &dyn McpSecrets) {
        self.approved.insert(server.id.clone(), Self::fingerprint(server, secrets));
    }

    /// Forgets the OK (the server was removed, or the user withdrew it).
    pub fn revoke(&mut self, server_id: &str) {
        self.approved.remove(server_id);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::mcp::McpValue;

    fn stdio(args: &[&str], env: &[(&str, &str)]) -> McpServer {
        let env: BTreeMap<String, McpValue> = env.iter().map(|(k, v)| (k.to_string(), McpValue::Plain { value: v.to_string() })).collect();
        McpServer::new("S", McpTransport::Stdio { command: "srv".into(), args: args.iter().map(|a| a.to_string()).collect(), env })
    }

    #[test]
    fn file_secrets_persist_privately_and_never_print() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local/mcp-secrets.json");
        let s = FileSecrets::open(&path);
        assert_eq!(s.get("a", "K"), None);
        s.set("a", "K", "v1");
        s.set("a", "J", "v2");
        s.set("b", "K", "v3");
        assert_eq!(s.names("a"), ["J", "K"]);
        let reopened = FileSecrets::open(&path);
        assert_eq!(reopened.get("a", "K").as_deref(), Some("v1"));
        assert!(!format!("{reopened:?}").contains("v1"));
        reopened.remove_server("a");
        assert_eq!(FileSecrets::open(&path).get("a", "K"), None);
        assert_eq!(FileSecrets::open(&path).get("b", "K").as_deref(), Some("v3"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn memory_secrets_work_like_the_file() {
        let m = MemorySecrets::default();
        m.set("a", "K", "v");
        assert_eq!(m.get("a", "K").as_deref(), Some("v"));
        m.remove_server("a");
        assert_eq!(m.get("a", "K"), None);
    }

    fn secret_env(name: &str) -> McpValue {
        McpValue::Secret { name: name.into() }
    }

    fn with_env(mut s: McpServer, env: &[(&str, McpValue)]) -> McpServer {
        s.transport = McpTransport::Stdio {
            command: "srv".into(),
            args: vec!["--x".into()],
            env: env.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        };
        s
    }

    #[test]
    fn any_change_to_what_a_server_runs_needs_approval_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-approved.json");
        let none = MemorySecrets::default();
        let base = stdio(&["--x"], &[("NODE_OPTIONS", "--max-old-space-size=64")]);
        let mut a = McpApprovals::load(&path);
        assert!(!a.is_approved(&base, &none));
        a.approve(&base, &none);
        a.save(&path).unwrap();
        let a = McpApprovals::load(&path);
        assert!(a.is_approved(&base, &none));
        let changed = |f: &dyn Fn(&mut McpServer)| {
            let mut s = base.clone();
            f(&mut s);
            s
        };
        // A synced change of an environment value, name, argument or program all ask again.
        let node_options = changed(&|s| s.transport = stdio(&["--x"], &[("NODE_OPTIONS", "--require /tmp/evil.js")]).transport);
        assert!(!a.is_approved(&node_options, &none), "an env value changed");
        assert!(!a.is_approved(&changed(&|s| s.transport = stdio(&["--x"], &[("OTHER", "--max-old-space-size=64")]).transport), &none));
        assert!(!a.is_approved(&changed(&|s| s.transport = stdio(&["--y"], &[("NODE_OPTIONS", "--max-old-space-size=64")]).transport), &none));
        let McpTransport::Stdio { args, env, .. } = &base.transport else { unreachable!() };
        let other_program = changed(&|s| s.transport = McpTransport::Stdio { command: "evil".into(), args: args.clone(), env: env.clone() });
        assert!(!a.is_approved(&other_program, &none));
        // Arguments are not run together: ["a b"] and ["a", "b"] differ.
        let mut x = a.clone();
        x.approve(&stdio(&["a b"], &[]), &none);
        assert!(!x.is_approved(
            &{
                let mut s = stdio(&["a", "b"], &[]);
                s.id = base.id.clone();
                s
            },
            &none
        ));
        // Name and value are not run together either.
        let one = with_env(base.clone(), &[("AB", McpValue::Plain { value: "C".into() })]);
        let two = with_env(base.clone(), &[("A", McpValue::Plain { value: "BC".into() })]);
        assert_ne!(McpApprovals::fingerprint(&one, &none), McpApprovals::fingerprint(&two, &none));
        let mut a = a;
        a.revoke(&base.id);
        assert!(!a.is_approved(&base, &none));
    }

    #[test]
    fn a_changed_secret_asks_again_and_no_value_is_stored_in_the_approvals_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-approved.json");
        let secrets = MemorySecrets::default();
        let s =
            with_env(stdio(&[], &[]), &[("API_TOKEN", secret_env("API_TOKEN")), ("MODE", McpValue::Plain { value: "plain-visible-value".into() })]);
        secrets.set(&s.id, "API_TOKEN", "sk-live-very-secret");
        let mut a = McpApprovals::default();
        a.approve(&s, &secrets);
        a.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("sk-live-very-secret") && !text.contains("plain-visible-value") && !text.contains("API_TOKEN"), "{text}");
        assert!(a.is_approved(&s, &secrets));
        secrets.set(&s.id, "API_TOKEN", "sk-live-another");
        assert!(!a.is_approved(&s, &secrets), "a different secret value is a change");
        secrets.set(&s.id, "API_TOKEN", "sk-live-very-secret");
        assert!(a.is_approved(&s, &secrets));
        secrets.remove_server(&s.id);
        assert!(!a.is_approved(&s, &secrets), "a secret this device lacks is a change");
    }

    #[test]
    fn an_http_server_is_approved_for_its_url_and_headers_too() {
        let secrets = MemorySecrets::default();
        let mut http = McpServer::new("H", McpTransport::Http { url: "https://docs.example/mcp".into(), headers: BTreeMap::new() });
        assert!(!McpApprovals::default().is_approved(&http, &secrets), "a new HTTP server asks first too");
        let mut a = McpApprovals::default();
        a.approve(&http, &secrets);
        assert!(a.is_approved(&http, &secrets));
        // A synced change of the URL would send the bearer token somewhere new: it asks again.
        http.transport = McpTransport::Http { url: "https://evil.example/mcp".into(), headers: BTreeMap::new() };
        assert!(!a.is_approved(&http, &secrets));
        http.transport = McpTransport::Http {
            url: "https://docs.example/mcp".into(),
            headers: BTreeMap::from([("X-Team".to_string(), McpValue::Plain { value: "a".into() })]),
        };
        assert!(!a.is_approved(&http, &secrets), "a header changed");
    }
}
