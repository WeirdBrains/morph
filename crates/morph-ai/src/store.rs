//! Persistence for the roster and the policy (grants and standing allows) behind one interface.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use weirdnet_agent::{AgentId, AllowRule, CompanionScope, Policy, SharedPolicy, SiteLevel, StandingAllow};

use crate::chats::ChatLog;
use crate::file_io::{FileIo, PlainIo};
use crate::journal::Journals;
use crate::mcp::McpConfigs;
use crate::roster::Roster;
use crate::settings::AgentSettings;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage failed: {0}")]
    Io(String),
    #[error("stored data is unreadable: {0}")]
    Corrupt(String),
    /// The profile is locked: the file is sealed and its key is not loaded. Nothing was changed.
    #[error("the profile is locked")]
    Locked,
}

/// Removes a companion from the stored roster and takes its id out of the MCP servers' scopes
/// (a server that was only for it ends up for nobody, not for everyone). The caller also forgets
/// its grants (see [`Grants::forget`]).
pub fn remove_companion(store: &dyn AiStore, id: &weirdnet_agent::AgentId) -> Result<crate::CompanionProfile, String> {
    let mut roster = store.load_roster().map_err(|e| e.to_string())?.ok_or_else(|| format!("no companion {id}"))?;
    let removed = roster.remove(id).map_err(|e| e.to_string())?;
    store.save_roster(&roster).map_err(|e| e.to_string())?;
    if let Some(mut mcp) = store.load_mcp().map_err(|e| e.to_string())?
        && mcp.forget_companion(id.as_str())
    {
        store.save_mcp(&mcp).map_err(|e| e.to_string())?;
    }
    Ok(removed)
}

/// Companions that left the stored roster since `seen` lose their place in the MCP servers'
/// scopes, the same as [`remove_companion`] (a server that was only for them ends up for
/// nobody). Whatever removed them: a removal that arrived through sync, a profile lock, any
/// later path. The caller runs this when it learns the stored files changed; `seen` becomes
/// the roster's ids now. A roster that cannot be read changes nothing. Returns whether the MCP
/// configs changed.
pub fn forget_vanished_companions(store: &dyn AiStore, seen: &mut std::collections::BTreeSet<String>) -> Result<bool, String> {
    let Ok(Some(roster)) = store.load_roster() else { return Ok(false) };
    let now: std::collections::BTreeSet<String> = roster.companions().iter().map(|p| p.id().as_str().to_string()).collect();
    let gone: Vec<String> = seen.difference(&now).cloned().collect();
    *seen = now;
    if gone.is_empty() {
        return Ok(false);
    }
    let Some(mut mcp) = store.load_mcp().map_err(|e| e.to_string())? else { return Ok(false) };
    let mut changed = false;
    for id in &gone {
        changed |= mcp.forget_companion(id);
    }
    if changed {
        store.save_mcp(&mcp).map_err(|e| e.to_string())?;
    }
    Ok(changed)
}

/// Where the roster and policy live. The app implements it over its encrypted store; tests
/// and first runs use [`MemoryStore`] or [`FileStore`].
pub trait AiStore: Send + Sync {
    fn load_roster(&self) -> Result<Option<Roster>, StoreError>;
    fn save_roster(&self, roster: &Roster) -> Result<(), StoreError>;
    fn load_policy(&self) -> Result<Option<Policy>, StoreError>;
    fn save_policy(&self, policy: &Policy) -> Result<(), StoreError>;
    fn load_settings(&self) -> Result<Option<AgentSettings>, StoreError>;
    fn save_settings(&self, settings: &AgentSettings) -> Result<(), StoreError>;
    /// Saved chats. Stores written before chats existed hold none.
    fn load_chats(&self) -> Result<Option<ChatLog>, StoreError> {
        Ok(None)
    }
    fn save_chats(&self, _chats: &ChatLog) -> Result<(), StoreError> {
        Err(StoreError::Io("this store does not keep chats".into()))
    }
    /// MCP server configs (secret values are never in here; see [`crate::mcp::McpSecrets`]).
    fn load_mcp(&self) -> Result<Option<McpConfigs>, StoreError> {
        Ok(None)
    }
    fn save_mcp(&self, _mcp: &McpConfigs) -> Result<(), StoreError> {
        Err(StoreError::Io("this store does not keep MCP configs".into()))
    }
    /// Notes and project journals.
    fn load_journals(&self) -> Result<Option<Journals>, StoreError> {
        Ok(None)
    }
    fn save_journals(&self, _journals: &Journals) -> Result<(), StoreError> {
        Err(StoreError::Io("this store does not keep journals".into()))
    }
    /// Research projects as saved (not recovered: steps in flight stay as they are), for sync.
    fn load_research(&self) -> Result<Option<crate::research::Projects>, StoreError> {
        Ok(None)
    }
    fn save_research(&self, _projects: &crate::research::Projects) -> Result<(), StoreError> {
        Err(StoreError::Io("this store does not keep research projects".into()))
    }
    /// This install's research device id (never synced).
    fn load_research_device(&self) -> Result<Option<crate::lease::Device>, StoreError> {
        Ok(None)
    }
}

