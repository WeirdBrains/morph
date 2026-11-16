//! The browser component's GPUI view: tab strip, toolbar and address bar,
//! drawn from `morph_browser::Browser`, with the page area left to the web
//! view. On X11 the web view is a child window above GPUI's surface, so
//! menus and panels never draw over the page and everything sits beside it.
//! On Wayland the page is rendered off screen and this view draws it and
//! hands it pointer and key input (`page_input`), keeping the shell's own
//! shortcuts.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    App, Bounds, ClipboardEntry, ClipboardItem, Context, Corners, CursorStyle, DispatchPhase, Entity, EntityInputHandler, FocusHandle, Focusable,
    Hitbox, HitboxBehavior, KeyDownEvent, KeyUpEvent, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, RenderImage, ScrollEvent, ScrollHandle, ShapedLine, SharedString, Subscription, UTF16Selection, Window, actions, anchored, canvas,
    deferred, div, point, prelude::*, px, rgb, size,
};
use morph_browser::{Browser, Config, Cover, MenuState};
use morph_core::settings::TabPosition;
use morph_core::tabs::TabId;
use morph_ui::actions::Action;
use morph_ui::chrome::SettingsChange;
use morph_ui::chrome::{TabStrip, WindowButtons};
use morph_ui::theme::Theme;
use morph_webview::{Input, PageCursor, Rect};

use crate::address::AddressElement;
use crate::menu_view::{self, Row};
use crate::shell::ShellRequest;
use crate::text_input::{TextInput, TextInputEvent};

actions!(
    address_bar,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Copy,
        Cut,
        Submit,
        Cancel,
        MenuDown,
        MenuUp,
        FocusNextStop,
        FocusPreviousStop
    ]
);

/// Key context of the tab bar's and toolbar's own buttons.
pub const BAR_BUTTON_CONTEXT: &str = "BarButton";

/// The key context the address bar's bindings apply in.
pub const ADDRESS_CONTEXT: &str = "AddressBar";

pub const TAB_STRIP_HEIGHT: f32 = 34.;
pub const TOOLBAR_HEIGHT: f32 = 42.;
pub const FIND_BAR_HEIGHT: f32 = 38.;

/// The key context of a page the shell draws: only the shell's global
/// shortcuts apply there, every other key goes to the page.
pub const PAGE_CONTEXT: &str = "Page";

pub struct BrowserView {
    browser: Browser,
    theme: Theme,
    address_focus: FocusHandle,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    selecting: bool,
    /// Address-bar focus as last mirrored into GPUI's focus.
    mirrored_focus: bool,
    find_input: Entity<TextInput>,
    /// The active page is bookmarked (the star), as the shell reports it.
    marked: bool,
    profile_label: String,
    /// Where the tabs go, from the profile's settings.
    strip: TabStrip,
    /// The window is frameless: the tab row (or the toolbar) is its caption
    /// and draws these buttons.
    caption: Option<WindowButtons>,
    /// The caption was pressed; moving the pointer now drags the window.
    caption_pressed: bool,
    /// Pages the shell draws (off-screen backend): the page's keyboard
    /// focus in GPUI, the last frame as an image, the pointer buttons held
    /// in the page, whether the pointer is over it, and the modifiers last
    /// seen (to report modifier keys).
    page_focus: FocusHandle,
    page_image: Option<Arc<RenderImage>>,
    page_buttons: u32,
    page_pointer: bool,
    page_modifiers: gpui::Modifiers,
    /// Where the pointer last was over the page (page coordinates), for
    /// the tooltip.
    page_pointer_at: (f64, f64),
    /// The open `<select>` list's scroll, and the row last scrolled to.
    select_scroll: ScrollHandle,
    select_scrolled: Option<usize>,
    /// The main menu (☰) or a tab's context menu, when open.
    shell_menu: Option<ShellMenu>,
    shell_menu_focus: FocusHandle,
    shell_menu_scroll: ScrollHandle,
    /// The shortcut shown beside each menu entry.
    shortcut_labels: HashMap<Action, String>,
    /// A short note at the bottom (`Bookmarked`), until it times out.
    toast: Option<Toast>,
    /// Main menu entries `MORPH_TEST_MENU` asks for (comma-separated
    /// labels), each opened and chosen as a click would, for the smoke
    /// tests on every desktop.
    test_menu: Vec<String>,
    /// What the last frame drew (the star, the toast), for the event log.
    drawn: (bool, Option<SharedString>),
    /// The preferences page (morph://settings), drawn in the page area.
    settings_view: Option<gpui::AnyView>,
    /// The profiles (id, name, shown), for the address menu.
    profiles: Vec<(String, String, bool)>,
    /// The tabs are collapsed to their icons (every position).
    tabs_collapsed: bool,
    /// The menu under the focused address bar.
    address_menu: Option<AddressMenu>,
    /// Keyboard stops of the tab bar's own buttons.
    position_focus: FocusHandle,
    collapse_focus: FocusHandle,
    engine_focus: FocusHandle,
    /// A page's file dialog is open.
    file_dialog: Option<gpui::Task<()>>,
    /// Save dialogs open for downloads.
    download_dialogs: Vec<gpui::Task<()>>,
    /// When the last download ended, for hiding the downloads bar.
    downloads_done: Option<std::time::Instant>,
    /// The shell wants the page shown (it hides it under its own panels).
    shell_shown: bool,
    /// Where the bar's buttons were last drawn (their menus open there).
    button_bounds: std::cell::RefCell<HashMap<&'static str, Bounds<Pixels>>>,
    /// Tab icons as images, by their PNG bytes.
    icon_images: std::cell::RefCell<HashMap<Arc<Vec<u8>>, Arc<gpui::Image>>>,
    _subscriptions: Vec<Subscription>,
}

impl gpui::EventEmitter<ShellRequest> for BrowserView {}

/// The menu under the focused address bar, as on mobile (address_sheet):
/// a row of page buttons, then the page's and Morph's places, then the
/// profiles.
struct AddressMenu {
    /// The entry the keyboard is on (into [`BrowserView::address_items`]).
    highlighted: Option<usize>,
    /// The page was hidden for it (child-window pages draw above the shell).
    hid_page: bool,
}

/// One entry of the address menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressItem {
    Back,
    Forward,
    CopyLink,
    Refresh,
    Bookmark,
    Find,
    ComingSoon(&'static str),
    Run(Action, &'static str),
    Identity,
    Profile(String, String, bool),
    ManageProfiles,
}

impl AddressItem {
    /// The four buttons in the first row.
    pub fn in_button_row(&self) -> bool {
        matches!(self, AddressItem::Back | AddressItem::Forward | AddressItem::CopyLink | AddressItem::Refresh)
    }
}

/// The address menu's entries, in order (keyboard order too).
pub fn address_items(profiles: &[(String, String, bool)]) -> Vec<AddressItem> {
    let mut items = vec![
        AddressItem::Back,
        AddressItem::Forward,
        AddressItem::CopyLink,
        AddressItem::Refresh,
        AddressItem::Bookmark,
        AddressItem::Find,
        AddressItem::ComingSoon("Extensions"),
        AddressItem::ComingSoon("More"),
        AddressItem::Run(Action::ShowHistory, "History"),
        AddressItem::Run(Action::ShowBookmarks, "Bookmarks"),
        AddressItem::ComingSoon("Downloads"),
        AddressItem::ComingSoon("Passwords"),
        AddressItem::Identity,
        AddressItem::Run(Action::ShowSettings, "Settings"),
    ];
    items.extend(profiles.iter().map(|(id, name, shown)| AddressItem::Profile(id.clone(), name.clone(), *shown)));
    items.push(AddressItem::ManageProfiles);
    items
}

/// Width of the address menu.
pub const ADDRESS_MENU_WIDTH: f32 = 360.;

/// A tooltip: one line of text.
struct Tip(SharedString, Theme);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = self.1;
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(4.))
            .bg(rgb(t.surface_variant))
            .border_1()
            .border_color(rgb(t.border))
            .text_size(px(12.))
            .text_color(rgb(t.text_primary))
            .child(self.0.clone())
    }
}

/// A tooltip builder for `text`.
fn tip(text: impl Into<SharedString>, t: Theme) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let text = text.into();
    move |_, cx| cx.new(|_| Tip(text.clone(), t)).into()
}

/// A short note at the bottom of the window.
struct Toast {
    text: SharedString,
    action: Option<(&'static str, Action)>,
    until: std::time::Instant,
}

/// How long the downloads bar stays after the last download ended.
const DOWNLOADS_SHOWN: std::time::Duration = std::time::Duration::from_secs(10);

/// A byte count for people: `512 B`, `4.5 MB`.
fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// How long a toast shows.
pub const TOAST_TIME: std::time::Duration = std::time::Duration::from_millis(3500);

/// What a shell menu entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MenuCommand {
    Run(Action),
    Duplicate(TabId),
    Reload(TabId),
    Close(TabId),
    CloseOthers(TabId),
    /// Move the tabs (the tab bar's position button).
    Position(TabPosition),
    /// Search with this engine in this tab (the selector left of the address).
    Engine(&'static str),
}

/// The main menu or a tab's context menu: rows (as a list's state, for the
/// keys), what each does, and where it opens in the window.
struct ShellMenu {
    id: &'static str,
    state: MenuState,
    rows: Vec<Row>,
    commands: Vec<MenuCommand>,
    at: Point<Pixels>,
    /// The page was hidden for the menu (child-window pages draw above the
    /// shell, so nothing can open over them).
    hid_page: bool,
}

/// The main menu: the old Flutter dot menu's entries (Find in page,
/// Bookmarks, History, Identity, Settings) with New tab, Profiles and
/// About added.
pub const MAIN_MENU: [(&str, Action, bool); 9] = [
    ("New tab", Action::NewTab, false),
    ("Find in page", Action::Find, true),
    ("Bookmarks", Action::ShowBookmarks, false),
    ("History", Action::ShowHistory, false),
    ("Identity", Action::ShowIdentity, true),
    ("Profiles", Action::ShowProfiles, false),
    ("Settings", Action::ShowSettings, false),
    ("About Morph", Action::ShowAbout, true),
    ("Quit", Action::Quit, true),
];

pub const MENU_WIDTH: f32 = 240.;

/// The label beside each menu entry, from the (platform's) keymap.
pub fn shortcut_labels(keymap: &morph_ui::actions::Keymap) -> HashMap<Action, String> {
    let mac = cfg!(target_os = "macos");
    MAIN_MENU
        .iter()
        .map(|e| e.1)
        .chain([Action::CloseTab, Action::Bookmark])
        .filter_map(|a| keymap.keystroke_for(a).map(|k| (a, menu_view::shortcut_label(k, mac))))
        .collect()
}

impl BrowserView {
    pub fn new(browser: Browser, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let find_input = cx.new(|cx| TextInput::new("Find in page", theme, cx));
        let find_events = cx.subscribe_in(&find_input, window, |this, input, event, window, cx| {
            match event {
                TextInputEvent::Changed => {
                    let query = input.read(cx).text().to_string();
                    this.browser.set_find_query(&query);
                }
                TextInputEvent::Submit { back } => this.browser.find_next(!back),
                TextInputEvent::Tab { .. } => {}
                TextInputEvent::Cancel => this.close_find(window, cx),
            }
            cx.notify();
        });
        Self {
            browser,
            theme,
            address_focus: cx.focus_handle(),
            last_layout: None,
            last_bounds: None,
            selecting: false,
            mirrored_focus: false,
            find_input,
            marked: false,
            drawn: (false, None),
            profile_label: String::new(),
            strip: TabStrip::Top,
            caption: None,
            caption_pressed: false,
            page_focus: cx.focus_handle(),
            page_image: None,
            page_buttons: 0,
            page_pointer: false,
            page_modifiers: gpui::Modifiers::default(),
            page_pointer_at: (0.0, 0.0),
            select_scroll: ScrollHandle::new(),
            select_scrolled: None,
            shell_menu: None,
            shell_menu_focus: cx.focus_handle(),
            shell_menu_scroll: ScrollHandle::new(),
            shortcut_labels: HashMap::new(),
            toast: None,
            test_menu: std::env::var("MORPH_TEST_MENU")
                .unwrap_or_default()
                .split(',')
                .rev()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            settings_view: None,
            profiles: Vec::new(),
            tabs_collapsed: false,
            address_menu: None,
            // Tab from the address bar: position, collapse, search engine,
            // in every layout (an explicit order: paint order changes with
            // the tab position).
            position_focus: cx.focus_handle().tab_stop(true).tab_index(1),
            collapse_focus: cx.focus_handle().tab_stop(true).tab_index(2),
            engine_focus: cx.focus_handle().tab_stop(true).tab_index(3),
            file_dialog: None,
            download_dialogs: Vec::new(),
            downloads_done: None,
            shell_shown: true,
            icon_images: std::cell::RefCell::new(HashMap::new()),
            button_bounds: std::cell::RefCell::new(HashMap::new()),
            _subscriptions: vec![find_events],
        }
    }

