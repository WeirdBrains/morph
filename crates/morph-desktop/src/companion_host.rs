//! What the Companion panel runs tasks on: a worker thread with its own small runtime, the
//! approval hand-off between the agent and the card the user answers, the plain messages the
//! panel polls, and the app's side of the companion tools: [`DesktopBrowser`] (`BrowserHost`)
//! and [`DesktopResearch`] (`ResearchHost`).
//!
//! The browser lives on the UI thread and its page reads (docs/bridge-api.md) resolve there, so
//! the hosts send [`HostCall`]s that the panel answers from its poll; logged-out research goes
//! over plain HTTP with no cookie store (`companion_web`) on a blocking thread. No GPUI here, so
//! it tests without a window.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use morph_ai::{
    ActedOn, AiStore, BrowserHost, FetchedPage, FileStore, HostError, LIBRARY, ModelProvider, PageElement, PageSnapshot, PageTarget, ResearchHost,
    Roster, ScrollTo, SearchHit, SearchProvider, TabInfo,
};
use morph_companion::{
    Canceller, Exclusions, PendingTarget, SessionError, Sessions, TaskOptions, ViewUpdate, browser_tools_with, doing_text, web_tools_with,
};
use morph_webview::{Action, ActionError, ActionRequest, ActionResult, Locator};
use weirdnet_agent::{
    AgentId, ApprovalAnswer, ApprovalPrompt, Approver, CallTarget, Outcome, SharedPolicy, SourceKind, Tier, ToolContext, ToolError, ToolOutput,
    ToolRegistry, ToolSource, ToolSpec,
};

use crate::companion_web;
use crate::mcp_host::ToolsSource;

/// What the worker tells the panel.
#[derive(Debug)]
pub enum Msg {
    Update(AgentId, ViewUpdate),
    Done(AgentId, Result<Box<Outcome>, SessionError>),
    /// What the card that is waiting (or about to arrive) is about.
    CardDetail(AgentId, String),
    /// The step line of a tool call that is starting ("Reading tokio.rs…").
    Doing(AgentId, String),
    /// The running task used its page budget and waits for "+N pages" or no ([`Worker::answer_pages`]).
    MorePages(AgentId, u32),
}

enum Cmd {
    Run { roster: Roster, companion: AgentId, input: String, options: TaskOptions },
}

/// How long the panel has to answer a host call before the tool gives up.
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// Logged-out fetches and searches.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// The research budget: the design's limits, including 90 s of research per task. Since
/// weirdnet-agent 0.3 `per_task` counts only tool time (not model turns or card waits), so a
/// local model on a CPU no longer ends tasks before they research; [`WallGuard`] bounds the
/// task's own wall time instead.
pub fn research_budget() -> weirdnet_agent::Budget {
    weirdnet_agent::Budget::default()
}

/// How long a task may run, not counting time a card waits for the user. weirdnet-agent's
/// `per_task` no longer bounds wall time, and a local model on a busy CPU can take minutes a turn.
pub const WALL_LIMIT: Duration = Duration::from_secs(15 * 60);

/// A running task's wall clock without the time it waited on the user (cards, "+5 pages").
#[derive(Debug, Clone, Copy)]
pub struct WallGuard {
    started: Instant,
    waited: Duration,
    waiting_since: Option<Instant>,
}

impl WallGuard {
    pub fn new(now: Instant) -> Self {
        Self { started: now, waited: Duration::ZERO, waiting_since: None }
    }

    /// Called as the panel polls: whether the task has now run longer than `limit`.
    pub fn over(&mut self, now: Instant, waiting: bool, limit: Duration) -> bool {
        match (waiting, self.waiting_since) {
            (true, None) => self.waiting_since = Some(now),
            (false, Some(since)) => {
                self.waited += now.saturating_duration_since(since);
                self.waiting_since = None;
            }
            _ => {}
        }
        let waiting_now = self.waiting_since.map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        now.saturating_duration_since(self.started).saturating_sub(self.waited + waiting_now) > limit
    }
}

pub type Reply<T> = tokio::sync::oneshot::Sender<Result<T, HostError>>;

/// A request from a companion tool that only the UI thread can answer.
pub enum HostCall {
    Tabs(Reply<Vec<TabInfo>>),
    Open(String, Reply<u64>),
    Switch(u64, Reply<()>),
    Navigate(String, Reply<()>),
    Find(String, Reply<u32>),
    /// A page action on the active tab (tier 2, scrolling, a card's highlight), always with the
    /// approved origin set. No reply for a highlight.
    Act {
        request: ActionRequest,
        reply: Option<Reply<ActedOn>>,
    },
    Bookmark {
        url: String,
        title: String,
        reply: Reply<()>,
    },
    /// Read `url` in a hidden host page: a throwaway session, or with `signed_in` this window's
    /// profile (the per-task opt-in). `Unsupported` when the engine has no throwaway storage.
    ReadHidden {
        url: String,
        signed_in: bool,
        reply: Reply<(PageSnapshot, Option<u16>)>,
    },
}

impl std::fmt::Debug for HostCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            HostCall::Tabs(_) => "Tabs",
            HostCall::Open(..) => "Open",
            HostCall::Switch(..) => "Switch",
            HostCall::Navigate(..) => "Navigate",
            HostCall::Find(..) => "Find",
            HostCall::Act { reply: None, .. } => "Act (highlight)",
            HostCall::Act { .. } => "Act",
            HostCall::Bookmark { .. } => "Bookmark",
            HostCall::ReadHidden { .. } => "ReadHidden",
        };
        f.write_str(name)
    }
}

/// What the panel last saw of the browser, shared with the hosts: the active address and tabs
/// (for per-site checks before a call) and the page attached to the running task.
#[derive(Clone, Default)]
pub struct Seen {
    pub active: Arc<Mutex<Option<String>>>,
    pub tabs: Arc<Mutex<Vec<TabInfo>>>,
    pub attached: Arc<Mutex<Option<PageSnapshot>>>,
}

async fn ask<T>(calls: &Mutex<Sender<HostCall>>, make: impl FnOnce(Reply<T>) -> HostCall) -> Result<T, HostError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    calls.lock().unwrap().send(make(tx)).map_err(|_| HostError::Gone)?;
    match tokio::time::timeout(CALL_TIMEOUT, rx).await {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => Err(HostError::Gone),
        Err(_) => Err(HostError::Read("the browser did not answer in time".into())),
    }
}

/// How long a card's outline stays on the page: the bridge's most (30 s). The panel clears it
/// as soon as the card is done; a card that waits longer than that shows no outline.
const HIGHLIGHT: Duration = morph_webview::page_action::HIGHLIGHT_MAX;
/// One `page.scroll` up or down, CSS pixels.
const SCROLL_STEP: f64 = 600.0;
/// Far enough for the top or the bottom of any page (the page clamps it).
const SCROLL_END: f64 = 1.0e7;

/// The bridge's locator for a target: by accessible name (labels, button and link text,
/// placeholders), narrowed by selector and role when given.
pub fn locator(t: &PageTarget) -> Locator {
    Locator { selector: t.selector.clone(), role: t.role.clone(), name: t.label.clone(), nth: t.nth, ..Locator::default() }
}