#[derive(Default)]
pub struct MemoryStore {
    roster: Mutex<Option<Roster>>,
    policy: Mutex<Option<Policy>>,
    settings: Mutex<Option<AgentSettings>>,
    chats: Mutex<Option<ChatLog>>,
    mcp: Mutex<Option<McpConfigs>>,
    journals: Mutex<Option<Journals>>,
    research: Mutex<Option<crate::research::Projects>>,
    research_device: Mutex<Option<crate::lease::Device>>,
}

impl MemoryStore {
    /// Sets this store's research device id (a test seam; the files keep it in `research-device.json`).
    pub fn set_research_device(&self, device: crate::lease::Device) {
        *self.research_device.lock().unwrap() = Some(device);
    }
}

impl AiStore for MemoryStore {
    fn load_roster(&self) -> Result<Option<Roster>, StoreError> {
        Ok(self.roster.lock().unwrap().clone())
    }
    fn save_roster(&self, roster: &Roster) -> Result<(), StoreError> {
        *self.roster.lock().unwrap() = Some(roster.clone());
        Ok(())
    }
    fn load_policy(&self) -> Result<Option<Policy>, StoreError> {
        Ok(self.policy.lock().unwrap().clone())
    }
    fn save_policy(&self, policy: &Policy) -> Result<(), StoreError> {
        *self.policy.lock().unwrap() = Some(policy.clone());
        Ok(())
    }
    fn load_settings(&self) -> Result<Option<AgentSettings>, StoreError> {
        Ok(self.settings.lock().unwrap().clone())
    }
    fn save_settings(&self, settings: &AgentSettings) -> Result<(), StoreError> {
        *self.settings.lock().unwrap() = Some(settings.clone());
        Ok(())
    }
    fn load_chats(&self) -> Result<Option<ChatLog>, StoreError> {
        Ok(self.chats.lock().unwrap().clone())
    }
    fn save_chats(&self, chats: &ChatLog) -> Result<(), StoreError> {
        *self.chats.lock().unwrap() = Some(chats.clone());
        Ok(())
    }
    fn load_mcp(&self) -> Result<Option<McpConfigs>, StoreError> {
        Ok(self.mcp.lock().unwrap().clone())
    }
    fn save_mcp(&self, mcp: &McpConfigs) -> Result<(), StoreError> {
        *self.mcp.lock().unwrap() = Some(mcp.for_sync());
        Ok(())
    }
    fn load_journals(&self) -> Result<Option<Journals>, StoreError> {
        Ok(self.journals.lock().unwrap().clone())
    }
    fn save_journals(&self, journals: &Journals) -> Result<(), StoreError> {
        *self.journals.lock().unwrap() = Some(journals.clone());
        Ok(())
    }
    fn load_research(&self) -> Result<Option<crate::research::Projects>, StoreError> {
        Ok(self.research.lock().unwrap().clone())
    }
    fn save_research(&self, projects: &crate::research::Projects) -> Result<(), StoreError> {
        *self.research.lock().unwrap() = Some(projects.clone());
        Ok(())
    }
    fn load_research_device(&self) -> Result<Option<crate::lease::Device>, StoreError> {
        Ok(self.research_device.lock().unwrap().clone())
    }
}

