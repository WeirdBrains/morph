//! The Companion panel (`morph.companions`): the right-hand slot that stays across tabs. It
//! draws what `morph-companion` builds (the roster picker, the conversation, approval cards,
//! skipped-approval notices, footnotes and the trace) and hands the user's questions and card
//! answers to the worker in `companion_host`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Subscription, Window, div, prelude::*, px, relative, rgb};
use morph_ai::{AiStore, EngineFactory, FileStore, Grants, LocalModels, ModelChoice, ModelStatus, Platform, Roster};
use morph_companion::{
    ApprovalCard, AttachedPage, Conversation, Entry, FootnoteView, NoticeView, OfferView, ToolState, TraceRowView, ViewUpdate, footnotes,
};
use morph_ui::theme::Theme;
use weirdnet_agent::{AgentId, ApprovalAnswer};

use crate::companion_host::{Msg, Worker};
use crate::shell::{Page, ShellRequest};
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::{button, chip, heading, icon};

pub struct CompanionView {
    theme: Theme,
    roster: Roster,
    selected: AgentId,
    conversations: HashMap<AgentId, Conversation>,
    /// The tab the shell shows now; attaching copies it into the conversation.
    page: Option<Page>,
    trace_open: bool,
    /// The footnote a marker in an answer pointed at.
    focus_note: Option<u32>,
    input: Entity<TextInput>,
    worker: Worker,
    models: Arc<LocalModels>,
    /// `MORPH_COMPANION_AUTOASK=<question>`: a dev flag that sends one question when the panel first polls.
    autoask: Option<String>,
    store: FileStore,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShellRequest> for CompanionView {}

impl CompanionView {
    pub fn new(home: &Path, profile: &str, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = FileStore::new(morph_core::profiles::profile_dir(home, profile).join("companions"));
        let roster = match store.load_roster() {
            Ok(Some(r)) if r.default_companion().is_some() => r,
            _ => Roster::first_run(ModelChoice::default_for(Platform::Desktop)),
        };
        let policy = Grants::load(Arc::new(FileStore::new(morph_core::profiles::profile_dir(home, profile).join("companions"))))
            .map(|g| g.shared())
            .unwrap_or_else(|_| weirdnet_agent::Policy::new().shared());
        let factory: EngineFactory = Arc::new(|path| {
            weirdnet_model::llama::LlamaEngine::load(path, weirdnet_model::llama::LlamaOptions::default())
                .map(|e| Box::new(e) as Box<dyn weirdnet_model::Engine>)
        });
        let models = Arc::new(LocalModels::new(weirdnet_model::ModelStore::from_env(), factory));
        let worker = Worker::start(models.clone(), policy);
        let selected = roster.default_companion().map(|c| c.id().clone()).unwrap_or_else(|| AgentId::new("local"));
        let input = cx.new(|cx| TextInput::new("Ask a companion", theme, cx));
        let sub = cx.subscribe_in(&input, window, |this, _, event, _, cx| match event {
            TextInputEvent::Submit { .. } => this.submit(cx),
            TextInputEvent::Cancel => cx.emit(ShellRequest::Run(morph_ui::actions::Action::ToggleCompanions)),
            TextInputEvent::Tab { .. } | TextInputEvent::Changed => {}
        });
        let mut view = Self {
            theme,
            roster,
            selected,
            conversations: HashMap::new(),
            page: None,
            trace_open: false,
            focus_note: None,
            input,
            worker,
            models,
            autoask: std::env::var("MORPH_COMPANION_AUTOASK").ok().filter(|q| !q.trim().is_empty()),
            store,
            focus: cx.focus_handle(),
            _subscriptions: vec![sub],
        };
        if std::env::var_os("MORPH_COMPANION_DEMO").is_some_and(|v| v == "1") {
            let id = view.selected.clone();
            view.conversations.insert(id, demo_conversation());
        }
        view
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        self.input.update(cx, |i, cx| i.set_theme(theme, cx));
        cx.notify();
    }

    /// The tab the main component shows (what "Attach this tab" would attach).
    pub fn set_page(&mut self, page: Option<Page>, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            cx.notify();
        }
    }

