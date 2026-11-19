//! Event-to-view-model translation.

use weirdnet_agent::{
    AgentEvent, ApprovalOutcome, ApprovalPrompt, Citation, CitationTarget, DenyReason, Scope, SessionMark, SkipNotice, StepKind, StopReason, Tier,
    Trace, retain_valid,
};

/// One choice on an approval card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferView {
    pub scope: Scope,
    pub label: String,
}

/// An approval card: what the companion wants to do and what the user can answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalCard {
    pub companion: String,
    pub title: String,
    pub site: Option<String>,
    /// Shown as a warning when research would use the user's signed-in session.
    pub signed_in: bool,
    /// What a tier 2 step acts on ("“Sign in”"), marked on the page while the card waits.
    pub detail: Option<String>,
    pub offers: Vec<OfferView>,
}

/// The notice shown when a standing allow skipped an approval. `allow_id` is what Stop revokes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeView {
    pub text: String,
    pub allow_id: u64,
}

/// A footnote under an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteView {
    pub n: u32,
    pub label: String,
    pub url: Option<String>,
    /// Trace step numbers to open from the footnote.
    pub steps: Vec<u32>,
}

/// One line of the trace view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceRowView {
    pub step: u32,
    pub text: String,
    /// "logged out" or "signed in as <profile>".
    pub session: String,
}

/// What the panel should do for one agent event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewUpdate {
    TaskStarted {
        task_id: String,
    },
    Text(String),
    /// Reasoning text from the model: the working line shows its latest words; the answer does not include it.
    Reasoning(String),
    Approval(ApprovalCard),
    Notice(NoticeView),
    Blocked(String),
    ToolRunning {
        call_id: String,
        tool: String,
    },
    ToolDone {
        call_id: String,
        tool: String,
        is_error: bool,
    },
    Trace(TraceRowView),
    Finished {
        task_id: String,
        status: String,
    },
    /// The answer so far is replaced by a follow-on task's (Morph asked it to read pages).
    Redo,
}

/// A search provider id as the panel names it ("duckduckgo" is "DuckDuckGo").
pub fn provider_label(provider: &str) -> String {
    match provider {
        "duckduckgo" => "DuckDuckGo".into(),
        "google" => "Google".into(),
        "bing" => "Bing".into(),
        "brave" => "Brave Search".into(),
        "kagi" => "Kagi".into(),
        "library" => "your Library".into(),
        "" => "the web".into(),
        other => other.to_string(),
    }
}

/// "tokio.rs" for "https://www.tokio.rs/tokio/topics/shutdown".
fn site_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() { "a page".into() } else { host.chars().take(60).collect() }
}

/// The live step line while a tool call runs: "Searching DuckDuckGo…", "Reading tokio.rs…".
/// `provider` is the research provider `web.search` asks after the Library.
pub fn doing_text(tool: &str, args: &serde_json::Value, provider: &str) -> String {
    let arg = |k: &str| args.get(k).and_then(serde_json::Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let label = |k: &str| arg(k).map(|v| format!(" “{}”", v.chars().take(40).collect::<String>())).unwrap_or_default();
    match tool {
        "web.search" => format!("Searching {}…", provider_label(provider)),
        "web.read" | "web.fetch" => format!("Reading {}…", arg("url").map(site_of).unwrap_or_else(|| "a page".into())),
        "page.text" | "page.selection" | "page.query" => "Reading the attached page…".into(),
        "tabs.list" => "Looking at your tabs…".into(),
        "tabs.open" | "browser.navigate" => format!("Opening {}…", arg("url").map(site_of).unwrap_or_else(|| "a page".into())),
        "tabs.switch" => "Switching tabs…".into(),
        "page.scroll" => "Scrolling the page…".into(),
        "browser.find" => format!("Finding{} on the page…", label("query")),
        "library.bookmark" => "Bookmarking the page…".into(),
        "page.click" => format!("Clicking{}…", label("target")),
        "page.fill" => format!("Filling in{}…", label("target")),
        other => format!("Running {other}…"),
    }
}

fn tier_verb(tier: Tier) -> &'static str {
    match tier {
        Tier::Read => "read the page you attached",
        Tier::Navigate => "open or navigate pages",
        Tier::Research => "search and read pages in the background",
        Tier::Interact => "act on the page",
        Tier::Never => "do something not allowed",
        // Non-exhaustive upstream: a tier this build does not know is described plainly.
        _ => "do something this version does not know",
    }
}

