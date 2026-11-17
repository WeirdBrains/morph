//! Calling a Weirdnet app from the address bar: `weirdnet-<app>://<locator>/<path>`.
//!
//! Such a link names a resource another peer serves (a peer id, a user id or
//! a name bound to a user, then a path), so opening it is a call over
//! Weirdnet: resolve the locator to the peers serving the app, send one of
//! them a request (method, path, headers, body) and show the streamed
//! response. It is never handed to a local OS handler and never becomes a
//! search.
//!
//! The shells hold an [`AppCaller`] (the one backed by weirdnet-net lives in
//! `morph-services`) and run [`fetch`] off the UI thread: it makes the call,
//! reads the body up to [`BODY_MAX`] and turns it into an [`AppPage`], a
//! `data:` address the web view loads ([`render`]: HTML as it is, images,
//! JSON and text on a page of Morph's). A call that fails gives an
//! [`AppCallError`], and the shells show [`error_text`] for it instead.

use std::fmt;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use weirdnet_schemes::link::{Action, Link};

/// Longest locator or path kept in a log-safe description.
const DESCRIBE_MAX: usize = 120;
/// The most of an app's answer Morph reads and shows (8 MiB).
pub const BODY_MAX: u64 = 8 * 1024 * 1024;
/// Longest app name in a link. weirdnet-net serves names up to 64
/// characters, but a link's scheme (`weirdnet-<name>`) is at most 64 in all
/// (weirdnet-schemes), which leaves 55 for the name.
pub const APP_NAME_MAX: usize = 64 - "weirdnet-".len();
/// The `data:` parameter that marks an app page, so a shell can show the
/// app address for it ([`page_token`]).
const TOKEN_PARAM: &str = "morph-app=";

/// One call to a Weirdnet app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRequest {
    /// The app's scheme, lowercase (`weirdnet-muninn`).
    pub app: String,
    /// Who serves it, as typed: a peer id (case matters), a user id or a
    /// name (`home.example.com`).
    pub locator: String,
    /// `GET` for a typed address.
    pub method: String,
    /// The path with its query, from `/` (`/notes/a.md?x=1`); no fragment.
    pub path: String,
    /// Request headers, in order.
    pub headers: Vec<(String, String)>,
    /// Request body (empty for `GET`).
    pub body: Vec<u8>,
}

impl AppRequest {
    /// The app's name on Weirdnet: the scheme without `weirdnet-` (`muninn`).
    pub fn app_name(&self) -> &str {
        self.app.strip_prefix("weirdnet-").unwrap_or(&self.app)
    }
}

/// An app's answer: status and headers first, the body as it streams in.
pub struct AppResponse {
    /// HTTP-style status (200, 404, …).
    pub status: u16,
    /// Response headers, in order (`content-type` among them).
    pub headers: Vec<(String, String)>,
    /// The body, read as it arrives. A read error may carry an
    /// [`AppCallError`] (see [`io_error`]).
    pub body: Box<dyn Read + Send>,
}

impl AppResponse {
    /// The first header called `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
}

impl fmt::Debug for AppResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppResponse").field("status", &self.status).field("headers", &self.headers).finish_non_exhaustive()
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Why an app call did not answer with a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum AppCallError {
    /// The address is not a valid app link.
    #[error("not a valid Weirdnet app link: {0}")]
    Invalid(String),
    /// The scheme's app name is not one Weirdnet allows (`a-z`, `0-9`, `-`,
    /// 1 to 64 characters).
    #[error("{0:?} is not a valid Weirdnet app name")]
    InvalidApp(String),
    /// `weirdnet://` is internal to Weirdnet and never opened.
    #[error("weirdnet:// addresses are internal to Weirdnet")]
    Internal,
    /// This build cannot call Weirdnet apps.
    #[error("calling Weirdnet apps is not supported")]
    NotSupported,
    /// No peer for the locator serves the app (or the link is an invite,
    /// which only the app itself takes).
    #[error("nothing serves {0}:// there")]
    NoHandler(String),
    /// The locator did not resolve to any peer.
    #[error("could not find {0} on Weirdnet")]
    NotFound(String),
    /// The peer was found but could not be reached (directly or by relay).
    #[error("could not reach it: {0}")]
    Unreachable(String),
    /// The peer did not answer in time.
    #[error("the app did not answer in time")]
    TimedOut,
    /// The app refused the call.
    #[error("the app refused the call: {0}")]
    Refused(String),
    /// The answer is larger than Morph shows ([`BODY_MAX`]), or a head
    /// larger than Weirdnet carries.
    #[error("the answer is too large to show")]
    TooLarge,
    /// The peer broke the app-call wire format.
    #[error("the peer broke the app protocol: {0}")]
    Protocol(String),
    /// The app answered with something Morph cannot show (its content type).
    #[error("Morph can't show {0}")]
    Unsupported(String),
    /// The app answered with an error status and nothing to show.
    #[error("the app answered {0}")]
    Status(u16),
    /// The call was stopped (Stop, or the tab went elsewhere).
    #[error("stopped")]
    Stopped,
}

