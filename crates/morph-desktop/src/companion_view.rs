//! The Companion panel (`morph.companions`): the right-hand slot that stays across tabs. It
//! draws what `morph-companion` builds (the roster picker, the conversation, approval cards,
//! skipped-approval notices, footnotes and the trace) and hands the user's questions and card
//! answers to the worker in `companion_host`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use gpui::{
    App, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Subscription, WeakEntity, Window, div, prelude::*, px,
    relative, rgb,
};
use morph_ai::{
    ActedOn, AiStore, EngineFactory, FileStore, Grants, HostError, LocalModels, ModelChoice, ModelStatus, PageLink, PageSnapshot, Platform, Roster,
    TabInfo,
};
use morph_browser::{HostPageId, HostPageRequest, HostPageState};
use morph_companion::{
    Activity, ApprovalCard, AttachedPage, Conversation, Entry, FootnoteView, NoticeView, OfferView, TaskOptions, ToolState, TraceRowView, ViewUpdate,
    export_markdown, footnotes, more_pages_prompt, parse_citations, ran_out_of_pages, with_current_tab,
};
use morph_core::tabs::TabId;
use morph_ui::theme::Theme;
use morph_webview::page_read::Pending;
use morph_webview::{Isolation, PageData, ReadRequest, Want};
use weirdnet_agent::{AgentId, ApprovalAnswer, Citation, Outcome};

use crate::browser_view::BrowserView;
use crate::companion_host::{DesktopBrowser, DesktopResearch, HostCall, Hosts, LockCheck, Msg, Reply, Seen, Worker, acted, profile_lock};
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
    /// The policy the worker enforces; Settings changes reach it through [`Self::reload_agents`].
    policy: weirdnet_agent::SharedPolicy,
    /// The policy as last loaded or saved: saves replay only what changed here since then onto
    /// what is on disk (`morph_ai::rebase_shared`), so a sync or Settings change is kept.
    policy_base: weirdnet_agent::Policy,
    /// `companion_sync::applied()` when the panel last took in synced files.
    synced: u64,
    /// The first-run roster is not on disk yet (saved once, never over a stored one).
    roster_unsaved: bool,
    /// The user's other devices, for Node companions.
    devices: crate::companion_devices::CompanionDevices,
    /// `MORPH_COMPANION_AUTOASK=<question>`: a dev flag that sends one question when the panel first polls.
    autoask: Option<String>,
    /// When the panel was made: the dev question waits for the tab's address, at most 10 s.
    made: Instant,
    /// The profile lock: while it is locked no task starts, and locking stops the running ones.
    lock: LockCheck,
    /// When the lock was last looked at, and what it said.
    lock_seen: Option<(Instant, bool)>,
    store: FileStore,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    home: PathBuf,
    profile: String,
    /// The window's browser: the companion tools act on its tabs and read its pages.
    browser: Option<WeakEntity<BrowserView>>,
    /// Calls from the companion tools that only the UI thread can answer.
    host_calls: Receiver<HostCall>,
    seen: Seen,
    /// Page reads, host-page loads and finds under way for those calls.
    ops: Vec<Op>,
    /// The tab each companion's conversation attached (read when the question is sent).
    attached_tab: HashMap<AgentId, u64>,
    /// "Use my signed-in session" for the next task only; off again after each send.
    signed_in_next: bool,
    /// The next task is the "+5 pages" one the user just took.
    more_pages_next: bool,
    /// What the waiting card is about, when it is known before the card arrives.
    card_detail: HashMap<AgentId, String>,
    /// Each companion's last finished task, for Markdown export.
    last: HashMap<AgentId, (String, Box<Outcome>, Vec<Citation>)>,
    /// Where the last export went.
    exported: Option<String>,
    /// The working line's last drawn frame (a quarter second), so it redraws as it ticks.
    activity_frame: Option<u128>,
    /// The panel is folded to a slim strip (kept per profile in `companions/panel.json`, which
    /// does not sync).
    collapsed: bool,
    /// The outline the waiting card put on the page.
    outline: crate::companion_host::CardOutline,
    /// Each running task's wall clock (card waits not counted), stopped at `WALL_LIMIT`.
    guards: HashMap<AgentId, crate::companion_host::WallGuard>,
    /// Tasks the guard stopped, told so when they end.
    timed_out: HashSet<AgentId>,
}

/// Width of the folded panel's strip, in logical pixels.
pub const COLLAPSED_WIDTH: f32 = 40.;
/// The panel's own state on disk, per profile.
const PANEL_FILE: &str = "panel.json";

fn save_collapsed(store: &FileStore, collapsed: bool) -> Result<(), morph_ai::StoreError> {
    store.write_file(PANEL_FILE, serde_json::json!({ "collapsed": collapsed }).to_string().as_bytes())
}

