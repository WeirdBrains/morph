//! Agent settings that are not per companion: where research looks and where model turns run. Stored in the companions
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSettings {
    /// The web provider after the Library. Research always searches the Library first.
    #[serde(default)]
    pub search_provider: SearchProvider,
    /// When model turns go to another of the user's devices.
    #[serde(default)]
    pub offload: crate::remote::OffloadSettings,
    /// The most requests per second Morph makes to any one site, for every companion and every
    /// research project (fractions allowed: 0.2 is one request every 5 seconds). Each wait is
    /// the minimum interval (1 / rate) plus a random 0 to 10% (see `research::SiteLimiter`).
    #[serde(default = "default_site_rate")]
    pub site_rate: f64,
}

/// Default: one request every 2 seconds to any one site.
pub const DEFAULT_SITE_RATE: f64 = 0.5;
/// What the settings offer: 0.1/s (one per 10 s) up to 2/s.
pub const SITE_RATE_STEPS: [f64; 5] = [0.1, 0.2, 0.5, 1.0, 2.0];
/// The lowest and highest rates a saved value is held to.
pub const SITE_RATE_RANGE: (f64, f64) = (0.01, 10.0);

fn default_site_rate() -> f64 {
    DEFAULT_SITE_RATE
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self { search_provider: SearchProvider::default(), offload: Default::default(), site_rate: DEFAULT_SITE_RATE }
    }
}

impl AgentSettings {
    /// The rate to use: the saved one held to [`SITE_RATE_RANGE`] (a damaged or synced value that
    /// is not a number, zero or negative gives the default).
    pub fn site_rate(&self) -> f64 {
        if self.site_rate.is_finite() && self.site_rate > 0.0 {
            self.site_rate.clamp(SITE_RATE_RANGE.0, SITE_RATE_RANGE.1)
        } else {
            DEFAULT_SITE_RATE
        }
    }
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
        assert_eq!(
            serde_json::to_string(&AgentSettings { search_provider: SearchProvider::Google, ..Default::default() }).unwrap(),
            r#"{"search_provider":"google","offload":{"mode":"auto","fallback_local":true},"site_rate":0.5}"#
        );
    }

    #[test]
    fn settings_round_trip_through_both_stores() {
        let dir = tempfile::tempdir().unwrap();
        let file = FileStore::new(dir.path().join("companions"));
        assert_eq!(file.load_settings().unwrap(), None);
        let s = AgentSettings { search_provider: SearchProvider::Google, ..Default::default() };
        file.save_settings(&s).unwrap();
        assert_eq!(FileStore::new(dir.path().join("companions")).load_settings().unwrap(), Some(s.clone()));
        let mem = MemoryStore::default();
        mem.save_settings(&s).unwrap();
        assert_eq!(mem.load_settings().unwrap(), Some(s));
    }

    #[test]
    fn the_site_rate_has_a_default_survives_old_files_and_is_held_in_range() {
        assert_eq!(AgentSettings::default().site_rate(), 0.5);
        let old: AgentSettings = serde_json::from_str(r#"{"search_provider":"google"}"#).unwrap();
        assert_eq!(old.site_rate(), 0.5);
        let fraction: AgentSettings = serde_json::from_str(r#"{"site_rate":0.2}"#).unwrap();
        assert_eq!(fraction.site_rate(), 0.2);
        for (saved, used) in [(0.0, 0.5), (-1.0, 0.5), (f64::NAN, 0.5), (1000.0, 10.0), (0.0001, 0.01)] {
            assert_eq!(AgentSettings { site_rate: saved, ..Default::default() }.site_rate(), used);
        }
        for r in SITE_RATE_STEPS {
            assert!((SITE_RATE_RANGE.0..=SITE_RATE_RANGE.1).contains(&r));
        }
    }

    #[test]
    fn provider_ids_match_the_engine_list() {
        for p in SearchProvider::ALL {
            assert!(!p.label().is_empty() && !p.id().is_empty());
        }
    }
}
