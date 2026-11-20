//! The password saver's side of the page bridge: a host script that notices
//! login forms and the login a user submits, the checks on what it sends,
//! and the page actions that fill a saved login back in
//! (`docs/bridge-api.md`, "Login forms"; `docs/profile-lock.md`, "Password
//! saver").
//!
//! The rules are strict on purpose:
//!
//! - Only the **host channel** counts ([`Trust::Host`]): page scripts cannot
//!   claim a form was submitted. [`crate::bridge::check_host`] already
//!   refused other tokens, subframes and origins that are not the view's.
//! - Only the **main frame** of a page the user sees: the host script does
//!   not run in frames, and background views, hidden host pages and popups
//!   are ignored. A login inside an iframe (same origin or not) is never
//!   captured or filled.
//! - Only **https**: nothing is captured on, or filled into, an `http` page.
//! - Filling goes to the exact origin the login was saved for (scheme, host
//!   and port; [`fill_allowed`]), and the page action itself refuses when
//!   the page moved elsewhere ([`ActionRequest::on_origin`]). It runs in
//!   Morph's script world and never reads a password field back.

use crate::ViewId;
use crate::bridge::{BridgeContext, FrameKind, Trust};
use crate::page_action::{Action, ActionRequest, Locator};

/// Sent once per page when a visible password field shows up.
pub const FORM: &str = "host.saver.form";
/// Sent when the user submits a login: `{username, password}`.
pub const SUBMIT: &str = "host.saver.submit";

/// The longest username or password taken from a page (bytes).
pub const FIELD_MAX: usize = 4096;

/// The host script (the body of `function (MorphHost) { … }`). It runs in
/// the main frame before the page's scripts; on WebKitGTK in Morph's own
/// script world, elsewhere with the builtins it uses captured first.
pub const HOST_SCRIPT: &str = r#"
const doc = document, A = Array.from, setT = setTimeout, P = 'input[type="password"]';
const ac = (e) => String(e.getAttribute("autocomplete") || "").toLowerCase();
const shown = (e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
let formSent = false, last = "", timer = 0, watcher = null;
const post = (m) => { try { MorphHost.post(m).catch(() => {}); } catch (_) {} };
const look = () => {
  timer = 0;
  if (formSent) return;
  if (A(doc.querySelectorAll(P)).some(shown)) {
    formSent = true;
    if (watcher) watcher.disconnect();
    post({ type: "host.saver.form" });
  }
};
const soon = () => { if (!timer && !formSent) timer = setT(look, 250); };
const userFor = (pw) => {
  const inputs = A((pw.form || doc).querySelectorAll("input"));
  const named = inputs.find((e) => /\busername\b/.test(ac(e)) && e.value);
  if (named) return named.value;
  for (let i = inputs.indexOf(pw) - 1; i >= 0; i--) {
    const e = inputs[i], t = String(e.type || "text").toLowerCase();
    if ((t === "text" || t === "email" || t === "tel") && e.value) return e.value;
  }
  return "";
};
const capture = (scope) => {
  const pws = A((scope || doc).querySelectorAll(P)).filter((e) => e.value);
  if (!pws.length) return;
  const pw = pws.find((e) => ac(e) === "new-password") || pws[0];
  const username = String(userFor(pw)).trim(), password = String(pw.value);
  const key = username + "\u0000" + password;
  if (key === last) return;
  last = key;
  post({ type: "host.saver.submit", payload: { username: username, password: password } });
};
doc.addEventListener("submit", (ev) => capture(ev.target instanceof HTMLFormElement ? ev.target : doc), true);
doc.addEventListener("click", (ev) => {
  if (!ev.isTrusted || !ev.target || !ev.target.closest) return;
  const b = ev.target.closest('button, input[type="submit"], input[type="image"], [role="button"]');
  if (!b) return;
  const type = String(b.getAttribute("type") || "").toLowerCase();
  if (b.form && (type === "button" || type === "reset")) return;
  capture(b.form || doc);
}, true);
doc.addEventListener("keydown", (ev) => {
  if (ev.isTrusted && ev.key === "Enter" && ev.target && ev.target.tagName === "INPUT") capture(ev.target.form || doc);
}, true);
const watch = () => {
  look();
  if (formSent) return;
  try {
    watcher = new MutationObserver(soon);
    watcher.observe(doc.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["type", "style", "class", "hidden"] });
  } catch (_) {}
};
if (doc.readyState === "loading") doc.addEventListener("DOMContentLoaded", watch, { once: true }); else watch();
"#;

