//! Host-initiated actions in a page (docs/bridge-api.md): find an element by
//! CSS selector, ARIA role, accessible name or text, then click it, fill it,
//! focus it, outline it, read it back, or scroll the page or to it. The
//! companion's tier 2 tools call these after the user approved the action;
//! nothing here asks.
//!
//! Same trust model as [`crate::page_read`]: only Morph's Rust side can start
//! one (there is no bridge message for it, and `host.*` types are refused on
//! the page channel), and the script runs in Morph's own script world
//! ([`Isolation::ScriptWorld`]: WebKitGTK script world, WKContentWorld, a CDP
//! isolated world on WebView2), so page scripts can't see it or change the
//! builtins it uses. They share the DOM with it, so they see its effects (the
//! click, the input events), which is the point. A view that can run it only
//! in the page's world refuses with [`ActionError::BlockedByIsolation`]
//! unless the request allows that ([`ActionRequest::allow_page_world`]).
//!
//! Events it sends are synthetic (`isTrusted` is false): pages that ignore
//! those (some anti-abuse checks) won't react to them.
//!
//! One answer per action, at most [`ANSWER_MAX`] bytes, handed over by
//! [`PageActions::tick`] from the shell's poll loop, like reads.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::page_read::{Isolation, Pending};
use crate::{ViewId, WebView};

/// How long an action waits for its answer unless the request says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most bytes (UTF-8 JSON) one action answers with.
pub const ANSWER_MAX: usize = 64 * 1024;

/// Characters of an element's text read back.
pub const TEXT_MAX: usize = 4000;

/// The longest value [`Action::Fill`] types (bytes).
pub const FILL_MAX: usize = 1024 * 1024;

/// The longest an [`Action::Highlight`] outline stays.
pub const HIGHLIGHT_MAX: Duration = Duration::from_secs(30);

/// Which element. Every part given must match; `nth` picks among the
/// matches (in document order, the ones on screen first).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Locator {
    /// A CSS selector (`querySelectorAll`, main document, no shadow roots).
    pub selector: Option<String>,
    /// An ARIA role, explicit (`role=`) or implied by the tag (`button`,
    /// `link`, `textbox`, `checkbox`, `combobox`, `heading`, ...).
    pub role: Option<String>,
    /// The accessible name (`aria-labelledby`, `aria-label`, `<label>`,
    /// the text of a button or link, `alt`, `placeholder`, `title`):
    /// whitespace-folded and case-insensitive, exact matches first, else
    /// names containing it.
    pub name: Option<String>,
    /// Text the element contains (case-insensitive); alone, the innermost
    /// elements containing it.
    pub text: Option<String>,
    pub nth: usize,
}

impl Locator {
    pub fn selector(selector: impl Into<String>) -> Self {
        Self { selector: Some(selector.into()), ..Self::default() }
    }
    pub fn role(role: impl Into<String>, name: Option<&str>) -> Self {
        Self { role: Some(role.into()), name: name.map(str::to_string), ..Self::default() }
    }
    pub fn name(name: impl Into<String>) -> Self {
        Self { name: Some(name.into()), ..Self::default() }
    }
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: Some(text.into()), ..Self::default() }
    }
    pub fn nth(self, nth: usize) -> Self {
        Self { nth, ..self }
    }
    fn is_empty(&self) -> bool {
        [&self.selector, &self.role, &self.name, &self.text].iter().all(|p| p.as_deref().is_none_or(|s| s.trim().is_empty()))
    }
}

/// What to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Scroll it into view, check it can be clicked (shown, enabled, not
    /// covered at its centre), focus it and send pointer and mouse
    /// down/up and a click.
    Click(Locator),
    /// Put `value` in a text field, text area, `<select>` (by option value,
    /// else option text) or contenteditable element, with `input` and
    /// `change` events, and read it back.
    Fill {
        target: Locator,
        value: String,
    },
    Focus(Locator),
    /// Read it back without touching it.
    Read(Locator),
    /// Scroll it into view and outline it for `duration` (at most
    /// [`HIGHLIGHT_MAX`]).
    Highlight {
        target: Locator,
        duration: Duration,
    },
    /// Scroll by `dx`, `dy` CSS pixels (positive: right, down): the page,
    /// or when the page can't move that way, the scrollable box under the
    /// middle of the view.
    ScrollBy {
        dx: f64,
        dy: f64,
    },
    /// Scroll the element into view (centred).
    ScrollTo(Locator),
}

/// One action.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRequest {
    pub action: Action,
    /// The origin the user approved the action for: refused with
    /// [`ActionError::WrongOrigin`] when the page is elsewhere now.
    pub expect_origin: Option<String>,
    /// Run it in the page's own world where the view has no script world
    /// of Morph's ([`Isolation::PageWorld`]); the page's scripts could then
    /// change what it does and what it reports. Off by default.
    pub allow_page_world: bool,
    pub timeout: Duration,
}