/// JSON files (`roster.json`, `policy.json`, `settings.json`, `chats.json`, `mcp.json`,
/// `journals.json`) in a directory, written atomically.
pub struct FileStore {
    dir: PathBuf,
    io: Arc<dyn FileIo>,
}

impl FileStore {
    /// Plain files in `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_io(dir, Arc::new(PlainIo))
    }

    /// Files in `dir` through `io` (the profile's sealed files, in the desktop).
    pub fn with_io(dir: impl Into<PathBuf>, io: Arc<dyn FileIo>) -> Self {
        Self { dir: dir.into(), io }
    }

    /// The raw bytes of `name` (a path under the directory); `Ok(None)` if it does not exist.
    pub fn read_file(&self, name: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.io.read(&self.dir.join(name))
    }

    /// Replaces `name` (a path under the directory) with `bytes`.
    pub fn write_file(&self, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.io.write(&self.dir.join(name), bytes)
    }

    /// Adds `line` and a newline to the end of `name`, creating it if need be.
    pub fn append_line(&self, name: &str, line: &str) -> Result<(), StoreError> {
        let mut bytes = self.read_file(name)?.unwrap_or_default();
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        self.write_file(name, &bytes)
    }

    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<Option<T>, StoreError> {
        match self.read_file(name)? {
            Some(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| StoreError::Corrupt(format!("{name}: {e}"))),
            None => Ok(None),
        }
    }

    fn write<T: serde::Serialize>(&self, name: &str, value: &T) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| StoreError::Io(e.to_string()))?;
        self.write_file(name, &bytes)
    }
}

impl AiStore for FileStore {
    fn load_roster(&self) -> Result<Option<Roster>, StoreError> {
        self.read("roster.json")
    }
    fn save_roster(&self, roster: &Roster) -> Result<(), StoreError> {
        self.write("roster.json", roster)
    }
    fn load_policy(&self) -> Result<Option<Policy>, StoreError> {
        self.read("policy.json")
    }
    fn save_policy(&self, policy: &Policy) -> Result<(), StoreError> {
        self.write("policy.json", policy)
    }
    fn load_settings(&self) -> Result<Option<AgentSettings>, StoreError> {
        self.read("settings.json")
    }
    fn save_settings(&self, settings: &AgentSettings) -> Result<(), StoreError> {
        self.write("settings.json", settings)
    }
    fn load_chats(&self) -> Result<Option<ChatLog>, StoreError> {
        self.read("chats.json")
    }
    fn save_chats(&self, chats: &ChatLog) -> Result<(), StoreError> {
        self.write("chats.json", chats)
    }
    fn load_mcp(&self) -> Result<Option<McpConfigs>, StoreError> {
        self.read("mcp.json")
    }
    /// Secret values are moved out first: `mcp.json` never holds one.
    fn save_mcp(&self, mcp: &McpConfigs) -> Result<(), StoreError> {
        self.write("mcp.json", &mcp.for_sync())
    }
    fn load_journals(&self) -> Result<Option<Journals>, StoreError> {
        self.read("journals.json")
    }
    fn save_journals(&self, journals: &Journals) -> Result<(), StoreError> {
        self.write("journals.json", journals)
    }
    fn load_research(&self) -> Result<Option<crate::research::Projects>, StoreError> {
        self.read(crate::research::FILE)
    }
    fn save_research(&self, projects: &crate::research::Projects) -> Result<(), StoreError> {
        self.write(crate::research::FILE, projects)
    }
    fn load_research_device(&self) -> Result<Option<crate::lease::Device>, StoreError> {
        self.read(crate::lease::DEVICE_FILE)
    }
}

/// The live policy plus its store. Every change is saved at once, and running sessions see it
/// at their next step through [`Grants::shared`].
pub struct Grants {
    store: Arc<dyn AiStore>,
    policy: SharedPolicy,
}

impl Grants {
    pub fn load(store: Arc<dyn AiStore>) -> Result<Self, StoreError> {
        let policy = store.load_policy()?.unwrap_or_default();
        Ok(Self { store, policy: policy.shared() })
    }

