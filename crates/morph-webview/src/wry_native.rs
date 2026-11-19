//! The macOS and Windows backend: wry's WKWebView (macOS) and WebView2
//! (Windows) placed as child views of the desktop shell's window
//! (ARCHITECTURE.md section 2.0). The page is a native view above GPUI's
//! surface, as on X11, so the shell lays out beside it.
//!
//! These engines report less than WebKitGTK, so part of what the shell needs
//! comes from a small script in every page (`native_page::PAGE_SCRIPT`): address changes
//! made by the page itself (`history.pushState`), focus, and the shell's
//! shortcuts while the page has the keyboard. Its messages arrive through
//! wry's IPC as one line, a letter and a payload (`native_page::Message`).
//!
//! Not done here: popups (`window.open`) open as the engine's own windows
//! and are not tracked as the tab's popups; the renderer process ending is
//! reported on macOS only; on macOS profiles get their own data store only
//! from macOS 14 (older systems share one).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebViewBuilder};

use crate::bridge::{self, HostTransport, ViewBridge};
use crate::native_page::{Message, PAGE_SCRIPT, canonical, find_script, keys_script};
use crate::{Backend, Error, Event, PageBridge, Rect, Result, Spec, ViewId, WebView, deliver_script};

/// The host window, as wry needs it for `build_as_child`.
pub struct Parent(RawWindowHandle);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the shell's window outlives the backend (the browser, and
        // with it the backend, is dropped before the window closes).
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

type Queue = Rc<RefCell<Vec<(ViewId, Event)>>>;

/// What one view's handlers saw, for [`Backend::poll`] to finish: bridge
/// replies to deliver, and whether a page loaded (its shortcut list and
/// history state need refreshing).
#[derive(Default)]
struct Pending {
    /// Page-channel messages, answered in `poll` with the view's context.
    page: Vec<String>,
    /// Host-channel envelopes and the address of the frame that sent each.
    host: Vec<(String, String)>,
    loaded: bool,
}

/// The host script travels in the page's world here: wry has no content
/// worlds (docs/bridge-api.md, "Per-platform status").
#[cfg(target_os = "macos")]
const TRANSPORT: HostTransport = HostTransport::WebKitIpc;
#[cfg(target_os = "windows")]
const TRANSPORT: HostTransport = HostTransport::WebView2;

pub struct NativeBackend {
    parent: Parent,
    next: u64,
    queue: Queue,
    contexts: HashMap<Option<PathBuf>, WebContext>,
    shortcuts: Vec<String>,
    bridge: Option<PageBridge>,
    views: Vec<(ViewId, std::rc::Weak<Shared>)>,
    downloads: Rc<Downloads>,
}

/// Downloads: where they go and the ones under way, by URL (wry names a
/// download by its URL only).
#[derive(Default)]
struct Downloads {
    /// The downloads folder (`None`: the system's).
    dir: RefCell<Option<PathBuf>>,
    next: std::cell::Cell<u64>,
    live: RefCell<Vec<(u64, String)>>,
    cancelled: RefCell<std::collections::HashSet<u64>>,
    /// The view each download started in: its profile's cookies are asked
    /// through it ([`Backend::request_cookies`]).
    started_in: RefCell<HashMap<u64, ViewId>>,
}

/// Report a view's downloads as [`Event::Download`]: the bytes go to a
/// staging file in the downloads folder, and the shell moves the file to
/// where the user wants it. WKWebView and WebView2 tell only the start
/// and the end, so there is no progress in between.
fn with_downloads<'a>(builder: WebViewBuilder<'a>, id: ViewId, downloads: &Rc<Downloads>, queue: &Queue) -> WebViewBuilder<'a> {
    let (d, q) = (downloads.clone(), queue.clone());
    let builder = builder.with_download_started_handler(move |url, path| {
        let n = d.next.get() + 1;
        d.next.set(n);
        let name = crate::safe_name(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let dir = d.dir.borrow().clone().or_else(|| path.parent().map(PathBuf::from)).unwrap_or_else(|| PathBuf::from("."));
        let staged = crate::download_staging(&dir, &name, n);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::remove_file(&staged);
        *path = staged.clone();
        d.live.borrow_mut().push((n, url.clone()));
        d.started_in.borrow_mut().insert(n, id);
        q.borrow_mut().push((id, Event::Download(crate::Download { id: n, change: crate::DownloadChange::Started { url, name, staged } })));
        true
    });
    let (d, q) = (downloads.clone(), queue.clone());
    builder.with_download_completed_handler(move |url, _path, ok| {
        let mut live = d.live.borrow_mut();
        let Some(i) = live.iter().position(|(_, u)| *u == url) else {
            return;
        };
        let (n, _) = live.remove(i);
        let change = if d.cancelled.borrow_mut().remove(&n) {
            crate::DownloadChange::Failed("cancelled".into())
        } else if ok {
            crate::DownloadChange::Finished
        } else {
            crate::DownloadChange::Failed("the download failed".into())
        };
        q.borrow_mut().push((id, Event::Download(crate::Download { id: n, change })));
    })
}

