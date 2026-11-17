//! Morph's preferences (morph://settings, Ctrl+, or ⌘, and the ☰ menu),
//! drawn in the page area as a page: a list of sections on the left that
//! jumps to each one, a search box that narrows them, and the settings
//! themselves. Everything but the default browser belongs to the shown
//! profile and is saved in its `settings.json`; a change asks the shell
//! ([`ShellRequest::SettingsChanged`]) to pass the new settings on.
//!
//! Keys: ↑/↓ move between settings, ←/→ change a choice, Enter or Space
//! runs a button, `/` or Ctrl+F searches, Esc leaves the search.

use std::path::{Path, PathBuf};

use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, ScrollHandle, SharedString, Window, div, prelude::*, px, rgb};
use morph_core::search::ENGINES;
use morph_core::settings::{self, Settings, TabPosition, ThemeChoice};
use morph_ui::actions::Action;
use morph_ui::chrome::SettingsChange;
use morph_ui::theme::Theme;

use std::sync::mpsc;

use morph_services::device_link::Links;
use morph_services::relays::{RelayReport, RelayStatus};

use crate::default_browser::{self, Status};
use crate::shell::ShellRequest;
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::{button, chip};

/// The parts of the page, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Profile,
    Browser,
    Search,
    StartPage,
    Layout,
    Appearance,
    Downloads,
    Privacy,
    Identity,
    Relays,
    About,
}

impl Section {
    pub const ALL: [Section; 11] = [
        Section::Profile,
        Section::Browser,
        Section::Search,
        Section::StartPage,
        Section::Layout,
        Section::Appearance,
        Section::Downloads,
        Section::Privacy,
        Section::Identity,
        Section::Relays,
        Section::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Profile => "Profile",
            Section::Browser => "Default browser",
            Section::Search => "Search engine",
            Section::StartPage => "Start page",
            Section::Layout => "Layout",
            Section::Appearance => "Appearance",
            Section::Downloads => "Downloads",
            Section::Privacy => "Privacy and site data",
            Section::Identity => "Identity and backup",
            Section::Relays => "Relays",
            Section::About => "About",
        }
    }

    /// Words a search also finds the section by.
    fn keywords(self) -> &'static str {
        match self {
            Section::Profile => "bookmarks history clear profile",
            Section::Browser => "default browser links open system",
            Section::Search => "search engine google duckduckgo address",
            Section::StartPage => "home start page new window blank",
            Section::Layout => "tabs position top bottom left right collapse side width",
            Section::Appearance => "theme dark light colours colors",
            Section::Downloads => "downloads folder save files",
            Section::Privacy => "clear browsing data cookies cache site permissions camera microphone location",
            Section::Identity => "identity backup recovery restore devices link",
            Section::Relays => "relays relay network weirdnet connected connection reservation slot link devices nat peer",
            Section::About => "about version licence license notices",
        }
    }

    /// Whether the setting is the same for every profile.
    pub fn app_wide(self) -> bool {
        matches!(self, Section::Browser | Section::Relays | Section::About)
    }

    /// Whether a search for `query` shows this section.
    pub fn matches(self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty() || q.split_whitespace().all(|w| self.title().to_lowercase().contains(w) || self.keywords().contains(w))
    }
}

/// One setting the keyboard can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    OpenBookmarks,
    OpenHistory,
    ClearHistory,
    MakeDefault,
    Engine,
    HomePage,
    BlankStart,
    TabPosition,
    Collapsed,
    SideWidth,
    Theme,
    ChooseDownloads,
    DefaultDownloads,
    ClearData,
    OpenIdentity,
    CheckRelays,
    OpenAbout,
}

impl Item {
    pub fn section(self) -> Section {
        match self {
            Item::OpenBookmarks | Item::OpenHistory | Item::ClearHistory => Section::Profile,
            Item::MakeDefault => Section::Browser,
            Item::Engine => Section::Search,
            Item::HomePage | Item::BlankStart => Section::StartPage,
            Item::TabPosition | Item::Collapsed | Item::SideWidth => Section::Layout,
            Item::Theme => Section::Appearance,
            Item::ChooseDownloads | Item::DefaultDownloads => Section::Downloads,
            Item::ClearData => Section::Privacy,
            Item::OpenIdentity => Section::Identity,
            Item::CheckRelays => Section::Relays,
            Item::OpenAbout => Section::About,
        }
    }