/// `action`, refused by the bridge unless the tab is still on `approved_url`'s origin.
pub fn on_approved(action: Action, approved_url: &str) -> ActionRequest {
    ActionRequest::new(action).on_origin(morph_webview::bridge::origin_of(approved_url))
}

/// A page action's answer for the tools: what it touched, or why nothing was done.
pub fn acted(r: ActionResult) -> Result<ActedOn, HostError> {
    match r {
        Ok(o) => Ok(ActedOn {
            url: o.url,
            matches: o.matches,
            element: o.element.map(|e| PageElement {
                tag: e.tag,
                role: e.role,
                name: e.name,
                text: e.text,
                value: if e.secret { None } else { e.value },
                secret: e.secret,
                editable: e.editable,
                disabled: e.disabled,
                href: e.href,
            }),
            moved: o.scroll.moved,
        }),
        Err(ActionError::Unsupported) => Err(HostError::Unsupported("acting on pages in this browser".into())),
        Err(ActionError::Gone) => Err(HostError::NoTab),
        Err(ActionError::Timeout) => Err(HostError::Timeout),
        Err(ActionError::NotFound) => Err(HostError::NotFound),
        Err(ActionError::NotInteractable(why)) => Err(HostError::NotInteractable(why)),
        Err(ActionError::BlockedByIsolation(i)) => Err(HostError::Blocked(format!("{i:?}"))),
        Err(ActionError::WrongOrigin { expected, actual }) => Err(HostError::WrongOrigin { expected, actual }),
        Err(ActionError::Navigated { from, to }) => Err(HostError::Navigated { from, to }),
        Err(e @ (ActionError::BadRequest(_) | ActionError::Script(_))) => Err(HostError::Read(e.to_string())),
    }
}

/// The outline a tier 2 card puts on its target while it waits.
pub fn highlight_request(target: &PageTarget, approved_url: &str) -> ActionRequest {
    on_approved(Action::Highlight { target: locator(target), duration: HIGHLIGHT }, approved_url)
}

/// The outline a waiting card put on its target ([`highlight_request`]). The panel clears that
/// one outline (`ClearHighlightOne`) once the card is answered or the task ends, rather than
/// leaving it to fade. The clear goes to the origin the outline was set on, so it never acts on
/// another site; an outline that is already gone makes it a no-op.
#[derive(Debug, Default, PartialEq)]
pub struct CardOutline(Outline);

#[derive(Debug, Default, PartialEq)]
enum Outline {
    #[default]
    None,
    /// The highlight was sent; the page has not answered yet.
    Asked {
        origin: Option<String>,
        done: bool,
    },
    Shown {
        id: morph_webview::HighlightId,
        origin: Option<String>,
    },
}

fn clear_one(id: morph_webview::HighlightId, origin: Option<String>) -> ActionRequest {
    ActionRequest { expect_origin: origin, ..ActionRequest::new(Action::ClearHighlightOne(id)) }
}

impl CardOutline {
    /// `request` went to the page: when it is a highlight, it is the card's outline now (and an
    /// earlier one still up is cleared).
    pub fn asked(&mut self, request: &ActionRequest) -> Option<ActionRequest> {
        if !matches!(request.action, Action::Highlight { .. }) {
            return None;
        }
        let earlier = self.done();
        self.0 = Outline::Asked { origin: request.expect_origin.clone(), done: false };
        earlier
    }

    /// The page answered the highlight with `id` (`None`: it failed). The clear to send when the
    /// card was already answered.
    pub fn shown(&mut self, id: Option<morph_webview::HighlightId>) -> Option<ActionRequest> {
        let Outline::Asked { origin, done } = std::mem::take(&mut self.0) else { return None };
        let id = id?;
        if done {
            return Some(clear_one(id, origin));
        }
        self.0 = Outline::Shown { id, origin };
        None
    }

    /// The card was answered (allowed or declined) or the task ended: the clear to send now, if
    /// the outline is up; one still on its way is cleared when it arrives.
    pub fn done(&mut self) -> Option<ActionRequest> {
        match std::mem::take(&mut self.0) {
            Outline::Shown { id, origin } => Some(clear_one(id, origin)),
            Outline::Asked { origin, .. } => {
                self.0 = Outline::Asked { origin, done: true };
                None
            }
            Outline::None => None,
        }
    }
}

/// Whether the profile is locked now (Lead Dev's profile lock): the hosts then refuse every
/// call with [`HostError::Locked`], as `DeviceCaller` and the MCP host fail closed.
pub type LockCheck = Arc<dyn Fn() -> bool + Send + Sync>;

fn never_locked() -> LockCheck {
    Arc::new(|| false)
}

/// The lock check for a window's profile.
pub fn profile_lock(home: PathBuf, profile: String) -> LockCheck {
    Arc::new(move || morph_services::profile_lock::is_locked(&home, &profile))
}

fn gate(locked: &LockCheck) -> Result<(), HostError> {
    if locked() { Err(HostError::Locked) } else { Ok(()) }
}

/// The browser host: tab actions go to the panel; the attached page was read when the user sent.
pub struct DesktopBrowser {
    calls: Mutex<Sender<HostCall>>,
    seen: Seen,
    locked: LockCheck,
}

impl DesktopBrowser {
    pub fn new(calls: Sender<HostCall>, seen: Seen) -> Self {
        Self { calls: Mutex::new(calls), seen, locked: never_locked() }
    }

    /// Refuse everything while `locked` says the profile is locked.
    pub fn locked_when(self, locked: LockCheck) -> Self {
        Self { locked, ..self }
    }
}