/// Label for one scope on an approval card.
pub fn scope_label(scope: &Scope, site: Option<&str>) -> String {
    let who = |all: &bool| if *all { "all companions" } else { "this companion" };
    match scope {
        Scope::Once => "Allow once".into(),
        Scope::Task => "Allow for this task".into(),
        Scope::AlwaysSite { all_companions } => format!("Always allow on {} for {}", site.unwrap_or("this site"), who(all_companions)),
        Scope::AlwaysEverywhere { all_companions } => format!("Always allow everywhere for {}", who(all_companions)),
        Scope::AlwaysResearch { all_companions, provider_only } => {
            // Short enough for one line of the panel's card.
            let what = if *provider_only { "this search engine" } else { "research" };
            format!("Always allow {what} for {}", who(all_companions))
        }
        _ => "Allow".into(),
    }
}

fn approval_card(companion: &str, prompt: &ApprovalPrompt) -> ApprovalCard {
    let tier = prompt.request.tier.unwrap_or(Tier::Never);
    let what = format!("{companion} wants to {}", tier_verb(tier));
    let title = match &prompt.request.site {
        Some(site) => format!("{what} on {site}"),
        None => what,
    };
    ApprovalCard {
        companion: companion.to_string(),
        title,
        site: prompt.request.site.clone(),
        signed_in: prompt.request.signed_in,
        detail: None,
        offers: prompt.offers.iter().map(|s| OfferView { scope: s.clone(), label: scope_label(s, prompt.request.site.as_deref()) }).collect(),
    }
}

fn notice(companion: &str, n: &SkipNotice) -> NoticeView {
    let target = match (&n.site, &n.provider) {
        (Some(site), _) => format!(" on {site}"),
        (None, Some(p)) => format!(" with {p}"),
        _ => String::new(),
    };
    NoticeView { text: format!("{companion}: {} allowed by Always allow{target}.", tier_verb(n.tier)), allow_id: n.allow_id }
}

fn block_text(tool: &str, reason: &DenyReason) -> String {
    format!("{tool} did not run: {reason}.")
}

fn session_text(s: &SessionMark) -> String {
    match s {
        SessionMark::LoggedOut => "logged out".into(),
        SessionMark::SignedIn { profile } => format!("signed in as {profile}"),
        _ => "unknown session".into(),
    }
}

fn step_text(kind: &StepKind) -> String {
    match kind {
        StepKind::Model { model, tool_calls } => format!("Asked {model} ({tool_calls} tool calls)"),
        StepKind::Search { provider, query, results } => format!("Searched {provider} for “{query}” ({results} results)"),
        StepKind::Fetch { url, status, .. } => match status {
            Some(s) => format!("Opened {url} ({s})"),
            None => format!("Opened {url}"),
        },
        StepKind::Extract { url, chars } => format!("Read {chars} characters from {url}"),
        StepKind::Tool { source, tool, is_error, .. } => format!("Called {source}.{tool}{}", if *is_error { " (failed)" } else { "" }),
        StepKind::Approval { tool, outcome, .. } => match outcome {
            ApprovalOutcome::Approved { scope } => format!("You approved {tool} ({})", scope_label(scope, None).to_lowercase()),
            ApprovalOutcome::AutoAllowed { allow_id: Some(_) } => format!("{tool} allowed by Always allow"),
            ApprovalOutcome::AutoAllowed { allow_id: None } => format!("{tool} allowed by a grant or earlier approval"),
            ApprovalOutcome::Denied => format!("You declined {tool}"),
            ApprovalOutcome::Blocked { reason } => block_text(tool, reason),
            _ => format!("{tool}: decided"),
        },
        StepKind::Skipped { reason, url } => match url {
            Some(u) => format!("Skipped {u}: {reason}"),
            None => format!("Skipped: {reason}"),
        },
        StepKind::Notice { text } => text.clone(),
        StepKind::Cancelled => "Stopped".into(),
        _ => "A step this version does not know how to show".into(),
    }
}

