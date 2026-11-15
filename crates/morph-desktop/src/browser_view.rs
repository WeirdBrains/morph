//! The browser component's GPUI view: tab strip, toolbar and address bar,
//! drawn from `morph_browser::Browser`, with the page area left to the web
//! view. Menus and panels never draw over the page (the web view is a child
//! window above GPUI's surface), so everything sits beside it.

use std::ops::Range;
use std::path::Path;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Entity, EntityInputHandler, FocusHandle, Focusable, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine, SharedString, Subscription, UTF16Selection, Window, actions, canvas, div, point,
    prelude::*, px, rgb,
};
use morph_browser::{Browser, Config, Cover};
use morph_ui::actions::Action;
use morph_ui::theme::Theme;
use morph_webview::Rect;

use crate::address::AddressElement;
use crate::text_input::{TextInput, TextInputEvent};

actions!(address_bar, [Backspace, Delete, Left, Right, SelectLeft, SelectRight, SelectAll, Home, End, Paste, Copy, Cut, Submit, Cancel]);

/// The key context the address bar's bindings apply in.
pub const ADDRESS_CONTEXT: &str = "AddressBar";

pub const TAB_STRIP_HEIGHT: f32 = 34.;
pub const TOOLBAR_HEIGHT: f32 = 42.;
pub const FIND_BAR_HEIGHT: f32 = 38.;

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
            _subscriptions: vec![find_events],
        }
    }

    /// A browser on the real web view backend, children of `window`.
    pub fn create(home: &Path, profile: &str, start: &str, history: bool, window: &Window, keystrokes: &[String]) -> Result<Browser, String> {
        use raw_window_handle::HasWindowHandle;
        let raw = HasWindowHandle::window_handle(window).map_err(|e| format!("no window handle: {e}"))?.as_raw();
        let xid = morph_webview::wry_linux::x11_window_id(raw).ok_or(
            "Morph's desktop shell needs X11 (or XWayland with DISPLAY set): the system web view cannot be embedded in a native Wayland window yet",
        )?;
        let backend = morph_webview::wry_linux::WryBackend::new(xid);
        let home_for_history = home.to_path_buf();
        let profile_id = profile.to_string();
        let sink: Option<morph_browser::HistorySink> = history.then(|| {
            Box::new(move |url: &str, title: &str| {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
                // A locked library simply records nothing until it is unlocked.
                let _ = morph_core::library::record_visit(&home_for_history, &profile_id, url, title, now);
            }) as morph_browser::HistorySink
        });
        let config = Config {
            data_dir: Some(morph_core::profiles::webkit_data_dir(home, profile)),
            start_url: start.to_string(),
            history: sink,
            log_events: std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1"),
        };
        let mut browser = Browser::new(Box::new(backend), config).map_err(|e| e.to_string())?;
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
        cx.notify();
    }

    /// Pump the web views; returns shortcuts pressed while a page had focus.
    pub fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<String> {
        let polled = self.browser.poll();
        if polled.changed {
            self.sync_focus(window, cx);
            cx.notify();
        }
        polled.shortcuts
    }

    pub fn set_shown(&mut self, shown: bool) {
        self.browser.set_shown(shown);
    }

    /// Mirror the browser's address-bar focus into GPUI's focus.
    fn sync_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.browser.address_focused();
        if want && !self.address_focus.is_focused(window) {
            window.focus(&self.address_focus, cx);
        } else if !want && self.mirrored_focus && self.address_focus.is_focused(window) {
            window.blur(cx);
        }
        self.mirrored_focus = want;
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
            .child(label)
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let tabs = self.browser.tabs();
        div()
            .flex()
            .flex_row()
            .items_end()
            .h(px(TAB_STRIP_HEIGHT))
            .px(px(6.))
            .gap(px(2.))
            .bg(rgb(t.background))
            .children(tabs.into_iter().enumerate().map(|(index, tab)| {
                let id = tab.id;
                let label: SharedString = if tab.loading { format!("… {}", tab.label).into() } else { tab.label.into() };
                div()
                    .id(("tab", index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .h(px(30.))
                    .min_w(px(80.))
                    .max_w(px(220.))
                    .px(px(10.))
                    .rounded_t(px(8.))
                    .bg(rgb(if tab.active { t.surface } else { t.background }))
                    .text_color(rgb(if tab.active { t.text_primary } else { t.text_secondary }))
                    .text_size(px(13.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.browser.select(id);
                        this.sync_focus(window, cx);
                        cx.notify();
                    }))
                    .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
                    .child(
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
            }))
            .child(self.button("new-tab", "+", true).on_click(cx.listener(|this, _, window, cx| {
                this.dispatch(Action::NewTab, window, cx);
            })))
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
            .when(state.popups > 0, |d| {
                let label: SharedString = if state.popups == 1 { "1 popup".into() } else { format!("{} popups", state.popups).into() };
                d.child(div().px(px(8.)).text_size(px(12.)).text_color(rgb(t.accent_alt)).child(label))
            })
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
        let this = cx.entity();
        div()
            .id("page")
            .flex_1()
            .relative()
            .bg(rgb(t.background))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let area = Rect::new(
                            f64::from(f32::from(bounds.origin.x)),
                            f64::from(f32::from(bounds.origin.y)),
                            f64::from(f32::from(bounds.size.width)),
                            f64::from(f32::from(bounds.size.height)),
                        );
                        this.update(cx, |view, _| view.browser.set_page_area(area));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(cover.map(|cover| self.cover_panel(&cover, cx)))
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
        div().flex().flex_col().size_full().child(self.tab_strip(cx)).child(self.toolbar(cx)).children(self.find_bar(cx)).child(self.page_area(cx))
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