    /// A browser on the real web view backend: child windows of `window`
    /// on X11, pages rendered off screen on Wayland, WKWebView and WebView2
    /// child views on macOS and Windows.
    pub fn create(home: &Path, profile: &str, start: &str, window: &Window, keystrokes: &[String]) -> Result<Browser, String> {
        use raw_window_handle::HasWindowHandle;
        let raw = HasWindowHandle::window_handle(window).map_err(|e| format!("no window handle: {e}"))?.as_raw();
        let mut backend = Self::backend(raw, window)?;
        backend.set_page_bridge(morph_webview::PageBridge {
            script: morph_core::page_bridge::BRIDGE_SCRIPT.into(),
            handler: std::rc::Rc::new(morph_core::page_bridge::answer),
        });
        let home_for_history = home.to_path_buf();
        let profile_id = profile.to_string();
        let sink: morph_browser::HistorySink = Box::new(move |url: &str, title: &str| {
            // A locked library records nothing until the library panel unlocks it.
            let _ = morph_core::library::record_visit(&home_for_history, &profile_id, url, title, crate::library::now_ms());
        });
        let config = Config {
            data_dir: Some(morph_core::profiles::webkit_data_dir(home, profile)),
            start_url: start.to_string(),
            history: Some(sink),
            log_events: std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1"),
            search_engine: morph_core::settings::load(home).map(|s| s.search_engine).unwrap_or_default(),
        };
        let mut browser = Browser::new(backend, config).map_err(|e| e.to_string())?;
        browser.set_shortcuts(keystrokes);
        Ok(browser)
    }

    #[cfg(target_os = "linux")]
    fn backend(raw: raw_window_handle::RawWindowHandle, window: &Window) -> Result<Box<dyn morph_webview::Backend>, String> {
        Ok(match raw {
            raw_window_handle::RawWindowHandle::Wayland(_) => {
                Box::new(morph_webview::offscreen::OffscreenBackend::new(f64::from(window.scale_factor())))
            }
            raw => {
                let xid = morph_webview::wry_linux::x11_window_id(raw).ok_or("Morph's desktop shell runs on X11 or Wayland only")?;
                Box::new(morph_webview::wry_linux::WryBackend::new(xid))
            }
        })
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn backend(raw: raw_window_handle::RawWindowHandle, _: &Window) -> Result<Box<dyn morph_webview::Backend>, String> {
        let backend = morph_webview::wry_native::NativeBackend::new(raw).ok_or("the shell's window is not an AppKit or Win32 window")?;
        Ok(Box::new(backend))
    }

    pub fn browser(&self) -> &Browser {
        &self.browser
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    pub fn address_focus(&self) -> &FocusHandle {
        &self.address_focus
    }

    pub fn set_address_layout(&mut self, line: ShapedLine, bounds: Bounds<Pixels>) {
        self.last_layout = Some(line);
        self.last_bounds = Some(bounds);
    }

    /// The title for the window: the active page's, else its address.
    pub fn title(&self) -> String {
        let title = self.browser.active_title();
        if title.is_empty() { self.browser.active_url().to_string() } else { title.to_string() }
    }

    /// Run a shell action. Returns whether the browser handled it.
    pub fn dispatch(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let opens_find =
            matches!(action, Action::Find) || (matches!(action, Action::FindNext | Action::FindPrevious) && self.browser.find().is_none());
        let handled = self.browser.dispatch(action);
        if handled {
            self.sync_focus(window, cx);
            if action == Action::FocusAddress {
                self.open_address_menu();
            }
            if opens_find {
                self.focus_find(window, cx);
            }
            cx.notify();
        }
        handled
    }

    /// Open `url` (from a bookmark or history) in the active tab or a new one.
    pub fn open_url(&mut self, url: &str, new_tab: bool, window: &mut Window, cx: &mut Context<Self>) {
        if new_tab {
            self.browser.new_tab(url);
        } else {
            self.browser.load(url);
        }
        self.sync_focus(window, cx);
        cx.notify();
    }

    pub fn set_profile_label(&mut self, name: &str, cx: &mut Context<Self>) {
        self.profile_label = name.to_string();
        cx.notify();
    }

    pub fn apply_settings(&mut self, settings: &morph_core::settings::Settings, cx: &mut Context<Self>) {
        let strip = TabStrip::from_settings(settings);
        if strip != self.strip {
            self.strip = strip;
            cx.notify();
        }
        self.tabs_collapsed = settings.sidebar_collapsed;
        let theme = Theme::for_choice(settings.theme);
        if theme != self.theme {
            self.theme = theme;
            self.find_input.update(cx, |i, cx| i.set_theme(theme, cx));
        }
        self.browser.set_default_engine(&settings.search_engine);
        let dir = (!settings.downloads_dir.is_empty()).then(|| std::path::PathBuf::from(&settings.downloads_dir));
        self.browser.set_downloads_dir(dir);
        cx.notify();
    }

    pub fn set_settings_view(&mut self, view: gpui::AnyView, cx: &mut Context<Self>) {
        self.settings_view = Some(view);
        cx.notify();
    }

    pub fn set_profiles(&mut self, profiles: Vec<(String, String, bool)>, cx: &mut Context<Self>) {
        self.profiles = profiles;
        cx.notify();
    }

    pub fn clear_site_data(&mut self) {
        self.browser.clear_site_data();
    }

    pub fn set_caption(&mut self, caption: Option<WindowButtons>, cx: &mut Context<Self>) {
        if caption != self.caption {
            self.caption = caption;
            cx.notify();
        }
    }

    /// A press on the caption's empty space: a double click maximizes, a
    /// press and move drags the window (started on the move, so the window
    /// manager does not swallow the second click of a double click).
    fn caption_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, _cx: &mut Context<Self>) {
        if self.caption.is_none() {
            return;
        }
        self.caption_pressed = event.click_count < 2;
        if event.click_count >= 2 {
            if self.caption == Some(WindowButtons::Native) {
                // What the user set for a title bar double click (zoom or minimize).
                window.titlebar_double_click();
            } else {
                window.zoom_window();
            }
        }
    }

    /// Room for the macOS traffic lights at the start of the top row.
    fn traffic_lights(&self) -> bool {
        self.caption == Some(WindowButtons::Native)
    }

    fn caption_mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, _cx: &mut Context<Self>) {
        if self.caption_pressed {
            self.caption_pressed = false;
            if event.pressed_button == Some(MouseButton::Left) {
                window.start_window_move();
            }
        }
    }

    fn caption_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.caption_pressed = false;
    }

    /// Minimize, maximize and close (or close alone on tiling desktops).
    fn window_controls(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let buttons = self.caption.filter(|b| *b != WindowButtons::Native)?;
        let t = self.theme;
        // The hover colour is the control's own: GPUI takes one hover style.
        let control = |id: &'static str, label: &'static str, hover: u32| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .w(px(32.))
                .h(px(26.))
                .rounded(px(6.))
                .text_color(rgb(t.text_secondary))
                .hover(move |s| s.bg(rgb(hover)).text_color(rgb(t.text_primary)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(label)
        };
        let mut row = div().flex().flex_row().items_center().gap(px(2.)).pl(px(8.));
        if buttons == WindowButtons::All {
            row = row
                .child(
                    control("window-minimize", "—", t.surface_variant)
                        .window_control_area(gpui::WindowControlArea::Min)
                        .on_click(|_, window, _| window.minimize_window()),
                )
                .child(
                    control("window-maximize", "□", t.surface_variant)
                        .window_control_area(gpui::WindowControlArea::Max)
                        .on_click(|_, window, _| window.zoom_window()),
                );
        }
        let _ = cx;
        // Windows answers its own hit test with the control areas (snap
        // layouts on maximize); elsewhere the clicks do the work.
        Some(row.child(
            control("window-close", "×", t.error).window_control_area(gpui::WindowControlArea::Close).on_click(|_, window, _| window.remove_window()),
        ))
    }

    /// Show `text` at the bottom for a few seconds, with `action` offered.
    pub fn toast(&mut self, text: &str, action: Option<(&'static str, Action)>, cx: &mut Context<Self>) {
        self.toast = Some(Toast { text: text.to_string().into(), action, until: std::time::Instant::now() + TOAST_TIME });
        if !self.browser.draws_pages() {
            // Child-window pages draw over the shell: make room below them.
            cx.notify();
        }
        cx.notify();
    }

    pub fn set_marked(&mut self, marked: bool, cx: &mut Context<Self>) {
        if self.marked != marked {
            self.marked = marked;
            cx.notify();
        }
    }

    /// Give the page the keyboard (a panel beside it closed).
    pub fn focus_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.focus_page();
        self.sync_focus(window, cx);
        cx.notify();
    }

    /// Put the keyboard in the find bar with its text selected.
    fn focus_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.browser.find().map(|f| f.query.clone()).unwrap_or_default();
        self.find_input.update(cx, |input, cx| input.set_text(&query, cx));
        window.focus(&self.find_input.focus_handle(cx), cx);
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_input.read(cx).is_focused(window) {
            window.blur(cx);
        }
        self.browser.close_find();
        self.sync_focus(window, cx);
        cx.notify();
    }

