//! Connected MCP servers for the Companion panel and Settings > Agents.
//!
//! What syncs is `companions/mcp.json` ([`McpConfigs`], without secret values). What stays on this
//! machine lives beside the profile, outside `companions/`: the secret values ([`LocalSecrets`]:
//! a user-only file for now; the keystore can replace it through `McpSecrets`) and the user's OK to
//! start each local command (`mcp-approved.local.json`).
//!
//! [`McpManager`] connects the enabled, approved servers on its own thread and keeps the resulting
//! registry and every server's status; the panel takes the registry at each task and Settings reads
//! the statuses. The functions at the top change configs without any window, so they test alone.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use morph_ai::mcp_runtime::{ConnectContext, ServerStatus, connect};
use morph_ai::{AiStore, FileStore, McpApprovals, McpConfigs, McpSecrets, McpServer, McpTransport, McpValue, write_private};
use weirdnet_agent::ToolRegistry;

/// This device as `McpServer::devices` names it. Configs made here run everywhere (`devices` is
/// `None`), so until devices have ids in Morph this only matters for configs synced from elsewhere.
const DEVICE: &str = "this-device";

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `<profile>`, the parent of `<profile>/companions`.
pub fn profile_dir(companions: &Path) -> PathBuf {
    companions.parent().map(Path::to_path_buf).unwrap_or_else(|| companions.to_path_buf())
}

fn approvals_path(companions: &Path) -> PathBuf {
    profile_dir(companions).join("mcp-approved.local.json")
}

fn secrets_path(companions: &Path) -> PathBuf {
    profile_dir(companions).join("mcp-secrets.local.json")
}

type Index = BTreeMap<String, BTreeMap<String, String>>;

/// This machine's MCP secrets, in `mcp-secrets.local.json` beside the profile (readable by the user
/// only, never synced, outside `companions/`). It implements [`McpSecrets`], so the system
/// keystore can take over by implementing the same trait once it holds MCP secrets.
pub struct LocalSecrets {
    companions: PathBuf,
    index: Mutex<Index>,
}

impl std::fmt::Debug for LocalSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSecrets").finish_non_exhaustive()
    }
}

impl LocalSecrets {
    /// Secrets for the profile whose companions directory is `companions`.
    pub fn open(companions: &Path) -> Self {
        let index = std::fs::read(secrets_path(companions)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { companions: companions.to_path_buf(), index: Mutex::new(index) }
    }

    fn save(&self, index: &Index) {
        if let Ok(bytes) = serde_json::to_vec_pretty(index) {
            let _ = write_private(&secrets_path(&self.companions), &bytes);
        }
    }

    /// Forgets one secret.
    pub fn remove(&self, server: &str, name: &str) {
        let mut index = lock(&self.index);
        if let Some(m) = index.get_mut(server) {
            m.remove(name);
        }
        index.retain(|_, m| !m.is_empty());
        self.save(&index);
    }
}

impl McpSecrets for LocalSecrets {
    fn get(&self, server: &str, name: &str) -> Option<String> {
        lock(&self.index).get(server)?.get(name).cloned()
    }

    fn set(&self, server: &str, name: &str, value: &str) {
        let mut index = lock(&self.index);
        index.entry(server.to_string()).or_default().insert(name.to_string(), value.to_string());
        self.save(&index);
    }

