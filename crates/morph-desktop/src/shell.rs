//! The window and the component host. The host routes actions and page
//! state through [`Hosted`]; the browser and the library are
//! implementations, and panels ask the host for things (open a page, close
//! the panel) with [`ShellRequest`]s instead of knowing each other.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    AnyView, App, Bounds, Context, CursorStyle, Decorations, Entity, FocusHandle, KeyBinding, MouseButton, ResizeEdge, Subscription, Window,
    WindowBounds, WindowDecorations, WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use morph_ui::actions::{Action, Keymap};
use morph_ui::chrome::{self, WindowButtons};
use morph_ui::component::{self, Catalog, KindId};
use morph_ui::layout::{self, Layout, Slot};
use morph_ui::theme::Theme;

use crate::browser_view::{self, BrowserView};
use crate::cli::Args;
use crate::identity_view::IdentityView;
use crate::library::Tab;
use crate::library_view::LibraryView;
use crate::profiles_view::ProfilesView;
use crate::settings_view::SettingsView;

actions!(
    morph,
    [
        NewTab,
        CloseTab,
        NextTab,
        PrevTab,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectLastTab,
        FocusAddress,
        Find,
        FindNext,
        FindPrevious,
        Reload,
        Stop,
        Back,
        Forward,
        ToggleFullscreen,
        Quit,
        Bookmark,
        ShowBookmarks,
        ShowHistory,
        ShowProfiles,
        ShowSettings,
        ShowIdentity,
        ShowAbout,
        ShowSearchSettings
    ]
);

/// The GPUI action for a Morph action.
pub(crate) fn gpui_action(action: Action) -> Box<dyn gpui::Action> {
    match action {
        Action::NewTab => Box::new(NewTab),
        Action::CloseTab => Box::new(CloseTab),
        Action::NextTab => Box::new(NextTab),
        Action::PrevTab => Box::new(PrevTab),
        Action::SelectTab(0) => Box::new(SelectTab1),
        Action::SelectTab(1) => Box::new(SelectTab2),
        Action::SelectTab(2) => Box::new(SelectTab3),
        Action::SelectTab(3) => Box::new(SelectTab4),
        Action::SelectTab(4) => Box::new(SelectTab5),
        Action::SelectTab(5) => Box::new(SelectTab6),
        Action::SelectTab(6) => Box::new(SelectTab7),
        Action::SelectTab(_) => Box::new(SelectTab8),
        Action::SelectLastTab => Box::new(SelectLastTab),
        Action::FocusAddress => Box::new(FocusAddress),
        Action::Find => Box::new(Find),
        Action::FindNext => Box::new(FindNext),
        Action::FindPrevious => Box::new(FindPrevious),
        Action::Reload => Box::new(Reload),
        Action::Stop => Box::new(Stop),
        Action::Back => Box::new(Back),
        Action::Forward => Box::new(Forward),
        Action::ToggleFullscreen => Box::new(ToggleFullscreen),
        Action::Quit => Box::new(Quit),
        Action::Bookmark => Box::new(Bookmark),
        Action::ShowBookmarks => Box::new(ShowBookmarks),
        Action::ShowHistory => Box::new(ShowHistory),
        Action::ShowProfiles => Box::new(ShowProfiles),
        Action::ShowSettings => Box::new(ShowSettings),
        Action::ShowIdentity => Box::new(ShowIdentity),
        Action::ShowAbout => Box::new(ShowAbout),
        Action::ShowSearchSettings => Box::new(ShowSearchSettings),
    }
}

/// The page the main component shows, for panels that act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub url: String,
    pub title: String,
}

/// What a component asks of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellRequest {
    /// Open `url` in the main component.
    Open { url: String, new_tab: bool },
    /// Hide the panel that asked.
    ClosePanel,
    /// Take the keyboard back from the pages for the shell's own UI.
    FocusShell,
    /// The profile's library was unlocked.
    LibraryUnlocked,
    /// Show this profile in the window.
    SwitchProfile(String),
    /// A profile's name changed.
    ProfileRenamed,
    /// The settings panel saved new settings.
    SettingsChanged,
    /// The profile got an identity (its library is encrypted and open now).
    IdentityChanged,
    /// Run a shell action (a button on a page or in a menu).
    Run(Action),
    /// Show Settings at one section (`morph://relays` and the like).
    ShowSettingsAt(crate::settings_view::Section),
    /// Change the shown profile's layout settings (the tab bar's buttons).
    Change(morph_ui::chrome::SettingsChange),
    /// Forget the shown profile's history.
    ClearHistory,
    /// Forget the shown profile's cookies, cache, site storage and history.
    ClearBrowsingData,
    /// The last tab closed: save and quit.
    Quit,
}

/// Components each profile has its own of; rebuilt on a profile switch.
const PER_PROFILE: [&str; 3] = [component::BROWSER, component::LIBRARY, component::IDENTITY];

