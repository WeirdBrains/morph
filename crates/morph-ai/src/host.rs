//! What the app provides to companion tools. The tools themselves live in the app, which
//! implements these over its tabs, web views and network stack.

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
    #[error("not supported here")]
    Unsupported,
}

/// A page the user attached, as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageSnapshot {
    pub url: String,
    pub title: String,
    pub text: String,
}

/// The browser, from a companion's point of view. Tier 1 and 2 actions go through here
/// after the policy has approved them.
#[async_trait]
pub trait BrowserHost: Send + Sync {
    /// The page the user attached to the conversation, if any (Tier 0).
    async fn attached_page(&self) -> Result<PageSnapshot, HostError>;
    /// Opens a URL in a new tab and returns its tab id.
    async fn open_url(&self, url: &str) -> Result<u64, HostError>;
    async fn switch_tab(&self, tab: u64) -> Result<(), HostError>;
    async fn find_in_page(&self, query: &str) -> Result<u32, HostError>;
    async fn bookmark(&self, url: &str, title: &str) -> Result<(), HostError>;
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

/// Background research: the user's chosen search provider (Library first, then DuckDuckGo by
/// default) and logged-out page fetches.
#[async_trait]
pub trait ResearchHost: Send + Sync {
    /// Names of the providers the user can pick from.
    fn providers(&self) -> Vec<String>;
    async fn search(&self, provider: &str, query: &str) -> Result<Vec<SearchHit>, HostError>;
    /// Fetches a page in a session with no cookies, unless `signed_in` names a profile the
    /// user opted in for this task.
    async fn fetch(&self, url: &str, signed_in: Option<&str>) -> Result<FetchedPage, HostError>;
}
