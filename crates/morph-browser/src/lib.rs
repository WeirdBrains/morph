//! The browser component (ARCHITECTURE.md section 2a): tabs, the address
//! bar, back, forward, reload and stop, and history, with no UI toolkit.
//!
//! A shell draws the tab strip and toolbar from this state and tells the
//! component where the page area is; pages are [`WebView`]s from whatever
//! [`Backend`] the shell gives it (webkit2gtk on Linux, the fake in tests).
//! Nothing outside this crate knows a web view exists.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

mod download_auth;
pub mod downloads;

use morph_core::app_call::{self, AppCallError, AppCaller, AppPage};
use morph_core::tabs::{TabId, TabSet};
use morph_core::{address, omnibox, search};
use morph_ui::actions::Action;
use morph_ui::text::TextField;
use morph_webview::{Backend, ErrorKind, Event, Frame, Input, LoadError, MenuChoice, PageCursor, Rect, Spec, ViewId, WebView};

pub mod menu;
pub use menu::{MenuKey, MenuState};

/// Called with `(url, title)` when a page finishes loading or its title
/// changes afterwards. The desktop shell writes it to the profile's history.
pub type HistorySink = Box<dyn FnMut(&str, &str)>;

/// What a browser starts with.
#[derive(Default)]
pub struct Config {
    /// The profile's web data directory; `None` uses the engine default.
    pub data_dir: Option<PathBuf>,
    /// First tab's address; empty for a blank tab with the address bar focused.
    pub start_url: String,
    pub history: Option<HistorySink>,
    /// Print every page event to stderr (`MORPH_LOG_EVENTS=1`), for the Xvfb
    /// checks in `scripts/xvfb-check.sh`.
    pub log_events: bool,
    /// The search engine new tabs use ([`morph_core::search`] id).
    pub search_engine: String,
    /// What calls Weirdnet apps for `weirdnet-<app>://` addresses (the
    /// shells pass weirdnet-net's, `morph_services::app_calls`); `None` is
    /// [`app_call::NotYet`] (no network: every call says so).
    pub app_caller: Option<Arc<dyn AppCaller>>,
    /// The first tab's back/forward history ([`DetachedTab::session`]): a
    /// tab moved here from another window. `start_url` is loaded instead
    /// when the engine can't restore it.
    pub start_session: Option<Vec<u8>>,
}

/// A tab taken out of a window ([`Browser::detach_tab`]) to open in
/// another: its address and title, its engine history when the engine
/// can hand it over, and the search engine it picked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetachedTab {
    pub url: String,
    pub title: String,
    pub session: Option<Vec<u8>>,
    pub engine: Option<String>,
}

/// The most of a `data:` address the engine takes. WebView2 refuses
/// addresses over 2 MB; WebKit has no such limit.
const DATA_URL_MAX: Option<usize> = if cfg!(windows) { Some(2_000_000) } else { None };

/// A `weirdnet-<app>://` call under way for a tab: it runs on a thread of
/// its own ([`app_call::fetch`]) and reports here; [`Browser::poll`] picks
/// the result up. Dropping it stops the call.
struct AppLoad {
    url: String,
    /// The tab's address and title before, put back if the call fails or is
    /// stopped.
    before: (String, String),
    events: mpsc::Receiver<AppLoadEvent>,
    stop: Arc<AtomicBool>,
    /// Body bytes read so far.
    received: u64,
}

impl Drop for AppLoad {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

enum AppLoadEvent {
    Progress(u64),
    Done(Result<AppPage, AppCallError>),
}

/// Per-tab page state beyond `morph_core::tabs::Tab`.
struct Page {
    view: Box<dyn WebView>,
    loading: bool,
    can_back: bool,
    can_forward: bool,
    /// What the shell shows instead of the page, if anything.
    cover: Option<Cover>,
    /// The user chose to wait for a page that stopped answering; it is not
    /// covered again until it has answered.
    waited: bool,
    /// The page's process was ended on purpose (Reload on a hung page); its
    /// crash report is expected and ignored once.
    terminating: bool,
    /// The address the page still shows under an error page (the engine
    /// puts it back when a load fails), for Back.
    shown_url: Option<String>,
    /// The page's title under Morph's own pages (About, Settings), for Back.
    shown_title: Option<String>,
    finished: bool,
    popups: usize,
    /// The page's open `<select>` list (pages the shell draws).
    menu: Option<MenuState>,
    /// The page's tooltip under the pointer.
    tooltip: Option<String>,
}

impl Page {
    fn new(view: Box<dyn WebView>) -> Self {
        Self {
            view,
            loading: false,
            can_back: false,
            can_forward: false,
            cover: None,
            waited: false,
            terminating: false,
            shown_url: None,
            shown_title: None,
            finished: false,
            popups: 0,
            menu: None,
            tooltip: None,
        }
    }
}

/// What covers the page area instead of the page. The web view is hidden
/// while one is shown, since nothing can draw over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cover {
    /// The page's process ended.
    Crashed,
    /// The page's process stopped answering.
    Unresponsive,
    /// The last main-frame load failed.
    Failed(LoadError),
    /// Morph's About page (morph://about), drawn by the shell.
    About,
    /// Morph's preferences (morph://settings), drawn by the shell.
    Settings,
    /// A tab with nothing in it yet (a new or blank tab): the shell draws
    /// Morph's start screen, as the phone app does, until an address loads.
    Start,
}

impl Cover {
    /// One of Morph's own pages (About, Settings) rather than a problem
    /// with the page.
    pub fn is_own(&self) -> bool {
        matches!(self, Cover::About | Cover::Settings | Cover::Start)
    }
}

/// The address of Morph's preferences.
pub const SETTINGS_URL: &str = "morph://settings";

/// The address of Morph's About page.
pub const ABOUT_URL: &str = "morph://about";

/// A `morph://` address: Morph's own pages and panels (the list is
/// [`morph_core::address::PAGES`], shared with the phone app).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Internal {
    About,
    Bookmarks,
    Downloads,
    History,
    Identity,
    Profiles,
    Relays,
    Search,
    Settings,
}

impl Internal {
    /// The internal page `text` names (`morph://about`, `morph:settings`,
    /// `morph://history/`), if any.
    pub fn parse(text: &str) -> Option<Internal> {
        let page = address::page(text)?.ok()?;
        Some(match page.name {
            "about" => Internal::About,
            "bookmarks" => Internal::Bookmarks,
            "downloads" => Internal::Downloads,
            "history" => Internal::History,
            "identity" => Internal::Identity,
            "profiles" => Internal::Profiles,
            "relays" => Internal::Relays,
            "search" => Internal::Search,
            "settings" => Internal::Settings,
            _ => return None,
        })
    }

    /// Its address.
    pub fn url(self) -> &'static str {
        match self {
            Internal::About => ABOUT_URL,
            Internal::Bookmarks => "morph://bookmarks",
            Internal::Downloads => "morph://downloads",
            Internal::History => "morph://history",
            Internal::Identity => "morph://identity",
            Internal::Profiles => "morph://profiles",
            Internal::Relays => "morph://relays",
            Internal::Search => "morph://search",
            Internal::Settings => SETTINGS_URL,
        }
    }

    /// The settings section it opens (`downloads`, `relays`, `search`), for
    /// the pages that are a part of Settings.
    pub fn settings_section(self) -> Option<&'static str> {
        match self {
            Internal::Downloads => Some("downloads"),
            Internal::Relays => Some("relays"),
            Internal::Search => Some("search"),
            _ => None,
        }
    }

    /// The shell action that opens it.
    pub fn action(self) -> Action {
        match self {
            Internal::About => Action::ShowAbout,
            Internal::Bookmarks => Action::ShowBookmarks,
            Internal::History => Action::ShowHistory,
            Internal::Identity => Action::ShowIdentity,
            Internal::Profiles => Action::ShowProfiles,
            Internal::Search => Action::ShowSearchSettings,
            Internal::Settings | Internal::Downloads | Internal::Relays => Action::ShowSettings,
        }
    }
}