impl ActionRequest {
    pub fn new(action: Action) -> Self {
        Self { action, expect_origin: None, allow_page_world: false, timeout: DEFAULT_TIMEOUT }
    }

    /// Only on `origin` (`scheme://host[:port]`).
    pub fn on_origin(self, origin: impl Into<String>) -> Self {
        Self { expect_origin: Some(origin.into()), ..self }
    }
}

/// An element as read back.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Element {
    /// Lower case (`button`, `input`).
    pub tag: String,
    pub role: String,
    pub name: String,
    /// Its text, whitespace folded, at most [`TEXT_MAX`] characters.
    pub text: String,
    /// A field's value (never a password field's: see `secret`).
    pub value: Option<String>,
    /// A password field: its value is not read back.
    pub secret: bool,
    pub checked: Option<bool>,
    pub disabled: bool,
    pub editable: bool,
    pub visible: bool,
    /// A link's absolute address.
    pub href: Option<String>,
    /// Where it is in the viewport, CSS pixels.
    pub rect: Option<Bounds>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where the page (or the box that scrolled) is after the action.
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct ScrollState {
    pub x: f64,
    pub y: f64,
    pub max_x: f64,
    pub max_y: f64,
    /// It moved ([`Action::ScrollBy`], [`Action::ScrollTo`]).
    pub moved: bool,
    /// A box in the page scrolled, not the page itself.
    pub inner: bool,
}

/// An action's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionOutcome {
    pub view: ViewId,
    /// The page's address when the action ran.
    pub url: String,
    pub origin: String,
    pub isolation: Isolation,
    /// How many elements matched the locator (0 for [`Action::ScrollBy`]).
    pub matches: usize,
    /// The element acted on, read back after the action.
    pub element: Option<Element>,
    pub scroll: ScrollState,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ActionError {
    #[error("this web view can't run page actions")]
    Unsupported,
    #[error("the web view is gone")]
    Gone,
    /// The page did not answer in time (the action may still have run).
    #[error("the page did not answer in time")]
    Timeout,
    #[error("no element matched")]
    NotFound,
    /// Found, but hidden, disabled, covered, read-only or not a field.
    #[error("the element can't be used: {0}")]
    NotInteractable(String),
    /// The view runs scripts only in the page's world and the request did not allow that.
    #[error("refused: this web view has no script world of Morph's ({0:?})")]
    BlockedByIsolation(Isolation),
    #[error("refused: approved for {expected}, but the page is on {actual}")]
    WrongOrigin { expected: String, actual: String },
    #[error("the page moved from {from} to {to}")]
    Navigated { from: String, to: String },
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("page action failed: {0}")]
    Script(String),
}

pub type ActionResult = Result<ActionOutcome, ActionError>;
pub type ActionDone = Box<dyn FnOnce(ActionResult)>;

/// Identifies an action for the life of its [`PageActions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActionId(pub u64);

/// What the page side gets (JSON).
#[derive(Serialize)]
struct Wire<'a> {
    op: &'static str,
    target: Option<&'a Locator>,
    value: Option<&'a str>,
    dx: f64,
    dy: f64,
    ms: u64,
    origin: &'a str,
    text_max: usize,
}

/// Check `request` and build its page side; `origin` is the engine's.
pub fn script(request: &ActionRequest, origin: &str, id: u64) -> Result<String, ActionError> {
    let bad = |m: &str| Err(ActionError::BadRequest(m.into()));
    let (op, target, value, (dx, dy), ms) = match &request.action {
        Action::Click(l) => ("click", Some(l), None, (0.0, 0.0), 0),
        Action::Fill { target, value } => {
            if value.len() > FILL_MAX {
                return bad("the value is too long");
            }
            ("fill", Some(target), Some(value.as_str()), (0.0, 0.0), 0)
        }
        Action::Focus(l) => ("focus", Some(l), None, (0.0, 0.0), 0),
        Action::Read(l) => ("read", Some(l), None, (0.0, 0.0), 0),
        Action::Highlight { target, duration } => ("highlight", Some(target), None, (0.0, 0.0), duration.min(&HIGHLIGHT_MAX).as_millis() as u64),
        Action::ScrollBy { dx, dy } => {
            if !dx.is_finite() || !dy.is_finite() {
                return bad("scroll by a finite distance");
            }
            ("scrollBy", None, None, (*dx, *dy), 0)
        }
        Action::ScrollTo(l) => ("scrollTo", Some(l), None, (0.0, 0.0), 0),
    };
    if target.is_some_and(Locator::is_empty) {
        return bad("the locator is empty: give a selector, role, name or text");
    }
    let wire = Wire { op, target, value, dx, dy, ms, origin, text_max: TEXT_MAX };
    let json = serde_json::to_string(&wire).map_err(|e| ActionError::BadRequest(e.to_string()))?;
    Ok(format!("/*morph-action:{id}*/(() => {{\n  const req = {json};\n  {ACTION_BODY}\n}})()"))
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    url: String,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    matches: usize,
    element: Option<Element>,
    #[serde(default)]
    scroll: ScrollState,
}