/// What the host needs from a component.
pub trait Hosted {
    fn kind(&self) -> KindId;
    fn view(&self) -> AnyView;
    /// Run a shell action; `false` when this component has no use for it.
    fn dispatch(&self, action: Action, window: &mut Window, cx: &mut App) -> bool;
    /// Periodic work (pumping web views). Returns keystrokes pressed inside
    /// the component that the shell should handle.
    fn tick(&self, window: &mut Window, cx: &mut App) -> Vec<String>;
    fn set_shown(&self, shown: bool, cx: &mut App);
    /// Pull keyboard focus back to the shell's own UI.
    fn focus_shell(&self, cx: &App);
    fn title(&self, cx: &App) -> String;
    /// Open `url`; `false` when this component does not show pages.
    fn open_url(&self, _url: &str, _new_tab: bool, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }
    /// The page it shows, if it shows one.
    fn page(&self, _cx: &App) -> Option<Page> {
        None
    }
    /// Whether the shown page is bookmarked (drives the star).
    fn set_page_marked(&self, _marked: bool, _cx: &mut App) {}
    /// A short note at the bottom of the component (`Bookmarked`), with an
    /// action to offer beside it.
    fn toast(&self, _text: &str, _action: Option<(&'static str, Action)>, _cx: &mut App) {}
    /// Give the component the keyboard (a panel above it closed).
    fn focus_content(&self, _window: &mut Window, _cx: &mut App) {}
    /// The profile's name, for a component that shows it.
    fn set_profile_label(&self, _name: &str, _cx: &mut App) {}
    /// The profile's settings, at start and whenever they change.
    fn apply_settings(&self, _settings: &morph_core::settings::Settings, _cx: &mut App) {}
    /// The window is frameless (`Some`, with the buttons to draw) and this
    /// component, in the main slot, should act as its caption.
    fn set_caption(&self, _caption: Option<WindowButtons>, _cx: &mut App) {}
    /// The preferences page, for the main component to draw.
    /// The profiles to list (id, name, shown).
    fn set_profiles(&self, _profiles: Vec<(String, String, bool)>, _cx: &mut App) {}
    /// Forget the profile's site data (cookies, cache, storage).
    fn clear_site_data(&self, _cx: &mut App) {}
}

impl Hosted for Entity<BrowserView> {
    fn kind(&self) -> KindId {
        KindId::new(component::BROWSER)
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn dispatch(&self, action: Action, window: &mut Window, cx: &mut App) -> bool {
        self.update(cx, |view, cx| view.dispatch(action, window, cx))
    }
    fn tick(&self, window: &mut Window, cx: &mut App) -> Vec<String> {
        self.update(cx, |view, cx| view.tick(window, cx))
    }
    fn set_shown(&self, shown: bool, cx: &mut App) {
        self.update(cx, |view, _| view.set_shown(shown));
    }
    fn focus_shell(&self, cx: &App) {
        self.read(cx).browser().focus_shell();
    }
    fn title(&self, cx: &App) -> String {
        self.read(cx).title()
    }
    fn open_url(&self, url: &str, new_tab: bool, window: &mut Window, cx: &mut App) -> bool {
        self.update(cx, |view, cx| view.open_url(url, new_tab, window, cx));
        true
    }
    fn page(&self, cx: &App) -> Option<Page> {
        let browser = self.read(cx).browser();
        let url = browser.active_url();
        (!url.is_empty()).then(|| Page { url: url.to_string(), title: browser.active_title().to_string() })
    }
    fn set_page_marked(&self, marked: bool, cx: &mut App) {
        self.update(cx, |view, cx| view.set_marked(marked, cx));
    }
    fn toast(&self, text: &str, action: Option<(&'static str, Action)>, cx: &mut App) {
        self.update(cx, |view, cx| view.toast(text, action, cx));
    }
    fn focus_content(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |view, cx| view.focus_page(window, cx));
    }
    fn set_profile_label(&self, name: &str, cx: &mut App) {
        self.update(cx, |view, cx| view.set_profile_label(name, cx));
    }
    fn apply_settings(&self, settings: &morph_core::settings::Settings, cx: &mut App) {
        self.update(cx, |view, cx| view.apply_settings(settings, cx));
    }
    fn set_profiles(&self, profiles: Vec<(String, String, bool)>, cx: &mut App) {
        self.update(cx, |v, cx| v.set_profiles(profiles, cx));
    }
    fn clear_site_data(&self, cx: &mut App) {
        self.update(cx, |v, _| v.clear_site_data());
    }
    fn set_caption(&self, caption: Option<WindowButtons>, cx: &mut App) {
        self.update(cx, |view, cx| view.set_caption(caption, cx));
    }
}

impl Hosted for Entity<IdentityView> {
    fn apply_settings(&self, settings: &morph_core::settings::Settings, cx: &mut App) {
        let theme = Theme::for_choice(settings.theme);
        self.update(cx, |view, cx| view.set_theme(theme, cx));
    }
    fn kind(&self) -> KindId {
        KindId::new(component::IDENTITY)
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn dispatch(&self, _action: Action, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }
    fn tick(&self, window: &mut Window, cx: &mut App) -> Vec<String> {
        self.update(cx, |view, cx| view.poll(window, cx));
        Vec::new()
    }
    fn set_shown(&self, _shown: bool, _cx: &mut App) {}
    fn focus_shell(&self, _cx: &App) {}
    fn title(&self, _cx: &App) -> String {
        "Identity".into()
    }
    fn set_profile_label(&self, name: &str, cx: &mut App) {
        self.update(cx, |view, _| view.set_profile_name(name));
    }
}

impl Hosted for Entity<SettingsView> {
    fn apply_settings(&self, settings: &morph_core::settings::Settings, cx: &mut App) {
        let theme = Theme::for_choice(settings.theme);
        self.update(cx, |view, cx| view.set_theme(theme, cx));
    }
    fn kind(&self) -> KindId {
        KindId::new(component::SETTINGS)
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn dispatch(&self, _action: Action, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }
    fn tick(&self, _window: &mut Window, cx: &mut App) -> Vec<String> {
        self.update(cx, |view, cx| view.poll(cx));
        Vec::new()
    }
    fn set_shown(&self, _shown: bool, _cx: &mut App) {}
    fn focus_shell(&self, _cx: &App) {}
    fn title(&self, _cx: &App) -> String {
        "Settings".into()
    }
}

impl Hosted for Entity<ProfilesView> {
    fn apply_settings(&self, settings: &morph_core::settings::Settings, cx: &mut App) {
        let theme = Theme::for_choice(settings.theme);
        self.update(cx, |view, cx| view.set_theme(theme, cx));
    }
    fn kind(&self) -> KindId {
        KindId::new(component::PROFILES)
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn dispatch(&self, _action: Action, _window: &mut Window, _cx: &mut App) -> bool {
        false
    }
    fn tick(&self, _window: &mut Window, _cx: &mut App) -> Vec<String> {
        Vec::new()
    }
    fn set_shown(&self, _shown: bool, _cx: &mut App) {}
    fn focus_shell(&self, _cx: &App) {}
    fn title(&self, _cx: &App) -> String {
        "Profiles".into()
    }
}

impl Hosted for Entity<LibraryView> {
    fn apply_settings(&self, settings: &morph_core::settings::Settings, cx: &mut App) {
        let theme = Theme::for_choice(settings.theme);
        self.update(cx, |view, cx| view.set_theme(theme, cx));
    }
    fn kind(&self) -> KindId {
        KindId::new(component::LIBRARY)
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn dispatch(&self, action: Action, _window: &mut Window, cx: &mut App) -> bool {
        match action {
            Action::Bookmark => self.update(cx, |view, cx| view.toggle_bookmark(cx)),
            _ => false,
        }
    }
    fn tick(&self, _window: &mut Window, _cx: &mut App) -> Vec<String> {
        Vec::new()
    }
    fn set_shown(&self, _shown: bool, _cx: &mut App) {}
    fn focus_shell(&self, _cx: &App) {}
    fn title(&self, _cx: &App) -> String {
        "Library".into()
    }
}

pub struct Shell {
    home: PathBuf,
    profile: String,
    theme: Theme,
    keymap: Keymap,
    catalog: Catalog,
    layout: Layout,
    components: Vec<Box<dyn Hosted>>,
    error: Option<String>,
    last_title: String,
    /// Holds GPUI's keyboard focus when no input does, so the shell's keys
    /// work while its own chrome (not a page or a field) has the keyboard.
    focus: FocusHandle,
    library: Option<Entity<LibraryView>>,
    profiles: Option<Entity<ProfilesView>>,
    settings: Option<Entity<SettingsView>>,
    identity: Option<Entity<IdentityView>>,
    /// Subscriptions to the profile's own components, dropped on a switch.
    profile_subscriptions: Vec<Subscription>,
    /// The main component's page as last passed to the panels.
    last_page: Option<Page>,
    last_marked: Option<bool>,
    /// Which window buttons to draw when the window is frameless.
    buttons: WindowButtons,
    /// The caption last passed to the main component.
    last_caption: Option<Option<WindowButtons>>,
    /// The panel in the sidebar, for `MORPH_LOG_EVENTS`.
    last_sidebar: Option<String>,
    /// Actions `MORPH_TEST_ACTIONS` asks for (comma-separated names, run
    /// one per tick from the first), for the smoke tests.
    test_actions: Vec<Action>,
    /// Later launches' URLs (single instance).
    listener: Option<crate::single::Listener>,
    /// weirdnet-morph:// links to route once the window runs.
    pending_links: Vec<String>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    #[allow(clippy::too_many_arguments)]
    fn new(
        home: PathBuf,
        profile: String,
        start: &str,
        theme: Theme,
        keymap: Keymap,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut catalog = Catalog::new();
        for kind in component::builtin() {
            catalog.register(kind);
        }
        let profiles = cx.new(|cx| ProfilesView::new(&home, &profile, theme, window, cx));
        let settings = cx.new(|cx| SettingsView::new(&home, theme, window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&profiles, window, |shell, _, request, window, cx| shell.request(request, window, cx)),
            cx.subscribe_in(&settings, window, |shell, _, request, window, cx| shell.request(request, window, cx)),
            // Back in front: the default browser may have changed meanwhile.
            cx.observe_window_activation(window, |shell, window, cx| {
                if window.is_window_active()
                    && let Some(settings) = &shell.settings
                {
                    settings.update(cx, |view, _| view.recheck());
                }
            }),
        ];
        // Settings are a page now, not a panel: an old layout's settings
        // sidebar is dropped.
        let mut layout = layout;
        if layout.shown(Slot::Sidebar).is_some_and(|k| k.as_str() == component::SETTINGS) {
            layout.toggle(Slot::Sidebar);
        }
        let mut shell = Self {
            home,
            profile,
            theme,
            keymap,
            catalog,
            layout,
            components: vec![Box::new(profiles.clone()), Box::new(settings.clone())],
            error: None,
            last_title: String::new(),
            focus: cx.focus_handle(),
            library: None,
            profiles: Some(profiles),
            settings: Some(settings),
            identity: None,
            last_page: None,
            last_marked: None,
            buttons: chrome::window_buttons(&|k| std::env::var(k).ok()),
            last_caption: None,
            last_sidebar: None,
            test_actions: std::env::var("MORPH_TEST_ACTIONS")
                .unwrap_or_default()
                .split(',')
                .rev()
                .filter_map(|name| Action::from_name(name.trim()))
                .collect(),
            listener: None,
            pending_links: Vec::new(),
            _subscriptions: subscriptions,
            profile_subscriptions: Vec::new(),
        };
        // A weirdnet-morph:// link is routed, never loaded as a page.
        let start = if morph_core::scheme::is_link(start) {
            shell.pending_links.push(start.to_string());
            ""
        } else {
            start
        };
        shell.build(start, window, cx);
        shell
    }

    /// Make the profile's own components (browser, library) and show the
    /// library's password prompt if it is locked.
    fn build(&mut self, start: &str, window: &mut Window, cx: &mut Context<Self>) {
        for c in &self.components {
            if PER_PROFILE.contains(&c.kind().as_str()) {
                c.set_shown(false, cx);
            }
        }
        self.components.retain(|c| !PER_PROFILE.contains(&c.kind().as_str()));
        self.profile_subscriptions.clear();
        let (home, profile, theme) = (self.home.clone(), self.profile.clone(), self.theme);
        let keystrokes = page_shortcuts(&self.keymap);
        self.error = match BrowserView::create(&home, &profile, start, window, &keystrokes) {
            Ok(browser) => {
                let labels = crate::browser_view::shortcut_labels(&self.keymap);
                let view = cx.new(|cx| {
                    let mut view = BrowserView::new(browser, theme, window, cx);
                    view.set_shortcut_labels(labels);
                    view
                });
                self.profile_subscriptions.push(cx.subscribe_in(&view, window, |shell, _, request, window, cx| shell.request(request, window, cx)));
                if let Some(settings) = &self.settings {
                    let any: AnyView = settings.clone().into();
                    view.update(cx, |v, cx| v.set_settings_view(any, cx));
                }
                self.components.insert(0, Box::new(view));
                None
            }
            Err(e) => {
                eprintln!("morph: {e}");
                Some(e)
            }
        };
        let library = cx.new(|cx| LibraryView::new(&home, &profile, theme, window, cx));
        self.profile_subscriptions.push(cx.subscribe_in(&library, window, |shell, _, request, window, cx| shell.request(request, window, cx)));
        self.components.push(Box::new(library.clone()));
        let locked = library.read(cx).is_locked();
        self.library = Some(library);
        let name = self.profiles.as_ref().map(|p| p.read(cx).shown_name()).unwrap_or_default();
        let identity = cx.new(|cx| IdentityView::new(&home, &profile, &name, theme, window, cx));
        self.profile_subscriptions.push(cx.subscribe_in(&identity, window, |shell, _, request, window, cx| shell.request(request, window, cx)));
        self.components.push(Box::new(identity.clone()));
        self.identity = Some(identity);
        self.last_page = None;
        self.last_marked = None;
        self.last_caption = None;
        // Components outside a visible slot keep their pages hidden.
        for c in &self.components {
            let shown = Slot::ALL.iter().any(|s| self.layout.shown(*s) == Some(&c.kind()));
            c.set_shown(shown, cx);
        }
        self.update_profile_label(cx);
        if let Some(settings) = &self.settings {
            settings.update(cx, |view, cx| view.reload(cx));
        }
        self.apply_settings(cx);
        self.update_profile_list(cx);
        if locked {
            self.show_library(Tab::Bookmarks, window, cx);
        }
    }

    /// Pass the shown profile's settings to every component.
    fn apply_settings(&mut self, cx: &mut App) {
        let Some(settings) = &self.settings else {
            return;
        };
        let current = settings.read(cx).settings().clone();
        self.theme = Theme::for_choice(current.theme);
        for c in &self.components {
            c.apply_settings(&current, cx);
        }
    }

    /// Tell the components which profiles there are (the address menu
    /// lists them).
    fn update_profile_list(&self, cx: &mut App) {
        let list: Vec<(String, String, bool)> = morph_core::profiles::list(&self.home)
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                let shown = p.id == self.profile;
                (p.id, p.name, shown)
            })
            .collect();
        for c in &self.components {
            c.set_profiles(list.clone(), cx);
        }
    }

    fn update_profile_label(&self, cx: &mut App) {
        let Some(profiles) = &self.profiles else {
            return;
        };
        let name = profiles.read(cx).shown_name();
        for c in &self.components {
            c.set_profile_label(&name, cx);
        }
    }

    /// Show profile `id` in this window: its tabs, cookies, library, layout.
    fn switch_profile(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profiles) = self.profiles.clone() else {
            return;
        };
        if !profiles.update(cx, |view, cx| view.switch(id, cx)) {
            return;
        }
        self.profile = id.to_string();
        crate::library::open_at_launch(&self.home, id);
        self.layout = layout::load(&self.home, id);
        self.build("", window, cx);
        cx.notify();
    }

    fn main(&self) -> Option<&dyn Hosted> {
        self.layout.shown(Slot::Main).and_then(|k| self.component(k))
    }

    /// A component asked for something.
    fn request(&mut self, request: &ShellRequest, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            ShellRequest::Open { url, new_tab } => {
                if let Some(main) = self.main() {
                    main.open_url(url, *new_tab, window, cx);
                }
            }
            ShellRequest::ClosePanel => {
                if self.layout.shown(Slot::Sidebar).is_some() {
                    self.layout.toggle(Slot::Sidebar);
                    self.save_layout();
                }
                if let Some(main) = self.main() {
                    main.focus_content(window, cx);
                }
            }
            ShellRequest::FocusShell => self.focus_shell(cx),
            ShellRequest::LibraryUnlocked => self.last_marked = None,
            ShellRequest::SwitchProfile(id) => self.switch_profile(id, window, cx),
            ShellRequest::ProfileRenamed => {
                self.update_profile_label(cx);
                self.update_profile_list(cx);
            }
            ShellRequest::SettingsChanged => self.apply_settings(cx),
            ShellRequest::IdentityChanged => {
                // The new identity encrypted the library; open it so history
                // and bookmarks work now.
                crate::library::open_at_launch(&self.home, &self.profile);
                if let Some(library) = &self.library {
                    library.update(cx, |view, cx| view.reload(cx));
                }
                self.last_marked = None;
            }
            ShellRequest::Run(action) => self.run(*action, window, cx),
            ShellRequest::ShowSettingsAt(section) => self.show_settings(Some(*section), window, cx),
            ShellRequest::Change(change) => {
                if let Some(settings) = self.settings.clone() {
                    settings.update(cx, |view, cx| view.change(*change, cx));
                }
            }
            ShellRequest::ClearHistory => self.clear_history(cx),
            ShellRequest::ClearBrowsingData => {
                self.clear_history(cx);
                for c in &self.components {
                    c.clear_site_data(cx);
                }
            }
            ShellRequest::Quit => self.quit(cx),
        }
        cx.notify();
    }

    fn clear_history(&self, cx: &mut App) {
        if let Err(e) = morph_core::library::clear_history(&self.home, &self.profile) {
            eprintln!("morph: could not clear the history: {e}");
        }
        if let Some(library) = &self.library {
            library.update(cx, |view, cx| view.reload(cx));
        }
    }

    /// Save what the window shows and quit (the last tab closed, Ctrl+Q,
    /// the menu). Settings are saved as they change; the layout is saved
    /// here once more so a quit never loses it.
    fn quit(&self, cx: &mut App) {
        self.save_layout();
        if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
            println!("morph-event quit");
        }
        cx.quit();
    }

    fn focus_shell(&self, cx: &App) {
        for component in &self.components {
            component.focus_shell(cx);
        }
    }

    fn save_layout(&self) {
        if let Err(e) = layout::save(&self.home, &self.profile, &self.layout) {
            eprintln!("morph: could not save the layout: {e}");
        }
    }

    /// Open the library on `tab`, or close it when it already shows that.
    /// Ctrl+D or the star: bookmark the page or remove its bookmark, say
    /// so, and redraw the star at once. A locked library opens to be
    /// unlocked instead of doing nothing.
    fn bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(library) = self.library.clone() else {
            return;
        };
        if self.main().and_then(|m| m.page(cx)).is_none() {
            return;
        }
        if library.read(cx).is_locked() {
            let kind = KindId::new(component::LIBRARY);
            if self.layout.shown(Slot::Sidebar) != Some(&kind) || library.read(cx).tab() != Tab::Bookmarks {
                self.show_library(Tab::Bookmarks, window, cx);
            }
            if let Some(main) = self.main() {
                main.toast("Unlock your library to bookmark this page", None, cx);
            }
            return;
        }
        // The library follows the page on the next tick; make sure it has
        // this one before toggling.
        let page = self.main().and_then(|c| c.page(cx));
        library.update(cx, |view, cx| view.set_page(page.clone(), cx));
        self.last_page = page;
        library.update(cx, |view, cx| view.toggle_bookmark(cx));
        let marked = library.read(cx).marked();
        self.last_marked = Some(marked);
        if let Some(main) = self.main() {
            main.set_page_marked(marked, cx);
            main.toast(if marked { "Bookmarked" } else { "Bookmark removed" }, Some(("Bookmarks", Action::ShowBookmarks)), cx);
        }
        if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
            println!("morph-event bookmark {}", if marked { "added" } else { "removed" });
        }
        cx.notify();
    }