/// What a page said about logins, after the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageLogin {
    /// The page shows a password field: offer to fill a saved login.
    Form { view: ViewId, origin: String },
    /// The user submitted a login: offer to save it.
    Submitted { view: ViewId, origin: String, url: String, username: String, password: String },
}

impl PageLogin {
    pub fn view(&self) -> ViewId {
        match self {
            PageLogin::Form { view, .. } | PageLogin::Submitted { view, .. } => *view,
        }
    }
}

/// Whether the saver may act on a page whose origin is `origin`: https only.
pub fn secure_origin(origin: &str) -> bool {
    origin.strip_prefix("https://").is_some_and(|host| !host.is_empty())
}

/// Whether a login saved for `saved_origin` may be filled into the page at
/// `page_url`: the page is https and its origin (scheme, host and port) is
/// exactly the saved one. Subdomains, other ports and http never match.
pub fn fill_allowed(saved_origin: &str, page_url: &str) -> bool {
    let page = crate::bridge::origin_of(page_url);
    secure_origin(&page) && page == saved_origin
}

/// Whether `body` (a bridge message) is one of the saver's.
pub fn is_saver_message(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(|t| t == FORM || t == SUBMIT))
        .unwrap_or(false)
}

/// Check one bridge message: `Some` only for a saver message from the host
/// channel, in the main frame of an https page a user sees.
pub fn parse(ctx: &BridgeContext, body: &str) -> Option<PageLogin> {
    if ctx.trust != Trust::Host || ctx.frame != FrameKind::Main || ctx.background || ctx.popup || !secure_origin(&ctx.origin) {
        return None;
    }
    let msg: serde_json::Value = serde_json::from_str(body).ok()?;
    match msg.get("type")?.as_str()? {
        FORM => Some(PageLogin::Form { view: ctx.view, origin: ctx.origin.clone() }),
        SUBMIT => {
            let payload = msg.get("payload")?;
            let field = |k: &str| payload.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let (username, password) = (field("username")?, field("password")?);
            if password.is_empty() || password.len() > FIELD_MAX || username.len() > FIELD_MAX {
                return None;
            }
            Some(PageLogin::Submitted {
                view: ctx.view,
                origin: ctx.origin.clone(),
                url: ctx.url.clone(),
                username: username.trim().to_string(),
                password,
            })
        }
        _ => None,
    }
}

/// The answer to a saver message (its id, so the host script's promise settles).
pub fn reply(body: &str) -> Option<String> {
    let id = serde_json::from_str::<serde_json::Value>(body).ok()?.get("id")?.as_str()?.to_string();
    Some(serde_json::json!({"id": id, "type": "ok"}).to_string())
}

/// Where a username goes: the field marked for it, else an email or a field
/// named like one (shown fields first: [`Locator`]).
pub const USERNAME_FIELDS: &str = "input[autocomplete~=\"username\" i], input[type=\"email\"], \
input:not([type]):is([name*=\"user\" i], [name*=\"login\" i], [name*=\"email\" i], [id*=\"user\" i], [id*=\"login\" i], [id*=\"email\" i]), \
input[type=\"text\"]:is([name*=\"user\" i], [name*=\"login\" i], [name*=\"email\" i], [id*=\"user\" i], [id*=\"login\" i], [id*=\"email\" i])";

/// Where a password goes: a password field that is not for a new password.
pub const PASSWORD_FIELDS: &str = "input[type=\"password\"]:not([autocomplete=\"new-password\" i])";

