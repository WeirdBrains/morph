//! The transcript the Companion panel draws for one companion, built from [`ViewUpdate`]s.
//! Pure state: the panel reads it and the host feeds it.

use std::time::Instant;

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
    /// `asking`: the running task waits on it (weirdnet-agent's `extend_pages`) until the user
    /// answers; `declined` once they said no or the task ended first. Otherwise it is offered
    /// after the task, as a follow-on task.
    MorePages {
        extension: u32,
        taken: bool,
        asking: bool,
        declined: bool,
    },
}

/// What a running task is doing now, for the panel's working line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activity {
    /// The model is on a turn and has not written anything in it yet.
    Thinking,
    /// The model is reasoning before it answers: its latest words.
    Reasoning(String),
    /// The model's answer is streaming in.
    Writing,
    /// A tool call is running ("Searching DuckDuckGo…").
    Doing(String),
    /// An approval card waits for the user.
    WaitingForApproval,
}

impl Activity {
    /// The line when the turn runs on another of the user's devices ("Thinking on my-desktop…").
    pub fn label_on(&self, loading: Option<&str>, device: Option<&str>) -> String {
        match (self, device) {
            (Activity::Thinking, Some(d)) => format!("Thinking on {d}…"),
            (Activity::Reasoning(tail), Some(d)) => format!("Thinking on {d}: {tail}"),
            _ => self.label(loading),
        }
    }

