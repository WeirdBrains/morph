# Page bridge API (hosts, contexts, reads)

Status: on `main` (2026-11-18). Owner: Morph Lead Dev. For Morph Dev's companion
browser and research hosts. Code: `crates/morph-webview/src/bridge.rs`,
`crates/morph-webview/src/page_read.rs`, `crates/morph-browser/src/host_pages.rs`.
Generic, but tied to the web view boundary, so it lives in `morph-webview` and
`morph-browser` rather than a weirdnet crate.

## API sketch

```rust
// morph_webview ---------------------------------------------------------------

/// Every handler call gets this, built from the engine, never from the page.
pub struct BridgeContext {
    pub view: ViewId,          // the tab's or host page's view; popups carry their opener's
    pub popup: bool,
    pub url: String,           // top-level address as the engine reports it now
    pub origin: String,        // origin_of(url): "scheme://host[:port]" or "null"
    pub frame: FrameKind,      // Main | Sub | Unknown
    pub trust: Trust,          // Page (any page script) | Host (Morph's host scripts)
    pub hidden: bool,          // not on screen (background tab or host page)
    pub background: bool,      // a host page no user ever sees (Spec::background)
}
pub type ContextHandler = Rc<dyn Fn(&BridgeContext, &str) -> Option<String>>;

pub struct PageBridge {
    pub script: Rc<str>,                    // page side: window.MorphBridge
    pub handler: BridgeHandler,             // old context-free handler (kept)
    pub on_message: Option<ContextHandler>, // new: both channels, with context
    pub host_scripts: Vec<Rc<str>>,         // Morph's own: body of function (MorphHost) { … }
}
impl PageBridge {
    pub fn new(script: impl Into<Rc<str>>, handler: BridgeHandler) -> Self;          // as before
    pub fn with_context(script: impl Into<Rc<str>>, on_message: ContextHandler) -> Self;
    pub fn host_script(self, script: impl Into<Rc<str>>) -> Self;                     // MorphHost.post(msg) -> Promise
}
// Message types starting with "host." (bridge::HOST_PREFIX) are refused on the page channel.

pub struct Spec { /* … */ pub background: bool }   // never shown, popups refused

pub trait WebView {
    /* … */
    fn isolation(&self) -> Isolation;                                // None | ScriptWorld | PageWorld
    fn evaluate_isolated(&self, script: &str, done: ScriptDone) -> bool;
    fn engine_url(&self) -> Option<String>;
}

// morph_webview::page_read
pub struct Want { pub title: bool, pub text: bool, pub readable: bool, pub links: bool, pub selection: bool, pub html: bool }
pub struct ReadRequest { pub want: Want, pub max_bytes: usize /* ≤ READ_MAX = 8 MiB */, pub max_links: usize, pub timeout: Duration }
pub struct PageData {
    pub view: ViewId, pub url: String, pub origin: String, pub isolation: Option<Isolation>,
    pub title: Option<String>, pub text: Option<String>, pub readable: Option<Readable /* title, byline, text */>,
    pub links: Option<Vec<Link /* href, text */>>, pub selection: Option<String>, pub html: Option<String>,
    pub truncated: bool, pub bytes: usize,
}
pub enum ReadError { Unsupported, Gone, Timeout, Navigated { from: String, to: String }, TooLarge(usize), Script(String) }
pub type ReadResult = Result<PageData, ReadError>;

pub struct Pending<T>;            // impl Future<Output = T> (!Send, UI thread) + try_take()
pub struct PageReads;             // one per window; Browser owns one
impl PageReads {
    pub fn read(&mut self, view: &dyn WebView, request: ReadRequest) -> Pending<ReadResult>;
    pub fn start(&mut self, view: &dyn WebView, request: ReadRequest, done: ReadDone) -> ReadId;
    pub fn tick<'a>(&mut self, now: Instant, view: impl Fn(ViewId) -> Option<&'a dyn WebView>);
    pub fn forget_view(&mut self, view: ViewId);
}

// morph_browser (what the companion hosts use) ---------------------------------

impl Browser {
    pub fn open_host_page(&mut self, url: &str, load_timeout: Duration) -> morph_webview::Result<HostPageId>;
    pub fn host_page_loaded(&mut self, id: HostPageId) -> Pending<HostPageState>; // Loaded | Failed(why) | Closed
    pub fn host_page_state(&self, id: HostPageId) -> HostPageState;
    pub fn host_page_url(&self, id: HostPageId) -> Option<String>;
    pub fn read_host_page(&mut self, id: HostPageId, request: ReadRequest) -> Pending<ReadResult>;
    pub fn read_tab(&mut self, tab: TabId, request: ReadRequest) -> Pending<ReadResult>;
    pub fn read_active_tab(&mut self, request: ReadRequest) -> Pending<ReadResult>;
    pub fn close_host_page(&mut self, id: HostPageId) -> bool;   // reads under way end with Gone
    pub fn host_pages(&self) -> Vec<HostPageId>;
}
```

Use from a host (GPUI foreground executor, same thread as the browser):

