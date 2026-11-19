//! The page bridge's trust model (docs/bridge-api.md).
//!
//! Two channels reach Rust from a page:
//!
//! - The **page channel**, `window.MorphBridge` ([`crate::PageBridge::script`]),
//!   lives in the page's own JavaScript world. Any script on the page, and on
//!   WebKitGTK any of its frames, can post on it, so its messages are
//!   [`Trust::Page`]: input, never authority. Message types starting with
//!   [`HOST_PREFIX`] are refused there before a handler sees them.
//! - The **host channel** carries Morph's own scripts
//!   ([`crate::PageBridge::host_script`]). On WebKitGTK they run in a script
//!   world of their own ([`HOST_WORLD`]) in the main frame only, which page
//!   scripts cannot reach; elsewhere they run in the page's world inside a
//!   closure. Either way every message carries the view's secret
//!   ([`ViewBridge::token`]) and the origin it saw, and [`check_host`]
//!   refuses a wrong token, a subframe, or an origin that is not the one the
//!   engine reports for the view. Accepted messages are [`Trust::Host`].
//!
//! Every handler call gets a [`BridgeContext`] built from what the engine
//! says (the view, its top-level address, whether it is hidden), never from
//! what the page claims.

use std::cell::Cell;
use std::rc::Rc;

use crate::ViewId;

/// Message types only Morph's host scripts may send (`host.read`, …).
pub const HOST_PREFIX: &str = "host.";

/// The WebKitGTK script world Morph's host scripts and page reads run in.
pub const HOST_WORLD: &str = "morph-host";

/// The script message handler of the page channel.
pub const PAGE_HANDLER: &str = "morphBridge";

/// The script message handler of the host channel (registered in [`HOST_WORLD`] only).
pub const HOST_HANDLER: &str = "morphHost";

/// Which frame a message came from, as far as the engine can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Main,
    Sub,
    /// The engine does not say (page-channel messages on WebKitGTK 4.1:
    /// any frame of the page can post on it).
    Unknown,
}

/// Who could have sent a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// The page channel: any script of the page.
    Page,
    /// The host channel: Morph's host scripts, checked by token, frame and origin.
    Host,
}

/// What a handler knows about the message it answers, from the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeContext {
    /// The view (tab, hidden host page) the message came from. A popup's
    /// messages carry its opener's view and `popup: true`.
    pub view: ViewId,
    pub popup: bool,
    /// The view's top-level address as the engine reports it now.
    pub url: String,
    /// [`origin_of`] `url`.
    pub origin: String,
    pub frame: FrameKind,
    pub trust: Trust,
    /// The view is not on screen (a background tab, or a hidden host page).
    pub hidden: bool,
    /// A view no user ever sees: made for a host to load and read
    /// ([`crate::Spec::background`]).
    pub background: bool,
}

/// Answers one message with its context; `None` sends no reply.
pub type ContextHandler = Rc<dyn Fn(&BridgeContext, &str) -> Option<String>>;

/// What a backend keeps per view for its bridge handlers.
#[derive(Clone, Debug)]
pub struct ViewBridge {
    pub id: ViewId,
    /// The view's secret: 128 random bits, hex. Only its host script has it.
    pub token: Rc<str>,
    /// The name the host script answers replies on (mac and Windows: a
    /// read-only property of the page's `window`).
    pub deliver_name: Rc<str>,
    pub hidden: Rc<Cell<bool>>,
    pub background: bool,
    pub popup: bool,
}

impl ViewBridge {
    pub fn new(id: ViewId, visible: bool, background: bool) -> Self {
        Self {
            id,
            token: new_token().into(),
            deliver_name: format!("__morphHost{}", &new_token()[..12]).into(),
            hidden: Rc::new(Cell::new(!visible || background)),
            background,
            popup: false,
        }
    }

    /// The same view's bridge for a popup it opened (same token: the popup
    /// gets no host script, so it never uses it).
    pub fn for_popup(&self) -> Self {
        Self { popup: true, ..self.clone() }
    }

