//! The off-screen Linux backend: webkit2gtk views in GTK off-screen windows,
//! rendered into [`Frame`]s the shell paints itself, with pointer, wheel and
//! key input the shell hands over (ARCHITECTURE.md section 2.0).
//!
//! For native Wayland, where wry has no child web view: GTK runs on GDK's
//! Wayland backend, the pages never get a surface of their own, and the
//! shell's window has the keyboard throughout, so the shell decides which
//! keys reach a page (everything but its own shortcuts). Popups
//! (`window.open`) are GTK windows of their own, as in the X11 backend. Call
//! [`crate::wry_linux::init`] first and [`Backend::poll`] often, as there.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::gdk;
use gtk::glib::translate::{ToGlibPtr, ToGlibPtrMut};
use gtk::prelude::*;
use javascriptcore::ValueExt;
use webkit2gtk::{FindControllerExt, FindOptions, HitTestResultExt, OptionMenuExt, SettingsExt, WebViewExt};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use crate::wry_linux::{
    CONTROL_SCHEME_SCRIPT, FIND_MAX_MATCHES, Popups, Queue, UNDO_REDO_SCRIPT, attach_bridge, backend_err, popup_handler, watch, with_bridge,
};
use crate::{
    Backend, Event, Frame, Input, Key, MenuChoice, MenuItem, Modifiers, MouseButton, OptionMenu, PageBridge, PageCursor, Rect, Result, Spec, ViewId,
    WebView, keys,
};

/// A `<select>` list the page opened, with the row it started on.
type OpenMenu = Rc<RefCell<Option<(webkit2gtk::OptionMenu, Option<u32>)>>>;

/// Most frames copied out of a page per second: a page that animates
/// (a spinner, a video) damages its window far more often than a screen
/// shows, and every copy is the whole page at the device scale.
const MAX_FPS: u32 = 60;

/// Whether pages draw on the CPU (`MORPH_PAGE_CPU=1`, off by default): with
/// shared-memory buffers (the only kind an off-screen window takes on
/// Wayland, see `render_check` in the shell) a GPU-drawn page is read back
/// into memory for every frame on WebKit's compositor thread, before this
/// backend copies it again; drawn on the CPU, WebKit paints only what
/// changed straight into the buffer. Not the default: switching the policy
/// on a new off-screen view kept pages from loading at all under headless
/// sway (CI, 180fd2d).
pub fn pages_on_cpu() -> bool {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| v == "1");
    set("WEBKIT_DMABUF_RENDERER_FORCE_SHM") && set("MORPH_PAGE_CPU")
}

/// Frame copy counts for `MORPH_FRAME_STATS=1`, printed every few seconds.
#[derive(Default)]
struct FrameStats {
    since: Option<Instant>,
    damage: u32,
    copied: u32,
    copy_time: Duration,
    size: (u32, u32),
}

impl FrameStats {
    fn on() -> bool {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("MORPH_FRAME_STATS").is_some_and(|v| v == "1"))
    }

    /// Print and start over every 2 s.
    fn report(&mut self) {
        let since = *self.since.get_or_insert_with(Instant::now);
        let secs = since.elapsed().as_secs_f64();
        if secs < 2.0 {
            return;
        }
        let avg = if self.copied > 0 { self.copy_time.as_secs_f64() * 1000.0 / f64::from(self.copied) } else { 0.0 };
        println!(
            "morph-frames copied_fps={:.1} damage_fps={:.1} copy_ms={avg:.2} size={}x{} cpu_pages={}",
            f64::from(self.copied) / secs,
            f64::from(self.damage) / secs,
            self.size.0,
            self.size.1,
            pages_on_cpu()
        );
        *self = FrameStats { since: Some(Instant::now()), ..FrameStats::default() };
    }
}

/// State the backend and its views share.
struct Shared {
    /// The view with the keyboard.
    keyboard: Cell<Option<ViewId>>,
    /// Every view's off-screen window, for focus changes.
    windows: RefCell<HashMap<ViewId, gtk::OffscreenWindow>>,
    /// Device pixels per logical pixel in the shell's window.
    scale: Cell<f64>,
    /// Event times count from here, in milliseconds.
    start: Instant,
    /// A realized, never shown window that every off-screen window is
    /// embedded in. GTK gives an off-screen window its embedder's scale,
    /// and a new Wayland window starts at the largest monitor scale, so
    /// pages render at HiDPI rather than at the scale-1 root window's.
    embedder: gtk::Window,
    stats: RefCell<FrameStats>,
}