/// What a view and its handlers share.
struct Shared {
    view: wry::WebView,
    pending: Rc<RefCell<Pending>>,
    bridge: ViewBridge,
}

impl NativeBackend {
    /// A backend whose views are children of `window` (an AppKit or Win32
    /// window handle). `None` for any other kind of window.
    pub fn new(window: RawWindowHandle) -> Option<Self> {
        match window {
            RawWindowHandle::AppKit(_) | RawWindowHandle::Win32(_) => Some(Self {
                parent: Parent(window),
                next: 0,
                queue: Rc::default(),
                contexts: HashMap::new(),
                shortcuts: Vec::new(),
                bridge: None,
                views: Vec::new(),
                downloads: Rc::default(),
            }),
            _ => None,
        }
    }
}

fn wry_rect(bounds: Rect) -> wry::Rect {
    let size = LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0));
    wry::Rect { position: LogicalPosition::new(bounds.x, bounds.y).into(), size: size.into() }
}

fn backend_err(e: impl std::fmt::Display) -> Error {
    Error::Backend(e.to_string())
}

impl Backend for NativeBackend {
    fn create(&mut self, spec: Spec) -> Result<Box<dyn WebView>> {
        self.next += 1;
        let id = ViewId(self.next);
        let url = if spec.url.is_empty() { "about:blank".to_string() } else { spec.url.clone() };
        let pending: Rc<RefCell<Pending>> = Rc::default();
        let view_bridge = ViewBridge::new(id, spec.visible, spec.background);
        let context = self.contexts.entry(spec.data_dir.clone()).or_insert_with(|| WebContext::new(spec.data_dir.clone()));
        let mut builder = WebViewBuilder::new_with_web_context(context)
            .with_url(url)
            .with_bounds(wry_rect(spec.bounds))
            .with_visible(spec.visible && !spec.background)
            .with_initialization_script_for_main_only(PAGE_SCRIPT, true)
            .with_initialization_script_for_main_only(crate::icons::ICON_SCRIPT, true);
        builder = with_downloads(builder, id, &self.downloads, &self.queue);
        if let Some(bridge) = &self.bridge {
            builder = builder.with_initialization_script(&*bridge.script);
            if !bridge.host_scripts.is_empty() {
                // After wry's own `ipc` script, which it adds first; the
                // host script captures what it needs before pages run.
                builder = builder.with_initialization_script_for_main_only(bridge::host_script(&view_bridge, TRANSPORT, &bridge.host_scripts), true);
            }
        }
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewBuilderExtDarwin;
            if let Some(dir) = &spec.data_dir {
                builder = builder.with_data_store_identifier(crate::native_page::store_id(dir));
            }
            let q = self.queue.clone();
            builder = builder.with_on_web_content_process_terminate_handler(move || q.borrow_mut().push((id, Event::Crashed)));
        }
        #[cfg(target_os = "windows")]
        {
            // WebView2 runs Edge's own accelerators first (Ctrl+D adds a
            // favorite, Ctrl+Shift+O opens favorites, Ctrl+H history,
            // Ctrl+F its find bar), so Morph's shortcuts never reached the
            // page script that reports them. Without them, every key
            // reaches the page, and the page script hands Morph its own.
            use wry::WebViewBuilderExtWindows;
            builder = builder.with_browser_accelerator_keys(false);
        }
        let q = self.queue.clone();
        builder = builder.with_document_title_changed_handler(move |title| q.borrow_mut().push((id, Event::TitleChanged(title))));
        let (q, p) = (self.queue.clone(), pending.clone());
        builder = builder.with_on_page_load_handler(move |event, url| {
            let mut q = q.borrow_mut();
            q.push((id, Event::UrlChanged(url)));
            match event {
                PageLoadEvent::Started => q.push((id, Event::LoadStarted)),
                PageLoadEvent::Finished => {
                    q.push((id, Event::LoadFinished));
                    p.borrow_mut().loaded = true;
                }
            }
        });
        // Popups are the engine's own windows (see the module comment).
        // A page no one sees opens no windows.
        let background = spec.background;
        builder = builder.with_new_window_req_handler(move |_, _| if background { NewWindowResponse::Deny } else { NewWindowResponse::Allow });
        let (q, p) = (self.queue.clone(), pending.clone());
        builder = builder.with_ipc_handler(move |request| {
            let event = match Message::parse(request.body()) {
                Some(Message::Bridge(body)) => {
                    p.borrow_mut().page.push(body);
                    return;
                }
                Some(Message::Host(body)) => {
                    p.borrow_mut().host.push((body, request.uri().to_string()));
                    return;
                }
                Some(Message::Focused) => Event::Focused,
                Some(Message::Shortcut(key)) => Event::Shortcut(key),
                Some(Message::Url(url)) => {
                    p.borrow_mut().loaded = true;
                    Event::UrlChanged(url)
                }
                Some(Message::Found(matches)) => Event::FindResult { matches },
                Some(Message::Icon(data)) => match crate::icons::decode(&data) {
                    Some(png) => Event::Favicon(png),
                    None => return,
                },
                None => return,
            };
            q.borrow_mut().push((id, event));
        });
        let view = builder.build_as_child(&self.parent).map_err(backend_err)?;
        let shared = Rc::new(Shared { view, pending, bridge: view_bridge });
        self.views.retain(|(_, v)| v.strong_count() > 0);
        self.views.push((id, Rc::downgrade(&shared)));
        Ok(Box::new(NativeView { id, shared, bounds: Cell::new(spec.bounds) }))
    }

    fn poll(&mut self) -> Vec<(ViewId, Event)> {
        // The engines run on the shell's own event loop (AppKit, Win32); what
        // is left is answering their handlers where a view is needed.
        let keys = keys_script(&self.shortcuts);
        for (id, shared) in self.views.iter().filter_map(|(id, v)| Some((*id, v.upgrade()?))) {
            let (page, host, loaded) = {
                let mut p = shared.pending.borrow_mut();
                (std::mem::take(&mut p.page), std::mem::take(&mut p.host), std::mem::take(&mut p.loaded))
            };
            if let Some(bridge) = &self.bridge
                && !(page.is_empty() && host.is_empty())
            {
                let url = shared.view.url().unwrap_or_default();
                for body in page {
                    if let Some(reply) = bridge.answer_page(&shared.bridge, &url, &body) {
                        let _ = shared.view.evaluate_script(&deliver_script(&reply));
                    }
                }
                for (body, frame_url) in host {
                    if let Some(reply) = bridge.answer_host(&shared.bridge, &url, Some(&frame_url), &body) {
                        let _ = shared.view.evaluate_script(&bridge::host_deliver_script(&shared.bridge, &reply));
                    }
                }
            }
            if loaded {
                let _ = shared.view.evaluate_script(&keys);
                let can_back = shared.view.can_go_back().unwrap_or(false);
                let can_forward = shared.view.can_go_forward().unwrap_or(false);
                self.queue.borrow_mut().push((id, Event::History { can_back, can_forward }));
            }
        }
        std::mem::take(&mut self.queue.borrow_mut())
    }

    fn focus_host(&self) {
        for shared in self.views.iter().filter_map(|(_, v)| v.upgrade()) {
            let _ = shared.view.focus_parent();
        }
    }

    fn set_shortcuts(&mut self, keystrokes: &[String]) {
        self.shortcuts = keystrokes.iter().filter_map(|k| canonical(k)).collect();
        let keys = keys_script(&self.shortcuts);
        for shared in self.views.iter().filter_map(|(_, v)| v.upgrade()) {
            let _ = shared.view.evaluate_script(&keys);
        }
    }

    fn set_page_bridge(&mut self, bridge: PageBridge) {
        self.bridge = Some(bridge);
    }

    fn set_downloads_dir(&mut self, dir: Option<std::path::PathBuf>) {
        *self.downloads.dir.borrow_mut() = dir;
    }

    /// wry cannot stop a download: it runs on, and its end reports the
    /// cancel (the shell deletes the staged bytes then).
    fn request_cookies(&mut self, download: u64, url: &str) -> bool {
        let Some(view) = self.downloads.started_in.borrow().get(&download).copied() else {
            return false;
        };
        let Some(shared) = self.views.iter().find(|(v, _)| *v == view).and_then(|(_, s)| s.upgrade()) else {
            return false;
        };
        // WKWebView's and WebView2's cookie stores answer here and now.
        let cookies = shared.view.cookies_for_url(url).ok().map(|list| {
            list.into_iter()
                .map(|c| crate::Cookie {
                    name: c.name().to_string(),
                    value: c.value().to_string(),
                    domain: c.domain().unwrap_or_default().to_string(),
                    path: c.path().unwrap_or_default().to_string(),
                    secure: c.secure().unwrap_or(false),
                })
                .collect()
        });
        self.queue.borrow_mut().push((view, Event::Cookies { download, cookies }));
        true
    }

    fn cancel_download(&mut self, id: u64) {
        self.downloads.cancelled.borrow_mut().insert(id);
    }
}