fn stop_text(stop: &StopReason) -> String {
    match stop {
        StopReason::Completed => "done".into(),
        StopReason::Cancelled => "stopped".into(),
        StopReason::Budget { exceeded } => format!("stopped: {exceeded}"),
        StopReason::Model { error } => format!("model error: {error}"),
        _ => "stopped".into(),
    }
}

/// Turns one event into what the panel shows.
pub fn translate(companion: &str, event: &AgentEvent) -> Vec<ViewUpdate> {
    match event {
        AgentEvent::TaskStarted { task_id } => vec![ViewUpdate::TaskStarted { task_id: task_id.clone() }],
        AgentEvent::TextDelta(t) => vec![ViewUpdate::Text(t.clone())],
        AgentEvent::ReasoningDelta(t) => vec![ViewUpdate::Reasoning(t.clone())],
        AgentEvent::ApprovalRequested(p) => vec![ViewUpdate::Approval(approval_card(companion, p))],
        AgentEvent::ApprovalSkipped(n) => vec![ViewUpdate::Notice(notice(companion, n))],
        AgentEvent::Blocked { tool, reason } => vec![ViewUpdate::Blocked(block_text(tool, reason))],
        AgentEvent::ToolStarted { call_id, tool } => vec![ViewUpdate::ToolRunning { call_id: call_id.clone(), tool: tool.clone() }],
        AgentEvent::ToolFinished { call_id, tool, is_error } => {
            vec![ViewUpdate::ToolDone { call_id: call_id.clone(), tool: tool.clone(), is_error: *is_error }]
        }
        AgentEvent::Step(s) => vec![ViewUpdate::Trace(TraceRowView { step: s.id.0, text: step_text(&s.kind), session: session_text(&s.session) })],
        AgentEvent::Finished { task_id, stop } => vec![ViewUpdate::Finished { task_id: task_id.clone(), status: stop_text(stop) }],
        // Events newer than this build knows show nothing.
        _ => Vec::new(),
    }
}

/// The whole trace as rows, for the trace view.
pub fn trace_rows(trace: &Trace) -> Vec<TraceRowView> {
    trace.steps.iter().map(|s| TraceRowView { step: s.id.0, text: step_text(&s.kind), session: session_text(&s.session) }).collect()
}

