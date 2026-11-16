//! The Linux backend: webkit2gtk views placed by wry as child windows of the
//! desktop shell's X11 window (ARCHITECTURE.md section 2.0).
//!
//! X11 only: wry has no child web view on Wayland, so the desktop shell runs
//! through XWayland on Wayland sessions. GTK must be initialised with
//! [`init`] on the UI thread, and [`Backend::poll`] pumps GTK's main loop, so
//! the shell calls it often (every few milliseconds).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib::Propagation;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use javascriptcore::ValueExt;
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle, XlibWindowHandle};
use webkit2gtk::{FindControllerExt, FindOptions, LoadEvent, UserContentManagerExt, WebViewExt};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{NewWindowFeatures, NewWindowResponse, WebContext, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix};

use crate::{Backend, Error, Event, LoadError, PageBridge, Rect, Result, Spec, ViewId, WebView, deliver_script, errors};

/// Start GTK on this thread. Call once, before [`WryBackend::new`].
pub fn init() -> Result<()> {
    gtk::init().map_err(|e| Error::Backend(format!("GTK did not start: {e}")))?;
    crate::appearance::start();
    Ok(())
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

pub(crate) type Queue = Rc<RefCell<Vec<(ViewId, Event)>>>;

/// WebKitGTK leaves undo and redo to the embedding app (Epiphany binds them
/// as app shortcuts). Morph does it in the page, after the page's own
/// handlers: if nothing called `preventDefault()` on Ctrl+Z, Ctrl+Shift+Z or
/// Ctrl+Y, the page's editing undo or redo runs. Pages with their own undo
/// (editors) keep it.
pub(crate) const UNDO_REDO_SCRIPT: &str = r#"(() => {
  addEventListener("keydown", (e) => {
    if (!e.ctrlKey || e.altKey || e.metaKey) return;
    const key = e.key.toLowerCase();
    const command = key === "z" ? (e.shiftKey ? "redo" : "undo") : (key === "y" && !e.shiftKey ? "redo" : null);
    if (!command) return;
    setTimeout(() => { if (!e.defaultPrevented) document.execCommand(command); }, 0);
  });
})();"#;

/// WebKitGTK draws a `<select>` with its own face, light or dark by the
/// element's `color-scheme`, and ignores the page's background for it, so
/// a dark site that styles its selects (`background: #1c2027; color: #eee`)
/// without saying `color-scheme: dark` got light text on a white face where
/// Chrome shows the page's dark one. A select whose page background is dark
/// and that asks for no scheme of its own is given `color-scheme: dark`
/// (and checked again when the page changes or the system scheme flips).
pub(crate) const CONTROL_SCHEME_SCRIPT: &str = r#"(() => {
  const MARK = "data-morph-scheme";
  const luminance = (color) => {
    const m = /rgba?\(([^)]+)\)/.exec(color);
    if (!m) return null;
    const [r, g, b, a = 1] = m[1].split(/[\s,\/]+/).filter(Boolean).map(Number);
    return a < 0.5 ? null : (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  };
  const fix = (el) => {
    if (el.hasAttribute(MARK)) {
      el.style.removeProperty("color-scheme");
      el.removeAttribute(MARK);
    }
    const style = getComputedStyle(el);
    if (style.colorScheme && style.colorScheme !== "normal") return;
    const bg = luminance(style.backgroundColor);
    if (bg !== null && bg < 0.4) {
      el.style.setProperty("color-scheme", "dark");
      el.setAttribute(MARK, "dark");
    }
  };
  let queued = false;
  const run = () => {
    queued = false;
    document.querySelectorAll("select").forEach(fix);
  };
  const later = () => {
    if (!queued) { queued = true; requestAnimationFrame(run); }
  };
  document.addEventListener("DOMContentLoaded", () => {
    run();
    new MutationObserver((records) => {
      if (records.some((r) => r.type === "childList" || (r.target.tagName === "SELECT" && r.attributeName !== MARK) || r.attributeName === "class")) later();
    }).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "style"] });
  });
  addEventListener("load", later);
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", later);
})();"#;

pub(crate) struct Popup {
    pub(crate) window: gtk::Window,
    _view: wry::WebView,
}

pub(crate) type Popups = Rc<RefCell<HashMap<ViewId, Vec<Popup>>>>;

/// The wry backend for one shell window.
pub struct WryBackend {
    parent: XlibParent,
    next: u64,
    queue: Queue,
    popups: Popups,
    contexts: HashMap<Option<PathBuf>, WebContext>,
    shortcuts: Rc<RefCell<HashSet<String>>>,
    keys: HostKeys,
    bridge: Option<PageBridge>,
    /// The shell's scale and every page's engine, so pages are zoomed to
    /// the shell's scale wherever GTK's own differs (GTK reads its scale
    /// from the X settings, GPUI from Xft.dpi).
    scale: f64,
    engines: Vec<gtk::glib::WeakRef<webkit2gtk::WebView>>,
}