#[async_trait]
impl BrowserHost for DesktopBrowser {
    async fn attached_page(&self) -> Result<PageSnapshot, HostError> {
        gate(&self.locked)?;
        self.seen.attached.lock().unwrap().clone().ok_or(HostError::NothingAttached)
    }
    async fn tabs(&self) -> Result<Vec<TabInfo>, HostError> {
        gate(&self.locked)?;
        ask(&self.calls, HostCall::Tabs).await
    }
    fn active_url(&self) -> Option<String> {
        self.seen.active.lock().unwrap().clone()
    }
    fn cached_tabs(&self) -> Vec<TabInfo> {
        self.seen.tabs.lock().unwrap().clone()
    }
    async fn open_url(&self, url: &str) -> Result<u64, HostError> {
        gate(&self.locked)?;
        ask(&self.calls, |r| HostCall::Open(url.to_string(), r)).await
    }
    async fn switch_tab(&self, tab: u64) -> Result<(), HostError> {
        gate(&self.locked)?;
        ask(&self.calls, |r| HostCall::Switch(tab, r)).await
    }
    async fn navigate(&self, url: &str) -> Result<(), HostError> {
        gate(&self.locked)?;
        ask(&self.calls, |r| HostCall::Navigate(url.to_string(), r)).await
    }
    async fn scroll(&self, to: &ScrollTo, approved_url: &str) -> Result<ActedOn, HostError> {
        gate(&self.locked)?;
        let by = |dy: f64| Action::ScrollBy { dx: 0.0, dy };
        let action = match to {
            ScrollTo::Up => by(-SCROLL_STEP),
            ScrollTo::Down => by(SCROLL_STEP),
            ScrollTo::Top => by(-SCROLL_END),
            ScrollTo::Bottom => by(SCROLL_END),
            ScrollTo::Element(t) => Action::ScrollTo(locator(t)),
        };
        ask(&self.calls, |r| HostCall::Act { request: on_approved(action, approved_url), reply: Some(r) }).await
    }
    async fn find_in_page(&self, query: &str) -> Result<u32, HostError> {
        gate(&self.locked)?;
        ask(&self.calls, |r| HostCall::Find(query.to_string(), r)).await
    }
    async fn bookmark(&self, url: &str, title: &str) -> Result<(), HostError> {
        gate(&self.locked)?;
        ask(&self.calls, |r| HostCall::Bookmark { url: url.to_string(), title: title.to_string(), reply: r }).await
    }
    async fn highlight(&self, target: &PageTarget, approved_url: &str) -> Result<(), HostError> {
        gate(&self.locked)?;
        let request = highlight_request(target, approved_url);
        self.calls.lock().unwrap().send(HostCall::Act { request, reply: None }).map_err(|_| HostError::Gone)
    }
    async fn click(&self, target: &PageTarget, approved_url: &str) -> Result<ActedOn, HostError> {
        gate(&self.locked)?;
        let request = on_approved(Action::Click(locator(target)), approved_url);
        ask(&self.calls, |r| HostCall::Act { request, reply: Some(r) }).await
    }
    async fn fill(&self, target: &PageTarget, value: &str, approved_url: &str) -> Result<ActedOn, HostError> {
        gate(&self.locked)?;
        let request = on_approved(Action::Fill { target: locator(target), value: value.to_string() }, approved_url);
        ask(&self.calls, |r| HostCall::Act { request, reply: Some(r) }).await
    }
}

/// The research host: the Library from the profile's files, the web provider and logged-out
/// pages over HTTP with no cookies, and signed-in pages in hidden host pages (opt-in per task).
pub struct DesktopResearch {
    calls: Mutex<Sender<HostCall>>,
    home: PathBuf,
    profile: String,
    store: FileStore,
    locked: LockCheck,
}

impl DesktopResearch {
    pub fn new(calls: Sender<HostCall>, home: PathBuf, profile: String, store: FileStore) -> Self {
        Self { calls: Mutex::new(calls), home, profile, store, locked: never_locked() }
    }

    /// The most requests a second to any one site (Agents > Site rate), read when a request is made.
    fn site_rate(&self) -> f64 {
        self.store.load_settings().ok().flatten().unwrap_or_default().site_rate()
    }

    /// Refuse everything (the Library, the web, hidden pages) while the profile is locked.
    pub fn locked_when(self, locked: LockCheck) -> Self {
        Self { locked, ..self }
    }

    fn check_profile(&self, signed_in: &str) -> Result<(), HostError> {
        // Hidden host pages share this window's profile, so only its session can be lent.
        if signed_in == self.profile {
            Ok(())
        } else {
            Err(HostError::Unsupported(format!("signed-in research with profile {signed_in} from this window")))
        }
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, HostError> + Send + 'static) -> Result<T, HostError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("morph-research".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .map_err(|e| HostError::Fetch(e.to_string()))?;
    rx.await.unwrap_or(Err(HostError::Gone))
}

/// Bookmarks then history entries matching `query`, as search hits.
pub fn library_hits(home: &std::path::Path, profile: &str, query: &str, max: usize) -> Vec<SearchHit> {
    let words = library_words(query);
    if words.is_empty() {
        return Vec::new();
    }
    // A model writes questions ("how to shut down a tokio app"), so a hit needs most of the
    // query's words rather than all of them, and better hits come first.
    let need = words.len().div_ceil(2).max(1);
    let score = |title: &str, url: &str| {
        let hay = format!("{} {}", title.to_lowercase(), url.to_lowercase());
        let n = words.iter().filter(|w| hay.contains(w.as_str())).count();
        (n >= need).then_some(n)
    };
    let mut hits: Vec<(usize, SearchHit)> = Vec::new();
    if let Ok(marks) = morph_core::bookmarks::bookmarks(home, profile) {
        for b in &marks {
            if let Some(n) = score(&b.title, &b.url) {
                hits.push((n, SearchHit { title: b.title.clone(), url: b.url.clone(), snippet: "Bookmark".into() }));
            }
        }
    }
    if let Ok(visits) = morph_core::library::history(home, profile, "", 2000) {
        for v in &visits {
            if let Some(n) = score(&v.title, &v.url)
                && !hits.iter().any(|(_, h)| h.url == v.url)
            {
                hits.push((n, SearchHit { title: v.title.clone(), url: v.url.clone(), snippet: "Visited".into() }));
            }
        }
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.0)); // stable: bookmarks before visits on a tie
    hits.into_iter().take(max).map(|(_, h)| h).collect()
}

/// The words of a query worth matching: lowercased, without short and common words.
fn library_words(query: &str) -> Vec<String> {
    const COMMON: &[&str] = &[
        "the",
        "and",
        "for",
        "how",
        "what",
        "why",
        "when",
        "where",
        "which",
        "who",
        "with",
        "from",
        "into",
        "about",
        "does",
        "can",
        "you",
        "your",
        "are",
        "this",
        "that",
        "way",
        "best",
        "recommended",
        "use",
        "using",
    ];
    let mut words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.')
        .map(|w| w.trim_matches('.').to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !COMMON.contains(&w.as_str()))
        .collect();
    words.dedup();
    words
}

#[async_trait]
impl ResearchHost for DesktopResearch {
    fn providers(&self) -> Vec<String> {
        SearchProvider::ALL.iter().map(|p| p.id().to_string()).collect()
    }
    fn provider(&self) -> String {
        self.store.load_settings().ok().flatten().unwrap_or_default().search_provider.id().to_string()
    }
    async fn search(&self, provider: &str, query: &str) -> Result<Vec<SearchHit>, HostError> {
        gate(&self.locked)?;
        let (provider, query) = (provider.to_string(), query.to_string());
        if provider == LIBRARY {
            let (home, profile) = (self.home.clone(), self.profile.clone());
            return blocking(move || Ok(library_hits(&home, &profile, &query, 8))).await;
        }
        let rate = self.site_rate();
        blocking(move || {
            companion_web::polite_wait(&companion_web::provider_host(&provider), rate);
            companion_web::search(&provider, &query, HTTP_TIMEOUT)
        })
        .await
    }
    async fn fetch(&self, url: &str, signed_in: Option<&str>) -> Result<FetchedPage, HostError> {
        gate(&self.locked)?;
        let (page, status) = self.hidden_or_http(url, signed_in, companion_web::fetch_page).await?;
        Ok(FetchedPage { bytes: page.text.len() as u64, url: page.url, status, text: page.text })
    }
    async fn read(&self, url: &str, signed_in: Option<&str>) -> Result<(PageSnapshot, Option<u16>), HostError> {
        gate(&self.locked)?;
        self.hidden_or_http(url, signed_in, companion_web::read).await
    }
}

