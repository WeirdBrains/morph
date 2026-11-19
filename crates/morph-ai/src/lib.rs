//! Morph's Companion foundations: who the companions are, what they may do, and the
//! interfaces they run on. No UI and no I/O beyond a pluggable [`AiStore`].
//!
//! - [`roster`]: named companions (Local, Node, External) and the default one.
//! - [`model`]: model choices (Gemma 4 E4B on desktop, E2B on phones) and the
//!   [`ModelProvider`] that turns a choice into a `weirdnet_agent::ModelBackend`.
//! - [`store`]: [`AiStore`] and [`Grants`], per-companion per-site levels and standing allows.
//! - [`host`]: [`BrowserHost`] and [`ResearchHost`], what the app provides to tools.
//! - [`chats`], [`journal`], [`mcp`]: saved chats, Notes and project journals (Save to Notes,
//!   Save to journal), and MCP server configs; all sync (`morph-services` `companion_sync`).
//! - [`mcp_local`] and `mcp_runtime` (feature `mcp`): per-machine MCP secrets and approvals, and
//!   connecting the configured servers into a tool registry.
//! - [`remote`]: [`NodeRouter`], routing for Node companions and inference offload.

pub mod chats;
mod file_io;
pub mod host;
mod ids;
pub mod journal;
#[cfg(feature = "local")]
pub mod local;
#[cfg(feature = "local")]
pub mod local_chat;
pub mod mcp;
pub mod mcp_local;
#[cfg(feature = "mcp")]
pub mod mcp_runtime;
pub mod model;
#[cfg(feature = "local")]
pub mod native_calls;
pub mod remote;
pub mod research;
pub mod roster;
pub mod settings;
pub mod steer;
pub mod store;

pub use chats::{CLIP_NOTE, ChatLog, ChatTurn, clip_markdown};
pub use file_io::{FileIo, PlainIo};
pub use host::{
    ActedOn, BrowserHost, FetchedPage, HostError, LIBRARY, PageElement, PageLink, PageSnapshot, PageTarget, ResearchHost, ScrollTo, SearchHit,
    TabInfo,
};
pub use ids::new_id;
pub use journal::{Journal, JournalEntry, JournalKind, Journals, NOTES_ID};
pub use mcp::{McpConfigs, McpSecrets, McpServer, McpTransport, McpValue};
pub use mcp_local::{APPROVAL_KEY_CONTEXT, FileSecrets, McpApprovals, MemorySecrets, write_private};
pub use model::{ModelChoice, ModelProvider, Platform, UnavailableModels};
#[cfg(feature = "local")]
pub use native_calls::{NativeCalls, split_native_calls};
pub use remote::{
    CompanionRoute, Link, LinkHandler, LinkedNode, NodeRouter, OffloadMode, OffloadPlan, OffloadSettings, RouteError, offload_plan, route_companion,
};
pub use roster::{CompanionProfile, Roster, RosterError};
pub use settings::{AgentSettings, DEFAULT_SITE_RATE, SITE_RATE_RANGE, SITE_RATE_STEPS, SearchProvider};
pub use store::{
    AiStore, FileStore, Grants, MemoryStore, StoreError, forget_vanished_companions, rebase_policy, rebase_shared, reload_shared, remove_companion,
    revoke_shared, save_shared,
};

#[cfg(feature = "local")]
pub use local::{CpuLimit, EngineFactory, LocalModels, ModelStatus, cpu_limit};
