//! The Linux backend: webkit2gtk views placed by wry as child windows of the
//! desktop shell's X11 window (ARCHITECTURE.md section 2.0).
//!
//! X11 only: wry has no child web view on Wayland, so the desktop shell runs
//! through XWayland on Wayland sessions. GTK must be initialised with
//! [`init`] on the UI thread, and [`Backend::poll`] pumps GTK's main loop, so
//! the shell calls it often (every few milliseconds).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib::Propagation;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle, XlibWindowHandle};
use webkit2gtk::{FindControllerExt, FindOptions, LoadEvent, WebViewExt};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{NewWindowFeatures, NewWindowResponse, WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use crate::{Backend, Error, Event, LoadError, Rect, Result, Spec, ViewId, WebView, errors};

/// Start GTK on this thread. Call once, before [`WryBackend::new`].
pub fn init() -> Result<()> {
    gtk::init().map_err(|e| Error::Backend(format!("GTK did not start: {e}")))
}

/// The X11 window id behind a window handle, if it is an X11 window.
pub fn x11_window_id(handle: RawWindowHandle) -> Option<u64> {
    match handle {
        RawWindowHandle::Xcb(h) => Some(u64::from(h.window.get())),
        #[allow(clippy::useless_conversion)] // c_ulong is u32 on 32-bit targets
        RawWindowHandle::Xlib(h) => Some(u64::from(h.window)),
        _ => None,
    }
}

/// wry wants an Xlib handle; GPUI hands out an XCB one. The window id is the
/// same under both, so this re-labels it.
struct XlibParent(RawWindowHandle);

impl XlibParent {
    fn new(window: u64) -> Self {
        Self(RawWindowHandle::Xlib(XlibWindowHandle::new(window as _)))
    }
}

impl HasWindowHandle for XlibParent {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the handle names the shell's X11 window, which outlives the
        // backend and every web view made from it.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

type Queue = Rc<RefCell<Vec<(ViewId, Event)>>>;

/// WebKitGTK leaves undo and redo to the embedding app (Epiphany binds them
/// as app shortcuts). Morph does it in the page, after the page's own
/// handlers: if nothing called `preventDefault()` on Ctrl+Z, Ctrl+Shift+Z or
/// Ctrl+Y, the page's editing undo or redo runs. Pages with their own undo
/// (editors) keep it.
const UNDO_REDO_SCRIPT: &str = r#"(() => {
  addEventListener("keydown", (e) => {
    if (!e.ctrlKey || e.altKey || e.metaKey) return;
    const key = e.key.toLowerCase();
    const command = key === "z" ? (e.shiftKey ? "redo" : "undo") : (key === "y" && !e.shiftKey ? "redo" : null);
    if (!command) return;
    setTimeout(() => { if (!e.defaultPrevented) document.execCommand(command); }, 0);
  });
})();"#;

struct Popup {
    window: gtk::Window,
    _view: wry::WebView,
}

type Popups = Rc<RefCell<HashMap<ViewId, Vec<Popup>>>>;

/// The wry backend for one shell window.
pub struct WryBackend {
    parent: XlibParent,
    next: u64,
    queue: Queue,
    popups: Popups,
    contexts: HashMap<Option<PathBuf>, WebContext>,
    shortcuts: Rc<RefCell<HashSet<String>>>,
    keys: HostKeys,
}

/// An Xlib connection of the backend's own, for focus changes and forwarded
/// keys.
struct XConn {
    xlib: x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
}

impl XConn {
    fn open() -> Option<Self> {
        let xlib = x11_dl::xlib::Xlib::open().ok()?;
        // SAFETY: a null name opens `$DISPLAY`; checked for null below.
        let display = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
        (!display.is_null()).then_some(Self { xlib, display })
    }
}

impl Drop for XConn {
    fn drop(&mut self) {
        // SAFETY: opened in `open` and closed once, here.
        unsafe { (self.xlib.XCloseDisplay)(self.display) };
    }
}

/// Sends keys to the shell while the shell has focus.
///
/// X11 delivers a key to the window under the pointer when the focused
/// window is its ancestor. The web views are children of the shell's window,
/// so after Ctrl+T or Ctrl+L with the pointer over the page, keys reach the
/// page's window instead of the address bar. While the shell has focus, the
/// page windows hand such keys on to the shell's window unchanged; a click
/// into a page (or [`WebView::focus`]) gives the keys back to the pages.
#[derive(Clone)]
struct HostKeys {
    conn: Option<Rc<XConn>>,
    host: u64,
    active: Rc<Cell<bool>>,
    /// A page window that should have X focus and how many more polls to
    /// check it got it. A window that was just shown is not mapped yet, and
    /// X refuses focus to an unmapped window, so the backend tries again.
    pending: Rc<Cell<Option<(u64, u8)>>>,
}

