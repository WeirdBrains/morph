//! The user's companions: named agents, one of them the default.

use serde::{Deserialize, Serialize};
use weirdnet_agent::{AgentId, AgentIdentity, AgentKind};

use crate::model::ModelChoice;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RosterError {
    #[error("a companion named {0:?} already exists")]
    NameTaken(String),
    #[error("a companion with id {0} already exists")]
    IdTaken(String),
    #[error("a companion needs a name")]
    EmptyName,
    #[error("no companion {0}")]
    Unknown(String),
}

/// A companion: identity plus how it thinks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanionProfile {
    pub identity: AgentIdentity,
    /// The model a Local or Node companion runs. External companions bring their own.
    #[serde(default)]
    pub model: Option<ModelChoice>,
}

impl CompanionProfile {
    pub fn local(id: &str, name: &str, model: ModelChoice) -> Self {
        Self { identity: AgentIdentity::new(id, name, AgentKind::Local), model: Some(model) }
    }

    pub fn id(&self) -> &AgentId {
        &self.identity.id
    }

    pub fn name(&self) -> &str {
        &self.identity.name
    }
}

/// All companions. Names are unique ignoring case, so `@name` and `>name` are unambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Roster {
    companions: Vec<CompanionProfile>,
    #[serde(default)]
    default: Option<AgentId>,
}

impl Roster {
    /// A fresh roster with one Local companion named "Local", as a first run gets.
    pub fn first_run(model: ModelChoice) -> Self {
        let mut r = Self::default();
        r.add(CompanionProfile::local("local", "Local", model)).expect("fresh roster");
        r
    }

    pub fn companions(&self) -> &[CompanionProfile] {
        &self.companions
    }

    pub fn get(&self, id: &AgentId) -> Option<&CompanionProfile> {
        self.companions.iter().find(|c| c.id() == id)
    }

    /// Looks a companion up by the name typed after `>` or `@`, ignoring case.
    pub fn find_by_name(&self, name: &str) -> Option<&CompanionProfile> {
        let name = name.trim();
        self.companions.iter().find(|c| c.name().eq_ignore_ascii_case(name))
    }

    /// The default companion: the chosen one, else the first.
    pub fn default_companion(&self) -> Option<&CompanionProfile> {
        self.default.as_ref().and_then(|id| self.get(id)).or_else(|| self.companions.first())
    }

    pub fn add(&mut self, profile: CompanionProfile) -> Result<(), RosterError> {
        if profile.name().trim().is_empty() {
            return Err(RosterError::EmptyName);
        }
        if self.get(profile.id()).is_some() {
            return Err(RosterError::IdTaken(profile.id().to_string()));
        }
        if self.find_by_name(profile.name()).is_some() {
            return Err(RosterError::NameTaken(profile.name().to_string()));
        }
        self.companions.push(profile);
        Ok(())
    }

    pub fn rename(&mut self, id: &AgentId, name: &str) -> Result<(), RosterError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(RosterError::EmptyName);
        }
        if self.find_by_name(name).is_some_and(|c| c.id() != id) {
            return Err(RosterError::NameTaken(name.to_string()));
        }
        let c = self.companions.iter_mut().find(|c| c.id() == id).ok_or_else(|| RosterError::Unknown(id.to_string()))?;
        c.identity.name = name.to_string();
        Ok(())
    }

    pub fn set_default(&mut self, id: &AgentId) -> Result<(), RosterError> {
        self.get(id).ok_or_else(|| RosterError::Unknown(id.to_string()))?;
        self.default = Some(id.clone());
        Ok(())
    }

    /// Choose the model a companion runs.
    pub fn set_model(&mut self, id: &AgentId, model: ModelChoice) -> Result<(), RosterError> {
        let c = self.companions.iter_mut().find(|c| c.id() == id).ok_or_else(|| RosterError::Unknown(id.to_string()))?;
        c.model = Some(model);
        Ok(())
    }

    /// Removes a companion. The caller also forgets its grants (see [`crate::Grants::forget`]).
    pub fn remove(&mut self, id: &AgentId) -> Result<CompanionProfile, RosterError> {
        let i = self.companions.iter().position(|c| c.id() == id).ok_or_else(|| RosterError::Unknown(id.to_string()))?;
        if self.default.as_ref() == Some(id) {
            self.default = None;
        }
        Ok(self.companions.remove(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster {
        Roster::first_run(ModelChoice::Gemma4E4B)
    }

    #[test]
    fn first_run_has_one_default_local() {
        let r = roster();
        assert_eq!(r.companions().len(), 1);
        assert_eq!(r.default_companion().unwrap().name(), "Local");
        assert_eq!(r.default_companion().unwrap().identity.kind, AgentKind::Local);
    }

    #[test]
    fn names_are_unique_ignoring_case_and_ids_too() {
        let mut r = roster();
        assert_eq!(r.add(CompanionProfile::local("x", "LOCAL", ModelChoice::Gemma4E2B)), Err(RosterError::NameTaken("LOCAL".into())));
        assert_eq!(r.add(CompanionProfile::local("local", "Other", ModelChoice::Gemma4E2B)), Err(RosterError::IdTaken("local".into())));
        assert_eq!(r.add(CompanionProfile::local("y", "  ", ModelChoice::Gemma4E2B)), Err(RosterError::EmptyName));
        r.add(CompanionProfile::local("atlas", "Atlas", ModelChoice::Gemma4E2B)).unwrap();
        assert_eq!(r.find_by_name(" atlas ").unwrap().id().as_str(), "atlas");
    }

    #[test]
    fn default_follows_choice_and_falls_back_after_remove() {
        let mut r = roster();
        r.add(CompanionProfile::local("atlas", "Atlas", ModelChoice::Gemma4E2B)).unwrap();
        r.set_default(&AgentId::new("atlas")).unwrap();
        assert_eq!(r.default_companion().unwrap().name(), "Atlas");
        r.remove(&AgentId::new("atlas")).unwrap();
        assert_eq!(r.default_companion().unwrap().name(), "Local");
        assert!(r.set_default(&AgentId::new("nope")).is_err());
    }

    #[test]
    fn rename_keeps_own_name_but_not_anothers() {
        let mut r = roster();
        r.add(CompanionProfile::local("atlas", "Atlas", ModelChoice::Gemma4E2B)).unwrap();
        r.rename(&AgentId::new("atlas"), "ATLAS").unwrap();
        assert_eq!(r.rename(&AgentId::new("atlas"), "local"), Err(RosterError::NameTaken("local".into())));
    }

    #[test]
    fn serde_roundtrip() {
        let r = roster();
        let back: Roster = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn a_companions_model_can_change() {
        let mut r = Roster::first_run(ModelChoice::Gemma4E4B);
        let id = r.default_companion().unwrap().id().clone();
        r.set_model(&id, ModelChoice::Custom("org/m".into())).unwrap();
        assert_eq!(r.get(&id).unwrap().model, Some(ModelChoice::Custom("org/m".into())));
        assert!(r.set_model(&AgentId::new("nope"), ModelChoice::Gemma4E2B).is_err());
    }
}