impl Shared {
    fn time(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }

    /// Give the keyboard to `view` (or to nobody), telling the views that
    /// lose and gain it.
    fn set_keyboard(&self, view: Option<ViewId>) {
        let old = self.keyboard.replace(view);
        if old == view {
            return;
        }
        let windows = self.windows.borrow();
        if let Some(window) = old.and_then(|id| windows.get(&id)) {
            send_focus(window, false);
        }
        if let Some(window) = view.and_then(|id| windows.get(&id)) {
            send_focus(window, true);
        }
    }
}

/// The off-screen backend for one shell window.
pub struct OffscreenBackend {
    next: u64,
    queue: Queue,
    popups: Popups,
    contexts: HashMap<Option<PathBuf>, WebContext>,
    shortcuts: Rc<RefCell<HashSet<String>>>,
    bridge: Option<PageBridge>,
    shared: Rc<Shared>,
    downloads: crate::wry_linux::Downloads,
    hooked: HashSet<Option<PathBuf>>,
}

impl OffscreenBackend {
    /// A backend for a shell window with `scale` device pixels per logical
    /// pixel.
    pub fn new(scale: f64) -> Self {
        Self {
            next: 0,
            queue: Rc::default(),
            popups: Rc::default(),
            contexts: HashMap::new(),
            shortcuts: Rc::default(),
            bridge: None,
            downloads: Rc::default(),
            hooked: HashSet::new(),
            shared: Rc::new(Shared {
                keyboard: Cell::new(None),
                windows: RefCell::default(),
                scale: Cell::new(scale.max(0.5)),
                start: Instant::now(),
                embedder: {
                    let window = gtk::Window::new(gtk::WindowType::Toplevel);
                    window.realize();
                    window
                },
                stats: RefCell::default(),
            }),
        }
    }
}

/// Tell `window` (and so its web view) that it gained or lost the keyboard.
fn send_focus(window: &gtk::OffscreenWindow, focus_in: bool) {
    let Some(target) = window.window() else {
        return;
    };
    let mut event = gdk::Event::new(gdk::EventType::FocusChange);
    // SAFETY: `event` is a focus-change event, so `focus_change` is the live
    // union member; the window reference is handed to the event, which
    // drops it when freed.
    unsafe {
        let raw: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
        (*raw).focus_change.window = target.to_glib_full();
        (*raw).focus_change.send_event = 1;
        (*raw).focus_change.in_ = i16::from(focus_in);
    }
    event.set_device(keyboard_device().as_ref());
    window.send_focus_change(&event);
}

fn seat() -> Option<gdk::Seat> {
    gdk::Display::default()?.default_seat()
}

fn keyboard_device() -> Option<gdk::Device> {
    seat()?.keyboard()
}

fn pointer_device() -> Option<gdk::Device> {
    seat()?.pointer()
}

