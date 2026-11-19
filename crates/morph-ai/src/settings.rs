//! Agent settings that are not per companion: where research looks. Stored in the companions
//! directory as `settings.json`; the research host reads it later.

use serde::{Deserialize, Serialize};

/// The web provider research uses after the Library (bookmarks and history) has been searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchProvider {
    #[default]
    DuckDuckGo,
    Google,
}

impl SearchProvider {
    pub const ALL: [SearchProvider; 2] = [SearchProvider::DuckDuckGo, SearchProvider::Google];

    pub fn label(self) -> &'static str {
        match self {
            SearchProvider::DuckDuckGo => "DuckDuckGo",
            SearchProvider::Google => "Google",
        }
    }

    /// The id the engine list (`morph-core` `search.rs`) and approval prompts use.
    pub fn id(self) -> &'static str {
        match self {
            SearchProvider::DuckDuckGo => "duckduckgo",
            SearchProvider::Google => "google",
        }
    }
}

/// Settings shared by every companion of a profile.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AgentSettings {
    /// The web provider after the Library. Research always searches the Library first.
    #[serde(default)]
    pub search_provider: SearchProvider,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AiStore, FileStore, MemoryStore};

    #[test]
    fn duckduckgo_is_the_default_and_unknown_fields_are_tolerated() {
        assert_eq!(AgentSettings::default().search_provider, SearchProvider::DuckDuckGo);
        let s: AgentSettings = serde_json::from_str(r#"{"future":1}"#).unwrap();
        assert_eq!(s, AgentSettings::default());
        assert_eq!(serde_json::to_string(&AgentSettings { search_provider: SearchProvider::Google }).unwrap(), r#"{"search_provider":"google"}"#);
    }

    #[test]
    fn settings_round_trip_through_both_stores() {
        let dir = tempfile::tempdir().unwrap();
        let file = FileStore::new(dir.path().join("companions"));
        assert_eq!(file.load_settings().unwrap(), None);
        let s = AgentSettings { search_provider: SearchProvider::Google };
        file.save_settings(&s).unwrap();
        assert_eq!(FileStore::new(dir.path().join("companions")).load_settings().unwrap(), Some(s.clone()));
        let mem = MemoryStore::default();
        mem.save_settings(&s).unwrap();
        assert_eq!(mem.load_settings().unwrap(), Some(s));
    }

    #[test]
    fn provider_ids_match_the_engine_list() {
        for p in SearchProvider::ALL {
            assert!(!p.label().is_empty() && !p.id().is_empty());
        }
    }
}