    /// Pump the web views; returns shortcuts pressed while a page had focus.
    pub fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<String> {
        if self.toast.as_ref().is_some_and(|t| std::time::Instant::now() >= t.until) {
            self.toast = None;
            cx.notify();
        }
        self.run_test_menu(window, cx);
        let polled = self.browser.poll();
        if polled.changed {
            self.sync_focus(window, cx);
            cx.notify();
        }
        if self.browser.take_quit() {
            cx.emit(ShellRequest::Quit);
        }
        if let Some(multiple) = self.browser.take_file_request() {
            let answer = crate::dialogs::ask(cx, crate::dialogs::Ask::Open { multiple });
            self.file_dialog = Some(cx.spawn(async move |this, cx| {
                let paths = answer.await.ok().filter(|p| !p.is_empty());
                let _ = this.update(cx, |view, _| {
                    view.file_dialog = None;
                    view.browser.choose_files(paths);
                });
            }));
        }
        self.tick_downloads(cx);
        // Pages follow the shell's scale on either backend.
        self.browser.set_scale(f64::from(window.scale_factor()));
        if self.browser.draws_pages()
            && let Some(text) = self.browser.take_copied()
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        if self.browser.draws_pages()
            && let Some(frame) = self.browser.take_frame()
        {
            self.set_page_frame(frame, window);
            cx.notify();
        }
        polled.shortcuts
    }

    /// Ask where each new download goes, and hide the downloads bar a
    /// while after the last one ended.
    fn tick_downloads(&mut self, cx: &mut Context<Self>) {
        while let Some((id, name, dir)) = self.browser.take_download_ask() {
            let answer = crate::dialogs::ask(cx, crate::dialogs::Ask::Download { dir, name });
            self.download_dialogs.push(cx.spawn(async move |this, cx| {
                let path = answer.await.ok().and_then(|p| p.into_iter().next());
                let _ = this.update(cx, |view, cx| {
                    view.browser.save_download(id, path);
                    cx.notify();
                });
            }));
        }
        let downloads = self.browser.downloads();
        if downloads.is_empty() {
            // Every dialog answered (their tasks are done).
            self.download_dialogs.clear();
        }
        if downloads.is_empty() || downloads.iter().any(|d| d.running()) {
            self.downloads_done = None;
        } else if let Some(done) = self.downloads_done {
            if done.elapsed() >= DOWNLOADS_SHOWN {
                self.downloads_done = None;
                self.browser.clear_downloads();
                cx.notify();
            }
        } else {
            self.downloads_done = Some(std::time::Instant::now());
        }
    }

