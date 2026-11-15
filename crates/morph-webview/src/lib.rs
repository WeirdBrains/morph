//! The web view boundary (ARCHITECTURE.md sections 2.0 and 2a).
//!
//! Only the browser component depends on this crate. It sees a [`WebView`]
//! per tab, made by a [`Backend`], and polls the backend for [`Event`]s. The
//! [`fake`] backend drives tests without a display; the wry backend
//! (`wry_linux`) places real webkit2gtk views as child windows of the
//! desktop shell's window.

use std::path::PathBuf;

pub mod fake;
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
}