    fn remove_server(&self, server: &str) {
        let mut index = lock(&self.index);
        index.remove(server);
        self.save(&index);
    }
}

fn store(companions: &Path) -> FileStore {
    FileStore::new(companions)
}

/// The configured servers (none when nothing is saved).
pub fn configs(companions: &Path) -> McpConfigs {
    store(companions).load_mcp().ok().flatten().unwrap_or_default()
}

fn save_configs(companions: &Path, configs: &McpConfigs) -> Result<(), String> {
    store(companions).save_mcp(configs).map_err(|e| e.to_string())
}

/// Adds or replaces a server. `new_secrets` are values the user typed for entries that are
/// secrets (`McpValue::Secret` entries in `server`); they go to `secrets`. Plain values under
/// names that look secret (`TOKEN`, `Authorization`) are moved there too. Secrets the server no
/// longer refers to are forgotten.
pub fn save_server(companions: &Path, secrets: &LocalSecrets, mut server: McpServer, new_secrets: &[(String, String)]) -> Result<(), String> {
    if server.name.trim().is_empty() {
        return Err("Give the server a name.".into());
    }
    match &server.transport {
        morph_ai::McpTransport::Stdio { command, .. } if command.trim().is_empty() => return Err("Enter the command to run.".into()),
        morph_ai::McpTransport::Http { url, .. } if !(url.starts_with("https://") || url.starts_with("http://")) => {
            return Err("The URL must start with https:// (or http:// for a server on this machine).".into());
        }
        _ => {}
    }
    for (name, value) in new_secrets {
        secrets.set(&server.id, name, value);
    }
    for (name, value) in server.take_secrets() {
        secrets.set(&server.id, &name, &value);
    }
    let mut all = configs(companions);
    if let Some(old) = all.servers.iter().find(|s| s.id == server.id) {
        let keep = server.secret_names();
        for name in old.secret_names().into_iter().filter(|n| !keep.contains(n)) {
            secrets.remove(&server.id, &name);
        }
    }
    all.upsert(server);
    save_configs(companions, &all)
}

/// Removes a server with its secrets and its approval.
pub fn remove_server(companions: &Path, secrets: &LocalSecrets, id: &str) -> Result<(), String> {
    let mut all = configs(companions);
    all.remove(id);
    save_configs(companions, &all)?;
    secrets.remove_server(id);
    let mut approvals = McpApprovals::load(&approvals_path(companions));
    approvals.revoke(id);
    approvals.save(&approvals_path(companions)).map_err(|e| e.to_string())
}

fn edit_server(companions: &Path, id: &str, f: impl FnOnce(&mut McpServer)) -> Result<(), String> {
    let mut all = configs(companions);
    let server = all.servers.iter_mut().find(|s| s.id == id).ok_or("That server is gone.")?;
    f(server);
    save_configs(companions, &all)
}

pub fn set_enabled(companions: &Path, id: &str, on: bool) -> Result<(), String> {
    edit_server(companions, id, |s| s.enabled = on)
}

/// Switches one of a server's tools on or off (by the server's own name for it).
pub fn set_tool(companions: &Path, id: &str, tool: &str, on: bool) -> Result<(), String> {
    edit_server(companions, id, |s| {
        if on {
            s.tools_off.remove(tool);
        } else {
            s.tools_off.insert(tool.to_string());
        }
    })
}

/// The user's OK to start this server on this machine, for the server as it is saved now.
pub fn approve(companions: &Path, secrets: &LocalSecrets, id: &str) -> Result<(), String> {
    let server = configs(companions).servers.into_iter().find(|s| s.id == id).ok_or("That server is gone.")?;
    let path = approvals_path(companions);
    let mut approvals = McpApprovals::load(&path);
    approvals.approve(&server, secrets);
    approvals.save(&path).map_err(|e| e.to_string())
}

pub fn is_approved(companions: &Path, secrets: &LocalSecrets, server: &McpServer) -> bool {
    McpApprovals::load(&approvals_path(companions)).is_approved(server, secrets)
}

/// What the manager knows right now.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub statuses: BTreeMap<String, ServerStatus>,
    /// A connect round is running.
    pub busy: bool,
    /// Counts finished rounds, so a view can tell something changed.
    pub generation: u64,
}

struct State {
    registry: Arc<ToolRegistry>,
    scoped: BTreeMap<String, Arc<ToolRegistry>>,
    snapshot: Snapshot,
}

/// What the Companion worker needs from the manager to run tasks with MCP tools.
pub trait ToolsSource: Send + Sync {
    /// The registry to use for `companion`'s next task.
    fn registry_for(&self, companion: &str) -> Arc<ToolRegistry>;
    /// The runtime the registry's servers live on. Tasks run on it, so server connections (child
    /// pipes, HTTP connections) are driven the whole time, not only while a task runs.
    fn handle(&self) -> Option<tokio::runtime::Handle>;
}