impl Backend for OffscreenBackend {
    fn create(&mut self, spec: Spec) -> Result<Box<dyn WebView>> {
        self.next += 1;
        let id = ViewId(self.next);
        let context = self.contexts.entry(spec.data_dir.clone()).or_insert_with(|| WebContext::new(spec.data_dir.clone()));
        let url = if spec.url.is_empty() { "about:blank".to_string() } else { spec.url.clone() };
        let window = gtk::OffscreenWindow::new();
        window.realize();
        if let (Some(own), Some(embedder)) = (window.window(), self.shared.embedder.window()) {
            own.offscreen_window_set_embedder(&embedder);
        }
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // WebKit only counts a page as "in a window", which it needs before
        // it loads any media, when the page's top-level widget is an
        // on-screen `GtkWindow`; an off-screen window never qualifies, so
        // every `<video>` stayed empty. The page is built inside the
        // (realized, never shown) embedder window, which WebKit then takes
        // as its window, and moved into its off-screen window unannounced.
        let embedder = &self.shared.embedder;
        embedder.add(&vbox);
        let builder = WebViewBuilder::new_with_web_context(context);
        // A page moved from another window starts from its history instead.
        let builder = if spec.session.is_some() { builder } else { builder.with_url(url.clone()) };
        let builder = builder
            .with_initialization_script(UNDO_REDO_SCRIPT)
            .with_initialization_script(CONTROL_SCHEME_SCRIPT)
            .with_initialization_script(crate::icons::ICON_SCRIPT)
            .with_new_window_req_handler(popup_handler(id, self.queue.clone(), self.popups.clone(), self.bridge.clone()));
        let view = match with_bridge(builder, &self.bridge).build_gtk(&vbox) {
            Ok(view) => view,
            Err(e) => {
                self.shared.embedder.remove(&vbox);
                // SAFETY: the window was never shown or shared.
                unsafe { window.destroy() };
                return Err(backend_err(e));
            }
        };
        let engine = view.webview();
        if pages_on_cpu()
            && let Some(settings) = WebViewExt::settings(&engine)
        {
            settings.set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
        }
        let quiet = engine.connect_hierarchy_changed(|engine, _| engine.stop_signal_emission_by_name("hierarchy-changed"));
        embedder.remove(&vbox);
        window.add(&vbox);
        engine.disconnect(quiet);
        crate::wry_linux::hook_downloads(&engine, id, &mut self.hooked, &spec.data_dir, &self.downloads, &self.queue);
        attach_bridge(&engine, &self.bridge);
        watch(&engine, id, &self.queue, &self.shortcuts);
        crate::wry_linux::watch_links(&engine, id, &self.queue);
        if let Some(session) = &spec.session
            && !crate::wry_linux::restore_session(&engine, session)
        {
            engine.load_uri(&url);
        }
        let chooser: crate::wry_linux::Chooser = Rc::default();
        crate::wry_linux::watch_more(&engine, id, &self.queue, &chooser);
        let dirty = Rc::new(Cell::new(true));
        let (d, shared) = (dirty.clone(), self.shared.clone());
        window.connect_damage_event(move |_, _| {
            d.set(true);
            if FrameStats::on() {
                shared.stats.borrow_mut().damage += 1;
            }
            false
        });
        let cursor = Rc::new(Cell::new(PageCursor::Default));
        let c = cursor.clone();
        let editable = Rc::new(Cell::new(false));
        let e = editable.clone();
        engine.connect_mouse_target_changed(move |_, hit, _| {
            e.set(hit.context_is_editable());
            c.set(if hit.context_is_link() {
                PageCursor::Pointer
            } else if hit.context_is_editable() || hit.context_is_selection() {
                PageCursor::Text
            } else {
                PageCursor::Default
            });
        });
        // WebKit's context menu is a GTK menu, which needs a window on the
        // screen to hang from; an off-screen page has none. On a link the
        // shell draws its own link menu instead.
        let zoom = Rc::new(Cell::new(0.0));
        let (queue, z) = (self.queue.clone(), zoom.clone());
        engine.connect_context_menu(move |_, _, event, hit| {
            if let Some(url) = hit.context_is_link().then(|| hit.link_uri()).flatten() {
                let scale = if z.get() > 0.0 { z.get() } else { 1.0 };
                let (x, y) = event.coords().unwrap_or((0.0, 0.0));
                queue.borrow_mut().push((id, Event::LinkMenu { url: url.to_string(), x: x / scale, y: y / scale }));
            }
            true
        });
        // The same goes for a <select>'s list (it opened in the middle of
        // the screen, far from the element) and for tooltips: the shell
        // draws both.
        let menu: OpenMenu = Rc::default();
        let (queue, open, z) = (self.queue.clone(), menu.clone(), zoom.clone());
        engine.connect_show_option_menu(move |_, options, _, rect| {
            let scale = if z.get() > 0.0 { z.get() } else { 1.0 };
            let (items, selected) = menu_items(options);
            let anchor = Rect::new(
                f64::from(rect.x()) / scale,
                f64::from(rect.y()) / scale,
                f64::from(rect.width()) / scale,
                f64::from(rect.height()) / scale,
            );
            let (q, o) = (queue.clone(), open.clone());
            options.connect_close(move |closed| {
                let mine = o.borrow().as_ref().is_some_and(|(m, _)| m == closed);
                if mine {
                    o.borrow_mut().take();
                    q.borrow_mut().push((id, Event::OptionMenuClosed));
                }
            });
            *open.borrow_mut() = Some((options.clone(), selected));
            queue.borrow_mut().push((id, Event::OptionMenu(OptionMenu { items, anchor })));
            true
        });
        engine.connect_query_tooltip(|_, _, _, _, _| false);
        let queue = self.queue.clone();
        let last_tip: Rc<RefCell<Option<String>>> = Rc::default();
        engine.connect_notify_local(Some("tooltip-text"), move |engine, _| {
            let tip = engine.tooltip_text().map(|t| t.to_string()).filter(|t| !t.trim().is_empty());
            if *last_tip.borrow() != tip {
                *last_tip.borrow_mut() = tip.clone();
                queue.borrow_mut().push((id, Event::Tooltip(tip)));
            }
        });
        self.shared.windows.borrow_mut().insert(id, window.clone());
        let page = OffscreenView {
            id,
            window,
            view,
            popups: self.popups.clone(),
            shared: self.shared.clone(),
            dirty,
            bounds: Cell::new(spec.bounds),
            visible: Cell::new(false),
            copied_at: Cell::new(None),
            zoom,
            menu,
            buttons: Cell::new(0),
            inside: Cell::new(false),
            cursor,
            chooser,
            copied: Rc::default(),
            editable,
            held_middle: Cell::new(None),
        };
        page.apply_size();
        page.set_visible(spec.visible)?;
        Ok(Box::new(page))
    }

