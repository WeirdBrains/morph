//! Pages a host loads and reads out of sight (docs/bridge-api.md): a
//! companion's research, a summary of a link. Each is a background view
//! ([`Spec::background`]): never shown, its popups refused, its bridge
//! messages marked `background`. By default it has throwaway storage of its
//! own ([`HostSession::Ephemeral`]: signed out, nothing kept); a host can ask
//! for the window profile's instead ([`HostSession::Profile`], signed in).
//! Hosts open one, wait for it to load, read it
//! ([`morph_webview::page_read`]), act in it ([`morph_webview::page_action`])
//! and close it; tabs can be read and acted in the same way.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use morph_core::tabs::TabId;
use morph_webview::page_action::{ActionError, ActionRequest, ActionResult};
use morph_webview::page_read::{Pending, ReadResult, Resolver};
use morph_webview::{Event, ReadError, ReadRequest, Rect, Spec, Storage, ViewId, WebView};

use crate::Browser;

/// How long a host page may take to load unless the host says otherwise.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// The size host pages lay out at (they are never shown).
const SIZE: (f64, f64) = (1280.0, 800.0);

/// Identifies a host page for the life of its browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HostPageId(pub u64);

/// Whose cookies and site storage a host page has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostSession {
    /// Throwaway storage of the page's own: signed out everywhere, never
    /// sees the profile's cookies or storage, nothing written to disk, gone
    /// when the page closes. The default (logged-out research).
    #[default]
    Ephemeral,
    /// The window profile's: signed in wherever the user is (the per-task
    /// signed-in opt-in).
    Profile,
}

/// A host page to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPageRequest {
    pub url: String,
    pub session: HostSession,
    /// [`HostPageState::Failed`] when it hasn't loaded by then.
    pub load_timeout: Duration,
}