/// Footnotes to show: only citations the trace backs (`retain_valid`), in order of `n`.
pub fn footnotes(trace: &Trace, citations: Vec<Citation>) -> Vec<FootnoteView> {
    let mut valid = retain_valid(trace, citations);
    valid.sort_by_key(|c| c.n);
    valid
        .into_iter()
        .map(|c| {
            let (label, url) = match &c.target {
                CitationTarget::Url { url, .. } => (url.clone(), Some(url.clone())),
                CitationTarget::ToolResult { source, tool } => (format!("{source}.{tool}"), None),
                _ => ("source".to_string(), None),
            };
            FootnoteView { n: c.n, label, url, steps: c.steps.iter().map(|s| s.0).collect() }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use weirdnet_agent::{ActionRequest, AgentId, StepId, TraceStep};

    fn trace() -> Trace {
        let mut t = Trace::new("t1", AgentId::new("local"));
        t.push(StepKind::Search { provider: "DuckDuckGo".into(), query: "tokio".into(), results: 5 }, SessionMark::LoggedOut, None, 1);
        t.push(StepKind::Fetch { url: "https://tokio.rs/".into(), status: Some(200), bytes: 10 }, SessionMark::LoggedOut, None, 2);
        t
    }

    #[test]
    fn approval_card_names_companion_site_and_offers() {
        let prompt = ApprovalPrompt {
            agent: AgentId::new("local"),
            request: ActionRequest::new(Tier::Navigate).site("tokio.rs"),
            offers: vec![Scope::Once, Scope::AlwaysSite { all_companions: false }],
        };
        let out = translate("Atlas", &AgentEvent::ApprovalRequested(prompt));
        let ViewUpdate::Approval(card) = &out[0] else { panic!("not a card") };
        assert!(card.title.starts_with("Atlas wants to open or navigate pages on tokio.rs"));
        assert_eq!(card.offers[0].label, "Allow once");
        assert_eq!(card.offers[1].label, "Always allow on tokio.rs for this companion");
        assert!(!card.signed_in);
    }

    #[test]
    fn signed_in_research_card_is_flagged() {
        let prompt = ApprovalPrompt {
            agent: AgentId::new("local"),
            request: ActionRequest::new(Tier::Research).provider("DuckDuckGo").signed_in(true),
            offers: vec![Scope::Once],
        };
        let out = translate("Local", &AgentEvent::ApprovalRequested(prompt));
        let ViewUpdate::Approval(card) = &out[0] else { panic!() };
        assert!(card.signed_in);
    }

    #[test]
    fn skipped_approval_becomes_a_notice_with_the_allow_id() {
        let n = SkipNotice { allow_id: 7, tier: Tier::Research, site: None, provider: Some("DuckDuckGo".into()) };
        let out = translate("Local", &AgentEvent::ApprovalSkipped(n));
        assert_eq!(
            out,
            vec![ViewUpdate::Notice(NoticeView {
                text: "Local: search and read pages in the background allowed by Always allow with DuckDuckGo.".into(),
                allow_id: 7
            })]
        );
    }

    #[test]
    fn blocked_text_uses_the_reason() {
        let out = translate("Local", &AgentEvent::Blocked { tool: "click".into(), reason: DenyReason::SiteOff { site: "bank.com".into() } });
        assert_eq!(out, vec![ViewUpdate::Blocked("click did not run: site bank.com is set to Off.".into())]);
    }

    #[test]
    fn steps_stream_as_trace_rows_with_session_marks() {
        let step = TraceStep {
            id: StepId(3),
            at_unix_ms: 0,
            agent: AgentId::new("local"),
            node: None,
            session: SessionMark::SignedIn { profile: "me".into() },
            kind: StepKind::Cancelled,
        };
        let out = translate("Local", &AgentEvent::Step(step));
        assert_eq!(out, vec![ViewUpdate::Trace(TraceRowView { step: 3, text: "Stopped".into(), session: "signed in as me".into() })]);
        let rows = trace_rows(&trace());
        assert_eq!(rows[0].text, "Searched DuckDuckGo for “tokio” (5 results)");
        assert_eq!(rows[1].text, "Opened https://tokio.rs/ (200)");
    }

    #[test]
    fn footnotes_drop_citations_the_trace_does_not_back() {
        let t = trace();
        let good = Citation { n: 2, target: CitationTarget::Url { url: "https://tokio.rs/".into(), quote: None }, steps: vec![StepId(2)] };
        let never_fetched =
            Citation { n: 1, target: CitationTarget::Url { url: "https://evil.example/".into(), quote: None }, steps: vec![StepId(2)] };
        let no_steps = Citation { n: 3, target: CitationTarget::Url { url: "https://tokio.rs/".into(), quote: None }, steps: vec![] };
        let f = footnotes(&t, vec![good, never_fetched, no_steps]);
        assert_eq!(f, vec![FootnoteView { n: 2, label: "https://tokio.rs/".into(), url: Some("https://tokio.rs/".into()), steps: vec![2] }]);
    }

    #[test]
    fn finished_status_text() {
        let out = translate("L", &AgentEvent::Finished { task_id: "t".into(), stop: StopReason::Cancelled });
        assert_eq!(out, vec![ViewUpdate::Finished { task_id: "t".into(), status: "stopped".into() }]);
    }

    #[test]
    fn step_lines_name_the_provider_the_site_and_the_label() {
        use serde_json::json;
        assert_eq!(doing_text("web.search", &json!({ "query": "x" }), "duckduckgo"), "Searching DuckDuckGo…");
        assert_eq!(doing_text("web.read", &json!({ "url": "https://www.tokio.rs/tokio/topics/shutdown" }), ""), "Reading tokio.rs…");
        assert_eq!(doing_text("web.fetch", &json!({ "url": "docs.rs/x?y=1" }), ""), "Reading docs.rs…");
        assert_eq!(doing_text("web.read", &json!({}), ""), "Reading a page…");
        assert_eq!(doing_text("page.fill", &json!({ "target": "Email", "value": "a@b.c" }), ""), "Filling in “Email”…");
        assert_eq!(doing_text("page.click", &json!({ "target": " Subscribe " }), ""), "Clicking “Subscribe”…");
        assert_eq!(doing_text("github.issues", &json!({}), ""), "Running github.issues…");
    }
}
