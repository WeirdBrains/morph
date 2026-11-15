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
use morph_webview::{Backend, Event, Rect, Spec, ViewId, WebView};

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
    crashed: bool,
    finished: bool,
    popups: usize,
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
    pub crashed: bool,
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
        self.pages.insert(id, Page { view, loading: false, can_back: false, can_forward: false, crashed: false, finished: false, popups: 0 });
        Ok(())
    }

    /// Show only the active tab's page, and refresh the address bar.
    fn after_switch(&mut self) {
        let active = self.tabs.active_id();
        for (id, page) in &self.pages {
            let visible = *id == active && self.shown && !page.crashed;
            if visible {
                let _ = page.view.set_bounds(self.page_area);
            }
            let _ = page.view.set_visible(visible);
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
        Toolbar { can_back: page.can_back, can_forward: page.can_forward, loading: page.loading, crashed: page.crashed, popups: page.popups }
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
            if page.crashed {
                page.crashed = false;
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
            Action::Back => {
                let _ = self.active_page().view.back();
            }
            Action::Forward => {
                let _ = self.active_page().view.forward();
            }
            Action::Find | Action::ToggleFullscreen | Action::Quit => return false,
        }
        true
    }

    /// Reload the active page; after a crash this brings the page back.
    pub fn reload(&mut self) {
        let shown = self.shown;
        let area = self.page_area;
        let id = self.tabs.active_id();
        if let Some(page) = self.pages.get_mut(&id) {
            if page.crashed {
                page.crashed = false;
                let _ = page.view.set_bounds(area);
                let _ = page.view.set_visible(shown);
            }
            let _ = page.view.reload();
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
            let _ = page.view.set_visible(*id == active && self.shown && !page.crashed);
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
                if let Some(page) = self.pages.get_mut(&tab) {
                    page.crashed = true;
                    page.loading = false;
                    let _ = page.view.set_visible(false);
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
