//! The web view boundary (ARCHITECTURE.md sections 2.0 and 2a).
//!
//! Only the browser component depends on this crate. It sees a [`WebView`]
//! per tab, made by a [`Backend`], and polls the backend for [`Event`]s. The
//! [`fake`] backend drives tests without a display; the wry backend
//! (`wry_linux`) places real webkit2gtk views as child windows of the
//! desktop shell's X11 window, and the off-screen backend (`offscreen`)
//! renders them into [`Frame`]s the shell draws itself and takes [`Input`]
//! from the shell (native Wayland, where there are no child windows). On
//! macOS and Windows `wry_native` places WKWebView and WebView2 views as
//! child views of the shell's window.

use std::path::PathBuf;
use std::rc::Rc;

#[cfg(target_os = "linux")]
pub mod appearance;
pub mod clipboard;
pub mod fake;
pub mod icons;
pub mod keys;
pub mod native_page;
#[cfg(target_os = "linux")]
pub mod offscreen;
#[cfg(target_os = "linux")]
pub mod render_probe;
#[cfg(target_os = "linux")]
pub mod wry_linux;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod wry_native;

/// The web engine and its version, for the About page.
pub fn engine_version() -> String {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: these return constants of the loaded library.
        let (major, minor, micro) = unsafe {
            (webkit2gtk::ffi::webkit_get_major_version(), webkit2gtk::ffi::webkit_get_minor_version(), webkit2gtk::ffi::webkit_get_micro_version())
        };
        format!("WebKitGTK {major}.{minor}.{micro}")
    }
    #[cfg(target_os = "macos")]
    {
        match wry::webview_version() {
            Ok(v) => format!("WKWebView (WebKit {v})"),
            Err(_) => "WKWebView (the system's WebKit)".into(),
        }
    }
    #[cfg(target_os = "windows")]
    {
        match wry::webview_version() {
            Ok(v) => format!("Microsoft Edge WebView2 {v}"),
            Err(_) => "Microsoft Edge WebView2 (runtime not found)".into(),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        "none".to_string()
    }
}

/// Matches counted and highlighted at most, as other browsers cap them.
pub const FIND_MAX_MATCHES: u32 = 1000;

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
    /// A `<select>`'s list opened (pages the shell draws): the shell shows
    /// it and answers with [`WebView::option_menu`].
    OptionMenu(OptionMenu),
    /// The open list closed (chosen, cancelled, or the page took it away).
    OptionMenuClosed,
    /// The page's tooltip under the pointer (`title` attributes), or none,
    /// for the shell to draw (pages the shell draws).
    Tooltip(Option<String>),
    /// The page's icon, as PNG bytes (32×32), from [`icons::ICON_SCRIPT`].
    Favicon(Vec<u8>),
    /// The page asked for files (`<input type=file>`); the shell shows a
    /// dialog and answers with [`WebView::choose_files`].
    FileChooser {
        multiple: bool,
    },
    /// A download started, moved on or ended. The backend reports it
    /// for whichever view it likes (downloads belong to the profile's
    /// web data, not to a page).
    Download(Download),
}

/// What happened to download `id` (unique within the backend).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub id: u64,
    pub change: DownloadChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadChange {
    /// It started: `name` is the file name the server suggested
    /// (Content-Disposition, or the URL's last part). The bytes go to
    /// `staged` (see [`download_staging`]) until the shell knows where the
    /// user wants the file and moves it there.
    Started { url: String, name: String, staged: PathBuf },
    /// `received` bytes so far, of `total` when the server said.
    Progress { received: u64, total: Option<u64> },
    /// All bytes are in `staged`.
    Finished,
    /// It stopped short: the network failed or it was cancelled.
    Failed(String),
}

/// A `<select>`'s list, for the shell to draw beside the element.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OptionMenu {
    pub items: Vec<MenuItem>,
    /// The element, in logical pixels from the page's top-left corner.
    pub anchor: Rect,
}

