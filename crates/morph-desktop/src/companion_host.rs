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
use std::time::Duration;

use async_trait::async_trait;
use morph_ai::{
    ActedOn, AiStore, BrowserHost, FetchedPage, FileStore, HostError, LIBRARY, ModelProvider, PageElement, PageSnapshot, PageTarget, ResearchHost,
    Roster, ScrollTo, SearchHit, SearchProvider, TabInfo,
};
use morph_companion::{Canceller, PendingTarget, SessionError, Sessions, TaskOptions, ViewUpdate, browser_tools, web_tools};
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
}

enum Cmd {
    Run { roster: Roster, companion: AgentId, input: String, options: TaskOptions },
}

/// How long the panel has to answer a host call before the tool gives up.
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// Logged-out fetches and searches.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// The research budget: the design's searches, pages, concurrency and per-page limits, with a
/// longer task limit. `weirdnet-agent` 0.1 counts model turns and time spent on approval cards
/// against `per_task`, and a local model on a CPU takes most of 90 s for one turn, so the
/// design's 90 s of research would end tasks before they research (docs/handoff.md).
pub fn research_budget() -> weirdnet_agent::Budget {
    weirdnet_agent::Budget { per_task: Duration::from_secs(600), ..Default::default() }
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

/// How long a card's outline stays on the page (the bridge's most; it has no way to clear one).
const HIGHLIGHT: Duration = Duration::from_secs(30);
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

/// The browser host: tab actions go to the panel; the attached page was read when the user sent.
pub struct DesktopBrowser {
    calls: Mutex<Sender<HostCall>>,
    seen: Seen,
}

impl DesktopBrowser {
    pub fn new(calls: Sender<HostCall>, seen: Seen) -> Self {
        Self { calls: Mutex::new(calls), seen }
    }
}

#[async_trait]
impl BrowserHost for DesktopBrowser {
    async fn attached_page(&self) -> Result<PageSnapshot, HostError> {
        self.seen.attached.lock().unwrap().clone().ok_or(HostError::NothingAttached)
    }
    async fn tabs(&self) -> Result<Vec<TabInfo>, HostError> {
        ask(&self.calls, HostCall::Tabs).await
    }
    fn active_url(&self) -> Option<String> {
        self.seen.active.lock().unwrap().clone()
    }
    fn cached_tabs(&self) -> Vec<TabInfo> {
        self.seen.tabs.lock().unwrap().clone()
    }
    async fn open_url(&self, url: &str) -> Result<u64, HostError> {
        ask(&self.calls, |r| HostCall::Open(url.to_string(), r)).await
    }
    async fn switch_tab(&self, tab: u64) -> Result<(), HostError> {
        ask(&self.calls, |r| HostCall::Switch(tab, r)).await
    }
    async fn navigate(&self, url: &str) -> Result<(), HostError> {
        ask(&self.calls, |r| HostCall::Navigate(url.to_string(), r)).await
    }
    async fn scroll(&self, to: &ScrollTo, approved_url: &str) -> Result<ActedOn, HostError> {
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
        ask(&self.calls, |r| HostCall::Find(query.to_string(), r)).await
    }
    async fn bookmark(&self, url: &str, title: &str) -> Result<(), HostError> {
        ask(&self.calls, |r| HostCall::Bookmark { url: url.to_string(), title: title.to_string(), reply: r }).await
    }
    async fn highlight(&self, target: &PageTarget, approved_url: &str) -> Result<(), HostError> {
        let request = highlight_request(target, approved_url);
        self.calls.lock().unwrap().send(HostCall::Act { request, reply: None }).map_err(|_| HostError::Gone)
    }
    async fn click(&self, target: &PageTarget, approved_url: &str) -> Result<ActedOn, HostError> {
        let request = on_approved(Action::Click(locator(target)), approved_url);
        ask(&self.calls, |r| HostCall::Act { request, reply: Some(r) }).await
    }
    async fn fill(&self, target: &PageTarget, value: &str, approved_url: &str) -> Result<ActedOn, HostError> {
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
}

impl DesktopResearch {
    pub fn new(calls: Sender<HostCall>, home: PathBuf, profile: String, store: FileStore) -> Self {
        Self { calls: Mutex::new(calls), home, profile, store }
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
        let (provider, query) = (provider.to_string(), query.to_string());
        if provider == LIBRARY {
            let (home, profile) = (self.home.clone(), self.profile.clone());
            return blocking(move || Ok(library_hits(&home, &profile, &query, 8))).await;
        }
        blocking(move || companion_web::search(&provider, &query, HTTP_TIMEOUT)).await
    }
    async fn fetch(&self, url: &str, signed_in: Option<&str>) -> Result<FetchedPage, HostError> {
        let (page, status) = self.hidden_or_http(url, signed_in, companion_web::fetch_page).await?;
        Ok(FetchedPage { bytes: page.text.len() as u64, url: page.url, status, text: page.text })
    }
    async fn read(&self, url: &str, signed_in: Option<&str>) -> Result<(PageSnapshot, Option<u16>), HostError> {
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

/// Asks the user through the panel: `decide` waits until [`Worker::answer`] is called. A tier 2
/// card's target is marked on the page while it waits.
struct PanelApprover {
    slot: Slot,
    pending: PendingTarget,
    msgs: Sender<Msg>,
    calls: Option<Mutex<Sender<HostCall>>>,
}

#[async_trait]
impl Approver for PanelApprover {
    async fn decide(&self, prompt: &ApprovalPrompt, tool: &ToolSpec) -> ApprovalAnswer {
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
        let pending = PendingTarget::default();
        let approver = Arc::new(PanelApprover {
            slot: approval.clone(),
            pending: pending.clone(),
            msgs: msg_tx.clone(),
            calls: hosts.as_ref().and_then(|h| h.calls.clone()).map(Mutex::new),
        });
        let sources: Vec<Arc<dyn ToolSource>> = match hosts {
            Some(h) => vec![Arc::new(browser_tools(h.browser, pending)), Arc::new(web_tools(h.research))],
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
                    let task = sessions.run_with(&roster, &companion, &input, options, |u| {
                        let _ = tx.send(Msg::Update(id.clone(), u));
                    });
                    let result = match tools.handle() {
                        Some(handle) => handle.block_on(task),
                        None => rt.block_on(task),
                    };
                    let _ = msg_tx.send(Msg::Done(companion, result.map(Box::new)));
                }
            })
            .expect("spawn the companion worker");
        let canceller = can_rx.recv().expect("the worker reports its canceller");
        Self { cmd, msgs, canceller, approval }
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
        self.canceller.cancel(companion)
    }

    /// The user's answer to the waiting card. Returns whether one was waiting.
    pub fn answer(&self, answer: ApprovalAnswer) -> bool {
        match self.approval.lock().unwrap().take() {
            Some(tx) => tx.send(answer).is_ok(),
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
    fn a_tier_two_card_names_its_target_and_outlines_it_on_the_approved_origin() {
        let (msg_tx, msg_rx) = mpsc::channel();
        let (call_tx, call_rx) = mpsc::channel();
        let pending = PendingTarget::default();
        *pending.lock().unwrap() = Some((PageTarget::label("Sign in"), "https://a.example/login?next=1".into()));
        let slot: Slot = Arc::default();
        let approver = PanelApprover { slot: slot.clone(), pending, msgs: msg_tx, calls: Some(Mutex::new(call_tx)) };
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
        };
        let a = acted(Ok(outcome)).unwrap();
        let el = a.element.unwrap();
        assert!(el.secret && el.value.is_none());
    }

    #[test]
    fn the_research_budget_keeps_the_design_limits_but_a_longer_task_time() {
        let b = research_budget();
        let d = weirdnet_agent::Budget::default();
        assert_eq!((b.max_searches, b.max_pages, b.page_extension, b.max_concurrent), (3, 12, 5, 2));
        assert_eq!((b.per_page, b.tokens_per_page), (d.per_page, d.tokens_per_page));
        assert!(b.per_task > d.per_task);
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
        let sources: Vec<Arc<dyn ToolSource>> = vec![Arc::new(browser_tools(browser, PendingTarget::default()))];
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