impl AppCallError {
    /// A short stable code (`not_found`), for logs and checks.
    pub fn code(&self) -> &'static str {
        match self {
            AppCallError::Invalid(_) => "invalid",
            AppCallError::InvalidApp(_) => "invalid_app",
            AppCallError::Internal => "internal",
            AppCallError::NotSupported => "not_supported",
            AppCallError::NoHandler(_) => "no_handler",
            AppCallError::NotFound(_) => "not_found",
            AppCallError::Unreachable(_) => "unreachable",
            AppCallError::TimedOut => "timed_out",
            AppCallError::Refused(_) => "refused",
            AppCallError::TooLarge => "too_large",
            AppCallError::Protocol(_) => "protocol",
            AppCallError::Unsupported(_) => "unsupported",
            AppCallError::Status(_) => "status",
            AppCallError::Stopped => "stopped",
        }
    }

    /// The error as one line of text, for a shell that keeps a failed load's
    /// message as a string ([`Self::decode`] reads it back).
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// An error from [`Self::encode`]; `None` for any other text.
    pub fn decode(text: &str) -> Option<AppCallError> {
        serde_json::from_str(text.trim()).ok()
    }
}

/// An [`std::io::Error`] carrying `error`, for an [`AppResponse::body`]
/// reader to hand an app-call failure to [`fetch`].
pub fn io_error(error: AppCallError) -> std::io::Error {
    std::io::Error::other(error)
}

fn from_io(e: std::io::Error) -> AppCallError {
    if e.get_ref().is_some_and(|inner| inner.is::<AppCallError>()) {
        return *e.into_inner().and_then(|inner| inner.downcast::<AppCallError>().ok()).expect("checked above");
    }
    match e.kind() {
        std::io::ErrorKind::TimedOut => AppCallError::TimedOut,
        std::io::ErrorKind::UnexpectedEof => AppCallError::Protocol("the answer ended early".into()),
        _ => AppCallError::Unreachable(e.to_string()),
    }
}

/// Something that can call Weirdnet apps. The shells hold one and send every
/// `weirdnet-<app>://` address through it, off the UI thread.
pub trait AppCaller: Send + Sync {
    /// Sends `request` and returns the response once its status and headers
    /// are in (the body streams after). May block for as long as finding
    /// and reaching the peer takes.
    fn call(&self, request: AppRequest) -> Result<AppResponse, AppCallError>;
}

/// A caller for builds and tests without a network: every call is
/// [`AppCallError::NotSupported`].
#[derive(Debug, Clone, Copy, Default)]
pub struct NotYet;

impl AppCaller for NotYet {
    fn call(&self, _request: AppRequest) -> Result<AppResponse, AppCallError> {
        Err(AppCallError::NotSupported)
    }
}

