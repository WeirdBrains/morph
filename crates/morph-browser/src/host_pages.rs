//! Pages a host loads and reads out of sight (docs/bridge-api.md): a
//! companion's research, a summary of a link. Each is a background view of
//! the window's profile ([`Spec::background`]): never shown, its popups
//! refused, its bridge messages marked `background`. Hosts open one, wait for
//! it to load, read it ([`morph_webview::page_read`]) and close it; tabs can
//! be read the same way.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use morph_core::tabs::TabId;
use morph_webview::page_read::{Pending, ReadResult, Resolver};
use morph_webview::{Event, ReadError, ReadRequest, Rect, Spec, ViewId, WebView};

use crate::Browser;

/// How long a host page may take to load unless the host says otherwise.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// The size host pages lay out at (they are never shown).
const SIZE: (f64, f64) = (1280.0, 800.0);

/// Identifies a host page for the life of its browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostPageId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPageState {
    Loading,
    /// The main frame finished loading (the page may still run scripts).
    Loaded,
    /// The load failed, timed out, or the page's process ended.
    Failed(String),
    /// The page was closed.
    Closed,
}

struct HostPage {
    view: Box<dyn WebView>,
    state: HostPageState,
    deadline: Instant,
    waiters: Vec<Resolver<HostPageState>>,
}

#[derive(Default)]
pub(crate) struct HostPages {
    next: u64,
    pages: HashMap<HostPageId, HostPage>,
    by_view: HashMap<ViewId, HostPageId>,
}

impl HostPages {
    pub(crate) fn view(&self, view: ViewId) -> Option<&dyn WebView> {
        self.by_view.get(&view).and_then(|id| self.pages.get(id)).map(|p| p.view.as_ref())
    }

    fn settle(page: &mut HostPage, state: HostPageState) {
        page.state = state.clone();
        for waiter in page.waiters.drain(..) {
            waiter.resolve(state.clone());
        }
    }

    pub(crate) fn clear(&mut self) {
        for (_, mut page) in self.pages.drain() {
            Self::settle(&mut page, HostPageState::Closed);
        }
        self.by_view.clear();
    }
}

/// A value already there.
fn ready<T>(value: T) -> Pending<T> {
    let (pending, resolver) = Pending::new();
    resolver.resolve(value);
    pending
}

impl Browser {
    /// Load `url` in a new host page of this window's profile.
    pub fn open_host_page(&mut self, url: &str, load_timeout: Duration) -> morph_webview::Result<HostPageId> {
        let view = self.backend.create(Spec {
            url: url.to_string(),
            data_dir: self.data_dir.clone(),
            bounds: Rect::new(0.0, 0.0, SIZE.0, SIZE.1),
            visible: false,
            session: None,
            background: true,
        })?;
        let host = &mut self.host_pages;
        host.next += 1;
        let id = HostPageId(host.next);
        host.by_view.insert(view.id(), id);
        host.pages.insert(id, HostPage { view, state: HostPageState::Loading, deadline: Instant::now() + load_timeout, waiters: Vec::new() });
        Ok(id)
    }

    pub fn host_page_state(&self, id: HostPageId) -> HostPageState {
        self.host_pages.pages.get(&id).map(|p| p.state.clone()).unwrap_or(HostPageState::Closed)
    }

    /// Resolves when host page `id` has loaded, failed or closed (at once if it already has).
    pub fn host_page_loaded(&mut self, id: HostPageId) -> Pending<HostPageState> {
        let Some(page) = self.host_pages.pages.get_mut(&id) else {
            return ready(HostPageState::Closed);
        };
        if page.state != HostPageState::Loading {
            return ready(page.state.clone());
        }
        let (pending, resolver) = Pending::new();
        page.waiters.push(resolver);
        pending
    }

    /// The host page's address as its engine reports it.
    pub fn host_page_url(&self, id: HostPageId) -> Option<String> {
        self.host_pages.pages.get(&id).and_then(|p| p.view.engine_url())
    }