    /// The newest download, as a bar below the page: its progress, then
    /// where it was saved or why it failed.
    fn download_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use morph_browser::downloads::DownloadState;
        let t = self.theme;
        let d = self.browser.downloads().iter().rev().find(|d| d.state != DownloadState::Cancelled)?;
        let others = self.browser.downloads().iter().filter(|o| o.running() && o.id != d.id).count();
        let text = match &d.state {
            DownloadState::Running => {
                let amount = match d.total {
                    Some(total) => format!("{} of {}", size_text(d.received), size_text(total)),
                    None => size_text(d.received),
                };
                let pct = d.fraction().map(|f| format!("{:.0}% · ", f * 100.0)).unwrap_or_default();
                format!("Downloading {} — {pct}{amount}", d.name)
            }
            DownloadState::Saved(path) => format!(
                "Saved {} to {}",
                path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
                path.parent().map(|p| p.display().to_string()).unwrap_or_default()
            ),
            DownloadState::Failed(why) => format!("Download of {} failed: {why}", d.name),
            DownloadState::Cancelled => return None,
        };
        let text = if others > 0 { format!("{text} (+{others} more)") } else { text };
        let fraction = d.fraction().filter(|_| d.running());
        let mut bar = div()
            .id("downloads")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .h(px(32.))
            .bg(rgb(t.surface_variant))
            .text_size(px(13.))
            .text_color(rgb(t.text_primary))
            .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().child(text));
        if let Some(f) = fraction {
            bar = bar.child(div().absolute().left_0().bottom_0().h(px(2.)).w(gpui::relative(f)).bg(rgb(t.accent)));
        }
        if !d.running() {
            bar = bar.child(
                div().id("downloads-close").px(px(6.)).cursor_pointer().text_color(rgb(t.accent)).hover(|s| s.underline()).child("Dismiss").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.browser.clear_downloads();
                        cx.notify();
                    }),
                ),
            );
        }
        Some(bar.into_any_element())
    }

    /// Show a new frame of the page (and free the old one's texture).
    fn set_page_frame(&mut self, frame: morph_webview::Frame, window: &mut Window) {
        let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels) else {
            return;
        };
        // The pixels are BGRA already, which is what GPUI's images hold.
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        if let Some(old) = self.page_image.replace(image) {
            let _ = window.drop_image(old);
        }
    }

    /// A pointer press in a page the shell draws: the page takes the
    /// keyboard and gets the press.
    fn page_mouse_down(&mut self, event: &MouseDownEvent, origin: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.focus_page();
        self.sync_focus(window, cx);
        window.focus(&self.page_focus, cx);
        self.page_buttons += 1;
        let (x, y) = local(event.position, origin);
        let modifiers = crate::page_input::modifiers(&event.modifiers);
        self.browser.page_input(Input::PointerButton { x, y, button: crate::page_input::button(event.button), pressed: true, modifiers });
        // Pages are drawn by the shell only on Linux (the primary selection
        // is Linux's too).
        #[cfg(target_os = "linux")]
        if event.button == MouseButton::Middle && self.browser.draws_pages() && self.browser.page_cursor() == PageCursor::Text {
            // The page does not paste the primary selection itself (see
            // morph_webview::clipboard): a click puts the caret there and
            // the shell pastes it.
            let left = morph_webview::MouseButton::Left;
            self.browser.page_input(Input::PointerButton { x, y, button: left, pressed: true, modifiers });
            self.browser.page_input(Input::PointerButton { x, y, button: left, pressed: false, modifiers });
            if let Some(text) = cx.read_from_primary().and_then(|item| item.text()).filter(|t| !t.is_empty()) {
                self.browser.page_input(Input::Paste(morph_webview::clipboard::Clipboard { text: Some(text), image: None }));
            }
        }
        cx.notify();
    }

    fn page_mouse_up(&mut self, event: &MouseUpEvent, origin: Point<Pixels>) {
        self.page_buttons = self.page_buttons.saturating_sub(1);
        let (x, y) = local(event.position, origin);
        let modifiers = crate::page_input::modifiers(&event.modifiers);
        self.browser.page_input(Input::PointerButton { x, y, button: crate::page_input::button(event.button), pressed: false, modifiers });
    }

    fn page_mouse_move(&mut self, event: &MouseMoveEvent, origin: Point<Pixels>, inside: bool) {
        if inside || self.page_buttons > 0 {
            self.page_pointer = true;
            let (x, y) = local(event.position, origin);
            self.page_pointer_at = (x, y);
            self.browser.page_input(Input::PointerMove { x, y, modifiers: crate::page_input::modifiers(&event.modifiers) });
        } else if self.page_pointer {
            self.page_pointer = false;
            self.browser.page_input(Input::PointerLeave);
        }
    }

    fn page_scroll(&mut self, event: &ScrollEvent, origin: Point<Pixels>) {
        let (x, y) = local(event.position, origin);
        let (dx, dy, precise) = crate::page_input::scroll(&event.delta);
        let modifiers = crate::page_input::modifiers(&event.modifiers);
        self.browser.page_input(Input::Scroll { x, y, dx, dy, precise, modifiers });
    }

    /// Keys the shell's bindings did not take go to the page, as long as
    /// it has the keyboard (a click on the toolbar takes it back).
    fn page_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.browser.option_menu().is_some() {
            // A <select>'s list is open: its keys are the list's.
            if let Some(key) = menu_view::menu_key(&event.keystroke) {
                self.browser.menu_key(key);
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if !self.browser.page_has_keyboard() {
            return;
        }
        if let Some(input) = crate::page_input::key(&event.keystroke, true, crate::page_input::current_raw_key()) {
            match &input {
                Input::Key(key) if let Some(clip) = morph_webview::clipboard::clipboard_key(key) => self.page_clipboard(clip, cx),
                _ => self.browser.page_input(input),
            }
            cx.stop_propagation();
        }
    }

    /// Copy, cut or paste in a page the shell draws, with the shell's
    /// clipboard (WebKit's crashed off screen: see morph_webview::clipboard).
    fn page_clipboard(&mut self, key: morph_webview::clipboard::ClipboardKey, cx: &mut Context<Self>) {
        use morph_webview::clipboard::{Clipboard, ClipboardKey};
        match key {
            ClipboardKey::Paste => {
                let Some(item) = cx.read_from_clipboard() else {
                    return;
                };
                let mut clip = Clipboard::default();
                for entry in item.entries() {
                    match entry {
                        ClipboardEntry::String(s) if clip.text.is_none() => clip.text = Some(s.text().clone()),
                        ClipboardEntry::Image(image) if clip.image.is_none() => {
                            clip.image = Some((image.format.mime_type().to_string(), image.bytes.clone()))
                        }
                        _ => {}
                    }
                }
                if clip.text.is_some() || clip.image.is_some() {
                    self.browser.page_input(Input::Paste(clip));
                }
            }
            ClipboardKey::Copy => self.browser.page_input(Input::Copy { cut: false }),
            ClipboardKey::Cut => self.browser.page_input(Input::Copy { cut: true }),
        }
    }

    fn page_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.browser.option_menu().is_some() {
            cx.stop_propagation();
            return;
        }
        if !self.browser.page_has_keyboard() {
            return;
        }
        if let Some(input) = crate::page_input::key(&event.keystroke, false, crate::page_input::current_raw_key()) {
            self.browser.page_input(input);
            cx.stop_propagation();
        }
    }

    fn page_modifiers_changed(&mut self, event: &ModifiersChangedEvent, _: &mut Window, _: &mut Context<Self>) {
        let before = std::mem::replace(&mut self.page_modifiers, event.modifiers);
        if self.browser.page_has_keyboard() {
            for input in crate::page_input::modifier_keys(&before, &event.modifiers) {
                self.browser.page_input(input);
            }
        }
    }

    pub fn set_shown(&mut self, shown: bool) {
        self.shell_shown = shown;
        if self.shell_menu.as_ref().is_none_or(|m| !m.hid_page) && self.address_menu.as_ref().is_none_or(|m| !m.hid_page) {
            self.browser.set_shown(shown);
        }
    }

    /// Mirror the browser's address-bar focus into GPUI's focus (and, for
    /// pages the shell draws, the page's).
    fn sync_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.browser.address_focused();
        if want && !self.address_focus.is_focused(window) {
            window.focus(&self.address_focus, cx);
        } else if !want && self.mirrored_focus && self.address_focus.is_focused(window) {
            window.blur(cx);
        }
        self.mirrored_focus = want;
        if !want && self.browser.draws_pages() && self.browser.page_has_keyboard() && !self.page_focus.is_focused(window) {
            window.focus(&self.page_focus, cx);
        }
    }

    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut morph_ui::text::TextField)) {
        if let Some(menu) = self.address_menu.as_mut() {
            // Typing or moving the caret leaves the menu's entries.
            menu.highlighted = None;
        }
        f(self.browser.address_mut());
        cx.notify();
    }

    fn index_for_point(&self, position: Point<Pixels>) -> usize {
        let text = self.browser.address().text();
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        if text.is_empty() || line.text.as_ref() != text {
            return 0;
        }
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return text.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn on_address_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        let index = self.index_for_point(event.position);
        if !self.browser.address_focused() {
            // First click focuses and selects everything, as browsers do,
            // and opens the address menu.
            self.browser.focus_address();
            self.sync_focus(window, cx);
            self.open_address_menu();
        } else if event.modifiers.shift {
            self.browser.address_mut().select_to(index);
            self.selecting = true;
        } else {
            self.browser.address_mut().move_to(index);
            self.selecting = true;
        }
        cx.notify();
    }

    fn on_address_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            let index = self.index_for_point(event.position);
            self.edit(cx, |f| f.select_to(index));
        }
    }

    fn on_address_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.address_menu.as_ref().and_then(|m| m.highlighted) {
            if let Some(item) = address_items(&self.profiles).get(i).cloned() {
                self.activate_address_item(item, window, cx);
            }
            return;
        }
        // Show the page again before it takes the keyboard.
        self.close_address_menu(cx);
        let internal = self.browser.submit_address();
        self.sync_focus(window, cx);
        if let Some(internal) = internal {
            // morph://bookmarks and the like open the shell's panels.
            cx.emit(ShellRequest::Run(internal.action()));
        }
        cx.notify();
    }

    /// The shortcut shown beside each menu entry (`Ctrl+Shift+O`, `⌘⇧O`).
    pub fn set_shortcut_labels(&mut self, labels: HashMap<Action, String>) {
        self.shortcut_labels = labels;
    }

    fn label_for(&self, action: Action) -> SharedString {
        self.shortcut_labels.get(&action).cloned().unwrap_or_default().into()
    }

    /// Open a shell menu at `at` (window coordinates) and give it the keyboard.
    fn open_shell_menu(
        &mut self,
        id: &'static str,
        entries: Vec<(Row, MenuCommand)>,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_shell_menu(window, cx);
        let items = entries
            .iter()
            .map(|(row, _)| morph_webview::MenuItem { label: row.label.to_string(), enabled: row.enabled, ..Default::default() })
            .collect();
        let mut state = MenuState::new(morph_webview::OptionMenu { items, anchor: Rect::default() });
        state.highlighted = None;
        let hid_page = !self.browser.draws_pages();
        if hid_page {
            self.browser.set_shown(false);
        }
        let (rows, commands) = entries.into_iter().unzip();
        self.shell_menu = Some(ShellMenu { id, state, rows, commands, at, hid_page });
        self.browser.focus_shell();
        window.focus(&self.shell_menu_focus, cx);
        cx.notify();
    }

    fn close_shell_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.shell_menu.take() {
            if menu.hid_page && self.address_menu.as_ref().is_none_or(|m| !m.hid_page) {
                self.browser.set_shown(self.shell_shown);
            }
            if self.shell_menu_focus.is_focused(window) {
                window.blur(cx);
            }
            cx.notify();
        }
    }

    /// The ☰ button: the main menu under it, at the window's right edge.
    fn open_main_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = MAIN_MENU
            .iter()
            .map(|&(label, action, separated)| {
                let mut row = Row::new(label).detail(self.label_for(action));
                row.separator = separated;
                (row, MenuCommand::Run(action))
            })
            .collect();
        let width = window.viewport_size().width;
        let y = match self.strip {
            TabStrip::Top => TAB_STRIP_HEIGHT + TOOLBAR_HEIGHT,
            _ => TOOLBAR_HEIGHT,
        };
        let at = point(width - px(MENU_WIDTH + 8.), px(y - 4.));
        self.open_shell_menu("main-menu", entries, at, window, cx);
    }

    /// A right click on a tab: what can be done with it.
    fn open_tab_menu(&mut self, tab: TabId, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let entries = vec![
            (Row::new("New tab").detail(self.label_for(Action::NewTab)), MenuCommand::Run(Action::NewTab)),
            (Row::new("Reload").separated(), MenuCommand::Reload(tab)),
            (Row::new("Duplicate"), MenuCommand::Duplicate(tab)),
            (Row::new("Close tab").separated().detail(self.label_for(Action::CloseTab)), MenuCommand::Close(tab)),
            (Row::new("Close other tabs"), MenuCommand::CloseOthers(tab)),
            (Row::new("Bookmarks").separated().detail(self.label_for(Action::ShowBookmarks)), MenuCommand::Run(Action::ShowBookmarks)),
            (Row::new("History").detail(self.label_for(Action::ShowHistory)), MenuCommand::Run(Action::ShowHistory)),
        ];
        self.open_shell_menu("tab-menu", entries, at, window, cx);
    }

    /// One step of `MORPH_TEST_MENU`: open the main menu, then (next tick)
    /// choose the asked entry the way a click does.
    fn run_test_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(label) = self.test_menu.last().cloned() else {
            return;
        };
        match &self.shell_menu {
            Some(menu) if menu.id == "main-menu" => {
                self.test_menu.pop();
                if let Some(i) = MAIN_MENU.iter().position(|e| e.0 == label) {
                    self.choose_shell_menu(i, window, cx);
                }
            }
            _ => self.open_main_menu(window, cx),
        }
    }

    /// Run shell menu row `i` and close the menu.
    fn choose_shell_menu(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(command) = self.shell_menu.as_ref().and_then(|m| m.state.choosable(i).then(|| m.commands.get(i).cloned()).flatten()) else {
            return;
        };
        self.close_shell_menu(window, cx);
        match command {
            MenuCommand::Run(action) => cx.emit(ShellRequest::Run(action)),
            MenuCommand::Duplicate(tab) => self.browser.duplicate_tab(tab),
            MenuCommand::Reload(tab) => self.browser.reload_tab(tab),
            MenuCommand::Close(tab) => self.browser.close_tab(tab),
            MenuCommand::CloseOthers(tab) => self.browser.close_other_tabs(tab),
            MenuCommand::Position(position) => cx.emit(ShellRequest::Change(SettingsChange::Position(position))),
            MenuCommand::Engine(id) => self.browser.set_tab_engine(id),
        }
        self.sync_focus(window, cx);
        cx.notify();
    }

    fn shell_menu_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = menu_view::menu_key(&event.keystroke) else {
            return;
        };
        cx.stop_propagation();
        let Some(menu) = self.shell_menu.as_mut() else {
            return;
        };
        match menu.state.key(key, std::time::Instant::now()) {
            Some(morph_webview::MenuChoice::Activate(i)) => self.choose_shell_menu(i, window, cx),
            Some(morph_webview::MenuChoice::Cancel) => {
                self.close_shell_menu(window, cx);
                self.browser.focus_page();
                self.sync_focus(window, cx);
            }
            Some(morph_webview::MenuChoice::Highlight(i)) => {
                self.shell_menu_scroll.scroll_to_item(i);
                cx.notify();
            }
            None => {}
        }
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.close_address_menu(cx);
        self.browser.blur_address();
        self.browser.focus_page();
        self.sync_focus(window, cx);
        cx.notify();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.edit(cx, |f| f.replace(None, &text));
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.browser.address().selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.browser.address().selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            self.edit(cx, |f| f.replace(None, ""));
        }
    }

    fn button(&self, id: &'static str, label: &'static str, enabled: bool) -> gpui::Stateful<gpui::Div> {
        let t = self.theme;
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .w(px(30.))
            .h(px(28.))
            .rounded(px(6.))
            .text_color(rgb(if enabled { t.text_primary } else { t.border }))
            .when(enabled, |d| d.hover(|s| s.bg(rgb(t.surface_variant))).cursor_pointer())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(label)
    }

    /// A tab's icon: the site's, a letter while it has none, `…` loading.
    fn tab_icon(&self, tab: &morph_browser::TabLabel) -> gpui::AnyElement {
        let t = self.theme;
        let frame = div().flex_none().size(px(16.)).flex().items_center().justify_center();
        if let Some(bytes) = &tab.icon {
            let image = self
                .icon_images
                .borrow_mut()
                .entry(bytes.clone())
                .or_insert_with(|| Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes.to_vec())))
                .clone();
            return frame.child(gpui::img(image).size(px(16.))).into_any_element();
        }
        let mark: SharedString = if tab.loading {
            "…".into()
        } else if tab.own {
            "M".into()
        } else {
            tab.label.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "·".into()).into()
        };
        frame
            .rounded(px(4.))
            .bg(rgb(t.surface_variant))
            .text_size(px(10.))
            .text_color(rgb(if tab.own { t.accent } else { t.text_secondary }))
            .child(mark)
            .into_any_element()
    }

    /// One tab: a rounded header in a row or a line in the side list; just
    /// its icon when the tabs are collapsed (the title is its tooltip).
    fn tab_item(&self, index: usize, tab: morph_browser::TabLabel, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let t = self.theme;
        let id = tab.id;
        let collapsed = self.tabs_collapsed;
        let icon = self.tab_icon(&tab);
        let title: SharedString = tab.label.clone().into();
        let item = div()
            .id(("tab", index))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .h(px(30.))
            .bg(rgb(if tab.active { t.surface } else { t.background }))
            .text_color(rgb(if tab.active { t.text_primary } else { t.text_secondary }))
            .text_size(px(13.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_tab_menu(id, event.position, window, cx);
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.browser.select(id);
                this.sync_focus(window, cx);
                cx.notify();
            }));
        if collapsed {
            let item = item.justify_center().w(px(34.)).flex_none().tooltip(tip(title, t)).child(icon);
            return match self.strip {
                TabStrip::Top => item.rounded_t(px(8.)),
                TabStrip::Bottom => item.rounded_b(px(8.)),
                TabStrip::Side { .. } => item.rounded(px(6.)),
            };
        }
        let item = match self.strip {
            TabStrip::Top => item.min_w(px(80.)).max_w(px(220.)).px(px(10.)).rounded_t(px(8.)),
            TabStrip::Bottom => item.min_w(px(80.)).max_w(px(220.)).px(px(10.)).rounded_b(px(8.)),
            TabStrip::Side { .. } => item.w_full().px(px(8.)).rounded(px(6.)),
        };
        item.child(icon).child(div().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().child(title)).child(
            div()
                .id(("close", index))
                .px(px(4.))
                .rounded(px(4.))
                .hover(|s| s.bg(rgb(t.surface_variant)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.browser.close_tab(id);
                    this.sync_focus(window, cx);
                    cx.notify();
                }))
                .child("×"),
        )
    }

    /// A tab-bar button the keyboard reaches too (Tab, then Enter or Space).
    #[allow(clippy::too_many_arguments)]
    fn bar_button(
        &self,
        id: &'static str,
        label: &'static str,
        tooltip: &'static str,
        focus: &FocusHandle,
        window: &Window,
        cx: &mut Context<Self>,
        run: fn(&mut Self, Bounds<Pixels>, &mut Window, &mut Context<Self>),
    ) -> gpui::Stateful<gpui::Div> {
        let t = self.theme;
        let focused = focus.is_focused(window);
        let at = move |this: &Self| this.button_bounds.borrow().get(id).copied().unwrap_or_default();
        let entity = cx.entity();
        self.button(id, label, true)
            .relative()
            .track_focus(focus)
            .key_context(BAR_BUTTON_CONTEXT)
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                this.browser.focus_page();
                this.sync_focus(window, cx);
                window.blur(cx);
            }))
            .when(focused, |d| d.border_1().border_color(rgb(t.accent)))
            .tooltip(tip(tooltip, t))
            // A click, or Enter or Space while focused (GPUI clicks focused
            // elements from the keyboard).
            .on_click(cx.listener(move |this, _, window, cx| {
                let bounds = at(this);
                log_bar(id);
                run(this, bounds, window, cx);
            }))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        entity.read(cx).button_bounds.borrow_mut().insert(id, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }

    fn new_tab_button(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        self.button("new-tab", "+", true).on_click(cx.listener(|this, _, window, cx| {
            this.dispatch(Action::NewTab, window, cx);
        }))
    }

    /// The buttons after the tabs: new tab, where the tabs go, collapse.
    fn tab_bar_buttons(&self, window: &Window, cx: &mut Context<Self>) -> (gpui::Stateful<gpui::Div>, gpui::Stateful<gpui::Div>) {
        let position = self.bar_button("tab-position", "⊞", "Tab position", &self.position_focus, window, cx, |this, at, window, cx| {
            this.open_position_menu(at, window, cx)
        });
        let (glyph, tooltip) = match (self.strip, self.tabs_collapsed) {
            (TabStrip::Side { left: false, .. }, false) => ("»", "Collapse tabs"),
            (TabStrip::Side { left: false, .. }, true) => ("«", "Expand tabs"),
            (_, false) => ("«", "Collapse tabs"),
            (_, true) => ("»", "Expand tabs"),
        };
        let collapse = self.bar_button("tab-collapse", glyph, tooltip, &self.collapse_focus, window, cx, |_, _, _, cx| {
            cx.emit(ShellRequest::Change(SettingsChange::ToggleCollapsed));
        });
        (position, collapse)
    }

    /// The tab-position menu, under its button.
    fn open_position_menu(&mut self, at: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.strip.position();
        let entries = [("Top", TabPosition::Top), ("Bottom", TabPosition::Bottom), ("Left", TabPosition::Left), ("Right", TabPosition::Right)]
            .into_iter()
            .map(|(label, position)| (Row { checked: position == current, ..Row::new(label) }, MenuCommand::Position(position)))
            .collect();
        self.open_shell_menu("position-menu", entries, at.bottom_left(), window, cx);
    }

    /// The search engine menu, under the selector left of the address.
    fn open_engine_menu(&mut self, at: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.browser.tab_engine().id;
        let mut entries: Vec<(Row, MenuCommand)> =
            morph_core::search::ENGINES.iter().map(|e| (Row { checked: e.id == current, ..Row::new(e.name) }, MenuCommand::Engine(e.id))).collect();
        entries.push((Row::new("Default search engine settings…").separated(), MenuCommand::Run(Action::ShowSearchSettings)));
        self.open_shell_menu("engine-menu", entries, at.bottom_left(), window, cx);
    }

    /// The tabs as a row (above the toolbar or below the page).
    fn tab_strip(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let tabs = self.browser.tabs();
        let (position, collapse) = self.tab_bar_buttons(window, cx);
        div()
            .id("tab-strip")
            .flex()
            .flex_row()
            .when(self.strip == TabStrip::Bottom, |d| d.items_start())
            .when(self.strip != TabStrip::Bottom, |d| d.items_end())
            .h(px(TAB_STRIP_HEIGHT))
            .px(px(6.))
            .when(self.strip == TabStrip::Top && self.traffic_lights(), |d| d.pl(px(morph_ui::chrome::TRAFFIC_LIGHTS_INSET)))
            .gap(px(2.))
            .bg(rgb(t.background))
            .children(tabs.into_iter().enumerate().map(|(index, tab)| self.tab_item(index, tab, cx)))
            .child(self.new_tab_button(cx))
            .child(position)
            .when(self.strip == TabStrip::Top, |d| {
                d.on_mouse_down(MouseButton::Left, cx.listener(Self::caption_mouse_down))
                    .on_mouse_move(cx.listener(Self::caption_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::caption_mouse_up))
            })
            .child(
                div()
                    .id("tab-strip-space")
                    .flex_1()
                    .h_full()
                    .when(self.strip == TabStrip::Top, |d| d.window_control_area(gpui::WindowControlArea::Drag)),
            )
            .child(collapse)
            .when(self.strip == TabStrip::Top, |d| d.children(self.window_controls(cx).map(|c| c.h_full().items_center())))
    }

    /// The tabs as a vertical list beside the toolbar and page.
    fn tab_list(&self, width: u32, collapsed: bool, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let tabs = self.browser.tabs();
        let (position, collapse) = self.tab_bar_buttons(window, cx);
        let buttons = div().flex().gap(px(2.)).when(collapsed, |d| d.flex_col().items_center()).when(!collapsed, |d| d.flex_row());
        div()
            .flex()
            .flex_col()
            .w(px(width as f32))
            .h_full()
            .bg(rgb(t.background))
            .when(self.traffic_lights() && matches!(self.strip, TabStrip::Side { left: true, .. }), |d| d.pt(px(28.)))
            .child(
                div()
                    .id("tab-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .p(px(6.))
                    .gap(px(2.))
                    .when(collapsed, |d| d.items_center())
                    .overflow_y_scroll()
                    .children(tabs.into_iter().enumerate().map(|(index, tab)| self.tab_item(index, tab, cx)))
                    .child(buttons.child(self.new_tab_button(cx)).child(position)),
            )
            .child(div().flex().flex_row().p(px(6.)).when(collapsed, |d| d.justify_center()).when(!collapsed, |d| d.justify_end()).child(collapse))
    }

    fn toolbar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let state = self.browser.toolbar();
        let focused = self.browser.address_focused();
        let engine = self.browser.tab_engine();
        let engine_button = self
            .bar_button("search-engine", engine.mark, "Search engine", &self.engine_focus, window, cx, |this, at, window, cx| {
                this.open_engine_menu(at, window, cx)
            })
            .text_size(px(12.))
            .text_color(rgb(t.accent));
        let entity = cx.entity();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .h(px(TOOLBAR_HEIGHT))
            .px(px(8.))
            .when(self.traffic_lights() && matches!(self.strip, TabStrip::Bottom | TabStrip::Side { left: false, .. }), |d| {
                d.pl(px(morph_ui::chrome::TRAFFIC_LIGHTS_INSET))
            })
            .bg(rgb(t.surface))
            .when(!self.strip.is_caption(), |d| {
                d.on_mouse_down(MouseButton::Left, cx.listener(Self::caption_mouse_down))
                    .on_mouse_move(cx.listener(Self::caption_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::caption_mouse_up))
            })
            // Windows moves a window only through its hit test: an empty
            // grip at the start of the toolbar when the toolbar is the top row.
            .when(cfg!(windows) && !self.strip.is_caption(), |d| {
                d.child(div().id("toolbar-grip").w(px(14.)).h_full().window_control_area(gpui::WindowControlArea::Drag))
            })
            .child(self.button("back", "←", state.can_back).on_click(cx.listener(|this, _, w, cx| {
                this.dispatch(Action::Back, w, cx);
            })))
            .child(self.button("forward", "→", state.can_forward).on_click(cx.listener(|this, _, w, cx| {
                this.dispatch(Action::Forward, w, cx);
            })))
            .child(if state.loading {
                self.button("stop", "✕", true).on_click(cx.listener(|this, _, w, cx| {
                    this.dispatch(Action::Stop, w, cx);
                }))
            } else {
                self.button("reload", "↻", true).on_click(cx.listener(|this, _, w, cx| {
                    this.dispatch(Action::Reload, w, cx);
                }))
            })
            .child(engine_button)
            .child(
                div()
                    .id("address")
                    .relative()
                    .flex_1()
                    .flex()
                    .items_center()
                    .h(px(30.))
                    .px(px(10.))
                    .rounded(px(15.))
                    .bg(rgb(t.background))
                    .border_1()
                    .border_color(rgb(if focused { t.accent } else { t.surface_variant }))
                    .text_color(rgb(t.text_primary))
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .key_context(ADDRESS_CONTEXT)
                    .track_focus(&self.address_focus)
                    .cursor(CursorStyle::IBeam)
                    .on_action(cx.listener(|this, _: &Backspace, _, cx| this.edit(cx, |f| f.backspace())))
                    .on_action(cx.listener(|this, _: &Delete, _, cx| this.edit(cx, |f| f.delete())))
                    .on_action(cx.listener(|this, _: &Left, _, cx| this.edit(cx, |f| f.left())))
                    .on_action(cx.listener(|this, _: &Right, _, cx| this.edit(cx, |f| f.right())))
                    .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.edit(cx, |f| f.select_left())))
                    .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.edit(cx, |f| f.select_right())))
                    .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.edit(cx, |f| f.select_all())))
                    .on_action(cx.listener(|this, _: &Home, _, cx| this.edit(cx, |f| f.home())))
                    .on_action(cx.listener(|this, _: &End, _, cx| this.edit(cx, |f| f.end())))
                    .on_action(cx.listener(Self::paste))
                    .on_action(cx.listener(Self::copy))
                    .on_action(cx.listener(Self::cut))
                    .on_action(cx.listener(Self::submit))
                    .on_action(cx.listener(Self::cancel))
                    .on_action(cx.listener(|this, _: &MenuDown, _, cx| this.step_address_menu(true, cx)))
                    .on_action(cx.listener(|this, _: &MenuUp, _, cx| this.step_address_menu(false, cx)))
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                entity.read(cx).button_bounds.borrow_mut().insert("address", bounds);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_address_mouse_down))
                    .on_mouse_move(cx.listener(Self::on_address_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_address_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_address_mouse_up))
                    .child(AddressElement { view: cx.entity() }),
            )
            .when(!self.browser.active_url().is_empty(), |d| {
                let marked = self.marked;
                d.child(
                    self.button("bookmark", if marked { "★" } else { "☆" }, true)
                        .text_color(rgb(if marked { t.accent } else { t.text_primary }))
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(ShellRequest::Run(Action::Bookmark));
                        })),
                )
            })
            .when(!self.profile_label.is_empty(), |d| {
                let label: SharedString = self.profile_label.clone().into();
                d.child(
                    div()
                        .id("profile")
                        .px(px(10.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .rounded(px(14.))
                        .bg(rgb(t.surface_variant))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(label)
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(ShellRequest::Run(Action::ShowProfiles));
                        })),
                )
            })
            .when(state.popups > 0, |d| {
                let label: SharedString = if state.popups == 1 { "1 popup".into() } else { format!("{} popups", state.popups).into() };
                d.child(div().px(px(8.)).text_size(px(12.)).text_color(rgb(t.accent_alt)).child(label))
            })
            .child(
                self.button("main-menu-button", "☰", true)
                    .when(self.shell_menu.as_ref().is_some_and(|m| m.id == "main-menu"), |d| d.bg(rgb(t.surface_variant)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.shell_menu.as_ref().is_some_and(|m| m.id == "main-menu") {
                            this.close_shell_menu(window, cx);
                        } else {
                            this.open_main_menu(window, cx);
                        }
                    })),
            )
            .when(!self.strip.is_caption(), |d| d.children(self.window_controls(cx)))
    }

    /// The find bar: a row between the toolbar and the page, so the page
    /// moves down instead of being covered.
    fn find_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let t = self.theme;
        let find = self.browser.find()?;
        let status: SharedString = match find.matches {
            _ if find.query.is_empty() => "".into(),
            None => "…".into(),
            Some(0) => "No matches".into(),
            Some(1) => "1 match".into(),
            Some(n) if n >= 1000 => "1000+ matches".into(),
            Some(n) => format!("{n} matches").into(),
        };
        let found = !find.query.is_empty() && find.matches != Some(0);
        Some(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .h(px(FIND_BAR_HEIGHT))
                .px(px(8.))
                .bg(rgb(t.surface))
                .border_t_1()
                .border_color(rgb(t.surface_variant))
                .child(div().w(px(320.)).flex().child(self.find_input.clone()))
                .child(
                    div()
                        .min_w(px(96.))
                        .text_size(px(12.))
                        .text_color(rgb(if find.matches == Some(0) && !find.query.is_empty() { t.error } else { t.text_secondary }))
                        .child(status),
                )
                .child(self.button("find-previous", "↑", found).on_click(cx.listener(|this, _, _, cx| {
                    this.browser.find_next(false);
                    cx.notify();
                })))
                .child(self.button("find-next", "↓", found).on_click(cx.listener(|this, _, _, cx| {
                    this.browser.find_next(true);
                    cx.notify();
                })))
                .child(div().flex_1())
                .child(self.button("find-close", "×", true).on_click(cx.listener(|this, _, window, cx| {
                    this.close_find(window, cx);
                }))),
        )
    }

    fn panel_button(&self, id: &'static str, label: &'static str, primary: bool) -> gpui::Stateful<gpui::Div> {
        let t = self.theme;
        div()
            .id(id)
            .px(px(14.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(rgb(if primary { t.accent } else { t.surface_variant }))
            .text_color(rgb(if primary { t.background } else { t.text_primary }))
            .cursor_pointer()
            .child(label)
    }

    /// What Morph shows instead of a page that crashed, hung or failed to
    /// load, Morph's own pages, and the start screen of a tab with nothing
    /// in it. The web view is hidden meanwhile.
    fn cover_panel(&self, cover: &Cover, cx: &mut Context<Self>) -> gpui::AnyElement {
        if *cover == Cover::About {
            return self.about_panel(cx);
        }
        if *cover == Cover::Start {
            return crate::brand::start_screen(self.theme).id("start-page").into_any_element();
        }
        if *cover == Cover::Settings {
            return match &self.settings_view {
                Some(view) => div().absolute().size_full().child(view.clone()).into_any_element(),
                None => div().into_any_element(),
            };
        }
        let t = self.theme;
        let reload = |label| {
            self.panel_button("cover-reload", label, true).on_click(cx.listener(|this, _, w, cx| {
                this.dispatch(Action::Reload, w, cx);
            }))
        };
        let (title, detail, buttons): (String, String, Vec<gpui::AnyElement>) = match cover {
            Cover::Crashed => {
                ("This page stopped working.".into(), "Its process ended. Reload to try again.".into(), vec![reload("Reload").into_any_element()])
            }
            Cover::Unresponsive => (
                "This page isn't responding".into(),
                "You can wait for it to answer, or reload it.".into(),
                vec![
                    self.panel_button("cover-wait", "Wait", false)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.browser.wait();
                            cx.notify();
                        }))
                        .into_any_element(),
                    reload("Reload").into_any_element(),
                ],
            ),
            Cover::Failed(error) => {
                let (title, detail) = morph_browser::error_text(error);
                (title, detail, vec![reload("Try again").into_any_element()])
            }
            Cover::About | Cover::Settings | Cover::Start => unreachable!("drawn above"),
        };
        let url = match cover {
            Cover::Failed(error) => error.url.clone(),
            _ => String::new(),
        };
        div()
            .absolute()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .p(px(24.))
            .text_color(rgb(t.text_primary))
            .child(div().text_size(px(20.)).child(title))
            .child(div().max_w(px(520.)).text_size(px(14.)).text_color(rgb(t.text_secondary)).child(detail))
            .when(!url.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(url)))
            .child(div().flex().flex_row().gap(px(8.)).children(buttons))
            .into_any_element()
    }

    /// What draws over everything: a shell menu or a `<select>`'s list
    /// (with a backdrop that closes it on a click elsewhere) and the
    /// page's tooltip.
    fn overlays(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let mut out = Vec::new();
        let area = self.browser.page_area();
        if self.shell_menu.is_some() || self.browser.option_menu().is_some() {
            out.push(
                deferred(
                    div()
                        .id("menu-backdrop")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.close_shell_menu(window, cx);
                                this.browser.menu_cancel();
                                cx.notify();
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, window, cx| {
                                this.close_shell_menu(window, cx);
                                this.browser.menu_cancel();
                                cx.notify();
                            }),
                        )
                        .on_scroll(|_, _, cx| cx.stop_propagation()),
                )
                .with_priority(1)
                .into_any_element(),
            );
        }
        if self.address_menu.is_some() && self.shell_menu.is_none() {
            out.extend(self.address_menu_overlay(cx));
        }
        if let Some(menu) = &self.shell_menu {
            let natural = menu_view::natural_height(&menu.rows);
            let this = cx.entity();
            let hover: menu_view::RowHandler = Rc::new({
                let this = this.clone();
                move |i, _, cx| {
                    this.update(cx, |view, cx| {
                        if let Some(m) = view.shell_menu.as_mut()
                            && m.state.hover(i).is_some()
                        {
                            cx.notify();
                        }
                    })
                }
            });
            let click: menu_view::RowHandler = Rc::new(move |i, window, cx| this.update(cx, |view, cx| view.choose_shell_menu(i, window, cx)));
            let list = menu_view::menu(t, menu.id, &menu.rows, menu.state.highlighted, MENU_WIDTH, natural, &self.shell_menu_scroll, hover, click)
                .track_focus(&self.shell_menu_focus)
                .key_context("ShellMenu")
                .on_key_down(cx.listener(Self::shell_menu_key));
            out.push(deferred(anchored().position(menu.at).snap_to_window_with_margin(px(4.)).child(list)).with_priority(2).into_any_element());
        }
        if let Some(menu) = self.browser.option_menu() {
            let a = menu.menu.anchor;
            let rows: Vec<Row> = menu
                .menu
                .items
                .iter()
                .map(|item| Row {
                    label: item.label.clone().into(),
                    detail: SharedString::default(),
                    enabled: item.enabled,
                    header: item.group_label,
                    indent: item.group_child,
                    separator: false,
                    checked: item.selected,
                })
                .collect();
            let anchor = ((area.x + a.x) as f32, (area.y + a.y) as f32, a.width as f32, a.height as f32);
            let (x, y, w, h) =
                menu_view::place(anchor, (area.x as f32, area.y as f32, area.width as f32, area.height as f32), menu_view::natural_height(&rows));
            if let Some(i) = menu.highlighted
                && self.select_scrolled != Some(i)
            {
                self.select_scroll.scroll_to_item(i);
                self.select_scrolled = Some(i);
            }
            let this = cx.entity();
            let hover: menu_view::RowHandler = Rc::new({
                let this = this.clone();
                move |i, _, cx| {
                    this.update(cx, |view, cx| {
                        view.browser.menu_hover(i);
                        cx.notify();
                    })
                }
            });
            let click: menu_view::RowHandler = Rc::new(move |i, window, cx| {
                this.update(cx, |view, cx| {
                    view.browser.menu_click(i);
                    view.select_scrolled = None;
                    view.sync_focus(window, cx);
                    cx.notify();
                })
            });
            let list = menu_view::menu(t, "select-menu", &rows, menu.highlighted, w, h, &self.select_scroll, hover, click);
            out.push(deferred(anchored().position(point(px(x), px(y))).child(list)).with_priority(2).into_any_element());
        } else {
            self.select_scrolled = None;
        }
        if self.shell_menu.is_none()
            && self.browser.option_menu().is_none()
            && let Some(tip) = self.browser.tooltip()
        {
            let (px_, py_) = self.page_pointer_at;
            let at = point(px((area.x + px_) as f32 + 12.), px((area.y + py_) as f32 + 20.));
            let tip: SharedString = tip.to_string().into();
            out.push(
                deferred(
                    anchored().position(at).snap_to_window_with_margin(px(4.)).child(
                        div()
                            .id("page-tooltip")
                            .max_w(px(420.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(4.))
                            .bg(rgb(t.surface_variant))
                            .border_1()
                            .border_color(rgb(t.border))
                            .text_size(px(12.))
                            .text_color(rgb(t.text_primary))
                            .child(tip),
                    ),
                )
                .with_priority(3)
                .into_any_element(),
            );
        }
        out
    }

    /// Morph's About page (morph://about), drawn in the page area.
    fn about_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let lines = crate::about::lines();
        let rows = lines.iter().map(|(label, value)| {
            let value: SharedString = value.clone().into();
            div()
                .flex()
                .flex_row()
                .gap(px(16.))
                .child(div().w(px(130.)).flex_none().text_color(rgb(t.text_secondary)).child(*label))
                .child(div().flex_1().font_family("monospace").child(value))
        });
        let link = |id: &'static str, label: &'static str, url: &'static str| {
            div().id(id).text_color(rgb(t.accent)).cursor_pointer().hover(|s| s.underline()).child(label).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.browser.new_tab(url);
                    this.sync_focus(window, cx);
                    cx.notify();
                },
            ))
        };
        let notices = cx.listener(|this, _, window, cx| {
            if let Some(path) = crate::notices::write_page() {
                this.browser.new_tab(&format!("file://{}", path.display()));
                this.sync_focus(window, cx);
                cx.notify();
            }
        });
        div()
            .id("about-page")
            .absolute()
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(t.background))
            .text_color(rgb(t.text_primary))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(18.))
                    .py(px(40.))
                    .px(px(24.))
                    .child(gpui::img(crate::brand::wordmark()).h(px(72.)).w(px(252.)))
                    .child(div().w(px(560.)).child(crate::brand::glitch_rule(t)))
                    .child(div().text_size(px(14.)).text_color(rgb(t.text_secondary)).child("A browser that keeps your data yours."))
                    .child(div().w(px(560.)).flex().flex_col().gap(px(6.)).text_size(px(13.)).children(rows))
                    .child(div().flex().flex_row().gap(px(8.)).child(crate::widgets::button(t, "about-copy", "Copy diagnostics", true).on_click(
                        cx.listener(|_, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(crate::about::diagnostics(&crate::about::lines())));
                        }),
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(18.))
                            .text_size(px(13.))
                            .child(link("about-site", "Website", crate::about::WEBSITE))
                            .child(link("about-repo", "Source code", crate::about::REPOSITORY))
                            .child(
                                div()
                                    .id("about-notices")
                                    .text_color(rgb(t.accent))
                                    .cursor_pointer()
                                    .hover(|s| s.underline())
                                    .child("Open-source notices")
                                    .on_click(notices),
                            ),
                    )
                    .child(
                        div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child("Morph is free software under the BSD 3-Clause license."),
                    ),
            )
            .into_any_element()
    }

    /// The toast, as a bar below the page (child-window pages draw over
    /// anything placed on them).
    fn toast_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let t = self.theme;
        let toast = self.toast.as_ref()?;
        let mut bar = div()
            .id("toast")
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .h(px(32.))
            .bg(rgb(t.surface_variant))
            .text_size(px(13.))
            .text_color(rgb(t.text_primary))
            .child(toast.text.clone());
        if let Some((label, action)) = toast.action {
            bar = bar.child(div().id("toast-action").text_color(rgb(t.accent)).cursor_pointer().hover(|s| s.underline()).child(label).on_click(
                cx.listener(move |this, _, _, cx| {
                    this.toast = None;
                    cx.emit(ShellRequest::Run(action));
                    cx.notify();
                }),
            ));
        }
        Some(bar.into_any_element())
    }

    fn page_area(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let cover = self.browser.cover().cloned();
        let drawn = self.browser.draws_pages();
        let this = cx.entity();
        let paint_this = cx.entity();
        div()
            .id("page")
            .flex_1()
            .relative()
            .bg(rgb(t.background))
            .when(drawn, |d| {
                d.track_focus(&self.page_focus)
                    .key_context(PAGE_CONTEXT)
                    .on_key_down(cx.listener(Self::page_key_down))
                    .on_key_up(cx.listener(Self::page_key_up))
                    .on_modifiers_changed(cx.listener(Self::page_modifiers_changed))
            })
            // A page in a window of its own (X11) takes its clicks itself; one
            // that reaches the shell landed while the page's window was being
            // mapped again (a menu or Morph's own page closed). It still means
            // the page: it gets the keyboard once it shows, not the shell.
            .when(!drawn && cover.is_none(), |d| {
                d.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|view, _, window, cx| {
                        view.focus_page(window, cx);
                        cx.stop_propagation();
                    }),
                )
            })
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let area = Rect::new(
                            f64::from(f32::from(bounds.origin.x)),
                            f64::from(f32::from(bounds.origin.y)),
                            f64::from(f32::from(bounds.size.width)),
                            f64::from(f32::from(bounds.size.height)),
                        );
                        this.update(cx, |view, _| view.browser.set_page_area(area));
                        drawn.then(|| window.insert_hitbox(bounds, HitboxBehavior::Normal))
                    },
                    move |bounds, hitbox, window, cx| {
                        if let Some(hitbox) = hitbox {
                            paint_page(&paint_this, bounds, hitbox, window, cx);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(cover.map(|cover| self.cover_panel(&cover, cx)))
    }
}