/// A page read as text, with its HTTP status when it came over HTTP.
type ReadPage = (PageSnapshot, Option<u16>);

impl DesktopResearch {
    /// A hidden host page: throwaway when logged out, this profile's session with the opt-in.
    /// Logged out on an engine without throwaway storage, plain HTTP with no cookies instead.
    async fn hidden_or_http(
        &self,
        url: &str,
        signed_in: Option<&str>,
        http: fn(&str, Duration) -> Result<ReadPage, HostError>,
    ) -> Result<ReadPage, HostError> {
        if let Some(p) = signed_in {
            self.check_profile(p)?;
        }
        let (rate, wait_url) = (self.site_rate(), url.to_string());
        blocking(move || {
            companion_web::polite_wait(&wait_url, rate);
            Ok(())
        })
        .await?;
        let hidden = ask(&self.calls, |r| HostCall::ReadHidden { url: url.to_string(), signed_in: signed_in.is_some(), reply: r }).await;
        match hidden {
            Err(HostError::Unsupported(_)) if signed_in.is_none() => {
                let url = url.to_string();
                blocking(move || http(&url, HTTP_TIMEOUT)).await
            }
            other => other,
        }
    }
}

type Slot = Arc<Mutex<Option<tokio::sync::oneshot::Sender<ApprovalAnswer>>>>;
type Autos = Arc<Mutex<std::collections::HashSet<AgentId>>>;

/// Whether a card is answered without the user: the companion's task has Always allow for the
/// research tools, and the tool only navigates or researches.
fn auto_answers(auto: &Autos, agent: &AgentId, tier: Tier) -> bool {
    matches!(tier, Tier::Navigate | Tier::Research) && auto.lock().unwrap().contains(agent)
}
/// The answer to a running task's "+5 pages" card.
type PagesSlot = Arc<Mutex<Option<tokio::sync::oneshot::Sender<bool>>>>;

/// Asks the user through the panel: `decide` waits until [`Worker::answer`] is called. A tier 2
/// card's target is marked on the page while it waits.
struct PanelApprover {
    /// Companions whose current task is a project step with Always allow: its navigate and
    /// research cards are answered here (never a click, fill or submit card).
    auto: Autos,
    slot: Slot,
    pages: PagesSlot,
    pending: PendingTarget,
    msgs: Sender<Msg>,
    calls: Option<Mutex<Sender<HostCall>>>,
}

#[async_trait]
impl Approver for PanelApprover {
    async fn decide(&self, prompt: &ApprovalPrompt, tool: &ToolSpec) -> ApprovalAnswer {
        if auto_answers(&self.auto, &prompt.agent, tool.tier) {
            return ApprovalAnswer::Approve { scope: weirdnet_agent::Scope::Task };
        }
        let target = if tool.tier == Tier::Interact { self.pending.lock().unwrap().take() } else { None };
        // The tool and what it touches: the element's text, else the search engine or site.
        let detail = match (&target, &prompt.request.provider, &prompt.request.site) {
            (Some((t, _)), ..) => format!("{} · {}", tool.name, t.describe()),
            (None, Some(p), _) => format!("{} · {p}", tool.name),
            (None, None, Some(site)) => format!("{} · {site}", tool.name),
            _ => tool.name.clone(),
        };
        let _ = self.msgs.send(Msg::CardDetail(prompt.agent.clone(), detail));
        // Outlined on the page it would act on (refused if the tab has moved to another site);
        // the outline fades on its own, as the bridge has no way to clear it on Deny.
        if let (Some((t, url)), Some(calls)) = (&target, &self.calls) {
            let _ = calls.lock().unwrap().send(HostCall::Act { request: highlight_request(t, url), reply: None });
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.slot.lock().unwrap() = Some(tx);
        rx.await.unwrap_or(ApprovalAnswer::Deny)
    }

    /// The "+5 pages" card while the task runs; no when the panel goes away or the task stops.
    async fn extend_pages(&self, prompt: &weirdnet_agent::PagesPrompt) -> bool {
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.pages.lock().unwrap() = Some(tx);
        let _ = self.msgs.send(Msg::MorePages(prompt.agent.clone(), prompt.extension));
        rx.await.unwrap_or(false)
    }
}

/// The app's tool hosts for a worker.
pub struct Hosts {
    pub browser: Arc<dyn BrowserHost>,
    pub research: Arc<dyn ResearchHost>,
    /// Where tier 2 card highlights go.
    pub calls: Option<Sender<HostCall>>,
}

pub struct Worker {
    cmd: Sender<Cmd>,
    msgs: Receiver<Msg>,
    canceller: Canceller,
    approval: Slot,
    pages: PagesSlot,
    /// A task is running on the worker (so a remote turn waits its turn: this device's own first).
    busy: Arc<std::sync::atomic::AtomicBool>,
}

/// No tools: the registry is the same empty one for every task.
#[cfg(test)]
struct NoTools(Arc<ToolRegistry>);

#[cfg(test)]
impl ToolsSource for NoTools {
    fn registry_for(&self, _companion: &str) -> Arc<ToolRegistry> {
        self.0.clone()
    }

    fn handle(&self) -> Option<tokio::runtime::Handle> {
        None
    }
}

/// A task's registry: the browser and research tools, then everything in `servers` (connected
/// MCP servers, already scoped to the companion with its switches and tiers applied).
async fn with_hosts(sources: &[Arc<dyn ToolSource>], servers: Arc<ToolRegistry>) -> Arc<ToolRegistry> {
    if sources.is_empty() {
        return servers;
    }
    let mut registry = ToolRegistry::new();
    for s in sources {
        registry.add_source(s.clone());
    }
    let own = !servers.specs().is_empty();
    if own {
        registry.add_source(Arc::new(Servers(servers.clone())));
    }
    match registry.refresh().await {
        Ok(()) => Arc::new(registry),
        // Server tools are prefixed, so this only happens if a server reuses a browser tool's
        // name; keep the browser tools rather than run with neither.
        Err(e) => {
            eprintln!("morph: companion tools: {e}; MCP server tools left out of this task");
            let mut registry = ToolRegistry::new();
            for s in sources {
                registry.add_source(s.clone());
            }
            let _ = registry.refresh().await;
            Arc::new(registry)
        }
    }
}

/// A tool source that tells the panel what each call is about to do ("Searching DuckDuckGo…",
/// "Reading tokio.rs…") before it runs. `research` names the search provider.
struct Reporting {
    inner: Arc<dyn ToolSource>,
    msgs: Sender<Msg>,
    research: Option<Arc<dyn ResearchHost>>,
}

#[async_trait]
impl ToolSource for Reporting {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn kind(&self) -> SourceKind {
        self.inner.kind()
    }

    async fn list(&self) -> Result<Vec<ToolSpec>, ToolError> {
        self.inner.list().await
    }

    fn target(&self, tool: &str, args: &serde_json::Value) -> CallTarget {
        self.inner.target(tool, args)
    }