/// The page side's answer (`O<json>` or `E<code>:<detail>`) as a result.
pub fn parse_answer(view: ViewId, isolation: Isolation, started_url: &str, answer: Option<&str>) -> ActionResult {
    let answer = answer.ok_or_else(|| ActionError::Script("the script did not run".into()))?;
    if answer.len() > ANSWER_MAX {
        return Err(ActionError::Script("the answer is too large".into()));
    }
    if let Some(e) = answer.strip_prefix('E') {
        let (code, detail) = e.split_once(':').unwrap_or((e, ""));
        return Err(match code {
            "notfound" => ActionError::NotFound,
            "interactable" => ActionError::NotInteractable(detail.into()),
            "origin" => ActionError::Navigated { from: started_url.into(), to: detail.into() },
            "bad" => ActionError::BadRequest(detail.into()),
            "frame" => ActionError::Script("the action ran outside the main frame".into()),
            _ => ActionError::Script(detail.into()),
        });
    }
    let json = answer.strip_prefix('O').ok_or_else(|| ActionError::Script("bad answer".into()))?;
    let raw: Raw = serde_json::from_str(json).map_err(|e| ActionError::Script(format!("bad answer: {e}")))?;
    // The engine's address is the truth: the page must have been where the
    // engine said when the action began.
    let origin = crate::bridge::origin_of(started_url);
    let page_origin = if raw.origin.is_empty() || raw.origin == "null" { crate::bridge::origin_of(&raw.url) } else { raw.origin.clone() };
    if page_origin != origin {
        return Err(ActionError::Navigated { from: started_url.into(), to: raw.url });
    }
    Ok(ActionOutcome { view, url: raw.url, origin, isolation, matches: raw.matches, element: raw.element, scroll: raw.scroll })
}

/// Script answers, from whatever thread the engine calls back on.
type Inbox = Arc<Mutex<Vec<(ActionId, Option<String>)>>>;

struct Live {
    view: ViewId,
    started_url: String,
    isolation: Isolation,
    deadline: Instant,
    done: ActionDone,
}

/// The actions under way (one per shell window, beside [`crate::PageReads`]).
#[derive(Default)]
pub struct PageActions {
    next: u64,
    inbox: Inbox,
    live: HashMap<ActionId, Live>,
    /// Actions refused before they began.
    failed: Vec<(ActionDone, ActionError)>,
}

impl PageActions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Actions not answered yet.
    pub fn pending(&self) -> usize {
        self.live.len() + self.failed.len()
    }

    /// Start `request` in `view`; `done` gets the answer from a later [`Self::tick`].
    pub fn start(&mut self, view: &dyn WebView, request: ActionRequest, done: ActionDone) -> ActionId {
        self.next += 1;
        let id = ActionId(self.next);
        match self.begin(view, &request, id) {
            Ok(started_url) => {
                let isolation = view.isolation();
                self.live.insert(id, Live { view: view.id(), started_url, isolation, deadline: Instant::now() + request.timeout, done });
            }
            Err(e) => self.failed.push((done, e)),
        }
        id
    }

    fn begin(&mut self, view: &dyn WebView, request: &ActionRequest, id: ActionId) -> Result<String, ActionError> {
        match view.isolation() {
            Isolation::None => return Err(ActionError::Unsupported),
            Isolation::PageWorld if !request.allow_page_world => return Err(ActionError::BlockedByIsolation(Isolation::PageWorld)),
            _ => {}
        }
        let url = view.engine_url().ok_or(ActionError::Gone)?;
        let origin = crate::bridge::origin_of(&url);
        if let Some(expected) = &request.expect_origin
            && *expected != origin
        {
            return Err(ActionError::WrongOrigin { expected: expected.clone(), actual: origin });
        }
        let script = script(request, &origin, id.0)?;
        let inbox = self.inbox.clone();
        if !view.evaluate_isolated(&script, Box::new(move |r| inbox.lock().map(|mut i| i.push((id, r))).unwrap_or(()))) {
            return Err(ActionError::Unsupported);
        }
        Ok(url)
    }

    /// [`Self::start`], answered through a future.
    pub fn run(&mut self, view: &dyn WebView, request: ActionRequest) -> Pending<ActionResult> {
        let (pending, resolver) = Pending::new();
        self.start(view, request, Box::new(move |r| resolver.resolve(r)));
        pending
    }

    /// Fail every action of `view` (it was closed).
    pub fn forget_view(&mut self, view: ViewId) {
        let ids: Vec<ActionId> = self.live.iter().filter(|(_, l)| l.view == view).map(|(id, _)| *id).collect();
        for id in ids {
            if let Some(live) = self.live.remove(&id) {
                self.failed.push((live.done, ActionError::Gone));
            }
        }
    }

    /// Hand over answers and timeouts. `view` finds a view by id; callbacks
    /// run here and must not call back into this `PageActions`.
    pub fn tick<'a>(&mut self, now: Instant, view: impl Fn(ViewId) -> Option<&'a dyn WebView>) {
        let mut ended: Vec<(ActionDone, ActionResult)> = self.failed.drain(..).map(|(d, e)| (d, Err(e))).collect();
        let answers = self.inbox.lock().map(|mut i| std::mem::take(&mut *i)).unwrap_or_default();
        for (id, answer) in answers {
            let Some(live) = self.live.remove(&id) else {
                continue;
            };
            let result = match view(live.view) {
                Some(_) => parse_answer(live.view, live.isolation, &live.started_url, answer.as_deref()),
                None => Err(ActionError::Gone),
            };
            ended.push((live.done, result));
        }
        let late: Vec<ActionId> = self.live.iter().filter(|(_, l)| now >= l.deadline).map(|(id, _)| *id).collect();
        for id in late {
            let live = self.live.remove(&id).expect("listed");
            ended.push((live.done, Err(ActionError::Timeout)));
        }
        for (done, result) in ended {
            done(result);
        }
    }
}