/// Connects the profile's MCP servers on its own thread and keeps the result.
pub struct McpManager {
    secrets: Arc<LocalSecrets>,
    trigger: Mutex<Sender<()>>,
    state: Arc<Mutex<State>>,
    handle: tokio::runtime::Handle,
}

type MayStart = Arc<dyn Fn(&Path, &str) -> bool + Send + Sync>;

/// What the manager asks the sync layer. The defaults are Lead Dev's; tests give their own.
#[derive(Clone)]
struct Hooks {
    /// Whether a synced local command may start (a linked, unrevoked device wrote its config).
    may_start: MayStart,
    /// Changes whenever synced data or a revocation was applied.
    revision: Arc<dyn Fn() -> u64 + Send + Sync>,
    /// The profile is locked (password not entered): no server runs for it.
    locked: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
}

impl Hooks {
    fn sync() -> Self {
        Self {
            may_start: Arc::new(morph_services::sync::mcp_may_start_in),
            revision: Arc::new(morph_services::sync::applied_revision),
            locked: Arc::new(morph_services::profile_lock::is_locked_in),
        }
    }
}

impl McpManager {
    fn start(companions: &Path, hooks: Hooks) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<()>();
        let state = Arc::new(Mutex::new(State { registry: Arc::new(ToolRegistry::new()), scoped: BTreeMap::new(), snapshot: Snapshot::default() }));
        let secrets = Arc::new(LocalSecrets::open(companions));
        // Sync reads and writes these same secrets (a sealed record type), not a second copy.
        let shared: Arc<dyn McpSecrets> = secrets.clone();
        morph_services::companion_sync::share_secrets(&profile_dir(companions), &shared);
        // One worker thread drives the servers' pipes and connections for as long as they live.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("morph-mcp-io")
            .enable_all()
            .build()
            .expect("build the MCP runtime");
        let handle = rt.handle().clone();
        let manager = Arc::new(Self { secrets: secrets.clone(), trigger: Mutex::new(tx), state: state.clone(), handle });
        let dir = companions.to_path_buf();
        let watch = (Arc::downgrade(&manager), hooks.revision.clone(), hooks.locked.clone(), dir.clone());
        let hooks = hooks.clone();
        std::thread::Builder::new()
            .name("morph-mcp".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    while rx.try_recv().is_ok() {}
                    lock(&state).snapshot.busy = true;
                    let mut configs = configs(&dir);
                    // A locked profile runs no servers at all (Lead Dev's profile lock).
                    if (hooks.locked)(&dir) {
                        configs.servers.clear();
                    }
                    // A synced local command never starts unless this device or a linked,
                    // unrevoked one wrote its config (Lead Dev's sync layer decides).
                    for s in configs.servers.iter_mut() {
                        if matches!(s.transport, McpTransport::Stdio { .. }) && !(hooks.may_start)(&dir, &s.id) {
                            s.enabled = false;
                        }
                    }
                    let approvals = McpApprovals::load(&approvals_path(&dir));
                    let ctx = ConnectContext { secrets: &*secrets, approvals: &approvals, device: DEVICE, timeout: Duration::from_secs(15) };
                    let hub = rt.block_on(connect(&configs, &ctx));
                    // Dropping the old servers stops their processes; do it inside the runtime.
                    let _inside = rt.enter();
                    let mut s = lock(&state);
                    let old = (std::mem::replace(&mut s.registry, hub.registry), std::mem::replace(&mut s.scoped, hub.scoped));
                    s.snapshot = Snapshot { statuses: hub.statuses, busy: false, generation: s.snapshot.generation + 1 };
                    drop(s);
                    drop(old);
                }
            })
            .expect("spawn the MCP thread");
        // A revocation (or any synced change) re-checks every server at once: a running local
        // command whose writer was just revoked stops now, not at the next reload.
        std::thread::Builder::new()
            .name("morph-mcp-watch".into())
            .spawn(move || {
                let (manager, revision, locked, dir) = watch;
                let mut seen = (revision(), locked(&dir));
                // The companions as of the last change, so one that vanished from the roster
                // (a removal through sync, or whatever bumps the same counter) leaves the
                // servers' scopes as `remove_companion` does.
                let mut companions = Default::default();
                let _ = morph_ai::forget_vanished_companions(&store(&dir), &mut companions);
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let Some(m) = manager.upgrade() else { return };
                    let now = (revision(), locked(&dir));
                    if now != seen {
                        seen = now;
                        if let Err(e) = morph_ai::forget_vanished_companions(&store(&dir), &mut companions) {
                            eprintln!("morph-mcp: scopes: {e}");
                        }
                        m.reload();
                    }
                }
            })
            .expect("spawn the MCP watcher");
        manager
    }

    /// This machine's secrets for the profile.
    pub fn secrets(&self) -> &Arc<LocalSecrets> {
        &self.secrets
    }

    /// Connect again with what is saved now (after a config, secret or approval changed).
    pub fn reload(&self) {
        lock(&self.state).snapshot.busy = true;
        let _ = lock(&self.trigger).send(());
    }

    /// The tools `companion` may use. A new registry replaces it after each connect round.
    pub fn registry_for(&self, companion: &str) -> Arc<ToolRegistry> {
        let s = lock(&self.state);
        s.scoped.get(companion).unwrap_or(&s.registry).clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.state).snapshot.clone()
    }
}