    async fn call(&self, tool: &str, ctx: &ToolContext, args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let provider = match (&self.research, tool) {
            (Some(r), "web.search") => r.provider(),
            _ => String::new(),
        };
        let _ = self.msgs.send(Msg::Doing(ctx.agent.id.clone(), doing_text(tool, &args, &provider)));
        self.inner.call(tool, ctx, args).await
    }
}

/// A registry as one tool source, so MCP servers' tools sit beside the browser tools.
struct Servers(Arc<ToolRegistry>);

#[async_trait]
impl ToolSource for Servers {
    fn id(&self) -> &str {
        "mcp-servers"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Mcp
    }

    async fn list(&self) -> Result<Vec<ToolSpec>, ToolError> {
        Ok(self.0.specs())
    }

    fn target(&self, tool: &str, args: &serde_json::Value) -> CallTarget {
        self.0.lookup(tool).map(|(src, _)| src.target(tool, args)).unwrap_or_default()
    }

    async fn call(&self, tool: &str, ctx: &ToolContext, args: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let (src, _) = self.0.lookup(tool)?;
        src.call(tool, ctx, args).await
    }
}

impl Worker {
    /// A worker with no tools (tests).
    #[cfg(test)]
    pub fn start(models: Arc<dyn ModelProvider>, policy: SharedPolicy) -> Self {
        Self::start_with(models, policy, Arc::new(NoTools(Arc::new(ToolRegistry::new()))), None)
    }