/// Whether `name` is an app name Weirdnet allows: `a-z`, `0-9` and `-`,
/// 1 to [`APP_NAME_MAX`] characters.
pub fn valid_app_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= APP_NAME_MAX && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The `GET` request a typed `weirdnet-<app>://<locator>/<path>` address
/// makes.
pub fn request(url: &str) -> Result<AppRequest, AppCallError> {
    let url = url.trim();
    let scheme = crate::address::scheme_of(url).unwrap_or("");
    if scheme.eq_ignore_ascii_case(weirdnet_schemes::names::INTERNAL_SCHEME) {
        return Err(AppCallError::Internal);
    }
    let name = scheme.get("weirdnet-".len()..).unwrap_or("").to_ascii_lowercase();
    if !valid_app_name(&name) {
        return Err(AppCallError::InvalidApp(name.chars().filter(|c| !c.is_control()).take(DESCRIBE_MAX).collect()));
    }
    let link = Link::parse(url).map_err(|e| AppCallError::Invalid(e.to_string()))?;
    let Action::Resource { .. } = link.action else {
        // `join/<code>` and `answer/<code>` are invites for that app, which
        // only the app itself (or Morph's device linking) takes.
        return Err(AppCallError::NoHandler(link.scheme));
    };
    let rest = url.split_once("://").map_or("", |(_, r)| r);
    let rest = rest.split('#').next().unwrap_or("");
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest[i..].starts_with('?') => (&rest[..i], format!("/{}", &rest[i..])),
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    // Peer ids are case-sensitive (base58): the locator keeps the case it
    // was typed in; the caller folds names and user ids.
    Ok(AppRequest { app: link.scheme, locator: authority.to_string(), method: "GET".into(), path, headers: vec![], body: vec![] })
}

/// Opens `url` through `caller` (blocking; the status and headers only).
pub fn open(url: &str, caller: &dyn AppCaller) -> Result<AppResponse, AppCallError> {
    caller.call(request(url)?)
}

/// A log- and page-safe form of `url`: scheme, locator and path, no query
/// or fragment (they may carry tokens), control characters dropped, capped.
pub fn describe(url: &str) -> String {
    let url = url.trim();
    let base = url.split(['?', '#']).next().unwrap_or("");
    let mut out: String = base.chars().filter(|c| !c.is_control()).take(DESCRIBE_MAX).collect();
    if base.chars().count() > DESCRIBE_MAX {
        out.push('…');
    }
    out
}

/// A locator for a page: control characters dropped, capped.
fn shown_locator(locator: &str) -> String {
    let mut out: String = locator.chars().filter(|c| !c.is_control()).take(DESCRIBE_MAX).collect();
    if locator.chars().count() > DESCRIBE_MAX {
        out.push('…');
    }
    out
}

/// What an HTTP-style status says, for an error page.
fn status_text(status: u16) -> String {
    let reason = match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    };
    if reason.is_empty() { status.to_string() } else { format!("{status} {reason}") }
}

fn clean(text: &str) -> String {
    let t: String = text.chars().filter(|c| !c.is_control()).take(300).collect();
    t.trim().trim_end_matches('.').to_string()
}