/// The zoom that makes a page as large as the shell around it.
pub(crate) fn page_zoom(shell_scale: f64, gtk_scale: i32) -> f64 {
    shell_scale.max(0.5) / f64::from(gtk_scale.max(1))
}

fn apply_zoom(engine: &webkit2gtk::WebView, shell_scale: f64) {
    let zoom = page_zoom(shell_scale, engine.scale_factor());
    if (engine.zoom_level() - zoom).abs() > 1e-6 {
        engine.set_zoom_level(zoom);
    }
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
    /// Key events a web view has already taken in (time, keycode, press).
    /// WebKit answers a key the page did not handle by sending the same
    /// event through GTK again, starting at the page's window; that copy
    /// must reach the web view, which expects it back (it skips the next key
    /// it sees otherwise, and the page loses a real key).
    seen: Rc<RefCell<SeenKeys>>,
}

/// How many key events [`SeenKeys`] remembers.
const SEEN_KEYS: usize = 64;

/// Key events (X time, keycode, press) a web view took in, newest last.
#[derive(Default)]
struct SeenKeys(VecDeque<(u32, u16, bool)>);

impl SeenKeys {
    fn saw(&mut self, id: (u32, u16, bool)) {
        if self.0.len() == SEEN_KEYS {
            self.0.pop_front();
        }
        self.0.push_back(id);
    }

    /// Whether `id` was taken in before (a resend); forgets it.
    fn take(&mut self, id: (u32, u16, bool)) -> bool {
        match self.0.iter().position(|k| *k == id) {
            Some(i) => {
                self.0.remove(i);
                true
            }
            None => false,
        }
    }
}

/// Polls (8 ms apart, so about a second) to keep re-checking a page's focus;
/// a resize or a panel closing beside it can take it back meanwhile.
const FOCUS_RETRIES: u8 = 125;

impl HostKeys {
    fn set_active(&self, active: bool) {
        self.active.set(active);
    }

    /// A web view took in `event`; remember it so its resend is let through.
    fn saw(&self, event: &gtk::gdk::EventKey, press: bool) {
        self.seen.borrow_mut().saw(key_id(event, press));
    }

    /// Whether `event` is WebKit sending back a key a web view already took
    /// in (and forgets it).
    fn is_resend(&self, event: &gtk::gdk::EventKey, press: bool) -> bool {
        self.seen.borrow_mut().take(key_id(event, press))
    }

    /// Forward `event` to the shell if the shell has focus. Returns whether
    /// it was forwarded (and must not reach the page).
    fn forward(&self, event: &gtk::gdk::EventKey, press: bool) -> bool {
        if self.is_resend(event, press) || !self.active.get() {
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
            keys: HostKeys { conn: XConn::open().map(Rc::new), host: window, active: Rc::default(), pending: Rc::default(), seen: Rc::default() },
            bridge: None,
            scale: 1.0,
            engines: Vec::new(),
        }
    }
}

/// X11 refuses windows of zero size (`BadValue` from `CreateWindow`), which
/// is what a page area is before the first layout, so sizes are at least 1.
fn wry_rect(bounds: Rect) -> wry::Rect {
    let size = LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0));
    wry::Rect { position: LogicalPosition::new(bounds.x, bounds.y).into(), size: size.into() }
}

pub(crate) fn backend_err(e: impl std::fmt::Display) -> Error {
    Error::Backend(e.to_string())
}

/// What identifies one key event: its X time, keycode and direction.
fn key_id(event: &gtk::gdk::EventKey, press: bool) -> (u32, u16, bool) {
    (event.time(), event.hardware_keycode(), press)
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
pub(crate) fn watch(view: &webkit2gtk::WebView, id: ViewId, queue: &Queue, shortcuts: &Rc<RefCell<HashSet<String>>>) {
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

/// Maps the bridge's `window.morphRust.postMessage` onto a WebKit script
/// message handler of Morph's own. (wry's IPC drops messages from pages whose
/// URL is not a valid `http::Uri`, such as `file://` pages.)
const BRIDGE_SHIM: &str = r#"(() => {
  if (window.morphRust && typeof window.morphRust.postMessage === "function") return;
  const handler = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.morphBridge;
  if (!handler) return;
  window.morphRust = Object.freeze({ postMessage: (payload) => handler.postMessage(String(payload)) });
})();"#;

/// Inject the page bridge's scripts (if any).
pub(crate) fn with_bridge<'a>(builder: WebViewBuilder<'a>, bridge: &Option<PageBridge>) -> WebViewBuilder<'a> {
    match bridge {
        Some(bridge) => builder.with_initialization_script(BRIDGE_SHIM).with_initialization_script(&*bridge.script),
        None => builder,
    }
}

/// Answer the bridge's messages from `engine`'s pages in the page that sent
/// them.
pub(crate) fn attach_bridge(engine: &webkit2gtk::WebView, bridge: &Option<PageBridge>) {
    let Some(bridge) = bridge.clone() else {
        return;
    };
    let Some(manager) = engine.user_content_manager() else {
        return;
    };
    let target = engine.downgrade();
    manager.connect_script_message_received(Some("morphBridge"), move |_, message| {
        let Some(body) = message.js_value().map(|v| v.to_str().to_string()) else {
            return;
        };
        let (Some(reply), Some(engine)) = ((bridge.handler)(&body), target.upgrade()) else {
            return;
        };
        engine.run_javascript(&deliver_script(&reply), None::<&webkit2gtk::gio::Cancellable>, |_| {});
    });
    manager.register_script_message_handler("morphBridge");
}

