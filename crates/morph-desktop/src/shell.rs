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
        ShowSettings
    ]
);

/// The GPUI action for a Morph action.
fn gpui_action(action: Action) -> Box<dyn gpui::Action> {
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
}

/// Components each profile has its own of; rebuilt on a profile switch.
const PER_PROFILE: [&str; 2] = [component::BROWSER, component::LIBRARY];

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
    /// Give the component the keyboard (a panel above it closed).
    fn focus_content(&self, _window: &mut Window, _cx: &mut App) {}
    /// The profile's name, for a component that shows it.
    fn set_profile_label(&self, _name: &str, _cx: &mut App) {}
    /// The profile's settings, at start and whenever they change.
    fn apply_settings(&self, _settings: &morph_core::settings::Settings, _cx: &mut App) {}
    /// The window is frameless (`Some`, with the buttons to draw) and this
    /// component, in the main slot, should act as its caption.
    fn set_caption(&self, _caption: Option<WindowButtons>, _cx: &mut App) {}
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
    fn focus_content(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |view, cx| view.focus_page(window, cx));
    }
    fn set_profile_label(&self, name: &str, cx: &mut App) {
        self.update(cx, |view, cx| view.set_profile_label(name, cx));
    }
    fn apply_settings(&self, settings: &morph_core::settings::Settings, cx: &mut App) {
        self.update(cx, |view, cx| view.apply_settings(settings, cx));
    }
    fn set_caption(&self, caption: Option<WindowButtons>, cx: &mut App) {
        self.update(cx, |view, cx| view.set_caption(caption, cx));
    }
}

impl Hosted for Entity<SettingsView> {
    fn kind(&self) -> KindId {
        KindId::new(component::SETTINGS)
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
        "Settings".into()
    }
}

impl Hosted for Entity<ProfilesView> {
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
    /// Subscriptions to the profile's own components, dropped on a switch.
    profile_subscriptions: Vec<Subscription>,
    /// The main component's page as last passed to the panels.
    last_page: Option<Page>,
    last_marked: Option<bool>,
    /// Which window buttons to draw when the window is frameless.
    buttons: WindowButtons,
    /// The caption last passed to the main component.
    last_caption: Option<Option<WindowButtons>>,
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
        let settings = cx.new(|cx| SettingsView::new(&home, theme, cx));
        let subscriptions = vec![
            cx.subscribe_in(&profiles, window, |shell, _, request, window, cx| shell.request(request, window, cx)),
            cx.subscribe_in(&settings, window, |shell, _, request, window, cx| shell.request(request, window, cx)),
        ];
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
            last_page: None,
            last_marked: None,
            buttons: chrome::window_buttons(&|k| std::env::var(k).ok()),
            last_caption: None,
            _subscriptions: subscriptions,
            profile_subscriptions: Vec::new(),
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
                let view = cx.new(|cx| BrowserView::new(browser, theme, window, cx));
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
            settings.update(cx, |view, _| view.reload());
        }
        self.apply_settings(cx);
        if locked {
            self.show_library(Tab::Bookmarks, window, cx);
        }
    }

    /// Pass the shown profile's settings to every component.
    fn apply_settings(&self, cx: &mut App) {
        let Some(settings) = &self.settings else {
            return;
        };
        let current = settings.read(cx).settings().clone();
        for c in &self.components {
            c.apply_settings(&current, cx);
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
            ShellRequest::ProfileRenamed => self.update_profile_label(cx),
            ShellRequest::SettingsChanged => self.apply_settings(cx),
        }
        cx.notify();
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

    /// Open or close the settings panel.
    fn show_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settings) = self.settings.clone() else {
            return;
        };
        if !self.layout.toggle_kind(Slot::Sidebar, KindId::new(component::SETTINGS)) {
            self.request(&ShellRequest::ClosePanel, window, cx);
            return;
        }
        self.save_layout();
        settings.update(cx, |view, cx| view.focus(window, cx));
        self.focus_shell(cx);
        cx.notify();
    }

    fn component(&self, kind: &KindId) -> Option<&dyn Hosted> {
        self.components.iter().find(|c| &c.kind() == kind).map(|c| c.as_ref())
    }

    /// Route a shell action: the shell's own first, then the component in
    /// the main slot, then the others.
    fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::Quit => {
                cx.quit();
                return;
            }
            Action::ToggleFullscreen => {
                window.toggle_fullscreen();
                return;
            }
            Action::ShowBookmarks => return self.show_library(Tab::Bookmarks, window, cx),
            Action::ShowHistory => return self.show_library(Tab::History, window, cx),
            Action::ShowProfiles => return self.show_profiles(window, cx),
            Action::ShowSettings => return self.show_settings(window, cx),
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

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        let mut shortcuts = Vec::new();
        for component in &self.components {
            shortcuts.extend(component.tick(window, cx));
        }
        for key in shortcuts {
            if let Some(action) = self.keymap.action_for(&key) {
                self.run(action, window, cx);
            }
        }
        // Frameless: our own caption and buttons. Tiling window managers
        // draw no frame either, so they get the close button regardless.
        let caption = match window.window_decorations() {
            Decorations::Client { .. } => Some(self.buttons),
            Decorations::Server => (self.buttons == WindowButtons::CloseOnly).then_some(WindowButtons::CloseOnly),
        };
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
    ]
}

/// Keys the shell takes even while a page has focus: every bound shortcut
/// (Ctrl+F opens the find bar). Everything else (Ctrl+C/V/X/A/Z, arrows,
/// Page Up/Down, Space, Tab, keypad) goes to the page.
fn page_shortcuts(keymap: &Keymap) -> Vec<String> {
    keymap.bindings().iter().map(|b| b.keystroke.clone()).collect()
}

/// Open the window and run until it closes.
pub fn run(home: PathBuf, args: Args) {
    gpui_platform::application().run(move |cx: &mut App| {
        let keymap = Keymap::default();
        let mut bindings = address_bindings();
        bindings.extend(crate::text_input::bindings());
        for binding in keymap.bindings() {
            match KeyBinding::load(&binding.keystroke, gpui_action(binding.action), None, false, None, cx) {
                Ok(b) => bindings.push(b),
                Err(e) => eprintln!("morph: ignoring key {}: {e:?}", binding.keystroke),
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
            // otherwise.
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };
        let start = args.open.clone().unwrap_or_default();
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| Shell::new(home.clone(), profile.clone(), &start, Theme::default(), keymap.clone(), saved_layout, window, cx))
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
        ] {
            assert!(keys.iter().any(|k| k == taken), "{taken}");
        }
        for page in ["ctrl-c", "ctrl-v", "ctrl-x", "ctrl-a", "ctrl-z", "pageup", "space", "tab"] {
            assert!(!keys.iter().any(|k| k == page), "{page}");
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