/// `position` relative to `origin`, in logical pixels.
fn local(position: Point<Pixels>, origin: Point<Pixels>) -> (f64, f64) {
    (f64::from(f32::from(position.x - origin.x)), f64::from(f32::from(position.y - origin.y)))
}

/// Draw a page the shell draws and route the pointer to it: presses,
/// moves and wheel over the page, and moves and releases anywhere while a
/// button pressed in the page is held (dragging a selection out of it).
fn paint_page(view: &Entity<BrowserView>, bounds: Bounds<Pixels>, hitbox: Hitbox, window: &mut Window, cx: &mut App) {
    let (image, covered, cursor) = {
        let v = view.read(cx);
        (v.page_image.clone(), v.browser.cover().is_some(), v.browser.page_cursor())
    };
    if covered {
        return;
    }
    if let Some(image) = image {
        let device = image.size(0);
        let scale = window.scale_factor();
        let shown = Bounds::new(bounds.origin, size(px(device.width.0 as f32 / scale), px(device.height.0 as f32 / scale)));
        let _ = window.paint_image(bounds, shown, Corners::default(), image, 0, false);
    }
    let style = match cursor {
        PageCursor::Default => CursorStyle::Arrow,
        PageCursor::Pointer => CursorStyle::PointingHand,
        PageCursor::Text => CursorStyle::IBeam,
    };
    window.set_cursor_style(style, &hitbox);
    let focus = view.read(cx).page_focus.clone();
    window.handle_input(&focus, PageIme { view: view.clone(), bounds }, cx);
    let origin = bounds.origin;
    let (v, h) = (view.clone(), hitbox.clone());
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && h.is_hovered(window) {
            v.update(cx, |view, cx| view.page_mouse_down(event, origin, window, cx));
            cx.stop_propagation();
        }
    });
    let v = view.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && v.read(cx).page_buttons > 0 {
            v.update(cx, |view, _| view.page_mouse_up(event, origin));
        }
    });
    let (v, h) = (view.clone(), hitbox.clone());
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            let inside = h.is_hovered(window);
            v.update(cx, |view, _| view.page_mouse_move(event, origin, inside));
        }
    });
    let (v, h) = (view.clone(), hitbox);
    window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && h.is_hovered(window) {
            v.update(cx, |view, _| view.page_scroll(event, origin));
            cx.stop_propagation();
        }
    });
}

