//! Calling a Weirdnet app from the address bar: `weirdnet-<app>://<locator>/<path>`.
//!
//! Such a link names a resource another peer serves (a name or user id, then
//! a path), so opening it is a call over Weirdnet: resolve the locator to a
//! peer, send it a request (method, path, headers, body) and show the
//! streamed response. It is never handed to a local OS handler and never
//! becomes a search.
//!
//! weirdnet-net 0.4 has no generic app call yet (its request/response node is
//! typed per protocol, and nothing resolves an app locator to a peer), so the
//! only [`AppCaller`] today is [`NotYet`], and every shell shows the "app call
//! not supported yet" page ([`error_text`]). When weirdnet-net gains the call,
//! an `AppCaller` backed by it plugs in here without touching the shells.

use std::fmt;
use std::io::Read;

use weirdnet_schemes::link::{Action, Link};

/// Longest locator or path kept in a log-safe description.
const DESCRIBE_MAX: usize = 120;

/// One call to a Weirdnet app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRequest {
    /// The app's scheme, lowercase (`weirdnet-muninn`).
    pub app: String,
    /// Who serves it: a name (`home.example.com`) or user id, lowercase.
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

/// An app's answer: status and headers first, the body as it streams in.
pub struct AppResponse {
    /// HTTP-style status (200, 404, …).
    pub status: u16,
    /// Response headers, in order (`content-type` among them).
    pub headers: Vec<(String, String)>,
    /// The body, read as it arrives.
    pub body: Box<dyn Read + Send>,
}

impl fmt::Debug for AppResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppResponse").field("status", &self.status).field("headers", &self.headers).finish_non_exhaustive()
    }
}

/// Why an app call did not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppCallError {
    /// The address is not a valid app link.
    #[error("not a valid Weirdnet app link: {0}")]
    Invalid(String),
    /// `weirdnet://` is internal to Weirdnet and never opened.
    #[error("weirdnet:// addresses are internal to Weirdnet")]
    Internal,
    /// This build cannot call Weirdnet apps yet.
    #[error("calling Weirdnet apps is not supported yet")]
    NotSupported,
    /// No app is registered for the scheme.
    #[error("no app is registered for {0}://")]
    NoHandler(String),
    /// The locator did not resolve to any peer.
    #[error("could not find {0} on Weirdnet")]
    NotFound(String),
    /// The peer was found but could not be reached (directly or by relay).
    #[error("could not reach {0}")]
    Unreachable(String),
    /// The peer did not answer in time.
    #[error("the app did not answer in time")]
    TimedOut,
    /// The peer refused the call.
    #[error("the app refused the call: {0}")]
    Refused(String),
}

/// Something that can call Weirdnet apps. The shells hold one and send every
/// `weirdnet-<app>://` address through it.
pub trait AppCaller: Send + Sync {
    /// Sends `request` and returns the response once its status and headers
    /// are in (the body streams after).
    fn call(&self, request: AppRequest) -> Result<AppResponse, AppCallError>;
}

/// The caller this build has: weirdnet-net 0.4 cannot make a generic app
/// call, so every call is [`AppCallError::NotSupported`].
#[derive(Debug, Clone, Copy, Default)]
pub struct NotYet;

impl AppCaller for NotYet {
    fn call(&self, _request: AppRequest) -> Result<AppResponse, AppCallError> {
        Err(AppCallError::NotSupported)
    }
}

/// The `GET` request a typed `weirdnet-<app>://<locator>/<path>` address
/// makes.
pub fn request(url: &str) -> Result<AppRequest, AppCallError> {
    let url = url.trim();
    let scheme = crate::address::scheme_of(url).unwrap_or("");
    if scheme.eq_ignore_ascii_case(weirdnet_schemes::names::INTERNAL_SCHEME) {
        return Err(AppCallError::Internal);
    }
    let link = Link::parse(url).map_err(|e| AppCallError::Invalid(e.to_string()))?;
    let Action::Resource { authority, .. } = link.action else {
        // `join/<code>` and `answer/<code>` are invites for that app, which
        // only the app itself (or Morph's device linking) takes.
        return Err(AppCallError::NoHandler(link.scheme));
    };
    let rest = url.split_once("://").map_or("", |(_, r)| r);
    let rest = rest.split('#').next().unwrap_or("");
    let path = match rest.find(['/', '?']) {
        Some(i) if rest[i..].starts_with('?') => format!("/{}", &rest[i..]),
        Some(i) => rest[i..].to_string(),
        None => "/".to_string(),
    };
    Ok(AppRequest { app: link.scheme, locator: authority, method: "GET".into(), path, headers: vec![], body: vec![] })
}