/// Polls (8 ms apart, so about a second) to keep re-checking a page's focus;
/// a resize or a panel closing beside it can take it back meanwhile.
const FOCUS_RETRIES: u8 = 125;

impl HostKeys {
    fn set_active(&self, active: bool) {
        self.active.set(active);
    }

    /// Forward `event` to the shell if the shell has focus. Returns whether
    /// it was forwarded (and must not reach the page).
    fn forward(&self, event: &gtk::gdk::EventKey, press: bool) -> bool {
        if !self.active.get() {
            return false;
        }
        let Some(conn) = self.conn.as_ref() else {
            return false;
        };
        let xlib = &conn.xlib;
        // SAFETY: the display is open; the event is a plain value naming the
        // shell's own window.
        unsafe {
            let root = (xlib.XDefaultRootWindow)(conn.display);
            let mut key = x11_dl::xlib::XKeyEvent {
                type_: if press { x11_dl::xlib::KeyPress } else { x11_dl::xlib::KeyRelease },
                serial: 0,
                send_event: 1,
                display: conn.display,
                window: self.host as _,
                root,
                subwindow: 0,
                time: event.time() as _,
                x: 1,
                y: 1,
                x_root: 1,
                y_root: 1,
                state: event.state().bits(),
                keycode: u32::from(event.hardware_keycode()),
                same_screen: 1,
            };
            let mask = if press { x11_dl::xlib::KeyPressMask } else { x11_dl::xlib::KeyReleaseMask };
            (xlib.XSendEvent)(conn.display, self.host as _, 1, mask, (&mut key as *mut x11_dl::xlib::XKeyEvent).cast());
            (xlib.XFlush)(conn.display);
        }
        true
    }

    /// Give X input focus to a page's GTK window `page` (an X window id), so
    /// keys reach the page wherever the pointer is. Without this the shell's
    /// window keeps X focus and keys typed with the pointer over the shell's
    /// tab strip or toolbar never reach the page.
    fn focus_page(&self, page: Option<u64>) {
        self.set_active(false);
        let Some(page) = page else {
            return;
        };
        self.pending.set(Some((page, FOCUS_RETRIES)));
        self.retry_focus();
    }

    /// Give the pending page X focus if it does not have it and can take it.
    fn retry_focus(&self) {
        let (Some(conn), Some((page, left))) = (self.conn.as_ref(), self.pending.get()) else {
            return;
        };
        self.pending.set(left.checked_sub(1).map(|left| (page, left)));
        // SAFETY: the display is open; the window id is a page window, a
        // child of the shell's window. Focus is only set while it is mapped.
        unsafe {
            let mut focused: x11_dl::xlib::Window = 0;
            let mut revert = 0;
            (conn.xlib.XGetInputFocus)(conn.display, &mut focused, &mut revert);
            if focused as u64 == page {
                return;
            }
            let mut attrs: x11_dl::xlib::XWindowAttributes = std::mem::zeroed();
            if (conn.xlib.XGetWindowAttributes)(conn.display, page as _, &mut attrs) == 0 || attrs.map_state != x11_dl::xlib::IsViewable {
                return;
            }
            (conn.xlib.XSetInputFocus)(conn.display, page as _, x11_dl::xlib::RevertToParent, x11_dl::xlib::CurrentTime);
            (conn.xlib.XFlush)(conn.display);
        }
    }

    fn focus_host(&self) {
        self.pending.set(None);
        self.set_active(true);
        let Some(conn) = self.conn.as_ref() else {
            return;
        };
        // SAFETY: the display is open; the window id is the shell's own
        // top-level window.
        unsafe {
            (conn.xlib.XSetInputFocus)(conn.display, self.host as _, x11_dl::xlib::RevertToParent, x11_dl::xlib::CurrentTime);
            (conn.xlib.XFlush)(conn.display);
        }
    }
}

