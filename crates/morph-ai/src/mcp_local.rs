//! What stays on one machine for MCP servers: secret values and the user's OK to start a local
//! command. Neither is ever part of [`crate::mcp::McpConfigs`], so neither syncs.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::mcp::{McpSecrets, McpServer, McpTransport};

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Writes `bytes` atomically, readable by the user only.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
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
    cache: Mutex<BTreeMap<String, BTreeMap<String, String>>>,
}

impl std::fmt::Debug for FileSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSecrets").field("path", &self.path).finish_non_exhaustive()
    }
}

impl FileSecrets {
    /// Opens (reading it now if it exists) the file at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let cache = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { path, cache: Mutex::new(cache) }
    }

    fn save(&self, cache: &BTreeMap<String, BTreeMap<String, String>>) {
        if let Ok(bytes) = serde_json::to_vec_pretty(cache) {
            let _ = write_private(&self.path, &bytes);
        }
    }

    /// Whether anything is kept for `server` (names only, never values).
    pub fn names(&self, server: &str) -> Vec<String> {
        lock(&self.cache).get(server).map(|m| m.keys().cloned().collect()).unwrap_or_default()
    }
}

impl McpSecrets for FileSecrets {
    fn get(&self, server: &str, name: &str) -> Option<String> {
        lock(&self.cache).get(server).and_then(|m| m.get(name)).cloned()
    }
    fn set(&self, server: &str, name: &str, value: &str) {
        let mut c = lock(&self.cache);
        c.entry(server.to_string()).or_default().insert(name.to_string(), value.to_string());
        self.save(&c);
    }
    fn remove_server(&self, server: &str) {
        let mut c = lock(&self.cache);
        if c.remove(server).is_some() {
            self.save(&c);
        }
    }
}

/// What the user agreed to start on this machine: a local-command server runs only after the user
/// has seen its warning, and again if its command or arguments change. Saved per machine.
#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct McpApprovals {
    /// Server id to the fingerprint that was approved.
    #[serde(default)]
    approved: BTreeMap<String, String>,
}

impl McpApprovals {
    /// Reads the file; a missing or unreadable file approves nothing.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Writes the file (readable by the user only).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        write_private(path, &serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)
    }

    /// What a local command is approved as: the program, its arguments and the *names* of its
    /// environment variables (never values, so changing a token does not ask again). `None` for
    /// HTTP servers, which need no confirmation.
    pub fn fingerprint(server: &McpServer) -> Option<String> {
        match &server.transport {
            McpTransport::Stdio { command, args, env } => {
                let names: Vec<&str> = env.keys().map(String::as_str).collect();
                Some(format!("{command}\u{1f}{}\u{1e}{}", args.join("\u{1f}"), names.join(",")))
            }
            McpTransport::Http { .. } => None,
        }
    }

    /// Whether `server` may start on this machine.
    pub fn is_approved(&self, server: &McpServer) -> bool {
        match Self::fingerprint(server) {
            None => true,
            Some(fp) => self.approved.get(&server.id) == Some(&fp),
        }
    }

    /// Records the user's OK for this server as it is now.
    pub fn approve(&mut self, server: &McpServer) {
        if let Some(fp) = Self::fingerprint(server) {
            self.approved.insert(server.id.clone(), fp);
        }
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

    #[test]
    fn a_local_command_needs_approval_again_when_it_changes_but_not_for_a_new_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-approved.json");
        let mut s = stdio(&["--x"], &[("TOKEN", "1")]);
        let mut a = McpApprovals::load(&path);
        assert!(!a.is_approved(&s));
        a.approve(&s);
        a.save(&path).unwrap();
        let a = McpApprovals::load(&path);
        assert!(a.is_approved(&s));
        // A new secret value does not ask again; a new argument or variable name does.
        s.transport = stdio(&["--x"], &[("TOKEN", "2")]).transport;
        assert!(a.is_approved(&s));
        s.transport = stdio(&["--y"], &[("TOKEN", "2")]).transport;
        assert!(!a.is_approved(&s));
        s.transport = stdio(&["--x"], &[("OTHER", "2")]).transport;
        assert!(!a.is_approved(&s));
        let mut a = a;
        a.revoke(&s.id);
        assert!(!a.is_approved(&stdio(&["--x"], &[("TOKEN", "1")])));
        let http = McpServer::new("H", McpTransport::Http { url: "https://x".into(), headers: BTreeMap::new() });
        assert!(McpApprovals::default().is_approved(&http), "HTTP servers need no confirmation");
    }
}