    pub fn shared(&self) -> SharedPolicy {
        self.policy.clone()
    }

    fn change<R>(&self, f: impl FnOnce(&mut Policy) -> R) -> Result<R, StoreError> {
        let mut p = self.policy.write().unwrap_or_else(|e| e.into_inner());
        let out = f(&mut p);
        self.store.save_policy(&p)?;
        Ok(out)
    }

    pub fn set_site_level(&self, agent: &AgentId, site: &str, level: SiteLevel) -> Result<(), StoreError> {
        self.change(|p| p.set_site_level(agent, site, level))
    }

    pub fn add_allow(&self, companion: CompanionScope, rule: AllowRule, now_unix_ms: u64) -> Result<u64, StoreError> {
        self.change(|p| p.add_allow(companion, rule, now_unix_ms))
    }

    pub fn revoke(&self, allow_id: u64) -> Result<bool, StoreError> {
        self.change(|p| p.revoke(allow_id))
    }

    pub fn revoke_all(&self) -> Result<(), StoreError> {
        self.change(|p| p.revoke_all())
    }

    /// Drops a removed companion's grants and allows.
    pub fn forget(&self, agent: &AgentId) -> Result<(), StoreError> {
        self.change(|p| p.forget_agent(agent))
    }

    /// Set one level for several companions at once (the "all companions" choice).
    pub fn set_site_level_for(&self, agents: &[AgentId], site: &str, level: SiteLevel) -> Result<(), StoreError> {
        self.change(|p| agents.iter().for_each(|a| p.set_site_level(a, site, level)))
    }