impl ToolsSource for McpManager {
    fn registry_for(&self, companion: &str) -> Arc<ToolRegistry> {
        McpManager::registry_for(self, companion)
    }

    fn handle(&self) -> Option<tokio::runtime::Handle> {
        Some(self.handle.clone())
    }
}

/// The manager for a profile's companions directory if one is running.
pub fn peek(companions: &Path) -> Option<Arc<McpManager>> {
    static_managers().lock().unwrap_or_else(|e| e.into_inner()).get(companions).cloned()
}

fn static_managers() -> &'static Mutex<HashMap<PathBuf, Arc<McpManager>>> {
    static MANAGERS: OnceLock<Mutex<HashMap<PathBuf, Arc<McpManager>>>> = OnceLock::new();
    MANAGERS.get_or_init(Default::default)
}

/// The manager for a profile's companions directory; the first call starts it and connects.
pub fn get(companions: &Path) -> Arc<McpManager> {
    let mut all = lock(static_managers());
    all.entry(companions.to_path_buf())
        .or_insert_with(|| {
            let m = McpManager::start(companions, Hooks::sync());
            m.reload();
            m
        })
        .clone()
}

/// A plain value (never a secret) for a header or environment entry.
pub fn plain(value: &str) -> McpValue {
    McpValue::Plain { value: value.to_string() }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use morph_ai::McpTransport;

    use super::*;

    const SCRIPT: &str = r#"while read l; do
  id=$(echo "$l" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$l" in
    *'"initialize"'*) echo '{"jsonrpc":"2.0","id":'$id',"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"sh","version":"1"}}}' ;;
    *'"tools/list"'*) echo '{"jsonrpc":"2.0","id":'$id',"result":{"tools":[{"name":"'"$MCP_NAME"'_a","annotations":{"readOnlyHint":true}},{"name":"'"$MCP_NAME"'_b"}]}}' ;;
  esac
