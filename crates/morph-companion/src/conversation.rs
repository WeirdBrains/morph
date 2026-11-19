//! The transcript the Companion panel draws for one companion, built from [`ViewUpdate`]s.
//! Pure state: the panel reads it and the host feeds it.

use crate::markdown::Markdown;
use crate::view::{ApprovalCard, FootnoteView, NoticeView, TraceRowView, ViewUpdate};

/// A page the user attached to the conversation (the current tab).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedPage {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done,
    Failed,
}

/// One thing in the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    User {
        text: String,
        attached: Option<AttachedPage>,
    },
    Assistant {
        text: String,
        /// `text` as blocks; call [`Conversation::settle`] once per frame to bring it up to date.
        doc: Markdown,
        footnotes: Vec<FootnoteView>,
    },
    /// An approval card; `answer` is set once the user picked an offer or declined.
    Card {
        card: ApprovalCard,
        answer: Option<String>,
    },
    Notice(NoticeView),
    Blocked(String),
    Tool {
        call_id: String,
        tool: String,
        state: ToolState,
    },
    Error(String),
    /// The task's page budget ran out: the "+5 pages" card (`taken` once the user took it).
    MorePages {
        extension: u32,
        taken: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Conversation {
    entries: Vec<Entry>,
    trace: Vec<TraceRowView>,
    running: bool,
    status: Option<String>,
    attached: Option<AttachedPage>,
}

impl Conversation {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Trace rows of the task that ran last, for the trace view.
    pub fn trace(&self) -> &[TraceRowView] {
        &self.trace
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// How the last task ended ("done", "stopped"...), once it has.
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// The page the next question will carry, if the user attached one.
    pub fn attached(&self) -> Option<&AttachedPage> {
        self.attached.as_ref()
    }

    pub fn attach(&mut self, page: Option<AttachedPage>) {
        self.attached = page;
    }

    /// The card waiting for an answer, if any.
    pub fn pending_card(&self) -> Option<&ApprovalCard> {
        self.entries.iter().rev().find_map(|e| match e {
            Entry::Card { card, answer: None } => Some(card),
            _ => None,
        })
    }

    /// Starts a task: adds the question (with the attached page, then cleared: attaching is explicit
    /// per question) and clears the last trace.
    pub fn start_task(&mut self, text: &str) {
        let attached = self.attached.take();
        self.entries.push(Entry::User { text: text.to_string(), attached });
        self.trace.clear();
        self.status = None;
        self.running = true;
    }

    /// Parse text that arrived since the last call (the tail of the streaming answer only). The
    /// panel calls this once per frame, however many deltas came in.
    pub fn settle(&mut self) {
        for e in &mut self.entries {
            if let Entry::Assistant { doc, .. } = e {
                doc.settle();
            }
        }
    }

    /// Says what the waiting card is about (the tool, and for tier 2 what it acts on).
    pub fn set_card_detail(&mut self, detail: &str) {
        if let Some(Entry::Card { card, .. }) = self.entries.iter_mut().rev().find(|e| matches!(e, Entry::Card { answer: None, .. })) {
            card.detail = Some(detail.to_string());
        }
    }

    /// Records the user's answer on the waiting card.
    pub fn answer_card(&mut self, label: &str) {
        if let Some(Entry::Card { answer, .. }) = self.entries.iter_mut().rev().find(|e| matches!(e, Entry::Card { answer: None, .. })) {
            *answer = Some(label.to_string());
        }
    }

    /// Shows the checked footnotes under the last answer.
    pub fn set_footnotes(&mut self, notes: Vec<FootnoteView>) {
        if let Some(Entry::Assistant { footnotes, .. }) = self.entries.iter_mut().rev().find(|e| matches!(e, Entry::Assistant { .. })) {
            *footnotes = notes;
        }
    }

    /// The task finished: the last answer becomes `text` (its sources list taken out and unbacked
    /// markers dropped, see [`crate::parse_citations`]) with the checked footnotes under it.
    /// Only this task's answer changes, and an empty final text (a task stopped by its budget or
    /// the Stop button) keeps what had streamed.
    pub fn finish_answer(&mut self, text: &str, notes: Vec<FootnoteView>) {
        let answer = self.entries.iter_mut().rev().take_while(|e| !matches!(e, Entry::User { .. })).find(|e| matches!(e, Entry::Assistant { .. }));
        if let Some(Entry::Assistant { text: t, doc, footnotes }) = answer {
            if text.trim().is_empty() {
                return;
            }
            if t != text {
                *t = text.to_string();
                *doc = Markdown::new();
                doc.push_str(text);
            }
            *footnotes = notes;
        }
    }

    /// The task that just finished wanted more pages than its budget: offer `extension` more.
    pub fn offer_more_pages(&mut self, extension: u32) {
        self.entries.push(Entry::MorePages { extension, taken: false });
    }

    /// The user took the waiting "+5 pages" offer: its extension, once (`None` when there is no
    /// offer waiting or a task is running). Offers before the last question no longer apply.
    pub fn take_more_pages(&mut self) -> Option<u32> {
        if self.running {
            return None;
        }
        match self.entries.iter_mut().rev().take_while(|e| !matches!(e, Entry::User { .. })).find(|e| matches!(e, Entry::MorePages { .. })) {
            Some(Entry::MorePages { extension, taken }) if !*taken => {
                *taken = true;
                Some(*extension)
            }
            _ => None,
        }
    }

    /// The task could not start or failed outside the agent loop.
    pub fn fail(&mut self, message: &str) {
        self.entries.push(Entry::Error(message.to_string()));
        self.running = false;
    }

    pub fn clear(&mut self) {
        *self = Self { attached: self.attached.take(), ..Self::default() };
    }

    pub fn apply(&mut self, update: ViewUpdate) {
        match update {
            ViewUpdate::TaskStarted { .. } => self.running = true,
            ViewUpdate::Text(delta) => match self.entries.last_mut() {
                Some(Entry::Assistant { text, doc, .. }) => {
                    text.push_str(&delta);
                    doc.push_str(&delta);
                }
                _ => {
                    let mut doc = Markdown::new();
                    doc.push_str(&delta);
                    self.entries.push(Entry::Assistant { text: delta, doc, footnotes: Vec::new() });
                }
            },
            ViewUpdate::Approval(card) => self.entries.push(Entry::Card { card, answer: None }),
            ViewUpdate::Notice(n) => self.entries.push(Entry::Notice(n)),
            ViewUpdate::Blocked(text) => self.entries.push(Entry::Blocked(text)),
            ViewUpdate::ToolRunning { call_id, tool } => self.entries.push(Entry::Tool { call_id, tool, state: ToolState::Running }),
            ViewUpdate::ToolDone { call_id, is_error, .. } => {
                if let Some(Entry::Tool { state, .. }) =
                    self.entries.iter_mut().rev().find(|e| matches!(e, Entry::Tool { call_id: c, .. } if *c == call_id))
                {
                    *state = if is_error { ToolState::Failed } else { ToolState::Done };
                }
            }
            ViewUpdate::Trace(row) => self.trace.push(row),
            ViewUpdate::Finished { status, .. } => {
                self.running = false;
                self.status = Some(status);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weirdnet_agent::Scope;

    use crate::view::OfferView;

    fn card() -> ApprovalCard {
        ApprovalCard {
            companion: "Local".into(),
            title: "Local wants to open pages".into(),
            site: Some("a.com".into()),
            signed_in: false,
            detail: None,
            offers: vec![OfferView { scope: Scope::Once, label: "Allow once".into() }],
        }
    }

    fn page() -> AttachedPage {
        AttachedPage { url: "https://a.com/".into(), title: "A".into() }
    }

    #[test]
    fn streamed_text_is_parsed_once_per_settle_and_matches_the_final_text() {
        let mut c = Conversation::default();
        c.apply(ViewUpdate::Text("Four is **".into()));
        c.apply(ViewUpdate::Text("four**.\n\n- a\n- b".into()));
        let Some(Entry::Assistant { doc, text, .. }) = c.entries().last() else { panic!() };
        assert!(doc.is_dirty() && doc.blocks().count() == 0, "nothing is parsed before settle");
        assert_eq!(text, doc.source());
        c.settle();
        let Some(Entry::Assistant { doc, .. }) = c.entries().last() else { panic!() };
        assert!(!doc.is_dirty());
        assert_eq!(doc.blocks().count(), 2);
    }

    #[test]
    fn text_deltas_extend_one_answer_and_a_new_task_starts_a_new_entry() {
        let mut c = Conversation::default();
        c.start_task("hi");
        c.apply(ViewUpdate::Text("Hel".into()));
        c.apply(ViewUpdate::Text("lo".into()));
        assert!(matches!(&c.entries()[1], Entry::Assistant { text, .. } if text == "Hello"));
        c.apply(ViewUpdate::Finished { task_id: "t".into(), status: "done".into() });
        assert!(!c.is_running());
        assert_eq!(c.status(), Some("done"));
        c.start_task("again");
        assert_eq!(c.status(), None);
        c.apply(ViewUpdate::Text("x".into()));
        assert_eq!(c.entries().len(), 4);
    }

    #[test]
    fn attaching_is_explicit_per_question() {
        let mut c = Conversation::default();
        c.attach(Some(page()));
        c.start_task("summarise this");
        assert!(matches!(&c.entries()[0], Entry::User { attached: Some(p), .. } if p.title == "A"));
        assert!(c.attached().is_none(), "the next question carries nothing unless attached again");
    }

    #[test]
    fn a_card_waits_until_answered() {
        let mut c = Conversation::default();
        c.apply(ViewUpdate::Approval(card()));
        assert!(c.pending_card().is_some());
        c.answer_card("Allow once");
        assert!(c.pending_card().is_none());
        assert!(matches!(&c.entries()[0], Entry::Card { answer: Some(a), .. } if a == "Allow once"));
    }

    #[test]
    fn tools_notices_and_blocks_are_listed_and_tools_settle() {
        let mut c = Conversation::default();
        c.apply(ViewUpdate::ToolRunning { call_id: "1".into(), tool: "open_url".into() });
        c.apply(ViewUpdate::Notice(NoticeView { text: "n".into(), allow_id: 3 }));
        c.apply(ViewUpdate::Blocked("b".into()));
        c.apply(ViewUpdate::ToolDone { call_id: "1".into(), tool: "open_url".into(), is_error: true });
        assert!(matches!(&c.entries()[0], Entry::Tool { state: ToolState::Failed, .. }));
        assert!(matches!(&c.entries()[1], Entry::Notice(n) if n.allow_id == 3));
        assert!(matches!(&c.entries()[2], Entry::Blocked(_)));
    }

    #[test]
    fn trace_resets_per_task_and_footnotes_attach_to_the_last_answer() {
        let mut c = Conversation::default();
        c.start_task("q");
        c.apply(ViewUpdate::Trace(TraceRowView { step: 1, text: "s".into(), session: "logged out".into() }));
        c.apply(ViewUpdate::Text("a [1]".into()));
        c.set_footnotes(vec![FootnoteView { n: 1, label: "u".into(), url: Some("u".into()), steps: vec![1] }]);
        assert!(matches!(&c.entries()[1], Entry::Assistant { footnotes, .. } if footnotes.len() == 1));
        assert_eq!(c.trace().len(), 1);
        c.start_task("q2");
        assert!(c.trace().is_empty());
    }

    #[test]
    fn failure_ends_the_run_and_clear_keeps_the_attachment() {
        let mut c = Conversation::default();
        c.start_task("q");
        c.fail("no model");
        assert!(!c.is_running());
        assert!(matches!(c.entries().last(), Some(Entry::Error(m)) if m == "no model"));
        c.attach(Some(page()));
        c.clear();
        assert!(c.entries().is_empty());
        assert!(c.attached().is_some());
    }

    #[test]
    fn finishing_keeps_a_stopped_answer_and_never_touches_an_earlier_task() {
        let mut c = Conversation::default();
        c.start_task("one");
        c.apply(ViewUpdate::Text("first answer".into()));
        c.finish_answer("first answer [1]", Vec::new());
        c.start_task("two");
        c.apply(ViewUpdate::Text("partial".into()));
        c.finish_answer("", Vec::new());
        assert!(matches!(c.entries().last(), Some(Entry::Assistant { text, .. }) if text == "partial"));
        c.start_task("three");
        c.finish_answer("nothing streamed", Vec::new());
        let texts: Vec<&str> =
            c.entries().iter().filter_map(|e| if let Entry::Assistant { text, .. } = e { Some(text.as_str()) } else { None }).collect();
        assert_eq!(texts, vec!["first answer [1]", "partial"]);
    }

    #[test]
    fn a_more_pages_offer_is_taken_once_and_only_after_the_task_and_before_the_next_question() {
        let mut c = Conversation::default();
        c.start_task("q");
        c.offer_more_pages(5);
        assert_eq!(c.take_more_pages(), None, "not while the task runs");
        c.apply(ViewUpdate::Finished { task_id: "t".into(), status: "done".into() });
        assert!(matches!(c.entries().last(), Some(Entry::MorePages { extension: 5, taken: false })));
        assert_eq!(c.take_more_pages(), Some(5));
        assert!(matches!(c.entries().last(), Some(Entry::MorePages { taken: true, .. })));
        assert_eq!(c.take_more_pages(), None);
        c.offer_more_pages(5);
        c.start_task("another question");
        c.apply(ViewUpdate::Finished { task_id: "t2".into(), status: "done".into() });
        assert_eq!(c.take_more_pages(), None, "an offer from before the last question is gone");
    }
}
