# Page bridge API (hosts, contexts, reads, actions, throwaway sessions)

Status: on `main` (2026-11-19). Owner: Morph Lead Dev. For Morph Dev's companion
browser and research hosts. Code: `crates/morph-webview/src/bridge.rs`,
`crates/morph-webview/src/page_read.rs`, `crates/morph-webview/src/page_action.rs`,
`crates/morph-browser/src/host_pages.rs`.
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

pub struct Spec { /* … */ pub background: bool, pub storage: Storage }   // background: never shown, popups refused
pub enum Storage { Profile /* default: the profile's data dir */, Ephemeral /* throwaway, see below */ }
pub trait Backend { /* … */ fn ephemeral_storage(&self) -> bool; }   // false: Ephemeral creates fail with Error::Unsupported

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

// morph_webview::page_action (tier 2: the caller already has the user's approval)
pub struct Locator { pub selector: Option<String>, pub role: Option<String>, pub name: Option<String>, pub text: Option<String>, pub nth: usize }
// Locator::selector("#q"), Locator::role("button", Some("Send")), Locator::name("Email"), Locator::text("Sign in").nth(1)
pub enum Action {
    Click(Locator), Fill { target: Locator, value: String }, Focus(Locator), Read(Locator),
    Highlight { target: Locator, duration: Duration /* ≤ 30 s */ },
    ScrollBy { dx: f64, dy: f64 }, ScrollTo(Locator),
}
pub struct ActionRequest { pub action: Action, pub expect_origin: Option<String>, pub allow_page_world: bool, pub timeout: Duration }
// ActionRequest::new(action).on_origin("https://shop.test")
pub struct ActionOutcome { pub view: ViewId, pub url: String, pub origin: String, pub isolation: Isolation,
    pub matches: usize, pub element: Option<Element>, pub scroll: ScrollState }
pub struct Element { pub tag, pub role, pub name, pub text /* ≤ 4000 chars */, pub value: Option<String>, pub secret: bool /* password: no value */,
    pub checked: Option<bool>, pub disabled, pub editable, pub visible, pub href: Option<String>, pub rect: Option<Bounds> }
pub struct ScrollState { pub x, pub y, pub max_x, pub max_y, pub moved: bool, pub inner: bool /* a box in the page scrolled */ }
pub enum ActionError { Unsupported, Gone, Timeout, NotFound, NotInteractable(String), BlockedByIsolation(Isolation),
    WrongOrigin { expected: String, actual: String }, Navigated { from: String, to: String }, BadRequest(String), Script(String) }
pub type ActionResult = Result<ActionOutcome, ActionError>;
pub struct PageActions;           // beside PageReads: run(view, request) -> Pending<ActionResult>, start, tick, forget_view

// morph_browser (what the companion hosts use) ---------------------------------

pub enum HostSession { Ephemeral /* default */, Profile }
pub struct HostPageRequest { pub url: String, pub session: HostSession, pub load_timeout: Duration }
// HostPageRequest::new(url) (throwaway, LOAD_TIMEOUT), HostPageRequest::signed_in(url), .timeout(d)