/// Text from an input method for a page the shell draws. Typed keys reach
/// the page as keys (`page_key_down`); what arrives here is an input
/// method's committed text, inserted where the page's caret is. The
/// composition itself is the input method's to show (WebKit gets no
/// preedit), and its candidates open at the page's corner.
struct PageIme {
    view: Entity<BrowserView>,
    bounds: Bounds<Pixels>,
}

impl gpui::InputHandler for PageIme {
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut App) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: 0..0, reversed: false })
    }
    fn marked_text_range(&mut self, _: &mut Window, _: &mut App) -> Option<Range<usize>> {
        None
    }
    fn text_for_range(&mut self, _: Range<usize>, _: &mut Option<Range<usize>>, _: &mut Window, _: &mut App) -> Option<String> {
        None
    }
    fn replace_text_in_range(&mut self, _: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut App) {
        if !text.is_empty() {
            self.view.update(cx, |view, _| {
                if view.browser.page_has_keyboard() {
                    view.browser.page_input(Input::Text(text.to_string()));
                }
            });
        }
    }
    fn replace_and_mark_text_in_range(&mut self, _: Option<Range<usize>>, _: &str, _: Option<Range<usize>>, _: &mut Window, _: &mut App) {}
    fn unmark_text(&mut self, _: &mut Window, _: &mut App) {}
    fn bounds_for_range(&mut self, _: Range<usize>, _: &mut Window, _: &mut App) -> Option<Bounds<Pixels>> {
        Some(Bounds::new(self.bounds.origin, size(px(1.), px(20.))))
    }
    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut App) -> Option<usize> {
        None
    }
}