    /// Read host page `id` (see [`morph_webview::page_read`]).
    pub fn read_host_page(&mut self, id: HostPageId, request: ReadRequest) -> Pending<ReadResult> {
        match self.host_pages.pages.get(&id) {
            Some(page) => self.reads.read(page.view.as_ref(), request),
            None => ready(Err(ReadError::Gone)),
        }
    }

    /// Read tab `tab`'s page, shown or not.
    pub fn read_tab(&mut self, tab: TabId, request: ReadRequest) -> Pending<ReadResult> {
        match self.pages.get(&tab) {
            Some(page) if page.cover.is_none() => self.reads.read(page.view.as_ref(), request),
            _ => ready(Err(ReadError::Gone)),
        }
    }

    /// Read the active tab's page.
    pub fn read_active_tab(&mut self, request: ReadRequest) -> Pending<ReadResult> {
        self.read_tab(self.tabs.active_id(), request)
    }

    /// Close host page `id`: its view goes, and its reads end with [`ReadError::Gone`].
    pub fn close_host_page(&mut self, id: HostPageId) -> bool {
        let Some(mut page) = self.host_pages.pages.remove(&id) else {
            return false;
        };
        let view = page.view.id();
        self.host_pages.by_view.remove(&view);
        self.reads.forget_view(view);
        HostPages::settle(&mut page, HostPageState::Closed);
        true
    }

    /// Host pages open now.
    pub fn host_pages(&self) -> Vec<HostPageId> {
        let mut ids: Vec<HostPageId> = self.host_pages.pages.keys().copied().collect();
        ids.sort();
        ids
    }

    /// An event of a host page's view. Whether `view` is one.
    pub(crate) fn host_page_event(&mut self, view: ViewId, event: &Event) -> bool {
        let Some(id) = self.host_pages.by_view.get(&view).copied() else {
            return false;
        };
        let Some(page) = self.host_pages.pages.get_mut(&id) else {
            return true;
        };
        if self.log_events {
            eprintln!("morph-event host={} {event:?}", id.0);
        }
        match event {
            Event::LoadStarted if page.state == HostPageState::Loaded => page.state = HostPageState::Loading,
            Event::LoadFinished => HostPages::settle(page, HostPageState::Loaded),
            Event::LoadFailed(e) => {
                let why = if e.message.is_empty() { format!("{:?}", e.kind) } else { e.message.clone() };
                HostPages::settle(page, HostPageState::Failed(why));
            }
            Event::Crashed => HostPages::settle(page, HostPageState::Failed("the page's process ended".into())),
            _ => {}
        }
        true
    }

    /// Time out slow loads and move page reads along.
    pub(crate) fn poll_host_pages(&mut self, now: Instant) {
        for page in self.host_pages.pages.values_mut() {
            if page.state == HostPageState::Loading && now >= page.deadline {
                HostPages::settle(page, HostPageState::Failed("the page did not load in time".into()));
            }
        }
        let (host, pages, by_view) = (&self.host_pages, &self.pages, &self.by_view);
        self.reads.tick(now, |v| host.view(v).or_else(|| by_view.get(&v).and_then(|t| pages.get(t)).map(|p| p.view.as_ref())));
    }
}

#[cfg(test)]
mod tests {
    use morph_webview::fake::{Call, FakeBackend};
    use morph_webview::{Want, page_read::Isolation};

    use super::*;
    use crate::Config;

    fn browser() -> (Browser, FakeBackend) {
        let fake = FakeBackend::new();
        let browser = Browser::new(Box::new(fake.clone()), Config { start_url: "https://example.com".into(), ..Config::default() }).unwrap();
        (browser, fake)
    }

