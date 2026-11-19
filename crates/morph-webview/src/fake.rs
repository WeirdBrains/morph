//! An in-memory backend for tests: records every call and lets the test
//! emit events as if a page had sent them.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{Backend, Event, Frame, Input, MenuChoice, Rect, Result, Spec, ViewId, WebView};

/// A call a fake web view received.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Create(Spec),
    Load(String),
    Back,
    Forward,
    Reload,
    Stop,
    Bounds(Rect),
    Visible(bool),
    Focus,
    Find(String),
    FindNext(bool),
    FindClear,
    Terminate,
    Input(Input),
    OptionMenu(MenuChoice),
    ChooseFiles(Option<Vec<std::path::PathBuf>>),
    ClearData,
    /// A script run with [`WebView::evaluate`].
    Evaluate(String),
    /// A script run with [`WebView::evaluate_isolated`].
    EvaluateIsolated(String),
    Dropped,
}

#[derive(Debug, Default)]
struct Shared {
    next: u64,
    calls: Vec<(ViewId, Call)>,
    pending: Vec<(ViewId, Event)>,
    focus_host: usize,
    /// The shell draws the pages (an off-screen backend).
    draws: bool,
    keyboard: Option<ViewId>,
    frames: Vec<(ViewId, Frame)>,
    /// What [`WebView::evaluate`] answers in pages whose address starts with
    /// the key (else `None`).
    script_answers: Vec<(String, String)>,
    /// What page reads ([`crate::page_read`]) answer in pages whose address
    /// starts with the key: the page side's JSON.
    page_reads: Vec<(String, String)>,
    /// Page reads get no answer (to test timeouts).
    hold_reads: bool,
    /// Chunks of answers under way, by read key.
    read_chunks: std::collections::HashMap<String, Vec<String>>,
}

/// The fake backend. Clone it to keep a handle for assertions after handing
/// it to the code under test.
#[derive(Debug, Clone, Default)]
pub struct FakeBackend {
    shared: Rc<RefCell<Shared>>,
}

