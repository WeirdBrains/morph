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
    AiStore, BrowserHost, FetchedPage, FileStore, HostError, LIBRARY, ModelProvider, PageSnapshot, ResearchHost, Roster, ScrollTo, SearchHit,
    SearchProvider, TabInfo,
};
use morph_companion::{Canceller, PendingTarget, SessionError, Sessions, TaskOptions, ViewUpdate, browser_tools, web_tools};
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

pub type Reply<T> = tokio::sync::oneshot::Sender<Result<T, HostError>>;

/// A request from a companion tool that only the UI thread can answer.
pub enum HostCall {
    Tabs(Reply<Vec<TabInfo>>),
    Open(String, Reply<u64>),
    Switch(u64, Reply<()>),
    Navigate(String, Reply<()>),
    Scroll(ScrollTo, Reply<()>),
    Find(String, Reply<u32>),
    /// Mark text on the active page while a card about it waits (`None` clears it).
    Highlight(Option<String>),
    Bookmark {
        url: String,
        title: String,
        reply: Reply<()>,
    },
    /// Read `url` in a hidden host page of this window's profile (its signed-in session).
    ReadHidden {
        url: String,
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
            HostCall::Scroll(..) => "Scroll",
            HostCall::Find(..) => "Find",
            HostCall::Highlight(_) => "Highlight",
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
    async fn scroll(&self, to: ScrollTo) -> Result<(), HostError> {
        ask(&self.calls, |r| HostCall::Scroll(to, r)).await
    }
    async fn find_in_page(&self, query: &str) -> Result<u32, HostError> {
        ask(&self.calls, |r| HostCall::Find(query.to_string(), r)).await
    }
    async fn bookmark(&self, url: &str, title: &str) -> Result<(), HostError> {
        ask(&self.calls, |r| HostCall::Bookmark { url: url.to_string(), title: title.to_string(), reply: r }).await
    }
    async fn highlight(&self, text: Option<&str>) -> Result<(), HostError> {
        self.calls.lock().unwrap().send(HostCall::Highlight(text.map(str::to_string))).map_err(|_| HostError::Gone)
    }
    // click and fill: the page bridge has no way yet to run a host script in a tab
    // (docs/handoff.md, bridge requests), so tier 2 answers Unsupported after its card.
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
    let mut hits: Vec<SearchHit> = Vec::new();
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let matches = |title: &str, url: &str| {
        let hay = format!("{} {}", title.to_lowercase(), url.to_lowercase());
        !words.is_empty() && words.iter().all(|w| hay.contains(w.as_str()))
    };
    if let Ok(marks) = morph_core::bookmarks::bookmarks(home, profile) {
        for b in marks.iter().filter(|b| matches(&b.title, &b.url)) {
            hits.push(SearchHit { title: b.title.clone(), url: b.url.clone(), snippet: "Bookmark".into() });
        }
    }
    if let Ok(visits) = morph_core::library::history(home, profile, "", 2000) {
        for v in visits.iter().filter(|v| matches(&v.title, &v.url)) {
            if !hits.iter().any(|h| h.url == v.url) {
                hits.push(SearchHit { title: v.title.clone(), url: v.url.clone(), snippet: "Visited".into() });
            }
        }
    }
    hits.truncate(max);
    hits
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
        match signed_in {
            None => {
                let url = url.to_string();
                blocking(move || companion_web::fetch(&url, HTTP_TIMEOUT)).await
            }
            Some(p) => {
                self.check_profile(p)?;
                let (page, status) = ask(&self.calls, |r| HostCall::ReadHidden { url: url.to_string(), reply: r }).await?;
                Ok(FetchedPage { bytes: page.text.len() as u64, url: page.url, status, text: page.text })
            }
        }
    }
    async fn read(&self, url: &str, signed_in: Option<&str>) -> Result<(PageSnapshot, Option<u16>), HostError> {
        match signed_in {
            None => {
                let url = url.to_string();
                blocking(move || companion_web::read(&url, HTTP_TIMEOUT)).await
            }
            Some(p) => {
                self.check_profile(p)?;
                ask(&self.calls, |r| HostCall::ReadHidden { url: url.to_string(), reply: r }).await
            }
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
        let detail = match &target {
            Some(t) => format!("{} · “{t}”", tool.name),
            None => tool.name.clone(),
        };
        let _ = self.msgs.send(Msg::CardDetail(prompt.agent.clone(), detail));
        let highlight = |text: Option<String>| {
            if let Some(calls) = &self.calls {
                let _ = calls.lock().unwrap().send(HostCall::Highlight(text));
            }
        };
        if target.is_some() {
            highlight(target.clone());
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.slot.lock().unwrap() = Some(tx);
        let answer = rx.await.unwrap_or(ApprovalAnswer::Deny);
        if target.is_some() && answer == ApprovalAnswer::Deny {
            highlight(None);
        }
        answer
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
    fn the_library_is_searched_by_every_word() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let profile = morph_core::profiles::create(home, "Default").unwrap().id;
        morph_core::bookmarks::add_bookmark(home, &profile, "https://tokio.rs/tokio/topics/shutdown", "Graceful Shutdown | Tokio", 1).unwrap();
        morph_core::bookmarks::add_bookmark(home, &profile, "https://serde.rs/", "Serde", 2).unwrap();
        let hits = library_hits(home, &profile, "tokio shutdown", 8);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "Bookmark");
        assert!(library_hits(home, &profile, "", 8).is_empty());
    }

    #[test]
    fn a_tier_two_card_names_its_target_and_marks_it_until_declined() {
        let (msg_tx, msg_rx) = mpsc::channel();
        let (call_tx, call_rx) = mpsc::channel();
        let pending = PendingTarget::default();
        *pending.lock().unwrap() = Some("Sign in".into());
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
        assert!(matches!(&calls[..], [HostCall::Highlight(Some(t)), HostCall::Highlight(None)] if t == "Sign in"), "{calls:?}");
    }
}