    fn page_json(url: &str, title: &str) -> String {
        format!(r#"{{"url":"{url}","origin":"{}","frame":"main","truncated":false,"title":"{title}"}}"#, morph_webview::bridge::origin_of(url))
    }

    #[test]
    fn a_host_page_loads_out_of_sight_is_read_and_closed() {
        let (mut b, fake) = browser();
        fake.answer_page_reads("https://docs.test/", &page_json("https://docs.test/guide", "Guide"));
        let id = b.open_host_page("https://docs.test/guide", LOAD_TIMEOUT).unwrap();
        let view = *fake.created().last().unwrap();
        match &fake.calls_for(view)[0] {
            Call::Create(spec) => assert!(spec.background && !spec.visible, "{spec:?}"),
            other => panic!("{other:?}"),
        }
        let loaded = b.host_page_loaded(id);
        assert_eq!(b.host_page_state(id), HostPageState::Loading);
        fake.emit(view, Event::LoadStarted);
        fake.emit(view, Event::TitleChanged("Guide".into()));
        fake.emit(view, Event::LoadFinished);
        let polled = b.poll();
        assert!(!polled.changed, "nothing the user sees changed");
        assert_eq!(b.tabs().len(), 1, "no tab came of it");
        assert_eq!(loaded.try_take(), Some(HostPageState::Loaded));
        let read = b.read_host_page(id, ReadRequest::new(Want { title: true, ..Want::default() }));
        b.poll();
        let data = read.try_take().unwrap().unwrap();
        assert_eq!(data.title.as_deref(), Some("Guide"));
        assert_eq!(data.origin, "https://docs.test");
        assert_eq!(data.isolation, Some(Isolation::ScriptWorld));
        assert_eq!(b.host_page_url(id).as_deref(), Some("https://docs.test/guide"));
        assert!(b.close_host_page(id));
        assert_eq!(fake.calls_for(view).last(), Some(&Call::Dropped));
        assert_eq!(b.host_page_state(id), HostPageState::Closed);
        assert_eq!(b.read_host_page(id, ReadRequest::default()).try_take(), Some(Err(ReadError::Gone)));
        assert!(b.host_pages().is_empty());
    }

    #[test]
    fn host_pages_that_fail_or_stall_say_so() {
        let (mut b, fake) = browser();
        let failing = b.open_host_page("https://down.test/", LOAD_TIMEOUT).unwrap();
        let view = *fake.created().last().unwrap();
        let loaded = b.host_page_loaded(failing);
        fake.emit(
            view,
            Event::LoadFailed(morph_webview::LoadError {
                url: "https://down.test/".into(),
                kind: morph_webview::ErrorKind::Unreachable,
                message: String::new(),
            }),
        );
        b.poll();
        assert!(matches!(loaded.try_take(), Some(HostPageState::Failed(_))));
        let stalled = b.open_host_page("https://slow.test/", Duration::ZERO).unwrap();
        let loaded = b.host_page_loaded(stalled);
        b.poll();
        assert_eq!(loaded.try_take(), Some(HostPageState::Failed("the page did not load in time".into())));
    }

    #[test]
    fn tabs_can_be_read_too() {
        let (mut b, fake) = browser();
        fake.answer_page_reads("https://example.com", &page_json("https://example.com/", "Example"));
        let read = b.read_active_tab(ReadRequest::default());
        b.poll();
        assert_eq!(read.try_take().unwrap().unwrap().title.as_deref(), Some("Example"));
    }

    #[test]
    fn closing_the_window_closes_its_host_pages() {
        let (mut b, _fake) = browser();
        let id = b.open_host_page("https://docs.test/", LOAD_TIMEOUT).unwrap();
        let loaded = b.host_page_loaded(id);
        b.close_pages();
        assert_eq!(loaded.try_take(), Some(HostPageState::Closed));
    }

    #[test]
    fn reads_are_bounded_like_app_bodies() {
        assert_eq!(morph_webview::page_read::READ_MAX as u64, morph_core::app_call::BODY_MAX);
    }
}
