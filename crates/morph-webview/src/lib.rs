//! The web view boundary (ARCHITECTURE.md sections 2.0 and 2a).
//!
//! Only the browser component depends on this crate. It sees a [`WebView`]
//! per tab, made by a [`Backend`], and polls the backend for [`Event`]s. The
//! [`fake`] backend drives tests without a display; the wry backend
//! (`wry_linux`) places real webkit2gtk views as child windows of the
//! desktop shell's X11 window, and the off-screen backend (`offscreen`)
//! renders them into [`Frame`]s the shell draws itself and takes [`Input`]
//! from the shell (native Wayland, where there are no child windows).

use std::path::PathBuf;
use std::rc::Rc;

pub mod fake;
pub mod keys;
#[cfg(target_os = "linux")]
pub mod offscreen;
#[cfg(target_os = "linux")]
pub mod render_probe;
#[cfg(target_os = "linux")]
pub mod wry_linux;

/// Identifies a web view for the life of its backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewId(pub u64);

/// A rectangle in logical pixels, relative to the host window's content.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width: width.max(0.0), height: height.max(0.0) }
    }
}

/// What a new web view starts with.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spec {
    /// First page; empty for `about:blank`.
    pub url: String,
    /// The profile's web data directory (cookies, storage). `None` keeps
    /// the engine's default, which tests and the fake use.
    pub data_dir: Option<PathBuf>,
    /// Where it sits in the host window.
    pub bounds: Rect,
    pub visible: bool,
}

/// Something a web view reports. Delivered by [`Backend::poll`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    UrlChanged(String),
    TitleChanged(String),
    LoadStarted,
    LoadFinished,
    /// Back/forward availability after a navigation.
    History {
        can_back: bool,
        can_forward: bool,
    },
    /// The page opened a popup window (`window.open`); the backend owns it.
    PopupOpened {
        url: String,
    },
    /// A popup of this view changed its title.
    PopupTitleChanged(String),
    /// A popup of this view closed (`window.close()` or its window).
    PopupClosed,
    /// The page's renderer process ended.
    Crashed,
    /// The page took keyboard focus (the user clicked into it).
    Focused,
    /// A key the shell asked for ([`Backend::set_shortcuts`]) was pressed
    /// while a page had focus, in GPUI keystroke notation (`ctrl-t`).
    Shortcut(String),
    /// A main-frame load failed. Cancelled loads (a newer navigation took
    /// over) and downloads are not reported.
    LoadFailed(LoadError),
    /// The page's process stopped answering (`true`) or answers again.
    Unresponsive(bool),
    /// How many matches the last [`WebView::find`] or
    /// [`WebView::find_next`] found; 0 for none.
    FindResult {
        matches: u32,
    },
}

/// Why a page failed to load, for the shell's error page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The site's TLS certificate is not trusted.
    Certificate,
    /// The host name does not resolve (often a typo).
    NotFound,
    /// The host exists but could not be reached (refused, offline, reset).
    Unreachable,
    /// The server took too long.
    TimedOut,
    /// Anything else; [`LoadError::message`] has the engine's text.
    Other,
}

/// A main-frame load that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub url: String,
    pub kind: ErrorKind,
    /// The engine's own text, possibly empty.
    pub message: String,
}

pub mod errors {
    //! Mapping engine errors (GLib error domains and codes, as WebKitGTK
    //! reports them) to [`ErrorKind`](super::ErrorKind). Kept free of GTK so
    //! it is tested everywhere.
    use super::ErrorKind;

    /// `WEBKIT_NETWORK_ERROR_CANCELLED`.
    const WEBKIT_CANCELLED: i32 = 302;
    /// `WEBKIT_POLICY_ERROR_FRAME_LOAD_INTERRUPTED_BY_POLICY_CHANGE`
    /// (a response turned into a download).
    const POLICY_CHANGE: i32 = 102;
    /// `WEBKIT_PLUGIN_ERROR_WILL_HANDLE_LOAD`.
    const PLUGIN_WILL_HANDLE: i32 = 204;
    /// `G_IO_ERROR_CANCELLED`.
    const IO_CANCELLED: i32 = 19;

    /// Whether a failure deserves an error page. Cancelled loads, downloads
    /// and plugin hand-offs do not.
    pub fn shown(domain: &str, code: i32) -> bool {
        !matches!(
            (domain, code),
            ("WebKitNetworkError", WEBKIT_CANCELLED)
                | ("g-io-error-quark", IO_CANCELLED)
                | ("WebKitPolicyError", POLICY_CHANGE)
                | ("WebKitPluginError", PLUGIN_WILL_HANDLE)
        )
    }