    /// A companion's per-site levels that differ from Ask, sorted by site.
    pub fn sites_for(&self, agent: &AgentId) -> Vec<(String, SiteLevel)> {
        let p = self.policy.read().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<(String, SiteLevel)> = p.grants_for(agent).map(|(s, l)| (s.to_string(), l)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// Every standing allow, oldest first.
    pub fn allows(&self) -> Vec<StandingAllow> {
        self.policy.read().unwrap_or_else(|e| e.into_inner()).allows().to_vec()
    }

    pub fn site_level(&self, agent: &AgentId, site: &str) -> SiteLevel {
        self.policy.read().unwrap_or_else(|e| e.into_inner()).site_level(agent, site)
    }
}

/// Write the live policy to the store (the panel does this after each task, since approvals can
/// add Always allow entries).
pub fn save_shared(store: &dyn AiStore, policy: &SharedPolicy) -> Result<(), StoreError> {
    store.save_policy(&policy.read().unwrap_or_else(|e| e.into_inner()))
}

/// Replace the live policy with what is stored (Settings changed it on disk).
pub fn reload_shared(store: &dyn AiStore, policy: &SharedPolicy) -> Result<(), StoreError> {
    if let Some(fresh) = store.load_policy()? {
        *policy.write().unwrap_or_else(|e| e.into_inner()) = fresh;
    }
    Ok(())
}

/// Stop on a skipped-approval notice: revoke that Always allow in the live policy and save it.
/// Returns whether the allow existed.
pub fn revoke_shared(store: &dyn AiStore, policy: &SharedPolicy, allow_id: u64) -> Result<bool, StoreError> {
    let found = policy.write().unwrap_or_else(|e| e.into_inner()).revoke(allow_id);
    save_shared(store, policy)?;
    Ok(found)
}

/// This device's policy changes since `base` (what it last loaded or saved), replayed onto
/// `theirs` (what is stored now, which sync or Settings may have changed). Site levels this
/// device changed take its value; allows it added or removed are added or removed there (an
/// added allow gets that policy's next id); external approvals likewise. Everything else is
/// `theirs`, so a grant sync voided is not brought back by a stale copy.
pub fn rebase_policy(base: &Policy, mine: &Policy, theirs: &Policy) -> Policy {
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};
    let mut out = theirs.clone();
    let grants = |p: &Policy| -> BTreeMap<(String, String), Value> {
        let v = serde_json::to_value(p).unwrap_or_default();
        let mut m = BTreeMap::new();
        for (agent, sites) in v.get("grants").and_then(Value::as_object).into_iter().flatten() {
            for (site, level) in sites.as_object().into_iter().flatten() {
                m.insert((agent.clone(), site.clone()), level.clone());
            }
        }
        m
    };
    let (gb, gm) = (grants(base), grants(mine));
    for key in gb.keys().chain(gm.keys()).collect::<BTreeSet<_>>() {
        if gb.get(key) != gm.get(key) {
            let level = gm.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(SiteLevel::Ask);
            out.set_site_level(&AgentId::new(key.0.as_str()), &key.1, level);
        }
    }
    let same = |a: &StandingAllow, b: &StandingAllow| a.companion == b.companion && a.rule == b.rule;
    for b in base.allows() {
        if !mine.allows().iter().any(|m| m.id == b.id && same(m, b)) {
            let gone: Vec<u64> = out.allows().iter().filter(|o| same(o, b)).map(|o| o.id).collect();
            gone.into_iter().for_each(|id| {
                out.revoke(id);
            });
        }
    }
    for m in mine.allows() {
        if !base.allows().iter().any(|b| b.id == m.id && same(b, m)) {
            out.add_allow(m.companion.clone(), m.rule.clone(), m.created_unix_ms);
        }
    }
    let ext = |p: &Policy| -> BTreeSet<String> {
        let v = serde_json::to_value(p).unwrap_or_default();
        v.get("approved_external").and_then(Value::as_array).into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect()
    };
    let (eb, em) = (ext(base), ext(mine));
    if eb != em {
        let mut set = ext(&out);
        set.extend(em.difference(&eb).cloned());
        set.retain(|a| !(eb.contains(a) && !em.contains(a)));
        let mut v = serde_json::to_value(&out).unwrap_or_default();
        v["approved_external"] = set.into_iter().map(Value::String).collect();
        if let Ok(p) = serde_json::from_value(v) {
            out = p;
        }
    }
    out
}

/// Save the live policy without undoing what changed on disk meanwhile: this device's changes
/// since `base` are replayed onto the stored policy ([`rebase_policy`]), the result becomes the
/// live policy and is saved. Returns it, the new `base`. Holds the policy's write lock
/// throughout, so an approval made meanwhile is not lost.
pub fn rebase_shared(store: &dyn AiStore, policy: &SharedPolicy, base: &Policy) -> Result<Policy, StoreError> {
    let mut live = policy.write().unwrap_or_else(|e| e.into_inner());
    let stored = store.load_policy()?;
    let merged = match &stored {
        Some(theirs) => rebase_policy(base, &live, theirs),
        None => live.clone(),
    };
    if stored.as_ref() != Some(&merged) {
        store.save_policy(&merged)?;
    }
    *live = merged.clone();
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelChoice;

    #[test]
    fn removing_a_companion_takes_its_id_out_of_mcp_scopes_without_widening_any() {
        use crate::{CompanionProfile, McpConfigs, McpServer, McpTransport, Roster};
        use std::collections::{BTreeMap, BTreeSet};
        let store = MemoryStore::default();
        let mut roster = Roster::first_run(ModelChoice::Gemma4E4B);
        roster.add(CompanionProfile::local("scout", "Scout", ModelChoice::Gemma4E4B)).unwrap();
        store.save_roster(&roster).unwrap();
        let http = || McpTransport::Http { url: "https://x".into(), headers: BTreeMap::new() };
        let mut both = McpServer::new("Both", http());
        both.companions = Some(BTreeSet::from(["local".to_string(), "scout".to_string()]));
        let mut only = McpServer::new("Only", http());
        only.companions = Some(BTreeSet::from(["scout".to_string()]));
        let everyone = McpServer::new("Everyone", http());
        let mut cfgs = McpConfigs::default();
        for s in [&both, &only, &everyone] {
            cfgs.upsert(s.clone());
        }
        store.save_mcp(&cfgs).unwrap();

        remove_companion(&store, &AgentId::new("scout")).unwrap();
        assert!(store.load_roster().unwrap().unwrap().get(&AgentId::new("scout")).is_none());
        let after = store.load_mcp().unwrap().unwrap();
        let scope = |id: &str| after.servers.iter().find(|s| s.id == id).unwrap().companions.clone();
        assert_eq!(scope(&both.id), Some(BTreeSet::from(["local".to_string()])));
        assert_eq!(scope(&only.id), Some(BTreeSet::new()), "for nobody now, not for everyone");
        assert!(!after.servers.iter().find(|s| s.id == only.id).unwrap().applies_to("local"));
        assert_eq!(scope(&everyone.id), None);
        // A companion that is not there is an error and changes nothing.
        assert!(remove_companion(&store, &AgentId::new("scout")).is_err());
        assert_eq!(store.load_mcp().unwrap().unwrap(), after);
        let mut again = after.clone();
        assert!(!again.forget_companion("scout"), "nothing left to take out");
    }

    #[test]
    fn companions_that_vanish_leave_mcp_scopes_like_a_removal() {
        use crate::mcp::{McpConfigs, McpServer, McpTransport};
        let store = MemoryStore::default();
        let mut roster = Roster::first_run(crate::ModelChoice::Gemma4E2B);
        roster.add(crate::CompanionProfile::local("scout", "Scout", crate::ModelChoice::Gemma4E2B)).unwrap();
        roster.add(crate::CompanionProfile::local("atlas", "Atlas", crate::ModelChoice::Gemma4E2B)).unwrap();
        store.save_roster(&roster).unwrap();
        let server = |name: &str, scope: Option<&[&str]>| {
            let mut s = McpServer::new(name, McpTransport::Http { url: "https://mcp.example/".into(), headers: Default::default() });
            s.companions = scope.map(|ids| ids.iter().map(|i| i.to_string()).collect());
            s
        };
        let mut mcp = McpConfigs::default();
        mcp.upsert(server("Only scout", Some(&["scout"])));
        mcp.upsert(server("Both", Some(&["scout", "atlas"])));
        mcp.upsert(server("Everyone", None));
        store.save_mcp(&mcp).unwrap();
        let mut seen = Default::default();
        assert!(!forget_vanished_companions(&store, &mut seen).unwrap(), "the first look only learns the roster");

        // Scout leaves the roster without remove_companion (as a synced removal does).
        roster.remove(&AgentId::new("scout")).unwrap();
        store.save_roster(&roster).unwrap();
        assert!(forget_vanished_companions(&store, &mut seen).unwrap());
        let scope = |name: &str| store.load_mcp().unwrap().unwrap().servers.into_iter().find(|s| s.name == name).unwrap().companions;
        assert_eq!(scope("Only scout"), Some(Default::default()), "for nobody, not for everyone");
        assert_eq!(scope("Both"), Some(["atlas".to_string()].into()));
        assert_eq!(scope("Everyone"), None);
        assert!(!forget_vanished_companions(&store, &mut seen).unwrap(), "nothing new");
    }

    #[test]
    fn grants_persist_through_the_store() {
        let store = Arc::new(MemoryStore::default());
        let g = Grants::load(store.clone()).unwrap();
        let a = AgentId::new("local");
        assert_eq!(g.site_level(&a, "example.com"), SiteLevel::Ask);
        g.set_site_level(&a, "example.com", SiteLevel::Off).unwrap();
        let id = g.add_allow(CompanionScope::All, AllowRule::Research { provider: None }, 5).unwrap();

        let again = Grants::load(store.clone()).unwrap();
        assert_eq!(again.site_level(&a, "example.com"), SiteLevel::Off);
        assert!(again.revoke(id).unwrap());
        assert!(!again.revoke(id).unwrap());
        assert!(Grants::load(store).unwrap().shared().read().unwrap().allows().is_empty());
    }

    #[test]
    fn revoke_is_visible_to_the_shared_policy() {
        let g = Grants::load(Arc::new(MemoryStore::default())).unwrap();
        let shared = g.shared();
        let id = g.add_allow(CompanionScope::All, AllowRule::Navigate { site: None }, 1).unwrap();
        assert_eq!(shared.read().unwrap().allows().len(), 1);
        g.revoke(id).unwrap();
        assert!(shared.read().unwrap().allows().is_empty());
    }

    #[test]
    fn forget_removes_a_companions_grants() {
        let g = Grants::load(Arc::new(MemoryStore::default())).unwrap();
        let a = AgentId::new("atlas");
        g.set_site_level(&a, "a.com", SiteLevel::Act).unwrap();
        g.forget(&a).unwrap();
        assert_eq!(g.site_level(&a, "a.com"), SiteLevel::Ask);
    }

    #[test]
    fn file_store_roundtrips_and_reports_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let s = FileStore::new(dir.path().join("ai"));
        assert!(s.load_roster().unwrap().is_none());
        let r = Roster::first_run(ModelChoice::Gemma4E4B);
        s.save_roster(&r).unwrap();
        assert_eq!(s.load_roster().unwrap(), Some(r));
        let mut p = Policy::new();
        p.set_site_level(&AgentId::new("local"), "x.org", SiteLevel::Act);
        s.save_policy(&p).unwrap();
        assert_eq!(s.load_policy().unwrap(), Some(p));
        std::fs::write(dir.path().join("ai/policy.json"), b"{nope").unwrap();
        assert!(matches!(s.load_policy(), Err(StoreError::Corrupt(_))));
    }