    fn poll(&mut self) -> Vec<(ViewId, Event)> {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        if FrameStats::on() {
            self.shared.stats.borrow_mut().report();
        }
        std::mem::take(&mut self.queue.borrow_mut())
    }

    fn focus_host(&self) {
        self.shared.set_keyboard(None);
    }

    fn set_shortcuts(&mut self, keystrokes: &[String]) {
        *self.shortcuts.borrow_mut() = keystrokes.iter().cloned().collect();
    }

    fn set_downloads_dir(&mut self, dir: Option<PathBuf>) {
        *self.downloads.dir.borrow_mut() = dir;
    }

    fn cancel_download(&mut self, id: u64) {
        self.downloads.cancel(id);
    }

    fn request_cookies(&mut self, download: u64, url: &str) -> bool {
        crate::wry_linux::request_cookies(&self.downloads, &self.queue, download, url)
    }

    fn set_page_bridge(&mut self, bridge: PageBridge) {
        self.bridge = Some(bridge);
    }

    fn draws_pages(&self) -> bool {
        true
    }

    fn keyboard(&self) -> Option<ViewId> {
        self.shared.keyboard.get()
    }

    fn set_scale(&mut self, scale: f64) {
        self.shared.scale.set(scale.max(0.5));
    }
}

struct OffscreenView {
    id: ViewId,
    window: gtk::OffscreenWindow,
    view: wry::WebView,
    popups: Popups,
    shared: Rc<Shared>,
    /// The window was drawn since the last frame was taken.
    dirty: Rc<Cell<bool>>,
    bounds: Cell<Rect>,
    visible: Cell<bool>,
    /// When the last frame was copied (see [`MAX_FPS`]).
    copied_at: Cell<Option<Instant>>,
    /// The page zoom that makes up for GTK's (whole-number) scale falling
    /// short of the shell's: the page renders at `scale` device pixels per
    /// logical pixel either way, and pointer positions are multiplied by it.
    zoom: Rc<Cell<f64>>,
    /// The open `<select>` list, if any.
    menu: OpenMenu,
    /// Pointer buttons held, as GDK button masks.
    buttons: Cell<u32>,
    /// The pointer is over the page.
    inside: Cell<bool>,
    cursor: Rc<Cell<PageCursor>>,
    chooser: crate::wry_linux::Chooser,
    /// What the last copy copied, for [`WebView::take_copied`].
    copied: Rc<RefCell<Option<String>>>,
    /// The pointer is over editable text.
    editable: Rc<Cell<bool>>,
    /// A middle button is down: `true` when it went to the page as a
    /// Ctrl+click (on a link), else it was kept from the page.
    held_middle: Cell<Option<bool>>,
}