    pub fn set_visible(&self, visible: bool) {
        self.hidden.set(!visible || self.background);
    }

    /// The context of a message from this view whose top-level address the engine reports as `url`.
    pub fn context(&self, url: &str, frame: FrameKind, trust: Trust) -> BridgeContext {
        BridgeContext {
            view: self.id,
            popup: self.popup,
            url: url.to_string(),
            origin: origin_of(url),
            frame,
            trust,
            hidden: self.hidden.get(),
            background: self.background,
        }
    }
}

/// 128 random bits as 32 hex digits, from the operating system.
pub fn new_token() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // No OS randomness: a per-process random hasher, twice (weaker,
        // still unknown to pages).
        use std::hash::{BuildHasher, Hasher};
        for half in bytes.chunks_mut(8) {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
            half.copy_from_slice(&h.finish().to_le_bytes());
        }
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The origin of `url` as browsers serialize it: `scheme://host[:port]`
/// with the scheme and host lower-cased and default ports dropped, or
/// `"null"` for addresses without one (`about:`, `data:`, `file:`, …).
pub fn origin_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "null".into();
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "file" || scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return "null".into();
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map(|(_, h)| h).unwrap_or(authority);
    if host_port.is_empty() {
        return "null".into();
    }
    let (host, port) = if host_port.starts_with('[') {
        match host_port.find(']') {
            Some(end) => (&host_port[..=end], host_port[end + 1..].strip_prefix(':')),
            None => return "null".into(),
        }
    } else {
        match host_port.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (host_port, None),
        }
    };
    let default = match scheme.as_str() {
        "http" | "ws" => Some("80"),
        "https" | "wss" => Some("443"),
        _ => None,
    };
    let host = host.to_ascii_lowercase();
    match port.filter(|p| !p.is_empty() && Some(*p) != default) {
        Some(p) => format!("{scheme}://{host}:{p}"),
        None => format!("{scheme}://{host}"),
    }
}

/// Whether the message `body` asks for a host-only type.
pub fn is_host_type(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(|t| t.starts_with(HOST_PREFIX)))
        .unwrap_or(false)
}

/// An error reply to `body`'s id (when it has one).
pub fn error_reply(body: &str, error: &str) -> Option<String> {
    let id = serde_json::from_str::<serde_json::Value>(body).ok()?.get("id")?.as_str()?.to_string();
    if id.is_empty() {
        return None;
    }
    Some(serde_json::json!({"id": id, "type": "error", "error": error}).to_string())
}

/// Why a host-channel message was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Not a host envelope.
    Malformed,
    /// The token is not this view's.
    Token,
    /// It came from a subframe.
    Subframe,
    /// The origin it saw (or its frame's) is not the view's.
    Origin { claimed: String, engine: String },
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Malformed => write!(f, "not a host message"),
            Refused::Token => write!(f, "wrong token"),
            Refused::Subframe => write!(f, "host messages come from the main frame only"),
            Refused::Origin { claimed, engine } => write!(f, "origin {claimed} is not the page's ({engine})"),
        }
    }
}

/// Check a host-channel envelope `{"t": token, "f": "main"|"sub", "o": origin, "m": message}`
/// from `view`, whose top-level address the engine reports as `engine_url`.
/// `frame_url` is the address of the sending frame when the engine says it
/// (WKWebView). The context and the inner message when it passes.
pub fn check_host(view: &ViewBridge, engine_url: &str, frame_url: Option<&str>, body: &str) -> Result<(BridgeContext, String), Refused> {
    let envelope: serde_json::Value = serde_json::from_str(body).map_err(|_| Refused::Malformed)?;
    let field = |k: &str| envelope.get(k).and_then(|v| v.as_str());
    let (Some(token), Some(frame), Some(claimed), Some(message)) = (field("t"), field("f"), field("o"), field("m")) else {
        return Err(Refused::Malformed);
    };
    if !same_secret(token, &view.token) {
        return Err(Refused::Token);
    }
    if frame != "main" {
        return Err(Refused::Subframe);
    }
    let engine = origin_of(engine_url);
    if let Some(frame_url) = frame_url
        && origin_of(frame_url) != engine
    {
        return Err(Refused::Origin { claimed: origin_of(frame_url), engine });
    }
    if claimed != engine {
        return Err(Refused::Origin { claimed: claimed.to_string(), engine });
    }
    Ok((view.context(engine_url, FrameKind::Main, Trust::Host), message.to_string()))
}