    /// The panel opened: the keyboard goes to the question field.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.input.focus_handle(cx), cx);
        cx.emit(ShellRequest::FocusShell);
        cx.notify();
    }

    /// The model the selected companion runs (its own, else the desktop default).
    fn model_choice(&self) -> ModelChoice {
        self.roster.get(&self.selected).and_then(|c| c.model.clone()).unwrap_or_else(|| ModelChoice::default_for(Platform::Desktop))
    }

    fn download_model(&mut self, cx: &mut Context<Self>) {
        self.models.start_download(&self.model_choice());
        cx.notify();
    }

    fn cancel_download(&mut self, cx: &mut Context<Self>) {
        self.models.cancel_download(&self.model_choice());
        cx.notify();
    }

    pub(crate) fn focus_footnote(&mut self, n: u32, cx: &mut Context<Self>) {
        self.focus_note = if self.focus_note == Some(n) { None } else { Some(n) };
        cx.notify();
    }

    fn conversation(&mut self) -> &mut Conversation {
        self.conversations.entry(self.selected.clone()).or_default()
    }

    fn select(&mut self, id: AgentId, cx: &mut Context<Self>) {
        self.selected = id;
        cx.notify();
    }

    fn toggle_attach(&mut self, cx: &mut Context<Self>) {
        let page = self.page.clone().map(|p| AttachedPage { url: p.url, title: p.title });
        let conv = self.conversation();
        let now = if conv.attached().is_some() { None } else { page };
        conv.attach(now);
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().trim().to_string();
        self.input.update(cx, |i, cx| i.set_text("", cx));
        self.send(text, cx);
    }

    /// A question from the address bar (`> q` or `>name q`): select that companion and send it.
    /// The tab is attached only if the user attached it in this conversation.
    pub fn ask(&mut self, name: Option<&str>, question: &str, cx: &mut Context<Self>) {
        let target = match name {
            Some(n) => self.roster.find_by_name(n).map(|c| c.id().clone()),
            None => self.roster.default_companion().map(|c| c.id().clone()),
        };
        match target {
            Some(id) => self.selected = id,
            None => {
                let msg = format!("No companion named {}.", name.unwrap_or("default"));
                self.conversation().fail(&msg);
                cx.notify();
                return;
            }
        }
        self.send(question.trim().to_string(), cx);
    }

    fn send(&mut self, text: String, cx: &mut Context<Self>) {
        let id = self.selected.clone();
        let conv = self.conversation();
        if text.is_empty() || conv.is_running() {
            cx.notify();
            return;
        }
        let input = match conv.attached() {
            Some(p) => format!("Attached page: {} <{}>\n\n{text}", p.title, p.url),
            None => text.clone(),
        };
        conv.start_task(&text);
        self.worker.run(self.roster.clone(), id, input);
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.worker.cancel(&self.selected);
        cx.notify();
    }

    fn answer(&mut self, offer: Option<OfferView>, cx: &mut Context<Self>) {
        let (label, answer) = match offer {
            Some(o) => (o.label.clone(), ApprovalAnswer::Approve { scope: o.scope }),
            None => ("Declined".to_string(), ApprovalAnswer::Deny),
        };
        self.conversation().answer_card(&label);
        self.worker.answer(answer);
        cx.notify();
    }

    /// Pump what the worker sent. Returns whether the panel changed.
    pub fn poll(&mut self, cx: &mut Context<Self>) {
        if let Some(q) = self.autoask.take() {
            self.send(q, cx);
        }
        let msgs = self.worker.poll();
        if matches!(self.models.status(&self.model_choice()), ModelStatus::Downloading { .. }) {
            cx.notify(); // progress
        }
        if msgs.is_empty() {
            return;
        }
        for msg in msgs {
            match msg {
                Msg::Update(id, update) => self.conversations.entry(id).or_default().apply(update),
                Msg::Done(id, Ok(outcome)) => {
                    let conv = self.conversations.entry(id).or_default();
                    // The model has no footnote syntax yet, so no citations arrive; any that do are checked.
                    conv.set_footnotes(footnotes(&outcome.trace, Vec::new()));
                }
                Msg::Done(id, Err(e)) => self.conversations.entry(id).or_default().fail(&e.to_string()),
            }
        }
        // One parse of each streaming tail per tick, however many deltas arrived.
        for conv in self.conversations.values_mut() {
            conv.settle();
        }
        let _ = self.store.save_roster(&self.roster);
        cx.notify();
    }
}

impl Focusable for CompanionView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn dim(t: Theme, text: impl Into<SharedString>) -> gpui::Div {
    div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(text.into())
}