    pub const ALL: [Item; 17] = [
        Item::OpenBookmarks,
        Item::OpenHistory,
        Item::ClearHistory,
        Item::MakeDefault,
        Item::Engine,
        Item::HomePage,
        Item::BlankStart,
        Item::TabPosition,
        Item::Collapsed,
        Item::SideWidth,
        Item::Theme,
        Item::ChooseDownloads,
        Item::DefaultDownloads,
        Item::ClearData,
        Item::OpenIdentity,
        Item::CheckRelays,
        Item::OpenAbout,
    ];

    /// Whether ←/→ change it.
    fn is_choice(self) -> bool {
        matches!(self, Item::Engine | Item::TabPosition | Item::Theme | Item::SideWidth | Item::Collapsed)
    }
}

/// The settings a search for `query` leaves.
pub fn visible_items(query: &str) -> Vec<Item> {
    Item::ALL.into_iter().filter(|i| i.section().matches(query)).collect()
}

/// The next choice after (or before) `current` in `list`, wrapping.
pub fn cycle<T: PartialEq + Copy>(list: &[T], current: T, forward: bool) -> T {
    let i = list.iter().position(|x| *x == current).unwrap_or(0);
    let n = list.len();
    list[if forward { (i + 1) % n } else { (i + n - 1) % n }]
}

/// How often the page reads the relay state again while it is open.
const RELAY_POLL: std::time::Duration = std::time::Duration::from_secs(1);

const POSITIONS: [TabPosition; 4] = [TabPosition::Top, TabPosition::Bottom, TabPosition::Left, TabPosition::Right];
const THEMES: [ThemeChoice; 2] = [ThemeChoice::Dark, ThemeChoice::Light];