/// Compare secrets without stopping at the first difference.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// How the host script reaches Rust and how Rust answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTransport {
    /// WebKitGTK: [`HOST_HANDLER`] in [`HOST_WORLD`].
    WebKitWorld,
    /// WKWebView: wry's `ipc` handler, captured at document start.
    WebKitIpc,
    /// WebView2: `chrome.webview`, captured at document start.
    WebView2,
}

/// The host script for `view`: Morph's `scripts` (each run as
/// `function (MorphHost) { … }`) with `MorphHost.post(message)` → Promise,
/// main frame only. Everything it uses is captured before page scripts run.
pub fn host_script(view: &ViewBridge, transport: HostTransport, scripts: &[Rc<str>]) -> String {
    let send = match transport {
        HostTransport::WebKitWorld => {
            r#"const h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.morphHost; const pm = h && h.postMessage; const prefix = "";"#
        }
        HostTransport::WebKitIpc => {
            r#"const h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ipc; const pm = h && h.postMessage; const prefix = "h:";"#
        }
        HostTransport::WebView2 => r#"const h = window.chrome && window.chrome.webview; const pm = h && h.postMessage; const prefix = "h:";"#,
    };
    let body: String = scripts.iter().map(|s| format!("try {{ (function (MorphHost) {{\n{s}\n}})(host); }} catch (_) {{}}\n")).collect();
    format!(
        r#"(() => {{
  "use strict";
  if (window !== window.top) return;
  {send}
  if (typeof pm !== "function") return;
  const TOKEN = {token};
  const apply = Reflect.apply, stringify = JSON.stringify, parse = JSON.parse, P = Promise, define = Object.defineProperty, freeze = Object.freeze;
  const pending = Object.create(null);
  let next = 0;
  const send = (s) => apply(pm, h, [prefix + s]);
  const host = freeze({{
    post: (message) => new P((resolve, reject) => {{
      const id = "h" + (++next);
      const m = {{ id: id, type: message.type, name: message.name, payload: message.payload }};
      pending[id] = {{ resolve: resolve, reject: reject }};
      send(stringify({{ t: TOKEN, f: window === window.top ? "main" : "sub", o: location.origin, m: stringify(m) }}));
    }}),
  }});
  const deliver = (raw) => {{
    const reply = typeof raw === "string" ? parse(raw) : raw;
    const wait = pending[reply.id];
    if (!wait) return;
    delete pending[reply.id];
    if (reply.error) wait.reject(new Error(reply.error)); else wait.resolve(reply);
  }};
  define(window, {name}, {{ value: deliver, writable: false, configurable: false, enumerable: false }});
{body}}})();"#,
        token = crate::native_page::js_string(&view.token),
        name = crate::native_page::js_string(&view.deliver_name),
    )
}