/// The page actions that fill a login saved for `origin`: the username
/// (when there is one; a page without a username field still gets the
/// password), then the password. Both are refused if the page is not on
/// `origin` when they run.
pub fn fill_actions(origin: &str, username: &str, password: &str) -> Vec<ActionRequest> {
    let mut out = Vec::new();
    if !username.is_empty() {
        out.push(ActionRequest::new(Action::Fill { target: Locator::selector(USERNAME_FIELDS), value: username.to_string() }).on_origin(origin));
    }
    out.push(ActionRequest::new(Action::Fill { target: Locator::selector(PASSWORD_FIELDS), value: password.to_string() }).on_origin(origin));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::ViewBridge;

    fn ctx(url: &str, trust: Trust, frame: FrameKind) -> BridgeContext {
        ViewBridge::new(ViewId(7), true, false).context(url, frame, trust)
    }

    fn submit(user: &str, pw: &str) -> String {
        serde_json::json!({"id": "h1", "type": SUBMIT, "payload": {"username": user, "password": pw}}).to_string()
    }

    #[test]
    fn only_host_messages_from_the_main_frame_of_a_shown_https_page_count() {
        let ok = ctx("https://example.com/login", Trust::Host, FrameKind::Main);
        assert_eq!(
            parse(&ok, &submit(" amy ", "s3cret")),
            Some(PageLogin::Submitted {
                view: ViewId(7),
                origin: "https://example.com".into(),
                url: "https://example.com/login".into(),
                username: "amy".into(),
                password: "s3cret".into()
            })
        );
        assert_eq!(
            parse(&ok, r#"{"id":"h2","type":"host.saver.form"}"#),
            Some(PageLogin::Form { view: ViewId(7), origin: "https://example.com".into() })
        );
        // The page channel can say anything: never a login.
        assert_eq!(parse(&ctx("https://example.com/", Trust::Page, FrameKind::Unknown), &submit("amy", "x")), None);
        assert_eq!(parse(&ctx("https://example.com/", Trust::Host, FrameKind::Sub), &submit("amy", "x")), None);
        assert_eq!(parse(&ctx("http://example.com/", Trust::Host, FrameKind::Main), &submit("amy", "x")), None, "no capture on http");
        let background = ViewBridge::new(ViewId(8), false, true).context("https://example.com/", FrameKind::Main, Trust::Host);
        assert_eq!(parse(&background, &submit("amy", "x")), None);
        let popup = ViewBridge::new(ViewId(9), true, false).for_popup().context("https://example.com/", FrameKind::Main, Trust::Host);
        assert_eq!(parse(&popup, &submit("amy", "x")), None);
        assert_eq!(parse(&ok, &submit("amy", "")), None, "no password, nothing to save");
        assert_eq!(parse(&ok, &submit("amy", &"x".repeat(FIELD_MAX + 1))), None);
        assert_eq!(parse(&ok, r#"{"id":"h3","type":"host.read"}"#), None);
        assert_eq!(parse(&ok, "not json"), None);
    }

    #[test]
    fn filling_needs_https_and_the_exact_origin() {
        assert!(fill_allowed("https://example.com", "https://example.com/login?next=/"));
        assert!(fill_allowed("https://example.com", "https://EXAMPLE.com:443/"));
        assert!(fill_allowed("https://example.com:8443", "https://example.com:8443/x"));
        for page in [
            "http://example.com/",
            "https://example.com:8443/",
            "https://www.example.com/",
            "https://example.com.evil.test/",
            "https://evil.test/?u=https://example.com",
            "about:blank",
            "",
        ] {
            assert!(!fill_allowed("https://example.com", page), "{page}");
        }
        assert!(!fill_allowed("http://example.com", "http://example.com/"), "never on http, even if saved so");
    }

    #[test]
    fn fill_actions_are_pinned_to_the_origin_and_skip_an_empty_username() {
        let actions = fill_actions("https://example.com", "amy", "s3cret");
        assert_eq!(actions.len(), 2);
        assert!(actions.iter().all(|a| a.expect_origin.as_deref() == Some("https://example.com") && !a.allow_page_world));
        assert!(
            matches!(&actions[0].action, Action::Fill { target, value } if value == "amy" && target.selector.as_deref() == Some(USERNAME_FIELDS))
        );
        assert!(
            matches!(&actions[1].action, Action::Fill { target, value } if value == "s3cret" && target.selector.as_deref() == Some(PASSWORD_FIELDS))
        );
        let only = fill_actions("https://example.com", "", "s3cret");
        assert_eq!(only.len(), 1);
        assert!(matches!(&only[0].action, Action::Fill { value, .. } if value == "s3cret"));
    }

    #[test]
    fn saver_messages_are_recognized_and_answered() {
        assert!(is_saver_message(&submit("a", "b")));
        assert!(is_saver_message(r#"{"type":"host.saver.form"}"#));
        assert!(!is_saver_message(r#"{"type":"host.read"}"#));
        assert_eq!(reply(&submit("a", "b")).as_deref(), Some(r#"{"id":"h1","type":"ok"}"#));
        assert!(!reply(&submit("a", "secret-value")).unwrap().contains("secret-value"), "replies never echo the password");
    }

    #[test]
    fn the_host_script_watches_submits_and_only_trusted_input() {
        assert!(HOST_SCRIPT.contains("\"submit\""));
        assert!(HOST_SCRIPT.contains("ev.isTrusted"), "Morph's own synthetic fill events never capture");
        assert!(HOST_SCRIPT.contains(FORM) && HOST_SCRIPT.contains(SUBMIT));
        assert!(!HOST_SCRIPT.contains("window.MorphBridge"), "never the page channel");
    }
}