```rust
let id = browser.open_host_page("https://docs.rs/serde", host_pages::LOAD_TIMEOUT)?;
let loaded = browser.host_page_loaded(id);             // awaited while the shell keeps calling browser.poll()
if loaded.await == HostPageState::Loaded {
    let page = browser.read_host_page(id, ReadRequest::new(Want { readable: true, links: true, ..Want::default() })).await?;
    // page.readable, page.links, page.origin (engine's), page.truncated
}
browser.close_host_page(id);
```

Everything is answered from `Browser::poll` (the shell's poll loop): load events,
page-read chunks and timeouts. Futures are `!Send` and resolve on the UI thread.

## How it works

- **Two channels.** The page channel (`window.MorphBridge`, `morphBridge` handler) is
  untrusted: any script on the page, or on WebKitGTK any frame of it, can post on it.
  Its messages arrive with `Trust::Page` and `FrameKind::Unknown`; `host.*` types are
  answered with a `refused` error before a handler sees them. The host channel carries
  Morph's own `host_scripts`, main frame only, with the view's 128-bit secret
  (`ViewBridge::token`, from the OS RNG) and the origin the script saw. `check_host`
  refuses a wrong token, a subframe, or an origin that differs from the engine's (and on
  WKWebView, a sending frame of another origin). Accepted messages arrive with
  `Trust::Host`, `FrameKind::Main`.
- **Context.** `url`/`origin` come from the engine's current top-level URI at the time
  the message is answered; `hidden` tracks `set_visible`; `background` comes from `Spec`.
- **Reads.** `begin_script` runs in `evaluate_isolated`: it gathers what was asked,
  shortens the largest parts until the JSON fits `max_bytes` (`truncated: true`), and
  answers `B<chunks>\n<first>`; Rust asks for `C<chunk>` one at a time (512 Ki UTF-16
  units each) and drops them on a timeout. The result is refused (`Navigated`) when the
  page's `location.origin` and the engine's origin disagree, or the engine's origin moved
  during the read. Answers over the bound are refused (`TooLarge`).
- **Hidden views.** `Spec { background: true, visible: false }` on the window's backend
  (reusing the hidden-view path the download token search already used): never shown,
  `window.open` refused, laid out at 1280×800.

## Per-platform anti-forgery status

| | Host channel | Reads run in | Gaps |
|---|---|---|---|
| Linux, X11 (`wry_linux`) and Wayland (`offscreen`), WebKitGTK | Script world `morph-host` (`UserScript::for_world`, top frame only) + handler registered in that world only; token, frame and origin checked | `morph-host` world (`run_javascript_in_world`) | Page-channel messages can't tell main frame from subframe (`FrameKind::Unknown`): WebKitGTK 4.1's `script-message-received` gives no frame. Pages share the DOM with the world, so a page can still change what its DOM says (its own content). Popups get the page channel only. |
| macOS, WKWebView (`wry_native`) | Page world, inside a closure: the token and wry's `webkit.messageHandlers.ipc` are captured at document start before page scripts run; never on `window`. Token, frame (WKWebView's `frameInfo` URL) and origin checked | Page world (`evaluate_script_with_callback`) | wry 0.57 has no `WKContentWorld`, so page scripts can replace the builtins a host script or read uses after document start (shape a read, see or forge host *replies* through the read-only `window.__morphHost…` deliver function). They cannot forge a host *message* without the token. Fix: `WKContentWorld` through objc2 on `WebViewExtMacOS::webview()` (not done). |
| Windows, WebView2 (`wry_native`) | Page world, inside a closure: `chrome.webview.postMessage` captured at document start; host script returns early outside the top frame (wry injects init scripts into every frame there). Token and origin checked (`Source` is the top document) | Page world | As macOS; WebView2 has no isolated world for `ExecuteScript`. |
| Android, iOS (Flutter) | Not wired: `morph-bridge` still answers `page_bridge::answer` without context | — | Follow-up when the mobile shells need host reads. |

Tests: `cargo test -p morph-webview` (contexts, refusals, envelopes, chunked reads,
timeouts, origin checks on the fake) and `cargo test -p morph-browser host_pages`; the
real-engine forgery test is `crates/morph-webview/tests/bridge_webkit.rs`, which needs a
display:

```sh
cargo test -p morph-webview --test bridge_webkit --no-run   # prints the test binary
env -i HOME=$HOME PATH=/usr/bin:/bin XDG_RUNTIME_DIR=$(mktemp -d) MORPH_BRIDGE_WEBKIT=1 \
  xvfb-run --server-num=137 target/debug/deps/bridge_webkit-<hash>
```

It loads a page that tries `MorphBridge.post({type: "host.read"})`, forges a host
envelope on the page handler, looks for `MorphHost` and the host handler from the page
and from a subframe, then poisons `JSON.stringify`, `Array.prototype.push` and
`document.title`; it checks every attempt is refused or blind, that the host script's
`host.hello` arrives as `Trust::Host` from the main frame, and that a read still gets
the real title, text, readable extract, links and HTML, chunked for a 3 MB page and cut
to 1 MiB when asked. Without `MORPH_BRIDGE_WEBKIT=1` it skips, so plain `cargo test` (CI
included) never opens a display.