    #[test]
    fn chats_mcp_and_journals_round_trip_and_mcp_secrets_stay_out_of_the_file() {
        use crate::chats::ChatTurn;
        use crate::mcp::{McpServer, McpTransport, McpValue};
        let dir = tempfile::tempdir().unwrap();
        let file = FileStore::new(dir.path().join("companions"));
        assert_eq!(file.load_chats().unwrap(), None);
        let mut chats = ChatLog::default();
        chats.push(ChatTurn::new(AgentId::new("local"), "q", "a", 1));
        file.save_chats(&chats).unwrap();
        assert_eq!(file.load_chats().unwrap(), Some(chats));
        let mut journals = Journals::default();
        journals.create_project("P").unwrap();
        file.save_journals(&journals).unwrap();
        assert_eq!(file.load_journals().unwrap(), Some(journals));
        let mut env = std::collections::BTreeMap::new();
        env.insert("API_KEY".to_string(), McpValue::Plain { value: "sk-live-1".into() });
        let mut mcp = McpConfigs::default();
        mcp.upsert(McpServer::new("S", McpTransport::Stdio { command: "s".into(), args: vec![], env }));
        file.save_mcp(&mcp).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("companions/mcp.json")).unwrap();
        assert!(!raw.contains("sk-live-1"), "{raw}");
        assert_eq!(file.load_mcp().unwrap().unwrap().servers[0].secret_names(), ["API_KEY"]);
    }

    #[test]
    fn all_companions_levels_sites_and_allows_list_in_order() {
        let g = Grants::load(Arc::new(MemoryStore::default())).unwrap();
        let (a, b) = (AgentId::new("atlas"), AgentId::new("scout"));
        g.set_site_level_for(&[a.clone(), b.clone()], "Example.com", SiteLevel::Off).unwrap();
        g.set_site_level(&a, "b.org", SiteLevel::Act).unwrap();
        assert_eq!(g.sites_for(&a), [("b.org".to_string(), SiteLevel::Act), ("example.com".to_string(), SiteLevel::Off)]);
        assert_eq!(g.sites_for(&b), [("example.com".to_string(), SiteLevel::Off)]);
        g.set_site_level(&a, "b.org", SiteLevel::Ask).unwrap();
        assert_eq!(g.sites_for(&a).len(), 1, "Ask is the default and is not listed");
        let id1 = g.add_allow(CompanionScope::One(a.clone()), AllowRule::Navigate { site: Some("tokio.rs".into()) }, 1).unwrap();
        let id2 = g.add_allow(CompanionScope::All, AllowRule::Research { provider: Some("duckduckgo".into()) }, 2).unwrap();
        assert_eq!(g.allows().iter().map(|x| x.id).collect::<Vec<_>>(), [id1, id2]);
    }

    #[test]
    fn stop_revokes_in_the_live_policy_and_the_store_and_settings_changes_reload() {
        let store = MemoryStore::default();
        let g = Grants::load(Arc::new(MemoryStore::default())).unwrap();
        let live = g.shared();
        let id = g.add_allow(CompanionScope::All, AllowRule::Research { provider: None }, 1).unwrap();
        save_shared(&store, &live).unwrap();
        assert!(revoke_shared(&store, &live, id).unwrap());
        assert!(live.read().unwrap().allows().is_empty());
        assert!(store.load_policy().unwrap().unwrap().allows().is_empty());
        assert!(!revoke_shared(&store, &live, id).unwrap(), "a second Stop finds nothing");

        // Settings wrote a site level on disk; the panel's live policy picks it up.
        let mut on_disk = Policy::new();
        on_disk.set_site_level(&AgentId::new("local"), "a.com", SiteLevel::Off);
        store.save_policy(&on_disk).unwrap();
        reload_shared(&store, &live).unwrap();
        assert_eq!(live.read().unwrap().site_level(&AgentId::new("local"), "a.com"), SiteLevel::Off);
        // Nothing stored: the live policy is left alone.
        reload_shared(&MemoryStore::default(), &live).unwrap();
        assert_eq!(live.read().unwrap().site_level(&AgentId::new("local"), "a.com"), SiteLevel::Off);
    }

    #[test]
    fn a_stale_policy_keeps_its_own_changes_and_takes_the_rest_from_disk() {
        let (a, b) = (AgentId::new("local"), AgentId::new("atlas"));
        let mut base = Policy::new();
        base.set_site_level(&a, "old.example", SiteLevel::Act);
        let kept = base.add_allow(CompanionScope::All, AllowRule::Research { provider: None }, 1);
        let voided = base.add_allow(CompanionScope::One(b.clone()), AllowRule::Navigate { site: None }, 2);
        // On disk, sync voided a revoked device's allow and site grant and added one of its own.
        let mut theirs = base.clone();
        theirs.revoke(voided);
        theirs.set_site_level(&a, "old.example", SiteLevel::Ask);
        let synced = theirs.add_allow(CompanionScope::One(a.clone()), AllowRule::Navigate { site: Some("docs.rs".into()) }, 3);
        // Here, meanwhile, an approval added an allow and Settings turned a site off.
        let mut mine = base.clone();
        mine.add_allow(CompanionScope::All, AllowRule::Navigate { site: Some("tokio.rs".into()) }, 4);
        mine.set_site_level(&a, "bad.example", SiteLevel::Off);
        mine.revoke(kept);

        let out = rebase_policy(&base, &mine, &theirs);
        assert_eq!(out.site_level(&a, "old.example"), SiteLevel::Ask, "the voided grant does not come back");
        assert_eq!(out.site_level(&a, "bad.example"), SiteLevel::Off, "this device's change stays");
        let rules: Vec<_> = out.allows().iter().map(|x| x.rule.clone()).collect();
        assert!(!out.allows().iter().any(|x| x.id == voided || x.id == kept), "{rules:?}");
        assert!(out.allows().iter().any(|x| x.id == synced));
        let added = out.allows().iter().find(|x| x.rule == AllowRule::Navigate { site: Some("tokio.rs".into()) }).expect("the new allow is kept");
        assert!(added.id > synced, "a fresh id, not one sync gave out");
        // Nothing changed here: the disk wins outright.
        assert_eq!(rebase_policy(&base, &base, &theirs), theirs);
    }

    #[test]
    fn rebase_shared_saves_the_merge_and_makes_it_the_live_policy() {
        let store = MemoryStore::default();
        let a = AgentId::new("local");
        let base = Policy::new();
        let mut theirs = Policy::new();
        theirs.set_site_level(&a, "synced.example", SiteLevel::Act);
        store.save_policy(&theirs).unwrap();
        let live = Policy::new().shared();
        live.write().unwrap().set_site_level(&a, "mine.example", SiteLevel::Off);
        let new_base = rebase_shared(&store, &live, &base).unwrap();
        let stored = store.load_policy().unwrap().unwrap();
        assert_eq!(stored, new_base);
        assert_eq!(*live.read().unwrap(), new_base);
        assert_eq!((stored.site_level(&a, "synced.example"), stored.site_level(&a, "mine.example")), (SiteLevel::Act, SiteLevel::Off));
    }
}