/// The X window id to give input focus for web view `engine`: the web view
/// widget's own window, the one that selects key events. (The GTK window
/// around it does not, so keys sent there would pass up to the shell.)
fn page_window(engine: &webkit2gtk::WebView) -> Option<u64> {
    let window = engine.window()?;
    let x11 = window.downcast::<gdkx11::X11Window>().ok()?;
    Some(x11.xid())
}

impl WryBackend {
    /// A backend whose views are children of X11 window `window`.
    pub fn new(window: u64) -> Self {
        Self {
            parent: XlibParent::new(window),
            next: 0,
            queue: Rc::default(),
            popups: Rc::default(),
            contexts: HashMap::new(),
            shortcuts: Rc::default(),
            keys: HostKeys { conn: XConn::open().map(Rc::new), host: window, active: Rc::default(), pending: Rc::default() },
        }
    }
}

/// X11 refuses windows of zero size (`BadValue` from `CreateWindow`), which
/// is what a page area is before the first layout, so sizes are at least 1.
fn wry_rect(bounds: Rect) -> wry::Rect {
    let size = LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0));
    wry::Rect { position: LogicalPosition::new(bounds.x, bounds.y).into(), size: size.into() }
}

fn backend_err(e: impl std::fmt::Display) -> Error {
    Error::Backend(e.to_string())
}

/// GPUI notation for a key press, if it has Ctrl or Alt held or is a
/// function key (the only keys a shell shortcut can be).
fn keystroke(event: &gtk::gdk::EventKey) -> Option<String> {
    let state = event.state();
    let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
    let alt = state.contains(gtk::gdk::ModifierType::MOD1_MASK);
    let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
    let name = event.keyval().to_lower().name()?.to_ascii_lowercase();
    let key = match name.as_str() {
        "iso_left_tab" => "tab".to_string(),
        "page_up" => "pageup".into(),
        "page_down" => "pagedown".into(),
        "comma" => ",".into(),
        other => other.to_string(),
    };
    let function_key = key.len() >= 2 && key.starts_with('f') && key[1..].chars().all(|c| c.is_ascii_digit());
    if !ctrl && !alt && !function_key {
        return None;
    }
    let mut out = String::new();
    if ctrl {
        out.push_str("ctrl-");
    }
    if alt {
        out.push_str("alt-");
    }
    if shift {
        out.push_str("shift-");
    }
    out.push_str(&key);
    Some(out)
}

/// Report page signals for `id` into `queue` and route shell shortcuts.
fn watch(view: &webkit2gtk::WebView, id: ViewId, queue: &Queue, shortcuts: &Rc<RefCell<HashSet<String>>>) {
    let q = queue.clone();
    view.connect_uri_notify(move |v| {
        if let Some(uri) = v.uri() {
            q.borrow_mut().push((id, Event::UrlChanged(uri.to_string())));
        }
    });
    let q = queue.clone();
    view.connect_title_notify(move |v| {
        let title = v.title().map(|t| t.to_string()).unwrap_or_default();
        q.borrow_mut().push((id, Event::TitleChanged(title)));
    });
    let q = queue.clone();
    view.connect_load_changed(move |v, load| {
        let event = match load {
            LoadEvent::Started => Event::LoadStarted,
            LoadEvent::Finished => Event::LoadFinished,
            LoadEvent::Committed => Event::History { can_back: v.can_go_back(), can_forward: v.can_go_forward() },
            _ => return,
        };
        q.borrow_mut().push((id, event));
    });
    let q = queue.clone();
    view.connect_web_process_terminated(move |_, _| {
        q.borrow_mut().push((id, Event::Crashed));
    });
    let q = queue.clone();
    view.connect_load_failed(move |_, _, uri, error| {
        let domain = error.domain().as_str().to_string();
        let raw: *const gtk::glib::ffi::GError = error.to_glib_none().0;
        // SAFETY: `raw` points at the live error the signal passed in.
        let code = unsafe { (*raw).code };
        if errors::shown(&domain, code) {
            let kind = errors::kind(&domain, code, false);
            q.borrow_mut().push((id, Event::LoadFailed(LoadError { url: uri.to_string(), kind, message: error.message().to_string() })));
        }
        // Morph draws its own error page; keep the engine's off the screen.
        true
    });
    let q = queue.clone();
    view.connect_load_failed_with_tls_errors(move |_, uri, _, flags| {
        let message = errors::tls_reason(flags.bits()).to_string();
        q.borrow_mut().push((id, Event::LoadFailed(LoadError { url: uri.to_string(), kind: crate::ErrorKind::Certificate, message })));
        true
    });
    let q = queue.clone();
    view.connect_is_web_process_responsive_notify(move |v| {
        q.borrow_mut().push((id, Event::Unresponsive(!v.is_web_process_responsive())));
    });
    if let Some(find) = view.find_controller() {
        let q = queue.clone();
        find.connect_found_text(move |_, matches| {
            q.borrow_mut().push((id, Event::FindResult { matches }));
        });
        let q = queue.clone();
        find.connect_failed_to_find_text(move |_| {
            q.borrow_mut().push((id, Event::FindResult { matches: 0 }));
        });
    }
    let q = queue.clone();
    view.connect_focus_in_event(move |_, _| {
        q.borrow_mut().push((id, Event::Focused));
        Propagation::Proceed
    });
    let q = queue.clone();
    let keys = shortcuts.clone();
    view.connect_key_press_event(move |_, event| match keystroke(event) {
        Some(key) if keys.borrow().contains(&key) => {
            q.borrow_mut().push((id, Event::Shortcut(key)));
            Propagation::Stop
        }
        _ => Propagation::Proceed,
    });
}

