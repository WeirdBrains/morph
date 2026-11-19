//! Host-initiated page reads (docs/bridge-api.md): a host asks a view for
//! its title, text, a readable extract, links, the selection or a bounded
//! HTML snapshot and gets one correlated answer, or an error when the time
//! is up. The read runs in Morph's own script world where the engine has one
//! ([`Isolation::ScriptWorld`], WebKitGTK), so page scripts cannot change
//! what the read's JavaScript builtins do; elsewhere in the page's world.
//!
//! The answer is at most [`READ_MAX`] bytes (the same bound as
//! `morph_core::app_call::BODY_MAX`): the page side shortens the largest
//! parts until it fits and says `truncated`. It comes back in chunks of
//! [`CHUNK`] UTF-16 units, one script call each, so no single engine reply
//! is large.
//!
//! [`PageReads`] keeps the reads under way. The shell calls
//! [`PageReads::tick`] from its poll loop; answers and timeouts are handed
//! over there, to the read's callback or its [`Pending`] future.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{ViewId, WebView};

/// The most bytes (UTF-8 JSON) one read answers with.
pub const READ_MAX: usize = 8 * 1024 * 1024;

/// UTF-16 units per chunk of an answer.
pub const CHUNK: usize = 512 * 1024;

/// How long a read waits for its answer unless the request says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a view runs host scripts and reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    /// The backend can't run them.
    None,
    /// A script world of Morph's own (WebKitGTK): page scripts can't see or
    /// change it, though they share the DOM.
    ScriptWorld,
    /// The page's own world (WKWebView, WebView2 through wry): page scripts
    /// that ran first can change the builtins a read uses, so a hostile page
    /// can shape what the read says about it.
    PageWorld,
}

/// What a read asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Want {
    pub title: bool,
    /// `document.body.innerText`.
    pub text: bool,
    /// The main content as paragraphs ([`Readable`]).
    pub readable: bool,
    pub links: bool,
    pub selection: bool,
    /// `document.documentElement.outerHTML`, bounded.
    pub html: bool,
}

impl Want {
    pub const ALL: Want = Want { title: true, text: true, readable: true, links: true, selection: true, html: true };
}

/// One read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRequest {
    pub want: Want,
    /// At most this many bytes of answer; capped at [`READ_MAX`].
    pub max_bytes: usize,
    pub max_links: usize,
    pub timeout: Duration,
}