/// The address to keep for a start page typed as `text` (Settings, Open at
/// start): blank stays blank, Morph's own pages (`morph://about`,
/// `morph:settings`) are kept in their usual form, `weirdnet-morph://` links
/// are kept as typed, and anything else follows the address bar's rule
/// ([`omnibox::target_for`]): addresses open, phrases search with `engine`.
pub fn start_address(text: &str, engine: &search::Engine) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Some(internal) = Internal::parse(text) {
        return internal.url().to_string();
    }
    if morph_core::scheme::is_link(text) {
        return text.to_string();
    }
    omnibox::target_for(text, engine).unwrap_or_default()
}

/// Title and explanation for a page that failed to load.
pub fn error_text(error: &LoadError) -> (String, String) {
    // Morph's own addresses: an unknown morph:// page, or a Weirdnet app
    // link this build cannot call ([`Browser::load`] never hands either to
    // the engine).
    if let Some(Err(name)) = address::page(&error.url) {
        return address::unknown_page_text(&name);
    }
    if address::is_app(&error.url) {
        return match (app_call::request(&error.url), AppCallError::decode(&error.message)) {
            (Err(e), _) | (Ok(_), Some(e)) => app_call::error_text(&error.url, &e),
            (Ok(_), None) if error.message.trim().is_empty() => app_call::error_text(&error.url, &AppCallError::NotSupported),
            (Ok(_), None) => {
                ("This app couldn't be opened".into(), format!("{}: {}", app_call::describe(&error.url), sentence(&error.message).trim_end()))
            }
        };
    }
    let host = host(&error.url);
    let site = if host.is_empty() { "This site".to_string() } else { host.to_string() };
    match error.kind {
        ErrorKind::Certificate => (
            "This connection isn't private".into(),
            format!(
                "{site} sent a security certificate Morph cannot trust, so the page was not loaded. {}Check the address for a typo.",
                sentence(&error.message)
            ),
        ),
        ErrorKind::NotFound => (format!("Can't find {site}"), "The address may be misspelled, or the site no longer exists.".into()),
        ErrorKind::Unreachable => {
            (format!("Can't reach {site}"), "The site refused the connection or the network is down. Check your connection and try again.".into())
        }
        ErrorKind::TimedOut => (format!("{site} took too long to respond"), "The site may be busy or your connection slow. Try again.".into()),
        ErrorKind::Other => (
            "This page couldn't load".into(),
            if error.message.trim().is_empty() { "Try again in a moment.".into() } else { error.message.trim().to_string() },
        ),
    }
}

/// The host of `url` (`https://user@example.com:8080/x` gives
/// `example.com`), or empty.
fn host(url: &str) -> &str {
    let Some((_, rest)) = url.split_once("://") else {
        return "";
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if authority.starts_with('[') {
        return authority.split_inclusive(']').next().unwrap_or(authority);
    }
    authority.split(':').next().unwrap_or("")
}

/// `text` as a sentence followed by a space, or nothing when empty.
fn sentence(text: &str) -> String {
    let trimmed = text.trim();
    match trimmed.chars().last() {
        None => String::new(),
        Some('.' | '!' | '?') => format!("{trimmed} "),
        Some(_) => format!("{trimmed}. "),
    }
}

/// The find bar's state. It belongs to the browser, not a tab: switching
/// tabs searches the new tab for the same text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindBar {
    pub query: String,
    /// Matches in the active page; `None` until the page answers.
    pub matches: Option<u32>,
}

/// One entry of the tab strip, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabLabel {
    pub id: TabId,
    pub label: String,
    pub active: bool,
    pub loading: bool,
    /// The site's icon (PNG), once the page reported one.
    pub icon: Option<std::sync::Arc<Vec<u8>>>,
    /// Morph's own page (About, Settings), drawn with Morph's mark.
    pub own: bool,
}

/// `scheme://host[:port]` of `url`, the key icons are kept under.
pub fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().filter(|h| !h.is_empty())?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    Some(format!("{}://{}", scheme.to_ascii_lowercase(), host.to_ascii_lowercase()))
}

/// The toolbar's state for the active tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Toolbar {
    pub can_back: bool,
    pub can_forward: bool,
    pub loading: bool,
    /// Something covers the page (crash, hang or error page).
    pub covered: bool,
    pub popups: usize,
}

/// What a [`Browser::poll`] found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Polled {
    /// Something visible changed; the shell should redraw.
    pub changed: bool,
    /// Shell shortcuts pressed while a page had focus.
    pub shortcuts: Vec<String>,
}

pub struct Browser {
    backend: Box<dyn Backend>,
    data_dir: Option<PathBuf>,
    tabs: TabSet,
    pages: HashMap<TabId, Page>,
    by_view: HashMap<ViewId, TabId>,
    address: TextField,
    address_focused: bool,
    page_area: Rect,
    shown: bool,
    history: Option<HistorySink>,
    log_events: bool,
    find: Option<FindBar>,
    /// The search engine new tabs use (profile setting).
    default_engine: &'static search::Engine,
    /// Tabs that picked another engine with the selector.
    tab_engines: HashMap<TabId, &'static search::Engine>,
    /// The last tab was closed: the shell should quit.
    quit: bool,
    /// Site icons by origin, for every tab of the site.
    icons: HashMap<String, std::sync::Arc<Vec<u8>>>,
    /// A page asked for files and waits for the shell's dialog.
    file_request: Option<(TabId, bool)>,
    /// The shell took the file request (its dialog is open).
    file_asked: bool,
    downloads: downloads::Downloads,
    app_caller: Arc<dyn AppCaller>,
    /// App calls under way, by tab.
    app_loads: HashMap<TabId, AppLoad>,
    /// The app address each app page (`data:` address, by its token) shows.
    app_pages: HashMap<String, String>,
    /// Links to open in windows of their own (Shift+click, the link
    /// menu), until the shell takes them.
    new_windows: Vec<String>,
    /// A right click on a link in a page the shell draws: the link and
    /// where (page coordinates), until the shell takes it for its menu.
    link_menu: Option<(String, f64, f64)>,
    /// Refused resumes' token searches and re-triggers.
    auth: download_auth::AuthWork,
    /// Added to tab ids in the event log, so each window's tabs read apart
    /// (the first window's tab 1 is `tab=1`, the second's `tab=1001`).
    log_base: u64,
    /// The pages were closed with their window; nothing is shown or pumped.
    closed: bool,
}

/// Whether `url` is an address Morph opens itself (`morph:`,
/// `weirdnet-<app>:`), never the engine.
fn is_own_address(url: &str) -> bool {
    address::is_morph(url) || address::is_app(url)
}

impl Browser {
    pub fn new(backend: Box<dyn Backend>, config: Config) -> morph_webview::Result<Self> {
        let mut browser = Self {
            backend,
            data_dir: config.data_dir,
            tabs: TabSet::new(),
            pages: HashMap::new(),
            by_view: HashMap::new(),
            address: TextField::new(),
            address_focused: false,
            page_area: Rect::default(),
            shown: true,
            history: config.history,
            log_events: config.log_events,
            find: None,
            default_engine: search::engine(&config.search_engine),
            tab_engines: HashMap::new(),
            quit: false,
            icons: HashMap::new(),
            file_request: None,
            file_asked: false,
            downloads: downloads::Downloads::default(),
            app_caller: config.app_caller.clone().unwrap_or_else(|| Arc::new(app_call::NotYet)),
            app_loads: HashMap::new(),
            app_pages: HashMap::new(),
            new_windows: Vec::new(),
            link_menu: None,
            auth: download_auth::AuthWork::default(),
            log_base: 0,
            closed: false,
        };
        let first = browser.tabs.active_id();
        let url = config.start_url.trim().to_string();
        // Morph's own pages are drawn by the shell, never loaded or searched.
        let session = config.start_session.filter(|_| !url.is_empty() && !is_own_address(&url));
        browser.attach_with(first, &url, session)?;
        browser.after_switch();
        browser.open_own(&url);
        Ok(browser)
    }