impl Focusable for BrowserView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.address_focus.clone()
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keep the model in step when GPUI moved focus away (a click on the
        // shell's own chrome outside the address bar).
        if self.browser.address_focused() && self.mirrored_focus && !self.address_focus.is_focused(window) {
            self.browser.blur_address();
            self.mirrored_focus = false;
        } else if self.browser.address_focused() && !self.mirrored_focus {
            // A blank first tab starts with the address bar focused.
            window.focus(&self.address_focus, cx);
            self.mirrored_focus = true;
        }
        self.sync_address_menu(cx);
        let drawn = (self.marked, self.toast.as_ref().map(|t| t.text.clone()));
        if drawn != self.drawn {
            if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
                println!("morph-event drawn star={} toast={}", if drawn.0 { "on" } else { "off" }, drawn.1.as_deref().unwrap_or("-"));
            }
            self.drawn = drawn;
        }
        let content = div().flex().flex_col().min_w(px(0.)).min_h(px(0.));
        let content = if matches!(self.strip, TabStrip::Side { .. }) { content.flex_1().h_full() } else { content.size_full() };
        let overlays = self.overlays(window, cx);
        let body = match self.strip {
            TabStrip::Top => content
                .child(self.tab_strip(window, cx))
                .child(self.toolbar(window, cx))
                .children(self.find_bar(cx))
                .child(self.page_area(cx))
                .children(self.toast_bar(cx))
                .children(self.download_bar(cx))
                .into_any_element(),
            TabStrip::Bottom => content
                .child(self.toolbar(window, cx))
                .children(self.find_bar(cx))
                .child(self.page_area(cx))
                .children(self.toast_bar(cx))
                .children(self.download_bar(cx))
                .child(self.tab_strip(window, cx))
                .into_any_element(),
            TabStrip::Side { left, width, collapsed } => {
                let content = content
                    .child(self.toolbar(window, cx))
                    .children(self.find_bar(cx))
                    .child(self.page_area(cx))
                    .children(self.toast_bar(cx))
                    .children(self.download_bar(cx));
                let list = self.tab_list(width, collapsed, window, cx);
                let row = div().flex().flex_row().size_full();
                if left { row.child(list).child(content) } else { row.child(content).child(list) }.into_any_element()
            }
        };
        div()
            .relative()
            .size_full()
            .on_action(|_: &FocusNextStop, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPreviousStop, window, cx| window.focus_prev(cx))
            .child(body)
            .children(overlays)
    }
}

impl EntityInputHandler for BrowserView {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let field = self.browser.address();
        let range = field.range_from_utf16(&range_utf16);
        actual.replace(field.range_to_utf16(&range));
        field.text().get(range).map(str::to_string)
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let field = self.browser.address();
        Some(UTF16Selection { range: field.range_to_utf16(&field.selection()), reversed: field.is_reversed() })
    }

    fn set_selected_text_range(&mut self, range_utf16: Range<usize>, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |f| f.set_selection_utf16(range_utf16));
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.browser.address().len_utf16())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let field = self.browser.address();
        field.marked().map(|r| field.range_to_utf16(&r))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |f| f.unmark());
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let range = range_utf16.map(|r| self.browser.address().range_from_utf16(&r));
        self.edit(cx, |f| f.replace(range, text));
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|r| self.browser.address().range_from_utf16(&r));
        let selected = selected_utf16.map(|r| utf16_to_byte(text, r.start)..utf16_to_byte(text, r.end));
        self.edit(cx, |f| f.replace_and_mark(range, text, selected));
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let field = self.browser.address();
        if line.text.as_ref() != field.text() {
            return None;
        }
        let range = field.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let local = self.last_bounds?.localize(&p)?;
        let line = self.last_layout.as_ref()?;
        let field = self.browser.address();
        if line.text.as_ref() != field.text() {
            return None;
        }
        Some(field.offset_to_utf16(line.closest_index_for_x(local.x)))
    }
}

