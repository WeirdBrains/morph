//! Sync type names of Morph's companion records (companions, grants, chats, MCP configs, MCP
//! secrets and journals). The records and their merge live in `morph-services` `companion_sync`, which
//! knows the `morph-ai` files; `morph-core` only needs the names for the sync defaults.

pub const COMPANIONS: &str = "morph-companions";
pub const GRANTS: &str = "morph-grants";
pub const CHATS: &str = "morph-chats";
pub const MCP: &str = "morph-mcp";
pub const JOURNAL: &str = "morph-journal";
/// MCP secret values, a sealed record type (only devices holding the current epoch key read
/// them). The configs in [`MCP`] never carry them.
pub const MCP_SECRETS: &str = "morph-mcp-secrets";
pub const TYPES: &[&str] = &[COMPANIONS, GRANTS, CHATS, MCP, MCP_SECRETS, JOURNAL];
/// Types where every op of a revoked writer is void, whenever it was written.
pub const SENSITIVE: &[&str] = &[COMPANIONS, GRANTS, MCP, MCP_SECRETS];