/// Handles `window.open` from a page of tab `tab`: Morph makes the popup
/// itself, a GTK window holding a web view related to the opener (same
/// process and storage, so `window.opener` and `postMessage` work). wry's
/// default response would panic for a child web view.
fn popup_handler(tab: ViewId, queue: Queue, popups: Popups) -> impl Fn(String, NewWindowFeatures) -> NewWindowResponse + 'static {
    move |url, features| {
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        let (w, h) = features.size.map(|s| (s.width as i32, s.height as i32)).unwrap_or((800, 600));
        window.set_default_size(w.max(200), h.max(150));
        window.set_title("Morph");
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&vbox);
        let built = WebViewBuilder::new()
            .with_related_view(features.opener.webview.clone())
            .with_initialization_script(UNDO_REDO_SCRIPT)
            .with_new_window_req_handler(popup_handler(tab, queue.clone(), popups.clone()))
            .build_gtk(&vbox);
        let view = match built {
            Ok(view) => view,
            Err(_) => {
                // SAFETY: the window was never shown or shared.
                unsafe { window.destroy() };
                return NewWindowResponse::Deny;
            }
        };
        let engine = view.webview();
        let title_window = window.clone();
        let q = queue.clone();
        engine.connect_title_notify(move |v| {
            if let Some(title) = v.title() {
                title_window.set_title(&title);
                q.borrow_mut().push((tab, Event::PopupTitleChanged(title.to_string())));
            }
        });
        // A popup is a plain window with no shell around it: F5 and Ctrl+R
        // reload it, Ctrl+W closes it, every other key goes to its page.
        let key_window = window.clone();
        engine.connect_key_press_event(move |v, event| match keystroke(event).as_deref() {
            Some("f5" | "ctrl-r") => {
                v.reload();
                Propagation::Stop
            }
            Some("ctrl-w") => {
                // SAFETY: GTK owns the window; destroying it runs `destroy` below.
                unsafe { key_window.destroy() };
                Propagation::Stop
            }
            _ => Propagation::Proceed,
        });
        // `window.close()` in the popup closes its window.
        let closing = window.clone();
        engine.connect_close(move |_| {
            // SAFETY: GTK owns the window; destroying it runs `destroy` below.
            unsafe { closing.destroy() };
        });
        let q = queue.clone();
        let list = popups.clone();
        let key = window.clone();
        window.connect_destroy(move |_| {
            if let Some(popups) = list.borrow_mut().get_mut(&tab) {
                popups.retain(|p| p.window != key);
            }
            q.borrow_mut().push((tab, Event::PopupClosed));
        });
        window.show_all();
        queue.borrow_mut().push((tab, Event::PopupOpened { url }));
        popups.borrow_mut().entry(tab).or_default().push(Popup { window, _view: view });
        NewWindowResponse::Create { webview: engine }
    }
}