struct NativeView {
    id: ViewId,
    shared: Rc<Shared>,
    bounds: Cell<Rect>,
}

impl NativeView {
    fn view(&self) -> &wry::WebView {
        &self.shared.view
    }
}

impl WebView for NativeView {
    fn evaluate(&self, script: &str, done: crate::ScriptDone) -> bool {
        crate::wry_evaluate(self.view(), script, done)
    }
    fn isolation(&self) -> crate::Isolation {
        crate::Isolation::PageWorld
    }
    fn evaluate_isolated(&self, script: &str, done: crate::ScriptDone) -> bool {
        crate::wry_evaluate_string(self.view(), script, done)
    }
    fn engine_url(&self) -> Option<String> {
        self.view().url().ok()
    }

    fn id(&self) -> ViewId {
        self.id
    }
    fn load_url(&self, url: &str) -> Result<()> {
        self.view().load_url(url).map_err(backend_err)
    }
    fn back(&self) -> Result<()> {
        self.view().go_back().map_err(backend_err)
    }
    fn forward(&self) -> Result<()> {
        self.view().go_forward().map_err(backend_err)
    }
    fn reload(&self) -> Result<()> {
        self.view().reload().map_err(backend_err)
    }
    fn stop(&self) -> Result<()> {
        self.view().evaluate_script("window.stop();").map_err(backend_err)
    }
    fn set_bounds(&self, bounds: Rect) -> Result<()> {
        self.bounds.set(bounds);
        self.view().set_bounds(wry_rect(bounds)).map_err(backend_err)
    }
    fn set_visible(&self, visible: bool) -> Result<()> {
        // A background view is never shown.
        let visible = visible && !self.shared.bridge.background;
        self.shared.bridge.set_visible(visible);
        self.view().set_visible(visible).map_err(backend_err)?;
        if visible {
            self.view().set_bounds(wry_rect(self.bounds.get())).map_err(backend_err)?;
        }
        Ok(())
    }
    fn focus(&self) -> Result<()> {
        self.view().focus().map_err(backend_err)
    }
    fn find(&self, query: &str) -> Result<()> {
        self.view().evaluate_script(&find_script(query)).map_err(backend_err)
    }
    fn clear_data(&self) -> Result<()> {
        self.view().clear_all_browsing_data().map_err(backend_err)
    }
    fn find_next(&self, forward: bool) -> Result<()> {
        let script = format!("window.__morphFind && window.find(window.__morphFind, false, {}, true);", !forward);
        self.view().evaluate_script(&script).map_err(backend_err)
    }
    fn find_clear(&self) -> Result<()> {
        self.view().evaluate_script("window.__morphFind = ''; getSelection().removeAllRanges();").map_err(backend_err)
    }
}

impl Drop for NativeView {
    fn drop(&mut self) {
        let _ = self.view().set_visible(false);
    }
}