    /// A worker whose companions get the browser and research tools over `hosts`, plus the tools
    /// (connected MCP servers) `tools` has for them at each task. When `tools` has a runtime,
    /// tasks run on it.
    pub fn start_with(models: Arc<dyn ModelProvider>, policy: SharedPolicy, tools: Arc<dyn ToolsSource>, hosts: Option<Hosts>) -> Self {
        let (cmd, cmd_rx) = mpsc::channel::<Cmd>();
        let (msg_tx, msgs) = mpsc::channel::<Msg>();
        let (can_tx, can_rx) = mpsc::channel();
        let approval: Slot = Arc::default();
        let pages: PagesSlot = Arc::default();
        let pending = PendingTarget::default();
        let autos = Autos::default();
        let approver = Arc::new(PanelApprover {
            auto: autos.clone(),
            slot: approval.clone(),
            pages: pages.clone(),
            pending: pending.clone(),
            msgs: msg_tx.clone(),
            calls: hosts.as_ref().and_then(|h| h.calls.clone()).map(Mutex::new),
        });
        let exclusions = Exclusions::default();
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let running = busy.clone();
        let sources: Vec<Arc<dyn ToolSource>> = match hosts {
            Some(h) => {
                let report = |inner: Arc<dyn ToolSource>| -> Arc<dyn ToolSource> {
                    Arc::new(Reporting { inner, msgs: msg_tx.clone(), research: Some(h.research.clone()) })
                };
                vec![
                    report(Arc::new(browser_tools_with(h.browser.clone(), pending, exclusions.clone()))),
                    report(Arc::new(web_tools_with(h.research.clone(), exclusions.clone()))),
                ]
            }
            None => Vec::new(),
        };
        std::thread::Builder::new()
            .name("morph-companions".into())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_time().build() else {
                    return;
                };
                let mut sessions = Sessions::new(models, rt.block_on(with_hosts(&sources, tools.registry_for(""))), policy, approver);
                sessions.set_budget(research_budget());
                let _ = can_tx.send(sessions.canceller());
                while let Ok(Cmd::Run { roster, companion, input, options }) = cmd_rx.recv() {
                    let tx = msg_tx.clone();
                    let id = companion.clone();
                    sessions.set_tools_for(&companion, rt.block_on(with_hosts(&sources, tools.registry_for(companion.as_str()))));
                    if options.always_allow {
                        autos.lock().unwrap().insert(companion.clone());
                    }
                    exclusions.set(options.excluded.clone());
                    running.store(true, std::sync::atomic::Ordering::SeqCst);
                    let task = sessions.run_with(&roster, &companion, &input, options, |u| {
                        let _ = tx.send(Msg::Update(id.clone(), u));
                    });
                    let result = match tools.handle() {
                        Some(handle) => handle.block_on(task),
                        None => rt.block_on(task),
                    };
                    running.store(false, std::sync::atomic::Ordering::SeqCst);
                    autos.lock().unwrap().remove(&companion);
                    exclusions.set(Vec::new());
                    let _ = msg_tx.send(Msg::Done(companion, result.map(Box::new)));
                }
            })
            .expect("spawn the companion worker");
        let canceller = can_rx.recv().expect("the worker reports its canceller");
        Self { cmd, msgs, canceller, approval, pages, busy }
    }

    /// Whether a task is running now.
    pub fn busy(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.busy.clone()
    }

    #[cfg(test)]
    pub fn run(&self, roster: Roster, companion: AgentId, input: String) {
        self.run_with(roster, companion, input, TaskOptions::default());
    }

    pub fn run_with(&self, roster: Roster, companion: AgentId, input: String, options: TaskOptions) {
        let _ = self.cmd.send(Cmd::Run { roster, companion, input, options });
    }

    pub fn cancel(&self, companion: &AgentId) -> bool {
        // A card waiting for an answer is declined so the task can end.
        self.answer(ApprovalAnswer::Deny);
        self.answer_pages(false);
        self.canceller.cancel(companion)
    }

    /// The user's answer to the waiting card. Returns whether one was waiting.
    pub fn answer(&self, answer: ApprovalAnswer) -> bool {
        match self.approval.lock().unwrap().take() {
            Some(tx) => tx.send(answer).is_ok(),
            None => false,
        }
    }

    /// The user's answer to the running task's "+5 pages" card. Returns whether one was waiting.
    pub fn answer_pages(&self, yes: bool) -> bool {
        match self.pages.lock().unwrap().take() {
            Some(tx) => tx.send(yes).is_ok(),
            None => false,
        }
    }

    /// Everything the worker has sent since the last call.
    pub fn poll(&self) -> Vec<Msg> {
        self.msgs.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn always_allow_answers_navigate_and_research_cards_but_never_interact_ones() {
        let autos = Autos::default();
        let me = AgentId::new("a");
        assert!(!auto_answers(&autos, &me, Tier::Research), "no cards are answered without Always allow");
        autos.lock().unwrap().insert(me.clone());
        assert!(auto_answers(&autos, &me, Tier::Navigate) && auto_answers(&autos, &me, Tier::Research));
        assert!(!auto_answers(&autos, &me, Tier::Interact), "click, fill and submit still ask");
        assert!(!auto_answers(&autos, &AgentId::new("b"), Tier::Research), "only that companion's task");
    }

    use std::time::{Duration, Instant};

    use morph_ai::{ModelChoice, UnavailableModels};
    use weirdnet_agent::Policy;

    use super::*;

    fn wait_for(worker: &Worker, want: impl Fn(&[Msg]) -> bool) -> Vec<Msg> {
        let mut got = Vec::new();
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            got.extend(worker.poll());
            if want(&got) {
                return got;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    }

    #[test]
    fn a_task_without_a_model_reports_why_and_the_worker_stays_usable() {
        let worker = Worker::start(Arc::new(UnavailableModels::default()), Policy::new().shared());
        let roster = Roster::first_run(ModelChoice::Gemma4E4B);
        for _ in 0..2 {
            worker.run(roster.clone(), AgentId::new("local"), "hi".into());
            let got = wait_for(&worker, |m| m.iter().any(|x| matches!(x, Msg::Done(..))));
            let done = got.iter().find_map(|m| if let Msg::Done(_, r) = m { Some(r) } else { None }).expect("done");
            let err = done.as_ref().expect_err("no model");
            assert!(err.to_string().contains("no local model is set up yet"), "{err}");
        }
    }

    #[test]
    fn answering_with_nothing_waiting_is_a_no_op_and_cancel_of_idle_is_false() {
        let worker = Worker::start(Arc::new(UnavailableModels::default()), Policy::new().shared());
        assert!(!worker.answer(ApprovalAnswer::Deny));
        assert!(!worker.cancel(&AgentId::new("local")));
    }

    #[test]
    fn an_unknown_companion_is_an_error_not_a_hang() {
        let worker = Worker::start(Arc::new(UnavailableModels::default()), Policy::new().shared());
        let roster = Roster::first_run(ModelChoice::Gemma4E4B);
        worker.run(roster, AgentId::new("nobody"), "x".into());
        let got = wait_for(&worker, |m| m.iter().any(|x| matches!(x, Msg::Done(..))));
        assert!(matches!(got.last(), Some(Msg::Done(_, Err(SessionError::UnknownCompanion(_))))));
    }

    #[test]
    fn browser_calls_reach_the_panel_and_its_answers_come_back() {
        let (tx, rx) = mpsc::channel();
        let seen = Seen::default();
        *seen.active.lock().unwrap() = Some("https://tokio.rs/".into());
        let host = DesktopBrowser::new(tx, seen.clone());
        let panel = std::thread::spawn(move || {
            for call in rx.iter() {
                match call {
                    HostCall::Tabs(r) => {
                        let _ = r.send(Ok(vec![TabInfo { id: 1, title: "Tokio".into(), url: "https://tokio.rs/".into(), active: true }]));
                    }
                    HostCall::Open(url, r) => {
                        let _ = r.send(if url.contains("bad") { Err(HostError::NoTab) } else { Ok(2) });
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
        });
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        rt.block_on(async {
            assert_eq!(host.tabs().await.unwrap()[0].title, "Tokio");
            assert_eq!(host.open_url("https://docs.rs/").await.unwrap(), 2);
            assert_eq!(host.open_url("https://bad.example/").await.unwrap_err(), HostError::NoTab);
            assert_eq!(host.attached_page().await.unwrap_err(), HostError::NothingAttached);
            *seen.attached.lock().unwrap() = Some(PageSnapshot { url: "https://tokio.rs/".into(), ..Default::default() });
            assert_eq!(host.attached_page().await.unwrap().url, "https://tokio.rs/");
        });
        assert_eq!(host.active_url().as_deref(), Some("https://tokio.rs/"));
        drop(host);
        panel.join().unwrap();
    }

    #[test]
    fn a_closed_panel_is_an_error_not_a_hang() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let host = DesktopBrowser::new(tx, Seen::default());
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        assert_eq!(rt.block_on(host.tabs()).unwrap_err(), HostError::Gone);
    }

    #[test]
    fn signed_in_research_only_lends_this_windows_profile() {
        let (tx, _rx) = mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let host = DesktopResearch::new(tx, dir.path().into(), "default".into(), FileStore::new(dir.path().join("companions")));
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        assert!(matches!(rt.block_on(host.read("https://a.example/", Some("work"))), Err(HostError::Unsupported(_))));
        assert_eq!(host.provider(), "duckduckgo", "DuckDuckGo until Settings says otherwise");
        assert_eq!(host.providers(), vec!["duckduckgo".to_string(), "google".to_string()]);
    }

    #[test]
    fn the_library_matches_most_of_a_question_s_words() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let profile = morph_core::profiles::create(home, "Default").unwrap().id;
        morph_core::bookmarks::add_bookmark(home, &profile, "https://tokio.rs/tokio/topics/shutdown", "Graceful Shutdown | Tokio", 1).unwrap();
        morph_core::bookmarks::add_bookmark(home, &profile, "https://serde.rs/", "Serde", 2).unwrap();
        let hits = library_hits(home, &profile, "tokio shutdown", 8);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "Bookmark");
        assert!(library_hits(home, &profile, "", 8).is_empty());
        assert!(library_hits(home, &profile, "how to", 8).is_empty(), "only common words");
        morph_core::bookmarks::add_bookmark(home, &profile, "https://tokio.rs/", "Tokio", 3).unwrap();
        let hits = library_hits(home, &profile, "How to gracefully shut down a Tokio application?", 8);
        assert_eq!(hits.iter().map(|h| h.url.as_str()).collect::<Vec<_>>(), vec!["https://tokio.rs/tokio/topics/shutdown"]);
        assert_eq!(library_words("What's the best way to use tokio::signal?"), vec!["tokio", "signal"]);
    }

    #[test]
    fn each_tool_call_tells_the_panel_its_step_line_before_it_runs() {
        struct Echo;
        #[async_trait]
        impl weirdnet_agent::Tool for Echo {
            fn spec(&self) -> ToolSpec {
                ToolSpec::new("web.read", "read")
            }
            async fn call(&self, _: &ToolContext, _: serde_json::Value) -> Result<ToolOutput, ToolError> {
                Ok(ToolOutput::text("page"))
            }
        }
        let (msg_tx, msg_rx) = mpsc::channel();
        let source = Reporting { inner: Arc::new(weirdnet_agent::LocalTools::new("web").with(Echo)), msgs: msg_tx, research: None };
        let ctx = ToolContext {
            agent: weirdnet_agent::AgentIdentity::new("local", "Local", weirdnet_agent::AgentKind::Local),
            task_id: "t".into(),
            session: weirdnet_agent::SessionMark::LoggedOut,
            cancel: weirdnet_agent::CancelToken::new(),
        };
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let out = rt.block_on(source.call("web.read", &ctx, serde_json::json!({ "url": "https://tokio.rs/tokio/topics/shutdown" }))).unwrap();
        assert_eq!(out.content, "page");
        assert!(matches!(msg_rx.try_recv(), Ok(Msg::Doing(id, d)) if id.as_str() == "local" && d == "Reading tokio.rs…"));
        assert_eq!(source.id(), "web");
    }

    #[test]
    fn a_tier_two_card_names_its_target_and_outlines_it_on_the_approved_origin() {
        let (msg_tx, msg_rx) = mpsc::channel();
        let (call_tx, call_rx) = mpsc::channel();
        let pending = PendingTarget::default();
        *pending.lock().unwrap() = Some((PageTarget::label("Sign in"), "https://a.example/login?next=1".into()));
        let slot: Slot = Arc::default();
        let approver = PanelApprover {
            auto: Autos::default(),
            slot: slot.clone(),
            pages: PagesSlot::default(),
            pending,
            msgs: msg_tx,
            calls: Some(Mutex::new(call_tx)),
        };
        let prompt = ApprovalPrompt {
            agent: AgentId::new("local"),
            request: weirdnet_agent::ActionRequest::new(Tier::Interact).site("a.example"),
            offers: vec![weirdnet_agent::Scope::Once],
        };
        let spec = ToolSpec::new("page.click", "click").tier(Tier::Interact);
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let answer = rt.block_on(async {
            let decide = approver.decide(&prompt, &spec);
            let answer_it = async {
                while slot.lock().unwrap().is_none() {
                    tokio::task::yield_now().await;
                }
                slot.lock().unwrap().take().unwrap().send(ApprovalAnswer::Deny).unwrap();
            };
            tokio::join!(decide, answer_it).0
        });
        assert_eq!(answer, ApprovalAnswer::Deny);
        assert!(matches!(msg_rx.try_recv(), Ok(Msg::CardDetail(_, d)) if d == "page.click · “Sign in”"));
        let calls: Vec<HostCall> = call_rx.try_iter().collect();
        let [HostCall::Act { request, reply: None }] = &calls[..] else { panic!("{calls:?}") };
        assert_eq!(request.expect_origin.as_deref(), Some("https://a.example"));
        assert!(!request.allow_page_world);
        assert!(
            matches!(&request.action, Action::Highlight { target, duration } if target.name.as_deref() == Some("Sign in") && *duration == HIGHLIGHT),
            "{:?}",
            request.action
        );
    }

    #[test]
    fn page_actions_go_to_the_approved_origin_and_their_refusals_keep_their_reason() {
        let (call_tx, call_rx) = mpsc::channel();
        let browser = DesktopBrowser::new(call_tx, Seen::default());
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let target = PageTarget { label: Some("Email".into()), role: Some("textbox".into()), ..Default::default() };
        let answers = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for answer in [
                Err(ActionError::WrongOrigin { expected: "https://shop.test".into(), actual: "https://evil.test".into() }),
                Err(ActionError::NotInteractable("covered by div#banner".into())),
                Err(ActionError::BlockedByIsolation(morph_webview::Isolation::PageWorld)),
                Err(ActionError::NotFound),
            ] {
                let Ok(HostCall::Act { request, reply: Some(reply) }) = call_rx.recv() else { panic!() };
                seen.push(request);
                let _ = reply.send(acted(answer));
            }
            seen
        });
        let r = rt.block_on(async {
            vec![
                browser.fill(&target, "me@shop.test", "https://shop.test/checkout").await,
                browser.click(&target, "https://shop.test/checkout").await,
                browser.scroll(&ScrollTo::Down, "https://shop.test/").await,
                browser.scroll(&ScrollTo::Element(PageTarget { selector: Some("#faq".into()), ..Default::default() }), "https://shop.test/").await,
            ]
        });
        let seen = answers.join().unwrap();
        assert!(seen.iter().all(|r| r.expect_origin.as_deref() == Some("https://shop.test") && !r.allow_page_world));
        assert!(
            matches!(&seen[0].action, Action::Fill { target, value } if target.name.as_deref() == Some("Email") && target.role.as_deref() == Some("textbox") && value == "me@shop.test")
        );
        assert!(matches!(&seen[1].action, Action::Click(_)));
        assert!(matches!(seen[2].action, Action::ScrollBy { dx, dy } if dx == 0.0 && dy == SCROLL_STEP));
        assert!(matches!(&seen[3].action, Action::ScrollTo(l) if l.selector.as_deref() == Some("#faq")));
        assert_eq!(r[0], Err(HostError::WrongOrigin { expected: "https://shop.test".into(), actual: "https://evil.test".into() }));
        assert_eq!(r[1], Err(HostError::NotInteractable("covered by div#banner".into())));
        assert_eq!(r[2], Err(HostError::Blocked("PageWorld".into())));
        assert_eq!(r[3], Err(HostError::NotFound));
    }

    #[test]
    fn a_locked_profile_refuses_every_host_call_without_asking_the_panel() {
        let (call_tx, call_rx) = mpsc::channel();
        let locked: LockCheck = Arc::new(|| true);
        let seen = Seen::default();
        *seen.attached.lock().unwrap() = Some(PageSnapshot { url: "https://a.example/".into(), ..Default::default() });
        let browser = DesktopBrowser::new(call_tx.clone(), seen).locked_when(locked.clone());
        let tmp = std::env::temp_dir().join(format!("morph-companion-locked-{}", std::process::id()));
        let research = DesktopResearch::new(call_tx, tmp.clone(), "p1".into(), FileStore::new(tmp.join("companions"))).locked_when(locked);
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let t = PageTarget::label("Send");
        rt.block_on(async {
            assert_eq!(browser.attached_page().await, Err(HostError::Locked));
            assert_eq!(browser.tabs().await, Err(HostError::Locked));
            assert_eq!(browser.click(&t, "https://a.example/").await, Err(HostError::Locked));
            assert_eq!(browser.fill(&t, "x", "https://a.example/").await, Err(HostError::Locked));
            assert_eq!(browser.scroll(&ScrollTo::Down, "https://a.example/").await, Err(HostError::Locked));
            assert_eq!(research.search(LIBRARY, "tokio").await, Err(HostError::Locked));
            assert_eq!(research.read("https://a.example/", None).await, Err(HostError::Locked));
            assert!(matches!(research.fetch("https://a.example/", None).await, Err(HostError::Locked)));
        });
        assert!(call_rx.try_recv().is_err(), "nothing reached the panel");
    }

    #[test]
    fn a_card_outline_is_cleared_once_on_its_origin_whenever_the_card_is_done() {
        use morph_webview::HighlightId;
        let highlight = highlight_request(&PageTarget::label("Sign in"), "https://a.example/login?next=1");
        let clear =
            |id| ActionRequest { expect_origin: Some("https://a.example".into()), ..ActionRequest::new(Action::ClearHighlightOne(HighlightId(id))) };

        // Shown, then the card is answered: that one outline is cleared, once.
        let mut o = CardOutline::default();
        assert_eq!(o.asked(&highlight), None);
        assert_eq!(o.shown(Some(HighlightId(7))), None);
        assert_eq!(o.done(), Some(clear(7)));
        assert_eq!(o.done(), None, "cleared once");

        // Answered before the page said which outline: cleared when the id arrives.
        let mut o = CardOutline::default();
        o.asked(&highlight);
        assert_eq!(o.done(), None);
        assert_eq!(o.shown(Some(HighlightId(8))), Some(clear(8)));
        assert_eq!(o.done(), None);

        // A highlight that failed leaves nothing to clear.
        let mut o = CardOutline::default();
        o.asked(&highlight);
        assert_eq!(o.shown(None), None);
        assert_eq!(o.done(), None);

        // A new card's outline clears the one before; other actions are not outlines.
        let mut o = CardOutline::default();
        o.asked(&highlight);
        o.shown(Some(HighlightId(1)));
        assert_eq!(o.asked(&highlight), Some(clear(1)));
        assert_eq!(o.asked(&on_approved(Action::Click(locator(&PageTarget::label("x"))), "https://a.example/")), None);
        assert_eq!(o.shown(Some(HighlightId(2))), None);
        assert_eq!(o.done(), Some(clear(2)));
    }

    #[test]
    fn an_action_outcome_never_carries_a_password_value() {
        let outcome = morph_webview::ActionOutcome {
            view: morph_webview::ViewId(1),
            url: "https://shop.test/login".into(),
            origin: "https://shop.test".into(),
            isolation: morph_webview::Isolation::ScriptWorld,
            matches: 1,
            element: Some(morph_webview::page_action::Element {
                tag: "input".into(),
                secret: true,
                value: Some("hunter2".into()),
                editable: true,
                ..Default::default()
            }),
            scroll: Default::default(),
            highlight: None,
        };
        let a = acted(Ok(outcome)).unwrap();
        let el = a.element.unwrap();
        assert!(el.secret && el.value.is_none());
    }

    #[test]
    fn the_research_budget_is_the_design_budget_with_90_s_of_tool_time() {
        let b = research_budget();
        assert_eq!((b.max_searches, b.max_pages, b.page_extension, b.max_concurrent), (3, 12, 5, 2));
        assert_eq!(b.per_task, Duration::from_secs(90));
        assert_eq!(b, weirdnet_agent::Budget::default());
    }

    #[test]
    fn the_wall_guard_counts_the_task_but_not_the_user_deciding() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let limit = s(100);
        let mut g = WallGuard::new(t0);
        assert!(!g.over(t0 + s(60), false, limit));
        assert!(!g.over(t0 + s(70), true, limit), "a card comes up at 70 s");
        assert!(!g.over(t0 + s(500), true, limit), "the user takes their time");
        assert!(!g.over(t0 + s(510), false, limit), "answered at 510 s: 70 s of task so far");
        assert!(!g.over(t0 + s(539), false, limit), "99 s");
        assert!(g.over(t0 + s(541), false, limit), "101 s of task");
    }

    #[test]
    fn a_more_pages_card_waits_for_the_panel_and_a_stop_says_no() {
        let (msg_tx, msg_rx) = mpsc::channel();
        let pages = PagesSlot::default();
        let approver = PanelApprover {
            auto: Autos::default(),
            slot: Slot::default(),
            pages: pages.clone(),
            pending: PendingTarget::default(),
            msgs: msg_tx,
            calls: None,
        };
        let prompt = weirdnet_agent::PagesPrompt { agent: AgentId::new("local"), task_id: "t".into(), used: 12, extension: 5 };
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let ask = |yes: Option<bool>| {
            rt.block_on(async {
                let extend = approver.extend_pages(&prompt);
                let answer_it = async {
                    while pages.lock().unwrap().is_none() {
                        tokio::task::yield_now().await;
                    }
                    let tx = pages.lock().unwrap().take().unwrap();
                    if let Some(yes) = yes {
                        tx.send(yes).unwrap();
                    }
                };
                tokio::join!(extend, answer_it).0
            })
        };
        assert!(ask(Some(true)));
        assert!(matches!(msg_rx.try_recv(), Ok(Msg::MorePages(id, 5)) if id == AgentId::new("local")));
        assert!(!ask(Some(false)));
        assert!(!ask(None), "the panel dropped the card (the task stopped)");
    }

    struct Ping(&'static str);

    #[async_trait]
    impl weirdnet_agent::Tool for Ping {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new(self.0, "ping").tier(Tier::Navigate)
        }
        fn target(&self, _: &serde_json::Value) -> CallTarget {
            CallTarget::url("https://srv.example/")
        }
        async fn call(&self, _: &ToolContext, _: serde_json::Value) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::default())
        }
    }

    #[test]
    fn a_task_gets_the_browser_tools_beside_the_servers_tools_with_their_switches() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut servers = ToolRegistry::new();
        servers.add_source(Arc::new(weirdnet_agent::LocalTools::new("srv").with(Ping("srv.ping")).with(Ping("srv.off"))));
        rt.block_on(servers.refresh()).unwrap();
        servers.set_tool_enabled("srv.off", false);
        let (calls, _rx) = std::sync::mpsc::channel();
        let browser: Arc<dyn BrowserHost> = Arc::new(DesktopBrowser::new(calls, Seen::default()));
        let sources: Vec<Arc<dyn ToolSource>> = vec![Arc::new(browser_tools_with(browser, PendingTarget::default(), Exclusions::default()))];
        let both = rt.block_on(with_hosts(&sources, Arc::new(servers)));
        let names: Vec<String> = both.specs().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"page.text".to_string()) && names.contains(&"srv.ping".to_string()));
        assert!(!names.contains(&"srv.off".to_string()), "a switched-off server tool stays off");
        let (src, spec) = both.lookup("srv.ping").unwrap();
        assert_eq!((src.id(), spec.tier), ("mcp-servers", Tier::Navigate));
        assert_eq!(src.target("srv.ping", &serde_json::json!({})), CallTarget::url("https://srv.example/"));
        let none = rt.block_on(with_hosts(&[], Arc::new(ToolRegistry::new())));
        assert!(none.specs().is_empty());
    }

    #[test]
    fn research_reads_in_a_throwaway_host_page_and_falls_back_to_http_only_when_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let host = DesktopResearch::new(tx, dir.path().to_path_buf(), "default".into(), FileStore::new(dir.path().join("c")));
        let panel = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for call in rx.iter() {
                let HostCall::ReadHidden { url, signed_in, reply } = call else { panic!("unexpected {call:?}") };
                asked.push((url.clone(), signed_in));
                let _ = reply.send(if url.contains("old-engine") {
                    Err(HostError::Unsupported("throwaway (ephemeral) storage".into()))
                } else {
                    Ok((PageSnapshot { url, text: "hidden page text".into(), isolated: true, ..Default::default() }, None))
                });
            }
            asked
        });
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        let (page, _) = rt.block_on(host.read("https://docs.example/", None)).unwrap();
        assert_eq!(page.text, "hidden page text");
        let fetched = rt.block_on(host.fetch("https://docs.example/raw", Some("default"))).unwrap();
        assert_eq!(fetched.text, "hidden page text");
        // No throwaway storage: plain HTTP (port 9 refuses, so the error is HTTP's, not Unsupported).
        let http = rt.block_on(host.read("http://127.0.0.1:9/old-engine", None));
        assert!(matches!(http, Err(HostError::Fetch(_))), "{http:?}");
        // Signed in never falls back to a logged-out fetch.
        let signed = rt.block_on(host.read("http://127.0.0.1:9/old-engine", Some("default")));
        assert!(matches!(signed, Err(HostError::Unsupported(_))), "{signed:?}");
        drop(host);
        let asked = panel.join().unwrap();
        assert_eq!(
            asked,
            vec![
                ("https://docs.example/".to_string(), false),
                ("https://docs.example/raw".to_string(), true),
                ("http://127.0.0.1:9/old-engine".to_string(), false),
                ("http://127.0.0.1:9/old-engine".to_string(), true),
            ]
        );
    }
}