/// Whether the panel was left folded.
fn load_collapsed(store: &FileStore) -> bool {
    store
        .read_file(PANEL_FILE)
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("collapsed").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// What a companion says when asked while the profile is locked.
const LOCKED: &str = "This profile is locked. Unlock it with its password, then ask again.";
/// How often the panel looks at the profile lock.
const LOCK_EVERY: Duration = Duration::from_secs(1);

/// One host call the panel is still answering.
enum Op {
    /// The attached tab is being read; the task starts when it answers.
    Attach {
        pending: Pending<morph_webview::page_read::ReadResult>,
        companion: AgentId,
        input: String,
        options: TaskOptions,
    },
    /// A hidden host page is loading, then being read.
    HostLoad {
        id: HostPageId,
        pending: Pending<HostPageState>,
        reply: Reply<(PageSnapshot, Option<u16>)>,
    },
    HostRead {
        id: HostPageId,
        pending: Pending<morph_webview::page_read::ReadResult>,
        reply: Reply<(PageSnapshot, Option<u16>)>,
    },
    /// A page action on the active tab (`reply` is `None` for a card's highlight and for
    /// clearing it; `outline` is set for the card's highlight).
    Act {
        outline: bool,
        pending: Pending<morph_webview::ActionResult>,
        reply: Option<Reply<ActedOn>>,
    },
    /// Waiting for the page to count find matches.
    Find {
        until: Instant,
        reply: Reply<u32>,
    },
}

/// How a page read becomes text for a companion: the readable part when there is enough of it.
fn snapshot(data: PageData) -> PageSnapshot {
    let readable = data.readable.as_ref().map(|r| r.text.clone()).filter(|t| t.len() > 200);
    let text = readable.or(data.text.clone()).unwrap_or_default();
    PageSnapshot {
        title: data.title.or(data.readable.map(|r| r.title)).unwrap_or_default(),
        text,
        selection: data.selection,
        links: data.links.unwrap_or_default().into_iter().map(|l| PageLink { href: l.href, text: l.text }).collect(),
        truncated: data.truncated,
        // Reads run in Morph's own script world on every desktop engine now; only a read in the
        // page's own world (an engine without one) gets the "page scripts could change this" note.
        isolated: data.isolation != Some(Isolation::PageWorld),
        url: data.url,
    }
}

fn gone<T>() -> Result<T, HostError> {
    Err(HostError::Gone)
}

fn read_error(e: morph_webview::ReadError) -> HostError {
    HostError::Read(e.to_string())
}

/// What a page read asks for: never the raw HTML.
fn page_request() -> ReadRequest {
    ReadRequest::new(Want { title: true, text: true, readable: true, links: true, selection: true, html: false })
}

/// A hidden research page's load and its read each get the design's per-page time.
const HOST_PAGE_TIMEOUT: Duration = Duration::from_secs(15);

fn host_read_request() -> ReadRequest {
    ReadRequest { timeout: HOST_PAGE_TIMEOUT, ..page_request() }
}

impl EventEmitter<ShellRequest> for CompanionView {}

impl CompanionView {
    pub fn new(home: &Path, profile: &str, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = morph_services::profile_io::file_store(&morph_core::profiles::profile_dir(home, profile).join("companions"));
        let (roster, roster_unsaved) = match store.load_roster() {
            Ok(Some(r)) if r.default_companion().is_some() => (r, false),
            _ => (Roster::first_run(ModelChoice::default_for(Platform::Desktop)), true),
        };
        let synced = morph_services::companion_sync::applied();
        let policy =
            Grants::load(Arc::new(morph_services::profile_io::file_store(&morph_core::profiles::profile_dir(home, profile).join("companions"))))
                .map(|g| g.shared())
                .unwrap_or_else(|_| weirdnet_agent::Policy::new().shared());
        let factory: EngineFactory = Arc::new(|path| {
            weirdnet_model::llama::LlamaEngine::load(path, weirdnet_model::llama::LlamaOptions::default())
                .map(|e| Box::new(e) as Box<dyn weirdnet_model::Engine>)
        });
        let models = Arc::new(LocalModels::new(weirdnet_model::ModelStore::from_env(), factory));
        let tools = crate::mcp_host::get(&morph_core::profiles::profile_dir(home, profile).join("companions"));
        let policy_base = policy.read().unwrap_or_else(|e| e.into_inner()).clone();
        let (calls, host_calls) = std::sync::mpsc::channel();
        let seen = Seen::default();
        let lock = profile_lock(home.to_path_buf(), profile.to_string());
        let hosts = Hosts {
            browser: Arc::new(DesktopBrowser::new(calls.clone(), seen.clone()).locked_when(lock.clone())),
            research: Arc::new(
                DesktopResearch::new(
                    calls.clone(),
                    home.to_path_buf(),
                    profile.to_string(),
                    morph_services::profile_io::file_store(&morph_core::profiles::profile_dir(home, profile).join("companions")),
                )
                .locked_when(lock.clone()),
            ),
            calls: Some(calls),
        };
        let worker = Worker::start_with(models.clone(), policy.clone(), tools, Some(hosts));
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
            policy,
            policy_base,
            synced,
            roster_unsaved,
            devices: crate::companion_devices::CompanionDevices::new(home.to_path_buf(), profile.to_string()).with_lock(lock.clone()),
            autoask: std::env::var("MORPH_COMPANION_AUTOASK").ok().filter(|q| !q.trim().is_empty()),
            made: Instant::now(),
            lock,
            lock_seen: None,
            store,
            focus: cx.focus_handle(),
            _subscriptions: vec![sub],
            home: home.to_path_buf(),
            profile: profile.to_string(),
            browser: None,
            host_calls,
            seen,
            ops: Vec::new(),
            attached_tab: HashMap::new(),
            signed_in_next: false,
            more_pages_next: false,
            card_detail: HashMap::new(),
            last: HashMap::new(),
            exported: None,
            activity_frame: None,
            collapsed: false,
            outline: Default::default(),
            guards: HashMap::new(),
            timed_out: HashSet::new(),
        };
        view.collapsed = load_collapsed(&view.store);
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

    /// The window's browser, whose tabs and pages the companion tools use.
    pub fn set_browser(&mut self, browser: WeakEntity<BrowserView>) {
        self.browser = Some(browser);
    }

    /// The tab the main component shows (what "Attach this tab" would attach).
    pub fn set_page(&mut self, page: Option<Page>, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            cx.notify();
        }
    }

    /// The panel opened: the keyboard goes to the question field.
    /// The panel is folded to its strip.
    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    /// Folds the panel to its strip or opens it out again, and remembers that for the profile.
    pub fn set_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        if self.collapsed == collapsed {
            return;
        }
        self.collapsed = collapsed;
        // A locked profile refuses the write; the panel still folds for this session.
        let _ = save_collapsed(&self.store, collapsed);
        // The shell gives the page the width back (or takes it again).
        cx.emit(ShellRequest::CompanionsFolded);
        cx.notify();
    }

    /// The folded strip: open it out, the selected companion's initial, and a pulse while a
    /// task runs, in the card colour with "!" while a card waits, so nothing waits unseen.
    fn strip(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let waiting = self.conversations.values().any(|c| c.pending_card().is_some());
        let running = self.conversations.values().any(|c| c.is_running());
        let pulse = self.activity_frame.map_or(0., |f| (f % 4) as f32);
        let initial = self.roster.get(&self.selected).and_then(|c| c.name().chars().next()).unwrap_or('C').to_string();
        div()
            .id("companions-strip")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .py(px(10.))
            .bg(rgb(t.surface))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.set_collapsed(false, cx)))
            .child(icon(t, "companions-expand", "«").on_click(cx.listener(|this, _, _, cx| this.set_collapsed(false, cx))))
            .child(
                div()
                    .size(px(24.))
                    .rounded(px(12.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(12.))
                    .bg(rgb(t.surface_variant))
                    .text_color(rgb(t.text_primary))
                    .child(initial),
            )
            .children((running || waiting).then(|| {
                div()
                    .id("companions-strip-status")
                    .size(px(10.))
                    .rounded(px(5.))
                    .bg(rgb(if waiting { t.accent_alt } else { t.accent }))
                    .opacity(0.35 + 0.65 * (1. - (pulse - 1.5).abs() / 1.5))
            }))
            .children(waiting.then(|| div().text_size(px(13.)).text_color(rgb(t.accent_alt)).child("!")))
            .into_any_element()
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.input.focus_handle(cx), cx);
        cx.emit(ShellRequest::FocusShell);
        cx.notify();
    }

    /// The model the selected companion runs (its own, else the desktop default).
    fn model_choice(&self) -> ModelChoice {
        self.roster.get(&self.selected).and_then(|c| c.model.clone()).unwrap_or_else(|| ModelChoice::default_for(Platform::Desktop))
    }

    /// The local model the selected companion runs on while it is still loading (its name), for
    /// the working line. `None` for a loaded model and for companions that run elsewhere.
    fn model_loading(&self) -> Option<String> {
        let c = self.roster.get(&self.selected)?;
        let choice = self.model_choice();
        (c.identity.kind == weirdnet_agent::AgentKind::Local && !self.models.is_loaded(&choice)).then(|| choice.label())
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

    /// Settings or sync changed the roster, grants or allows on disk: take them in, keeping
    /// any approval made here that is not saved yet.
    pub fn reload_agents(&mut self, cx: &mut Context<Self>) {
        if let Ok(Some(r)) = self.store.load_roster()
            && r.default_companion().is_some()
        {
            self.roster = r;
            self.roster_unsaved = false;
        }
        if self.roster.get(&self.selected).is_none() {
            self.selected = self.roster.default_companion().map(|c| c.id().clone()).unwrap_or_else(|| AgentId::new("local"));
        }
        self.save_policy();
        cx.notify();
    }

    /// Save the live policy over what is on disk now, not over what the panel loaded.
    fn save_policy(&mut self) {
        if let Ok(base) = morph_ai::rebase_shared(&self.store, &self.policy, &self.policy_base) {
            self.policy_base = base;
        }
    }

    /// Stop on a skipped-approval notice: revoke that Always allow and end the running task.
    fn stop_notice(&mut self, allow_id: u64, cx: &mut Context<Self>) {
        self.policy.write().unwrap_or_else(|e| e.into_inner()).revoke(allow_id);
        self.save_policy();
        self.worker.cancel(&self.selected);
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
        let active = self.seen.tabs.lock().unwrap().iter().find(|t| t.active).map(|t| t.id);
        let id = self.selected.clone();
        let conv = self.conversation();
        let now = if conv.attached().is_some() { None } else { page };
        let attaching = now.is_some();
        conv.attach(now);
        match (attaching, active) {
            (true, Some(tab)) => {
                self.attached_tab.insert(id, tab);
            }
            _ => {
                self.attached_tab.remove(&id);
            }
        }
        cx.notify();
    }

    fn toggle_signed_in(&mut self, cx: &mut Context<Self>) {
        self.signed_in_next = !self.signed_in_next;
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

    /// At most every second: when the profile has just locked, stop every running task (its
    /// host calls already refuse with `HostError::Locked`).
    fn follow_lock(&mut self) {
        if self.lock_seen.is_some_and(|(t, _)| t.elapsed() < LOCK_EVERY) {
            return;
        }
        let locked = (self.lock)();
        let was = self.lock_seen.is_some_and(|(_, l)| l);
        self.lock_seen = Some((Instant::now(), locked));
        if locked && !was {
            let running: Vec<AgentId> = self.conversations.iter().filter(|(_, c)| c.is_running()).map(|(id, _)| id.clone()).collect();
            for id in running {
                self.worker.cancel(&id);
            }
        }
    }

    /// Stops a task that ran past `WALL_LIMIT`, not counting time the user took on its cards.
    fn guard_tasks(&mut self) {
        let now = Instant::now();
        let mut over = Vec::new();
        for (id, guard) in &mut self.guards {
            let waiting = self.conversations.get(id).is_some_and(|c| c.pending_card().is_some() || c.asking_more_pages());
            if guard.over(now, waiting, crate::companion_host::WALL_LIMIT) {
                over.push(id.clone());
            }
        }
        for id in over {
            self.guards.remove(&id);
            self.timed_out.insert(id.clone());
            self.worker.cancel(&id);
        }
    }

    /// A task finished or failed: an unanswered "+5 pages" card is a no, and a task the guard
    /// stopped says why.
    fn task_ended(&mut self, id: &AgentId) {
        self.guards.remove(id);
        let conv = self.conversations.entry(id.clone()).or_default();
        conv.answer_more_pages(false);
        if self.timed_out.remove(id) {
            let mins = crate::companion_host::WALL_LIMIT.as_secs() / 60;
            conv.fail(&format!("Stopped after {mins} minutes of work. Ask again with a narrower question, or try a faster model."));
        }
    }

    /// The running task's "+5 pages" card: read more pages now, or not.
    fn more_pages_now(&mut self, yes: bool, cx: &mut Context<Self>) {
        if self.conversation().answer_more_pages(yes) {
            self.worker.answer_pages(yes);
        }
        cx.notify();
    }

    /// The "+5 pages" card: go on with the extension's pages, in the same conversation.
    fn more_pages(&mut self, cx: &mut Context<Self>) {
        if let Some(n) = self.conversation().take_more_pages() {
            self.more_pages_next = true;
            self.send(more_pages_prompt(n), cx);
            // Only that task: a send that did not start leaves no extension for the next one.
            self.more_pages_next = false;
        }
    }

    fn send(&mut self, text: String, cx: &mut Context<Self>) {
        let id = self.selected.clone();
        let not_here = self.roster.get(&id).and_then(|c| self.devices.not_here(c));
        let locked = (self.lock)();
        let conv = self.conversation();
        if text.is_empty() || conv.is_running() {
            cx.notify();
            return;
        }
        if locked {
            conv.fail(LOCKED);
            cx.notify();
            return;
        }
        // A Node companion runs on its device, never here in its place.
        if let Some(reason) = not_here {
            conv.fail(&reason);
            cx.notify();
            return;
        }
        let attached = conv.attached().cloned();
        let page = self.page.clone();
        let conv = self.conversation();
        let input = match (&attached, &page) {
            (Some(p), _) => format!("I attached the page \"{}\" <{}>; read it with page.text.\n\n{text}", p.title.replace('"', "'"), p.url),
            // The tab the user is on, so page.click, page.fill and page.scroll need no attaching.
            (None, Some(p)) if !p.url.is_empty() && p.url != "about:blank" => with_current_tab(&text, &p.url, &p.title),
            (None, _) => text.clone(),
        };
        conv.start_task(&text);
        self.guards.insert(id.clone(), crate::companion_host::WallGuard::new(Instant::now()));
        // Signed-in research is chosen per task and never kept.
        let options = TaskOptions {
            signed_in: std::mem::take(&mut self.signed_in_next).then(|| self.profile.clone()),
            more_pages: std::mem::take(&mut self.more_pages_next),
        };
        *self.seen.attached.lock().unwrap() = None;
        self.exported = None;
        match attached {
            // The page is read now, because the user attached it to this question, and only then.
            Some(_) => {
                let tab = self.attached_tab.remove(&id);
                let pending = self.with_browser(cx, |b| {
                    let active = b.tabs().iter().find(|t| t.active).map(|t| t.id.0);
                    if tab.is_none() || tab == active {
                        b.read_active_tab(page_request())
                    } else {
                        b.read_tab(TabId(tab.unwrap_or_default()), page_request())
                    }
                });
                match pending {
                    Some(pending) => self.ops.push(Op::Attach { pending, companion: id, input, options }),
                    None => self.conversation().fail("The page could not be read: this window has no browser."),
                }
            }
            None => self.worker.run_with(self.roster.clone(), id, input, options),
        }
        cx.notify();
    }

    /// Runs `f` on the window's browser, if it is still there.
    fn with_browser<R>(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut morph_browser::Browser) -> R) -> Option<R> {
        let browser = self.browser.as_ref()?.upgrade()?;
        Some(browser.update(cx, |view, _| f(view.browser_mut())))
    }

    /// Answers the companion tools' host calls and advances the reads they started.
    fn serve_hosts(&mut self, cx: &mut Context<Self>) {
        // What the tools check before a call: the active address and the tabs.
        if let Some(tabs) = self.with_browser(cx, |b| {
            b.tabs().into_iter().map(|t| TabInfo { id: t.id.0, title: t.label, url: String::new(), active: t.active }).collect::<Vec<_>>()
        }) {
            let urls = self.with_browser(cx, |b| b.tab_urls().0).unwrap_or_default();
            let tabs: Vec<TabInfo> = tabs.into_iter().zip(urls).map(|(t, url)| TabInfo { url, ..t }).collect();
            *self.seen.active.lock().unwrap() = tabs.iter().find(|t| t.active).map(|t| t.url.clone());
            *self.seen.tabs.lock().unwrap() = tabs;
        }
        while let Ok(call) = self.host_calls.try_recv() {
            self.serve(call, cx);
        }
        let ops = std::mem::take(&mut self.ops);
        for op in ops {
            match op {
                Op::Attach { pending, companion, input, options } => match pending.try_take() {
                    None => self.ops.push(Op::Attach { pending, companion, input, options }),
                    Some(Ok(data)) => {
                        *self.seen.attached.lock().unwrap() = Some(snapshot(data));
                        self.worker.run_with(self.roster.clone(), companion, input, options);
                    }
                    Some(Err(e)) => {
                        self.conversations.entry(companion).or_default().fail(&format!("The attached page could not be read: {e}"));
                        cx.notify();
                    }
                },
                Op::HostLoad { id, pending, reply } => match pending.try_take() {
                    None => self.ops.push(Op::HostLoad { id, pending, reply }),
                    Some(HostPageState::Loaded) => match self.with_browser(cx, |b| b.read_host_page(id, host_read_request())) {
                        Some(pending) => self.ops.push(Op::HostRead { id, pending, reply }),
                        None => {
                            let _ = reply.send(Err(HostError::Gone));
                        }
                    },
                    Some(state) => {
                        self.with_browser(cx, |b| b.close_host_page(id));
                        let why = match state {
                            HostPageState::Failed(why) => why,
                            other => format!("{other:?}"),
                        };
                        let _ = reply.send(Err(HostError::Fetch(why)));
                    }
                },
                Op::HostRead { id, pending, reply } => match pending.try_take() {
                    None => self.ops.push(Op::HostRead { id, pending, reply }),
                    Some(result) => {
                        // Close at once: hidden views end when their read does.
                        self.with_browser(cx, |b| b.close_host_page(id));
                        let _ = reply.send(result.map(|d| (snapshot(d), None)).map_err(read_error));
                    }
                },
                Op::Act { pending, reply, outline } => match pending.try_take() {
                    None => self.ops.push(Op::Act { pending, reply, outline }),
                    Some(result) if outline => {
                        let id = result.ok().and_then(|o| o.highlight);
                        if let Some(clear) = self.outline.shown(id) {
                            self.act_quietly(clear, cx);
                        }
                    }
                    Some(result) => {
                        if let Some(reply) = reply {
                            let _ = reply.send(acted(result));
                        }
                    }
                },
                Op::Find { until, reply } => match self.with_browser(cx, |b| b.find().and_then(|f| f.matches)).flatten() {
                    Some(n) => {
                        let _ = reply.send(Ok(n));
                    }
                    None if Instant::now() < until => self.ops.push(Op::Find { until, reply }),
                    None => {
                        let _ = reply.send(Ok(0));
                    }
                },
            }
        }
    }

    fn serve(&mut self, call: HostCall, cx: &mut Context<Self>) {
        match call {
            HostCall::Tabs(reply) => {
                let _ = reply.send(Ok(self.seen.tabs.lock().unwrap().clone()));
            }
            HostCall::Open(url, reply) => {
                let r = self.with_browser(cx, |b| {
                    b.new_tab(&url);
                    b.tabs().iter().find(|t| t.active).map(|t| t.id.0).ok_or(HostError::NoTab)
                });
                let _ = reply.send(r.unwrap_or_else(gone));
            }
            HostCall::Switch(tab, reply) => {
                let r = self.with_browser(cx, |b| {
                    if b.tabs().iter().any(|t| t.id.0 == tab) {
                        b.select(TabId(tab));
                        Ok(())
                    } else {
                        Err(HostError::NoTab)
                    }
                });
                let _ = reply.send(r.unwrap_or_else(gone));
            }
            HostCall::Navigate(url, reply) => {
                let r = self.with_browser(cx, |b| b.load(&url));
                let _ = reply.send(r.ok_or(HostError::Gone));
            }
            HostCall::Act { request, reply } => {
                let outline = reply.is_none() && matches!(request.action, morph_webview::Action::Highlight { .. });
                if outline && let Some(clear) = self.outline.asked(&request) {
                    self.act_quietly(clear, cx);
                }
                match self.with_browser(cx, |b| b.active_tab_action(request)) {
                    Some(pending) => self.ops.push(Op::Act { pending, reply, outline }),
                    None => {
                        if outline {
                            self.outline.shown(None);
                        }
                        if let Some(reply) = reply {
                            let _ = reply.send(gone());
                        }
                    }
                }
            }
            HostCall::Find(query, reply) => {
                if self
                    .with_browser(cx, |b| {
                        b.open_find();
                        b.set_find_query("");
                        b.set_find_query(&query);
                    })
                    .is_some()
                {
                    self.ops.push(Op::Find { until: Instant::now() + Duration::from_secs(3), reply });
                } else {
                    let _ = reply.send(gone());
                }
            }
            HostCall::Bookmark { url, title, reply } => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default();
                let r = morph_core::bookmarks::add_bookmark(&self.home, &self.profile, &url, &title, now)
                    .map(|_| ())
                    .map_err(|e| HostError::Fetch(e.to_string()));
                let _ = reply.send(r);
            }
            HostCall::ReadHidden { url, signed_in, reply } => {
                // Throwaway unless the user opted in for this task; never falls back to the profile.
                let request = if signed_in { HostPageRequest::signed_in(&url) } else { HostPageRequest::new(&url) }.timeout(HOST_PAGE_TIMEOUT);
                let opened = self.with_browser(cx, |b| b.open_host_page_with(request).map(|id| (id, b.host_page_loaded(id))));
                match opened {
                    Some(Ok((id, pending))) => self.ops.push(Op::HostLoad { id, pending, reply }),
                    Some(Err(morph_webview::Error::Unsupported(what))) => {
                        let _ = reply.send(Err(HostError::Unsupported(what.to_string())));
                    }
                    Some(Err(e)) => {
                        let _ = reply.send(Err(HostError::Fetch(e.to_string())));
                    }
                    None => {
                        let _ = reply.send(gone());
                    }
                }
            }
        }
    }

    /// Writes the selected companion's last task as Markdown (answer, footnotes, trace) next to
    /// its chats and puts it on the clipboard.
    fn export(&mut self, cx: &mut Context<Self>) {
        let Some((question, outcome, citations)) = self.last.get(&self.selected) else { return };
        let who = self.roster.get(&self.selected).map(|c| c.name().to_string()).unwrap_or_default();
        let md = export_markdown(&who, question, outcome, citations.clone());
        // Sealed with the profile's file key, like the rest of `companions/`.
        let name = format!("exports/{}.md", outcome.task_id);
        self.exported = Some(match self.store.write_file(&name, md.as_bytes()) {
            Ok(()) => format!("Saved {name} in the companions folder and copied it"),
            Err(morph_ai::StoreError::Locked) => "The profile is locked; unlock it to save".to_string(),
            Err(e) => format!("Could not save: {e}"),
        });
        cx.write_to_clipboard(ClipboardItem::new_string(md));
        cx.notify();
    }

    /// Appends a finished task's trace (steps and sessions, never page bodies) to the tool log.
    fn log_task(&self, companion: &AgentId, outcome: &Outcome) {
        let line = serde_json::json!({ "companion": companion, "task": outcome.task_id, "stop": outcome.stop, "usage": outcome.usage, "trace": outcome.trace });
        let _ = self.store.append_line("tool-log.jsonl", &line.to_string());
    }

    /// A page action nobody waits for (clearing the card's outline).
    fn act_quietly(&mut self, request: morph_webview::ActionRequest, cx: &mut Context<Self>) {
        if let Some(pending) = self.with_browser(cx, |b| b.active_tab_action(request)) {
            self.ops.push(Op::Act { pending, reply: None, outline: false });
        }
    }

    /// The card is answered or the task ended: its outline goes now, not after its 30 s fade.
    fn clear_outline(&mut self, cx: &mut Context<Self>) {
        if let Some(clear) = self.outline.done() {
            self.act_quietly(clear, cx);
        }
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.clear_outline(cx);
        self.worker.cancel(&self.selected);
        cx.notify();
    }

    fn answer(&mut self, offer: Option<OfferView>, cx: &mut Context<Self>) {
        self.card_detail.remove(&self.selected);
        let (label, answer) = match offer {
            Some(o) => (o.label.clone(), ApprovalAnswer::Approve { scope: o.scope }),
            None => ("Declined".to_string(), ApprovalAnswer::Deny),
        };
        self.conversation().answer_card(&label);
        self.clear_outline(cx);
        self.worker.answer(answer);
        cx.notify();
    }

    /// Pump what the worker sent. Returns whether the panel changed.
    pub fn poll(&mut self, cx: &mut Context<Self>) {
        // A sync merge wrote companion files: take them in before anything here is saved.
        let synced = morph_services::companion_sync::applied();
        if synced != self.synced {
            self.synced = synced;
            self.reload_agents(cx);
        }
        self.devices.tick();
        self.follow_lock();
        // Sent once the tab's address is known, as a question typed by the user would be.
        let tab_known = self.page.as_ref().is_some_and(|p| !p.url.is_empty() && p.url != "about:blank");
        if self.autoask.is_some() && (tab_known || self.made.elapsed() > Duration::from_secs(10)) {
            let q = self.autoask.take().unwrap_or_default();
            self.send(q, cx);
        }
        self.serve_hosts(cx);
        let msgs = self.worker.poll();
        if matches!(self.models.status(&self.model_choice()), ModelStatus::Downloading { .. }) {
            cx.notify(); // progress
        }
        // The working line's pulse and seconds move while a task runs.
        let frame = match self.collapsed {
            // The strip pulses for any companion's task.
            true => self.conversations.values().filter_map(|c| c.activity()).map(|(_, since)| since.elapsed().as_millis() / 250).max(),
            false => self.conversations.get(&self.selected).and_then(|c| c.activity()).map(|(_, since)| since.elapsed().as_millis() / 250),
        };
        if frame != self.activity_frame {
            self.activity_frame = frame;
            cx.notify();
        }
        self.guard_tasks();
        if msgs.is_empty() {
            return;
        }
        for msg in msgs {
            match msg {
                Msg::Update(id, update) => {
                    let is_card = matches!(update, ViewUpdate::Approval(_));
                    let conv = self.conversations.entry(id.clone()).or_default();
                    conv.apply(update);
                    if is_card && let Some(d) = self.card_detail.get(&id) {
                        conv.set_card_detail(d);
                    }
                }
                Msg::Doing(id, text) => self.conversations.entry(id).or_default().set_doing(&text),
                Msg::MorePages(id, extension) => self.conversations.entry(id).or_default().ask_more_pages(extension),
                Msg::CardDetail(id, detail) => {
                    let conv = self.conversations.entry(id.clone()).or_default();
                    if conv.pending_card().is_some_and(|c| c.detail.is_none()) {
                        conv.set_card_detail(&detail);
                    }
                    self.card_detail.insert(id, detail);
                }
                Msg::Done(id, Ok(outcome)) => {
                    self.clear_outline(cx);
                    self.task_ended(&id);
                    // Footnotes the trace backs stay; unbacked ones go with their markers.
                    let f = parse_citations(&outcome.text, &outcome.trace);
                    let notes = footnotes(&outcome.trace, f.citations.clone());
                    let conv = self.conversations.entry(id.clone()).or_default();
                    conv.finish_answer(&f.text, notes);
                    // The task wanted more pages than its budget and did not ask while it ran
                    // (tasks on another node): the "+5 pages" card for a follow-on task.
                    if ran_out_of_pages(&outcome) && !conv.asked_more_pages() {
                        conv.offer_more_pages(crate::companion_host::research_budget().page_extension);
                    }
                    let question = conv
                        .entries()
                        .iter()
                        .rev()
                        .find_map(|e| if let Entry::User { text, .. } = e { Some(text.clone()) } else { None })
                        .unwrap_or_default();
                    self.log_task(&id, &outcome);
                    self.card_detail.remove(&id);
                    *self.seen.attached.lock().unwrap() = None;
                    let mut shown = outcome;
                    shown.text = f.text;
                    self.last.insert(id, (question, shown, f.citations));
                }
                Msg::Done(id, Err(e)) => {
                    self.clear_outline(cx);
                    self.task_ended(&id);
                    self.conversations.entry(id).or_default().fail(&e.to_string());
                }
            }
        }
        // One parse of each streaming tail per tick, however many deltas arrived.
        for conv in self.conversations.values_mut() {
            conv.settle();
        }
        if self.roster_unsaved && self.store.save_roster(&self.roster).is_ok() {
            self.roster_unsaved = false;
        }
        self.save_policy();
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
        ModelStatus::CpuUnsupported { label, reason } => Some(
            card()
                .child(div().text_color(rgb(t.error)).child(format!("{label} cannot run on this device. {reason}.")))
                .child(dim(t, morph_ai::CpuLimit::INSTEAD.to_string()))
                .child(dim(t, "Settings > Agents has the details.".to_string())),
        ),
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

/// The working line under the transcript while a task runs: a pulsing dot, what the task is
/// doing ("Thinking…", "Reading tokio.rs…", "Waiting for your approval") and for how long.
fn activity_row(t: Theme, activity: &Activity, since: Instant, loading: Option<&str>) -> gpui::Div {
    let elapsed = since.elapsed();
    let pulse = (elapsed.as_millis() / 250 % 4) as f32;
    let waiting = *activity == Activity::WaitingForApproval;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(rgb(if waiting { t.accent_alt } else { t.text_secondary }))
        .child(
            div()
                .size(px(8.))
                .rounded(px(4.))
                .bg(rgb(if waiting { t.accent_alt } else { t.accent }))
                .opacity(0.35 + 0.65 * (1. - (pulse - 1.5).abs() / 1.5)),
        )
        .child(activity.label(loading))
        .child(div().text_color(rgb(t.text_secondary)).child(format!("{} s", elapsed.as_secs())))
}

fn notice_row(t: Theme, i: usize, n: &NoticeView, running: bool, cx: &mut Context<CompanionView>) -> gpui::Div {
    let allow_id = n.allow_id;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .p(px(6.))
        .rounded(px(6.))
        .text_size(px(11.))
        .border_1()
        .border_color(rgb(t.surface_variant))
        .text_color(rgb(t.text_secondary))
        .child(div().flex_1().child(n.text.clone()))
        .children(running.then(|| {
            chip(t, ("companion-notice-stop", i), "Stop", false).on_click(cx.listener(move |this, _, _, cx| this.stop_notice(allow_id, cx)))
        }))
}

impl Render for CompanionView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.collapsed {
            return div().track_focus(&self.focus).size_full().child(self.strip(cx));
        }
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
        let running = conv.is_running();
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
                Entry::Notice(n) => notice_row(t, i, n, running, cx).into_any_element(),
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
                Entry::MorePages { extension, taken, asking, declined } => {
                    more_pages_card(t, i, *extension, *taken, *asking, *declined, running, cx).into_any_element()
                }
            })
            .collect();

        let activity = conv.activity();
        let loading = self.model_loading();
        let attached = conv.attached().cloned();
        let can_attach = self.page.is_some();
        let running = conv.is_running();
        let rows = conv.trace().to_vec();
        let trace_summary = format!("Trace: {} step{}", rows.len(), if rows.len() == 1 { "" } else { "s" });
        let trace_open = self.trace_open;
        let status = conv.status().map(str::to_string);
        let can_export = !running && self.last.contains_key(&self.selected);
        let exported = self.exported.clone();
        let signed_in = self.signed_in_next;

        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(10.))
            .bg(rgb(t.surface))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .child(div().flex_1().text_size(px(13.)).child("Companions"))
                    .child(icon(t, "companions-collapse", "»").on_click(cx.listener(|this, _, _, cx| this.set_collapsed(true, cx))))
                    .child(icon(t, "companions-close", "×").on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::CloseCompanions)))),
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
                    .children(entries)
                    .children(activity.map(|(a, since)| activity_row(t, &a, since, loading.as_deref()))),
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
                    .children(trace_open.then(|| dim(t, "Every source in an answer links to a step here. A citation no step backs is never shown.")))
            }))
            .children(can_export.then(|| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(chip(t, "companion-export", "Export as Markdown", false).on_click(cx.listener(|this, _, _, cx| this.export(cx))))
                    .children(exported.map(|e| dim(t, e)))
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(
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
                    )
                    .child(
                        chip(t, "companion-signed-in", if signed_in { "Research: signed in, this task" } else { "Research: logged out" }, signed_in)
                            .when(signed_in, |d| d.text_color(rgb(t.accent_alt)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_signed_in(cx))),
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
        .children(card.detail.clone().map(|d| dim(t, d)))
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

/// "+5 pages": the research wanted more pages than its budget.
#[allow(clippy::too_many_arguments)]
fn more_pages_card(
    t: Theme,
    i: usize,
    extension: u32,
    taken: bool,
    asking: bool,
    declined: bool,
    running: bool,
    cx: &mut Context<CompanionView>,
) -> gpui::Div {
    let base =
        div().flex().flex_col().gap(px(6.)).p(px(8.)).rounded(px(8.)).border_1().border_color(rgb(t.accent)).child(
            div().text_size(px(12.)).text_color(rgb(t.text_primary)).child("Research used its page budget before reading everything it wanted."),
        );
    if taken {
        base.child(dim(t, format!("Reading up to {extension} more pages")))
    } else if declined {
        base.child(dim(t, "No more pages"))
    } else if asking {
        // The task waits on this answer.
        base.child(
            div()
                .flex()
                .gap(px(6.))
                .child(
                    button(t, ("companion-more-pages-now", i), format!("+{extension} pages"), true)
                        .on_click(cx.listener(|this, _, _, cx| this.more_pages_now(true, cx))),
                )
                .child(button(t, ("companion-no-more-pages", i), "No", false).on_click(cx.listener(|this, _, _, cx| this.more_pages_now(false, cx)))),
        )
    } else if running {
        base
    } else {
        base.child(
            button(t, ("companion-more-pages", i), format!("+{extension} pages"), true).on_click(cx.listener(|this, _, _, cx| this.more_pages(cx))),
        )
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
    fn the_folded_panel_is_remembered_per_profile_and_starts_open() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (FileStore::new(dir.path().join("a/companions")), FileStore::new(dir.path().join("b/companions")));
        assert!(!load_collapsed(&a), "open by default");
        save_collapsed(&a, true).unwrap();
        assert!(load_collapsed(&a));
        assert!(!load_collapsed(&b), "another profile keeps its own");
        save_collapsed(&a, false).unwrap();
        assert!(!load_collapsed(&a));
        a.write_file(PANEL_FILE, b"not json").unwrap();
        assert!(!load_collapsed(&a), "a damaged file opens the panel");
    }

    #[test]
    fn only_a_page_world_read_is_marked_as_one_page_scripts_could_change() {
        let data = |isolation| PageData {
            view: morph_webview::ViewId(1),
            url: "https://a.example/".into(),
            origin: "https://a.example".into(),
            isolation,
            title: Some("A".into()),
            text: Some("text".into()),
            readable: None,
            links: None,
            selection: None,
            html: None,
            truncated: false,
            bytes: 4,
        };
        assert!(snapshot(data(Some(Isolation::ScriptWorld))).isolated);
        assert!(snapshot(data(None)).isolated);
        assert!(!snapshot(data(Some(Isolation::PageWorld))).isolated);
    }

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
                Entry::MorePages { .. } => "more pages",
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