    /// Tell the backend which keys the shell wants while a page has focus.
    pub fn set_shortcuts(&mut self, keystrokes: &[String]) {
        self.backend.set_shortcuts(keystrokes);
    }

    fn attach(&mut self, id: TabId, url: &str) -> morph_webview::Result<()> {
        self.attach_with(id, url, None)
    }

    /// [`Self::attach`], starting from `session` (engine history) when given.
    fn attach_with(&mut self, id: TabId, url: &str, session: Option<Vec<u8>>) -> morph_webview::Result<()> {
        let target =
            if url.is_empty() || is_own_address(url) { String::new() } else { omnibox::target_for(url, self.default_engine).unwrap_or_default() };
        let active = self.tabs.active_id() == id;
        let view = self.backend.create(Spec {
            url: target.clone(),
            data_dir: self.data_dir.clone(),
            bounds: self.page_area,
            visible: active && self.shown,
            session: session.filter(|_| !target.is_empty()),
            background: false,
        })?;
        self.by_view.insert(view.id(), id);
        let blank = target.is_empty();
        let _ = self.tabs.update(id, target, String::new());
        let mut page = Page::new(view);
        if blank {
            // Nothing to show: the start screen covers the empty web view.
            page.cover = Some(Cover::Start);
            let _ = page.view.set_visible(false);
        }
        self.pages.insert(id, page);
        Ok(())
    }

    /// Show only the active tab's page, and refresh the address bar.
    fn after_switch(&mut self) {
        self.close_menus();
        let active = self.tabs.active_id();
        for (id, page) in &self.pages {
            let visible = *id == active && self.shown && page.cover.is_none();
            if visible {
                let _ = page.view.set_bounds(self.page_area);
            }
            let _ = page.view.set_visible(visible);
        }
        if let Some(find) = self.find.as_mut() {
            find.matches = None;
            for (id, page) in &self.pages {
                if *id == active && !find.query.is_empty() {
                    let _ = page.view.find(&find.query);
                } else {
                    let _ = page.view.find_clear();
                }
            }
        }
        let url = self.tabs.active().url.clone();
        self.address.set_text_selected(&url);
        if url.is_empty() {
            self.focus_address();
        } else {
            self.address_focused = false;
        }
    }

    fn active_page(&self) -> &Page {
        self.pages.get(&self.tabs.active_id()).expect("every tab has a page")
    }

    pub fn tabs(&self) -> Vec<TabLabel> {
        let active = self.tabs.active_id();
        self.tabs
            .tabs()
            .iter()
            .map(|tab| {
                let label = if !tab.title.is_empty() {
                    tab.title.clone()
                } else if !tab.url.is_empty() {
                    tab.url.clone()
                } else {
                    "New tab".to_string()
                };
                let page = self.pages.get(&tab.id);
                let loading = page.is_some_and(|p| p.loading);
                let own = page.is_some_and(|p| p.cover.as_ref().is_some_and(Cover::is_own));
                let icon = if own { None } else { origin(&tab.url).and_then(|o| self.icons.get(&o).cloned()) };
                TabLabel { id: tab.id, label, active: tab.id == active, loading, icon, own }
            })
            .collect()
    }

    pub fn active_url(&self) -> &str {
        &self.tabs.active().url
    }

    pub fn active_title(&self) -> &str {
        &self.tabs.active().title
    }

    pub fn toolbar(&self) -> Toolbar {
        let page = self.active_page();
        Toolbar { can_back: page.can_back, can_forward: page.can_forward, loading: page.loading, covered: page.cover.is_some(), popups: page.popups }
    }

    /// What covers the active page, if anything.
    pub fn cover(&self) -> Option<&Cover> {
        self.active_page().cover.as_ref()
    }

    /// The find bar, when open.
    pub fn find(&self) -> Option<&FindBar> {
        self.find.as_ref()
    }

    /// Open the find bar (Ctrl+F) and take keyboard focus from the page for
    /// it. Opening it again keeps its text.
    pub fn open_find(&mut self) {
        if self.address_focused {
            self.blur_address();
        }
        if self.find.is_none() {
            self.find = Some(FindBar::default());
        }
        self.backend.focus_host();
    }