/// The script that hands `reply` to `view`'s host script.
pub fn host_deliver_script(view: &ViewBridge, reply: &str) -> String {
    let name = crate::native_page::js_string(&view.deliver_name);
    format!("typeof window[{name}] === \"function\" && window[{name}]({reply});")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> ViewBridge {
        ViewBridge::new(ViewId(3), true, false)
    }

    fn envelope(token: &str, frame: &str, origin: &str) -> String {
        serde_json::json!({"t": token, "f": frame, "o": origin, "m": r#"{"id":"h1","type":"host.ping"}"#}).to_string()
    }

    #[test]
    fn origins_follow_the_browser_rules() {
        assert_eq!(origin_of("https://Example.COM/a?b#c"), "https://example.com");
        assert_eq!(origin_of("https://example.com:443/"), "https://example.com");
        assert_eq!(origin_of("http://example.com:8080"), "http://example.com:8080");
        assert_eq!(origin_of("http://user:pw@example.com/x"), "http://example.com");
        assert_eq!(origin_of("http://[::1]:3000/x"), "http://[::1]:3000");
        assert_eq!(origin_of("weirdnet-morph://peer/site"), "weirdnet-morph://peer");
        for none in ["about:blank", "data:text/html,hi", "file:///etc/passwd", "", "javascript:alert(1)"] {
            assert_eq!(origin_of(none), "null", "{none}");
        }
    }

    #[test]
    fn tokens_are_random_and_long() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert_ne!(view().token, view().token);
    }

    #[test]
    fn host_messages_need_the_token_the_main_frame_and_the_origin() {
        let v = view();
        let url = "https://example.com/page";
        let (ctx, msg) = check_host(&v, url, None, &envelope(&v.token, "main", "https://example.com")).unwrap();
        assert_eq!(ctx.trust, Trust::Host);
        assert_eq!(ctx.frame, FrameKind::Main);
        assert_eq!(ctx.origin, "https://example.com");
        assert_eq!(ctx.view, ViewId(3));
        assert!(msg.contains("host.ping"));
        assert_eq!(check_host(&v, url, None, &envelope("0123456789abcdef0123456789abcdef", "main", "https://example.com")), Err(Refused::Token));
        assert_eq!(check_host(&v, url, None, &envelope(&v.token, "sub", "https://example.com")), Err(Refused::Subframe));
        assert!(matches!(check_host(&v, url, None, &envelope(&v.token, "main", "https://evil.test")), Err(Refused::Origin { .. })));
        // WKWebView says which frame sent it: another origin's frame is refused.
        assert!(matches!(
            check_host(&v, url, Some("https://ads.test/frame"), &envelope(&v.token, "main", "https://example.com")),
            Err(Refused::Origin { .. })
        ));
        assert_eq!(check_host(&v, url, None, r#"{"id":"1","type":"host.ping"}"#), Err(Refused::Malformed));
        assert_eq!(check_host(&v, url, None, "not json"), Err(Refused::Malformed));
    }

    #[test]
    fn host_types_are_recognized_and_refusals_answer_the_id() {
        assert!(is_host_type(r#"{"id":"1","type":"host.read"}"#));
        assert!(!is_host_type(r#"{"id":"1","type":"ping"}"#));
        assert!(!is_host_type("nope"));
        let reply = error_reply(r#"{"id":"7","type":"host.read"}"#, "refused").unwrap();
        assert!(reply.contains(r#""id":"7""#) && reply.contains("refused"));
        assert!(error_reply(r#"{"type":"host.read"}"#, "x").is_none());
    }

    #[test]
    fn contexts_say_when_a_view_is_hidden() {
        let v = view();
        assert!(!v.context("about:blank", FrameKind::Unknown, Trust::Page).hidden);
        v.set_visible(false);
        assert!(v.context("about:blank", FrameKind::Unknown, Trust::Page).hidden);
        let bg = ViewBridge::new(ViewId(4), true, true);
        bg.set_visible(true);
        let ctx = bg.context("https://a.test/", FrameKind::Main, Trust::Host);
        assert!(ctx.hidden && ctx.background);
        assert!(v.for_popup().context("about:blank", FrameKind::Unknown, Trust::Page).popup);
    }

    #[test]
    fn the_host_script_keeps_its_secret_in_a_closure() {
        let v = view();
        for transport in [HostTransport::WebKitWorld, HostTransport::WebKitIpc, HostTransport::WebView2] {
            let s = host_script(&v, transport, &["MorphHost.post({type: 'host.hello'});".into()]);
            assert!(s.contains(&*v.token));
            assert!(s.contains("if (window !== window.top) return;"), "main frame only");
            assert!(!s.contains("window.MorphHost"), "never on the page's window");
            assert!(s.contains("configurable: false"));
        }
        assert!(host_deliver_script(&v, r#"{"id":"h1"}"#).contains(&*v.deliver_name));
        assert!(!host_deliver_script(&v, "{}").contains(&*v.token), "replies never carry the token");
    }
}