    /// The kind of failure; `tls` is set for certificate failures.
    pub fn kind(domain: &str, code: i32, tls: bool) -> ErrorKind {
        if tls || domain == "g-tls-error-quark" {
            return ErrorKind::Certificate;
        }
        match domain {
            "g-resolver-error-quark" => ErrorKind::NotFound,
            // GIOErrorEnum values.
            "g-io-error-quark" => match code {
                24 => ErrorKind::TimedOut,
                28 => ErrorKind::NotFound,
                39 | 44 | 45 | 46 => ErrorKind::Unreachable,
                _ => ErrorKind::Other,
            },
            _ => ErrorKind::Other,
        }
    }

    /// A short reason for a certificate failure from GLib's
    /// `GTlsCertificateFlags`.
    pub fn tls_reason(flags: u32) -> &'static str {
        const UNKNOWN_CA: u32 = 1 << 0;
        const BAD_IDENTITY: u32 = 1 << 1;
        const NOT_ACTIVATED: u32 = 1 << 2;
        const EXPIRED: u32 = 1 << 3;
        const REVOKED: u32 = 1 << 4;
        const INSECURE: u32 = 1 << 5;
        if flags & BAD_IDENTITY != 0 {
            "The certificate is for a different site."
        } else if flags & EXPIRED != 0 {
            "The certificate has expired."
        } else if flags & NOT_ACTIVATED != 0 {
            "The certificate is not valid yet."
        } else if flags & REVOKED != 0 {
            "The certificate was revoked."
        } else if flags & UNKNOWN_CA != 0 {
            "The certificate is not signed by a trusted authority."
        } else if flags & INSECURE != 0 {
            "The certificate uses insecure cryptography."
        } else {
            "The certificate is not valid."
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn cancelled_loads_and_downloads_show_no_error_page() {
            assert!(!shown("WebKitNetworkError", 302));
            assert!(!shown("g-io-error-quark", 19));
            assert!(!shown("WebKitPolicyError", 102));
            assert!(!shown("WebKitPluginError", 204));
            assert!(shown("g-resolver-error-quark", 0));
            assert!(shown("WebKitNetworkError", 399));
        }

        #[test]
        fn engine_errors_map_to_kinds() {
            assert_eq!(kind("g-resolver-error-quark", 0, false), ErrorKind::NotFound);
            assert_eq!(kind("g-io-error-quark", 28, false), ErrorKind::NotFound);
            assert_eq!(kind("g-io-error-quark", 24, false), ErrorKind::TimedOut);
            assert_eq!(kind("g-io-error-quark", 39, false), ErrorKind::Unreachable);
            assert_eq!(kind("g-io-error-quark", 1, false), ErrorKind::Other);
            assert_eq!(kind("g-tls-error-quark", 3, false), ErrorKind::Certificate);
            assert_eq!(kind("WebKitNetworkError", 399, true), ErrorKind::Certificate);
            assert_eq!(kind("WebKitNetworkError", 399, false), ErrorKind::Other);
        }

        #[test]
        fn certificate_reasons_name_the_worst_problem() {
            assert!(tls_reason(2).contains("different site"));
            assert!(tls_reason(8 | 1).contains("expired"));
            assert!(tls_reason(1).contains("trusted authority"));
            assert!(tls_reason(0).contains("not valid"));
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("web view: {0}")]
    Backend(String),
    #[error("no web view {0:?}")]
    Missing(ViewId),
}

pub type Result<T> = std::result::Result<T, Error>;

/// One page. Methods take `&self`; engines are single-threaded and live on
/// the UI thread.
pub trait WebView {
    fn id(&self) -> ViewId;
    fn load_url(&self, url: &str) -> Result<()>;
    fn back(&self) -> Result<()>;
    fn forward(&self) -> Result<()>;
    fn reload(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn set_bounds(&self, bounds: Rect) -> Result<()>;
    fn set_visible(&self, visible: bool) -> Result<()>;
    /// Give the page keyboard focus.
    fn focus(&self) -> Result<()>;
    /// Highlight `query` in the page (case-insensitive, wrapping) and select
    /// the first match; answered by [`Event::FindResult`].
    fn find(&self, _query: &str) -> Result<()> {
        Ok(())
    }
    /// Move to the next (or, with `forward` false, previous) match.
    fn find_next(&self, _forward: bool) -> Result<()> {
        Ok(())
    }
    /// Drop the find highlights.
    fn find_clear(&self) -> Result<()> {
        Ok(())
    }
    /// End a page that stopped answering; it reports [`Event::Crashed`] and
    /// a reload brings it back.
    fn terminate(&self) -> Result<()> {
        Ok(())
    }
    /// The page as drawn since the last call, for backends whose pages the
    /// shell draws ([`Backend::draws_pages`]); `None` when nothing changed or
    /// the view is hidden.
    fn take_frame(&self) -> Option<Frame> {
        None
    }
    /// Pointer, wheel and key input for the page, for backends whose pages
    /// the shell draws. Coordinates are logical pixels from the page's
    /// top-left corner.
    fn input(&self, _input: Input) {}
    /// The pointer shape the page wants where the pointer last was.
    fn cursor(&self) -> PageCursor {
        PageCursor::Default
    }
}

/// A page drawn off screen: premultiplied BGRA (Cairo's ARGB32 on a
/// little-endian machine), `width * 4` bytes a row, in device pixels.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({}x{})", self.width, self.height)
    }
}