fn bubble(t: Theme, mine: bool) -> gpui::Div {
    div()
        .p(px(8.))
        .rounded(px(8.))
        .text_size(px(13.))
        .bg(rgb(if mine { t.surface_variant } else { t.background }))
        .border_1()
        .border_color(rgb(if mine { t.surface_variant } else { t.border }))
        .text_color(rgb(t.text_primary))
}

/// The first-run download prompt, its progress, or a failure. Nothing shows once the model is ready.
fn model_card(t: Theme, status: ModelStatus, cx: &mut Context<CompanionView>) -> Option<gpui::Div> {
    let card = || {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.accent))
            .text_size(px(12.))
            .text_color(rgb(t.text_primary))
    };
    match status {
        ModelStatus::Ready { .. } | ModelStatus::Unsupported { .. } => None,
        ModelStatus::Missing { label, partial, consent, .. } => {
            Some(
                card()
                    .child(format!("{label} is not on this device yet."))
                    .children(consent.into_iter().map(|l| dim(t, l)))
                    .children((partial > 0).then(|| dim(t, format!("{} already downloaded; it will resume.", weirdnet_model::human(partial)))))
                    .child(div().flex().flex_row().child(
                        chip(t, "companion-model-download", "Download", true).on_click(cx.listener(|this, _, _, cx| this.download_model(cx))),
                    )),
            )
        }
        ModelStatus::Downloading { label, done, total } => {
            let frac = if total == 0 { 0. } else { (done as f32 / total as f32).clamp(0., 1.) };
            Some(
                card()
                    .child(format!("Downloading {label}: {} of {} ({:.0}%)", weirdnet_model::human(done), weirdnet_model::human(total), frac * 100.))
                    .child(
                        div()
                            .h(px(6.))
                            .w_full()
                            .rounded(px(3.))
                            .bg(rgb(t.surface_variant))
                            .child(div().h_full().w(relative(frac)).rounded(px(3.)).bg(rgb(t.accent))),
                    )
                    .child(
                        div().flex().flex_row().child(
                            chip(t, "companion-model-cancel", "Cancel", false).on_click(cx.listener(|this, _, _, cx| this.cancel_download(cx))),
                        ),
                    ),
            )
        }
        ModelStatus::Failed { label, error } => Some(
            card().child(div().text_color(rgb(t.error)).child(format!("{label} could not be downloaded: {error}"))).child(
                div()
                    .flex()
                    .flex_row()
                    .child(chip(t, "companion-model-retry", "Retry", true).on_click(cx.listener(|this, _, _, cx| this.download_model(cx)))),
            ),
        ),
    }
}

fn footnote_row(t: Theme, f: &FootnoteView, focused: bool) -> gpui::Div {
    let steps = f.steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ");
    div()
        .flex()
        .flex_row()
        .gap(px(6.))
        .text_size(px(11.))
        .rounded(px(4.))
        .bg(rgb(if focused { t.selection } else { t.surface }))
        .child(div().text_color(rgb(t.accent)).child(format!("[{}]", f.n)))
        .child(div().flex_1().overflow_hidden().text_ellipsis().text_color(rgb(t.text_secondary)).child(f.label.clone()))
        .child(div().text_color(rgb(t.text_secondary)).child(format!("steps {steps}")))
}

fn trace_row(t: Theme, r: &TraceRowView) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .gap(px(6.))
        .text_size(px(11.))
        .child(div().w(px(18.)).text_color(rgb(t.text_secondary)).child(r.step.to_string()))
        .child(div().flex_1().text_color(rgb(t.text_primary)).child(r.text.clone()))
        .child(div().text_color(rgb(if r.session == "logged out" { t.text_secondary } else { t.accent_alt })).child(r.session.clone()))
}

fn notice_row(t: Theme, n: &NoticeView) -> gpui::Div {
    div()
        .p(px(6.))
        .rounded(px(6.))
        .text_size(px(11.))
        .border_1()
        .border_color(rgb(t.surface_variant))
        .text_color(rgb(t.text_secondary))
        .child(n.text.clone())
}