/// Title and body for the page shown instead of `url` when its call fails.
/// Every one says it was not searched.
pub fn error_text(url: &str, error: &AppCallError) -> (String, String) {
    const NOT_SEARCHED: &str = "It was not sent to a search engine.";
    let shown = describe(url);
    let who = request(url).map(|r| shown_locator(&r.locator)).unwrap_or_default();
    let app = request(url).map(|r| r.app).unwrap_or_default();
    match error {
        AppCallError::NotSupported => (
            "App calls aren't available".into(),
            format!(
                "{shown} is a Weirdnet app link. Morph opens these by calling the app over Weirdnet, and this build can't make that call. {NOT_SEARCHED}"
            ),
        ),
        AppCallError::NoHandler(scheme) if !who.is_empty() && scheme == &app => (
            format!("{who} doesn't serve {scheme}://"),
            format!("Morph reached {who} on Weirdnet, but nothing there answers {scheme}:// calls, so {shown} can't be opened. {NOT_SEARCHED}"),
        ),
        AppCallError::NoHandler(scheme) => (
            format!("No app for {scheme}://"),
            format!("Nothing on Weirdnet answers {scheme}:// links like {shown} (invites go to the app itself). {NOT_SEARCHED}"),
        ),
        AppCallError::Internal => {
            ("weirdnet:// can't be opened".into(), "weirdnet:// addresses are internal to Weirdnet. App links start with weirdnet-<app>://.".into())
        }
        AppCallError::Invalid(why) => (
            "Not a valid Weirdnet link".into(),
            format!("{shown} is not a valid weirdnet-<app>://<peer, user or name>/<path> link ({}). {NOT_SEARCHED}", clean(why)),
        ),
        AppCallError::InvalidApp(name) => (
            "Not a valid Weirdnet app name".into(),
            format!(
                "\"{}\" can't be a Weirdnet app: app names use only a-z, 0-9 and -, up to {APP_NAME_MAX} characters (weirdnet-notes://…). {NOT_SEARCHED}",
                shown_locator(name)
            ),
        ),
        AppCallError::NotFound(locator) => (
            format!("Couldn't find {}", shown_locator(locator)),
            format!(
                "No peer, user or name {} is known on Weirdnet (Morph asked its relays' names servers and the DHT, and looked for a _weirdnet TXT record for names), so {shown} can't be opened. {NOT_SEARCHED}",
                shown_locator(locator)
            ),
        ),
        AppCallError::Unreachable(why) => (
            format!("Couldn't reach {}", if who.is_empty() { "the app".to_string() } else { who.clone() }),
            format!("Morph found where {shown} is served but couldn't connect to it, directly or through a relay ({}). {NOT_SEARCHED}", clean(why)),
        ),
        AppCallError::TimedOut => (
            "The app didn't answer in time".into(),
            format!(
                "{} stopped answering before {shown} arrived. Try again in a moment. {NOT_SEARCHED}",
                if who.is_empty() { "The peer" } else { &who }
            ),
        ),
        AppCallError::Refused(why) => {
            ("The app refused the call".into(), format!("The app serving {shown} said no ({}). {NOT_SEARCHED}", clean(why)))
        }
        AppCallError::TooLarge => (
            "The answer is too large".into(),
            format!(
                "The app's answer for {shown} is larger than Morph can show (at most {} MiB, less on some systems). {NOT_SEARCHED}",
                BODY_MAX / (1024 * 1024)
            ),
        ),
        AppCallError::Protocol(why) => {
            ("The app's answer was garbled".into(), format!("The peer serving {shown} broke the app-call format ({}). {NOT_SEARCHED}", clean(why)))
        }
        AppCallError::Unsupported(kind) => (
            "Morph can't show this answer".into(),
            format!("The app answered {shown} with {}, which Morph doesn't show yet (it shows HTML, text, JSON and images).", clean(kind)),
        ),
        AppCallError::Status(status) => (
            format!("The app answered {}", status_text(*status)),
            format!("The app serving {shown} answered {} with nothing to show.", status_text(*status)),
        ),
        AppCallError::Stopped => ("Stopped".into(), format!("The call for {shown} was stopped.")),
    }
}

/// An app's answer, ready for a web view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPage {
    /// The page's title until the page names itself.
    pub title: String,
    /// The `data:` address to load, marked with the page's token
    /// ([`page_token`]).
    pub data_url: String,
    /// The app's status.
    pub status: u16,
    /// What it was shown as: `html`, `image`, `json` or `text`.
    pub kind: &'static str,
    /// The body's size in bytes.
    pub bytes: u64,
}

/// A fresh token marking one app page's `data:` address.
pub fn new_token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let pid = u64::from(std::process::id());
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    format!("{:x}", t ^ (pid << 40) ^ n.rotate_left(17)) + &format!("{n:x}")
}

/// The token of an app page's `data:` address ([`AppPage::data_url`]);
/// `None` for any other address.
pub fn page_token(url: &str) -> Option<&str> {
    let head = url.strip_prefix("data:")?.split(',').next()?;
    head.split(';').find_map(|p| p.trim().strip_prefix(TOKEN_PARAM)).filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Calls the app `url` names through `caller`, reads its answer (at most
/// [`BODY_MAX`]) and renders it ([`render`]) with a fresh token. Blocks:
/// run it off the UI thread. `progress` gets the bytes read so far after
/// every chunk; `stop` set to true stops the call ([`AppCallError::Stopped`]).
pub fn fetch(url: &str, caller: &dyn AppCaller, stop: &AtomicBool, progress: &mut dyn FnMut(u64)) -> Result<AppPage, AppCallError> {
    let request = request(url)?;
    let response = caller.call(request)?;
    if stop.load(Ordering::Relaxed) {
        return Err(AppCallError::Stopped);
    }
    let AppResponse { status, headers, mut body } = response;
    let bytes = read_body(&mut *body, BODY_MAX, stop, progress)?;
    render(url, status, &headers, &bytes, &new_token())
}

/// Reads `body` to its end, failing past `max` bytes or once `stop` is set.
pub fn read_body(body: &mut dyn Read, max: u64, stop: &AtomicBool, progress: &mut dyn FnMut(u64)) -> Result<Vec<u8>, AppCallError> {
    let mut out = vec![];
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(AppCallError::Stopped);
        }
        let n = match body.read(&mut chunk) {
            Ok(0) => return Ok(out),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(from_io(e)),
        };
        if out.len() as u64 + n as u64 > max {
            return Err(AppCallError::TooLarge);
        }
        out.extend_from_slice(&chunk[..n]);
        progress(out.len() as u64);
    }
}