    fn show_library(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        let Some(library) = self.library.clone() else {
            return;
        };
        let kind = KindId::new(component::LIBRARY);
        let open = self.layout.shown(Slot::Sidebar) == Some(&kind);
        if open && library.read(cx).tab() == tab {
            self.request(&ShellRequest::ClosePanel, window, cx);
            return;
        }
        if !open {
            self.layout.toggle_kind(Slot::Sidebar, kind);
            self.save_layout();
        }
        library.update(cx, |view, cx| {
            view.set_tab(tab, cx);
            view.focus(window, cx);
        });
        self.focus_shell(cx);
        cx.notify();
    }

    /// Open or close the profiles panel.
    fn show_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profiles) = self.profiles.clone() else {
            return;
        };
        if !self.layout.toggle_kind(Slot::Sidebar, KindId::new(component::PROFILES)) {
            self.request(&ShellRequest::ClosePanel, window, cx);
            return;
        }
        self.save_layout();
        profiles.update(cx, |view, cx| view.focus(window, cx));
        self.focus_shell(cx);
        cx.notify();
    }

    /// Open or close the identity panel.
    fn show_identity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(identity) = self.identity.clone() else {
            return;
        };
        if !self.layout.toggle_kind(Slot::Sidebar, KindId::new(component::IDENTITY)) {
            self.request(&ShellRequest::ClosePanel, window, cx);
            return;
        }
        self.save_layout();
        identity.update(cx, |view, cx| view.focus(window, cx));
        self.focus_shell(cx);
        cx.notify();
    }

    /// Show the preferences page (in a tab of the main component), at
    /// `section` if given.
    fn show_settings(&mut self, section: Option<crate::settings_view::Section>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings.clone() else {
            return;
        };
        if let Some(main) = self.main() {
            main.dispatch(Action::ShowSettings, window, cx);
        }
        settings.update(cx, |view, cx| view.focus(section, window, cx));
        self.focus_shell(cx);
        cx.notify();
    }

    fn component(&self, kind: &KindId) -> Option<&dyn Hosted> {
        self.components.iter().find(|c| &c.kind() == kind).map(|c| c.as_ref())
    }

    /// Route a shell action: the shell's own first, then the component in
    /// the main slot, then the others.
    fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
            println!("morph-event action {}", action.name());
        }
        match action {
            Action::Quit => {
                self.quit(cx);
                return;
            }
            Action::ToggleFullscreen => {
                window.toggle_fullscreen();
                return;
            }
            Action::Bookmark => return self.bookmark(window, cx),
            Action::ShowBookmarks => return self.show_library(Tab::Bookmarks, window, cx),
            Action::ShowHistory => return self.show_library(Tab::History, window, cx),
            Action::ShowProfiles => return self.show_profiles(window, cx),
            Action::ShowSettings => return self.show_settings(None, window, cx),
            Action::ShowSearchSettings => return self.show_settings(Some(crate::settings_view::Section::Search), window, cx),
            Action::ShowIdentity => return self.show_identity(window, cx),
            _ => {}
        }
        let main = self.layout.shown(Slot::Main).cloned();
        let mut order: Vec<&dyn Hosted> = Vec::new();
        if let Some(c) = main.as_ref().and_then(|k| self.component(k)) {
            order.push(c);
        }
        order.extend(self.components.iter().map(|c| c.as_ref()).filter(|c| Some(c.kind()) != main));
        for component in order {
            if component.dispatch(action, window, cx) {
                break;
            }
        }
        cx.notify();
    }

    /// A URL from outside (another launch, the OS): a new tab, or for a
    /// weirdnet-morph:// link, its route.
    fn open_external(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        use morph_core::scheme;
        if !scheme::is_link(url) {
            if let Some(main) = self.main() {
                main.open_url(url, true, window, cx);
            }
            return;
        }
        let note = match scheme::parse(url).map(scheme::route) {
            Some(scheme::Route::Unknown(link)) => {
                // Only the log-safe form: a link's query may carry tokens.
                eprintln!("morph: no handler for {} yet", link.describe());
                if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
                    println!("morph-event link unknown {}", link.describe());
                }
                "weirdnet-morph links open peer content from v0.3 on"
            }
            None => {
                eprintln!("morph: ignoring a malformed {} link", scheme::SCHEME);
                "That weirdnet-morph link is not valid"
            }
        };
        if let Some(main) = self.main() {
            main.toast(note, None, cx);
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::perf::count(&crate::perf::TICKS);
        crate::perf::report();
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        let mut handed = self.listener.as_ref().map(|l| l.take()).unwrap_or_default();
        // Links the OS handed over directly (Apple Events on macOS) and a
        // weirdnet-morph:// link Morph was started with.
        handed.extend(take_opened_urls().into_iter().map(crate::single::Request::Open));
        handed.extend(self.pending_links.drain(..).map(crate::single::Request::Open));
        for request in handed {
            if let crate::single::Request::Open(url) = &request {
                self.open_external(url, window, cx);
            }
            window.activate_window();
        }
        let mut shortcuts = Vec::new();
        for component in &self.components {
            shortcuts.extend(component.tick(window, cx));
        }
        for key in shortcuts {
            if let Some(action) = page_action(&self.keymap, &key) {
                self.run(action, window, cx);
            }
        }
        let sidebar = self.layout.shown(Slot::Sidebar).map(|k| k.as_str().to_string());
        if sidebar != self.last_sidebar {
            if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
                println!("morph-event panel {}", sidebar.as_deref().unwrap_or("none"));
            }
            self.last_sidebar = sidebar;
        }
        if let Some(action) = self.test_actions.pop() {
            self.run(action, window, cx);
        }
        // Frameless: our own caption and buttons. Tiling window managers
        // draw no frame either, so they get the close button regardless.
        // macOS and Windows hide the system title bar (`appears_transparent`):
        // the traffic lights sit in Morph's caption on macOS, and Morph draws
        // minimize, maximize and close on Windows.
        let host = if MAC {
            chrome::CaptionHost::Mac
        } else if cfg!(windows) {
            chrome::CaptionHost::Windows
        } else {
            chrome::CaptionHost::Linux { frameless: matches!(window.window_decorations(), Decorations::Client { .. }) }
        };
        let caption = chrome::caption(host, self.buttons);
        if self.last_caption != Some(caption) {
            if let Some(main) = self.main() {
                main.set_caption(caption, cx);
            }
            self.last_caption = Some(caption);
        }
        let title = match self.main() {
            Some(c) => c.title(cx),
            None => String::new(),
        };
        let page = self.main().and_then(|c| c.page(cx));
        if page != self.last_page {
            if let Some(library) = &self.library {
                library.update(cx, |view, cx| view.set_page(page.clone(), cx));
            }
            self.last_page = page;
        }
        let marked = self.library.as_ref().map(|l| l.read(cx).marked());
        if marked != self.last_marked {
            if let Some(main) = self.main() {
                main.set_page_marked(marked.unwrap_or(false), cx);
            }
            self.last_marked = marked;
        }
        if title != self.last_title {
            let full = if title.is_empty() { "Morph".to_string() } else { format!("{title} — Morph") };
            window.set_window_title(&full);
            self.last_title = title;
        }
    }

    fn slot(&self, slot: Slot, cx: &App) -> Option<gpui::AnyElement> {
        let t = self.theme;
        let kind = self.layout.shown(slot)?;
        let content = match self.component(kind) {
            Some(component) => component.view().into_any_element(),
            None => {
                let title = self.catalog.get(kind).map(|k| k.title.clone()).unwrap_or_else(|| kind.as_str().to_string());
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(t.text_secondary))
                    .child(format!("{title} is not available yet"))
                    .into_any_element()
            }
        };
        let size = px(self.layout.state(slot).size as f32);
        let _ = cx;
        Some(match slot {
            Slot::Main => div().flex_1().flex().flex_col().min_w(px(0.)).child(content).into_any_element(),
            Slot::Sidebar => div().w(size).h_full().border_r_1().border_color(rgb(t.surface_variant)).child(content).into_any_element(),
            Slot::Bottom => div().h(size).w_full().border_t_1().border_color(rgb(t.surface_variant)).child(content).into_any_element(),
        })
    }
}