    /// The find bar's text changed: search the active page for it.
    pub fn set_find_query(&mut self, query: &str) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        if find.query == query {
            return;
        }
        find.query = query.to_string();
        find.matches = None;
        let view = &self.pages.get(&self.tabs.active_id()).expect("every tab has a page").view;
        let _ = if query.is_empty() { view.find_clear() } else { view.find(query) };
    }

    /// Next match (Enter, F3) or previous (Shift+Enter, Shift+F3).
    pub fn find_next(&mut self, forward: bool) {
        if self.find.as_ref().is_some_and(|f| !f.query.is_empty()) {
            let _ = self.active_page().view.find_next(forward);
        }
    }

    /// Close the find bar (Escape), drop the highlights and give the page
    /// keyboard focus back.
    pub fn close_find(&mut self) {
        if self.find.take().is_some() {
            let _ = self.active_page().view.find_clear();
            self.focus_page();
        }
    }

    pub fn address(&self) -> &TextField {
        &self.address
    }

    pub fn address_mut(&mut self) -> &mut TextField {
        &mut self.address
    }

    pub fn address_focused(&self) -> bool {
        self.address_focused
    }

    /// Focus the address bar with its text selected, taking keyboard focus
    /// back from the page.
    pub fn focus_address(&mut self) {
        self.address_focused = true;
        self.address.select_all();
        self.backend.focus_host();
    }

    /// Take keyboard focus back from the page for the shell's own UI (the
    /// user clicked the tab strip or toolbar).
    pub fn focus_shell(&self) {
        self.backend.focus_host();
    }

    /// The address bar lost focus (the user clicked the page or elsewhere):
    /// put the active tab's address back.
    pub fn blur_address(&mut self) {
        self.address_focused = false;
        let url = self.tabs.active().url.clone();
        self.address.set_text_selected(&url);
    }

    /// Open what the address bar holds in the active tab (Enter). Phrases
    /// become a web search. A `morph://` panel address is handed back for
    /// the shell to open (morph://about shows here).
    ///
    /// `morph:` and `weirdnet-<app>:` addresses never search: an unknown
    /// `morph://` page and a Weirdnet app link this build cannot call get
    /// Morph's error page ([`Self::open_own`]).
    pub fn submit_address(&mut self) -> Option<Internal> {
        if let Some(internal) = Internal::parse(self.address.text()) {
            match internal {
                Internal::About => self.show_own(Cover::About, true),
                Internal::Settings => self.show_own(Cover::Settings, true),
                _ => {
                    self.blur_address();
                    return Some(internal);
                }
            }
            return None;
        }
        let target = omnibox::target_for(self.address.text(), self.tab_engine())?;
        if is_own_address(&target) {
            self.open_own(&target);
            return None;
        }
        self.load(&target);
        None
    }

    /// The search engine the active tab sends phrases to.
    pub fn tab_engine(&self) -> &'static search::Engine {
        self.tab_engines.get(&self.tabs.active_id()).copied().unwrap_or(self.default_engine)
    }

    /// The engine new tabs use (the profile's setting). Tabs that picked
    /// one keep theirs.
    pub fn set_default_engine(&mut self, id: &str) {
        self.default_engine = search::engine(id);
    }

    pub fn default_engine(&self) -> &'static search::Engine {
        self.default_engine
    }

    /// The active tab sends phrases to `id` from now on (the selector left
    /// of the address).
    pub fn set_tab_engine(&mut self, id: &str) {
        let engine = search::engine(id);
        self.tab_engines.insert(self.tabs.active_id(), engine);
    }

    /// A page waits for files: whether it takes several. The shell shows
    /// the dialog and answers with [`Self::choose_files`].
    pub fn take_file_request(&mut self) -> Option<bool> {
        let (_, multiple) = *self.file_request.as_ref()?;
        (!std::mem::replace(&mut self.file_asked, true)).then_some(multiple)
    }

    /// The downloads, newest last.
    pub fn downloads(&self) -> &[downloads::DownloadItem] {
        self.downloads.items()
    }

    /// A download waits for its save dialog: its id, suggested name and
    /// the folder to open in. The shell answers with [`Self::save_download`].
    pub fn take_download_ask(&mut self) -> Option<(u64, String, PathBuf)> {
        self.downloads.take_ask()
    }

    /// Where download `id` goes (`None`: the user cancelled; it stops).
    pub fn save_download(&mut self, id: u64, path: Option<PathBuf>) {
        let (stop, outcome) = self.downloads.answer(id, path);
        if stop {
            self.backend.cancel_download(self.downloads.engine_id(id));
        }
        self.log_download(&outcome);
    }

    /// Forget the downloads that ended (the bar's close button, the
    /// panel's Clear).
    pub fn clear_downloads(&mut self) {
        self.downloads.clear_done();
    }

    /// Pause download `id`, keeping its bytes so far.
    pub fn pause_download(&mut self, id: u64) {
        if self.downloads.pause(id) {
            self.backend.cancel_download(self.downloads.engine_id(id));
        }
    }

    /// Resume paused download `id` (from where it stopped when the server
    /// can, else from the start).
    pub fn resume_download(&mut self, id: u64) {
        let outcome = self.downloads.resume(id);
        self.log_download(&outcome);
        self.ask_cookies(id);
    }

    /// Keep download sessions (cookies, Referer, ...) in `store`, the
    /// profile's encrypted library, so resumes work behind a login.
    pub fn set_download_sessions(&mut self, store: Arc<dyn morph_core::download_sessions::SessionStore>) {
        self.downloads.set_sessions(store);
    }

    /// A resume or retry of `id` waits for fresh cookies: ask the engine
    /// (it goes with the saved session when the engine can't say).
    fn ask_cookies(&mut self, id: u64) {
        if !self.downloads.awaiting_cookies(id) {
            return;
        }
        let url = self.downloads.fetch_url(id).unwrap_or_default();
        if !self.backend.request_cookies(self.downloads.engine_id(id), &url) {
            self.downloads.cookies_answered(id, None);
        }
    }

    /// Cancel download `id` and delete its bytes.
    pub fn cancel_download(&mut self, id: u64) {
        if self.downloads.cancel(id) {
            self.backend.cancel_download(self.downloads.engine_id(id));
        }
    }

    /// Start failed or cancelled download `id` again.
    pub fn retry_download(&mut self, id: u64) {
        self.downloads.retry(id);
        self.ask_cookies(id);
    }

    /// A click on download `id`: the saved file to open now, or (while it
    /// runs) open it when done; a second click takes that back.
    pub fn activate_download(&mut self, id: u64) -> Option<PathBuf> {
        self.downloads.activate(id)
    }

    /// A download clicked while it ran is saved: open this file now.
    pub fn take_download_open(&mut self) -> Option<PathBuf> {
        self.downloads.take_open()
    }

    /// How many downloads run and their combined progress, for the
    /// toolbar's downloads button.
    pub fn downloads_progress(&self) -> Option<(usize, Option<f32>)> {
        self.downloads.progress()
    }

    fn log_download(&self, outcome: &downloads::Outcome) {
        if !self.log_events {
            return;
        }
        match outcome {
            downloads::Outcome::Saved(path) => eprintln!("morph-event download saved {}", path.display()),
            downloads::Outcome::Failed(why) => eprintln!("morph-event download failed {why}"),
            downloads::Outcome::Nothing => {}
        }
    }

    /// Answer the page that asked for files (`None`: cancelled).
    pub fn choose_files(&mut self, paths: Option<Vec<PathBuf>>) {
        if let Some((tab, _)) = self.file_request.take()
            && let Some(page) = self.pages.get(&tab)
        {
            page.view.choose_files(paths);
        }
    }

    /// Forget the profile's cookies, cache and site storage, and the icons.
    pub fn clear_site_data(&mut self) {
        self.icons.clear();
        let _ = self.active_page().view.clear_data();
    }

    /// Where downloads go (`None`: the Downloads folder).
    pub fn set_downloads_dir(&mut self, dir: Option<PathBuf>) {
        self.downloads.set_folder(dir.clone());
        self.backend.set_downloads_dir(dir);
    }

    /// The last tab was closed since the last call: time to quit.
    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
    }

    /// Show the About page: in the active tab if it is blank or `here`,
    /// else in a new tab.
    pub fn show_about(&mut self, here: bool) {
        self.show_own(Cover::About, here);
    }

    /// Show the preferences page (morph://settings), the same way.
    pub fn show_settings(&mut self, here: bool) {
        self.show_own(Cover::Settings, here);
    }

    /// Show one of Morph's own pages (`cover` is About or Settings). An
    /// open one is selected instead of opening another.
    fn show_own(&mut self, cover: Cover, here: bool) {
        let (url, title) = match cover {
            Cover::Settings => (SETTINGS_URL, "Settings"),
            _ => (ABOUT_URL, "About Morph"),
        };
        if let Some(open) = self.pages.iter().find(|(_, p)| p.cover.as_ref() == Some(&cover)).map(|(id, _)| *id) {
            self.select(open);
            return;
        }
        if !here && !self.tabs.active().url.is_empty() && !self.active_page().cover.as_ref().is_some_and(Cover::is_own) {
            self.new_tab("");
        }
        self.close_menus();
        let id = self.tabs.active_id();
        let shown = Some(self.tabs.active().url.clone()).filter(|u| !u.is_empty() && Internal::parse(u).is_none());
        let shown_title = shown.as_ref().map(|_| self.tabs.active().title.clone());
        let _ = self.tabs.update(id, url.to_string(), title.to_string());
        self.address_focused = false;
        self.address.set_text_selected(url);
        if let Some(page) = self.pages.get_mut(&id) {
            if shown.is_some() {
                page.shown_url = shown;
                page.shown_title = shown_title;
            }
            page.loading = false;
        }
        self.set_cover(id, Some(cover));
    }

    /// Open one of Morph's own addresses in the active tab: About and
    /// Settings are drawn here, an unknown `morph://` page gets the page
    /// listing the real ones, and a `weirdnet-<app>://` address is called
    /// through the [`AppCaller`] in the background ([`Self::start_app_call`]). Panel pages (`morph://history`) are the shell's to open
    /// ([`Self::submit_address`] hands them back) and are left alone.
    /// Returns whether `url` was one; none of them reaches the engine.
    fn open_own(&mut self, url: &str) -> bool {
        let url = url.trim();
        match address::page(url) {
            Some(Ok(_)) => {
                match Internal::parse(url) {
                    Some(Internal::About) => self.show_own(Cover::About, true),
                    Some(Internal::Settings) => self.show_own(Cover::Settings, true),
                    _ => {}
                }
                true
            }
            Some(Err(_)) => {
                self.show_notice(url, String::new());
                true
            }
            None if address::is_app(url) => {
                self.start_app_call(self.tabs.active_id(), url);
                true
            }
            None => false,
        }
    }

    fn log_app(&self, url: &str, what: &str) {
        if self.log_events {
            // Never the query or fragment: they may carry tokens.
            eprintln!("morph-event app-call {} {what}", app_call::describe(url));
        }
    }

    /// Call the app `url` names for tab `id`, off the UI thread: the tab
    /// shows `url` and loads while the call runs, then the app's page
    /// ([`Self::poll`] loads it) or Morph's error page for why it failed.
    /// An address that is not a valid app link fails at once. Never a
    /// search, never the engine with `url` itself.
    fn start_app_call(&mut self, id: TabId, url: &str) {
        self.app_loads.remove(&id);
        if let Err(e) = app_call::request(url) {
            self.log_app(url, &format!("failed {}", e.code()));
            self.show_notice_on(id, url, e.encode());
            return;
        }
        let Some(current) = self.tabs.tab(id).cloned() else {
            return;
        };
        let (tx, events) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (caller, target, stopping) = (self.app_caller.clone(), url.to_string(), stop.clone());
        let spawned = std::thread::Builder::new().name("morph-app-call".into()).spawn(move || {
            let progress = tx.clone();
            let result = app_call::fetch(&target, caller.as_ref(), &stopping, &mut |n| {
                let _ = progress.send(AppLoadEvent::Progress(n));
            });
            let _ = tx.send(AppLoadEvent::Done(result));
        });
        if spawned.is_err() {
            self.show_notice_on(id, url, AppCallError::Unreachable("Morph could not start the call".into()).encode());
            return;
        }
        if self.tabs.active_id() == id {
            self.close_menus();
            self.address_focused = false;
            self.address.set_text_selected(url);
        }
        let _ = self.tabs.update(id, url.to_string(), current.title.clone());
        if let Some(page) = self.pages.get_mut(&id) {
            page.loading = true;
            page.finished = false;
        }
        self.log_app(url, "started");
        self.app_loads.insert(id, AppLoad { url: url.to_string(), before: (current.url, current.title), events, stop, received: 0 });
    }

    /// Stop tab `id`'s app call, if one runs, and put its address back.
    /// Returns whether one ran.
    fn stop_app_call(&mut self, id: TabId) -> bool {
        let Some(load) = self.app_loads.remove(&id) else {
            return false;
        };
        self.log_app(&load.url, "stopped");
        let (url, title) = load.before.clone();
        let _ = self.tabs.update(id, url.clone(), title);
        if let Some(page) = self.pages.get_mut(&id) {
            page.loading = false;
        }
        if self.tabs.active_id() == id && !self.address_focused {
            self.address.set_text_selected(&url);
        }
        true
    }

    /// Bytes of the active tab's app answer read so far, while its call
    /// runs (for a progress line).
    pub fn app_call_progress(&self) -> Option<u64> {
        self.app_loads.get(&self.tabs.active_id()).map(|l| l.received)
    }

    /// Pick up finished app calls: load each app's page, or show why it
    /// failed. Returns whether anything changed.
    fn poll_app_calls(&mut self) -> bool {
        let mut changed = false;
        let ids: Vec<TabId> = self.app_loads.keys().copied().collect();
        for id in ids {
            let Some(load) = self.app_loads.get_mut(&id) else { continue };
            let mut done = None;
            loop {
                match load.events.try_recv() {
                    Ok(AppLoadEvent::Progress(n)) => {
                        load.received = n;
                        changed = true;
                    }
                    Ok(AppLoadEvent::Done(result)) => {
                        done = Some(result);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        done = Some(Err(AppCallError::Unreachable("the call ended without an answer".into())));
                        break;
                    }
                }
            }
            if let Some(result) = done
                && let Some(load) = self.app_loads.remove(&id)
            {
                changed = true;
                self.finish_app_call(id, load, result);
            }
        }
        changed
    }

    fn finish_app_call(&mut self, id: TabId, load: AppLoad, result: Result<AppPage, AppCallError>) {
        let result = result.and_then(|page| match DATA_URL_MAX {
            Some(max) if page.data_url.len() > max => Err(AppCallError::TooLarge),
            _ => Ok(page),
        });
        let page = match result {
            Ok(page) => page,
            Err(e) => {
                self.log_app(&load.url, &format!("failed {}", e.code()));
                let (url, title) = load.before.clone();
                let _ = self.tabs.update(id, url, title);
                self.show_notice_on(id, &load.url, e.encode());
                return;
            }
        };
        self.log_app(&load.url, &format!("ok status={} kind={} bytes={}", page.status, page.kind, page.bytes));
        if let Some(token) = app_call::page_token(&page.data_url) {
            self.app_pages.insert(token.to_string(), load.url.clone());
        }
        let active = self.tabs.active_id() == id;
        let _ = self.tabs.update(id, load.url.clone(), page.title.clone());
        if active && !self.address_focused {
            self.address.set_text_selected(&load.url);
        }
        let covered = self.pages.get(&id).and_then(|p| p.cover.clone());
        if covered.as_ref().is_some_and(|c| !matches!(c, Cover::About | Cover::Settings)) {
            self.set_cover(id, None);
        }
        if let Some(p) = self.pages.get_mut(&id) {
            p.finished = false;
            p.shown_url = None;
            p.shown_title = None;
            let _ = p.view.load_url(&page.data_url);
            if active && p.cover.is_none() {
                let _ = p.view.focus();
            }
        }
    }

    /// The address to show for one the engine reports: an app page's
    /// `data:` address shows as the app address it came from.
    fn shown_address(&self, url: String) -> String {
        match app_call::page_token(&url).and_then(|t| self.app_pages.get(t)) {
            Some(app) => app.clone(),
            None => url,
        }
    }

    /// Cover the active tab with Morph's error page for `url` (see
    /// [`error_text`]), as a failed load does: Back returns to the page
    /// under it, Reload tries `url` again.
    fn show_notice(&mut self, url: &str, message: String) {
        self.show_notice_on(self.tabs.active_id(), url, message);
    }

    /// [`Self::show_notice`] on tab `id`.
    fn show_notice_on(&mut self, id: TabId, url: &str, message: String) {
        let active = self.tabs.active_id() == id;
        if active {
            self.close_menus();
        }
        let Some(current) = self.tabs.tab(id).cloned() else {
            return;
        };
        let error = LoadError { url: url.to_string(), kind: ErrorKind::Other, message };
        let title = error_text(&error).0;
        if let Some(page) = self.pages.get_mut(&id) {
            page.loading = false;
            page.finished = false;
            let under = matches!(page.cover, None | Some(Cover::Crashed | Cover::Unresponsive));
            if under && !current.url.is_empty() {
                page.shown_url = Some(current.url.clone());
                page.shown_title = Some(current.title.clone());
            }
        }
        let _ = self.tabs.update(id, url.to_string(), title);
        if active {
            self.address_focused = false;
            self.address.set_text_selected(url);
        }
        self.set_cover(id, Some(Cover::Failed(error)));
    }

    /// Load `url` (already an address) in the active tab and focus the page.
    /// Morph's own addresses never reach the engine ([`Self::open_own`]).
    pub fn load(&mut self, url: &str) {
        if is_own_address(url) {
            self.open_own(url);
            return;
        }
        self.close_menus();
        let id = self.tabs.active_id();
        self.stop_app_call(id);
        let title = self.tabs.active().title.clone();
        let _ = self.tabs.update(id, url.to_string(), title);
        self.address_focused = false;
        self.address.set_text_selected(url);
        let shown = self.shown;
        let area = self.page_area;
        if let Some(page) = self.pages.get_mut(&id) {
            page.finished = false;
            if page.cover.take().is_some() {
                let _ = page.view.set_bounds(area);
                let _ = page.view.set_visible(shown);
            }
            let _ = page.view.load_url(url);
            let _ = page.view.focus();
        }
    }

    /// Open a tab after the active one and select it. An empty `url` opens a
    /// blank tab with the address bar focused.
    pub fn new_tab(&mut self, url: &str) {
        let id = self.tabs.open(String::new());
        if self.attach(id, url).is_err() {
            let _ = self.tabs.close(id);
            self.after_switch();
            return;
        }
        self.after_switch();
        if is_own_address(url) {
            self.open_own(url);
        }
    }

    /// Close tab `id`. Closing the last tab asks the shell to quit
    /// ([`Self::take_quit`]) and leaves the tab as it is.
    pub fn close_tab(&mut self, id: TabId) {
        if self.tabs.len() == 1 && self.tabs.tab(id).is_some() {
            self.quit = true;
            return;
        }
        self.tab_engines.remove(&id);
        self.app_loads.remove(&id);
        let Ok(active) = self.tabs.close(id) else {
            return;
        };
        if let Some(page) = self.pages.remove(&id) {
            self.by_view.remove(&page.view.id());
        }
        if !self.pages.contains_key(&active) {
            // Closing the only tab left a fresh blank one.
            let _ = self.attach(active, "");
        }
        self.after_switch();
    }

    /// Open a copy of tab `id` (its address) after the active tab.
    pub fn duplicate_tab(&mut self, id: TabId) {
        if let Some(url) = self.tabs.tab(id).map(|t| t.url.clone()).filter(|u| !u.is_empty() && Internal::parse(u).is_none()) {
            let engine = self.tab_engines.get(&id).copied();
            self.new_tab(&url);
            if let Some(engine) = engine {
                self.tab_engines.insert(self.tabs.active_id(), engine);
            }
        }
    }

    /// Take tab `id` out of this window to open in another, with its
    /// history when the engine can hand it over. `None` (and nothing
    /// changes) when it is the only tab: its window is what would move.
    pub fn detach_tab(&mut self, id: TabId) -> Option<DetachedTab> {
        if self.tabs.len() < 2 {
            return None;
        }
        let tab = self.tabs.tab(id)?.clone();
        let page = self.pages.get(&id)?;
        // Morph's own pages, app pages and error pages reopen by address.
        let session = (page.cover.is_none() && !tab.url.is_empty() && !is_own_address(&tab.url)).then(|| page.view.session_state()).flatten();
        let engine = self.tab_engines.get(&id).map(|e| e.id.to_string());
        self.close_tab(id);
        Some(DetachedTab { url: tab.url, title: tab.title, session, engine })
    }

    /// Open `more` (a saved window's other tabs) after the first, in order,
    /// then select the tab at `active`.
    pub fn restore_tabs(&mut self, more: &[String], active: usize) {
        for url in more {
            self.new_tab(url);
        }
        self.select_index(active);
    }

    /// Number this browser's tabs from `base` in the event log (one window
    /// of several).
    pub fn set_log_base(&mut self, base: u64) {
        self.log_base = base;
    }

    /// Close every page at once, while the window they live in still
    /// stands (the engine's windows sit inside it and must go first).
    /// The browser shows and pumps nothing afterwards.
    pub fn close_pages(&mut self) {
        self.closed = true;
        self.by_view.clear();
        self.pages.clear();
        let _ = self.backend.poll();
    }

    /// Whether [`Browser::close_pages`] ran.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Links the user asked to open in new windows since the last call.
    pub fn take_new_windows(&mut self) -> Vec<String> {
        std::mem::take(&mut self.new_windows)
    }

    /// A link a page asked the shell's link menu for (pages the shell
    /// draws): the link and where, in page coordinates.
    pub fn take_link_menu(&mut self) -> Option<(String, f64, f64)> {
        self.link_menu.take()
    }

    /// The link menu's choice: open `url` in a new tab here.
    pub fn open_link_in_tab(&mut self, url: &str) {
        self.new_tab(url);
    }

    /// Every tab's address, left to right, and the active one's index, for
    /// the saved session.
    pub fn tab_urls(&self) -> (Vec<String>, usize) {
        let urls = self.tabs.tabs().iter().map(|t| t.url.clone()).collect();
        (urls, self.tabs.index_of(self.tabs.active_id()).unwrap_or(0))
    }

    /// Close every tab but `id`.
    pub fn close_other_tabs(&mut self, id: TabId) {
        let others: Vec<TabId> = self.tabs.tabs().iter().map(|t| t.id).filter(|&t| t != id).collect();
        for other in others {
            self.close_tab(other);
        }
        self.select(id);
    }

    /// Reload tab `id` (selecting it).
    pub fn reload_tab(&mut self, id: TabId) {
        self.select(id);
        self.reload();
    }

    pub fn select(&mut self, id: TabId) {
        if self.tabs.switch(id).is_ok() {
            self.after_switch();
        }
    }

    /// Select by 0-based position; out of range does nothing.
    pub fn select_index(&mut self, index: usize) {
        if let Some(tab) = self.tabs.tabs().get(index) {
            self.select(tab.id);
        }
    }

    /// Run a named action. Returns whether this component handled it.
    pub fn dispatch(&mut self, action: Action) -> bool {
        match action {
            Action::NewTab => self.new_tab(""),
            Action::CloseTab => self.close_tab(self.tabs.active_id()),
            Action::NextTab => {
                self.tabs.cycle_next();
                self.after_switch();
            }
            Action::PrevTab => {
                self.tabs.cycle_prev();
                self.after_switch();
            }
            Action::SelectTab(i) => self.select_index(i),
            Action::SelectLastTab => self.select_index(self.tabs.len() - 1),
            Action::FocusAddress => self.focus_address(),
            Action::Reload => self.reload(),
            Action::Stop => {
                if !self.stop_app_call(self.tabs.active_id()) {
                    let _ = self.active_page().view.stop();
                }
            }
            Action::Back => self.back(),
            Action::Forward => {
                let _ = self.active_page().view.forward();
            }
            Action::Find => self.open_find(),
            Action::FindNext | Action::FindPrevious if self.find.is_none() => self.open_find(),
            Action::FindNext => self.find_next(true),
            Action::FindPrevious => self.find_next(false),
            Action::ShowAbout => self.show_about(false),
            Action::ShowSettings | Action::ShowSearchSettings => self.show_settings(false),
            Action::ToggleFullscreen
            // The shell opens windows.
            | Action::NewWindow
            | Action::Quit
            | Action::Bookmark
            | Action::ShowBookmarks
            | Action::ShowHistory
            | Action::ShowProfiles
            | Action::ShowIdentity
            | Action::ToggleCompanions
            // The shell's downloads panel.
            | Action::ShowDownloads => {
                return false;
            }
        }
        true
    }

    /// Reload the active page. After a crash or an error page this brings
    /// the page back; a page that stopped answering is ended first.
    pub fn reload(&mut self) {
        if let Some(Cover::Failed(error)) = &self.active_page().cover
            && is_own_address(&error.url)
        {
            let url = error.url.clone();
            self.open_own(&url);
            return;
        }
        // An app's page (or a call under way) is called again, not reloaded
        // from its `data:` copy.
        if self.active_page().cover.is_none() && address::is_app(self.active_url()) {
            let url = self.active_url().to_string();
            self.start_app_call(self.tabs.active_id(), &url);
            return;
        }
        let shown = self.shown;
        let area = self.page_area;
        let id = self.tabs.active_id();
        if let Some(page) = self.pages.get_mut(&id) {
            if page.cover.as_ref().is_some_and(Cover::is_own) {
                return;
            }
            let cover = page.cover.take();
            match &cover {
                Some(Cover::Unresponsive) => {
                    page.terminating = true;
                    let _ = page.view.terminate();
                }
                Some(_) => {}
                None => {
                    let _ = page.view.reload();
                    return;
                }
            }
            page.waited = false;
            let _ = page.view.set_bounds(area);
            let _ = page.view.set_visible(shown);
            // A failed load never became the page, so reloading would show
            // the one before it: load the failed address again instead.
            let _ = match cover {
                Some(Cover::Failed(error)) => page.view.load_url(&error.url),
                _ => page.view.reload(),
            };
        }
    }

    /// Go back. From an error page that means the page under it: the failed
    /// load never became a history entry, so the engine's Back would skip
    /// past it.
    pub fn back(&mut self) {
        let id = self.tabs.active_id();
        if self.stop_app_call(id) {
            // Back during a call: the call is dropped, the page stays.
            return;
        }
        let page = self.pages.get_mut(&id).expect("every tab has a page");
        if matches!(page.cover, Some(Cover::Failed(_) | Cover::About | Cover::Settings)) {
            let url = page.shown_url.take().unwrap_or_default();
            // Morph's own error pages (an unknown morph:// page, an app
            // link) keep the title of the page under them, as its pages do.
            let notice = matches!(&page.cover, Some(Cover::Failed(e)) if is_own_address(&e.url));
            let own = notice || page.cover.as_ref().is_some_and(Cover::is_own);
            let shown_title = page.shown_title.take();
            // Nothing under it (a blank tab): back to the start screen.
            self.set_cover(id, url.is_empty().then_some(Cover::Start));
            let title = match shown_title {
                Some(title) if own => title,
                _ if own => String::new(),
                _ => self.tabs.active().title.clone(),
            };
            let _ = self.tabs.update(id, url.clone(), title);
            if !self.address_focused {
                self.address.set_text_selected(&url);
            }
            let _ = self.active_page().view.focus();
            return;
        }
        let _ = page.view.back();
    }

    /// Keep waiting for a page that stopped answering: show it again and do
    /// not cover it until it has answered.
    pub fn wait(&mut self) {
        let shown = self.shown;
        let area = self.page_area;
        if let Some(page) = self.pages.get_mut(&self.tabs.active_id())
            && page.cover == Some(Cover::Unresponsive)
        {
            page.cover = None;
            page.waited = true;
            let _ = page.view.set_bounds(area);
            let _ = page.view.set_visible(shown);
        }
    }

    /// Cover or uncover tab `tab`'s page, hiding or showing its web view.
    /// Covering the active page gives the keyboard back to the shell, since a
    /// hidden page would swallow it.
    fn set_cover(&mut self, tab: TabId, cover: Option<Cover>) {
        let active = self.tabs.active_id() == tab;
        if active && cover.is_some() {
            self.backend.focus_host();
        }
        let visible = active && self.shown && cover.is_none();
        let area = self.page_area;
        if let Some(page) = self.pages.get_mut(&tab) {
            if cover.is_some() && page.menu.take().is_some() {
                page.view.option_menu(MenuChoice::Cancel);
            }
            let was = page.cover.is_some();
            page.cover = cover;
            if was != page.cover.is_some() {
                if visible {
                    let _ = page.view.set_bounds(area);
                }
                let _ = page.view.set_visible(visible);
            }
        }
    }

    /// Where the page goes, in window coordinates. Called on every layout.
    pub fn set_page_area(&mut self, area: Rect) {
        if area == self.page_area {
            return;
        }
        self.page_area = area;
        let _ = self.active_page().view.set_bounds(area);
    }

    pub fn page_area(&self) -> Rect {
        self.page_area
    }

    /// Show or hide the whole component (its slot was hidden or swapped).
    pub fn set_shown(&mut self, shown: bool) {
        if shown != self.shown {
            self.shown = shown;
            self.after_switch_keep_address();
        }
    }

    fn after_switch_keep_address(&mut self) {
        let active = self.tabs.active_id();
        for (id, page) in &self.pages {
            let _ = page.view.set_visible(*id == active && self.shown && page.cover.is_none());
        }
    }

    /// Give the page keyboard focus (the user clicked into the page area).
    pub fn focus_page(&mut self) {
        if self.address_focused {
            self.blur_address();
        }
        let _ = self.active_page().view.focus();
    }

    /// Whether the shell draws the pages ([`Browser::take_frame`]) and hands
    /// them input ([`Browser::page_input`]).
    pub fn draws_pages(&self) -> bool {
        self.backend.draws_pages()
    }

    /// The active page as drawn since the last call, when the shell draws
    /// pages and the page shows (not covered or hidden).
    pub fn take_frame(&self) -> Option<Frame> {
        let page = self.active_page();
        if !self.shown || page.cover.is_some() {
            return None;
        }
        page.view.take_frame()
    }

    /// A still of the active page, for painting where its window was while a
    /// popup hides the window ([`morph_webview::WebView::snapshot`]).
    pub fn snapshot_page(&self) -> Option<Frame> {
        let page = self.active_page();
        if !self.shown || page.cover.is_some() {
            return None;
        }
        page.view.snapshot()
    }

    /// Hand the active page pointer or key input (pages the shell draws).
    pub fn page_input(&self, input: Input) {
        let page = self.active_page();
        if self.shown && page.cover.is_none() {
            page.view.input(input);
        }
    }

    /// The text a page copied ([`Input::Copy`]), once, for the clipboard.
    pub fn take_copied(&self) -> Option<String> {
        self.pages.values().find_map(|page| page.view.take_copied())
    }

    /// The pointer shape the active page wants.
    pub fn page_cursor(&self) -> PageCursor {
        self.active_page().view.cursor()
    }

    /// Whether the active page has the keyboard, for backends that track it
    /// ([`Backend::keyboard`]).
    pub fn page_has_keyboard(&self) -> bool {
        self.backend.keyboard() == Some(self.active_page().view.id())
    }

    /// The window's scale, for backends that render pages themselves.
    pub fn set_scale(&mut self, scale: f64) {
        self.backend.set_scale(scale);
    }

    /// Cancel every open `<select>` list (the tab or the page changed).
    fn close_menus(&mut self) {
        for page in self.pages.values_mut() {
            if page.menu.take().is_some() {
                page.view.option_menu(MenuChoice::Cancel);
            }
            page.tooltip = None;
        }
    }

    /// The active page's open `<select>` list, to draw.
    pub fn option_menu(&self) -> Option<&MenuState> {
        self.pages.get(&self.tabs.active_id()).and_then(|p| p.menu.as_ref())
    }

    /// The active page's tooltip, to draw by the pointer.
    pub fn tooltip(&self) -> Option<&str> {
        self.active_page().tooltip.as_deref()
    }

    fn answer_menu(&mut self, f: impl FnOnce(&mut MenuState) -> Option<MenuChoice>) -> bool {
        let id = self.tabs.active_id();
        let Some(page) = self.pages.get_mut(&id) else {
            return false;
        };
        let Some(menu) = page.menu.as_mut() else {
            return false;
        };
        if let Some(choice) = f(menu) {
            if matches!(choice, MenuChoice::Activate(_) | MenuChoice::Cancel) {
                page.menu = None;
            }
            page.view.option_menu(choice);
        }
        true
    }

    /// A key while the active page's list is open. Returns whether a list
    /// was open (and so took the key).
    pub fn menu_key(&mut self, key: MenuKey) -> bool {
        let now = std::time::Instant::now();
        self.answer_menu(|m| m.key(key, now))
    }

    /// The pointer is over row `i` of the open list.
    pub fn menu_hover(&mut self, i: usize) {
        self.answer_menu(|m| m.hover(i));
    }

    /// Row `i` of the open list was clicked.
    pub fn menu_click(&mut self, i: usize) {
        self.answer_menu(|m| m.click(i));
    }

    /// Close the open list without choosing (a click outside it).
    pub fn menu_cancel(&mut self) {
        self.answer_menu(|_| Some(MenuChoice::Cancel));
    }

    /// Drain the backend's events into tab state.
    pub fn poll(&mut self) -> Polled {
        let mut polled = Polled::default();
        if self.closed {
            return polled;
        }
        for outcome in self.downloads.poll_fetches() {
            self.log_download(&outcome);
            polled.changed = true;
        }
        for (view, event) in self.backend.poll() {
            if let Event::Shortcut(key) = event {
                polled.shortcuts.push(key);
                continue;
            }
            // Downloads belong to the window, not to the page that started them.
            if let Event::Cookies { download, cookies } = event {
                if self.log_events {
                    // Never the values: only how many came.
                    eprintln!("morph-event tab=0 DownloadCookies({download}, {:?})", cookies.as_ref().map(Vec::len));
                }
                self.downloads.cookies_answered(download, cookies);
                polled.changed = true;
                continue;
            }
            if let Event::Download(download) = event {
                if self.log_events {
                    eprintln!("morph-event tab=0 {:?}", Event::Download(download.clone()));
                }
                // Note the cookies it starts with, for a resume behind a login.
                let started = match &download.change {
                    morph_webview::DownloadChange::Started { url, .. } => Some((download.id, url.clone())),
                    _ => None,
                };
                let outcome = self.downloads.apply(download);
                if let Some((id, url)) = started {
                    self.backend.request_cookies(id, &url);
                    if let Some(&tab) = self.by_view.get(&view) {
                        self.auth.tabs.insert(id, tab);
                    }
                }
                self.log_download(&outcome);
                polled.changed = true;
                continue;
            }
            if self.hidden_view_event(view, &event) {
                continue;
            }
            let Some(&tab) = self.by_view.get(&view) else {
                continue;
            };
            if self.log_events {
                match &event {
                    // The icon's bytes would flood the log.
                    Event::Favicon(png) => eprintln!("morph-event tab={} Favicon({} bytes)", self.log_base + tab.0, png.len()),
                    _ => eprintln!("morph-event tab={} {event:?}", self.log_base + tab.0),
                }
            }
            polled.changed = true;
            self.apply(tab, event);
        }
        if self.poll_app_calls() {
            polled.changed = true;
        }
        if self.poll_download_auth() {
            polled.changed = true;
        }
        polled
    }

    fn apply(&mut self, tab: TabId, event: Event) {
        let active = self.tabs.active_id() == tab;
        let Some(current) = self.tabs.tab(tab).cloned() else {
            return;
        };
        let mut record = false;
        // Under the About page the tab shows morph://about, whatever the
        // page beneath it does.
        let about = self.pages.get(&tab).is_some_and(|p| p.cover.as_ref().is_some_and(Cover::is_own));
        if about && matches!(event, Event::UrlChanged(_) | Event::TitleChanged(_) | Event::LoadFailed(_)) {
            // Kept for Back: the tab shows the page's title again.
            if let (Event::TitleChanged(title), Some(page)) = (&event, self.pages.get_mut(&tab)) {
                page.shown_title = Some(title.clone());
            }
            return;
        }
        // A link on a page to a Weirdnet app: called like a typed one.
        if let Event::LoadFailed(error) = &event
            && address::is_app(&error.url)
        {
            let url = error.url.clone();
            self.start_app_call(tab, &url);
            return;
        }
        match event {
            Event::UrlChanged(url) => {
                let url = self.shown_address(url);
                let _ = self.tabs.update(tab, url.clone(), current.title);
                if active && !self.address_focused {
                    self.address.set_text_selected(&url);
                }
            }
            Event::TitleChanged(title) => {
                let _ = self.tabs.update(tab, current.url, title.clone());
                record = !title.is_empty() && self.pages.get(&tab).is_some_and(|p| p.finished);
            }
            Event::LoadStarted => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.loading = true;
                    page.finished = false;
                }
                if matches!(self.pages.get(&tab).and_then(|p| p.cover.as_ref()), Some(Cover::Failed(_))) {
                    self.set_cover(tab, None);
                }
            }
            Event::LoadFinished => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.loading = false;
                    page.finished = true;
                }
                record = true;
            }
            Event::History { can_back, can_forward } => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.can_back = can_back;
                    page.can_forward = can_forward;
                }
            }
            Event::PopupOpened { .. } => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.popups += 1;
                }
            }
            Event::PopupTitleChanged(_) => {}
            Event::PopupClosed => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.popups = page.popups.saturating_sub(1);
                }
            }
            Event::Crashed => {
                let expected = self.pages.get_mut(&tab).is_some_and(|p| std::mem::take(&mut p.terminating));
                if !expected {
                    if let Some(page) = self.pages.get_mut(&tab) {
                        page.loading = false;
                    }
                    self.set_cover(tab, Some(Cover::Crashed));
                }
            }
            Event::LoadFailed(error) => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.loading = false;
                    page.finished = false;
                    if !matches!(page.cover, Some(Cover::Failed(_))) {
                        page.shown_url = Some(current.url.clone());
                    }
                }
                // The engine puts the previous address back; the tab shows the
                // one that failed, so Enter or Reload tries it again.
                let _ = self.tabs.update(tab, error.url.clone(), current.title);
                if active && !self.address_focused {
                    self.address.set_text_selected(&error.url);
                }
                self.set_cover(tab, Some(Cover::Failed(error)));
            }
            Event::Unresponsive(true) => {
                if self.pages.get(&tab).is_some_and(|p| p.cover.is_none() && !p.waited) {
                    self.set_cover(tab, Some(Cover::Unresponsive));
                }
            }
            Event::Unresponsive(false) => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.waited = false;
                }
                if self.pages.get(&tab).is_some_and(|p| p.cover == Some(Cover::Unresponsive)) {
                    self.set_cover(tab, None);
                }
            }
            Event::FindResult { matches } => {
                if let (true, Some(find)) = (active, self.find.as_mut()) {
                    find.matches = Some(matches);
                }
            }
            Event::Focused => {
                if active && self.address_focused {
                    self.blur_address();
                }
            }
            Event::OptionMenu(menu) => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    if active && page.cover.is_none() {
                        page.menu = Some(MenuState::new(menu));
                    } else {
                        page.view.option_menu(MenuChoice::Cancel);
                    }
                }
            }
            Event::OptionMenuClosed => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.menu = None;
                }
            }
            Event::Tooltip(tip) => {
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.tooltip = tip;
                }
            }
            Event::Favicon(png) => {
                if let Some(origin) = origin(&current.url) {
                    self.icons.insert(origin, std::sync::Arc::new(png));
                }
            }
            Event::FileChooser { multiple } => {
                self.file_asked = false;
                if let Some((old, _)) = self.file_request.replace((tab, multiple))
                    && let Some(page) = self.pages.get(&old)
                    && old != tab
                {
                    page.view.choose_files(None);
                }
            }
            Event::OpenLink { url, target } => match target {
                morph_webview::LinkTarget::NewTab => self.open_link_in_tab(&url),
                morph_webview::LinkTarget::NewWindow => self.new_windows.push(url),
            },
            Event::LinkMenu { url, x, y } => {
                if active {
                    self.link_menu = Some((url, x, y));
                }
            }
            Event::Shortcut(_) | Event::Download(_) | Event::Cookies { .. } => {}
        }
        if record {
            let tab = self.tabs.tab(tab).cloned();
            if let (Some(tab), Some(sink)) = (tab, self.history.as_mut())
                && morph_core::library::is_recordable(&tab.url)
            {
                sink(&tab.url, &tab.title);
            }
        }
    }
}

#[cfg(test)]
mod tests;