impl Render for CompanionView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let conv = self.conversations.get(&self.selected).cloned().unwrap_or_default();
        let picker: Vec<_> = self
            .roster
            .companions()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let id = c.id().clone();
                chip(t, ("companion-pick", i), c.name().to_string(), *c.id() == self.selected)
                    .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
            })
            .collect();
        let who = self.roster.get(&self.selected).map(|c| c.name().to_string()).unwrap_or_default();
        let model = self.roster.get(&self.selected).and_then(|c| c.model.as_ref()).map(|m| m.label()).unwrap_or_else(|| "its own model".into());

        let focus_note = self.focus_note;
        let entries: Vec<gpui::AnyElement> = conv
            .entries()
            .iter()
            .enumerate()
            .map(|(i, e)| match e {
                Entry::User { text, attached } => bubble(t, true)
                    .child(text.clone())
                    .children(attached.as_ref().map(|p| dim(t, format!("with page: {}", p.title)).mt(px(4.))))
                    .into_any_element(),
                Entry::Assistant { doc, footnotes, .. } => bubble(t, false)
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(crate::companion_markdown::blocks(t, doc, i, cx))
                    .children((!footnotes.is_empty()).then(|| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .border_t_1()
                            .border_color(rgb(t.surface_variant))
                            .pt(px(4.))
                            .children(footnotes.iter().map(|f| footnote_row(t, f, focus_note == Some(f.n))))
                    }))
                    .into_any_element(),
                Entry::Card { card, answer } => approval_card(t, i, card, answer.as_deref(), cx).into_any_element(),
                Entry::Notice(n) => notice_row(t, n).into_any_element(),
                Entry::Blocked(text) => div().text_size(px(11.)).text_color(rgb(t.error)).child(text.clone()).into_any_element(),
                Entry::Tool { tool, state, .. } => {
                    let (mark, color) = match state {
                        ToolState::Running => ("…", t.text_secondary),
                        ToolState::Done => ("✓", t.accent),
                        ToolState::Failed => ("✗", t.error),
                    };
                    div().text_size(px(11.)).text_color(rgb(color)).child(format!("{mark} {tool}")).into_any_element()
                }
                Entry::Error(m) => div().text_size(px(12.)).text_color(rgb(t.error)).child(m.clone()).into_any_element(),
            })
            .collect();

        let attached = conv.attached().cloned();
        let can_attach = self.page.is_some();
        let running = conv.is_running();
        let rows = conv.trace().to_vec();
        let trace_summary = format!("Trace: {} step{}", rows.len(), if rows.len() == 1 { "" } else { "s" });
        let trace_open = self.trace_open;
        let status = conv.status().map(str::to_string);

        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(10.))
            .bg(rgb(t.surface))
            .child(
                div().flex().flex_row().items_center().child(div().flex_1().text_size(px(13.)).child("Companions")).child(
                    icon(t, "companions-close", "×")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::Run(morph_ui::actions::Action::ToggleCompanions)))),
                ),
            )
            .child(div().flex().flex_row().flex_wrap().gap(px(4.)).children(picker))
            .child(dim(t, format!("{who} · {model} · runs on this device")))
            .children(model_card(t, self.models.status(&self.model_choice()), cx))
            .child(
                div()
                    .id("companion-log")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(conv.entries().is_empty().then(|| {
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(t.text_secondary))
                            .child("Ask a question, or attach this tab to talk about the page you are on.")
                    }))
                    .children(entries),
            )
            .children((!rows.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .id("companion-trace-toggle")
                            .cursor_pointer()
                            .child(heading(
                                t,
                                format!(
                                    "{} {}{}",
                                    if trace_open { "▾" } else { "▸" },
                                    trace_summary,
                                    status.map(|s| format!(" · {s}")).unwrap_or_default()
                                ),
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.trace_open = !this.trace_open;
                                cx.notify();
                            })),
                    )
                    .children(trace_open.then(|| div().flex().flex_col().gap(px(2.)).children(rows.iter().map(|r| trace_row(t, r)))))
            }))
            .child(
                div().flex().flex_row().items_center().gap(px(6.)).child(
                    chip(
                        t,
                        "companion-attach",
                        match &attached {
                            Some(p) => format!("Attached: {}", p.title),
                            None => "Attach this tab".to_string(),
                        },
                        attached.is_some(),
                    )
                    .when(!can_attach && attached.is_none(), |d| d.opacity(0.5))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_attach(cx))),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(div().flex_1().h(px(28.)).px(px(8.)).rounded(px(6.)).bg(rgb(t.background)).child(self.input.clone()))
                    .child(if running {
                        button(t, "companion-stop", "Stop", false).on_click(cx.listener(|this, _, _, cx| this.stop(cx))).into_any_element()
                    } else {
                        button(t, "companion-send", "Send", true).on_click(cx.listener(|this, _, _, cx| this.submit(cx))).into_any_element()
                    }),
            )
    }
}

