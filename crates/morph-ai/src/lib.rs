//! Morph's Companion foundations: who the companions are, what they may do, and the
//! interfaces they run on. No UI and no I/O beyond a pluggable [`AiStore`].
//!
//! - [`roster`]: named companions (Local, Node, External) and the default one.
//! - [`model`]: model choices (Gemma 4 E4B on desktop, E2B on phones) and the
//!   [`ModelProvider`] that turns a choice into a `weirdnet_agent::ModelBackend`.
//! - [`store`]: [`AiStore`] and [`Grants`], per-companion per-site levels and standing allows.
//! - [`host`]: [`BrowserHost`] and [`ResearchHost`], what the app provides to tools.

pub mod host;
#[cfg(feature = "local")]
pub mod local;
pub mod model;
pub mod roster;
pub mod settings;
pub mod store;

pub use host::{BrowserHost, FetchedPage, HostError, PageSnapshot, ResearchHost, SearchHit};
pub use model::{ModelChoice, ModelProvider, Platform, UnavailableModels};
pub use roster::{CompanionProfile, Roster, RosterError};
pub use settings::{AgentSettings, SearchProvider};
pub use store::{AiStore, FileStore, Grants, MemoryStore, StoreError};

#[cfg(feature = "local")]
pub use local::{EngineFactory, LocalModels, ModelStatus};