/// One row of an [`OptionMenu`]. Indexes count every row, group labels too.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuItem {
    pub label: String,
    pub tooltip: String,
    pub enabled: bool,
    /// An `<optgroup>` label: shown, never chosen.
    pub group_label: bool,
    /// An option inside an `<optgroup>` (drawn indented).
    pub group_child: bool,
    /// The element's current value.
    pub selected: bool,
}

/// The shell's answer to an open [`OptionMenu`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuChoice {
    /// Show row `n` in the element while the list stays open.
    Highlight(usize),
    /// Choose row `n` (the page gets `input` and `change`) and close.
    Activate(usize),
    /// Close without choosing (the element keeps its value).
    Cancel,
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
    /// The text the last [`Input::Copy`] copied, once, for the shell to put
    /// on the clipboard.
    fn take_copied(&self) -> Option<String> {
        None
    }
    /// The pointer shape the page wants where the pointer last was.
    fn cursor(&self) -> PageCursor {
        PageCursor::Default
    }
    /// Answer the page's open [`Event::OptionMenu`]; nothing when none is.
    fn option_menu(&self, _choice: MenuChoice) {}
    /// Answer the page's [`Event::FileChooser`]: the chosen files, or
    /// `None` when the user cancelled.
    fn choose_files(&self, _paths: Option<Vec<std::path::PathBuf>>) {}
    /// Forget the profile's cookies, cache and site storage (every view of
    /// the same data directory shares them).
    fn clear_data(&self) -> Result<()> {
        Ok(())
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
    /// A paste of the shell's clipboard where the page has the keyboard
    /// (see [`clipboard`]).
    Paste(clipboard::Clipboard),
    /// Copy (or cut) the page's selection; the text comes back from
    /// [`WebView::take_copied`].
    Copy {
        cut: bool,
    },
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
    /// Where downloads are saved from now on; `None` is the system's
    /// Downloads folder.
    fn set_downloads_dir(&mut self, _dir: Option<std::path::PathBuf>) {}
    /// Stop download `id` (the user cancelled the save dialog). Its
    /// staged bytes are the shell's to delete.
    fn cancel_download(&mut self, _id: u64) {}
}

/// Where a download's bytes go until the user picks the file's place: a
/// hidden `.<name>.<id>.morph-part` in `dir` (the downloads folder), so
/// moving it to the chosen place is mostly a rename.
pub fn download_staging(dir: &std::path::Path, name: &str, id: u64) -> std::path::PathBuf {
    dir.join(format!(".{}.{id}.morph-part", safe_name(name)))
}

/// `name` as a plain file name: no folders, nothing hidden, never empty.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().map(|c| if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    if clean.is_empty() { "download".to_string() } else { clean }
}

/// Where a download goes: `dir` (when set) with the file name the engine
/// suggested in `suggested`, numbered like `name (1).ext` when taken.
pub fn download_target(dir: Option<&std::path::Path>, suggested: &std::path::Path) -> std::path::PathBuf {
    let Some(dir) = dir else {
        return suggested.to_path_buf();
    };
    let name = suggested.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "download".into());
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let mut target = dir.join(&name);
    let mut n = 1;
    while target.exists() {
        target = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    target
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    #[test]
    fn download_names_stay_plain_file_names() {
        assert_eq!(safe_name("report.pdf"), "report.pdf");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("C:\\x\\a:b.txt"), "a_b.txt");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name(""), "download");
        let staged = download_staging(std::path::Path::new("/d"), "morph.tar.gz", 7);
        assert_eq!(staged, std::path::PathBuf::from("/d/.morph.tar.gz.7.morph-part"));
    }

    #[test]
    fn downloads_go_to_the_chosen_folder_without_replacing_files() {
        let tmp = std::env::temp_dir().join(format!("morph-dl-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let suggested = std::path::Path::new("/home/x/Downloads/report.pdf");
        assert_eq!(download_target(None, suggested), suggested);
        assert_eq!(download_target(Some(&tmp), suggested), tmp.join("report.pdf"));
        std::fs::write(tmp.join("report.pdf"), b"x").unwrap();
        assert_eq!(download_target(Some(&tmp), suggested), tmp.join("report (1).pdf"));
        assert_eq!(download_target(Some(&tmp), std::path::Path::new("/a/README")), tmp.join("README"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

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