macro_rules! on_actions {
    ($el:expr, $cx:expr, [$($gpui:ident => $action:expr),* $(,)?]) => {
        $el$(.on_action($cx.listener(|this, _: &$gpui, window, cx| this.run($action, window, cx))))*
    };
}

/// Width of the frameless window's resize border, in logical pixels.
const RESIZE_BORDER: f32 = 4.;

/// Grab handles along the edges and corners of a frameless window, in the
/// border kept free of the page (the page's own window would take the
/// clicks otherwise).
fn resize_handles() -> Vec<gpui::AnyElement> {
    let b = px(RESIZE_BORDER);
    let corner = px(RESIZE_BORDER * 3.);
    let handle = |id: &'static str, edge: ResizeEdge, cursor: CursorStyle| {
        div().id(id).absolute().cursor(cursor).on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            window.start_window_resize(edge);
        })
    };
    use CursorStyle::*;
    vec![
        handle("resize-top", ResizeEdge::Top, ResizeUpDown).top_0().left_0().w_full().h(b).into_any_element(),
        handle("resize-bottom", ResizeEdge::Bottom, ResizeUpDown).bottom_0().left_0().w_full().h(b).into_any_element(),
        handle("resize-left", ResizeEdge::Left, ResizeLeftRight).top_0().left_0().h_full().w(b).into_any_element(),
        handle("resize-right", ResizeEdge::Right, ResizeLeftRight).top_0().right_0().h_full().w(b).into_any_element(),
        handle("resize-top-left", ResizeEdge::TopLeft, ResizeUpLeftDownRight).top_0().left_0().size(corner).into_any_element(),
        handle("resize-top-right", ResizeEdge::TopRight, ResizeUpRightDownLeft).top_0().right_0().size(corner).into_any_element(),
        handle("resize-bottom-left", ResizeEdge::BottomLeft, ResizeUpRightDownLeft).bottom_0().left_0().size(corner).into_any_element(),
        handle("resize-bottom-right", ResizeEdge::BottomRight, ResizeUpLeftDownRight).bottom_0().right_0().size(corner).into_any_element(),
    ]
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::count(&crate::perf::SHELL_RENDERS);
        let t = self.theme;
        let root = div()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(t.background))
            .text_color(rgb(t.text_primary))
            .font_family(".SystemUIFont");
        let root = on_actions!(root, cx, [
            NewTab => Action::NewTab,
            CloseTab => Action::CloseTab,
            NextTab => Action::NextTab,
            PrevTab => Action::PrevTab,
            SelectTab1 => Action::SelectTab(0),
            SelectTab2 => Action::SelectTab(1),
            SelectTab3 => Action::SelectTab(2),
            SelectTab4 => Action::SelectTab(3),
            SelectTab5 => Action::SelectTab(4),
            SelectTab6 => Action::SelectTab(5),
            SelectTab7 => Action::SelectTab(6),
            SelectTab8 => Action::SelectTab(7),
            SelectLastTab => Action::SelectLastTab,
            FocusAddress => Action::FocusAddress,
            Find => Action::Find,
            FindNext => Action::FindNext,
            FindPrevious => Action::FindPrevious,
            Reload => Action::Reload,
            Stop => Action::Stop,
            Back => Action::Back,
            Forward => Action::Forward,
            ToggleFullscreen => Action::ToggleFullscreen,
            Quit => Action::Quit,
            Bookmark => Action::Bookmark,
            ShowBookmarks => Action::ShowBookmarks,
            ShowHistory => Action::ShowHistory,
            ShowProfiles => Action::ShowProfiles,
            ShowSettings => Action::ShowSettings,
            ShowIdentity => Action::ShowIdentity,
            ShowAbout => Action::ShowAbout,
            ShowSearchSettings => Action::ShowSearchSettings,
        ]);
        let root = root.on_any_mouse_down(cx.listener(|this, _, _, cx| this.focus_shell(cx)));
        if let Some(error) = &self.error {
            return root.items_center().justify_center().child(div().max_w(px(640.)).p(px(24.)).child(error.clone()));
        }
        let middle = div().flex().flex_row().flex_1().min_h(px(0.)).children(self.slot(Slot::Sidebar, cx)).children(self.slot(Slot::Main, cx));
        // A frameless window that is not maximized or tiled keeps a thin
        // border to resize it by.
        let floating = match window.window_decorations() {
            Decorations::Client { tiling } => !(tiling.top && tiling.bottom && tiling.left && tiling.right),
            Decorations::Server => false,
        };
        let root = if floating {
            root.relative().p(px(RESIZE_BORDER)).border_1().border_color(rgb(t.surface_variant)).children(resize_handles())
        } else {
            root
        };
        root.child(middle).children(self.slot(Slot::Bottom, cx))
    }
}