    /// The line the panel shows. `loading` names the local model while it is still loading.
    pub fn label(&self, loading: Option<&str>) -> String {
        match (self, loading) {
            (Activity::Thinking, Some(model)) => format!("Loading {model}…"),
            (Activity::Thinking, None) => "Thinking…".into(),
            (Activity::Reasoning(tail), _) => format!("Thinking: {tail}"),
            (Activity::Writing, _) => "Writing…".into(),
            (Activity::Doing(text), _) => text.clone(),
            (Activity::WaitingForApproval, _) => "Waiting for your approval".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Conversation {
    entries: Vec<Entry>,
    trace: Vec<TraceRowView>,
    running: bool,
    status: Option<String>,
    attached: Option<AttachedPage>,
    /// The step line of the tool call that is running.
    doing: Option<String>,
    /// Text has come in since the model's turn began.
    writing: bool,
    /// The latest words of the model's reasoning this turn (the tail only).
    reasoning: String,
    /// When the current [`Activity`] began.
    since: Option<Instant>,
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

    /// What the running task is doing and since when; `None` when no task runs.
    pub fn activity(&self) -> Option<(Activity, Instant)> {
        let now = self.kind()?;
        Some((now, self.since.unwrap_or_else(Instant::now)))
    }

    fn kind(&self) -> Option<Activity> {
        if !self.running {
            return None;
        }
        Some(if self.pending_card().is_some() || self.asking_more_pages() {
            Activity::WaitingForApproval
        } else if let Some(d) = &self.doing {
            Activity::Doing(d.clone())
        } else if self.writing {
            Activity::Writing
        } else if !self.reasoning.is_empty() {
            Activity::Reasoning(self.reasoning.clone())
        } else {
            Activity::Thinking
        })
    }

    /// Runs `change`, restarting the activity clock when what the task is doing changed.
    fn track(&mut self, change: impl FnOnce(&mut Self)) {
        let before = self.kind();
        change(self);
        if self.kind() != before {
            self.since = Some(Instant::now());
        }
    }

    /// The step line for the tool call that just started ("Reading tokio.rs…").
    pub fn set_doing(&mut self, text: &str) {
        if self.running {
            self.track(|c| c.doing = Some(text.to_string()));
        }
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
        (self.doing, self.writing, self.since) = (None, false, Some(Instant::now()));
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
        self.track(|c| {
            if let Some(Entry::Card { answer, .. }) = c.entries.iter_mut().rev().find(|e| matches!(e, Entry::Card { answer: None, .. })) {
                *answer = Some(label.to_string());
            }
        });
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
        self.entries.push(Entry::MorePages { extension, taken: false, asking: false, declined: false });
    }

    /// The running task used its page budget and waits for the user: "+`extension` pages" or no.
    pub fn ask_more_pages(&mut self, extension: u32) {
        self.track(|c| c.entries.push(Entry::MorePages { extension, taken: false, asking: true, declined: false }));
    }

    /// Whether the running task waits on a "+5 pages" answer.
    pub fn asking_more_pages(&self) -> bool {
        self.entries.iter().any(|e| matches!(e, Entry::MorePages { asking: true, taken: false, declined: false, .. }))
    }

    /// The answer to the card the running task waits on (`false` too when the task ended first).
    /// Returns whether one was waiting.
    pub fn answer_more_pages(&mut self, yes: bool) -> bool {
        let mut found = false;
        self.track(|c| {
            if let Some(Entry::MorePages { taken, declined, .. }) =
                c.entries.iter_mut().find(|e| matches!(e, Entry::MorePages { asking: true, taken: false, declined: false, .. }))
            {
                (*taken, *declined, found) = (yes, !yes, true);
            }
        });
        found
    }

    /// Whether the task since the last question already asked for more pages while it ran, so no
    /// follow-on offer is made after it.
    pub fn asked_more_pages(&self) -> bool {
        self.entries.iter().rev().take_while(|e| !matches!(e, Entry::User { .. })).any(|e| matches!(e, Entry::MorePages { asking: true, .. }))
    }

    /// The user took the waiting "+5 pages" offer: its extension, once (`None` when there is no
    /// offer waiting or a task is running). Offers before the last question no longer apply.
    pub fn take_more_pages(&mut self) -> Option<u32> {
        if self.running {
            return None;
        }
        match self.entries.iter_mut().rev().take_while(|e| !matches!(e, Entry::User { .. })).find(|e| matches!(e, Entry::MorePages { .. })) {
            Some(Entry::MorePages { extension, taken, asking: false, .. }) if !*taken => {
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
        (self.doing, self.writing) = (None, false);
    }

    pub fn clear(&mut self) {
        *self = Self { attached: self.attached.take(), ..Self::default() };
    }

    pub fn apply(&mut self, update: ViewUpdate) {
        self.track(|c| c.apply_now(update));
    }

    fn apply_now(&mut self, update: ViewUpdate) {
        match update {
            ViewUpdate::TaskStarted { .. } => self.running = true,
            ViewUpdate::Text(delta) if delta.is_empty() => {}
            ViewUpdate::Reasoning(delta) => {
                self.reasoning.push_str(&delta);
                // Only the tail: one line of the panel.
                let mut flat = String::new();
                for ch in self.reasoning.chars() {
                    if !ch.is_whitespace() {
                        flat.push(ch);
                    } else if !flat.ends_with(' ') {
                        flat.push(' ');
                    }
                }
                let n = flat.chars().count();
                self.reasoning = flat.chars().skip(n.saturating_sub(70)).collect();
            }
            ViewUpdate::Text(delta) => {
                self.writing = true;
                self.reasoning.clear();
                match self.entries.last_mut() {
                    Some(Entry::Assistant { text, doc, .. }) => {
                        text.push_str(&delta);
                        doc.push_str(&delta);
                    }
                    _ => {
                        let mut doc = Markdown::new();
                        doc.push_str(&delta);
                        self.entries.push(Entry::Assistant { text: delta, doc, footnotes: Vec::new() });
                    }
                }
            }
            ViewUpdate::Approval(card) => self.entries.push(Entry::Card { card, answer: None }),
            ViewUpdate::Notice(n) => self.entries.push(Entry::Notice(n)),
            ViewUpdate::Blocked(text) => self.entries.push(Entry::Blocked(text)),
            ViewUpdate::ToolRunning { call_id, tool } => {
                // The tool's own step line may have come first; it says more than the name.
                self.doing.get_or_insert_with(|| format!("Running {tool}…"));
                self.writing = false;
                self.reasoning.clear();
                self.entries.push(Entry::Tool { call_id, tool, state: ToolState::Running });
            }
            ViewUpdate::ToolDone { call_id, is_error, .. } => {
                // The model takes its next turn.
                (self.doing, self.writing) = (None, false);
                self.reasoning.clear();
                if let Some(Entry::Tool { state, .. }) =
                    self.entries.iter_mut().rev().find(|e| matches!(e, Entry::Tool { call_id: c, .. } if *c == call_id))
                {
                    *state = if is_error { ToolState::Failed } else { ToolState::Done };
                }
            }
            ViewUpdate::Trace(row) => self.trace.push(row),
            ViewUpdate::Redo => {
                // The answer from search results goes; the tool rows stay as what was done.
                let start = self.entries.iter().rposition(|e| matches!(e, Entry::User { .. })).map_or(0, |i| i + 1);
                let mut i = start;
                while i < self.entries.len() {
                    if matches!(self.entries[i], Entry::Assistant { .. }) {
                        self.entries.remove(i);
                    } else {
                        i += 1;
                    }
                }
                (self.doing, self.writing) = (None, false);
            }
            ViewUpdate::Finished { status, .. } => {
                self.running = false;
                (self.doing, self.writing) = (None, false);
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
    fn a_more_pages_ask_while_the_task_runs_is_answered_once_and_is_never_a_follow_on_offer() {
        let mut c = Conversation::default();
        c.start_task("q");
        c.ask_more_pages(5);
        assert!(c.asking_more_pages());
        assert!(matches!(c.activity(), Some((Activity::WaitingForApproval, _))));
        assert!(c.answer_more_pages(true));
        assert!(!c.asking_more_pages());
        assert!(!c.answer_more_pages(false), "answered once");
        assert!(matches!(c.entries().last(), Some(Entry::MorePages { taken: true, declined: false, .. })));
        assert!(c.asked_more_pages());
        c.ask_more_pages(5);
        assert!(c.answer_more_pages(false));
        assert!(matches!(c.entries().last(), Some(Entry::MorePages { taken: false, declined: true, .. })));
        c.finish_answer("a", vec![]);
        assert_eq!(c.take_more_pages(), None, "a mid-task card is not a follow-on");
        c.start_task("next");
        assert!(!c.asked_more_pages());
    }

    #[test]
    fn a_more_pages_offer_is_taken_once_and_only_after_the_task_and_before_the_next_question() {
        let mut c = Conversation::default();
        c.start_task("q");
        c.offer_more_pages(5);
        assert_eq!(c.take_more_pages(), None, "not while the task runs");
        c.apply(ViewUpdate::Finished { task_id: "t".into(), status: "done".into() });
        assert!(matches!(c.entries().last(), Some(Entry::MorePages { extension: 5, taken: false, .. })));
        assert_eq!(c.take_more_pages(), Some(5));
        assert!(matches!(c.entries().last(), Some(Entry::MorePages { taken: true, .. })));
        assert_eq!(c.take_more_pages(), None);
        c.offer_more_pages(5);
        c.start_task("another question");
        c.apply(ViewUpdate::Finished { task_id: "t2".into(), status: "done".into() });
        assert_eq!(c.take_more_pages(), None, "an offer from before the last question is gone");
    }

    #[test]
    fn the_working_line_names_the_device_a_turn_runs_on() {
        assert_eq!(
            Activity::Thinking.label_on(Some("Gemma 4 E4B"), Some("my-desktop")),
            "Thinking on my-desktop…",
            "not Loading: the model loads over there"
        );
        assert_eq!(Activity::Thinking.label_on(Some("Gemma 4 E4B"), None), "Loading Gemma 4 E4B…");
        assert_eq!(Activity::Reasoning("hmm".into()).label_on(None, Some("my-desktop")), "Thinking on my-desktop: hmm");
        assert_eq!(Activity::Writing.label_on(None, Some("my-desktop")), "Writing…");
    }

    #[test]
    fn reasoning_shows_its_latest_words_on_the_working_line_until_the_answer_starts() {
        let mut c = Conversation::default();
        c.apply(ViewUpdate::TaskStarted { task_id: "t".into() });
        c.apply(ViewUpdate::Reasoning("Let me think about ".into()));
        c.apply(ViewUpdate::Reasoning("the shutdown order.".into()));
        let (a, _) = c.activity().unwrap();
        assert_eq!(a.label(None), "Thinking: Let me think about the shutdown order.");
        c.apply(ViewUpdate::Reasoning("word ".repeat(60)));
        let label = c.activity().unwrap().0.label(None);
        assert!(label.chars().count() <= "Thinking: ".len() + 70, "{label}");
        assert!(c.entries().is_empty(), "reasoning is not part of the answer");
        c.apply(ViewUpdate::Text("Four.".into()));
        assert_eq!(c.activity().unwrap().0, Activity::Writing);
        c.apply(ViewUpdate::ToolRunning { call_id: "1".into(), tool: "web.search".into() });
        c.apply(ViewUpdate::ToolDone { call_id: "1".into(), tool: "web.search".into(), is_error: false });
        assert_eq!(c.activity().unwrap().0, Activity::Thinking, "the next turn starts without the old reasoning");
    }

    #[test]
    fn the_working_line_follows_the_task_from_thinking_to_tools_cards_and_writing() {
        let mut c = Conversation::default();
        assert_eq!(c.activity(), None, "nothing runs");
        c.start_task("q");
        let (a, t0) = c.activity().unwrap();
        assert_eq!(a, Activity::Thinking);
        assert_eq!(a.label(Some("Gemma 4 E4B")), "Loading Gemma 4 E4B…");
        assert_eq!(a.label(None), "Thinking…");
        c.apply(ViewUpdate::Text(String::new()));
        assert_eq!(c.activity().unwrap().0, Activity::Thinking, "an empty delta is not writing");
        c.apply(ViewUpdate::Text("Let me look.".into()));
        assert_eq!(c.activity().unwrap().0, Activity::Writing);
        c.apply(ViewUpdate::Approval(card()));
        assert_eq!(c.activity().unwrap().0.label(None), "Waiting for your approval");
        c.answer_card("Allow once");
        c.set_doing("Searching DuckDuckGo…");
        c.apply(ViewUpdate::ToolRunning { call_id: "c1".into(), tool: "web.search".into() });
        let (a, t1) = c.activity().unwrap();
        assert_eq!(a.label(None), "Searching DuckDuckGo…", "the tool's own line beats its name");
        assert!(t1 >= t0);
        c.apply(ViewUpdate::ToolDone { call_id: "c1".into(), tool: "web.search".into(), is_error: false });
        assert_eq!(c.activity().unwrap().0, Activity::Thinking, "the next model turn");
        c.apply(ViewUpdate::ToolRunning { call_id: "c2".into(), tool: "github.issues".into() });
        assert_eq!(c.activity().unwrap().0.label(None), "Running github.issues…");
        c.apply(ViewUpdate::Finished { task_id: "t".into(), status: "done".into() });
        assert_eq!(c.activity(), None);
    }

    #[test]
    fn the_activity_clock_restarts_only_when_the_activity_changes() {
        let mut c = Conversation::default();
        c.start_task("q");
        c.apply(ViewUpdate::Text("a".into()));
        let (_, t) = c.activity().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        c.apply(ViewUpdate::Text("b".into()));
        c.apply(ViewUpdate::Trace(TraceRowView { step: 1, text: "x".into(), session: "logged out".into() }));
        assert_eq!(c.activity().unwrap().1, t, "still writing");
        c.set_doing("Reading tokio.rs…");
        assert!(c.activity().unwrap().1 > t);
    }
}
