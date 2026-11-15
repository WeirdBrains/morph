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

use morph_core::omnibox;
use morph_core::tabs::{TabId, TabSet};
use morph_ui::actions::Action;
use morph_ui::text::TextField;
use morph_webview::{Backend, ErrorKind, Event, LoadError, Rect, Spec, ViewId, WebView};

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
    finished: bool,
    popups: usize,
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
            finished: false,
            popups: 0,
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
}

/// Title and explanation for a page that failed to load.
pub fn error_text(error: &LoadError) -> (String, String) {
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
        };
        let first = browser.tabs.active_id();
        let url = config.start_url.trim().to_string();
        browser.attach(first, &url)?;
        browser.after_switch();
        Ok(browser)
    }

    /// Tell the backend which keys the shell wants while a page has focus.
    pub fn set_shortcuts(&mut self, keystrokes: &[String]) {
        self.backend.set_shortcuts(keystrokes);
    }

    fn attach(&mut self, id: TabId, url: &str) -> morph_webview::Result<()> {
        let target = if url.is_empty() { String::new() } else { omnibox::target(url).unwrap_or_default() };
        let active = self.tabs.active_id() == id;
        let view = self.backend.create(Spec {
            url: target.clone(),
            data_dir: self.data_dir.clone(),
            bounds: self.page_area,
            visible: active && self.shown,
        })?;
        self.by_view.insert(view.id(), id);
        let _ = self.tabs.update(id, target, String::new());
        self.pages.insert(id, Page::new(view));
        Ok(())
    }

    /// Show only the active tab's page, and refresh the address bar.
    fn after_switch(&mut self) {
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
                let loading = self.pages.get(&tab.id).is_some_and(|p| p.loading);
                TabLabel { id: tab.id, label, active: tab.id == active, loading }
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
    /// become a web search.
    pub fn submit_address(&mut self) {
        let Some(target) = omnibox::target(self.address.text()) else {
            return;
        };
        self.load(&target);
    }

    /// Load `url` (already an address) in the active tab and focus the page.
    pub fn load(&mut self, url: &str) {
        let id = self.tabs.active_id();
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
        }
        self.after_switch();
    }

    pub fn close_tab(&mut self, id: TabId) {
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
                let _ = self.active_page().view.stop();
            }
            Action::Back => self.back(),
            Action::Forward => {
                let _ = self.active_page().view.forward();
            }
            Action::Find => self.open_find(),
            Action::FindNext | Action::FindPrevious if self.find.is_none() => self.open_find(),
            Action::FindNext => self.find_next(true),
            Action::FindPrevious => self.find_next(false),
            Action::ToggleFullscreen
            | Action::Quit
            | Action::Bookmark
            | Action::ShowBookmarks
            | Action::ShowHistory
            | Action::ShowProfiles
            | Action::ShowSettings
            | Action::ShowIdentity => {
                return false;
            }
        }
        true
    }

    /// Reload the active page. After a crash or an error page this brings
    /// the page back; a page that stopped answering is ended first.
    pub fn reload(&mut self) {
        let shown = self.shown;
        let area = self.page_area;
        let id = self.tabs.active_id();
        if let Some(page) = self.pages.get_mut(&id) {
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
        let page = self.pages.get_mut(&id).expect("every tab has a page");
        if matches!(page.cover, Some(Cover::Failed(_))) {
            let url = page.shown_url.take().unwrap_or_default();
            self.set_cover(id, None);
            let title = self.tabs.active().title.clone();
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

    /// Drain the backend's events into tab state.
    pub fn poll(&mut self) -> Polled {
        let mut polled = Polled::default();
        for (view, event) in self.backend.poll() {
            if let Event::Shortcut(key) = event {
                polled.shortcuts.push(key);
                continue;
            }
            let Some(&tab) = self.by_view.get(&view) else {
                continue;
            };
            if self.log_events {
                eprintln!("morph-event tab={} {event:?}", tab.0);
            }
            polled.changed = true;
            self.apply(tab, event);
        }
        polled
    }

    fn apply(&mut self, tab: TabId, event: Event) {
        let active = self.tabs.active_id() == tab;
        let Some(current) = self.tabs.tab(tab).cloned() else {
            return;
        };
        let mut record = false;
        match event {
            Event::UrlChanged(url) => {
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
            Event::Shortcut(_) => {}
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