/// Opens `url` through `caller`.
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

/// Title and body for the page shown instead of `url` when its call fails.
pub fn error_text(url: &str, error: &AppCallError) -> (String, String) {
    let shown = describe(url);
    match error {
        AppCallError::NotSupported => (
            "App call not supported yet".into(),
            format!(
                "{shown} is a Weirdnet app link. Morph opens these by calling the app over Weirdnet, and that call is not supported in this version yet. It was not sent to a search engine."
            ),
        ),
        AppCallError::NoHandler(app) => (
            format!("No app for {app}://"),
            format!("No Weirdnet app is registered for {app}:// links, so {shown} can't be opened. It was not sent to a search engine."),
        ),
        AppCallError::Internal => {
            ("weirdnet:// can't be opened".into(), "weirdnet:// addresses are internal to Weirdnet. App links start with weirdnet-<app>://.".into())
        }
        AppCallError::Invalid(why) => {
            ("Not a valid Weirdnet link".into(), format!("{shown} is not a valid weirdnet-<app>://<name>/<path> link ({why})."))
        }
        other => ("This app couldn't be opened".into(), format!("{shown}: {other}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_app_links_become_get_requests() {
        let r = request("weirdnet-muninn://Home.Example.com/notes/a.md?x=1#top").unwrap();
        assert_eq!(r.app, "weirdnet-muninn");
        assert_eq!(r.locator, "home.example.com");
        assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/notes/a.md?x=1"));
        assert!(r.headers.is_empty() && r.body.is_empty());
        assert_eq!(request("WEIRDNET-Notes://alice").unwrap().path, "/");
        assert_eq!(request("weirdnet-notes://alice?q=1").unwrap().path, "/?q=1");
    }

    #[test]
    fn bad_internal_and_invite_links_are_refused() {
        assert_eq!(request("weirdnet://open/x"), Err(AppCallError::Internal));
        assert!(matches!(request("weirdnet-notes://"), Err(AppCallError::Invalid(_))));
        assert!(matches!(request("weirdnet-notes://a b"), Err(AppCallError::Invalid(_))));
        assert_eq!(request("weirdnet-notes://join/ABCD"), Err(AppCallError::NoHandler("weirdnet-notes".into())));
    }

    #[test]
    fn this_build_says_app_calls_are_not_supported_yet() {
        let err = open("weirdnet-muninn://home.example.com/a", &NotYet).unwrap_err();
        assert_eq!(err, AppCallError::NotSupported);
        let (title, body) = error_text("weirdnet-muninn://home.example.com/a?token=secret", &err);
        assert_eq!(title, "App call not supported yet");
        assert!(body.contains("weirdnet-muninn://home.example.com/a") && !body.contains("secret"));
        assert!(body.contains("not sent to a search engine"));
    }

    #[test]
    fn a_caller_plugs_in_behind_the_trait() {
        struct Echo;
        impl AppCaller for Echo {
            fn call(&self, r: AppRequest) -> Result<AppResponse, AppCallError> {
                let body = format!("{} {}{}", r.method, r.locator, r.path).into_bytes();
                Ok(AppResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/plain".into())],
                    body: Box::new(std::io::Cursor::new(body)),
                })
            }
        }
        let mut res = open("weirdnet-notes://alice/x", &Echo).unwrap();
        let mut text = String::new();
        res.body.read_to_string(&mut text).unwrap();
        assert_eq!((res.status, text.as_str()), (200, "GET alice/x"));
    }
}
