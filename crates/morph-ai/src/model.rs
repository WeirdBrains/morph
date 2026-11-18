//! Which model a companion runs, and the seam to the inference crate.
//!
//! `weirdnet-model` is not a dependency yet; the app adapts it behind [`ModelProvider`].

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use weirdnet_agent::{ModelBackend, ModelError};

/// Device class, which picks the default model size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Desktop,
    Phone,
}

/// A model a companion can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelChoice {
    Gemma4E4B,
    Gemma4E2B,
    /// Any other model by registry id (for example a Hugging Face repo).
    Custom(String),
}

impl ModelChoice {
    /// E4B on desktop, E2B on phones.
    pub fn default_for(platform: Platform) -> Self {
        match platform {
            Platform::Desktop => ModelChoice::Gemma4E4B,
            Platform::Phone => ModelChoice::Gemma4E2B,
        }
    }

    /// Short name for the UI and traces.
    pub fn label(&self) -> String {
        match self {
            ModelChoice::Gemma4E4B => "Gemma 4 E4B".into(),
            ModelChoice::Gemma4E2B => "Gemma 4 E2B".into(),
            ModelChoice::Custom(id) => id.clone(),
        }
    }
}

/// Opens a model for a choice. The app implements it over `weirdnet-model`; tests use scripted models.
pub trait ModelProvider: Send + Sync {
    fn open(&self, choice: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_platform() {
        assert_eq!(ModelChoice::default_for(Platform::Desktop), ModelChoice::Gemma4E4B);
        assert_eq!(ModelChoice::default_for(Platform::Phone), ModelChoice::Gemma4E2B);
    }

    #[test]
    fn labels_and_serde() {
        assert_eq!(ModelChoice::Gemma4E2B.label(), "Gemma 4 E2B");
        let c = ModelChoice::Custom("org/m".into());
        assert_eq!(c.label(), "org/m");
        let back: ModelChoice = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(c, back);
    }
}
