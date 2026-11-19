//! Morph's Companion sessions: runs tasks for named companions and turns the agent's events
//! into plain view models the Companion panel draws. No UI types here, so it tests without a window.
//!
//! - [`view`]: view models and the event translation.
//! - [`conversation`]: the transcript a panel draws, built from view updates.
//! - [`session`]: [`Sessions`], one conversation per companion, with cancel.
//! - [`tools`]: the browser (tiers 0 to 2) and research (tier R) tools over the app's hosts.
//! - [`citations`]: an answer's footnotes, kept only when the trace backs them.
//! - [`export`]: Markdown export of a task with its footnotes and trace, and the saved chat
//!   turn and Notes / journal entry a finished task becomes.

pub mod citations;
pub mod conversation;
pub mod export;
pub mod markdown;
pub mod session;
pub mod tools;
pub mod view;

pub use citations::{Footnoted, parse_citations};
pub use conversation::{Activity, AttachedPage, Conversation, Entry, ToolState};
pub use export::{chat_turn, export_markdown, journal_entry};
pub use markdown::{Block, Link, Markdown, Span};
pub use session::{
    Canceller, MORE_PAGES_PROMPT, READ_NUDGE, SYSTEM_PROMPT, SessionError, Sessions, TaskOptions, answered_without_reading, more_pages_prompt,
    ran_out_of_pages,
};
pub use tools::{BROWSER, PendingTarget, WEB, browser_tools, untrusted, web_tools};
pub use view::{
    ApprovalCard, FootnoteView, NoticeView, OfferView, TraceRowView, ViewUpdate, doing_text, footnotes, provider_label, trace_rows, translate,
};
