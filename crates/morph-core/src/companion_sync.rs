//! Sync type names of Morph's companion records (companions, grants, chats, MCP configs and
//! journals). The records and their merge live in `morph-services` `companion_sync`, which
//! knows the `morph-ai` files; `morph-core` only needs the names for the sync defaults.

pub const COMPANIONS: &str = "morph-companions";
pub const GRANTS: &str = "morph-grants";
pub const CHATS: &str = "morph-chats";
pub const MCP: &str = "morph-mcp";
pub const JOURNAL: &str = "morph-journal";
pub const TYPES: &[&str] = &[COMPANIONS, GRANTS, CHATS, MCP, JOURNAL];
/// Types where every op of a revoked writer is void, whenever it was written.
pub const SENSITIVE: &[&str] = &[COMPANIONS, GRANTS, MCP];