/// The rows of a `<select>` list, and the one selected.
fn menu_items(menu: &webkit2gtk::OptionMenu) -> (Vec<MenuItem>, Option<u32>) {
    let mut selected = None;
    let items = (0..menu.n_items())
        .filter_map(|i| {
            let mut item = menu.item(i)?;
            let row = MenuItem {
                label: item.label().map(|s| s.to_string()).unwrap_or_default(),
                tooltip: item.tooltip().map(|s| s.to_string()).unwrap_or_default(),
                enabled: item.is_enabled(),
                group_label: item.is_group_label(),
                group_child: item.is_group_child(),
                selected: item.is_selected(),
            };
            if row.selected && selected.is_none() {
                selected = Some(i);
            }
            Some(row)
        })
        .collect();
    (items, selected)
}

/// GDK's number for a pointer button.
fn button_number(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Middle => 2,
        MouseButton::Right => 3,
        MouseButton::Back => 8,
        MouseButton::Forward => 9,
    }
}

/// GDK's mask for a held button (buttons 1 to 5 have one).
fn button_mask(number: u32) -> u32 {
    if (1..=5).contains(&number) { keys::BUTTON1 << (number - 1) } else { 0 }
}

impl OffscreenView {
    fn engine(&self) -> webkit2gtk::WebView {
        self.view.webview()
    }

    /// The zoom for the current scale (see [`OffscreenView::zoom`]).
    fn wanted_zoom(&self) -> f64 {
        let gtk_scale = f64::from(self.window.scale_factor().max(1));
        self.shared.scale.get() / gtk_scale
    }

    /// Size the off-screen window to the page area (times the zoom).
    fn apply_size(&self) {
        let zoom = self.wanted_zoom();
        if (zoom - self.zoom.get()).abs() > f64::EPSILON {
            self.zoom.set(zoom);
            self.engine().set_zoom_level(zoom);
        }
        let bounds = self.bounds.get();
        let width = (bounds.width * zoom).round().max(1.0) as i32;
        let height = (bounds.height * zoom).round().max(1.0) as i32;
        self.window.set_default_size(width, height);
        self.window.queue_resize();
    }

    /// A new event for the web view's own window, or `None` before it has
    /// one.
    fn new_event(&self, kind: gdk::EventType, target: Option<gdk::Window>) -> Option<(gdk::Event, *mut gdk::ffi::GdkEvent)> {
        let target = target?;
        let mut event = gdk::Event::new(kind);
        let raw: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
        // SAFETY: every GdkEvent member starts with the type and the window;
        // the window reference is handed to the event, which drops it when
        // freed.
        unsafe { (*raw).any.window = target.to_glib_full() };
        Some((event, raw))
    }

    fn pointer_event(&self, kind: gdk::EventType) -> Option<(gdk::Event, *mut gdk::ffi::GdkEvent)> {
        let (mut event, raw) = self.new_event(kind, self.engine().window())?;
        let device = pointer_device();
        event.set_device(device.as_ref());
        event.set_source_device(device.as_ref());
        Some((event, raw))
    }

    fn enter(&self, x: f64, y: f64, state: u32) {
        if self.inside.replace(true) {
            return;
        }
        self.crossing(gdk::EventType::EnterNotify, x, y, state);
    }

    fn crossing(&self, kind: gdk::EventType, x: f64, y: f64, state: u32) {
        let Some((mut event, raw)) = self.pointer_event(kind) else {
            return;
        };
        // SAFETY: a crossing event, so `crossing` is the live member.
        unsafe {
            let c = &mut (*raw).crossing;
            c.time = self.shared.time();
            c.x = x;
            c.y = y;
            c.x_root = x;
            c.y_root = y;
            c.mode = gdk::ffi::GDK_CROSSING_NORMAL;
            c.detail = gdk::ffi::GDK_NOTIFY_NONLINEAR;
            c.focus = 1;
            c.state = state;
        }
        gtk::main_do_event(&mut event);
    }

