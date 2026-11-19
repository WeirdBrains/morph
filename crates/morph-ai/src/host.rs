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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollTo {
    Up,
    Down,
    Top,
    Bottom,
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
    async fn scroll(&self, _to: ScrollTo) -> Result<(), HostError> {
        Err(HostError::Unsupported("scrolling".into()))
    }
    /// Finds and highlights `query` in the active page; the number of matches.
    async fn find_in_page(&self, query: &str) -> Result<u32, HostError>;
    async fn bookmark(&self, url: &str, title: &str) -> Result<(), HostError>;
    /// Marks `text` on the active page while an approval card about it waits (`None` clears it).
    async fn highlight(&self, _text: Option<&str>) -> Result<(), HostError> {
        Ok(())
    }
    /// Tier 2: clicks the link or button labelled `target` on the active page.
    async fn click(&self, _target: &str) -> Result<(), HostError> {
        Err(HostError::Unsupported("acting on pages".into()))
    }
    /// Tier 2: types `value` into the field labelled `target` (never submits).
    async fn fill(&self, _target: &str, _value: &str) -> Result<(), HostError> {
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
