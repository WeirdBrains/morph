//! Persistence for the roster and the policy (grants and standing allows) behind one interface.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use weirdnet_agent::{AgentId, AllowRule, CompanionScope, Policy, SharedPolicy, SiteLevel, StandingAllow};

use crate::roster::Roster;
use crate::settings::AgentSettings;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage failed: {0}")]
    Io(String),
    #[error("stored data is unreadable: {0}")]
    Corrupt(String),
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
}

#[derive(Default)]
pub struct MemoryStore {
    roster: Mutex<Option<Roster>>,
    policy: Mutex<Option<Policy>>,
    settings: Mutex<Option<AgentSettings>>,
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
}

/// JSON files `roster.json` and `policy.json` in a directory, written atomically.
pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> Result<Option<T>, StoreError> {
        match std::fs::read(self.dir.join(name)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| StoreError::Corrupt(format!("{name}: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::Io(e.to_string())),
        }
    }

    fn write<T: serde::Serialize>(&self, name: &str, value: &T) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| StoreError::Io(e.to_string()))?;
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| StoreError::Io(e.to_string()))?;
        let tmp = self.dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| StoreError::Io(e.to_string()))?;
        std::fs::rename(&tmp, self.dir.join(name)).map_err(|e| StoreError::Io(e.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelChoice;

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
}
