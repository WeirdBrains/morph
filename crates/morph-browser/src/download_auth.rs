//! The shell's half of resuming behind a JavaScript login
//! ([`downloads::AuthNeed`]): read the site's storage from a page of the
//! same profile and origin (a live tab, else a hidden view loading the
//! origin), pick a JWT ([`morph_core::download_auth::pick_jwt`]) and hand it
//! to the download; or have the page start the download again. Storage
//! values live only here, in memory, until the pick, and are never logged.
use std::collections::HashMap;
use std::sync::mpsc;

use morph_core::download_auth::{STORAGE_SCRIPT, StorageRead, origin_of, parse_storage, pick_jwt, retrigger_script};
use morph_core::download_sessions::BearerToken;
use morph_core::tabs::TabId;
use morph_webview::{Event, Rect, Spec, ViewId, WebView};

use crate::Browser;
use crate::downloads::AuthNeed;

/// The most pages one token search reads.
const MAX_READS: usize = 4;

/// A script's answer for download `download`.
pub(crate) struct Reply {
    download: u64,
    result: Option<String>,
}

/// One download's token search under way.
struct Search {
    origins: Vec<String>,
    captured: Option<String>,
    pending: usize,
    reads: Vec<StorageRead>,
}

/// A hidden view that loads an origin so its storage can be read.
struct Hidden {
    download: u64,
    view: Box<dyn WebView>,
    asked: bool,
}

/// Token searches, hidden views and the channel their scripts answer on.
pub(crate) struct AuthWork {
    tx: mpsc::Sender<Reply>,
    rx: mpsc::Receiver<Reply>,
    searches: HashMap<u64, Search>,
    hidden: HashMap<ViewId, Hidden>,
    /// The tab each engine download started in.
    pub(crate) tabs: HashMap<u64, TabId>,
}

impl Default for AuthWork {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, searches: HashMap::new(), hidden: HashMap::new(), tabs: HashMap::new() }
    }
}

fn now_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Browser {
    /// Ask `view` for its storage on behalf of `download`.
    fn read_storage(tx: &mpsc::Sender<Reply>, view: &dyn WebView, download: u64) -> bool {
        let tx = tx.clone();
        view.evaluate(
            STORAGE_SCRIPT,
            Box::new(move |result| {
                let _ = tx.send(Reply { download, result });
            }),
        )
    }

    /// Serve what refused resumes need, and take in script answers.
    /// Whether anything changed.
    pub(crate) fn poll_download_auth(&mut self) -> bool {
        let mut changed = false;
        while let Some(need) = self.downloads.take_auth_need() {
            changed = true;
            if self.log_events {
                eprintln!("morph-event tab=0 DownloadAuth({need:?})");
            }
            match need {
                AuthNeed::Token { id, origins, captured } => self.search_token(id, origins, captured),
                AuthNeed::Retrigger { id, engine, url } => self.retrigger(id, engine, &url),
            }
        }
        while let Ok(reply) = self.auth.rx.try_recv() {
            changed = true;
            self.take_reply(reply);
        }
        // Searches that timed out in the downloads (they went on without a token).
        let downloads = &self.downloads;
        self.auth.searches.retain(|id, _| downloads.awaiting_token(*id));
        let searches = &self.auth.searches;
        self.auth.hidden.retain(|_, h| searches.contains_key(&h.download));
        changed
    }

    fn search_token(&mut self, id: u64, origins: Vec<String>, captured: Option<String>) {
        let mut pending = 0;
        let tabs: Vec<TabId> =
            self.tabs.tabs().iter().filter(|t| origin_of(&t.url).is_some_and(|o| origins.contains(&o))).map(|t| t.id).take(MAX_READS).collect();
        for tab in tabs {
            if let Some(page) = self.pages.get(&tab)
                && Self::read_storage(&self.auth.tx, page.view.as_ref(), id)
            {
                pending += 1;
            }
        }
        if pending == 0 {
            // No page of the site is open: load its origin out of sight.
            let spec = Spec {
                url: format!("{}/", origins.first().cloned().unwrap_or_default()),
                data_dir: self.data_dir.clone(),
                bounds: Rect::new(0.0, 0.0, 1.0, 1.0),
                visible: false,
                session: None,
                background: true,
            };
            match self.backend.create(spec) {
                Ok(view) if !origins.is_empty() => {
                    self.auth.hidden.insert(view.id(), Hidden { download: id, view, asked: false });
                    pending = 1;
                }
                _ => {}
            }
        }
        if pending == 0 {
            self.downloads.token_found(id, None);
            return;
        }
        self.auth.searches.insert(id, Search { origins, captured, pending, reads: Vec::new() });
    }

    /// An event of a hidden view: once it loaded, read its storage.
    /// Whether `view` is one of them.
    pub(crate) fn hidden_view_event(&mut self, view: ViewId, event: &Event) -> bool {
        let Some(h) = self.auth.hidden.get_mut(&view) else {
            return false;
        };
        match event {
            Event::LoadFinished if !h.asked => {
                h.asked = true;
                let download = h.download;
                if !Self::read_storage(&self.auth.tx, h.view.as_ref(), download) {
                    let _ = self.auth.tx.send(Reply { download, result: None });
                }
            }
            Event::LoadFailed(_) | Event::Crashed if !h.asked => {
                h.asked = true;
                let _ = self.auth.tx.send(Reply { download: h.download, result: None });
            }
            _ => {}
        }
        true
    }

    fn take_reply(&mut self, reply: Reply) {
        let Some(search) = self.auth.searches.get_mut(&reply.download) else {
            return;
        };
        search.pending = search.pending.saturating_sub(1);
        // Only storage of the download's own origins counts.
        if let Some(read) = reply.result.as_deref().and_then(parse_storage)
            && origin_of(&read.origin).is_some_and(|o| search.origins.contains(&o))
        {
            search.reads.push(read);
        }
        if search.pending > 0 {
            return;
        }
        let search = self.auth.searches.remove(&reply.download).expect("just found");
        self.auth.hidden.retain(|_, h| h.download != reply.download);
        let token = choose(&search, now_s());
        if self.log_events {
            eprintln!("morph-event tab=0 DownloadToken({}, found: {})", reply.download, token.is_some());
        }
        self.downloads.token_found(reply.download, token);
    }

    /// Start download `id` again from its page (or a new tab).
    fn retrigger(&mut self, id: u64, engine: u64, url: &str) {
        let _ = id;
        if let Some(tab) = self.auth.tabs.get(&engine)
            && let Some(page) = self.pages.get(tab)
            && page.view.evaluate(&retrigger_script(url), Box::new(|_| {}))
        {
            return;
        }
        self.new_tab(url);
    }
}

/// The token from `search`'s reads: the captured one wherever it is, else
/// the first origin (Referer's first) with a usable one.
fn choose(search: &Search, now_s: i64) -> Option<BearerToken> {
    let mut first = None;
    for origin in &search.origins {
        let items: Vec<&(String, String)> =
            search.reads.iter().filter(|r| origin_of(&r.origin).as_ref() == Some(origin)).flat_map(|r| r.items.iter()).collect();
        if let Some(token) = pick_jwt(items.iter().copied(), search.captured.as_deref(), now_s) {
            if search.captured.as_deref() == Some(token.as_str()) {
                return Some(BearerToken { origin: origin.clone(), token });
            }
            first.get_or_insert(BearerToken { origin: origin.clone(), token });
        }
    }
    first
}
