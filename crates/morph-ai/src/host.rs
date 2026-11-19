//! What the app provides to companion tools. The tools themselves (`morph-companion` `tools`)
//! call these after the policy has approved them; the app implements them over its tabs, web
//! views (the page bridge, docs/bridge-api.md) and network stack.
//!
//! Everything a page or a site returns here is untrusted model input: the tools wrap it before
//! a model sees it, and nothing in it can widen what a companion may do.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    #[error("no such tab")]
    NoTab,
    #[error("nothing is attached")]
    NothingAttached,
    #[error("the page could not be loaded: {0}")]
    Fetch(String),
    #[error("search failed: {0}")]
    Search(String),
    #[error("the page could not be read: {0}")]
    Read(String),
    #[error("not supported here: {0}")]
    Unsupported(String),
    #[error("the browser window is gone")]
    Gone,
    /// Tier 2 and scrolling: nothing on the page matched the target.
    #[error("no element on the page matches")]
    NotFound,
    /// Found, but hidden, disabled, covered, read-only or not a field.
    #[error("the element can't be used: {0}")]
    NotInteractable(String),
    /// The engine could only act where the page's own scripts could interfere.
    #[error("refused: {0}")]
    Blocked(String),
    /// The tab is no longer on the site the user approved.
    #[error("refused: approved for {expected}, but the tab is now on {actual}")]
    WrongOrigin { expected: String, actual: String },
    /// The page moved to another site while the action was starting.
    #[error("refused: the page moved from {from} to {to}")]
    Navigated { from: String, to: String },
    /// The page did not answer in time; the action may still have run.
    #[error("the page did not answer in time (the action may still have run)")]
    Timeout,
}

/// A link on a page.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PageLink {
    pub href: String,
    pub text: String,
}

/// A page as text: one the user attached, or one research read.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PageSnapshot {
    pub url: String,
    pub title: String,
    pub text: String,
    /// What the user had selected when they attached the page.
    #[serde(default)]
    pub selection: Option<String>,
    #[serde(default)]
    pub links: Vec<PageLink>,
    /// The text was shortened to fit the read limit.
    #[serde(default)]
    pub truncated: bool,
    /// The read ran where page scripts could not change it (`WebView::isolation()`); when false
    /// the tools say so next to the text.
    #[serde(default)]
    pub isolated: bool,
}

/// One tab, as `tabs.list` shows it (titles and addresses only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub active: bool,
}

/// An element on a page a tier 2 tool or a scroll names: its label (accessible name: label,
/// aria-label, button or link text, placeholder), and optionally a CSS selector, a role and
/// which match (0 is the first).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PageTarget {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub nth: usize,
}

impl PageTarget {
    pub fn label(label: impl Into<String>) -> Self {
        Self { label: Some(label.into()), ..Self::default() }
    }

    /// How a card or a tool result names it: “Sign in”, the button “Send”, `#q`.
    pub fn describe(&self) -> String {
        let mut s = match (&self.role, &self.label) {
            (Some(role), Some(label)) => format!("the {role} “{label}”"),
            (None, Some(label)) => format!("“{label}”"),
            (Some(role), None) => format!("the {role}"),
            (None, None) => String::new(),
        };
        if let Some(sel) = &self.selector {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&format!("`{sel}`"));
        }
        if self.nth > 0 {
            s.push_str(&format!(" (match {})", self.nth + 1));
        }
        if s.is_empty() { "the element".into() } else { s }
    }
}

/// An element as the page reported it after an action (untrusted page content).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PageElement {
    pub tag: String,
    pub role: String,
    pub name: String,
    pub text: String,
    /// A field's value; never a password field's (`secret`).
    pub value: Option<String>,
    pub secret: bool,
    pub editable: bool,
    pub disabled: bool,
    pub href: Option<String>,
}