impl Backend for WryBackend {
    fn create(&mut self, spec: Spec) -> Result<Box<dyn WebView>> {
        self.next += 1;
        let id = ViewId(self.next);
        let context = self.contexts.entry(spec.data_dir.clone()).or_insert_with(|| WebContext::new(spec.data_dir.clone()));
        let url = if spec.url.is_empty() { "about:blank".to_string() } else { spec.url.clone() };
        let view = WebViewBuilder::new_with_web_context(context)
            .with_url(url)
            .with_bounds(wry_rect(spec.bounds))
            .with_initialization_script(UNDO_REDO_SCRIPT)
            .with_visible(spec.visible)
            .with_new_window_req_handler(popup_handler(id, self.queue.clone(), self.popups.clone()))
            .build_as_child(&self.parent)
            .map_err(backend_err)?;
        let engine = view.webview();
        watch(&engine, id, &self.queue, &self.shortcuts);
        let keys = self.keys.clone();
        engine.connect_button_press_event(move |engine, _| {
            keys.focus_page(page_window(engine));
            Propagation::Proceed
        });
        if let Some(top) = engine.toplevel() {
            let keys = self.keys.clone();
            top.connect_key_press_event(move |_, e| if keys.forward(e, true) { Propagation::Stop } else { Propagation::Proceed });
            let keys = self.keys.clone();
            top.connect_key_release_event(move |_, e| if keys.forward(e, false) { Propagation::Stop } else { Propagation::Proceed });
        }
        Ok(Box::new(WryView { id, view, popups: self.popups.clone(), keys: self.keys.clone(), bounds: Cell::new(spec.bounds) }))
    }

    fn poll(&mut self) -> Vec<(ViewId, Event)> {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        self.keys.retry_focus();
        std::mem::take(&mut self.queue.borrow_mut())
    }

    fn focus_host(&self) {
        self.keys.focus_host();
    }

    fn set_shortcuts(&mut self, keystrokes: &[String]) {
        *self.shortcuts.borrow_mut() = keystrokes.iter().cloned().collect();
    }
}

struct WryView {
    id: ViewId,
    view: wry::WebView,
    popups: Popups,
    keys: HostKeys,
    bounds: Cell<Rect>,
}

impl WebView for WryView {
    fn id(&self) -> ViewId {
        self.id
    }
    fn load_url(&self, url: &str) -> Result<()> {
        self.view.load_url(url).map_err(backend_err)
    }
    fn back(&self) -> Result<()> {
        self.view.webview().go_back();
        Ok(())
    }
    fn forward(&self) -> Result<()> {
        self.view.webview().go_forward();
        Ok(())
    }
    fn reload(&self) -> Result<()> {
        self.view.webview().reload();
        Ok(())
    }
    fn stop(&self) -> Result<()> {
        self.view.webview().stop_loading();
        Ok(())
    }
    fn set_bounds(&self, bounds: Rect) -> Result<()> {
        self.bounds.set(bounds);
        self.view.set_bounds(wry_rect(bounds)).map_err(backend_err)
    }
    fn set_visible(&self, visible: bool) -> Result<()> {
        self.view.set_visible(visible).map_err(backend_err)?;
        if visible {
            // Showing the GTK window again sizes it to its default (1x1).
            self.view.set_bounds(wry_rect(self.bounds.get())).map_err(backend_err)?;
        }
        Ok(())
    }
    fn focus(&self) -> Result<()> {
        self.keys.focus_page(page_window(&self.view.webview()));
        self.view.focus().map_err(backend_err)
    }
    fn find(&self, query: &str) -> Result<()> {
        if let Some(find) = self.view.webview().find_controller() {
            let options = FindOptions::CASE_INSENSITIVE | FindOptions::WRAP_AROUND;
            find.search(query, options.bits(), FIND_MAX_MATCHES);
        }
        Ok(())
    }
    fn find_next(&self, forward: bool) -> Result<()> {
        if let Some(find) = self.view.webview().find_controller() {
            if forward {
                find.search_next();
            } else {
                find.search_previous();
            }
        }
        Ok(())
    }
    fn find_clear(&self) -> Result<()> {
        if let Some(find) = self.view.webview().find_controller() {
            find.search_finish();
        }
        Ok(())
    }
    fn terminate(&self) -> Result<()> {
        self.view.webview().terminate_web_process();
        Ok(())
    }
}

/// Matches counted and highlighted at most, as other browsers cap them.
const FIND_MAX_MATCHES: u32 = 1000;

impl Drop for WryView {
    fn drop(&mut self) {
        let _ = self.view.set_visible(false);
        let popups = self.popups.borrow_mut().remove(&self.id).unwrap_or_default();
        for popup in popups {
            // SAFETY: the popup window belongs to this tab, which is closing.
            unsafe { popup.window.destroy() };
        }
    }
}