    fn motion(&self, x: f64, y: f64, modifiers: Modifiers) {
        let state = keys::state(modifiers) | self.buttons.get();
        self.enter(x, y, state);
        let Some((mut event, raw)) = self.pointer_event(gdk::EventType::MotionNotify) else {
            return;
        };
        // SAFETY: a motion event, so `motion` is the live member.
        unsafe {
            let m = &mut (*raw).motion;
            m.time = self.shared.time();
            m.x = x;
            m.y = y;
            m.x_root = x;
            m.y_root = y;
            m.state = state;
        }
        gtk::main_do_event(&mut event);
    }

    fn button(&self, x: f64, y: f64, button: MouseButton, pressed: bool, modifiers: Modifiers) {
        // WebKit's middle click went through its clipboard code (the
        // primary selection), which crashed off screen (see
        // crate::clipboard), wherever it landed. So the page never gets one:
        // on a link it is a Ctrl+click (a new tab, as before), on editable
        // text the shell pastes the primary selection itself.
        let (button, modifiers) = if button == MouseButton::Middle {
            let link = if pressed {
                let link = self.cursor.get() == PageCursor::Pointer && !self.editable.get();
                self.held_middle.set(Some(link));
                link
            } else {
                self.held_middle.take().unwrap_or(false)
            };
            if !link {
                return;
            }
            (MouseButton::Left, Modifiers { control: true, ..modifiers })
        } else {
            (button, modifiers)
        };
        let number = button_number(button);
        // X11 reports the buttons held before the event.
        let state = keys::state(modifiers) | self.buttons.get();
        self.enter(x, y, state);
        let mask = button_mask(number);
        self.buttons.set(if pressed { self.buttons.get() | mask } else { self.buttons.get() & !mask });
        let kind = if pressed { gdk::EventType::ButtonPress } else { gdk::EventType::ButtonRelease };
        let Some((mut event, raw)) = self.pointer_event(kind) else {
            return;
        };
        // SAFETY: a button event, so `button` is the live member.
        unsafe {
            let b = &mut (*raw).button;
            b.time = self.shared.time();
            b.x = x;
            b.y = y;
            b.x_root = x;
            b.y_root = y;
            b.state = state;
            b.button = number;
        }
        gtk::main_do_event(&mut event);
    }

    fn scroll(&self, x: f64, y: f64, dx: f64, dy: f64, precise: bool, modifiers: Modifiers) {
        let state = keys::state(modifiers) | self.buttons.get();
        self.enter(x, y, state);
        let Some((mut event, raw)) = self.pointer_event(gdk::EventType::Scroll) else {
            return;
        };
        // GTK's smooth deltas count wheel steps; WebKit scrolls a step by
        // 40 pixels (`Scrollbar::pixelsPerLineStep`).
        let (dx, dy) = if precise { (dx / 40.0, dy / 40.0) } else { (dx, dy) };
        // SAFETY: a scroll event, so `scroll` is the live member.
        unsafe {
            let s = &mut (*raw).scroll;
            s.time = self.shared.time();
            s.x = x;
            s.y = y;
            s.x_root = x;
            s.y_root = y;
            s.state = state;
            s.direction = gdk::ffi::GDK_SCROLL_SMOOTH;
            s.delta_x = dx;
            s.delta_y = dy;
        }
        gtk::main_do_event(&mut event);
    }

    fn key(&self, key: Key) {
        // WebKit must never act on these itself (see crate::clipboard): the
        // shell turns them into Input::Paste and Input::Copy.
        if crate::clipboard::clipboard_key(&key).is_some() {
            return;
        }
        let mut keycode = key.keycode;
        if keycode == 0
            && let Some(keymap) = gdk::Display::default().and_then(|d| gdk::Keymap::for_display(&d))
        {
            keycode = keymap.entries_for_keyval(key.keyval).first().map_or(0, |k| k.keycode());
        }
        let kind = if key.pressed { gdk::EventType::KeyPress } else { gdk::EventType::KeyRelease };
        let Some((mut event, raw)) = self.new_event(kind, self.window.window()) else {
            return;
        };
        event.set_device(keyboard_device().as_ref());
        // SAFETY: a key event, so `key` is the live member; `string` stays
        // null (WebKit takes text from the keyval).
        unsafe {
            let k = &mut (*raw).key;
            k.time = self.shared.time();
            k.state = key.state;
            k.keyval = key.keyval;
            k.hardware_keycode = keycode as u16;
            k.group = key.group as u8;
            k.is_modifier = u32::from(key.is_modifier);
        }
        gtk::main_do_event(&mut event);
    }