impl Default for ReadRequest {
    fn default() -> Self {
        Self {
            want: Want { title: true, text: true, readable: true, links: true, selection: true, html: false },
            max_bytes: READ_MAX,
            max_links: 500,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl ReadRequest {
    pub fn new(want: Want) -> Self {
        Self { want, ..Self::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Link {
    /// Absolute.
    pub href: String,
    /// Its text, whitespace folded, at most 200 characters.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Readable {
    pub title: String,
    pub byline: Option<String>,
    /// Headings and paragraphs of the main content, blank-line separated.
    pub text: String,
}

/// A read's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageData {
    pub view: ViewId,
    /// The view's address as the engine reports it when the answer came
    /// (the page's own `location.href` when the engine can't say).
    pub url: String,
    /// [`crate::bridge::origin_of`] `url`.
    pub origin: String,
    pub isolation: Option<Isolation>,
    pub title: Option<String>,
    pub text: Option<String>,
    pub readable: Option<Readable>,
    pub links: Option<Vec<Link>>,
    pub selection: Option<String>,
    pub html: Option<String>,
    /// Something was cut to fit `max_bytes` or `max_links`.
    pub truncated: bool,
    /// Bytes of the answer as it crossed the bridge.
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("this web view can't run page reads")]
    Unsupported,
    #[error("the web view is gone")]
    Gone,
    #[error("the page did not answer in time")]
    Timeout,
    #[error("the page moved from {from} to {to} during the read")]
    Navigated { from: String, to: String },
    #[error("the answer is larger than {0} bytes")]
    TooLarge(usize),
    #[error("page read failed: {0}")]
    Script(String),
}

/// Identifies a read for the life of its [`PageReads`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReadId(pub u64);

/// What a read's callback gets.
pub type ReadResult = Result<PageData, ReadError>;
pub type ReadDone = Box<dyn FnOnce(ReadResult)>;

/// A value that arrives later on this thread: a [`Future`], or
/// [`Pending::try_take`] from a poll loop.
pub struct Pending<T>(Rc<RefCell<Slot<T>>>);

struct Slot<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

/// Fills a [`Pending`].
pub struct Resolver<T>(Rc<RefCell<Slot<T>>>);

impl<T> Pending<T> {
    pub fn new() -> (Pending<T>, Resolver<T>) {
        let slot = Rc::new(RefCell::new(Slot { value: None, waker: None }));
        (Pending(slot.clone()), Resolver(slot))
    }

    /// The value, once, if it arrived.
    pub fn try_take(&self) -> Option<T> {
        self.0.borrow_mut().value.take()
    }

    pub fn is_ready(&self) -> bool {
        self.0.borrow().value.is_some()
    }
}

impl<T> Resolver<T> {
    pub fn resolve(self, value: T) {
        let waker = {
            let mut slot = self.0.borrow_mut();
            slot.value = Some(value);
            slot.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    }
}

impl<T> Future for Pending<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut slot = self.0.borrow_mut();
        match slot.value.take() {
            Some(v) => Poll::Ready(v),
            None => {
                slot.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Script answers, from whatever thread the engine calls back on.
type Inbox = Arc<Mutex<Vec<(ReadId, Option<String>)>>>;

struct Live {
    view: ViewId,
    key: String,
    max_bytes: usize,
    deadline: Instant,
    chunks: Option<usize>,
    /// Answers taken in (the first included).
    taken: usize,
    got: String,
    started_url: Option<String>,
    isolation: Isolation,
    done: ReadDone,
}

/// The reads under way (one per shell window).
pub struct PageReads {
    next: u64,
    chunk: usize,
    inbox: Inbox,
    live: HashMap<ReadId, Live>,
    /// Reads that ended before they began (unsupported views).
    failed: Vec<(ReadDone, ReadError)>,
}

impl Default for PageReads {
    fn default() -> Self {
        Self::new()
    }
}

impl PageReads {
    pub fn new() -> Self {
        Self::with_chunk(CHUNK)
    }

    /// Reads whose answers come in chunks of `chunk` UTF-16 units (tests).
    pub fn with_chunk(chunk: usize) -> Self {
        Self { next: 0, chunk: chunk.max(1), inbox: Arc::default(), live: HashMap::new(), failed: Vec::new() }
    }

    /// Reads not answered yet.
    pub fn pending(&self) -> usize {
        self.live.len() + self.failed.len()
    }

    /// Start reading `view`; `done` gets the answer from a later [`Self::tick`].
    pub fn start(&mut self, view: &dyn WebView, request: ReadRequest, done: ReadDone) -> ReadId {
        self.next += 1;
        let id = ReadId(self.next);
        let isolation = view.isolation();
        if isolation == Isolation::None {
            self.failed.push((done, ReadError::Unsupported));
            return id;
        }
        let key = format!("r{}_{}", self.next, &crate::bridge::new_token()[..8]);
        let max_bytes = request.max_bytes.clamp(1024, READ_MAX);
        let script = begin_script(&key, &request, max_bytes, self.chunk);
        let inbox = self.inbox.clone();
        let ok = view.evaluate_isolated(&script, Box::new(move |r| inbox.lock().map(|mut i| i.push((id, r))).unwrap_or(())));
        if !ok {
            self.failed.push((done, ReadError::Unsupported));
            return id;
        }
        self.live.insert(
            id,
            Live {
                view: view.id(),
                key,
                max_bytes,
                deadline: Instant::now() + request.timeout,
                chunks: None,
                taken: 0,
                got: String::new(),
                started_url: view.engine_url(),
                isolation,
                done,
            },
        );
        id
    }

    /// [`Self::start`], answered through a future.
    pub fn read(&mut self, view: &dyn WebView, request: ReadRequest) -> Pending<ReadResult> {
        let (pending, resolver) = Pending::new();
        self.start(view, request, Box::new(move |r| resolver.resolve(r)));
        pending
    }

    /// Fail every read of `view` (it was closed).
    pub fn forget_view(&mut self, view: ViewId) {
        let ids: Vec<ReadId> = self.live.iter().filter(|(_, l)| l.view == view).map(|(id, _)| *id).collect();
        for id in ids {
            if let Some(live) = self.live.remove(&id) {
                self.failed.push((live.done, ReadError::Gone));
            }
        }
    }

    /// Take in answers, ask for the next chunks, end reads that are done or
    /// out of time. `view` finds a view by id. Callbacks run here; they must
    /// not call back into this `PageReads` (use [`Self::read`]'s future to
    /// chain reads).
    pub fn tick<'a>(&mut self, now: Instant, view: impl Fn(ViewId) -> Option<&'a dyn WebView>) {
        let mut ended: Vec<(ReadDone, ReadResult)> = self.failed.drain(..).map(|(d, e)| (d, Err(e))).collect();
        let answers = self.inbox.lock().map(|mut i| std::mem::take(&mut *i)).unwrap_or_default();
        for (id, answer) in answers {
            let Some(mut live) = self.live.remove(&id) else {
                continue;
            };
            let Some(v) = view(live.view) else {
                ended.push((live.done, Err(ReadError::Gone)));
                continue;
            };
            match Self::take_answer(&mut live, answer) {
                Step::More(k) => {
                    let inbox = self.inbox.clone();
                    if v.evaluate_isolated(&chunk_script(&live.key, k), Box::new(move |r| inbox.lock().map(|mut i| i.push((id, r))).unwrap_or(()))) {
                        self.live.insert(id, live);
                    } else {
                        ended.push((live.done, Err(ReadError::Gone)));
                    }
                }
                Step::Done(json) => {
                    let result = finish(&live, &json, v);
                    ended.push((live.done, result));
                }
                Step::Fail(e) => ended.push((live.done, Err(e))),
            }
        }
        let late: Vec<ReadId> = self.live.iter().filter(|(_, l)| now >= l.deadline).map(|(id, _)| *id).collect();
        for id in late {
            let live = self.live.remove(&id).expect("listed");
            if let Some(v) = view(live.view) {
                v.evaluate_isolated(&forget_script(&live.key), Box::new(|_| {}));
            }
            ended.push((live.done, Err(ReadError::Timeout)));
        }
        for (done, result) in ended {
            done(result);
        }
    }

    fn take_answer(live: &mut Live, answer: Option<String>) -> Step {
        let Some(answer) = answer else {
            return Step::Fail(ReadError::Script("the script did not run".into()));
        };
        if let Some(e) = answer.strip_prefix('E') {
            return Step::Fail(if e == "large" { ReadError::TooLarge(live.max_bytes) } else { ReadError::Script(e.to_string()) });
        }
        let data = match (live.chunks, answer.strip_prefix('B'), answer.strip_prefix('C')) {
            (None, Some(rest), _) => {
                let Some((n, first)) = rest.split_once('\n') else {
                    return Step::Fail(ReadError::Script("bad answer".into()));
                };
                let Ok(n) = n.parse::<usize>() else {
                    return Step::Fail(ReadError::Script("bad answer".into()));
                };
                live.chunks = Some(n.max(1));
                first
            }
            (Some(_), _, Some(rest)) => rest,
            _ => return Step::Fail(ReadError::Script("bad answer".into())),
        };
        live.got.push_str(data);
        // The page side keeps the answer under max_bytes; a world it can't
        // vouch for (the page's) may still send more.
        if live.got.len() > live.max_bytes + 4096 {
            return Step::Fail(ReadError::TooLarge(live.max_bytes));
        }
        live.taken += 1;
        let received = live.taken;
        match live.chunks {
            Some(n) if received < n => Step::More(received),
            _ => Step::Done(std::mem::take(&mut live.got)),
        }
    }
}

enum Step {
    More(usize),
    Done(String),
    Fail(ReadError),
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    url: String,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    frame: String,
    #[serde(default)]
    truncated: bool,
    title: Option<String>,
    text: Option<String>,
    readable: Option<Readable>,
    links: Option<Vec<Link>>,
    selection: Option<String>,
    html: Option<String>,
}

fn finish(live: &Live, json: &str, view: &dyn WebView) -> ReadResult {
    let raw: Raw = serde_json::from_str(json).map_err(|e| ReadError::Script(format!("bad answer: {e}")))?;
    if raw.frame != "main" {
        return Err(ReadError::Script("the read ran outside the main frame".into()));
    }
    let now = view.engine_url();
    let url = now.clone().unwrap_or_else(|| raw.url.clone());
    let origin = crate::bridge::origin_of(&url);
    // The engine's address is the truth; the page's own `location` must
    // agree with it, and the engine's must not have moved since the start.
    let page_origin = if raw.origin.is_empty() || raw.origin == "null" { crate::bridge::origin_of(&raw.url) } else { raw.origin.clone() };
    if page_origin != origin {
        return Err(ReadError::Navigated { from: raw.url, to: url });
    }
    if let (Some(from), Some(to)) = (&live.started_url, &now)
        && crate::bridge::origin_of(from) != crate::bridge::origin_of(to)
    {
        return Err(ReadError::Navigated { from: from.clone(), to: to.clone() });
    }
    Ok(PageData {
        view: live.view,
        url,
        origin,
        isolation: Some(live.isolation),
        title: raw.title,
        text: raw.text,
        readable: raw.readable,
        links: raw.links,
        selection: raw.selection,
        html: raw.html,
        truncated: raw.truncated,
        bytes: json.len(),
    })
}

/// The read's page side (see the module comment for the answer format:
/// `B<chunks>\n<first chunk>`, then `C<chunk>`, or `E<error>`).
pub fn begin_script(key: &str, request: &ReadRequest, max_bytes: usize, chunk: usize) -> String {
    let w = request.want;
    format!(
        "/*morph-read:begin:{key}:{chunk}*/(() => {{
  const want = {{ title: {}, text: {}, readable: {}, links: {}, selection: {}, html: {} }};
  const max = {max_bytes}, maxLinks = {max_links}, CHUNK = {chunk}, KEY = \"{key}\";
  {READ_BODY}
}})()",
        w.title,
        w.text,
        w.readable,
        w.links,
        w.selection,
        w.html,
        max_links = request.max_links,
    )
}

const READ_BODY: &str = r#"try {
  const out = { url: location.href, origin: location.origin, frame: window === window.top ? "main" : "sub", truncated: false };
  const clean = (s) => (s || "").replace(/\s+/g, " ").trim();
  const meta = (n) => { const m = document.querySelector('meta[name="' + n + '"], meta[property="' + n + '"]'); return m ? m.getAttribute("content") : null; };
  const readable = () => {
    let root = document.querySelector("article") || document.querySelector("main, [role=main]");
    if (!root) {
      let best = null, score = 0;
      for (const el of document.querySelectorAll("div, section, td")) {
        let n = 0;
        for (const p of el.children) if (p.tagName === "P") n += (p.textContent || "").length;
        if (n > score) { score = n; best = el; }
      }
      root = best || document.body;
    }
    if (!root) return { title: document.title || "", byline: meta("author"), text: "" };
    const skip = "script, style, noscript, nav, footer, aside, form, [aria-hidden=true]";
    const parts = [];
    for (const el of root.querySelectorAll("h1, h2, h3, h4, h5, h6, p, li, pre, blockquote, figcaption")) {
      if (el.closest(skip)) continue;
      if (el.parentElement && el.parentElement.closest("p, li, pre, blockquote")) continue;
      const t = el.tagName === "PRE" ? (el.textContent || "") : clean(el.innerText || el.textContent);
      if (t) parts.push(t);
    }
    if (!parts.length) parts.push(clean(root.innerText || root.textContent));
    const h1 = root.querySelector("h1");
    return { title: meta("og:title") || (h1 ? clean(h1.textContent) : "") || document.title || "", byline: meta("author"), text: parts.join("\n\n") };
  };
  if (want.title) out.title = document.title || "";
  if (want.text) out.text = document.body ? (document.body.innerText || document.body.textContent || "") : "";
  if (want.selection) out.selection = getSelection ? String(getSelection()) : "";
  if (want.links) {
    const seen = new Set();
    out.links = [];
    for (const a of document.querySelectorAll("a[href]")) {
      const href = a.href;
      if (!href || seen.has(href) || /^javascript:/i.test(href)) continue;
      if (out.links.length >= maxLinks) { out.truncated = true; break; }
      seen.add(href);
      out.links.push({ href: href, text: clean(a.innerText || a.textContent).slice(0, 200) });
    }
  }
  if (want.readable) out.readable = readable();
  if (want.html) out.html = document.documentElement ? document.documentElement.outerHTML : "";
  const enc = new TextEncoder();
  let s = JSON.stringify(out);
  for (let i = 0; i < 64 && enc.encode(s).length > max; i++) {
    out.truncated = true;
    const parts = [
      ["html", out.html ? out.html.length : 0],
      ["text", out.text ? out.text.length : 0],
      ["readable", out.readable ? out.readable.text.length : 0],
      ["selection", out.selection ? out.selection.length : 0],
      ["links", out.links ? JSON.stringify(out.links).length : 0],
    ].sort((a, b) => b[1] - a[1]);
    const [name, size] = parts[0];
    if (!size) break;
    const half = (t) => { let n = Math.floor(t.length / 2); if (n > 0 && /[\uD800-\uDBFF]/.test(t[n - 1])) n--; return t.slice(0, n); };
    if (name === "links") out.links = out.links.slice(0, Math.floor(out.links.length / 2));
    else if (name === "readable") out.readable.text = half(out.readable.text);
    else out[name] = half(out[name]);
    s = JSON.stringify(out);
  }
  if (enc.encode(s).length > max) return "Elarge";
  const chunks = [];
  for (let i = 0; i < s.length;) {
    let j = Math.min(s.length, i + CHUNK);
    if (j < s.length && /[\uD800-\uDBFF]/.test(s[j - 1])) j--;
    chunks.push(s.slice(i, j));
    i = j;
  }
  if (!chunks.length) chunks.push("");
  if (chunks.length > 1) {
    const all = globalThis.__morphReads || (Object.defineProperty(globalThis, "__morphReads", { value: Object.create(null) }), globalThis.__morphReads);
    all[KEY] = chunks;
  }
  return "B" + chunks.length + "\n" + chunks[0];
} catch (e) {
  return "E" + String(e && e.message || e);
}"#;

/// Chunk `k` of read `key`'s answer.
pub fn chunk_script(key: &str, k: usize) -> String {
    format!(
        "/*morph-read:chunk:{key}:{k}*/(() => {{ const all = globalThis.__morphReads; const c = all && all[\"{key}\"]; if (!c || {k} >= c.length) return \"Egone\"; const x = c[{k}]; if ({k} === c.length - 1) delete all[\"{key}\"]; return \"C\" + x; }})()"
    )
}

/// Drops read `key`'s chunks (it timed out).
pub fn forget_script(key: &str) -> String {
    format!("/*morph-read:forget:{key}*/(() => {{ const all = globalThis.__morphReads; if (all) delete all[\"{key}\"]; return \"\"; }})()")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeBackend;
    use crate::{Backend, Spec};

    fn page(url: &str, text: &str) -> String {
        serde_json::json!({
            "url": url, "origin": crate::bridge::origin_of(url), "frame": "main", "truncated": false,
            "title": "Example", "text": text,
            "links": [{"href": "https://example.com/a", "text": "A"}],
            "readable": {"title": "Example", "byline": null, "text": text},
            "selection": "",
        })
        .to_string()
    }

    fn open(fake: &FakeBackend, url: &str) -> Box<dyn WebView> {
        fake.clone().create(Spec { url: url.into(), background: true, ..Spec::default() }).unwrap()
    }

    /// Tick until `pending` resolves (at most `n` ticks).
    fn run(reads: &mut PageReads, view: &dyn WebView, pending: &Pending<ReadResult>, n: usize) -> ReadResult {
        for _ in 0..n {
            reads.tick(Instant::now(), |id| (id == view.id()).then_some(view));
            if let Some(r) = pending.try_take() {
                return r;
            }
        }
        panic!("no answer after {n} ticks");
    }

    #[test]
    fn a_read_comes_back_whole_from_its_chunks() {
        let fake = FakeBackend::new();
        let text = "word ".repeat(2000);
        fake.answer_page_reads("https://example.com/", &page("https://example.com/doc", &text));
        let view = open(&fake, "https://example.com/doc");
        let mut reads = PageReads::with_chunk(1000);
        let pending = reads.read(view.as_ref(), ReadRequest::default());
        let data = run(&mut reads, view.as_ref(), &pending, 50).unwrap();
        assert_eq!(data.text.as_deref(), Some(text.as_str()));
        assert_eq!(data.title.as_deref(), Some("Example"));
        assert_eq!(data.links.unwrap()[0].href, "https://example.com/a");
        assert_eq!(data.readable.unwrap().text, text);
        assert_eq!(data.url, "https://example.com/doc");
        assert_eq!(data.origin, "https://example.com");
        assert_eq!(data.isolation, Some(Isolation::ScriptWorld));
        let chunks = fake
            .calls_for(view.id())
            .iter()
            .filter(|c| matches!(c, crate::fake::Call::EvaluateIsolated(s) if s.starts_with("/*morph-read:chunk:")))
            .count();
        assert!(chunks >= 10, "{chunks} chunk calls");
        assert_eq!(reads.pending(), 0);
    }

    #[test]
    fn a_read_awaited_as_a_future_resolves_on_tick() {
        let fake = FakeBackend::new();
        fake.answer_page_reads("https://example.com/", &page("https://example.com/", "hi"));
        let view = open(&fake, "https://example.com/");
        let mut reads = PageReads::new();
        let mut pending = reads.read(view.as_ref(), ReadRequest::new(Want { title: true, ..Want::default() }));
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        assert!(Pin::new(&mut pending).poll(&mut cx).is_pending());
        reads.tick(Instant::now(), |id| (id == view.id()).then_some(view.as_ref()));
        match Pin::new(&mut pending).poll(&mut cx) {
            Poll::Ready(Ok(data)) => assert_eq!(data.title.as_deref(), Some("Example")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_page_that_does_not_answer_times_out() {
        let fake = FakeBackend::new();
        fake.hold_page_reads();
        let view = open(&fake, "https://example.com/");
        let mut reads = PageReads::new();
        let pending = reads.read(view.as_ref(), ReadRequest { timeout: Duration::from_millis(50), ..ReadRequest::default() });
        reads.tick(Instant::now(), |_| Some(view.as_ref()));
        assert!(pending.try_take().is_none());
        reads.tick(Instant::now() + Duration::from_secs(1), |_| Some(view.as_ref()));
        assert_eq!(pending.try_take(), Some(Err(ReadError::Timeout)));
        assert!(
            fake.calls_for(view.id()).iter().any(|c| matches!(c, crate::fake::Call::EvaluateIsolated(s) if s.starts_with("/*morph-read:forget:")))
        );
    }

    #[test]
    fn an_answer_from_another_origin_than_the_engines_is_refused() {
        let fake = FakeBackend::new();
        // The page claims to be the bank; the engine says it is elsewhere.
        fake.answer_page_reads("https://evil.test/", &page("https://bank.test/", "balance"));
        let view = open(&fake, "https://evil.test/");
        let mut reads = PageReads::new();
        let pending = reads.read(view.as_ref(), ReadRequest::default());
        assert!(matches!(run(&mut reads, view.as_ref(), &pending, 3), Err(ReadError::Navigated { .. })));
    }

    #[test]
    fn closed_views_end_their_reads() {
        let fake = FakeBackend::new();
        fake.hold_page_reads();
        let view = open(&fake, "https://example.com/");
        let mut reads = PageReads::new();
        let pending = reads.read(view.as_ref(), ReadRequest::default());
        reads.forget_view(view.id());
        reads.tick(Instant::now(), |_| None);
        assert_eq!(pending.try_take(), Some(Err(ReadError::Gone)));
    }

    #[test]
    fn answers_past_the_bound_are_refused() {
        let fake = FakeBackend::new();
        // A page-world read the page tampered with: it sends more than asked.
        fake.answer_page_reads("https://example.com/", &page("https://example.com/", &"x".repeat(20_000)));
        let view = open(&fake, "https://example.com/");
        let mut reads = PageReads::with_chunk(4096);
        let pending = reads.read(view.as_ref(), ReadRequest { max_bytes: 1024, ..ReadRequest::default() });
        assert_eq!(run(&mut reads, view.as_ref(), &pending, 20), Err(ReadError::TooLarge(1024)));
    }

    #[test]
    fn the_page_side_is_bounded_and_main_frame_only() {
        let r = ReadRequest { want: Want::ALL, max_bytes: 4096, ..ReadRequest::default() };
        let s = begin_script("k1", &r, 4096, 100);
        assert!(s.starts_with("/*morph-read:begin:k1:100*/"));
        assert!(s.contains("const max = 4096"));
        assert!(s.contains("html: true"));
        assert!(s.contains("window === window.top"));
        assert!(chunk_script("k1", 2).contains("c[2]"));
        assert_eq!(READ_MAX, 8 * 1024 * 1024);
    }
}