impl Browser {
    pub fn open_host_page_with(&mut self, request: HostPageRequest) -> morph_webview::Result<HostPageId>; // Ephemeral never falls back to Profile
    pub fn open_host_page(&mut self, url: &str, load_timeout: Duration) -> morph_webview::Result<HostPageId>; // = signed_in(url)
    pub fn host_page_session(&self, id: HostPageId) -> Option<HostSession>;
    pub fn tab_action(&mut self, tab: TabId, request: ActionRequest) -> Pending<ActionResult>;
    pub fn active_tab_action(&mut self, request: ActionRequest) -> Pending<ActionResult>;
    pub fn host_page_action(&mut self, id: HostPageId, request: ActionRequest) -> Pending<ActionResult>;
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
let id = browser.open_host_page_with(HostPageRequest::new("https://docs.rs/serde"))?;   // signed out
let loaded = browser.host_page_loaded(id);             // awaited while the shell keeps calling browser.poll()
if loaded.await == HostPageState::Loaded {
    let page = browser.read_host_page(id, ReadRequest::new(Want { readable: true, links: true, ..Want::default() })).await?;
    // page.readable, page.links, page.origin (engine's), page.truncated
}
browser.close_host_page(id);

// Tier 2, after the user approved "click Send on shop.test":
let out = browser
    .active_tab_action(ActionRequest::new(Action::Click(Locator::role("button", Some("Send")))).on_origin("https://shop.test"))
    .await?;   // out.element (read back after the click), out.matches, out.isolation
let filled = browser.active_tab_action(ActionRequest::new(Action::Fill { target: Locator::name("Email"), value: email })).await?;
browser.active_tab_action(ActionRequest::new(Action::ScrollBy { dx: 0.0, dy: 600.0 })).await?;   // page.scroll on any tab
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
  `window.open` refused, laid out at 1280×800 (on Wayland the off-screen window is shown
  off screen for that; before, it had WebKit's minimum viewport).
- **Throwaway sessions.** `Storage::Ephemeral` (what `HostPageRequest::new` asks for): a
  store of the view's own that starts empty (signed out everywhere), is never written to
  disk, never shares cookies or site storage with the profile, and goes with the view.
  WebKitGTK: an ephemeral `WebKitWebContext` (ephemeral `WebsiteDataManager`) per view
  (wry's incognito), whose downloads are cancelled. WKWebView: a new
  `WKWebsiteDataStore.nonPersistent()` per view. WebView2: an InPrivate controller
  (`IsInPrivateModeEnabled`) of the `MorphEphemeral` profile in the window profile's
  environment: InPrivate pages of one profile share a session while any is open, so two
  ephemeral pages open at once can see each other's cookies (never the profile's), and it
  is all gone when the last one closes. A backend without one refuses
  (`Error::Unsupported`); `open_host_page_with` never falls back to the profile.
- **Actions.** `PageActions::run` checks the request (a non-empty locator, finite scroll,
  a value ≤ 1 MiB), refuses when the view has no script world of Morph's
  (`BlockedByIsolation`, unless `allow_page_world`) or when the engine's origin isn't
  `expect_origin` (`WrongOrigin`), then runs one script with `evaluate_isolated` and the
  engine's origin: the page side refuses when `location.origin` differs (`Navigated`) or
  outside the top frame. Locators match in document order, on-screen elements first; role is
  explicit or implied by the tag, name is the accessible name (labelledby, aria-label,
  `<label>`, button/link text, alt, placeholder, title), exact matches before partial ones.
  Click and fill first scroll the element to the middle and refuse what is hidden, disabled,
  inert, read-only, not a text field, or covered at its centre (`NotInteractable("covered by
  div#banner")`). Click sends pointer/mouse down and up, focus and `click()`; fill sets the
  value with the element prototype's own setter (from Morph's world, so a page's poisoned
  setter never runs, and React-style value trackers see a change) with `beforeinput`,
  `input` and `change`, picks a `<select>` option by value or text, and types into
  contenteditable with `insertText`. Events are synthetic (`isTrusted` false). Scrolls jump
  (whatever the page's `scroll-behavior`), the page or, when it can't move that way, the
  scrollable box under the middle of the view. The answer (≤ 64 KiB) reads the element back
  after the action; password values are never read back (`secret`). A timed-out action may
  still have run. There is no bridge message for any of this; `host.*` types stay refused on
  the page channel.

Generic beyond Morph: `page_read` and `page_action` (their scripts and types) would fit a
weirdnet crate (a "webview automation" crate over any engine that can run a script in an
isolated world); they stay in `morph-webview` for now, next to the backends that provide
the worlds.

## Per-platform anti-forgery status

| | Host channel | Reads run in | Gaps |
|---|---|---|---|
| Linux, X11 (`wry_linux`) and Wayland (`offscreen`), WebKitGTK | Script world `morph-host` (`UserScript::for_world`, top frame only) + handler registered in that world only; token, frame and origin checked | `morph-host` world (`run_javascript_in_world`) | Page-channel messages can't tell main frame from subframe (`FrameKind::Unknown`): WebKitGTK 4.1's `script-message-received` gives no frame. Pages share the DOM with the world, so a page can still change what its DOM says (its own content). Popups get the page channel only. |
| macOS, WKWebView (`wry_native`) | Page world, inside a closure: the token and wry's `webkit.messageHandlers.ipc` are captured at document start before page scripts run; never on `window`. Token, frame (WKWebView's `frameInfo` URL) and origin checked | `WKContentWorld` `morph-host` (`evaluateJavaScript:inFrame:inContentWorld:`, main frame; the world is kept for the view's life) | Reads and actions are safe from page scripts. The host *channel* still runs in the page's world (wry 0.57 adds init scripts there): page scripts can replace the builtins a host script uses after document start and see host *replies*; they cannot forge a host message without the token. Moving it needs `WKUserScript`/`addScriptMessageHandler:contentWorld:` through objc2 (not done). |
| Windows, WebView2 (`wry_native`) | Page world, inside a closure: `chrome.webview.postMessage` captured at document start; host script returns early outside the top frame (wry injects init scripts into every frame there). Token and origin checked (`Source` is the top document) | CDP isolated world `morph-host` (`Page.createIsolatedWorld` on the top frame through `CallDevToolsProtocolMethod`, then `Runtime.evaluate` with its `contextId`; made again for each new document) | Reads and actions are safe from page scripts. Host channel as macOS (WebView2 has no isolated world for init scripts). If DevTools protocol calls are disabled (enterprise policy), reads and actions fail instead of running in the page's world. |
| Android, iOS (Flutter) | Not wired: `morph-bridge` still answers `page_bridge::answer` without context | — | No host pages, reads or actions there yet. When wired: Android WebView has no isolated world (`evaluateJavascript` is the page's), so actions would report `Isolation::PageWorld` and need `allow_page_world`; throwaway sessions would need a separate profile (`androidx.webkit` `ProfileStore`, WebView 123+) or be `Unsupported`. iOS can use `WKContentWorld` and `nonPersistent()` like macOS. |

Throwaway sessions per platform: WebKitGTK yes (one store per page, tested in Xvfb);
WKWebView yes (one store per page); WebView2 yes (InPrivate, shared among ephemeral
pages open at once); Android/iOS not wired.

Tests: `cargo test -p morph-webview` (contexts, refusals, envelopes, chunked reads,
timeouts, origin checks, action requests, answers, errors, isolation and origin refusals
on the fake) and `cargo test -p morph-browser host_pages` (sessions, refusal without
throwaway storage, tab and host page actions); the real-engine tests are
`crates/morph-webview/tests/bridge_webkit.rs` (forgery, reads) and
`crates/morph-webview/tests/session_actions_webkit.rs` (an ephemeral page sees neither the
profile's cookie nor its localStorage, nor another ephemeral page's; fill by role and
label, password not read back, `<select>` by text, contenteditable, click seen by the
page's listener, covered/disabled/missing/bad-selector refusals, scroll by and to, on a
page that poisons `HTMLInputElement.prototype.value`, `dispatchEvent`, `querySelectorAll`,
`JSON.stringify` and `getBoundingClientRect`), which need a display:

```sh
cargo test -p morph-webview --test bridge_webkit --no-run   # prints the test binary
env -i HOME=$HOME PATH=/usr/bin:/bin XDG_RUNTIME_DIR=$(mktemp -d) MORPH_BRIDGE_WEBKIT=1 \
  xvfb-run --server-num=137 target/debug/deps/bridge_webkit-<hash>
# the same for session_actions_webkit-<hash>
```

On macOS and Windows the same checks run in CI on the release build:
`scripts/desktop-smoke.py` launches Morph with `MORPH_TEST_BRIDGE=1`
(`crates/morph-desktop/src/bridge_selftest.rs`) on a page that poisons its builtins, and
checks that the read and the fill report `ScriptWorld` and the real values, the page sees
the fill and the click, and a throwaway host page gets no profile cookie while a signed-in
one does.

It loads a page that tries `MorphBridge.post({type: "host.read"})`, forges a host
envelope on the page handler, looks for `MorphHost` and the host handler from the page
and from a subframe, then poisons `JSON.stringify`, `Array.prototype.push` and
`document.title`; it checks every attempt is refused or blind, that the host script's
`host.hello` arrives as `Trust::Host` from the main frame, and that a read still gets
the real title, text, readable extract, links and HTML, chunked for a 3 MB page and cut
to 1 MiB when asked. Without `MORPH_BRIDGE_WEBKIT=1` it skips, so plain `cargo test` (CI
included) never opens a display.