pub struct SettingsView {
    home: PathBuf,
    settings: Settings,
    theme: Theme,
    focus: FocusHandle,
    error: Option<String>,
    note: Option<String>,
    /// Whether Morph opens links, read when the page opens.
    browser: Option<Status>,
    browser_note: Option<String>,
    /// The default-browser work (never on the UI thread).
    browser_check: Option<default_browser::Check>,
    search: Entity<TextInput>,
    home_page: Entity<TextInput>,
    /// The setting the keyboard is on (index into [`visible_items`]).
    current: Option<usize>,
    scroll: ScrollHandle,
    /// A folder dialog is open.
    dialog: Option<gpui::Task<()>>,
    /// The relays as last read (after the page first opened), and when.
    relays: Option<RelayReport>,
    relays_read: Option<std::time::Instant>,
    /// A relay check starting on its own thread.
    relay_check: Option<mpsc::Receiver<Result<(), String>>>,
    relay_note: Option<String>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<ShellRequest> for SettingsView {}

impl SettingsView {
    pub fn new(home: &Path, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new("Search settings", theme, cx));
        let home_page = cx.new(|cx| TextInput::new("Blank page", theme, cx));
        let subscriptions = vec![
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                TextInputEvent::Changed => {
                    this.current = None;
                    this.scroll.set_offset(gpui::point(px(0.), px(0.)));
                    cx.notify();
                }
                TextInputEvent::Cancel => {
                    this.search.update(cx, |s, cx| s.set_text("", cx));
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
                TextInputEvent::Submit { .. } | TextInputEvent::Tab { back: false } => {
                    this.current = Some(0);
                    window.focus(&this.focus, cx);
                    cx.notify();
                }
                TextInputEvent::Tab { back: true } => {}
            }),
            cx.subscribe_in(&home_page, window, |this, input, event, window, cx| match event {
                TextInputEvent::Submit { .. } | TextInputEvent::Tab { .. } => {
                    let text = input.read(cx).text().trim().to_string();
                    this.set_home_page(text, cx);
                    window.focus(&this.focus, cx);
                }
                TextInputEvent::Cancel => {
                    let saved = this.settings.home_page.clone();
                    input.update(cx, |i, cx| i.set_text(&saved, cx));
                    window.focus(&this.focus, cx);
                }
                TextInputEvent::Changed => {}
            }),
        ];
        let mut view = Self {
            home: home.to_path_buf(),
            settings: Settings::default(),
            theme,
            focus: cx.focus_handle(),
            error: None,
            note: None,
            browser: None,
            browser_note: None,
            browser_check: None,
            search,
            home_page,
            current: None,
            scroll: ScrollHandle::new(),
            dialog: None,
            relays: None,
            relays_read: None,
            relay_check: None,
            relay_note: None,
            _subscriptions: subscriptions,
        };
        view.reload_quietly();
        let saved = view.settings.home_page.clone();
        view.home_page.update(cx, |i, cx| i.set_text(&saved, cx));
        view
    }

    fn reload_quietly(&mut self) {
        match settings::load(&self.home) {
            Ok(s) => {
                self.settings = s;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Could not read the settings: {e}")),
        }
    }

    /// Read the shown profile's settings again (after a profile switch).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.reload_quietly();
        let saved = self.settings.home_page.clone();
        self.home_page.update(cx, |i, cx| i.set_text(&saved, cx));
        cx.notify();
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        self.search.update(cx, |i, cx| i.set_theme(theme, cx));
        self.home_page.update(cx, |i, cx| i.set_theme(theme, cx));
        cx.notify();
    }

    /// The page opened (or was selected): it takes the keyboard and,
    /// with `section`, shows that section.
    pub fn focus(&mut self, section: Option<Section>, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser_check.is_none() && self.browser.is_none() {
            self.browser_check = Some(default_browser::Check::status(default_browser::system));
        }
        self.browser_note = None;
        self.relays_read = None;
        self.read_relays(cx);
        if let Some(section) = section {
            self.search.update(cx, |s, cx| s.set_text("", cx));
            self.current = visible_items("").iter().position(|i| i.section() == section);
            self.scroll_to(section);
        }
        window.focus(&self.focus, cx);
        cx.emit(ShellRequest::FocusShell);
        cx.notify();
    }

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).text().to_string()
    }

    fn scroll_to(&mut self, section: Section) {
        if let Some(i) = Section::ALL.iter().position(|s| *s == section) {
            self.scroll.scroll_to_item(i);
        }
    }

    /// Make `change` to the layout settings and save it.
    pub fn change(&mut self, change: SettingsChange, cx: &mut Context<Self>) {
        let next = change.apply(&self.settings);
        self.save(next, cx);
    }

    fn save(&mut self, next: Settings, cx: &mut Context<Self>) {
        if next == self.settings {
            return;
        }
        match settings::save(&self.home, &next) {
            Ok(()) => {
                self.settings = next;
                self.error = None;
                cx.emit(ShellRequest::SettingsChanged);
            }
            Err(e) => self.error = Some(format!("Could not save the settings: {e}")),
        }
        cx.notify();
    }

    /// The default search engine for new tabs.
    pub fn set_engine(&mut self, id: &str, cx: &mut Context<Self>) {
        let next = Settings { search_engine: id.to_string(), ..self.settings.clone() };
        self.save(next, cx);
    }

    fn set_theme_choice(&mut self, theme: ThemeChoice, cx: &mut Context<Self>) {
        let next = Settings { theme, ..self.settings.clone() };
        self.save(next, cx);
    }

    fn set_home_page(&mut self, text: String, cx: &mut Context<Self>) {
        let page = morph_browser::start_address(&text, self.settings.engine());
        let next = Settings { home_page: page.clone(), ..self.settings.clone() };
        self.home_page.update(cx, |i, cx| i.set_text(&page, cx));
        self.save(next, cx);
    }

    fn set_downloads(&mut self, dir: String, cx: &mut Context<Self>) {
        let next = Settings { downloads_dir: dir, ..self.settings.clone() };
        self.save(next, cx);
    }

    fn choose_downloads(&mut self, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        let answer = crate::dialogs::ask(cx, crate::dialogs::Ask::Folder);
        self.dialog = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |view, cx| {
                view.dialog = None;
                match result {
                    Ok(paths) => {
                        if let Some(dir) = paths.first() {
                            view.set_downloads(dir.display().to_string(), cx);
                        }
                    }
                    Err(e) => view.error = Some(e),
                }
                cx.notify();
            });
        }));
    }

    fn make_default(&mut self, cx: &mut Context<Self>) {
        self.browser_check = Some(default_browser::Check::make_default(default_browser::system));
        self.browser_note = Some("Asking the system…".into());
        cx.notify();
    }

    /// Pick up finished default-browser work; called by the host's tick.
    pub fn poll(&mut self, cx: &mut Context<Self>) {
        self.poll_relays(cx);
        let Some((status, made)) = self.browser_check.as_ref().and_then(|c| c.take()) else {
            return;
        };
        self.browser_check = None;
        self.browser = Some(status);
        self.browser_note = made.map(|made| match made {
            Ok(note) | Err(note) => note,
        });
        cx.notify();
    }

    /// Read the relay state now; cheap, never touches the network.
    fn read_relays(&mut self, cx: &mut Context<Self>) {
        self.relays_read = Some(std::time::Instant::now());
        let next = Links::global().relays();
        if self.relays.as_ref() != Some(&next) {
            if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
                let state = next.relays.first().map_or("none", |r| next.status(r).as_str());
                let rtt = next.relays.first().and_then(|r| r.rtt_ms).map_or("none".to_string(), |ms| ms.to_string());
                let addrs = next.relays.first().map_or(0, |r| r.addrs.len());
                println!(
                    "morph-event relays nodes={} checking={} state={state} count={} rtt={rtt} addrs={addrs}",
                    next.nodes,
                    next.checking,
                    next.relays.len()
                );
            }
            self.relays = Some(next);
            cx.notify();
        }
    }

    /// Follow the relay state once the page has opened, and a starting check.
    fn poll_relays(&mut self, cx: &mut Context<Self>) {
        if let Some(rx) = &self.relay_check {
            match rx.try_recv() {
                Ok(done) => {
                    self.relay_check = None;
                    self.relay_note = done.err().map(|e| format!("Could not check the relays: {e}"));
                    self.read_relays(cx);
                    cx.notify();
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.relay_check = None,
            }
        }
        if self.relays_read.is_some_and(|t| t.elapsed() >= RELAY_POLL) {
            self.read_relays(cx);
        }
    }

    /// Reach the relays for a moment (a node like a link's), off the UI thread.
    fn check_relays(&mut self, cx: &mut Context<Self>) {
        if self.relay_check.is_some() || self.relays.as_ref().is_some_and(|r| r.checking) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("morph-relay-check".into()).spawn(move || {
            let _ = tx.send(Links::global().relay_check_start().map_err(|e| e.to_string()));
        });
        match spawned {
            Ok(_) => {
                self.relay_check = Some(rx);
                self.relay_note = None;
            }
            Err(e) => self.relay_note = Some(format!("Could not check the relays: {e}")),
        }
        cx.notify();
    }

    /// The window came back to the front: the user may have changed the
    /// default browser in the system's settings.
    pub fn recheck(&mut self) {
        if self.browser_check.is_none() && self.browser.is_some() {
            self.browser_check = Some(default_browser::Check::status(default_browser::system));
        }
    }

    /// Run `item` (Enter, Space or a click).
    fn activate(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        match item {
            Item::OpenBookmarks => cx.emit(ShellRequest::Run(Action::ShowBookmarks)),
            Item::OpenHistory => cx.emit(ShellRequest::Run(Action::ShowHistory)),
            Item::ClearHistory => {
                cx.emit(ShellRequest::ClearHistory);
                self.note = Some("History cleared for this profile.".into());
            }
            Item::MakeDefault => self.make_default(cx),
            Item::Engine => {
                let next = cycle(&ENGINES.map(|e| e.id), self.settings.engine().id, true);
                self.set_engine(next, cx);
            }
            Item::HomePage => {
                window.focus(&self.home_page.focus_handle(cx), cx);
                self.home_page.update(cx, |i, cx| i.select_all(cx));
            }
            Item::BlankStart => self.set_home_page(String::new(), cx),
            Item::TabPosition => self.change(SettingsChange::Position(cycle(&POSITIONS, self.settings.tab_position, true)), cx),
            Item::Collapsed => self.change(SettingsChange::ToggleCollapsed, cx),
            Item::SideWidth => self.change(SettingsChange::Wider, cx),
            Item::Theme => self.set_theme_choice(cycle(&THEMES, self.settings.theme, true), cx),
            Item::ChooseDownloads => self.choose_downloads(cx),
            Item::DefaultDownloads => self.set_downloads(String::new(), cx),
            Item::ClearData => {
                cx.emit(ShellRequest::ClearBrowsingData);
                self.note = Some("Cookies, cache, site storage and history cleared for this profile.".into());
            }
            Item::OpenIdentity => cx.emit(ShellRequest::Run(Action::ShowIdentity)),
            Item::CheckRelays => self.check_relays(cx),
            Item::OpenAbout => cx.emit(ShellRequest::Run(Action::ShowAbout)),
        }
        cx.notify();
    }

    /// ←/→ on a choice.
    fn step(&mut self, item: Item, forward: bool, cx: &mut Context<Self>) {
        match item {
            Item::Engine => {
                let next = cycle(&ENGINES.map(|e| e.id), self.settings.engine().id, forward);
                self.set_engine(next, cx);
            }
            Item::TabPosition => self.change(SettingsChange::Position(cycle(&POSITIONS, self.settings.tab_position, forward)), cx),
            Item::Theme => self.set_theme_choice(cycle(&THEMES, self.settings.theme, forward), cx),
            Item::SideWidth => self.change(if forward { SettingsChange::Wider } else { SettingsChange::Narrower }, cx),
            Item::Collapsed => self.change(SettingsChange::ToggleCollapsed, cx),
            _ => {}
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        let k = &event.keystroke;
        let items = visible_items(&self.query(cx));
        let command = k.modifiers.control || k.modifiers.platform;
        if (command && k.key == "f") || (!command && k.key == "/") {
            window.focus(&self.search.focus_handle(cx), cx);
            self.search.update(cx, |s, cx| s.select_all(cx));
            cx.stop_propagation();
            return;
        }
        if command || k.modifiers.alt {
            return;
        }
        let current = self.current.filter(|i| *i < items.len());
        match k.key.as_str() {
            "down" | "tab" if !k.modifiers.shift || k.key == "down" => {
                self.current = Some(current.map_or(0, |i| (i + 1).min(items.len().saturating_sub(1))));
            }
            "up" | "tab" => {
                self.current = match current {
                    Some(0) | None => {
                        window.focus(&self.search.focus_handle(cx), cx);
                        None
                    }
                    Some(i) => Some(i - 1),
                };
            }
            "left" | "right" => {
                if let Some(item) = current.map(|i| items[i]).filter(|i| i.is_choice()) {
                    self.step(item, k.key == "right", cx);
                }
            }
            "enter" | "space" => {
                if let Some(item) = current.map(|i| items[i]) {
                    self.activate(item, window, cx);
                }
            }
            "escape" => {
                if self.current.take().is_none() {
                    cx.emit(ShellRequest::ClosePanel);
                }
            }
            _ => return,
        }
        if let Some(item) = self.current.and_then(|i| items.get(i)) {
            self.scroll_to(item.section());
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// One setting's row: label and help on the left, controls on the right.
    fn row(&self, item: Item, label: &str, help: Option<String>, controls: gpui::AnyElement, cx: &App) -> gpui::AnyElement {
        let t = self.theme;
        let items = visible_items(&self.query(cx));
        let current = self.current.and_then(|i| items.get(i)) == Some(&item);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(16.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(if current { t.accent } else { t.surface }))
            .bg(rgb(t.surface))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(13.)).child(label.to_string()))
                    .children(help.map(|h| div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(h))),
            )
            .child(controls)
            .into_any_element()
    }

    fn section_block(&self, section: Section, rows: Vec<gpui::AnyElement>) -> gpui::AnyElement {
        let t = self.theme;
        div()
            .id(SharedString::from(format!("settings-section-{}", section.title())))
            .flex()
            .flex_col()
            .gap(px(8.))
            .pb(px(20.))
            .child(div().flex().flex_row().items_end().gap(px(8.)).child(div().text_size(px(16.)).child(section.title())).child(
                div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(if section.app_wide() { "All profiles" } else { "This profile" }),
            ))
            .children(rows)
            .into_any_element()
    }

    /// Settings → Relays: the summary with a check button, then each relay.
    fn relay_rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let Some(report) = &self.relays else {
            return vec![self.row(Item::CheckRelays, "Relays", Some("Reading…".into()), div().into_any_element(), cx)];
        };
        let checking = report.checking || self.relay_check.is_some();
        let help = self.relay_note.clone().unwrap_or_else(|| report.summary());
        let control = if report.relays.is_empty() {
            div().into_any_element()
        } else if checking {
            div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("Checking…").into_any_element()
        } else {
            button(t, "settings-relays-check", "Check now", false)
                .on_click(cx.listener(|this, _, w, cx| this.activate(Item::CheckRelays, w, cx)))
                .into_any_element()
        };
        let mut rows = vec![self.row(Item::CheckRelays, "Relays this device links through", Some(help), control, cx)];
        for relay in &report.relays {
            let status = report.status(relay);
            let color = match status {
                RelayStatus::Reserved => t.accent,
                RelayStatus::Connected => t.text_primary,
                _ => t.text_secondary,
            };
            let mut state = status.label().to_string();
            if let Some(ms) = relay.rtt_ms {
                state.push_str(&format!(" · {ms} ms"));
            }
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(8.))
                    .bg(rgb(t.surface))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(div().text_size(px(13.)).child(relay.title()))
                            .child(div().text_size(px(12.)).text_color(rgb(color)).child(state)),
                    )
                    .child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(format!("Peer {}", relay.peer)))
                    .children(relay.addrs.iter().map(|a| div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(a.clone())))
                    .into_any_element(),
            );
        }
        rows.push(
            div()
                .px(px(12.))
                .text_size(px(12.))
                .text_color(rgb(t.text_secondary))
                .child("Morph reaches a relay only while linking a device or during a check (30 s). A reserved slot lets a device on another network reach this one.")
                .into_any_element(),
        );
        rows
    }

    fn section_rows(&self, section: Section, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let s = self.settings.clone();
        match section {
            Section::Profile => vec![
                self.row(
                    Item::OpenBookmarks,
                    "Bookmarks",
                    None,
                    button(t, "settings-bookmarks", "Open", false)
                        .on_click(cx.listener(|this, _, w, cx| this.activate(Item::OpenBookmarks, w, cx)))
                        .into_any_element(),
                    cx,
                ),
                self.row(
                    Item::OpenHistory,
                    "History",
                    None,
                    button(t, "settings-history", "Open", false)
                        .on_click(cx.listener(|this, _, w, cx| this.activate(Item::OpenHistory, w, cx)))
                        .into_any_element(),
                    cx,
                ),
                self.row(
                    Item::ClearHistory,
                    "Clear history",
                    Some("Forget every page this profile visited.".into()),
                    button(t, "settings-clear-history", "Clear history", false)
                        .on_click(cx.listener(|this, _, w, cx| this.activate(Item::ClearHistory, w, cx)))
                        .into_any_element(),
                    cx,
                ),
            ],
            Section::Browser => {
                let status = match &self.browser {
                    Some(Status::Default) => "Morph opens links from other apps.".to_string(),
                    Some(Status::Other(id)) => format!("Links open in {id}."),
                    Some(Status::Unknown(e)) => e.clone(),
                    None => "Checking…".to_string(),
                };
                // The newest word wins: what the last attempt said, else the
                // status (never both: they often say the same thing).
                let help = self.browser_note.clone().unwrap_or(status);
                let control = if self.browser == Some(Status::Default) {
                    div().text_size(px(12.)).text_color(rgb(t.accent)).child("Default").into_any_element()
                } else {
                    button(t, "settings-default-browser", "Make Morph the default", true)
                        .on_click(cx.listener(|this, _, w, cx| this.activate(Item::MakeDefault, w, cx)))
                        .into_any_element()
                };
                vec![self.row(Item::MakeDefault, "Open links from other apps in Morph", Some(help), control, cx)]
            }
            Section::Search => {
                let chips = div().flex().flex_row().gap(px(6.)).children(ENGINES.iter().enumerate().map(|(i, e)| {
                    let id = e.id;
                    chip(t, ("settings-engine", i), e.name, s.engine().id == id).on_click(cx.listener(move |this, _, _, cx| this.set_engine(id, cx)))
                }));
                vec![self.row(
                    Item::Engine,
                    "Search with",
                    Some("New tabs search with this. A tab can pick another from the button left of the address.".into()),
                    chips.into_any_element(),
                    cx,
                )]
            }
            Section::StartPage => vec![
                self.row(
                    Item::HomePage,
                    "Open at start",
                    Some("The page a new window opens. Empty opens a blank tab.".into()),
                    div().w(px(280.)).flex().child(self.home_page.clone()).into_any_element(),
                    cx,
                ),
                self.row(
                    Item::BlankStart,
                    "Start with a blank tab",
                    None,
                    button(t, "settings-blank-start", "Use a blank tab", false)
                        .on_click(cx.listener(|this, _, w, cx| this.activate(Item::BlankStart, w, cx)))
                        .into_any_element(),
                    cx,
                ),
            ],
            Section::Layout => {
                let positions = div().flex().flex_row().gap(px(6.)).children(POSITIONS.into_iter().enumerate().map(|(i, p)| {
                    let label = match p {
                        TabPosition::Top => "Top",
                        TabPosition::Bottom => "Bottom",
                        TabPosition::Left => "Left",
                        TabPosition::Right => "Right",
                    };
                    chip(t, ("settings-position", i), label, s.tab_position == p)
                        .on_click(cx.listener(move |this, _, _, cx| this.change(SettingsChange::Position(p), cx)))
                }));
                let side = matches!(s.tab_position, TabPosition::Left | TabPosition::Right);
                let mut rows = vec![
                    self.row(Item::TabPosition, "Tab position", None, positions.into_any_element(), cx),
                    self.row(
                        Item::Collapsed,
                        "Collapse the tabs",
                        Some("Collapsed tabs show only their icons; hover one for its title.".into()),
                        chip(t, "settings-collapsed", if s.sidebar_collapsed { "Collapsed" } else { "Expanded" }, s.sidebar_collapsed)
                            .on_click(cx.listener(|this, _, _, cx| this.change(SettingsChange::ToggleCollapsed, cx)))
                            .into_any_element(),
                        cx,
                    ),
                ];
                if side && !s.sidebar_collapsed {
                    rows.push(
                        self.row(
                            Item::SideWidth,
                            "Side tab list width",
                            None,
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    button(t, "settings-narrower", "−", false)
                                        .on_click(cx.listener(|this, _, _, cx| this.change(SettingsChange::Narrower, cx))),
                                )
                                .child(div().text_size(px(12.)).child(format!("{} px", s.sidebar_width)))
                                .child(
                                    button(t, "settings-wider", "+", false)
                                        .on_click(cx.listener(|this, _, _, cx| this.change(SettingsChange::Wider, cx))),
                                )
                                .into_any_element(),
                            cx,
                        ),
                    );
                }
                rows
            }
            Section::Appearance => {
                let chips = div().flex().flex_row().gap(px(6.)).children(THEMES.into_iter().enumerate().map(|(i, theme)| {
                    let label = if theme == ThemeChoice::Dark { "Dark" } else { "Light" };
                    chip(t, ("settings-theme", i), label, s.theme == theme)
                        .on_click(cx.listener(move |this, _, _, cx| this.set_theme_choice(theme, cx)))
                }));
                vec![self.row(Item::Theme, "Theme", None, chips.into_any_element(), cx)]
            }
            Section::Downloads => {
                let shown = if s.downloads_dir.is_empty() { "Your Downloads folder".to_string() } else { s.downloads_dir.clone() };
                vec![
                    self.row(
                        Item::ChooseDownloads,
                        "Save downloads in",
                        Some(shown),
                        button(t, "settings-downloads", "Choose…", false)
                            .on_click(cx.listener(|this, _, w, cx| this.activate(Item::ChooseDownloads, w, cx)))
                            .into_any_element(),
                        cx,
                    ),
                    self.row(
                        Item::DefaultDownloads,
                        "Use the Downloads folder",
                        None,
                        button(t, "settings-downloads-default", "Reset", false)
                            .on_click(cx.listener(|this, _, w, cx| this.activate(Item::DefaultDownloads, w, cx)))
                            .into_any_element(),
                        cx,
                    ),
                ]
            }
            Section::Privacy => vec![
                self.row(
                    Item::ClearData,
                    "Clear browsing data",
                    Some("Cookies, cache, site storage and history of this profile. Other profiles keep theirs.".into()),
                    button(t, "settings-clear-data", "Clear data", false)
                        .on_click(cx.listener(|this, _, w, cx| this.activate(Item::ClearData, w, cx)))
                        .into_any_element(),
                    cx,
                ),
                div()
                    .px(px(12.))
                    .text_size(px(12.))
                    .text_color(rgb(t.text_secondary))
                    .child("Site permissions: per-site controls for camera, microphone, location and notifications are coming soon.")
                    .into_any_element(),
            ],
            Section::Identity => vec![
                self.row(
                    Item::OpenIdentity,
                    "Identity, recovery words, backups and linked devices",
                    None,
                    button(t, "settings-identity", "Open Identity", false)
                        .on_click(cx.listener(|this, _, w, cx| this.activate(Item::OpenIdentity, w, cx)))
                        .into_any_element(),
                    cx,
                ),
            ],
            Section::Relays => self.relay_rows(cx),
            Section::About => vec![
                self.row(
                    Item::OpenAbout,
                    &format!("Morph {}", crate::about::version()),
                    None,
                    button(t, "settings-about", "About Morph", false)
                        .on_click(cx.listener(|this, _, w, cx| this.activate(Item::OpenAbout, w, cx)))
                        .into_any_element(),
                    cx,
                ),
            ],
        }
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let query = self.query(cx);
        let shown: Vec<Section> = Section::ALL.into_iter().filter(|s| s.matches(&query)).collect();
        let nav = div().flex().flex_col().gap(px(2.)).w(px(200.)).flex_none().p(px(12.)).children(shown.iter().enumerate().map(|(i, &section)| {
            div()
                .id(("settings-nav", i))
                .px(px(10.))
                .py(px(5.))
                .rounded(px(6.))
                .text_size(px(13.))
                .text_color(rgb(t.text_secondary))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.surface_variant)).text_color(rgb(t.text_primary)))
                .child(section.title())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.current = visible_items(&this.query(cx)).iter().position(|i| i.section() == section);
                    if let Some(i) = Section::ALL.iter().filter(|s| s.matches(&this.query(cx))).position(|s| *s == section) {
                        this.scroll.scroll_to_item(i);
                    }
                    window.focus(&this.focus, cx);
                    cx.notify();
                }))
        }));
        let blocks: Vec<gpui::AnyElement> = shown
            .iter()
            .map(|&section| {
                let rows = self.section_rows(section, cx);
                self.section_block(section, rows)
            })
            .collect();
        let empty = shown.is_empty();
        div()
            .id("settings-page")
            .key_context("SettingsPage")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .absolute()
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(t.background))
            .text_color(rgb(t.text_primary))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .pt(px(16.))
                    .child(div().px(px(22.)).text_size(px(20.)).child("Settings"))
                    .child(div().px(px(12.)).w(px(200.)).flex().child(self.search.clone()))
                    .child(nav),
            )
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .max_w(px(820.))
                    .children(blocks)
                    .when(empty, |d| d.child(div().text_color(rgb(t.text_secondary)).child(format!("No settings match “{query}”."))))
                    .children(self.note.clone().map(|n| div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(n)))
                    .children(self.error.clone().map(|e| div().text_size(px(12.)).text_color(rgb(t.error)).child(e))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_narrows_the_sections_by_title_or_keyword() {
        assert_eq!(Section::ALL.iter().filter(|s| s.matches("")).count(), Section::ALL.len());
        let found: Vec<_> = Section::ALL.into_iter().filter(|s| s.matches("duckduckgo")).collect();
        assert_eq!(found, [Section::Search]);
        let found: Vec<_> = Section::ALL.into_iter().filter(|s| s.matches("Tab position")).collect();
        assert_eq!(found, [Section::Layout]);
        assert!(Section::Privacy.matches("cookies"));
        assert!(Section::ALL.iter().all(|s| !s.matches("zzzz")));
        assert!(visible_items("theme").iter().all(|i| i.section() == Section::Appearance));
    }

    #[test]
    fn every_setting_has_a_section_and_sections_keep_the_profile_split() {
        for item in Item::ALL {
            assert!(Section::ALL.contains(&item.section()));
        }
        for s in Section::ALL {
            assert!(Item::ALL.iter().any(|i| i.section() == s), "{s:?} has nothing to reach by keyboard");
        }
        assert!(Section::Browser.app_wide() && Section::Relays.app_wide());
        assert_eq!(Section::ALL.into_iter().filter(|s| s.matches("relay")).collect::<Vec<_>>(), [Section::Relays]);
        assert_eq!(visible_items("relays"), [Item::CheckRelays]);
        for s in [Section::Profile, Section::Search, Section::Layout, Section::Appearance, Section::Downloads, Section::Privacy] {
            assert!(!s.app_wide(), "{s:?} is per profile");
        }
    }

    #[test]
    fn choices_cycle_both_ways() {
        assert_eq!(cycle(&POSITIONS, TabPosition::Top, true), TabPosition::Bottom);
        assert_eq!(cycle(&POSITIONS, TabPosition::Top, false), TabPosition::Right);
        assert_eq!(cycle(&THEMES, ThemeChoice::Light, true), ThemeChoice::Dark);
        assert_eq!(cycle(&ENGINES.map(|e| e.id), "google", true), "duckduckgo");
    }
}