impl HostPageRequest {
    /// `url` with throwaway storage and [`LOAD_TIMEOUT`].
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), session: HostSession::Ephemeral, load_timeout: LOAD_TIMEOUT }
    }

    /// `url` with the window profile's storage (signed in).
    pub fn signed_in(url: impl Into<String>) -> Self {
        Self { session: HostSession::Profile, ..Self::new(url) }
    }

    pub fn timeout(self, load_timeout: Duration) -> Self {
        Self { load_timeout, ..self }
    }
}

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
    session: HostSession,
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
    /// Load `url` in a new host page with this window profile's storage
    /// (signed in): [`Self::open_host_page_with`] and
    /// [`HostPageRequest::signed_in`].
    pub fn open_host_page(&mut self, url: &str, load_timeout: Duration) -> morph_webview::Result<HostPageId> {
        self.open_host_page_with(HostPageRequest::signed_in(url).timeout(load_timeout))
    }

    /// Open a host page. [`HostSession::Ephemeral`] fails with
    /// [`morph_webview::Error::Unsupported`] where the engine has no
    /// throwaway storage (never falls back to the profile's).
    pub fn open_host_page_with(&mut self, request: HostPageRequest) -> morph_webview::Result<HostPageId> {
        let storage = match request.session {
            HostSession::Ephemeral => Storage::Ephemeral,
            HostSession::Profile => Storage::Profile,
        };
        let view = self.backend.create(Spec {
            url: request.url,
            data_dir: self.data_dir.clone(),
            bounds: Rect::new(0.0, 0.0, SIZE.0, SIZE.1),
            visible: false,
            session: None,
            background: true,
            storage,
        })?;
        let load_timeout = request.load_timeout;
        let session = request.session;
        let host = &mut self.host_pages;
        host.next += 1;
        let id = HostPageId(host.next);
        host.by_view.insert(view.id(), id);
        host.pages
            .insert(id, HostPage { view, session, state: HostPageState::Loading, deadline: Instant::now() + load_timeout, waiters: Vec::new() });
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

    /// Whose storage host page `id` has (`None`: no such page).
    pub fn host_page_session(&self, id: HostPageId) -> Option<HostSession> {
        self.host_pages.pages.get(&id).map(|p| p.session)
    }

    /// Run `request` in host page `id` (see [`morph_webview::page_action`]).
    pub fn host_page_action(&mut self, id: HostPageId, request: ActionRequest) -> Pending<ActionResult> {
        match self.host_pages.pages.get(&id) {
            Some(page) => self.actions.run(page.view.as_ref(), request),
            None => ready(Err(ActionError::Gone)),
        }
    }

    /// Run `request` in tab `tab`'s page, shown or not: click, fill, focus,
    /// read back, highlight or scroll. The caller has the user's approval;
    /// this only refuses what isn't safe to run (wrong origin, no script
    /// world unless allowed).
    pub fn tab_action(&mut self, tab: TabId, request: ActionRequest) -> Pending<ActionResult> {
        match self.pages.get(&tab) {
            Some(page) if page.cover.is_none() => self.actions.run(page.view.as_ref(), request),
            _ => ready(Err(ActionError::Gone)),
        }
    }

    /// [`Self::tab_action`] in the active tab.
    pub fn active_tab_action(&mut self, request: ActionRequest) -> Pending<ActionResult> {
        self.tab_action(self.tabs.active_id(), request)
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
        self.actions.forget_view(view);
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
        let find = |v: ViewId| host.view(v).or_else(|| by_view.get(&v).and_then(|t| pages.get(t)).map(|p| p.view.as_ref()));
        self.reads.tick(now, find);
        self.actions.tick(now, find);
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

    fn storage_of(fake: &FakeBackend, view: ViewId) -> Storage {
        match &fake.calls_for(view)[0] {
            Call::Create(spec) => spec.storage,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn host_pages_are_signed_out_unless_asked() {
        let (mut b, fake) = browser();
        let research = b.open_host_page_with(HostPageRequest::new("https://docs.test/")).unwrap();
        assert_eq!(storage_of(&fake, *fake.created().last().unwrap()), Storage::Ephemeral);
        assert_eq!(b.host_page_session(research), Some(HostSession::Ephemeral));
        let signed_in = b.open_host_page_with(HostPageRequest::signed_in("https://mail.test/")).unwrap();
        assert_eq!(storage_of(&fake, *fake.created().last().unwrap()), Storage::Profile);
        assert_eq!(b.host_page_session(signed_in), Some(HostSession::Profile));
        // The old call keeps what it did: the profile's storage.
        b.open_host_page("https://mail.test/", LOAD_TIMEOUT).unwrap();
        assert_eq!(storage_of(&fake, *fake.created().last().unwrap()), Storage::Profile);
        assert_eq!(HostPageRequest::new("x").session, HostSession::Ephemeral, "the default");
    }

    #[test]
    fn an_engine_without_throwaway_storage_refuses_rather_than_signing_in() {
        let (mut b, fake) = browser();
        fake.refuse_ephemeral();
        let before = fake.created().len();
        assert!(matches!(b.open_host_page_with(HostPageRequest::new("https://docs.test/")), Err(morph_webview::Error::Unsupported(_))));
        assert_eq!(fake.created().len(), before, "no view, and no fallback to the profile");
        assert!(b.host_pages().is_empty());
    }

    #[test]
    fn tabs_and_host_pages_take_actions() {
        use morph_webview::page_action::{Action, ActionError, ActionRequest, Locator};
        let (mut b, fake) = browser();
        fake.answer_page_actions(
            "https://example.com",
            r#"O{"url":"https://example.com/","origin":"https://example.com","matches":1,"element":{"tag":"button","name":"Go"}}"#,
        );
        let click = b.active_tab_action(ActionRequest::new(Action::Click(Locator::role("button", Some("Go")))).on_origin("https://example.com"));
        b.poll();
        let out = click.try_take().unwrap().unwrap();
        assert_eq!(out.element.unwrap().name, "Go");
        let wrong = b.active_tab_action(ActionRequest::new(Action::Click(Locator::text("Go"))).on_origin("https://bank.test"));
        b.poll();
        assert!(matches!(wrong.try_take(), Some(Err(ActionError::WrongOrigin { .. }))));

        fake.hold_page_reads();
        let id = b.open_host_page_with(HostPageRequest::new("https://docs.test/")).unwrap();
        let scroll = b.host_page_action(id, ActionRequest::new(Action::ScrollBy { dx: 0.0, dy: 600.0 }));
        assert!(b.close_host_page(id));
        b.poll();
        assert_eq!(scroll.try_take(), Some(Err(ActionError::Gone)));
        assert_eq!(b.host_page_action(id, ActionRequest::new(Action::ScrollBy { dx: 0.0, dy: 1.0 })).try_take(), Some(Err(ActionError::Gone)));
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