/// The content type's essence, lowercase (`text/html`), and its charset.
fn content_type(headers: &[(String, String)]) -> (String, Option<String>) {
    let raw = header(headers, "content-type").unwrap_or("");
    let mut parts = raw.split(';');
    let essence = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let charset = parts.find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case("charset").then(|| v.trim().trim_matches('"').to_ascii_lowercase())
    });
    let charset = charset.filter(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':')));
    (essence, charset)
}

/// The images Morph shows, by content type.
const IMAGES: &[&str] =
    &["image/png", "image/jpeg", "image/gif", "image/webp", "image/avif", "image/bmp", "image/svg+xml", "image/x-icon", "image/vnd.microsoft.icon"];

/// What `body` looks like when the app named no content type.
fn sniff(body: &[u8]) -> &'static str {
    let starts = |m: &[u8]| body.starts_with(m);
    if starts(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if starts(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "image/gif"
    } else if body.len() > 12 && starts(b"RIFF") && &body[8..12] == b"WEBP" {
        "image/webp"
    } else if let Ok(text) = std::str::from_utf8(body) {
        let t = text.trim_start().to_ascii_lowercase();
        if t.starts_with("<!doctype html") || t.starts_with("<html") {
            "text/html"
        } else if (t.starts_with('{') || t.starts_with('[')) && serde_json::from_str::<serde_json::Value>(text).is_ok() {
            "application/json"
        } else {
            "text/plain"
        }
    } else {
        "application/octet-stream"
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The address relative links on an app's HTML page resolve against: the
/// app address without its query, up to the last `/` of its path.
fn base_href(url: &str) -> String {
    let base = url.trim().split(['?', '#']).next().unwrap_or("");
    let after = base.find("://").map_or(0, |i| i + 3);
    match base[after..].rfind('/') {
        Some(i) => base[..after + i + 1].to_string(),
        None => format!("{base}/"),
    }
}

/// The `<title>` of an HTML page, if it has one near the top.
fn html_title(html: &str) -> Option<String> {
    let head: String = html.chars().take(64 * 1024).collect();
    let lower = head.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open = start + lower[start..].find('>')? + 1;
    let end = open + lower[open..].find("</title")?;
    let title = head[open..end].split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then(|| title.chars().take(200).collect())
}

/// `html` with a `<base>` for the app address first in its head, so its
/// relative links stay on the app.
fn with_base(html: &str, url: &str) -> String {
    let tag = format!("<base href=\"{}\">", escape(&base_href(url)));
    let lower: String = html.chars().take(64 * 1024).collect::<String>().to_ascii_lowercase();
    if let Some(i) = lower.find("<head").and_then(|i| lower[i..].find('>').map(|j| i + j + 1)) {
        return format!("{}{tag}{}", &html[..i], &html[i..]);
    }
    if let Some(i) = lower.find("<html").and_then(|i| lower[i..].find('>').map(|j| i + j + 1)) {
        return format!("{}<head>{tag}</head>{}", &html[..i], &html[i..]);
    }
    format!("{tag}{html}")
}

/// One of Morph's pages around an app's answer that is not HTML.
fn wrap(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>:root{{color-scheme:light dark}}body{{margin:0;font:14px/1.5 system-ui,sans-serif}}pre{{margin:0;padding:16px;white-space:pre-wrap;word-break:break-word;font:13px/1.5 ui-monospace,monospace}}.img{{display:flex;min-height:100vh;align-items:center;justify-content:center}}.img img{{max-width:100%;max-height:100vh}}</style></head><body>{body}</body></html>",
        escape(title)
    )
}

fn data_url(mime: &str, charset: Option<&str>, token: &str, bytes: &[u8]) -> String {
    let charset = charset.map(|c| format!(";charset={c}")).unwrap_or_default();
    format!("data:{mime}{charset};{TOKEN_PARAM}{token};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// The page for an app's answer to `url`: HTML as the app sent it (with a
/// `<base>` so relative links stay on the app), images, JSON (indented) and
/// text on a page of Morph's. An error status with nothing to show is
/// [`AppCallError::Status`]; content Morph does not show is
/// [`AppCallError::Unsupported`].
pub fn render(url: &str, status: u16, headers: &[(String, String)], body: &[u8], token: &str) -> Result<AppPage, AppCallError> {
    let (mut essence, charset) = content_type(headers);
    if body.is_empty() && status >= 300 {
        return Err(AppCallError::Status(status));
    }
    if essence.is_empty() || essence == "application/octet-stream" && !body.is_empty() {
        essence = sniff(body).to_string();
    }
    let fallback = describe(url);
    let page = |title: String, kind: &'static str, data_url: String| AppPage { title, data_url, status, kind, bytes: body.len() as u64 };
    if essence == "text/html" || essence == "application/xhtml+xml" {
        let html = String::from_utf8_lossy(body);
        let title = html_title(&html).unwrap_or(fallback);
        let charset = if std::str::from_utf8(body).is_ok() { Some("utf-8") } else { charset.as_deref() };
        return Ok(page(title, "html", data_url("text/html", charset.or(Some("utf-8")), token, with_base(&html, url).as_bytes())));
    }
    if IMAGES.contains(&essence.as_str()) {
        let name = url.split(['?', '#']).next().unwrap_or("").rsplit('/').find(|s| !s.is_empty()).unwrap_or("").to_string();
        let title = if name.is_empty() || name.contains("://") { fallback.clone() } else { name };
        let src = format!("data:{essence};base64,{}", base64::engine::general_purpose::STANDARD.encode(body));
        let html = wrap(&title, &format!("<div class=\"img\"><img alt=\"{}\" src=\"{src}\"></div>", escape(&title)));
        return Ok(page(title, "image", data_url("text/html", Some("utf-8"), token, html.as_bytes())));
    }
    let json = essence == "application/json" || essence.ends_with("+json");
    if json || essence.starts_with("text/") || essence == "application/xml" || essence.ends_with("+xml") || essence == "application/javascript" {
        let text = String::from_utf8_lossy(body);
        let (shown, kind) = match json.then(|| serde_json::from_slice::<serde_json::Value>(body).ok()).flatten() {
            Some(v) => (serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string()), "json"),
            None => (text.to_string(), if json { "json" } else { "text" }),
        };
        let html = wrap(&fallback, &format!("<pre>{}</pre>", escape(&shown)));
        return Ok(page(fallback, kind, data_url("text/html", Some("utf-8"), token, html.as_bytes())));
    }
    if status >= 300 {
        return Err(AppCallError::Status(status));
    }
    Err(AppCallError::Unsupported(if essence.is_empty() { "an unnamed content type".into() } else { essence }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(status: u16, ct: &str, body: &[u8]) -> Result<AppResponse, AppCallError> {
        Ok(AppResponse { status, headers: vec![("Content-Type".into(), ct.into())], body: Box::new(std::io::Cursor::new(body.to_vec())) })
    }

    struct Fixed(fn() -> Result<AppResponse, AppCallError>);
    impl AppCaller for Fixed {
        fn call(&self, _r: AppRequest) -> Result<AppResponse, AppCallError> {
            (self.0)()
        }
    }

    fn decoded(page: &AppPage) -> String {
        let b64 = page.data_url.split_once(";base64,").unwrap().1;
        String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap()
    }

    #[test]
    fn typed_app_links_become_get_requests() {
        let r = request("weirdnet-muninn://Home.Example.com/notes/a.md?x=1#top").unwrap();
        assert_eq!((r.app.as_str(), r.app_name()), ("weirdnet-muninn", "muninn"));
        assert_eq!(r.locator, "Home.Example.com", "the caller folds names; peer ids keep their case");
        assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/notes/a.md?x=1"));
        assert!(r.headers.is_empty() && r.body.is_empty());
        assert_eq!(request("WEIRDNET-Notes://alice").unwrap().path, "/");
        assert_eq!(request("weirdnet-notes://alice?q=1").unwrap().path, "/?q=1");
        let peer = "12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB";
        assert_eq!(request(&format!("weirdnet-notes://{peer}/x")).unwrap().locator, peer);
    }

    #[test]
    fn bad_internal_and_invite_links_are_refused() {
        assert_eq!(request("weirdnet://open/x"), Err(AppCallError::Internal));
        assert!(matches!(request("weirdnet-notes://"), Err(AppCallError::Invalid(_))));
        assert!(matches!(request("weirdnet-notes://a b"), Err(AppCallError::Invalid(_))));
        assert_eq!(request("weirdnet-notes://join/ABCD"), Err(AppCallError::NoHandler("weirdnet-notes".into())));
    }

    #[test]
    fn app_names_follow_weirdnets_rule() {
        assert!(valid_app_name("notes") && valid_app_name("morph-test") && valid_app_name("a1"));
        for bad in ["", "No", "a.b", "a+b", "a_b", &"a".repeat(56)] {
            assert!(!valid_app_name(bad), "{bad:?}");
        }
        assert_eq!(request("weirdnet-a.b://alice/x"), Err(AppCallError::InvalidApp("a.b".into())));
        assert!(request(&format!("weirdnet-{}://alice", "a".repeat(55))).is_ok());
        assert!(matches!(request(&format!("weirdnet-{}://alice", "a".repeat(56))), Err(AppCallError::InvalidApp(_))));
        let (title, body) = error_text("weirdnet-a.b://alice/x", &AppCallError::InvalidApp("a.b".into()));
        assert_eq!(title, "Not a valid Weirdnet app name");
        assert!(body.contains("\"a.b\"") && body.contains("a-z, 0-9 and -"), "{body}");
    }

    #[test]
    fn every_error_has_a_page_that_never_mentions_the_query() {
        let url = "weirdnet-notes://alice/a?token=secret";
        let all = [
            AppCallError::NotSupported,
            AppCallError::NoHandler("weirdnet-notes".into()),
            AppCallError::NoHandler("weirdnet-other".into()),
            AppCallError::NotFound("alice".into()),
            AppCallError::Unreachable("no route".into()),
            AppCallError::TimedOut,
            AppCallError::Refused("members only".into()),
            AppCallError::TooLarge,
            AppCallError::Protocol("bad frame".into()),
            AppCallError::Unsupported("application/zip".into()),
            AppCallError::Status(404),
            AppCallError::Invalid("bad".into()),
        ];
        for e in &all {
            let (title, body) = error_text(url, e);
            assert!(!title.is_empty() && !body.is_empty(), "{e:?}");
            assert!(!title.contains("secret") && !body.contains("secret"), "{e:?}: {body}");
            assert_eq!(AppCallError::decode(&e.encode()).as_ref(), Some(e), "round trip");
        }
        assert_eq!(error_text(url, &AppCallError::NoHandler("weirdnet-notes".into())).0, "alice doesn't serve weirdnet-notes://");
        assert_eq!(error_text(url, &AppCallError::NotFound("alice".into())).0, "Couldn't find alice");
        assert_eq!(error_text(url, &AppCallError::Unreachable("x".into())).0, "Couldn't reach alice");
        assert_eq!(error_text(url, &AppCallError::TimedOut).0, "The app didn't answer in time");
        assert_eq!(error_text(url, &AppCallError::Status(404)).0, "The app answered 404 Not Found");
        assert!(error_text(url, &AppCallError::NotFound("alice".into())).1.contains("not sent to a search engine"));
        assert_eq!(AppCallError::decode("plain words"), None);
    }

    #[test]
    fn without_a_network_calls_are_not_supported() {
        let err = open("weirdnet-muninn://home.example.com/a", &NotYet).unwrap_err();
        assert_eq!(err, AppCallError::NotSupported);
        let stop = AtomicBool::new(false);
        assert_eq!(fetch("weirdnet-muninn://home.example.com/a", &NotYet, &stop, &mut |_| {}), Err(AppCallError::NotSupported));
    }

    #[test]
    fn html_answers_load_as_they_are_with_a_base_and_a_token() {
        let caller =
            Fixed(|| ok(200, "text/html; charset=utf-8", b"<html><head><title> Hi  there </title></head><body><a href=x>x</a></body></html>"));
        let stop = AtomicBool::new(false);
        let mut seen = vec![];
        let page = fetch("weirdnet-notes://Alice/dir/page?q=1", &caller, &stop, &mut |n| seen.push(n)).unwrap();
        assert_eq!((page.title.as_str(), page.kind, page.status), ("Hi there", "html", 200));
        assert!(page.data_url.starts_with("data:text/html;charset=utf-8;morph-app="), "{}", &page.data_url[..60]);
        let token = page_token(&page.data_url).unwrap();
        assert!(!token.is_empty());
        assert!(decoded(&page).contains("<head><base href=\"weirdnet-notes://Alice/dir/\"><title>"), "{}", decoded(&page));
        assert_eq!(seen.last().copied(), Some(page.bytes));
        assert_eq!(page_token("https://example.com"), None);
        assert_eq!(page_token("data:text/html;base64,AAAA"), None);
        assert_ne!(new_token(), new_token());
    }

    #[test]
    fn json_text_and_images_get_a_page_of_morphs() {
        let url = "weirdnet-notes://alice/api/list.json";
        let json = render(url, 200, &[("content-type".into(), "application/json".into())], br#"{"a":[1,2],"b":"<x>"}"#, "ab").unwrap();
        assert_eq!(json.kind, "json");
        let html = decoded(&json);
        assert!(html.contains("<pre>{\n  &quot;a&quot;: [\n    1,") && html.contains("&quot;&lt;x&gt;&quot;"), "{html}");
        let text = render(url, 200, &[("content-type".into(), "text/plain".into())], b"<b>hi</b>", "ab").unwrap();
        assert_eq!(text.kind, "text");
        assert!(decoded(&text).contains("<pre>&lt;b&gt;hi&lt;/b&gt;</pre>"));
        let png = b"\x89PNG\r\n\x1a\n0000";
        let img = render("weirdnet-notes://alice/pics/cat.png", 200, &[], png, "ab").unwrap();
        assert_eq!((img.kind, img.title.as_str()), ("image", "cat.png"));
        assert!(decoded(&img).contains("<img alt=\"cat.png\" src=\"data:image/png;base64,iVBORw0KGgow"));
        let sniffed = render(url, 200, &[], b"{\"x\":1}", "ab").unwrap();
        assert_eq!(sniffed.kind, "json");
    }

    #[test]
    fn errors_statuses_and_unknown_types() {
        let url = "weirdnet-notes://alice/x";
        assert_eq!(render(url, 404, &[], b"", "ab"), Err(AppCallError::Status(404)));
        let page = render(url, 404, &[("content-type".into(), "text/html".into())], b"<p>gone</p>", "ab").unwrap();
        assert_eq!((page.status, page.kind), (404, "html"));
        assert_eq!(
            render(url, 200, &[("content-type".into(), "application/zip".into())], b"PK", "ab"),
            Err(AppCallError::Unsupported("application/zip".into()))
        );
        assert_eq!(render(url, 200, &[], b"\x00\xff\xfe", "ab"), Err(AppCallError::Unsupported("application/octet-stream".into())));
    }

    #[test]
    fn bodies_are_capped_stoppable_and_keep_the_callers_error() {
        let stop = AtomicBool::new(false);
        let mut big = std::io::Cursor::new(vec![0u8; 100]);
        assert_eq!(read_body(&mut big, 99, &stop, &mut |_| {}), Err(AppCallError::TooLarge));
        let mut fine = std::io::Cursor::new(vec![1u8; 100]);
        assert_eq!(read_body(&mut fine, 100, &stop, &mut |_| {}).unwrap().len(), 100);
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(io_error(AppCallError::TimedOut))
            }
        }
        assert_eq!(read_body(&mut Broken, 10, &stop, &mut |_| {}), Err(AppCallError::TimedOut));
        stop.store(true, Ordering::Relaxed);
        let mut any = std::io::Cursor::new(vec![1u8; 10]);
        assert_eq!(read_body(&mut any, 100, &stop, &mut |_| {}), Err(AppCallError::Stopped));
    }
}