fn approval_card(t: Theme, i: usize, card: &ApprovalCard, answer: Option<&str>, cx: &mut Context<CompanionView>) -> gpui::Div {
    let base = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(if card.signed_in { t.accent_alt } else { t.accent }))
        .child(div().text_size(px(12.)).text_color(rgb(t.text_primary)).child(card.title.clone()))
        .children(card.signed_in.then(|| div().text_size(px(11.)).text_color(rgb(t.accent_alt)).child("This would use your signed-in session.")));
    match answer {
        Some(a) => base.child(dim(t, a.to_string())),
        None => base
            .children(card.offers.iter().enumerate().map(|(j, o)| {
                let offer = o.clone();
                button(t, ("companion-offer", i * 16 + j), o.label.clone(), j == 0)
                    .on_click(cx.listener(move |this, _, _, cx| this.answer(Some(offer.clone()), cx)))
            }))
            .child(button(t, ("companion-decline", i), "Decline", false).on_click(cx.listener(|this, _, _, cx| this.answer(None, cx)))),
    }
}

/// A made-up conversation for the headless screenshot check (`MORPH_COMPANION_DEMO=1`): every
/// kind of row, built through the same translation real events use.
fn demo_conversation() -> Conversation {
    use morph_companion::translate;
    use weirdnet_agent::{
        ActionRequest, AgentEvent, ApprovalPrompt, Citation, CitationTarget, Scope, SessionMark, SkipNotice, StepId, StepKind, Tier, Trace,
    };
    let mut c = Conversation::default();
    c.attach(Some(AttachedPage { url: "https://tokio.rs/".into(), title: "Tokio".into() }));
    c.start_task("How does Tokio cancel a task?");
    let mut trace = Trace::new("demo", AgentId::new("local"));
    trace.push(StepKind::Search { provider: "DuckDuckGo".into(), query: "tokio cancel task".into(), results: 5 }, SessionMark::LoggedOut, None, 1);
    trace.push(
        StepKind::Fetch { url: "https://tokio.rs/tokio/topics/shutdown".into(), status: Some(200), bytes: 9000 },
        SessionMark::LoggedOut,
        None,
        2,
    );
    let events = vec![
        AgentEvent::ApprovalRequested(ApprovalPrompt {
            agent: AgentId::new("local"),
            request: ActionRequest::new(Tier::Navigate).site("tokio.rs"),
            offers: vec![Scope::Once, Scope::Task, Scope::AlwaysSite { all_companions: false }],
        }),
        AgentEvent::ApprovalSkipped(SkipNotice { allow_id: 1, tier: Tier::Research, site: None, provider: Some("DuckDuckGo".into()) }),
        AgentEvent::ToolStarted { call_id: "1".into(), tool: "search".into() },
        AgentEvent::ToolFinished { call_id: "1".into(), tool: "search".into(), is_error: false },
        AgentEvent::TextDelta("Dropping the task's future cancels it at its next await point [1].".into()),
    ];
    for e in &events {
        for u in translate("Local", e) {
            c.apply(u);
        }
    }
    c.answer_card("Allow once");
    for row in morph_companion::trace_rows(&trace) {
        c.apply(ViewUpdate::Trace(row));
    }
    let cite =
        Citation { n: 1, target: CitationTarget::Url { url: "https://tokio.rs/tokio/topics/shutdown".into(), quote: None }, steps: vec![StepId(2)] };
    c.set_footnotes(footnotes(&trace, vec![cite]));
    c.apply(ViewUpdate::Finished { task_id: "demo".into(), status: "done".into() });
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_conversation_has_every_row_kind() {
        let c = demo_conversation();
        let kinds: Vec<&str> = c
            .entries()
            .iter()
            .map(|e| match e {
                Entry::User { .. } => "user",
                Entry::Assistant { .. } => "assistant",
                Entry::Card { .. } => "card",
                Entry::Notice(_) => "notice",
                Entry::Blocked(_) => "blocked",
                Entry::Tool { .. } => "tool",
                Entry::Error(_) => "error",
            })
            .collect();
        for k in ["user", "assistant", "card", "notice", "tool"] {
            assert!(kinds.contains(&k), "missing {k}: {kinds:?}");
        }
        assert_eq!(c.trace().len(), 2);
        assert!(matches!(c.entries().last(), Some(Entry::Assistant { footnotes, .. }) if footnotes.len() == 1));
        assert!(c.pending_card().is_none(), "the demo card is already answered");
    }
}
