//! Morph's Companion foundations: who the companions are, what they may do, and the
//! interfaces they run on. No UI and no I/O beyond a pluggable [`AiStore`].
//!
//! - [`roster`]: named companions (Local, Node, External) and the default one.
//! - [`model`]: model choices (Gemma 4 E4B on desktop, E2B on phones) and the
//!   [`ModelProvider`] that turns a choice into a `weirdnet_agent::ModelBackend`.
//! - [`store`]: [`AiStore`] and [`Grants`], per-companion per-site levels and standing allows.
//! - [`host`]: [`BrowserHost`] and [`ResearchHost`], what the app provides to tools.
//! - [`chats`], [`journal`], [`mcp`]: saved chats, Notes and project journals (Save to Notes,
//!   Save to journal), and MCP server configs; all sync (`morph-core` `companion_sync`).
//! - [`mcp_local`] and `mcp_runtime` (feature `mcp`): per-machine MCP secrets and approvals, and
//!   connecting the configured servers into a tool registry.
//! - [`remote`]: [`NodeRouter`], routing for Node companions and inference offload.

pub mod chats;
pub mod host;
mod ids;
pub mod journal;
#[cfg(feature = "local")]
pub mod local;
pub mod mcp;
pub mod mcp_local;
#[cfg(feature = "mcp")]
pub mod mcp_runtime;
pub mod model;
pub mod remote;
pub mod roster;
pub mod settings;
pub mod store;

pub use chats::{ChatLog, ChatTurn};
pub use host::{BrowserHost, FetchedPage, HostError, PageSnapshot, ResearchHost, SearchHit};
pub use ids::new_id;
pub use journal::{Journal, JournalEntry, JournalKind, Journals, NOTES_ID};
pub use mcp::{McpConfigs, McpSecrets, McpServer, McpTransport, McpValue};
pub use mcp_local::{FileSecrets, McpApprovals, MemorySecrets, write_private};
pub use model::{ModelChoice, ModelProvider, Platform, UnavailableModels};
pub use remote::{
    CompanionRoute, Link, LinkedNode, NodeRouter, OffloadMode, OffloadPlan, OffloadSettings, RouteError, offload_plan, route_companion,
};
pub use roster::{CompanionProfile, Roster, RosterError};
pub use settings::{AgentSettings, SearchProvider};
pub use store::{AiStore, FileStore, Grants, MemoryStore, StoreError, reload_shared, revoke_shared, save_shared};

#[cfg(feature = "local")]
pub use local::{EngineFactory, LocalModels, ModelStatus};