    /// Copy the off-screen window's pixels.
    fn capture(&self) -> Option<Frame> {
        let surface = self.window.surface()?;
        let image = match gtk::cairo::ImageSurface::try_from(surface.clone()) {
            Ok(image) => image,
            Err(_) => {
                // Not an image surface (X11): draw it into one.
                let scale = self.window.scale_factor().max(1);
                let alloc = self.window.allocation();
                let image = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, alloc.width() * scale, alloc.height() * scale).ok()?;
                let cr = gtk::cairo::Context::new(&image).ok()?;
                cr.scale(f64::from(scale), f64::from(scale));
                cr.set_source_surface(&surface, 0.0, 0.0).ok()?;
                cr.paint().ok()?;
                drop(cr);
                image
            }
        };
        let (width, height, stride) = (image.width().max(0) as usize, image.height().max(0) as usize, image.stride().max(0) as usize);
        if width == 0 || height == 0 {
            return None;
        }
        let mut pixels = Vec::with_capacity(width * height * 4);
        image
            .with_data(|data| {
                for row in 0..height {
                    let start = row * stride;
                    pixels.extend_from_slice(&data[start..start + width * 4]);
                }
            })
            .ok()?;
        Some(Frame { width: width as u32, height: height as u32, pixels })
    }
}

