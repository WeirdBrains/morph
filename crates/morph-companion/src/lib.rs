//! Morph's Companion sessions: runs tasks for named companions and turns the agent's events
//! into plain view models the Companion panel draws. No UI types here, so it tests without a window.
//!
//! - [`view`]: view models and the event translation.
//! - [`session`]: [`Sessions`], one conversation per companion, with cancel.
//! - [`export`]: Markdown export of a task with its footnotes and trace.

pub mod export;
pub mod session;
pub mod view;

pub use export::export_markdown;
pub use session::{SessionError, Sessions};
pub use view::{ApprovalCard, FootnoteView, NoticeView, OfferView, TraceRowView, ViewUpdate, footnotes, trace_rows, translate};