/// Handles `window.open` from a page of tab `tab`: Morph makes the popup
/// itself, a GTK window holding a web view related to the opener (same
/// process and storage, so `window.opener` and `postMessage` work). wry's
/// default response would panic for a child web view.
pub(crate) fn popup_handler(
    tab: ViewId,
    queue: Queue,
    popups: Popups,
    bridge: Option<PageBridge>,
) -> impl Fn(String, NewWindowFeatures) -> NewWindowResponse + 'static {
    move |url, features| {
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        let (w, h) = features.size.map(|s| (s.width as i32, s.height as i32)).unwrap_or((800, 600));
        window.set_default_size(w.max(200), h.max(150));
        window.set_title("Morph");
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&vbox);
        let builder = WebViewBuilder::new()
            .with_related_view(features.opener.webview.clone())
            .with_initialization_script(UNDO_REDO_SCRIPT)
            .with_initialization_script(CONTROL_SCHEME_SCRIPT)
            .with_new_window_req_handler(popup_handler(tab, queue.clone(), popups.clone(), bridge.clone()));
        let built = with_bridge(builder, &bridge).build_gtk(&vbox);
        let view = match built {
            Ok(view) => view,
            Err(_) => {
                // SAFETY: the window was never shown or shared.
                unsafe { window.destroy() };
                return NewWindowResponse::Deny;
            }
        };
        let engine = view.webview();
        attach_bridge(&engine, &bridge);
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
        // `window.close()` in the popup closes its window. wry answers
        // WebKit's `close` signal by destroying the web view, which also
        // drops every other handler of that signal, so the window follows
        // the web view's `destroy` instead (the popup stayed open, empty,
        // after a sign-in page closed itself).
        let closing = window.clone();
        engine.connect_destroy(move |_| {
            // SAFETY: GTK owns the window; destroying it runs `destroy`
            // below (a second destroy, from Ctrl+W, is a no-op).
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
        let builder = WebViewBuilder::new_with_web_context(context)
            .with_url(url)
            .with_bounds(wry_rect(spec.bounds))
            .with_initialization_script(UNDO_REDO_SCRIPT)
            .with_initialization_script(CONTROL_SCHEME_SCRIPT)
            .with_visible(spec.visible)
            .with_new_window_req_handler(popup_handler(id, self.queue.clone(), self.popups.clone(), self.bridge.clone()));
        let view = with_bridge(builder, &self.bridge).build_as_child(&self.parent).map_err(backend_err)?;
        let engine = view.webview();
        attach_bridge(&engine, &self.bridge);
        watch(&engine, id, &self.queue, &self.shortcuts);
        apply_zoom(&engine, self.scale);
        self.engines.retain(|e| e.upgrade().is_some());
        self.engines.push(engine.downgrade());
        let keys = self.keys.clone();
        engine.connect_button_press_event(move |engine, _| {
            keys.focus_page(page_window(engine));
            Propagation::Proceed
        });
        // Runs before WebKit's own key handling: note what the web view takes in.
        let keys = self.keys.clone();
        engine.connect_key_press_event(move |_, e| {
            keys.saw(e, true);
            Propagation::Proceed
        });
        let keys = self.keys.clone();
        engine.connect_key_release_event(move |_, e| {
            keys.saw(e, false);
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

    fn set_page_bridge(&mut self, bridge: PageBridge) {
        self.bridge = Some(bridge);
    }

    fn set_scale(&mut self, scale: f64) {
        if (self.scale - scale).abs() < 1e-9 {
            return;
        }
        self.scale = scale;
        for engine in self.engines.iter().filter_map(|e| e.upgrade()) {
            apply_zoom(&engine, scale);
        }
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

pub(crate) use crate::FIND_MAX_MATCHES;

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

#[cfg(test)]
mod tests {
    use super::{SEEN_KEYS, SeenKeys};

    #[test]
    fn a_resent_key_is_recognised_once() {
        let mut seen = SeenKeys::default();
        seen.saw((100, 37, true));
        assert!(!seen.take((100, 37, false)), "the release is another event");
        assert!(!seen.take((101, 37, true)), "a later press is a real key");
        assert!(seen.take((100, 37, true)), "WebKit sending the press back");
        assert!(!seen.take((100, 37, true)), "only once");
    }

    #[test]
    fn only_recent_keys_are_remembered() {
        let mut seen = SeenKeys::default();
        for t in 0..(SEEN_KEYS as u32 + 10) {
            seen.saw((t, 38, true));
        }
        assert!(!seen.take((0, 38, true)));
        assert!(seen.take((SEEN_KEYS as u32 + 9, 38, true)));
    }
}