/// What a page action did.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActedOn {
    /// The page's address when the action ran.
    pub url: String,
    /// How many elements matched the target.
    pub matches: usize,
    pub element: Option<PageElement>,
    /// A scroll moved the page (or a box in it).
    pub moved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollTo {
    Up,
    Down,
    Top,
    Bottom,
    /// Bring an element to the middle of the view.
    Element(PageTarget),
}

/// The browser, from a companion's point of view. Tier 0 reads only what the user attached;
/// tier 1 navigates; tier 2 acts on the page.
#[async_trait]
pub trait BrowserHost: Send + Sync {
    /// The page the user attached to this message, read when they sent it (Tier 0).
    async fn attached_page(&self) -> Result<PageSnapshot, HostError>;
    /// Titles and addresses of the window's tabs (Tier 0).
    async fn tabs(&self) -> Result<Vec<TabInfo>, HostError>;
    /// The active tab's address as last seen, for the policy's per-site check before a call.
    fn active_url(&self) -> Option<String> {
        None
    }
    /// The tabs as last seen, without asking the browser (for the policy's per-site check).
    fn cached_tabs(&self) -> Vec<TabInfo> {
        Vec::new()
    }
    /// Opens a URL in a new tab and returns its tab id.
    async fn open_url(&self, url: &str) -> Result<u64, HostError>;
    async fn switch_tab(&self, tab: u64) -> Result<(), HostError>;
    /// Loads a URL in the active tab.
    async fn navigate(&self, url: &str) -> Result<(), HostError>;
    /// Scrolls the active tab. `approved_url` is the address the call was approved for; the
    /// host refuses when the tab has moved to another site since.
    async fn scroll(&self, _to: &ScrollTo, _approved_url: &str) -> Result<ActedOn, HostError> {
        Err(HostError::Unsupported("scrolling".into()))
    }
    /// Finds and highlights `query` in the active page; the number of matches.
    async fn find_in_page(&self, query: &str) -> Result<u32, HostError>;
    async fn bookmark(&self, url: &str, title: &str) -> Result<(), HostError>;
    /// Outlines `target` on the active page while an approval card about it waits (it fades on
    /// its own; there is no clear).
    async fn highlight(&self, _target: &PageTarget, _approved_url: &str) -> Result<(), HostError> {
        Ok(())
    }
    /// Tier 2, after approval: clicks `target` on the active tab, refused unless the tab is still
    /// on `approved_url`'s site.
    async fn click(&self, _target: &PageTarget, _approved_url: &str) -> Result<ActedOn, HostError> {
        Err(HostError::Unsupported("acting on pages".into()))
    }
    /// Tier 2, after approval: types `value` into the field `target` (never submits), refused
    /// unless the tab is still on `approved_url`'s site.
    async fn fill(&self, _target: &PageTarget, _value: &str, _approved_url: &str) -> Result<ActedOn, HostError> {
        Err(HostError::Unsupported("acting on pages".into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchedPage {
    pub url: String,
    pub status: Option<u16>,
    pub text: String,
    pub bytes: u64,
}

/// The Library (bookmarks and history), searched before any web provider.
pub const LIBRARY: &str = "library";

/// Background research: the Library, the user's chosen web provider (DuckDuckGo by default, no
/// API key), and page fetches and reads in hidden contexts. Logged out unless `signed_in` names
/// the profile the user opted in for this one task.
#[async_trait]
pub trait ResearchHost: Send + Sync {
    /// Ids of the web providers the user can pick from (not counting the Library).
    fn providers(&self) -> Vec<String>;
    /// The provider research uses now (Settings › Agents).
    fn provider(&self) -> String;
    /// `provider` is [`LIBRARY`] or one of [`ResearchHost::providers`].
    async fn search(&self, provider: &str, query: &str) -> Result<Vec<SearchHit>, HostError>;
    /// The page's body as text.
    async fn fetch(&self, url: &str, signed_in: Option<&str>) -> Result<FetchedPage, HostError>;
    /// The page's readable text and links, rendered in a hidden context.
    async fn read(&self, url: &str, signed_in: Option<&str>) -> Result<(PageSnapshot, Option<u16>), HostError>;
}
