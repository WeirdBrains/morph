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

use gpui::{
    App, Bounds, ClipboardItem, Context, Corners, CursorStyle, DispatchPhase, Entity, EntityInputHandler, FocusHandle, Focusable, Hitbox,
    HitboxBehavior, KeyDownEvent, KeyUpEvent, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    RenderImage, ScrollEvent, ShapedLine, SharedString, Subscription, UTF16Selection, Window, actions, canvas, div, point, prelude::*, px, rgb, size,
};
use morph_browser::{Browser, Config, Cover};
use morph_ui::actions::Action;
use morph_ui::chrome::{TabStrip, WindowButtons};
use morph_ui::theme::Theme;
use morph_webview::{Input, PageCursor, Rect};

use crate::address::AddressElement;
use crate::text_input::{TextInput, TextInputEvent};

actions!(address_bar, [Backspace, Delete, Left, Right, SelectLeft, SelectRight, SelectAll, Home, End, Paste, Copy, Cut, Submit, Cancel]);

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
    _subscriptions: Vec<Subscription>,
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
            profile_label: String::new(),
            strip: TabStrip::Top,
            caption: None,
            caption_pressed: false,
            page_focus: cx.focus_handle(),
            page_image: None,
            page_buttons: 0,
            page_pointer: false,
            page_modifiers: gpui::Modifiers::default(),
            _subscriptions: vec![find_events],
        }
    }

    /// A browser on the real web view backend: child windows of `window`
    /// on X11, pages rendered off screen on Wayland.
    pub fn create(home: &Path, profile: &str, start: &str, window: &Window, keystrokes: &[String]) -> Result<Browser, String> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let raw = HasWindowHandle::window_handle(window).map_err(|e| format!("no window handle: {e}"))?.as_raw();
        let mut backend: Box<dyn morph_webview::Backend> = match raw {
            RawWindowHandle::Wayland(_) => Box::new(morph_webview::offscreen::OffscreenBackend::new(f64::from(window.scale_factor()))),
            raw => {
                let xid = morph_webview::wry_linux::x11_window_id(raw).ok_or("Morph's desktop shell runs on X11 or Wayland only")?;
                Box::new(morph_webview::wry_linux::WryBackend::new(xid))
            }
        };
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
        };
        let mut browser = Browser::new(backend, config).map_err(|e| e.to_string())?;
        browser.set_shortcuts(keystrokes);
        Ok(browser)
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
            window.zoom_window();
        }
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
        let buttons = self.caption?;
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
                .child(control("window-minimize", "—", t.surface_variant).on_click(|_, window, _| window.minimize_window()))
                .child(control("window-maximize", "□", t.surface_variant).on_click(|_, window, _| window.zoom_window()));
        }
        let _ = cx;
        Some(row.child(control("window-close", "×", t.error).on_click(|_, window, _| window.remove_window())))
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
        let polled = self.browser.poll();
        if polled.changed {
            self.sync_focus(window, cx);
            cx.notify();
        }
        if self.browser.draws_pages() {
            self.browser.set_scale(f64::from(window.scale_factor()));
            if let Some(frame) = self.browser.take_frame() {
                self.set_page_frame(frame, window);
                cx.notify();
            }
        }
        polled.shortcuts
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
        if !self.browser.page_has_keyboard() {
            return;
        }
        if let Some(input) = crate::page_input::key(&event.keystroke, true, gpui_linux::raw_key::current()) {
            self.browser.page_input(input);
            cx.stop_propagation();
        }
    }

    fn page_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.browser.page_has_keyboard() {
            return;
        }
        if let Some(input) = crate::page_input::key(&event.keystroke, false, gpui_linux::raw_key::current()) {
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
        self.browser.set_shown(shown);
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
            // First click focuses and selects everything, as browsers do.
            self.browser.focus_address();
            self.sync_focus(window, cx);
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
        self.browser.submit_address();
        self.sync_focus(window, cx);
        cx.notify();
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
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

    /// One tab: a rounded header in a row, a line in the side list, or
    /// just its first letter in a collapsed list.
    fn tab_item(&self, index: usize, tab: morph_browser::TabLabel, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let t = self.theme;
        let id = tab.id;
        let collapsed = matches!(self.strip, TabStrip::Side { collapsed: true, .. });
        let label: SharedString = if collapsed {
            let letter = tab.label.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "·".into());
            if tab.loading { "…".into() } else { letter.into() }
        } else if tab.loading {
            format!("… {}", tab.label).into()
        } else {
            tab.label.into()
        };
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
            .on_click(cx.listener(move |this, _, window, cx| {
                this.browser.select(id);
                this.sync_focus(window, cx);
                cx.notify();
            }));
        let item = match self.strip {
            TabStrip::Top => item.min_w(px(80.)).max_w(px(220.)).px(px(10.)).rounded_t(px(8.)),
            TabStrip::Bottom => item.min_w(px(80.)).max_w(px(220.)).px(px(10.)).rounded_b(px(8.)),
            TabStrip::Side { .. } if collapsed => item.justify_center().rounded(px(6.)),
            TabStrip::Side { .. } => item.w_full().px(px(8.)).rounded(px(6.)),
        };
        let item = item
            .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().when(collapsed, |d| d.flex().justify_center()).child(label));
        if collapsed {
            return item;
        }
        item.child(
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

    fn new_tab_button(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        self.button("new-tab", "+", true).on_click(cx.listener(|this, _, window, cx| {
            this.dispatch(Action::NewTab, window, cx);
        }))
    }

    /// The tabs as a row (above the toolbar or below the page).
    fn tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let tabs = self.browser.tabs();
        div()
            .id("tab-strip")
            .flex()
            .flex_row()
            .when(self.strip == TabStrip::Bottom, |d| d.items_start())
            .when(self.strip != TabStrip::Bottom, |d| d.items_end())
            .h(px(TAB_STRIP_HEIGHT))
            .px(px(6.))
            .gap(px(2.))
            .bg(rgb(t.background))
            .children(tabs.into_iter().enumerate().map(|(index, tab)| self.tab_item(index, tab, cx)))
            .child(self.new_tab_button(cx))
            .when(self.strip == TabStrip::Top, |d| {
                d.on_mouse_down(MouseButton::Left, cx.listener(Self::caption_mouse_down))
                    .on_mouse_move(cx.listener(Self::caption_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::caption_mouse_up))
                    .child(div().flex_1().h_full())
                    .children(self.window_controls(cx).map(|c| c.h_full().items_center()))
            })
    }

    /// The tabs as a vertical list beside the toolbar and page.
    fn tab_list(&self, width: u32, collapsed: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let tabs = self.browser.tabs();
        div()
            .id("tab-list")
            .flex()
            .flex_col()
            .w(px(width as f32))
            .h_full()
            .p(px(6.))
            .gap(px(2.))
            .bg(rgb(t.background))
            .overflow_y_scroll()
            .children(tabs.into_iter().enumerate().map(|(index, tab)| self.tab_item(index, tab, cx)))
            .child(div().flex().flex_row().when(collapsed, |d| d.justify_center()).child(self.new_tab_button(cx)))
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let state = self.browser.toolbar();
        let focused = self.browser.address_focused();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .h(px(TOOLBAR_HEIGHT))
            .px(px(8.))
            .bg(rgb(t.surface))
            .when(!self.strip.is_caption(), |d| {
                d.on_mouse_down(MouseButton::Left, cx.listener(Self::caption_mouse_down))
                    .on_mouse_move(cx.listener(Self::caption_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::caption_mouse_up))
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
            .child(
                div()
                    .id("address")
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
                        .on_click(cx.listener(|_, _, window, cx| {
                            window.dispatch_action(Box::new(crate::shell::Bookmark), cx);
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
                        .on_click(cx.listener(|_, _, window, cx| {
                            window.dispatch_action(Box::new(crate::shell::ShowProfiles), cx);
                        })),
                )
            })
            .when(state.popups > 0, |d| {
                let label: SharedString = if state.popups == 1 { "1 popup".into() } else { format!("{} popups", state.popups).into() };
                d.child(div().px(px(8.)).text_size(px(12.)).text_color(rgb(t.accent_alt)).child(label))
            })
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
    /// load. The web view is hidden meanwhile.
    fn cover_panel(&self, cover: &Cover, cx: &mut Context<Self>) -> gpui::AnyElement {
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
        let content = div().flex().flex_col().min_w(px(0.)).min_h(px(0.));
        let content = if matches!(self.strip, TabStrip::Side { .. }) { content.flex_1().h_full() } else { content.size_full() };
        match self.strip {
            TabStrip::Top => {
                content.child(self.tab_strip(cx)).child(self.toolbar(cx)).children(self.find_bar(cx)).child(self.page_area(cx)).into_any_element()
            }
            TabStrip::Bottom => {
                content.child(self.toolbar(cx)).children(self.find_bar(cx)).child(self.page_area(cx)).child(self.tab_strip(cx)).into_any_element()
            }
            TabStrip::Side { left, width, collapsed } => {
                let content = content.child(self.toolbar(cx)).children(self.find_bar(cx)).child(self.page_area(cx));
                let list = self.tab_list(width, collapsed, cx);
                let row = div().flex().flex_row().size_full();
                if left { row.child(list).child(content) } else { row.child(content).child(list) }.into_any_element()
            }
        }
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

#[cfg(test)]
mod tests {
    #[test]
    fn utf16_offsets_map_to_byte_offsets() {
        assert_eq!(super::utf16_to_byte("é🙂", 0), 0);
        assert_eq!(super::utf16_to_byte("é🙂", 1), 2);
        assert_eq!(super::utf16_to_byte("é🙂", 3), 6);
        assert_eq!(super::utf16_to_byte("é🙂", 9), 6);
    }
}