impl FakeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fake whose pages the shell draws, like the off-screen backend: it
    /// tracks which view has the keyboard and hands out frames given to
    /// [`FakeBackend::push_frame`].
    pub fn drawing() -> Self {
        let fake = Self::default();
        fake.shared.borrow_mut().draws = true;
        fake
    }

    /// The next frame `id` hands out.
    pub fn push_frame(&self, id: ViewId, frame: Frame) {
        self.shared.borrow_mut().frames.push((id, frame));
    }

    /// Input handed to `id`, oldest first.
    pub fn inputs(&self, id: ViewId) -> Vec<Input> {
        self.calls_for(id)
            .into_iter()
            .filter_map(|c| match c {
                Call::Input(i) => Some(i),
                _ => None,
            })
            .collect()
    }

    /// Every call so far, oldest first.
    pub fn calls(&self) -> Vec<(ViewId, Call)> {
        self.shared.borrow().calls.clone()
    }

    /// Calls made to `id`.
    pub fn calls_for(&self, id: ViewId) -> Vec<Call> {
        self.shared.borrow().calls.iter().filter(|(v, _)| *v == id).map(|(_, c)| c.clone()).collect()
    }

    /// The last bounds and visibility set on `id`.
    pub fn last_visible(&self, id: ViewId) -> Option<bool> {
        self.calls_for(id).into_iter().rev().find_map(|c| match c {
            Call::Visible(v) => Some(v),
            Call::Create(spec) => Some(spec.visible),
            _ => None,
        })
    }

    pub fn last_bounds(&self, id: ViewId) -> Option<Rect> {
        self.calls_for(id).into_iter().rev().find_map(|c| match c {
            Call::Bounds(b) => Some(b),
            Call::Create(spec) => Some(spec.bounds),
            _ => None,
        })
    }

    /// Views created so far, in order.
    pub fn created(&self) -> Vec<ViewId> {
        self.shared.borrow().calls.iter().filter(|(_, c)| matches!(c, Call::Create(_))).map(|(v, _)| *v).collect()
    }

    /// Scripts run in pages whose address starts with `url_prefix` answer
    /// `result` from now on.
    pub fn answer_scripts(&self, url_prefix: &str, result: &str) {
        self.shared.borrow_mut().script_answers.push((url_prefix.into(), result.into()));
    }

    /// Page reads in pages whose address starts with `url_prefix` answer
    /// `json` (what the page side would: `url`, `origin`, `frame`, and the
    /// parts asked for) from now on.
    pub fn answer_page_reads(&self, url_prefix: &str, json: &str) {
        self.shared.borrow_mut().page_reads.push((url_prefix.into(), json.into()));
    }

    /// Page reads get no answer from now on.
    pub fn hold_page_reads(&self) {
        self.shared.borrow_mut().hold_reads = true;
    }

    /// The page side of a page read, for `script` in a page at `url`.
    fn page_read(&self, url: &str, script: &str) -> Option<String> {
        let mut shared = self.shared.borrow_mut();
        if shared.hold_reads {
            return None;
        }
        let head = script.strip_prefix("/*morph-read:")?.split("*/").next()?.to_string();
        let parts: Vec<&str> = head.split(':').collect();
        match parts.as_slice() {
            ["begin", key, chunk] => {
                let chunk: usize = chunk.parse().ok()?;
                let Some(json) = shared.page_reads.iter().rev().find(|(p, _)| url.starts_with(p.as_str())).map(|(_, j)| j.clone()) else {
                    return Some("Eno page".into());
                };
                let units: Vec<u16> = json.encode_utf16().collect();
                let chunks: Vec<String> = units.chunks(chunk.max(1)).map(String::from_utf16_lossy).collect();
                let first = chunks.first().cloned().unwrap_or_default();
                let n = chunks.len().max(1);
                if n > 1 {
                    shared.read_chunks.insert(key.to_string(), chunks);
                }
                Some(format!("B{n}\n{first}"))
            }
            ["chunk", key, k] => {
                let k: usize = k.parse().ok()?;
                Some(shared.read_chunks.get(*key).and_then(|c| c.get(k)).map(|c| format!("C{c}")).unwrap_or_else(|| "Egone".into()))
            }
            ["forget", key] => {
                shared.read_chunks.remove(*key);
                Some(String::new())
            }
            _ => None,
        }
    }

    /// Scripts run in `id`, oldest first.
    pub fn scripts(&self, id: ViewId) -> Vec<String> {
        self.calls_for(id)
            .into_iter()
            .filter_map(|c| match c {
                Call::Evaluate(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    /// Queue `event` from `id` for the next poll.
    pub fn emit(&self, id: ViewId, event: Event) {
        self.shared.borrow_mut().pending.push((id, event));
    }

    pub fn focus_host_count(&self) -> usize {
        self.shared.borrow().focus_host
    }

    fn record(&self, id: ViewId, call: Call) {
        self.shared.borrow_mut().calls.push((id, call));
    }
}

impl Backend for FakeBackend {
    fn create(&mut self, spec: Spec) -> Result<Box<dyn WebView>> {
        let id = {
            let mut shared = self.shared.borrow_mut();
            shared.next += 1;
            ViewId(shared.next)
        };
        let url = spec.url.clone();
        self.record(id, Call::Create(spec));
        Ok(Box::new(FakeView { id, backend: self.clone(), url: RefCell::new(url) }))
    }

    fn poll(&mut self) -> Vec<(ViewId, Event)> {
        std::mem::take(&mut self.shared.borrow_mut().pending)
    }

    fn focus_host(&self) {
        let mut shared = self.shared.borrow_mut();
        shared.focus_host += 1;
        shared.keyboard = None;
    }

    fn draws_pages(&self) -> bool {
        self.shared.borrow().draws
    }

    fn keyboard(&self) -> Option<ViewId> {
        let shared = self.shared.borrow();
        if shared.draws { shared.keyboard } else { None }
    }
}

struct FakeView {
    id: ViewId,
    backend: FakeBackend,
    /// The last address it was given, as its "history" for
    /// [`WebView::session_state`].
    url: RefCell<String>,
}

impl WebView for FakeView {
    fn id(&self) -> ViewId {
        self.id
    }
    fn evaluate(&self, script: &str, done: crate::ScriptDone) -> bool {
        self.backend.record(self.id, Call::Evaluate(script.into()));
        let url = self.url.borrow().clone();
        let answer = self.backend.shared.borrow().script_answers.iter().rev().find(|(p, _)| url.starts_with(p.as_str())).map(|(_, r)| r.clone());
        done(answer);
        true
    }
    fn isolation(&self) -> crate::Isolation {
        crate::Isolation::ScriptWorld
    }
    fn evaluate_isolated(&self, script: &str, done: crate::ScriptDone) -> bool {
        self.backend.record(self.id, Call::EvaluateIsolated(script.into()));
        let url = self.url.borrow().clone();
        if let Some(answer) = self.backend.page_read(&url, script) {
            done(Some(answer));
        }
        true
    }
    fn engine_url(&self) -> Option<String> {
        Some(self.url.borrow().clone())
    }
    fn load_url(&self, url: &str) -> Result<()> {
        self.backend.record(self.id, Call::Load(url.into()));
        *self.url.borrow_mut() = url.into();
        Ok(())
    }
    fn back(&self) -> Result<()> {
        self.backend.record(self.id, Call::Back);
        Ok(())
    }
    fn forward(&self) -> Result<()> {
        self.backend.record(self.id, Call::Forward);
        Ok(())
    }
    fn reload(&self) -> Result<()> {
        self.backend.record(self.id, Call::Reload);
        Ok(())
    }
    fn stop(&self) -> Result<()> {
        self.backend.record(self.id, Call::Stop);
        Ok(())
    }
    fn set_bounds(&self, bounds: Rect) -> Result<()> {
        self.backend.record(self.id, Call::Bounds(bounds));
        Ok(())
    }
    fn set_visible(&self, visible: bool) -> Result<()> {
        self.backend.record(self.id, Call::Visible(visible));
        Ok(())
    }
    fn focus(&self) -> Result<()> {
        self.backend.record(self.id, Call::Focus);
        self.backend.shared.borrow_mut().keyboard = Some(self.id);
        Ok(())
    }
    fn find(&self, query: &str) -> Result<()> {
        self.backend.record(self.id, Call::Find(query.into()));
        Ok(())
    }
    fn find_next(&self, forward: bool) -> Result<()> {
        self.backend.record(self.id, Call::FindNext(forward));
        Ok(())
    }
    fn find_clear(&self) -> Result<()> {
        self.backend.record(self.id, Call::FindClear);
        Ok(())
    }
    fn terminate(&self) -> Result<()> {
        self.backend.record(self.id, Call::Terminate);
        Ok(())
    }
    fn take_frame(&self) -> Option<Frame> {
        let mut shared = self.backend.shared.borrow_mut();
        let at = shared.frames.iter().position(|(id, _)| *id == self.id)?;
        Some(shared.frames.remove(at).1)
    }
    fn input(&self, input: Input) {
        self.backend.record(self.id, Call::Input(input));
    }
    fn option_menu(&self, choice: MenuChoice) {
        self.backend.record(self.id, Call::OptionMenu(choice));
    }
    fn choose_files(&self, paths: Option<Vec<std::path::PathBuf>>) {
        self.backend.record(self.id, Call::ChooseFiles(paths));
    }
    fn session_state(&self) -> Option<Vec<u8>> {
        Some(format!("fake-session:{}", self.url.borrow()).into_bytes())
    }
    fn clear_data(&self) -> crate::Result<()> {
        self.backend.record(self.id, Call::ClearData);
        Ok(())
    }
}

impl Drop for FakeView {
    fn drop(&mut self) {
        self.backend.record(self.id, Call::Dropped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_calls_and_delivers_emitted_events_once() {
        let mut backend = FakeBackend::new();
        let handle = backend.clone();
        let view = backend.create(Spec { url: "about:blank".into(), visible: true, ..Spec::default() }).unwrap();
        view.load_url("https://example.com/").unwrap();
        view.set_visible(false).unwrap();
        handle.emit(view.id(), Event::TitleChanged("Example".into()));
        assert_eq!(backend.poll(), vec![(view.id(), Event::TitleChanged("Example".into()))]);
        assert!(backend.poll().is_empty());
        assert_eq!(handle.last_visible(view.id()), Some(false));
        let id = view.id();
        drop(view);
        assert_eq!(handle.calls_for(id).last(), Some(&Call::Dropped));
    }
}