/// The page side: `req` is [`Wire`]. Answers `O<json>` or `E<code>:<detail>`.
const ACTION_BODY: &str = r##"const fail = (code, detail) => { throw { morph: code, detail: String(detail || "") }; };
  try {
    if (window !== window.top) fail("frame");
    if (location.origin !== req.origin && !(req.origin === "null" && location.origin === "null")) fail("origin", location.href);
    const fold = (s) => String(s || "").replace(/\s+/g, " ").trim();
    const low = (s) => fold(s).toLowerCase();
    const INPUT_ROLES = { button: "button", submit: "button", reset: "button", image: "button", checkbox: "checkbox", radio: "radio", range: "slider", number: "spinbutton", search: "searchbox", hidden: "" };
    const IMPLICIT = {
      A: (e) => e.hasAttribute("href") ? "link" : "", AREA: (e) => e.hasAttribute("href") ? "link" : "", BUTTON: () => "button", SUMMARY: () => "button",
      SELECT: (e) => (e.multiple || e.size > 1) ? "listbox" : "combobox", TEXTAREA: () => "textbox", OPTION: () => "option",
      INPUT: (e) => { const t = (e.getAttribute("type") || "text").toLowerCase(); return t in INPUT_ROLES ? INPUT_ROLES[t] : "textbox"; },
      H1: () => "heading", H2: () => "heading", H3: () => "heading", H4: () => "heading", H5: () => "heading", H6: () => "heading",
      IMG: (e) => e.getAttribute("alt") === "" ? "presentation" : "img", NAV: () => "navigation", MAIN: () => "main", FORM: () => "form", DIALOG: () => "dialog",
      UL: () => "list", OL: () => "list", LI: () => "listitem", TABLE: () => "table", TR: () => "row", TD: () => "cell", TH: () => "columnheader",
      ARTICLE: () => "article", ASIDE: () => "complementary", HEADER: () => "banner", FOOTER: () => "contentinfo", PROGRESS: () => "progressbar",
    };
    const roleOf = (e) => { const r = e.getAttribute("role"); if (r && r.trim()) return r.trim().split(/\s+/)[0].toLowerCase(); const f = IMPLICIT[e.tagName]; return f ? f(e) : ""; };
    const TEXT_NAMED = /^(button|link|heading|tab|menuitem|menuitemcheckbox|menuitemradio|option|checkbox|radio|switch|cell|columnheader|rowheader|listitem|treeitem|tooltip)$/;
    const nameOf = (e) => {
      const lb = e.getAttribute("aria-labelledby");
      if (lb) { const t = fold(lb.split(/\s+/).map((id) => { const x = document.getElementById(id); return x ? x.textContent : ""; }).join(" ")); if (t) return t; }
      const al = fold(e.getAttribute("aria-label")); if (al) return al;
      if (e.labels && e.labels.length) { const t = fold(Array.from(e.labels).map((l) => l.textContent).join(" ")); if (t) return t; }
      if (e.tagName === "INPUT" && /^(button|submit|reset)$/i.test(e.type)) return fold(e.value) || (e.type === "submit" ? "Submit" : e.type === "reset" ? "Reset" : "");
      if (e.tagName === "IMG" || e.tagName === "AREA" || (e.tagName === "INPUT" && e.type === "image")) { const a = fold(e.getAttribute("alt")); if (a) return a; }
      if (TEXT_NAMED.test(roleOf(e))) { const t = fold(e.textContent); if (t) return t.slice(0, 500); }
      const ph = fold(e.getAttribute("placeholder")); if (ph) return ph;
      return fold(e.getAttribute("title"));
    };
    const shown = (e) => {
      const r = e.getBoundingClientRect();
      if (r.width <= 0 && r.height <= 0) return false;
      const s = getComputedStyle(e);
      return s.visibility !== "hidden" && s.visibility !== "collapse" && s.display !== "none" && parseFloat(s.opacity || "1") > 0;
    };
    const TEXT_INPUTS = /^(text|search|email|url|tel|password|number|date|datetime-local|month|week|time)$/;
    const editable = (e) => (e.tagName === "INPUT" && TEXT_INPUTS.test((e.getAttribute("type") || "text").toLowerCase())) || e.tagName === "TEXTAREA" || e.isContentEditable;
    const disabled = (e) => !!(e.disabled || e.getAttribute("aria-disabled") === "true" || e.closest("[inert]") || e.closest("fieldset[disabled]"));
    const find = (loc) => {
      let pool;
      if (loc.selector) { try { pool = Array.from(document.querySelectorAll(loc.selector)); } catch (err) { fail("bad", "selector: " + (err && err.message || err)); } }
      else pool = Array.from(document.querySelectorAll("body *"));
      const role = loc.role ? low(loc.role) : "", want = loc.name != null ? low(loc.name) : null, text = loc.text != null ? low(loc.text) : null;
      let hits = pool;
      if (role) hits = hits.filter((e) => roleOf(e) === role);
      else if (want !== null && !loc.selector) hits = hits.filter((e) => roleOf(e) || e.hasAttribute("aria-label") || e.hasAttribute("aria-labelledby") || e.hasAttribute("placeholder") || e.hasAttribute("title"));
      if (text) {
        hits = hits.filter((e) => low(e.textContent).includes(text));
        if (!role && !loc.selector && want === null) hits = hits.filter((e) => !hits.some((o) => o !== e && e.contains(o)));
      }
      if (want !== null) {
        const named = hits.map((e) => [e, low(nameOf(e))]);
        const exact = named.filter((p) => p[1] === want);
        hits = (exact.length ? exact : named.filter((p) => want && p[1].includes(want))).map((p) => p[0]);
      }
      const on = hits.filter(shown);
      return on.concat(hits.filter((e) => !on.includes(e)));
    };
    const describe = (e) => {
      const r = e.getBoundingClientRect();
      const out = { tag: e.tagName.toLowerCase(), role: roleOf(e), name: nameOf(e).slice(0, 500), text: fold(e.innerText || e.textContent).slice(0, req.text_max),
        disabled: disabled(e), editable: editable(e), visible: shown(e), rect: { x: r.x, y: r.y, width: r.width, height: r.height } };
      if (e.tagName === "INPUT" && (e.type === "password")) { out.secret = true; }
      else if (e.tagName === "INPUT" || e.tagName === "TEXTAREA" || e.tagName === "SELECT") out.value = String(e.value).slice(0, req.text_max);
      else if (e.isContentEditable) out.value = fold(e.innerText).slice(0, req.text_max);
      if (e.tagName === "INPUT" && (e.type === "checkbox" || e.type === "radio")) out.checked = !!e.checked;
      else if (e.hasAttribute("aria-checked")) out.checked = e.getAttribute("aria-checked") === "true";
      if (e.href && typeof e.href === "string") out.href = e.href;
      return out;
    };
    const se = document.scrollingElement || document.documentElement;
    const scrollState = (box, moved) => ({ x: box.scrollLeft, y: box.scrollTop, max_x: Math.max(0, box.scrollWidth - box.clientWidth), max_y: Math.max(0, box.scrollHeight - box.clientHeight), moved: !!moved, inner: box !== se });
    // Jump, whatever the page's scroll-behavior says, so positions read back are where it ended.
    const jump = (box, f) => { const st = box.style, was = st.scrollBehavior; st.scrollBehavior = "auto"; try { f(); } finally { st.scrollBehavior = was; } };
    const usable = (e, fill) => {
      if (!e.isConnected) return "it left the page";
      if (!shown(e)) return "not visible";
      if (disabled(e)) return "disabled";
      if (fill && e.tagName !== "SELECT" && !editable(e)) return "not a field that takes text";
      if (fill && e.readOnly) return "read-only";
      jump(se, () => e.scrollIntoView({ block: "center", inline: "center" }));
      const r = e.getBoundingClientRect();
      const x = r.left + r.width / 2, y = r.top + r.height / 2;
      if (x < 0 || y < 0 || x > innerWidth || y > innerHeight) return "outside the view";
      const top = document.elementFromPoint(x, y);
      if (top && top !== e && !e.contains(top) && !(top.control === e) && !(top.closest && top.closest("label") && top.closest("label").control === e))
        return "covered by " + top.tagName.toLowerCase() + (top.id ? "#" + top.id : "");
      return null;
    };
    const out = { url: location.href, origin: location.origin, matches: 0 };
    if (req.op === "scrollBy") {
      const can = (b) => (req.dy > 0 && b.scrollTop + b.clientHeight < b.scrollHeight - 1) || (req.dy < 0 && b.scrollTop > 0) || (req.dx > 0 && b.scrollLeft + b.clientWidth < b.scrollWidth - 1) || (req.dx < 0 && b.scrollLeft > 0);
      let box = se;
      if (!can(se)) {
        for (let el = document.elementFromPoint(innerWidth / 2, innerHeight / 2); el && el !== se && el !== document.body; el = el.parentElement) {
          const s = getComputedStyle(el);
          if (/(auto|scroll|overlay)/.test(s.overflowY + " " + s.overflowX) && can(el)) { box = el; break; }
        }
      }
      const before = [box.scrollLeft, box.scrollTop];
      jump(box, () => box.scrollBy(req.dx, req.dy));
      out.scroll = scrollState(box, before[0] !== box.scrollLeft || before[1] !== box.scrollTop);
      return "O" + JSON.stringify(out);
    }
    const hits = find(req.target);
    out.matches = hits.length;
    const e = hits[req.target.nth || 0];
    if (!e) fail("notfound");
    const before = [se.scrollLeft, se.scrollTop];
    if (req.op === "click") {
      const why = usable(e, false); if (why) fail("interactable", why);
      const r = e.getBoundingClientRect();
      const at = { bubbles: true, cancelable: true, composed: true, view: window, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, button: 0 };
      const P = typeof PointerEvent === "function" ? PointerEvent : MouseEvent;
      e.dispatchEvent(new P("pointerdown", Object.assign({ pointerType: "mouse", isPrimary: true, buttons: 1 }, at)));
      e.dispatchEvent(new MouseEvent("mousedown", Object.assign({ buttons: 1 }, at)));
      if (e.focus) e.focus({ preventScroll: true });
      e.dispatchEvent(new P("pointerup", Object.assign({ pointerType: "mouse", isPrimary: true }, at)));
      e.dispatchEvent(new MouseEvent("mouseup", at));
      e.click();
    } else if (req.op === "fill") {
      const why = usable(e, true); if (why) fail("interactable", why);
      const v = String(req.value);
      e.focus({ preventScroll: true });
      if (e.tagName === "SELECT") {
        const opts = Array.from(e.options);
        const o = opts.find((x) => x.value === v) || opts.find((x) => low(x.text) === low(v));
        if (!o) fail("interactable", "no option " + JSON.stringify(v));
        e.value = o.value;
        e.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
      } else if (e.isContentEditable) {
        const range = document.createRange(); range.selectNodeContents(e);
        const sel = getSelection(); sel.removeAllRanges(); sel.addRange(range);
        let ok = false; try { ok = document.execCommand("insertText", false, v); } catch (err) { ok = false; }
        if (!ok) { e.textContent = v; e.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "insertText", data: v })); }
      } else {
        const proto = e.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
        e.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, cancelable: true, composed: true, inputType: "insertReplacementText", data: v }));
        Object.getOwnPropertyDescriptor(proto, "value").set.call(e, v);
        e.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "insertReplacementText", data: v }));
      }
      e.dispatchEvent(new Event("change", { bubbles: true }));
    } else if (req.op === "focus") {
      if (!e.isConnected || disabled(e)) fail("interactable", disabled(e) ? "disabled" : "it left the page");
      e.focus({ preventScroll: true });
    } else if (req.op === "highlight" || req.op === "scrollTo") {
      jump(se, () => e.scrollIntoView({ block: "center", inline: "nearest" }));
      if (req.op === "highlight") {
        const st = e.style, was = [st.outline, st.outlineOffset];
        st.outline = "3px solid #f5a623"; st.outlineOffset = "2px";
        setTimeout(() => { st.outline = was[0]; st.outlineOffset = was[1]; }, req.ms);
      }
    } else if (req.op !== "read") fail("bad", "unknown action " + req.op);
    out.element = describe(e);
    out.scroll = scrollState(se, before[0] !== se.scrollLeft || before[1] !== se.scrollTop);
    return "O" + JSON.stringify(out);
  } catch (err) {
    if (err && err.morph) return "E" + err.morph + ":" + err.detail;
    return "Escript:" + String(err && err.message || err);
  }"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeBackend;
    use crate::{Backend, Spec};

    fn open(fake: &FakeBackend, url: &str) -> Box<dyn WebView> {
        fake.clone().create(Spec { url: url.into(), ..Spec::default() }).unwrap()
    }

    fn run(actions: &mut PageActions, view: &dyn WebView, pending: &Pending<ActionResult>) -> ActionResult {
        actions.tick(Instant::now(), |_| Some(view));
        pending.try_take().expect("answered")
    }

    fn ok(url: &str, extra: &str) -> String {
        format!(r#"O{{"url":"{url}","origin":"{}","matches":2{extra}}}"#, crate::bridge::origin_of(url))
    }

    #[test]
    fn requests_become_one_bounded_script_with_the_request_as_json() {
        let r = ActionRequest::new(Action::Fill { target: Locator::role("textbox", Some("Email")), value: "a\"b</script>\u{2028}".into() });
        let s = script(&r, "https://example.com", 7).unwrap();
        assert!(s.starts_with("/*morph-action:7*/"));
        let json = s.split("const req = ").nth(1).unwrap().split(";\n").next().unwrap();
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["op"], "fill");
        assert_eq!(v["target"]["role"], "textbox");
        assert_eq!(v["target"]["name"], "Email");
        assert_eq!(v["value"], "a\"b</script>\u{2028}");
        assert_eq!(v["origin"], "https://example.com");
        assert!(s.contains("window !== window.top"), "main frame only");
        let h = script(&ActionRequest::new(Action::Highlight { target: Locator::text("x"), duration: Duration::from_secs(600) }), "null", 1).unwrap();
        assert!(h.contains(r#""ms":30000"#), "capped at HIGHLIGHT_MAX");
    }

    #[test]
    fn bad_requests_are_refused_before_the_page_sees_them() {
        let empty = ActionRequest::new(Action::Click(Locator { selector: Some("  ".into()), ..Locator::default() }));
        assert!(matches!(script(&empty, "null", 1), Err(ActionError::BadRequest(_))));
        let far = ActionRequest::new(Action::ScrollBy { dx: f64::NAN, dy: 1.0 });
        assert!(matches!(script(&far, "null", 1), Err(ActionError::BadRequest(_))));
        let long = ActionRequest::new(Action::Fill { target: Locator::selector("input"), value: "x".repeat(FILL_MAX + 1) });
        assert!(matches!(script(&long, "null", 1), Err(ActionError::BadRequest(_))));
        assert!(script(&ActionRequest::new(Action::ScrollBy { dx: 0.0, dy: 400.0 }), "null", 1).is_ok(), "scrolling needs no element");
    }

    #[test]
    fn answers_and_errors_come_back_typed() {
        let fake = FakeBackend::new();
        let view = open(&fake, "https://shop.test/cart");
        let mut actions = PageActions::new();
        fake.answer_page_actions(
            "https://shop.test/",
            &ok(
                "https://shop.test/cart",
                r#","element":{"tag":"input","role":"textbox","name":"Email","value":"a@b.test","editable":true,"visible":true},"scroll":{"y":120,"max_y":900,"moved":true}"#,
            ),
        );
        let p = actions.run(view.as_ref(), ActionRequest::new(Action::Fill { target: Locator::name("Email"), value: "a@b.test".into() }));
        let out = run(&mut actions, view.as_ref(), &p).unwrap();
        assert_eq!((out.matches, out.origin.as_str(), out.isolation), (2, "https://shop.test", Isolation::ScriptWorld));
        let el = out.element.unwrap();
        assert_eq!((el.tag.as_str(), el.value.as_deref(), el.secret), ("input", Some("a@b.test"), false));
        assert_eq!((out.scroll.y, out.scroll.moved), (120.0, true));
        let calls = fake.calls_for(view.id());
        assert!(
            calls.iter().any(|c| matches!(c, crate::fake::Call::EvaluateIsolated(s) if s.starts_with("/*morph-action:"))),
            "isolated, never evaluate()"
        );
        assert!(!calls.iter().any(|c| matches!(c, crate::fake::Call::Evaluate(_))));

        for (answer, want) in [
            ("Enotfound:", ActionError::NotFound),
            ("Einteractable:covered by div#cookie-banner", ActionError::NotInteractable("covered by div#cookie-banner".into())),
            ("Ebad:selector: bad", ActionError::BadRequest("selector: bad".into())),
            ("Eorigin:https://evil.test/", ActionError::Navigated { from: "https://shop.test/cart".into(), to: "https://evil.test/".into() }),
            ("Escript:boom", ActionError::Script("boom".into())),
        ] {
            fake.answer_page_actions("https://shop.test/", answer);
            let p = actions.run(view.as_ref(), ActionRequest::new(Action::Click(Locator::text("Buy"))));
            assert_eq!(run(&mut actions, view.as_ref(), &p), Err(want));
        }
    }

    #[test]
    fn a_page_claiming_another_origin_is_refused() {
        let fake = FakeBackend::new();
        let view = open(&fake, "https://evil.test/");
        fake.answer_page_actions("https://evil.test/", &ok("https://bank.test/", ""));
        let mut actions = PageActions::new();
        let p = actions.run(view.as_ref(), ActionRequest::new(Action::Read(Locator::selector("h1"))));
        assert!(matches!(run(&mut actions, view.as_ref(), &p), Err(ActionError::Navigated { .. })));
    }

    #[test]
    fn the_approved_origin_is_checked_before_anything_runs() {
        let fake = FakeBackend::new();
        let view = open(&fake, "https://elsewhere.test/");
        let mut actions = PageActions::new();
        let p = actions.run(view.as_ref(), ActionRequest::new(Action::Click(Locator::text("Pay"))).on_origin("https://shop.test"));
        assert_eq!(
            run(&mut actions, view.as_ref(), &p),
            Err(ActionError::WrongOrigin { expected: "https://shop.test".into(), actual: "https://elsewhere.test".into() })
        );
        assert!(!fake.calls_for(view.id()).iter().any(|c| matches!(c, crate::fake::Call::EvaluateIsolated(_))), "no script ran");
    }

    #[test]
    fn page_world_views_refuse_unless_allowed() {
        let fake = FakeBackend::new();
        fake.set_isolation(Isolation::PageWorld);
        let view = open(&fake, "https://shop.test/");
        fake.answer_page_actions("https://shop.test/", &ok("https://shop.test/", ""));
        let mut actions = PageActions::new();
        let p = actions.run(view.as_ref(), ActionRequest::new(Action::ScrollBy { dx: 0.0, dy: 100.0 }));
        assert_eq!(run(&mut actions, view.as_ref(), &p), Err(ActionError::BlockedByIsolation(Isolation::PageWorld)));
        let p = actions.run(view.as_ref(), ActionRequest { allow_page_world: true, ..ActionRequest::new(Action::ScrollBy { dx: 0.0, dy: 100.0 }) });
        assert_eq!(run(&mut actions, view.as_ref(), &p).unwrap().isolation, Isolation::PageWorld, "the answer says where it ran");
        fake.set_isolation(Isolation::None);
        let p = actions.run(view.as_ref(), ActionRequest { allow_page_world: true, ..ActionRequest::new(Action::ScrollBy { dx: 0.0, dy: 100.0 }) });
        assert_eq!(run(&mut actions, view.as_ref(), &p), Err(ActionError::Unsupported));
    }

    #[test]
    fn silent_pages_time_out_and_closed_views_end_their_actions() {
        let fake = FakeBackend::new();
        fake.hold_page_reads();
        let view = open(&fake, "https://slow.test/");
        let mut actions = PageActions::new();
        let p = actions
            .run(view.as_ref(), ActionRequest { timeout: Duration::from_millis(50), ..ActionRequest::new(Action::Click(Locator::text("Go"))) });
        actions.tick(Instant::now(), |_| Some(view.as_ref()));
        assert!(p.try_take().is_none());
        actions.tick(Instant::now() + Duration::from_secs(1), |_| Some(view.as_ref()));
        assert_eq!(p.try_take(), Some(Err(ActionError::Timeout)));
        let p = actions.run(view.as_ref(), ActionRequest::new(Action::Click(Locator::text("Go"))));
        actions.forget_view(view.id());
        actions.tick(Instant::now(), |_| None);
        assert_eq!(p.try_take(), Some(Err(ActionError::Gone)));
        assert_eq!(actions.pending(), 0);
    }

    #[test]
    fn the_page_channel_cannot_ask_for_actions() {
        let called = std::rc::Rc::new(std::cell::Cell::new(false));
        let c = called.clone();
        let bridge = crate::PageBridge::with_context(
            "",
            std::rc::Rc::new(move |_: &crate::BridgeContext, _: &str| {
                c.set(true);
                None
            }),
        );
        let view = crate::bridge::ViewBridge::new(ViewId(1), true, false);
        for kind in ["host.action", "host.click", "host.fill", "host.scroll", "host.read"] {
            let reply = bridge
                .answer_page(&view, "https://evil.test/", &format!(r##"{{"id":"1","type":"{kind}","payload":{{"selector":"#pay"}}}}"##))
                .unwrap();
            assert!(reply.contains("refused"), "{reply}");
        }
        assert!(!called.get(), "no handler saw them");
    }

    #[test]
    fn oversized_answers_are_refused() {
        let r = parse_answer(ViewId(1), Isolation::ScriptWorld, "https://a.test/", Some(&format!("O{}", "x".repeat(ANSWER_MAX))));
        assert!(matches!(r, Err(ActionError::Script(_))));
    }
}