/// Keys for the address bar's editing actions, in its key context.
fn address_bindings() -> Vec<KeyBinding> {
    use browser_view::*;
    let c = Some(ADDRESS_CONTEXT);
    vec![
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("shift-insert", Paste, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new("escape", Cancel, c),
        KeyBinding::new("down", MenuDown, c),
        KeyBinding::new("up", MenuUp, c),
        KeyBinding::new("tab", FocusNextStop, c),
        KeyBinding::new("tab", FocusNextStop, Some(BAR_BUTTON_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPreviousStop, Some(BAR_BUTTON_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(BAR_BUTTON_CONTEXT)),
    ]
    .into_iter()
    .chain(
        MAC.then(|| {
            [
                KeyBinding::new("cmd-a", SelectAll, c),
                KeyBinding::new("cmd-v", Paste, c),
                KeyBinding::new("cmd-c", Copy, c),
                KeyBinding::new("cmd-x", Cut, c),
            ]
        })
        .into_iter()
        .flatten(),
    )
    .collect()
}

/// macOS: Morph's shortcuts use Cmd there (`Keymap::for_platform`).
pub const MAC: bool = cfg!(target_os = "macos");

/// Keys the shell takes even while a page has focus: every bound shortcut
/// (Ctrl+F opens the find bar). Everything else (Ctrl+C/V/X/A/Z, arrows,
/// Page Up/Down, Space, Tab, keypad) goes to the page.
fn page_shortcuts(keymap: &Keymap) -> Vec<String> {
    keymap.bindings().iter().map(|b| canonical(&b.keystroke)).collect()
}

/// `keystroke` with its modifiers in one order (ctrl, alt, shift, cmd), the
/// order the page script writes them: `cmd-shift-l` is `shift-cmd-l`.
pub fn canonical(keystroke: &str) -> String {
    const ORDER: [&str; 4] = ["ctrl", "alt", "shift", "cmd"];
    let (mods, key) = match keystroke.rsplit_once('-') {
        // `ctrl--` (the minus key) splits into `ctrl-` and ``.
        Some((m, "")) => (m.strip_suffix('-').unwrap_or(m), "-"),
        Some((m, k)) => (m, k),
        None => return keystroke.to_string(),
    };
    let held: Vec<&str> = mods.split('-').collect();
    let mut out: String = ORDER.iter().filter(|m| held.contains(m)).map(|m| format!("{m}-")).collect();
    out.push_str(key);
    out
}

/// The action for a shortcut a page reported (the keymap already has each
/// platform's modifier).
fn page_action(keymap: &Keymap, key: &str) -> Option<Action> {
    let key = canonical(key);
    keymap.bindings().iter().find(|b| canonical(&b.keystroke) == key).map(|b| b.action)
}

/// Open the window and run until it closes.
/// URLs the OS asked the running app to open (macOS Apple Events: links
/// and weirdnet-morph:// clicked elsewhere), until the shell's tick takes
/// them.
static OPENED_URLS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn take_opened_urls() -> Vec<String> {
    OPENED_URLS.lock().map(|mut urls| std::mem::take(&mut *urls)).unwrap_or_default()
}

pub fn run(home: PathBuf, args: Args, listener: Option<crate::single::Listener>) {
    let app = gpui_platform::application();
    app.on_open_urls(|urls| {
        if let Ok(mut opened) = OPENED_URLS.lock() {
            opened.extend(urls);
        }
    });
    app.run(move |cx: &mut App| {
        // On macOS every shortcut uses Cmd (`Keymap::for_platform`).
        let keymap = Keymap::default().for_platform(MAC);
        let mut bindings = address_bindings();
        bindings.extend(crate::text_input::bindings());
        for binding in keymap.bindings() {
            let keystroke = &binding.keystroke;
            match KeyBinding::load(keystroke, gpui_action(binding.action), None, false, None, cx) {
                Ok(b) => bindings.push(b),
                Err(e) => eprintln!("morph: ignoring key {keystroke}: {e:?}"),
            }
        }
        cx.bind_keys(bindings);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let profile = match morph_core::profiles::load_catalog(&home) {
            Ok(catalog) => {
                let profile = catalog.current().id.clone();
                crate::library::open_at_launch(&home, &profile);
                profile
            }
            Err(e) => {
                eprintln!("morph: profiles: {e}; using the default profile");
                morph_core::profiles::DEFAULT_PROFILE_ID.to_string()
            }
        };
        let saved_layout = layout::load(&home, &profile);
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            app_id: Some("morph".into()),
            window_min_size: Some(size(px(480.), px(320.))),
            // Frameless where the desktop can draw it (a compositor runs):
            // the tab row is the caption. GPUI keeps server decorations
            // otherwise. macOS and Windows hide the system title bar: the
            // top row (tabs, or the toolbar) is the caption, with the traffic
            // lights inset on macOS and Morph's own buttons on Windows.
            window_decorations: Some(if cfg!(target_os = "linux") { WindowDecorations::Client } else { WindowDecorations::Server }),
            titlebar: (!cfg!(target_os = "linux")).then(|| gpui::TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(gpui::point(px(12.), px(11.))),
            }),
            ..Default::default()
        };
        // A URL on the command line, else Settings' Open at start (blank by default).
        let start = args.open.clone().unwrap_or_else(|| morph_core::settings::load(&home).map(|s| s.home_page).unwrap_or_default());
        let mut listener = listener;
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| {
                let mut shell = Shell::new(home.clone(), profile.clone(), &start, Theme::default(), keymap.clone(), saved_layout, window, cx);
                shell.listener = listener.take();
                shell
            })
        });
        let window = match opened {
            Ok(window) => window,
            Err(e) => {
                eprintln!("morph: cannot open a window: {e:#}");
                cx.quit();
                return;
            }
        };
        let _ = window.update(cx, |shell, window, cx| {
            window.set_window_title("Morph");
            let _ = shell;
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(8)).await;
                    if this.update_in(cx, |shell, window, cx| shell.tick(window, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
            cx.activate(true);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_keep_keys_the_shell_does_not_use() {
        let keys = page_shortcuts(&Keymap::default());
        for taken in [
            "ctrl-t",
            "ctrl-w",
            "ctrl-l",
            "ctrl-r",
            "f5",
            "ctrl-tab",
            "ctrl-shift-tab",
            "f11",
            "ctrl-1",
            "ctrl-9",
            "ctrl-f",
            "f3",
            "shift-f3",
            "ctrl-g",
            "ctrl-shift-g",
            "ctrl-,",
            "ctrl-shift-l",
        ] {
            assert!(keys.iter().any(|k| k == taken), "{taken}");
        }
        for page in ["ctrl-c", "ctrl-v", "ctrl-x", "ctrl-a", "ctrl-z", "pageup", "space", "tab"] {
            assert!(!keys.iter().any(|k| k == page), "{page}");
        }
    }

    #[test]
    fn page_shortcuts_match_whatever_order_the_modifiers_come_in() {
        assert_eq!(canonical("cmd-shift-l"), "shift-cmd-l");
        assert_eq!(canonical("shift-ctrl-o"), "ctrl-shift-o");
        assert_eq!(canonical("ctrl--"), "ctrl--");
        assert_eq!(canonical("f3"), "f3");
        let mac = Keymap::default().for_platform(true);
        // What the page script sends on macOS for Cmd+Shift+L and Cmd+T.
        assert_eq!(page_action(&mac, "shift-cmd-l"), Some(Action::ShowIdentity));
        assert_eq!(page_action(&mac, "cmd-t"), Some(Action::NewTab));
        let other = Keymap::default().for_platform(false);
        assert_eq!(page_action(&other, "shift-cmd-l"), None, "Cmd is not Ctrl off macOS");
        assert_eq!(page_action(&other, "shift-ctrl-l"), Some(Action::ShowIdentity));
        for keymap in [mac, other] {
            for key in page_shortcuts(&keymap) {
                assert!(page_action(&keymap, &key).is_some(), "{key}");
            }
        }
    }

    #[test]
    fn every_platform_key_parses_for_gpui() {
        for keymap in [Keymap::default().for_platform(true), Keymap::default().for_platform(false)] {
            for b in keymap.bindings() {
                assert!(gpui::Keystroke::parse(Box::leak(b.keystroke.clone().into_boxed_str())).is_ok(), "{}", b.keystroke);
                assert_eq!(page_action(&keymap, &canonical(&b.keystroke)), Some(b.action), "{}", b.keystroke);
            }
        }
    }

    #[test]
    fn every_bound_action_has_a_gpui_action() {
        for binding in Keymap::default().bindings() {
            let action = gpui_action(binding.action);
            assert!(action.name().starts_with("morph::"), "{}", action.name());
        }
    }
}