/// The pointer shapes a page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageCursor {
    #[default]
    Default,
    /// Over a link.
    Pointer,
    /// Over editable or selectable text.
    Text,
}

/// Modifier keys held during pointer input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Back,
    Forward,
}

/// One key event for a page. `keycode` and `keyval` are what X11 and GDK
/// use: the XKB keycode (evdev code + 8) and the keysym. A `keycode` of 0
/// means unknown; the backend looks one up for `keyval`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub pressed: bool,
    pub keycode: u32,
    pub keyval: u32,
    /// X11/GDK modifier state: Shift 1, Lock 2, Control 4, Mod1 (Alt) 8,
    /// Mod2 (Num Lock) 16, Mod4 (Super) 64.
    pub state: u32,
    pub group: u32,
    pub is_modifier: bool,
}

/// Input the shell hands a page it draws ([`WebView::input`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    PointerMove {
        x: f64,
        y: f64,
        modifiers: Modifiers,
    },
    PointerButton {
        x: f64,
        y: f64,
        button: MouseButton,
        pressed: bool,
        modifiers: Modifiers,
    },
    /// The pointer left the page.
    PointerLeave,
    /// A wheel or touchpad scroll; `dx`/`dy` in wheel steps (positive
    /// scrolls right/down), or pixels when `precise`.
    Scroll {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        precise: bool,
        modifiers: Modifiers,
    },
    Key(Key),
    /// Text an input method committed.
    Text(String),
}

/// The page bridge a backend injects into every page: `script` runs at
/// document start and exposes `window.MorphBridge`; a message the page posts
/// goes to `handler`, and the reply it returns (JSON) is handed back to the
/// page with `MorphBridge.deliver`. Rust never touches the DOM.
#[derive(Clone)]
pub struct PageBridge {
    pub script: Rc<str>,
    pub handler: BridgeHandler,
}

/// Answers one page message (JSON) with a reply (JSON), or nothing.
pub type BridgeHandler = Rc<dyn Fn(&str) -> Option<String>>;

/// The script that hands `reply` (a JSON object) to the page's bridge.
/// JSON is a JavaScript expression, so it goes in as it is.
pub fn deliver_script(reply: &str) -> String {
    format!("window.MorphBridge && window.MorphBridge.deliver({reply});")
}

/// Makes web views and collects their events.
pub trait Backend {
    fn create(&mut self, spec: Spec) -> Result<Box<dyn WebView>>;
    /// Run the engine's pending work and return events since the last call,
    /// in order.
    fn poll(&mut self) -> Vec<(ViewId, Event)>;
    /// Take keyboard focus back from the pages for the host window (the user
    /// clicked the shell's own UI). Backends without child windows do nothing.
    fn focus_host(&self) {}
    /// Keystrokes (GPUI notation, such as `ctrl-t`) that the shell handles
    /// even while a page has focus. A backend whose pages take keyboard focus
    /// away from the shell reports them as [`Event::Shortcut`] instead of
    /// passing them to the page.
    fn set_shortcuts(&mut self, _keystrokes: &[String]) {}
    /// Inject `bridge` into the pages of views created from now on.
    fn set_page_bridge(&mut self, _bridge: PageBridge) {}
    /// Whether the shell draws the pages ([`WebView::take_frame`]) and
    /// hands them input ([`WebView::input`]), instead of the pages being
    /// windows of their own.
    fn draws_pages(&self) -> bool {
        false
    }
    /// The view that has the keyboard (the last one [`WebView::focus`]ed,
    /// unless [`Backend::focus_host`] took it back since). Backends whose
    /// pages are windows leave this to the window system and say `None`.
    fn keyboard(&self) -> Option<ViewId> {
        None
    }
    /// The host window's scale (device pixels per logical pixel), for
    /// backends that render pages themselves.
    fn set_scale(&mut self, _scale: f64) {}
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    #[test]
    fn a_reply_is_delivered_as_an_object() {
        assert_eq!(deliver_script(r#"{"id":"1","type":"pong"}"#), r#"window.MorphBridge && window.MorphBridge.deliver({"id":"1","type":"pong"});"#);
    }

    #[test]
    fn backends_without_a_bridge_ignore_it() {
        let mut backend = fake::FakeBackend::default();
        backend.set_page_bridge(PageBridge { script: "".into(), handler: Rc::new(|_| None) });
    }
}