/// Byte offset of UTF-16 offset `utf16` within `text`, clamped to its end.
fn utf16_to_byte(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}

/// The menu under the focused address bar.
impl BrowserView {
    /// Open the address menu (the address bar was just focused by hand).
    fn open_address_menu(&mut self) {
        if self.address_menu.is_some() || !self.browser.address_focused() {
            return;
        }
        // Child-window pages draw above the shell: hide the page under it.
        let hid_page = !self.browser.draws_pages();
        if hid_page {
            self.browser.set_shown(false);
        }
        self.address_menu = Some(AddressMenu { highlighted: None, hid_page });
    }

    /// Close the address menu once the address bar lost the keyboard.
    fn sync_address_menu(&mut self, cx: &mut Context<Self>) {
        if self.address_menu.is_some() && (!self.browser.address_focused() || self.shell_menu.is_some()) {
            self.close_address_menu(cx);
        }
    }

    fn close_address_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.address_menu.take() {
            if menu.hid_page && self.shell_menu.as_ref().is_none_or(|m| !m.hid_page) {
                self.browser.set_shown(self.shell_shown);
            }
            cx.notify();
        }
    }

    fn step_address_menu(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = address_items(&self.profiles).len();
        if self.address_menu.is_none() {
            self.open_address_menu();
        }
        let Some(menu) = self.address_menu.as_mut() else {
            return;
        };
        menu.highlighted = step(menu.highlighted, count, down);
        cx.notify();
    }

    /// Leave the address bar for the page and close the menu.
    fn leave_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_address_menu(cx);
        self.browser.blur_address();
        self.browser.focus_page();
        self.sync_focus(window, cx);
    }

    fn activate_address_item(&mut self, item: AddressItem, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_address(window, cx);
        let action = match item {
            AddressItem::Back => Some(Action::Back),
            AddressItem::Forward => Some(Action::Forward),
            AddressItem::Refresh => Some(Action::Reload),
            AddressItem::Find => Some(Action::Find),
            AddressItem::Run(action, _) => Some(action),
            AddressItem::Identity => Some(Action::ShowIdentity),
            AddressItem::ManageProfiles => Some(Action::ShowProfiles),
            AddressItem::CopyLink => {
                let url = self.browser.active_url().to_string();
                if !url.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                    self.toast("Link copied", None, cx);
                }
                None
            }
            AddressItem::Bookmark => {
                cx.emit(ShellRequest::Run(Action::Bookmark));
                None
            }
            AddressItem::ComingSoon(what) => {
                self.toast(&format!("{what} is coming soon"), None, cx);
                None
            }
            AddressItem::Profile(id, _, _) => {
                cx.emit(ShellRequest::SwitchProfile(id));
                None
            }
        };
        if let Some(action) = action
            && !self.dispatch(action, window, cx)
        {
            cx.emit(ShellRequest::Run(action));
        }
        cx.notify();
    }

    /// The address menu, under the address bar, with a backdrop that
    /// closes it on a click elsewhere.
    fn address_menu_overlay(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let Some(menu) = &self.address_menu else {
            return Vec::new();
        };
        let Some(bar) = self.button_bounds.borrow().get("address").copied() else {
            return Vec::new();
        };
        let items = address_items(&self.profiles);
        let highlighted = menu.highlighted;
        let state = self.browser.toolbar();
        let url = self.browser.active_url().to_string();
        let backdrop = div().id("address-menu-backdrop").absolute().top_0().left_0().size_full().on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                if !bar.contains(&event.position) {
                    this.leave_address(window, cx);
                }
            }),
        );
        let enabled = |item: &AddressItem| match item {
            AddressItem::Back => state.can_back,
            AddressItem::Forward => state.can_forward,
            AddressItem::CopyLink | AddressItem::Bookmark | AddressItem::Find | AddressItem::Refresh => !url.is_empty(),
            _ => true,
        };
        let click = |i: usize, item: AddressItem| {
            cx.listener(move |this: &mut Self, _: &gpui::ClickEvent, window, cx| {
                cx.stop_propagation();
                if let Some(m) = this.address_menu.as_mut() {
                    m.highlighted = Some(i);
                }
                this.activate_address_item(item.clone(), window, cx);
            })
        };
        let row_buttons = items.iter().enumerate().filter(|(_, item)| item.in_button_row()).map(|(i, item)| {
            let (glyph, label) = match item {
                AddressItem::Back => ("←", "Back"),
                AddressItem::Forward => ("→", "Forward"),
                AddressItem::CopyLink => ("⧉", "Copy link"),
                _ => ("↻", "Refresh"),
            };
            let on = enabled(item);
            div()
                .id(("address-button", i))
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(2.))
                .py(px(6.))
                .rounded(px(8.))
                .text_size(px(11.))
                .text_color(rgb(if on { t.text_primary } else { t.border }))
                .when(highlighted == Some(i), |d| d.bg(rgb(t.surface_variant)))
                .when(on, |d| d.cursor_pointer().hover(|s| s.bg(rgb(t.surface_variant))).on_click(click(i, item.clone())))
                .child(div().text_size(px(16.)).child(glyph))
                .child(label)
        });
        let marked = self.marked;
        let mut list = div().flex().flex_col().py(px(4.));
        let mut profiles_started = false;
        for (i, item) in items.iter().enumerate().filter(|(_, item)| !item.in_button_row()) {
            let (label, detail, checked): (SharedString, SharedString, bool) = match item {
                AddressItem::Bookmark => {
                    ((if marked { "Remove bookmark" } else { "Bookmark this page" }).into(), self.label_for(Action::Bookmark), marked)
                }
                AddressItem::Find => ("Find in page".into(), self.label_for(Action::Find), false),
                AddressItem::ComingSoon(what) => ((*what).into(), "Coming soon".into(), false),
                AddressItem::Run(action, label) => ((*label).into(), self.label_for(*action), false),
                AddressItem::Identity => ("Identity".into(), self.profile_label.clone().into(), false),
                AddressItem::Profile(_, name, shown) => (name.clone().into(), SharedString::default(), *shown),
                AddressItem::ManageProfiles => ("Manage profiles…".into(), SharedString::default(), false),
                _ => continue,
            };
            let is_profile = matches!(item, AddressItem::Profile(..) | AddressItem::ManageProfiles);
            if is_profile && !profiles_started {
                profiles_started = true;
                list = list
                    .child(div().h(px(1.)).my(px(4.)).bg(rgb(t.surface_variant)))
                    .child(div().px(px(14.)).py(px(4.)).text_size(px(11.)).text_color(rgb(t.text_secondary)).child("Profiles"));
            }
            let separated = matches!(item, AddressItem::Run(Action::ShowHistory, _) | AddressItem::Identity);
            let on = enabled(item);
            list = list.when(separated, |d| d.child(div().h(px(1.)).my(px(4.)).bg(rgb(t.surface_variant)))).child(
                div()
                    .id(("address-item", i))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .h(px(30.))
                    .px(px(14.))
                    .text_size(px(13.))
                    .text_color(rgb(if on { t.text_primary } else { t.border }))
                    .when(highlighted == Some(i), |d| d.bg(rgb(t.surface_variant)))
                    .when(on, |d| d.cursor_pointer().hover(|s| s.bg(rgb(t.surface_variant))).on_click(click(i, item.clone())))
                    .child(div().w(px(12.)).text_color(rgb(t.accent)).child(if checked { "✓" } else { "" }))
                    .child(div().flex_1().child(label))
                    .child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(detail)),
            );
        }
        let panel = div()
            .id("address-menu")
            .occlude()
            .w(px(ADDRESS_MENU_WIDTH.max(f32::from(bar.size.width))))
            .max_h(px(560.))
            .overflow_y_scroll()
            .rounded(px(10.))
            .bg(rgb(t.surface))
            .border_1()
            .border_color(rgb(t.border))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex().flex_row().gap(px(4.)).p(px(6.)).children(row_buttons))
            .child(div().h(px(1.)).bg(rgb(t.surface_variant)))
            .child(list);
        vec![
            deferred(backdrop).with_priority(1).into_any_element(),
            deferred(anchored().position(point(bar.left(), bar.bottom() + px(6.))).snap_to_window_with_margin(px(4.)).child(panel))
                .with_priority(2)
                .into_any_element(),
        ]
    }
}

/// Say which tab-bar button ran, for the xvfb checks.
fn log_bar(id: &str) {
    if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
        println!("morph-event bar {id}");
    }
}

/// The next highlighted entry of `count`, moving down or up and wrapping.
fn step(from: Option<usize>, count: usize, down: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (from, down) {
        (None, true) => 0,
        (None, false) => count - 1,
        (Some(i), true) => (i + 1) % count,
        (Some(i), false) => (i + count - 1) % count,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn download_sizes_read_well() {
        assert_eq!(super::size_text(512), "512 B");
        assert_eq!(super::size_text(4_500_000), "4.5 MB");
        assert_eq!(super::size_text(1_200_000_000), "1.2 GB");
    }
    #[test]
    fn utf16_offsets_map_to_byte_offsets() {
        assert_eq!(super::utf16_to_byte("é🙂", 0), 0);
        assert_eq!(super::utf16_to_byte("é🙂", 1), 2);
        assert_eq!(super::utf16_to_byte("é🙂", 3), 6);
        assert_eq!(super::utf16_to_byte("é🙂", 9), 6);
    }

    #[test]
    fn the_address_menu_matches_the_mobile_sheet() {
        use super::AddressItem as A;
        let items = super::address_items(&[("a".into(), "Work".into(), true), ("b".into(), "Home".into(), false)]);
        let row: Vec<_> = items.iter().filter(|i| i.in_button_row()).cloned().collect();
        assert_eq!(row, [A::Back, A::Forward, A::CopyLink, A::Refresh]);
        let names: Vec<String> = items
            .iter()
            .filter_map(|i| match i {
                A::ComingSoon(w) => Some(w.to_string()),
                A::Run(_, l) => Some(l.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["Extensions", "More", "History", "Bookmarks", "Downloads", "Passwords", "Settings"]);
        assert!(items.contains(&A::Bookmark) && items.contains(&A::Find) && items.contains(&A::Identity));
        assert!(!format!("{items:?}").contains("Desktop"), "no desktop-site entry on desktop");
        assert_eq!(items.last(), Some(&A::ManageProfiles));
        assert!(items.contains(&A::Profile("a".into(), "Work".into(), true)));
    }

    #[test]
    fn menu_steps_wrap() {
        assert_eq!(super::step(None, 3, true), Some(0));
        assert_eq!(super::step(None, 3, false), Some(2));
        assert_eq!(super::step(Some(2), 3, true), Some(0));
        assert_eq!(super::step(Some(0), 3, false), Some(2));
        assert_eq!(super::step(None, 0, true), None);
    }
}