done"#;

    fn dir() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let companions = d.path().join("profiles/default/companions");
        std::fs::create_dir_all(&companions).unwrap();
        (d, companions)
    }

    fn server(name: &str, env: BTreeMap<String, McpValue>) -> McpServer {
        McpServer::new(name, McpTransport::Stdio { command: "sh".into(), args: vec!["-c".into(), SCRIPT.into()], env })
    }

    fn wait_for(m: &McpManager, ok: impl Fn(&Snapshot) -> bool) -> Snapshot {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let s = m.snapshot();
            if (!s.busy && ok(&s)) || Instant::now() > end {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn secrets_go_to_a_private_file_and_never_to_the_synced_config() {
        let (d, companions) = dir();
        let secrets = LocalSecrets::open(&companions);
        let mut env = BTreeMap::new();
        env.insert("API_TOKEN".to_string(), plain("sk-live-123"));
        env.insert("MODE".to_string(), plain("fast"));
        env.insert("PLAIN_BUT_SECRET".to_string(), McpValue::Secret { name: "PLAIN_BUT_SECRET".into() });
        let s = server("S", env);
        save_server(&companions, &secrets, s.clone(), &[("PLAIN_BUT_SECRET".into(), "typed-secret".into())]).unwrap();
        let synced = std::fs::read_to_string(companions.join("mcp.json")).unwrap();
        assert!(!synced.contains("sk-live-123") && !synced.contains("typed-secret"), "{synced}");
        assert!(synced.contains("fast"));
        assert_eq!(secrets.get(&s.id, "API_TOKEN").as_deref(), Some("sk-live-123"));
        assert_eq!(secrets.get(&s.id, "PLAIN_BUT_SECRET").as_deref(), Some("typed-secret"));
        // Dropping a variable forgets its secret.
        let mut edited = configs(&companions).servers[0].clone();
        if let McpTransport::Stdio { env, .. } = &mut edited.transport {
            env.remove("API_TOKEN");
        }
        save_server(&companions, &secrets, edited, &[]).unwrap();
        assert_eq!(secrets.get(&s.id, "API_TOKEN"), None);
        assert_eq!(secrets.get(&s.id, "PLAIN_BUT_SECRET").as_deref(), Some("typed-secret"));
        remove_server(&companions, &secrets, &s.id).unwrap();
        assert!(configs(&companions).servers.is_empty());
        assert_eq!(secrets.get(&s.id, "PLAIN_BUT_SECRET"), None);
        drop(d);
    }

    #[test]
    fn values_stay_in_a_user_only_file_on_this_machine() {
        let (_d, companions) = dir();
        let secrets = LocalSecrets::open(&companions);
        secrets.set("srv", "TOKEN", "t-1");
        assert_eq!(LocalSecrets::open(&companions).get("srv", "TOKEN").as_deref(), Some("t-1"));
        let path = secrets_path(&companions);
        assert!(path.starts_with(profile_dir(&companions)) && !path.starts_with(&companions), "outside the synced companions dir");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(!format!("{secrets:?}").contains("t-1"));
    }

    #[test]
    fn validation_rejects_empty_names_commands_and_bad_urls() {
        let (_d, companions) = dir();
        let secrets = LocalSecrets::open(&companions);
        let bad_name = McpServer::new(" ", McpTransport::Http { url: "https://x".into(), headers: BTreeMap::new() });
        assert!(save_server(&companions, &secrets, bad_name, &[]).is_err());
        let bad_cmd = McpServer::new("a", McpTransport::Stdio { command: " ".into(), args: vec![], env: BTreeMap::new() });
        assert!(save_server(&companions, &secrets, bad_cmd, &[]).unwrap_err().contains("command"));
        let bad_url = McpServer::new("a", McpTransport::Http { url: "ftp://x".into(), headers: BTreeMap::new() });
        assert!(save_server(&companions, &secrets, bad_url, &[]).unwrap_err().contains("https://"));
        assert!(configs(&companions).servers.is_empty());
    }

    /// A manager whose sync answers are the test's.
    fn manager_with(
        companions: &Path,
        allowed: &Arc<std::sync::atomic::AtomicBool>,
        revision: &Arc<std::sync::atomic::AtomicU64>,
    ) -> Arc<McpManager> {
        use std::sync::atomic::Ordering::SeqCst;
        let (allowed, revision) = (allowed.clone(), revision.clone());
        let hooks = Hooks {
            may_start: Arc::new(move |_, _| allowed.load(SeqCst)),
            revision: Arc::new(move || revision.load(SeqCst)),
            locked: Arc::new(|_| false),
        };
        let m = McpManager::start(companions, hooks);
        m.reload();
        m
    }

    #[cfg(target_os = "linux")]
    fn alive(pid: &str) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| !s.rsplit(") ").next().unwrap_or("").starts_with('Z'))
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_revocation_stops_a_running_local_server_at_once_and_never_restarts_it() {
        use std::sync::atomic::Ordering::SeqCst;
        let (d, companions) = dir();
        let (allowed, revision) = (Arc::new(std::sync::atomic::AtomicBool::new(true)), Arc::new(std::sync::atomic::AtomicU64::new(1)));
        let manager = manager_with(&companions, &allowed, &revision);
        let secrets = manager.secrets().clone();
        let pidfile = d.path().join("pid");
        let mut env = BTreeMap::new();
        env.insert("MCP_NAME".to_string(), plain("sync"));
        let mut s = server("Synced", env);
        if let McpTransport::Stdio { args, .. } = &mut s.transport {
            args[1] = format!("echo $$ > '{}'; exec sh -c '{}'", pidfile.display(), SCRIPT.replace('\'', "'\\''"));
        }
        save_server(&companions, &secrets, s.clone(), &[]).unwrap();
        approve(&companions, &secrets, &s.id).unwrap();
        manager.reload();
        let id = s.id.clone();
        wait_for(&manager, |snap| matches!(snap.statuses.get(&id), Some(ServerStatus::Connected { .. })));
        assert_eq!(manager.registry_for("local").specs().len(), 2);
        let pid = std::fs::read_to_string(&pidfile).unwrap().trim().to_string();
        assert!(alive(&pid), "the server runs");

        // The writer of this config is revoked: the sync layer says no and bumps its revision.
        // Nothing reloads the manager by hand.
        allowed.store(false, SeqCst);
        revision.fetch_add(1, SeqCst);
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end && (alive(&pid) || !manager.registry_for("local").specs().is_empty()) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!alive(&pid), "the process was stopped");
        assert!(manager.registry_for("local").specs().is_empty(), "its tools are gone from the sessions");
        let snap = wait_for(&manager, |snap| snap.statuses.get(&id) == Some(&ServerStatus::Off));
        assert_eq!(snap.statuses[&id], ServerStatus::Off);
        // Later reloads keep it off while the sync layer refuses it.
        manager.reload();
        wait_for(&manager, |_| true);
        assert!(manager.registry_for("local").specs().is_empty());
    }

    #[test]
    fn a_companion_removed_through_sync_leaves_the_server_scopes() {
        use std::sync::atomic::Ordering::SeqCst;
        let (_d, companions) = dir();
        let mut roster = morph_ai::Roster::first_run(morph_ai::ModelChoice::Gemma4E2B);
        roster.add(morph_ai::CompanionProfile::local("scout", "Scout", morph_ai::ModelChoice::Gemma4E2B)).unwrap();
        store(&companions).save_roster(&roster).unwrap();
        let mut only = McpServer::new("Only scout", McpTransport::Http { url: "https://mcp.example/".into(), headers: BTreeMap::new() });
        only.enabled = false;
        only.companions = Some(["scout".to_string()].into());
        let mut all = McpConfigs::default();
        all.upsert(only.clone());
        save_configs(&companions, &all).unwrap();
        let (allowed, revision) = (Arc::new(std::sync::atomic::AtomicBool::new(true)), Arc::new(std::sync::atomic::AtomicU64::new(1)));
        let _manager = manager_with(&companions, &allowed, &revision);
        std::thread::sleep(Duration::from_millis(400));

        // Sync rewrites the roster without scout and bumps the counter; nothing calls
        // remove_companion.
        roster.remove(&weirdnet_agent::AgentId::new("scout")).unwrap();
        store(&companions).save_roster(&roster).unwrap();
        revision.fetch_add(1, SeqCst);
        let scope = || configs(&companions).servers.into_iter().find(|s| s.id == only.id).unwrap().companions;
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end && scope() != Some(Default::default()) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(scope(), Some(Default::default()), "a server only for scout is for nobody now, not for everyone");
    }

    #[test]
    fn a_server_limited_to_a_companion_reaches_only_that_companions_sessions() {
        let (_d, companions) = dir();
        let manager = get(&companions);
        let secrets = manager.secrets().clone();
        let mut env = BTreeMap::new();
        env.insert("MCP_NAME".to_string(), plain("wiki"));
        let mut s = server("Wiki", env);
        s.companions = Some(["scout".to_string()].into());
        save_server(&companions, &secrets, s.clone(), &[]).unwrap();
        approve(&companions, &secrets, &s.id).unwrap();
        manager.reload();
        let id = s.id.clone();
        wait_for(&manager, |snap| matches!(snap.statuses.get(&id), Some(ServerStatus::Connected { .. })));
        let names = |who: &str| manager.registry_for(who).specs().into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert_eq!(names("scout"), ["wiki_wiki_a", "wiki_wiki_b"]);
        assert!(names("local").is_empty(), "another companion has no tools from it");
        // Saving the form with the scope cleared opens it to everyone again.
        let mut open = configs(&companions).servers[0].clone();
        open.companions = None;
        save_server(&companions, &secrets, open, &[]).unwrap();
        manager.reload();
        for _ in 0..200 {
            if !names("local").is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(names("local"), ["wiki_wiki_a", "wiki_wiki_b"]);
    }

    #[test]
    fn the_manager_waits_for_approval_connects_and_honours_tool_switches() {
        let (_d, companions) = dir();
        let manager = get(&companions);
        let secrets = manager.secrets().clone();
        let mut env = BTreeMap::new();
        env.insert("MCP_NAME".to_string(), plain("docs"));
        let s = server("Docs", env);
        save_server(&companions, &secrets, s.clone(), &[]).unwrap();
        manager.reload();
        let snap = wait_for(&manager, |s| s.statuses.len() == 1);
        assert!(matches!(snap.statuses[&s.id], ServerStatus::NeedsApproval { .. }), "{:?}", snap.statuses);
        assert!(manager.registry_for("local").specs().is_empty(), "nothing starts before the user agrees");
        assert!(!is_approved(&companions, &secrets, &configs(&companions).servers[0]));

        approve(&companions, &secrets, &s.id).unwrap();
        manager.reload();
        let id = s.id.clone();
        let snap = wait_for(&manager, |snap| matches!(snap.statuses.get(&id), Some(ServerStatus::Connected { .. })));
        assert!(matches!(snap.statuses[&s.id], ServerStatus::Connected { .. }), "{:?}", snap.statuses);
        let names: Vec<String> = manager.registry_for("local").specs().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["docs_docs_a", "docs_docs_b"]);

        set_tool(&companions, &s.id, "docs_b", false).unwrap();
        manager.reload();
        wait_for(&manager, |_| true);
        let names: Vec<String> = manager.registry_for("local").specs().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["docs_docs_a"]);

        // Changing the command asks again; switching off and removing work.
        let mut changed = configs(&companions).servers[0].clone();
        if let McpTransport::Stdio { args, .. } = &mut changed.transport {
            args.push("--extra".into());
        }
        save_server(&companions, &secrets, changed, &[]).unwrap();
        manager.reload();
        let snap = wait_for(&manager, |s| s.statuses.values().any(|v| matches!(v, ServerStatus::NeedsApproval { .. })));
        assert!(matches!(snap.statuses[&s.id], ServerStatus::NeedsApproval { .. }));
        set_enabled(&companions, &s.id, false).unwrap();
        manager.reload();
        let snap = wait_for(&manager, |s| s.statuses.values().any(|v| *v == ServerStatus::Off));
        assert_eq!(snap.statuses[&s.id], ServerStatus::Off);
        remove_server(&companions, &secrets, &s.id).unwrap();
        manager.reload();
        let snap = wait_for(&manager, |s| s.statuses.is_empty());
        assert!(snap.statuses.is_empty());
    }
}