impl WebView for OffscreenView {
    fn id(&self) -> ViewId {
        self.id
    }
    fn choose_files(&self, paths: Option<Vec<PathBuf>>) {
        crate::wry_linux::answer_chooser(&self.chooser, paths);
    }
    fn clear_data(&self) -> Result<()> {
        self.view.clear_all_browsing_data().map_err(backend_err)
    }
    fn session_state(&self) -> Option<Vec<u8>> {
        crate::wry_linux::session_bytes(&self.engine())
    }
    fn load_url(&self, url: &str) -> Result<()> {
        self.view.load_url(url).map_err(backend_err)
    }
    fn back(&self) -> Result<()> {
        self.engine().go_back();
        Ok(())
    }
    fn forward(&self) -> Result<()> {
        self.engine().go_forward();
        Ok(())
    }
    fn reload(&self) -> Result<()> {
        self.engine().reload();
        Ok(())
    }
    fn stop(&self) -> Result<()> {
        self.engine().stop_loading();
        Ok(())
    }
    fn set_bounds(&self, bounds: Rect) -> Result<()> {
        self.bounds.set(bounds);
        self.apply_size();
        Ok(())
    }
    fn set_visible(&self, visible: bool) -> Result<()> {
        if self.visible.replace(visible) == visible {
            return Ok(());
        }
        if visible {
            self.apply_size();
            self.window.show_all();
            self.dirty.set(true);
        } else {
            self.window.hide();
        }
        Ok(())
    }
    fn focus(&self) -> Result<()> {
        self.engine().grab_focus();
        self.shared.set_keyboard(Some(self.id));
        Ok(())
    }
    fn find(&self, query: &str) -> Result<()> {
        if let Some(find) = self.engine().find_controller() {
            let options = FindOptions::CASE_INSENSITIVE | FindOptions::WRAP_AROUND;
            find.search(query, options.bits(), FIND_MAX_MATCHES);
        }
        Ok(())
    }
    fn find_next(&self, forward: bool) -> Result<()> {
        if let Some(find) = self.engine().find_controller() {
            if forward {
                find.search_next();
            } else {
                find.search_previous();
            }
        }
        Ok(())
    }
    fn find_clear(&self) -> Result<()> {
        if let Some(find) = self.engine().find_controller() {
            find.search_finish();
        }
        Ok(())
    }
    fn terminate(&self) -> Result<()> {
        self.engine().terminate_web_process();
        Ok(())
    }
    fn take_frame(&self) -> Option<Frame> {
        if !self.visible.get() {
            return None;
        }
        // The scale can change (the window moved to another screen).
        if (self.wanted_zoom() - self.zoom.get()).abs() > f64::EPSILON {
            self.apply_size();
        }
        if !self.dirty.get() {
            return None;
        }
        // Damage keeps: the next tick after the interval copies it.
        let now = Instant::now();
        if self.copied_at.get().is_some_and(|at| now.duration_since(at) < Duration::from_secs(1) / MAX_FPS) {
            return None;
        }
        self.dirty.set(false);
        self.copied_at.set(Some(now));
        let frame = self.capture();
        if FrameStats::on() {
            let mut stats = self.shared.stats.borrow_mut();
            stats.copied += 1;
            stats.copy_time += now.elapsed();
            if let Some(f) = &frame {
                stats.size = (f.width, f.height);
            }
        }
        frame
    }
    fn input(&self, input: Input) {
        let zoom = self.zoom.get();
        match input {
            Input::PointerMove { x, y, modifiers } => self.motion(x * zoom, y * zoom, modifiers),
            Input::PointerButton { x, y, button, pressed, modifiers } => {
                if pressed && self.shared.keyboard.get() != Some(self.id) {
                    let _ = self.focus();
                }
                self.button(x * zoom, y * zoom, button, pressed, modifiers);
            }
            Input::PointerLeave => {
                if self.inside.replace(false) {
                    self.crossing(gdk::EventType::LeaveNotify, -1.0, -1.0, self.buttons.get());
                }
            }
            Input::Scroll { x, y, dx, dy, precise, modifiers } => self.scroll(x * zoom, y * zoom, dx, dy, precise, modifiers),
            Input::Key(key) => self.key(key),
            Input::Text(text) => self.engine().execute_editing_command_with_argument("InsertText", &text),
            Input::Paste(clip) => {
                let script = crate::clipboard::paste_script(&clip);
                self.engine().run_javascript(&script, None::<&webkit2gtk::gio::Cancellable>, |_| {});
            }
            Input::Copy { cut } => {
                let copied = self.copied.clone();
                self.engine().run_javascript(&crate::clipboard::copy_script(cut), None::<&webkit2gtk::gio::Cancellable>, move |result| {
                    let text = result.ok().and_then(|r| r.js_value()).map(|v| v.to_str().to_string()).filter(|t| !t.is_empty());
                    if text.is_some() {
                        *copied.borrow_mut() = text;
                    }
                });
            }
        }
    }
    fn take_copied(&self) -> Option<String> {
        self.copied.borrow_mut().take()
    }
    fn cursor(&self) -> PageCursor {
        self.cursor.get()
    }
    fn option_menu(&self, choice: MenuChoice) {
        let Some((menu, start)) = self.menu.borrow().clone() else {
            return;
        };
        let count = menu.n_items();
        let index = |n: usize| u32::try_from(n).ok().filter(|&i| i < count);
        match choice {
            MenuChoice::Highlight(n) => {
                if let Some(i) = index(n) {
                    menu.select_item(i);
                }
            }
            MenuChoice::Activate(n) => {
                if let Some(i) = index(n) {
                    menu.activate_item(i);
                }
                menu.close();
            }
            MenuChoice::Cancel => {
                // Closing activates the row last highlighted: put back the
                // one the element had.
                if let Some(i) = start {
                    menu.select_item(i);
                }
                menu.close();
            }
        }
    }
}

impl Drop for OffscreenView {
    fn drop(&mut self) {
        if self.shared.keyboard.get() == Some(self.id) {
            self.shared.keyboard.set(None);
        }
        self.shared.windows.borrow_mut().remove(&self.id);
        let popups = self.popups.borrow_mut().remove(&self.id).unwrap_or_default();
        for popup in popups {
            // SAFETY: the popup window belongs to this tab, which is closing.
            unsafe { popup.window.destroy() };
        }
        // SAFETY: the off-screen window belongs to this view alone.
        unsafe { self.window.destroy() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_have_gdk_numbers_and_masks() {
        assert_eq!(button_number(MouseButton::Left), 1);
        assert_eq!(button_number(MouseButton::Right), 3);
        assert_eq!(button_mask(1), keys::BUTTON1);
        assert_eq!(button_mask(3), keys::BUTTON1 << 2);
        assert_eq!(button_mask(8), 0);
    }
}
