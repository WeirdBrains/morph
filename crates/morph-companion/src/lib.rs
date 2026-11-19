//! Morph's Companion sessions: runs tasks for named companions and turns the agent's events
//! into plain view models the Companion panel draws. No UI types here, so it tests without a window.
//!
//! - [`view`]: view models and the event translation.
//! - [`conversation`]: the transcript a panel draws, built from view updates.
//! - [`session`]: [`Sessions`], one conversation per companion, with cancel.
//! - [`export`]: Markdown export of a task with its footnotes and trace, and the saved chat
//!   turn and Notes / journal entry a finished task becomes.

pub mod conversation;
pub mod export;
pub mod markdown;
pub mod session;
pub mod view;

pub use conversation::{AttachedPage, Conversation, Entry, ToolState};
pub use export::{chat_turn, export_markdown, journal_entry};
pub use markdown::{Block, Link, Markdown, Span};
pub use session::{Canceller, SessionError, Sessions};
pub use view::{ApprovalCard, FootnoteView, NoticeView, OfferView, TraceRowView, ViewUpdate, footnotes, trace_rows, translate};
