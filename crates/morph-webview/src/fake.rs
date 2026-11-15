//! An in-memory backend for tests: records every call and lets the test
//! emit events as if a page had sent them.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{Backend, Event, Rect, Result, Spec, ViewId, WebView};

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
    Dropped,
}

#[derive(Debug, Default)]
struct Shared {
    next: u64,
    calls: Vec<(ViewId, Call)>,
    pending: Vec<(ViewId, Event)>,
    focus_host: usize,
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
        self.record(id, Call::Create(spec));
        Ok(Box::new(FakeView { id, backend: self.clone() }))
    }

    fn poll(&mut self) -> Vec<(ViewId, Event)> {
        std::mem::take(&mut self.shared.borrow_mut().pending)
    }

    fn focus_host(&self) {
        self.shared.borrow_mut().focus_host += 1;
    }
}

struct FakeView {
    id: ViewId,
    backend: FakeBackend,
}

impl WebView for FakeView {
    fn id(&self) -> ViewId {
        self.id
    }
    fn load_url(&self, url: &str) -> Result<()> {
        self.backend.record(self.id, Call::Load(url.into()));
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
