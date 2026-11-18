//! Downloads as the shell shows them. The backend writes a download's
//! bytes to a staging file ([`morph_webview::download_staging`]) and
//! reports it; the shell asks where the file goes (a save dialog with the
//! name the server suggested) and the file moves there once both the bytes
//! and the answer are in. Cancelling the dialog stops the download.
//!
//! The downloads panel works on these too: pause stops the engine's
//! download and keeps the bytes so far; resume fetches the rest itself
//! ([`fetch`], an HTTP `Range` request, starting over when the server can't
//! resume). A click on a running download opens the file once it is saved.
//! The speed is smoothed so the time left doesn't jump around.
//!
//! Each download keeps the session it started with ([`DownloadContext`]:
//! cookies, Referer, User-Agent, the address after redirects, validators) in
//! the profile's encrypted library ([`SessionStore`]), so a resume works
//! behind a login. A resume asks the engine for fresh cookies first and falls
//! back to the saved ones; it sends `If-Range` so a file that changed on the
//! server starts over cleanly. The session goes when the download completes,
//! is cancelled or is cleared.

mod fetch;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use morph_core::download_sessions::{DownloadContext, SessionCookie, SessionStore, is_auth_header};
use morph_webview::{Download, DownloadChange};

/// How long a resume waits for the engine's fresh cookies before it goes
/// with the saved ones.
const COOKIE_WAIT: Duration = Duration::from_secs(2);

/// How a download stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    /// Bytes are coming (or in, waiting for the save dialog).
    Running,
    /// The user paused it; the bytes so far are kept.
    Paused,
    /// The file is at this path.
    Saved(PathBuf),
    /// It stopped: why.
    Failed(String),
    /// The user cancelled it (the save dialog or the panel).
    Cancelled,
}

/// Who moves a download's bytes.
#[derive(Debug, Clone)]
enum Transfer {
    /// The web engine.
    Engine,
    /// The engine was stopped for a pause; its last word is still coming.
    EngineStopping,
    /// Morph's own HTTP transfer (after a resume or a retry).
    Own { generation: u64, stop: Arc<AtomicU8> },
    /// A resume (or retry) asked the engine for fresh cookies and starts
    /// from `offset` once they come or [`COOKIE_WAIT`] is up.
    AwaitingCookies { since: Instant, offset: u64 },
    /// Nobody (paused, or all bytes in).
    Idle,
}

/// Speed over the last few seconds: the bytes that came in that window
/// over its length, so a burst (the first buffers, one fast second) moves
/// it a little and the time left doesn't jump around.
#[derive(Debug, Clone, Default)]
struct Rate {
    /// (when, bytes so far), oldest first, spanning about [`Self::WINDOW`].
    samples: VecDeque<(Instant, u64)>,
}

impl Rate {
    /// How far back the speed looks.
    const WINDOW: Duration = Duration::from_secs(5);
    /// How much it needs to have seen before it says anything.
    const LEAST: Duration = Duration::from_secs(1);

    fn sample(&mut self, now: Instant, received: u64) {
        if self.samples.back().is_some_and(|(_, r)| received < *r) {
            self.samples.clear();
        }
        self.samples.push_back((now, received));
        // Keep one sample at or before the window's start.
        while self.samples.len() > 2 && self.samples.get(1).is_some_and(|(at, _)| now.saturating_duration_since(*at) >= Self::WINDOW) {
            self.samples.pop_front();
        }
    }

    fn speed(&self) -> Option<f64> {
        let (first, last) = (self.samples.front()?, self.samples.back()?);
        let dt = last.0.saturating_duration_since(first.0);
        (dt >= Self::LEAST).then(|| last.1.saturating_sub(first.1) as f64 / dt.as_secs_f64())
    }

    /// The bytes start again from a new place (pause, resume, restart).
    fn restart(&mut self) {
        self.samples.clear();
    }
}

/// One download.
#[derive(Debug, Clone)]
pub struct DownloadItem {
    pub id: u64,
    /// The file name the server suggested.
    pub name: String,
    pub url: String,
    pub received: u64,
    pub total: Option<u64>,
    pub state: DownloadState,
    /// Open the file once it is saved (a click while it runs).
    pub open_when_done: bool,
    /// The server could not resume: the last resume started over.
    pub restarted: bool,
    staged: PathBuf,
    /// Where the user wants it, once the dialog answered.
    target: Option<PathBuf>,
    /// All bytes are in `staged`.
    complete: bool,
    rate: Rate,
    transfer: Transfer,
    /// Its row in the [`SessionStore`].
    session_key: String,
    /// The session it started with (fresh cookies replace the saved ones).
    context: DownloadContext,
}

impl PartialEq for DownloadItem {
    fn eq(&self, other: &Self) -> bool {
        (self.id, &self.name, &self.url, self.received, self.total, &self.state, self.open_when_done)
            == (other.id, &other.name, &other.url, other.received, other.total, &other.state, other.open_when_done)
    }
}

impl DownloadItem {
    /// Progress from 0 to 1, when the size is known.
    pub fn fraction(&self) -> Option<f32> {
        match self.total {
            Some(total) if total > 0 => Some((self.received as f64 / total as f64).clamp(0.0, 1.0) as f32),
            _ => None,
        }
    }

    /// Whether the user still waits on it (bytes or the move).
    pub fn running(&self) -> bool {
        self.state == DownloadState::Running
    }

    /// All bytes are in (it may still wait for the save dialog).
    pub fn bytes_in(&self) -> bool {
        self.complete
    }

    /// Running or paused: not over yet.
    pub fn active(&self) -> bool {
        matches!(self.state, DownloadState::Running | DownloadState::Paused)
    }

    /// The folder its save dialog opens in.
    pub fn folder(&self) -> PathBuf {
        self.staged.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// Bytes per second, smoothed, while it runs.
    pub fn speed(&self) -> Option<f64> {
        self.rate.speed().filter(|_| self.running() && !self.complete)
    }

    /// The time left at the smoothed speed: only while it runs with a
    /// known size and a measured speed.
    pub fn eta(&self) -> Option<Duration> {
        let total = self.total?;
        let speed = self.speed().filter(|s| *s >= 1.0)?;
        let left = total.saturating_sub(self.received) as f64 / speed;
        Some(Duration::from_secs_f64(left.min(99.0 * 86_400.0)))
    }

    /// The tile's label while it waits to open: `Opens in 12s`, or
    /// `Opens when done` without an estimate.
    pub fn open_label(&self) -> Option<String> {
        if !self.open_when_done || !self.active() {
            return None;
        }
        Some(match self.eta() {
            Some(eta) => format!("Opens in {}", eta_text(eta)),
            None => "Opens when done".into(),
        })
    }

    /// The tile's status line: progress, speed and time left while it
    /// runs, `Paused`, where it went, or why it failed.
    pub fn status_text(&self) -> String {
        let amount = match self.total {
            Some(total) => format!("{} of {}", size_text(self.received), size_text(total)),
            None => size_text(self.received),
        };
        match &self.state {
            DownloadState::Running if self.complete => "Waiting for a place to save".into(),
            DownloadState::Running => {
                let mut parts = vec![amount];
                if let Some(speed) = self.speed() {
                    parts.push(format!("{}/s", size_text(speed as u64)));
                }
                match self.eta() {
                    Some(eta) => parts.push(format!("{} left", eta_text(eta))),
                    None if self.total.is_some() => parts.push("Estimating…".into()),
                    None => {}
                }
                if self.restarted {
                    parts.push("server can't resume, started over".into());
                }
                parts.join(" · ")
            }
            DownloadState::Paused => format!("Paused · {amount}"),
            DownloadState::Saved(path) => match std::fs::metadata(path) {
                Ok(meta) => format!("Done · {}", size_text(meta.len())),
                Err(_) => "Moved or deleted".into(),
            },
            DownloadState::Failed(why) => format!("Failed: {why}"),
            DownloadState::Cancelled => "Cancelled".into(),
        }
    }

    /// Where the file is (saved) or will be.
    pub fn path(&self) -> Option<&Path> {
        match &self.state {
            DownloadState::Saved(path) => Some(path),
            _ => self.target.as_deref(),
        }
    }
}

/// A byte count for people: `512 B`, `4.5 MB`.
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// A time left for people: `12s`, `3m` (rounded up), `1h 5m`.
pub fn eta_text(left: Duration) -> String {
    let secs = left.as_secs_f64().ceil() as u64;
    match secs {
        0..60 => format!("{}s", secs.max(1)),
        60..3600 => format!("{}m", secs.div_ceil(60)),
        _ => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// What a change did, for the shell's event log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Saved(PathBuf),
    Failed(String),
}

/// Every download of the window, newest last.
#[derive(Debug)]
pub struct Downloads {
    items: Vec<DownloadItem>,
    /// Downloads whose save dialog has not been shown yet.
    asks: VecDeque<u64>,
    /// The downloads folder the settings name, where save dialogs open.
    folder: Option<PathBuf>,
    /// Saved files to open now (they were clicked while running).
    opens: VecDeque<PathBuf>,
    /// News from Morph's own transfers.
    fetch_tx: Sender<fetch::Msg>,
    fetch_rx: Receiver<fetch::Msg>,
    generation: u64,
    /// Where sessions are kept (`None`: in memory only).
    sessions: Option<Arc<dyn SessionStore>>,
}

/// A download's row in the session store: unique across runs, so a row a
/// crash left behind is never mistaken for a new download's.
fn session_key(id: u64) -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("dl-{}-{nanos}-{id}", std::process::id())
}

impl Default for Downloads {
    fn default() -> Self {
        let (fetch_tx, fetch_rx) = channel();
        Self { items: Vec::new(), asks: VecDeque::new(), folder: None, opens: VecDeque::new(), fetch_tx, fetch_rx, generation: 0, sessions: None }
    }
}

/// Where a paused download's bytes wait (the engine deletes its own file
/// when it stops).
fn kept_path(staged: &Path) -> PathBuf {
    let mut name = staged.as_os_str().to_owned();
    name.push(".paused");
    PathBuf::from(name)
}

/// The engine's file for `staged` so far: `staged` itself or a file it
/// writes beside it first (`….wkdownload`, `….crdownload`), the biggest.
fn partial_file(staged: &Path) -> Option<PathBuf> {
    let dir = staged.parent()?;
    let name = staged.file_name()?.to_string_lossy().into_owned();
    let kept = kept_path(staged);
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&name)) && *p != kept && p.is_file())
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
}

impl Downloads {
    pub fn items(&self) -> &[DownloadItem] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<&DownloadItem> {
        self.items.iter().find(|d| d.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut DownloadItem> {
        self.items.iter_mut().find(|d| d.id == id)
    }

    /// Keep sessions in `store` from now on. Rows left by an earlier run go
    /// (downloads don't outlive the process).
    pub fn set_sessions(&mut self, store: Arc<dyn SessionStore>) {
        store.clear();
        for d in &self.items {
            if d.active() {
                store.save(&d.session_key, &d.context);
            }
        }
        self.sessions = Some(store);
    }

    /// The URL a resume of `id` fetches and its cookies are asked for: where
    /// the bytes came from after redirects.
    pub fn fetch_url(&self, id: u64) -> Option<String> {
        self.get(id).map(|d| if d.context.final_url.is_empty() { d.url.clone() } else { d.context.final_url.clone() })
    }

    /// Whether `id` waits for the engine's cookies to resume.
    pub fn awaiting_cookies(&self, id: u64) -> bool {
        self.get(id).is_some_and(|d| matches!(d.transfer, Transfer::AwaitingCookies { .. }))
    }

    /// The engine answered which cookies it sends for `id` now (`None`: it
    /// could not say). Fresh cookies replace the saved ones; a waiting resume
    /// starts, with the saved session when no fresh cookies came.
    pub fn cookies_answered(&mut self, id: u64, cookies: Option<Vec<morph_webview::Cookie>>) {
        let sessions = self.sessions.clone();
        let Some(d) = self.get_mut(id) else {
            return;
        };
        let fresh = cookies.filter(|c| !c.is_empty()).map(|list| {
            list.into_iter()
                .map(|c| SessionCookie { name: c.name, value: c.value, domain: c.domain, path: c.path, secure: c.secure })
                .collect::<Vec<_>>()
        });
        let got_fresh = fresh.is_some();
        if let Some(cookies) = fresh {
            d.context.cookies = cookies;
            if d.active()
                && let Some(s) = &sessions
            {
                s.save(&d.session_key, &d.context);
            }
        }
        if let Transfer::AwaitingCookies { offset, .. } = d.transfer {
            self.start_waiting(id, offset, got_fresh);
        }
    }

    /// Start waiting resumes whose cookies took too long, with the saved session.
    fn poll_waits(&mut self, now: Instant) {
        let late: Vec<(u64, u64)> = self
            .items
            .iter()
            .filter_map(|d| match d.transfer {
                Transfer::AwaitingCookies { since, offset } if now.duration_since(since) >= COOKIE_WAIT => Some((d.id, offset)),
                _ => None,
            })
            .collect();
        for (id, offset) in late {
            self.start_waiting(id, offset, false);
        }
    }

    /// Start `id`'s own transfer from `offset`: with the session in memory
    /// when it has fresh cookies, else with the saved one.
    fn start_waiting(&mut self, id: u64, offset: u64, fresh: bool) {
        self.generation += 1;
        let generation = self.generation;
        let tx = self.fetch_tx.clone();
        let sessions = self.sessions.clone();
        let Some(d) = self.items.iter_mut().find(|d| d.id == id) else {
            return;
        };
        if d.state != DownloadState::Running {
            d.transfer = Transfer::Idle;
            return;
        }
        if !fresh && let Some(saved) = sessions.as_ref().and_then(|s| s.load(&d.session_key)) {
            d.context.cookies = saved.cookies;
        }
        Self::start_own(d, generation, offset, tx);
    }

    /// The downloads folder from the settings (`None`: the system's).
    pub fn set_folder(&mut self, folder: Option<PathBuf>) {
        self.folder = folder;
    }

    /// The next download to ask a save path for: its id, name and the
    /// folder to open in (the settings' downloads folder, else where the
    /// bytes are staged).
    pub fn take_ask(&mut self) -> Option<(u64, String, PathBuf)> {
        let id = self.asks.pop_front()?;
        let folder = self.folder.clone();
        self.get(id).map(|d| (d.id, d.name.clone(), folder.unwrap_or_else(|| d.folder())))
    }

    /// A saved file to open now (it was clicked while it ran).
    pub fn take_open(&mut self) -> Option<PathBuf> {
        self.opens.pop_front()
    }

    /// How many downloads run, and their combined progress when every
    /// size is known (for the toolbar button). `None`: nothing runs.
    pub fn progress(&self) -> Option<(usize, Option<f32>)> {
        let running: Vec<&DownloadItem> = self.items.iter().filter(|d| d.running()).collect();
        if running.is_empty() {
            return None;
        }
        let total: Option<u64> = running.iter().map(|d| d.total).sum();
        let received: u64 = running.iter().map(|d| d.received).sum();
        let fraction = total.filter(|t| *t > 0).map(|t| (received as f64 / t as f64).clamp(0.0, 1.0) as f32);
        Some((running.len(), fraction))
    }

    /// The backend reported `event`.
    pub fn apply(&mut self, event: Download) -> Outcome {
        self.apply_at(event, Instant::now())
    }

    /// [`Self::apply`] at time `now` (the speed's clock).
    pub fn apply_at(&mut self, event: Download, now: Instant) -> Outcome {
        let id = event.id;
        match event.change {
            DownloadChange::Started { url, name, staged } => {
                self.items.retain(|d| d.id != id);
                self.items.push(DownloadItem {
                    id,
                    name,
                    url,
                    received: 0,
                    total: None,
                    state: DownloadState::Running,
                    open_when_done: false,
                    restarted: false,
                    staged,
                    target: None,
                    complete: false,
                    rate: Rate::default(),
                    transfer: Transfer::Engine,
                    session_key: session_key(id),
                    context: DownloadContext::default(),
                });
                self.asks.push_back(id);
                Outcome::Nothing
            }
            DownloadChange::Context(engine) => {
                let sessions = self.sessions.clone();
                let Some(d) = self.get_mut(id) else {
                    return Outcome::Nothing;
                };
                let c = &mut d.context;
                c.final_url = engine.final_url;
                c.referer = engine.referer.or(c.referer.take());
                c.user_agent = engine.user_agent.or(c.user_agent.take());
                c.auth_headers = engine.headers.into_iter().filter(|(n, _)| is_auth_header(n)).collect();
                c.etag = engine.etag;
                c.last_modified = engine.last_modified;
                if let Some(s) = sessions {
                    s.save(&d.session_key, &d.context);
                }
                Outcome::Nothing
            }
            DownloadChange::Progress { received, total } => {
                if let Some(d) = self.get_mut(id)
                    && matches!(d.transfer, Transfer::Engine)
                {
                    d.received = received;
                    d.total = total.or(d.total);
                    d.rate.sample(now, received);
                }
                Outcome::Nothing
            }
            DownloadChange::Finished => {
                let Some(d) = self.items.iter_mut().find(|d| d.id == id) else {
                    return Outcome::Nothing;
                };
                match d.transfer {
                    Transfer::Engine => {}
                    // It finished before the pause reached the engine: the
                    // bytes are all in after all.
                    Transfer::EngineStopping => {
                        d.complete = true;
                        d.transfer = Transfer::Idle;
                        if d.staged.exists() {
                            let _ = std::fs::remove_file(kept_path(&d.staged));
                        }
                        return Outcome::Nothing;
                    }
                    _ => return Outcome::Nothing,
                }
                d.complete = true;
                d.transfer = Transfer::Idle;
                if d.state == DownloadState::Cancelled {
                    let _ = std::fs::remove_file(&d.staged);
                    return Outcome::Nothing;
                }
                Self::settle(d, &mut self.opens, self.sessions.as_deref())
            }
            DownloadChange::Failed(why) => {
                let Some(d) = self.get_mut(id) else {
                    return Outcome::Nothing;
                };
                match d.transfer {
                    Transfer::Engine => {}
                    Transfer::EngineStopping => {
                        // The pause stopped it; the kept bytes stay.
                        d.transfer = Transfer::Idle;
                        let _ = std::fs::remove_file(&d.staged);
                        return Outcome::Nothing;
                    }
                    _ => return Outcome::Nothing,
                }
                d.transfer = Transfer::Idle;
                let _ = std::fs::remove_file(&d.staged);
                if d.state != DownloadState::Running {
                    return Outcome::Nothing;
                }
                d.open_when_done = false;
                d.state = DownloadState::Failed(why.clone());
                Outcome::Failed(why)
            }
        }
    }

    /// News from Morph's own transfers (resumed downloads).
    pub fn poll_fetches(&mut self) -> Vec<Outcome> {
        self.poll_waits(Instant::now());
        let mut out = Vec::new();
        while let Ok(msg) = self.fetch_rx.try_recv() {
            out.push(self.apply_fetch(msg, Instant::now()));
        }
        out
    }

    /// Wait up to `limit` for news from Morph's own transfers (tests).
    pub fn wait_fetches(&mut self, limit: Duration) -> Vec<Outcome> {
        match self.fetch_rx.recv_timeout(limit) {
            Ok(msg) => {
                let mut out = vec![self.apply_fetch(msg, Instant::now())];
                out.extend(self.poll_fetches());
                out
            }
            Err(_) => Vec::new(),
        }
    }

    fn apply_fetch(&mut self, msg: fetch::Msg, now: Instant) -> Outcome {
        let Some(d) = self.items.iter_mut().find(|d| d.id == msg.id) else {
            return Outcome::Nothing;
        };
        if !matches!(&d.transfer, Transfer::Own { generation, .. } if *generation == msg.generation) {
            return Outcome::Nothing;
        }
        match msg.change {
            fetch::Change::Restarted => {
                d.restarted = true;
                d.received = 0;
                d.rate.restart();
                Outcome::Nothing
            }
            fetch::Change::Progress { received, total } => {
                d.received = received;
                d.total = total.or(d.total);
                d.rate.sample(now, received);
                Outcome::Nothing
            }
            fetch::Change::Validators { etag, last_modified } => {
                d.context.etag = etag;
                d.context.last_modified = last_modified;
                if let Some(s) = &self.sessions {
                    s.save(&d.session_key, &d.context);
                }
                Outcome::Nothing
            }
            fetch::Change::Finished => {
                d.complete = true;
                d.transfer = Transfer::Idle;
                Self::settle(d, &mut self.opens, self.sessions.as_deref())
            }
            fetch::Change::Failed(why) => {
                d.transfer = Transfer::Idle;
                let _ = std::fs::remove_file(&d.staged);
                d.open_when_done = false;
                d.state = DownloadState::Failed(why.clone());
                Outcome::Failed(why)
            }
        }
    }

    /// The save dialog for `id` answered: a path, or `None` (cancelled).
    /// Returns whether the backend must stop the download.
    pub fn answer(&mut self, id: u64, path: Option<PathBuf>) -> (bool, Outcome) {
        self.asks.retain(|a| *a != id);
        let Some(d) = self.items.iter_mut().find(|d| d.id == id) else {
            return (false, Outcome::Nothing);
        };
        if !d.active() {
            return (false, Outcome::Nothing);
        }
        match path {
            Some(path) => {
                d.target = Some(path);
                if d.state == DownloadState::Running {
                    (false, Self::settle(d, &mut self.opens, self.sessions.as_deref()))
                } else {
                    (false, Outcome::Nothing)
                }
            }
            None => (self.cancel(id), Outcome::Nothing),
        }
    }

    /// Pause `id`: its bytes so far are kept. Returns whether the backend
    /// must stop the engine's download.
    pub fn pause(&mut self, id: u64) -> bool {
        let Some(d) = self.get_mut(id) else {
            return false;
        };
        if d.state != DownloadState::Running || d.complete {
            return false;
        }
        d.state = DownloadState::Paused;
        d.rate.restart();
        match std::mem::replace(&mut d.transfer, Transfer::Idle) {
            Transfer::Engine => {
                // The engine deletes its file when it stops: keep a link.
                let kept = kept_path(&d.staged);
                let _ = std::fs::remove_file(&kept);
                if let Some(partial) = partial_file(&d.staged)
                    && std::fs::hard_link(&partial, &kept).is_err()
                {
                    let _ = std::fs::copy(&partial, &kept);
                }
                d.transfer = Transfer::EngineStopping;
                true
            }
            Transfer::Own { stop, .. } => {
                stop.store(fetch::PAUSE, Ordering::SeqCst);
                false
            }
            // Paused before the resume got going: the kept bytes stay put.
            Transfer::AwaitingCookies { .. } => false,
            other => {
                d.transfer = other;
                false
            }
        }
    }

    /// Resume paused `id`: the rest comes over HTTP from where the kept
    /// bytes end (all of it when the server can't resume).
    pub fn resume(&mut self, id: u64) -> Outcome {
        let Some(d) = self.items.iter_mut().find(|d| d.id == id) else {
            return Outcome::Nothing;
        };
        if d.state != DownloadState::Paused {
            return Outcome::Nothing;
        }
        d.state = DownloadState::Running;
        d.rate.restart();
        if d.complete {
            return Self::settle(d, &mut self.opens, self.sessions.as_deref());
        }
        let kept = kept_path(&d.staged);
        if kept.exists() {
            let _ = std::fs::remove_file(&d.staged);
            if std::fs::rename(&kept, &d.staged).is_err() {
                let _ = std::fs::copy(&kept, &d.staged);
                let _ = std::fs::remove_file(&kept);
            }
        }
        let offset = std::fs::metadata(&d.staged).map(|m| m.len()).unwrap_or(0);
        d.received = offset;
        d.restarted = false;
        // The caller asks the engine for fresh cookies ([`Self::cookies_answered`]).
        d.transfer = Transfer::AwaitingCookies { since: Instant::now(), offset };
        Outcome::Nothing
    }

    fn start_own(d: &mut DownloadItem, generation: u64, offset: u64, tx: Sender<fetch::Msg>) {
        let stop = Arc::new(AtomicU8::new(fetch::RUN));
        d.transfer = Transfer::Own { generation, stop: stop.clone() };
        if let Some(dir) = d.staged.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let url = if d.context.final_url.is_empty() { d.url.clone() } else { d.context.final_url.clone() };
        let headers = d.context.resume_headers();
        fetch::spawn(fetch::Job { id: d.id, generation, url, path: d.staged.clone(), offset, headers, stop }, tx);
    }

    /// Cancel `id` and delete its bytes. Returns whether the backend must
    /// stop the engine's download.
    pub fn cancel(&mut self, id: u64) -> bool {
        self.asks.retain(|a| *a != id);
        let sessions = self.sessions.clone();
        let Some(d) = self.get_mut(id) else {
            return false;
        };
        if !d.active() {
            return false;
        }
        d.state = DownloadState::Cancelled;
        d.open_when_done = false;
        let _ = std::fs::remove_file(kept_path(&d.staged));
        Self::forget_session(d, sessions.as_deref());
        match std::mem::replace(&mut d.transfer, Transfer::Idle) {
            // Its bytes go when the engine reports it stopped.
            Transfer::Engine if !d.complete => {
                d.transfer = Transfer::Engine;
                true
            }
            Transfer::EngineStopping => {
                d.transfer = Transfer::EngineStopping;
                false
            }
            Transfer::Own { stop, .. } => {
                stop.store(fetch::CANCEL, Ordering::SeqCst);
                false
            }
            _ => {
                let _ = std::fs::remove_file(&d.staged);
                false
            }
        }
    }

    /// Start failed or cancelled `id` again from its address (Morph's own
    /// transfer; the save dialog asks again when it never answered).
    pub fn retry(&mut self, id: u64) {
        let Some(d) = self.items.iter_mut().find(|d| d.id == id) else {
            return;
        };
        if !matches!(d.state, DownloadState::Failed(_) | DownloadState::Cancelled) {
            return;
        }
        d.state = DownloadState::Running;
        d.complete = false;
        d.received = 0;
        d.restarted = false;
        d.rate = Rate::default();
        let _ = std::fs::remove_file(&d.staged);
        if d.target.is_none() {
            self.asks.push_back(id);
        }
        let d = self.items.iter_mut().find(|d| d.id == id).expect("just found");
        d.transfer = Transfer::AwaitingCookies { since: Instant::now(), offset: 0 };
    }

    /// A click on `id`'s tile: a saved file's path to open now; while it
    /// runs or is paused, open it when it is saved (a second click takes
    /// that back).
    pub fn activate(&mut self, id: u64) -> Option<PathBuf> {
        let d = self.get_mut(id)?;
        match &d.state {
            DownloadState::Saved(path) => Some(path.clone()),
            DownloadState::Running | DownloadState::Paused => {
                d.open_when_done = !d.open_when_done;
                None
            }
            _ => None,
        }
    }

    /// Move the file into place once the bytes and the target are in.
    fn settle(d: &mut DownloadItem, opens: &mut VecDeque<PathBuf>, sessions: Option<&dyn SessionStore>) -> Outcome {
        let (true, Some(target)) = (d.complete, d.target.clone()) else {
            return Outcome::Nothing;
        };
        match move_file(&d.staged, &target) {
            Ok(()) => {
                d.state = DownloadState::Saved(target.clone());
                // Complete: its session is no longer needed anywhere.
                Self::forget_session(d, sessions);
                if std::mem::take(&mut d.open_when_done) {
                    opens.push_back(target.clone());
                }
                Outcome::Saved(target)
            }
            Err(e) => {
                let why = format!("could not save {}: {e}", target.display());
                let _ = std::fs::remove_file(&d.staged);
                d.open_when_done = false;
                d.state = DownloadState::Failed(why.clone());
                Outcome::Failed(why)
            }
        }
    }

    /// Forget the downloads that ended (and their sessions).
    pub fn clear_done(&mut self) {
        let sessions = self.sessions.clone();
        for d in self.items.iter_mut().filter(|d| !d.active()) {
            Self::forget_session(d, sessions.as_deref());
        }
        self.items.retain(DownloadItem::active);
    }

    /// Delete `d`'s session from the store and from memory.
    fn forget_session(d: &mut DownloadItem, sessions: Option<&dyn SessionStore>) {
        d.context = DownloadContext::default();
        if let Some(s) = sessions {
            s.delete(&d.session_key);
        }
    }
}

/// Rename `from` to `to`, or copy it across file systems.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("morph-dl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn started(d: &mut Downloads, folder: &Path, id: u64) -> PathBuf {
        let staged = morph_webview::download_staging(folder, "a.bin", id);
        std::fs::write(&staged, b"bytes").unwrap();
        d.apply(Download { id, change: DownloadChange::Started { url: "https://x/a.bin".into(), name: "a.bin".into(), staged: staged.clone() } });
        staged
    }

    #[test]
    fn the_file_moves_once_bytes_and_answer_are_in_either_order() {
        let folder = dir("order");
        let mut d = Downloads::default();
        let staged = started(&mut d, &folder, 1);
        assert_eq!(d.take_ask(), Some((1, "a.bin".into(), folder.clone())));
        assert_eq!(d.take_ask(), None);
        let target = folder.join("chosen.bin");
        assert_eq!(d.answer(1, Some(target.clone())), (false, Outcome::Nothing), "bytes still coming");
        d.apply(Download { id: 1, change: DownloadChange::Progress { received: 5, total: Some(10) } });
        assert_eq!(d.get(1).unwrap().fraction(), Some(0.5));
        assert_eq!(d.apply(Download { id: 1, change: DownloadChange::Finished }), Outcome::Saved(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), b"bytes");
        assert!(!staged.exists());

        let staged = started(&mut d, &folder, 2);
        assert_eq!(d.apply(Download { id: 2, change: DownloadChange::Finished }), Outcome::Nothing, "waits for the dialog");
        let target = folder.join("second.bin");
        assert_eq!(d.answer(2, Some(target.clone())), (false, Outcome::Saved(target.clone())));
        assert!(target.exists() && !staged.exists());
    }

    #[test]
    fn cancelling_the_dialog_stops_the_download_and_deletes_the_bytes() {
        let folder = dir("cancel");
        let mut d = Downloads::default();
        let staged = started(&mut d, &folder, 1);
        assert_eq!(d.answer(1, None), (true, Outcome::Nothing), "still running: stop it");
        d.apply(Download { id: 1, change: DownloadChange::Failed("cancelled".into()) });
        assert_eq!(d.get(1).unwrap().state, DownloadState::Cancelled);
        assert!(!staged.exists());

        let staged = started(&mut d, &folder, 2);
        d.apply(Download { id: 2, change: DownloadChange::Finished });
        assert_eq!(d.answer(2, None), (false, Outcome::Nothing), "already done: nothing to stop");
        assert!(!staged.exists());
    }

    #[test]
    fn a_failed_download_reports_why() {
        let folder = dir("fail");
        let mut d = Downloads::default();
        let staged = started(&mut d, &folder, 1);
        assert_eq!(d.apply(Download { id: 1, change: DownloadChange::Failed("net".into()) }), Outcome::Failed("net".into()));
        assert!(!staged.exists());
        assert_eq!(d.answer(1, Some(folder.join("x"))), (false, Outcome::Nothing));
        d.clear_done();
        assert!(d.items().is_empty());
    }

    /// A tiny HTTP server for `body`: honours `Range` when `ranges`,
    /// sends in 4 KiB pieces `pace` apart, and records each request's
    /// Range header.
    fn serve(body: Vec<u8>, ranges: bool, pace: Duration) -> (String, Arc<std::sync::Mutex<Vec<Option<String>>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file.bin", listener.local_addr().unwrap());
        let seen: Arc<std::sync::Mutex<Vec<Option<String>>>> = Arc::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range:") {
                        range = Some(v.trim().to_string());
                    }
                }
                log.lock().unwrap().push(range.clone());
                let start: usize =
                    range.as_deref().filter(|_| ranges).and_then(|r| r.strip_prefix("bytes=")?.trim_end_matches('-').parse().ok()).unwrap_or(0);
                let mut out = stream;
                let head = if ranges && range.is_some() {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len() - start,
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                };
                let _ = out.write_all(head.as_bytes());
                for piece in body[start..].chunks(4096) {
                    if out.write_all(piece).is_err() {
                        break;
                    }
                    std::thread::sleep(pace);
                }
            }
        });
        (url, seen)
    }

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 % 251) as u8).collect()
    }

    /// Wait for Morph's own transfer of `id` to end; the outcome.
    fn settle_own(d: &mut Downloads, id: u64) -> Outcome {
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            for outcome in d.wait_fetches(Duration::from_millis(200)) {
                if outcome != Outcome::Nothing {
                    return outcome;
                }
            }
            if !d.get(id).unwrap().active() {
                break;
            }
        }
        Outcome::Nothing
    }

    /// An engine download of `url` that has written `kept` bytes so far.
    fn started_from(d: &mut Downloads, folder: &Path, id: u64, url: &str, kept: &[u8]) -> PathBuf {
        let staged = morph_webview::download_staging(folder, "file.bin", id);
        std::fs::write(&staged, kept).unwrap();
        d.apply(Download { id, change: DownloadChange::Started { url: url.into(), name: "file.bin".into(), staged: staged.clone() } });
        d.apply(Download { id, change: DownloadChange::Progress { received: kept.len() as u64, total: Some(100_000) } });
        staged
    }

    #[test]
    fn pause_keeps_the_bytes_and_resume_fetches_only_the_rest() {
        let body = bytes(100_000);
        let (url, seen) = serve(body.clone(), true, Duration::ZERO);
        let folder = dir("resume");
        let mut d = Downloads::default();
        let staged = started_from(&mut d, &folder, 1, &url, &body[..40_000]);
        let target = folder.join("file.bin");
        d.answer(1, Some(target.clone()));
        assert!(d.pause(1), "the engine's download must stop");
        assert_eq!(d.get(1).unwrap().state, DownloadState::Paused);
        assert_eq!(d.get(1).unwrap().eta(), None, "paused: no estimate");
        assert!(d.get(1).unwrap().status_text().starts_with("Paused"));
        // The engine stops and deletes its file; the bytes stay kept.
        assert_eq!(d.apply(Download { id: 1, change: DownloadChange::Failed("cancelled".into()) }), Outcome::Nothing);
        assert!(!staged.exists());
        assert_eq!(d.get(1).unwrap().state, DownloadState::Paused, "the engine's stop is not a failure");
        assert!(!d.pause(1), "already paused");

        assert_eq!(d.resume(1), Outcome::Nothing);
        d.cookies_answered(1, None);
        assert!(d.get(1).unwrap().running());
        assert_eq!(settle_own(&mut d, 1), Outcome::Saved(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), body);
        assert_eq!(seen.lock().unwrap().as_slice(), [Some("bytes=40000-".to_string())]);
        assert!(!d.get(1).unwrap().restarted);
        assert_eq!(d.get(1).unwrap().eta(), None, "done: no estimate");
        assert!(d.get(1).unwrap().status_text().starts_with("Done"));
        assert!(!kept_path(&staged).exists() && !staged.exists());
    }

    #[test]
    fn a_server_that_cannot_resume_starts_over_and_says_so() {
        let body = bytes(50_000);
        let (url, _) = serve(body.clone(), false, Duration::ZERO);
        let folder = dir("restart");
        let mut d = Downloads::default();
        started_from(&mut d, &folder, 1, &url, &body[..10_000]);
        let target = folder.join("whole.bin");
        d.answer(1, Some(target.clone()));
        assert!(d.pause(1));
        d.apply(Download { id: 1, change: DownloadChange::Failed("cancelled".into()) });
        d.resume(1);
        d.cookies_answered(1, None);
        assert_eq!(settle_own(&mut d, 1), Outcome::Saved(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), body, "not the kept bytes twice");
        assert!(d.get(1).unwrap().restarted);
    }

    #[test]
    fn a_resumed_download_pauses_and_resumes_again() {
        let body = bytes(200_000);
        let (url, seen) = serve(body.clone(), true, Duration::from_millis(25));
        let folder = dir("pause-own");
        let mut d = Downloads::default();
        let staged = started_from(&mut d, &folder, 1, &url, &body[..1000]);
        let target = folder.join("again.bin");
        d.answer(1, Some(target.clone()));
        d.pause(1);
        d.apply(Download { id: 1, change: DownloadChange::Failed("cancelled".into()) });
        d.resume(1);
        d.cookies_answered(1, None);
        // Some bytes in, then pause Morph's own transfer.
        let until = Instant::now() + Duration::from_secs(10);
        while d.get(1).unwrap().received < 20_000 && Instant::now() < until {
            d.wait_fetches(Duration::from_millis(100));
        }
        assert!(!d.pause(1), "Morph's own transfer stops itself");
        std::thread::sleep(Duration::from_millis(300));
        d.poll_fetches();
        assert_eq!(d.get(1).unwrap().state, DownloadState::Paused);
        let kept = std::fs::metadata(&staged).unwrap().len();
        assert!(kept > 1000 && kept < body.len() as u64, "{kept}");
        d.resume(1);
        d.cookies_answered(1, None);
        assert_eq!(settle_own(&mut d, 1), Outcome::Saved(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), body);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1], Some(format!("bytes={kept}-")));
    }

    #[test]
    fn a_click_while_running_opens_the_file_once_saved() {
        let folder = dir("open");
        let mut d = Downloads::default();
        let staged = started(&mut d, &folder, 1);
        assert_eq!(d.activate(1), None, "still running: nothing to open yet");
        assert!(d.get(1).unwrap().open_when_done);
        assert_eq!(d.get(1).unwrap().open_label().as_deref(), Some("Opens when done"), "no size: no estimate");
        let t0 = Instant::now();
        d.apply_at(Download { id: 1, change: DownloadChange::Progress { received: 0, total: Some(1200) } }, t0);
        d.apply_at(Download { id: 1, change: DownloadChange::Progress { received: 100, total: Some(1200) } }, t0 + Duration::from_secs(1));
        assert_eq!(d.get(1).unwrap().open_label().as_deref(), Some("Opens in 11s"));
        let target = folder.join("open.bin");
        d.answer(1, Some(target.clone()));
        assert_eq!(d.take_open(), None);
        assert_eq!(d.apply(Download { id: 1, change: DownloadChange::Finished }), Outcome::Saved(target.clone()));
        assert_eq!(d.take_open(), Some(target.clone()), "opened once saved");
        assert_eq!(d.take_open(), None);
        assert!(!staged.exists());
        assert_eq!(d.activate(1), Some(target), "a saved file opens at once");

        // A second click takes the open back.
        started(&mut d, &folder, 2);
        d.activate(2);
        d.activate(2);
        assert!(!d.get(2).unwrap().open_when_done);
        d.answer(2, Some(folder.join("two.bin")));
        d.apply(Download { id: 2, change: DownloadChange::Finished });
        assert_eq!(d.take_open(), None);

        // A failed download never opens.
        started(&mut d, &folder, 3);
        d.activate(3);
        assert_eq!(d.apply(Download { id: 3, change: DownloadChange::Failed("net".into()) }), Outcome::Failed("net".into()));
        assert!(!d.get(3).unwrap().open_when_done);
        assert_eq!(d.get(3).unwrap().open_label(), None);
        assert_eq!(d.take_open(), None);
        assert_eq!(d.get(3).unwrap().status_text(), "Failed: net");
    }

    #[test]
    fn the_time_left_is_smoothed_and_only_shown_while_running_with_a_size() {
        let folder = dir("eta");
        let mut d = Downloads::default();
        started(&mut d, &folder, 1);
        let t0 = Instant::now();
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        // Unknown size: no estimate (an indeterminate bar).
        d.apply_at(Download { id: 1, change: DownloadChange::Progress { received: 0, total: None } }, at(0.0));
        d.apply_at(Download { id: 1, change: DownloadChange::Progress { received: 1000, total: None } }, at(1.0));
        assert_eq!(d.get(1).unwrap().fraction(), None);
        assert_eq!(d.get(1).unwrap().eta(), None);
        assert!(d.get(1).unwrap().speed().is_some());
        // A steady 1000 B/s for 100 000 bytes.
        for s in 2..=10u64 {
            d.apply_at(Download { id: 1, change: DownloadChange::Progress { received: s * 1000, total: Some(100_000) } }, at(s as f64));
        }
        let steady = d.get(1).unwrap().eta().unwrap().as_secs_f64();
        assert!((steady - 90.0).abs() < 2.0, "{steady}");
        // One fast second moves it a little; a raw rate would say 9 s.
        d.apply_at(Download { id: 1, change: DownloadChange::Progress { received: 21_000, total: Some(100_000) } }, at(11.0));
        let after = d.get(1).unwrap().eta().unwrap().as_secs_f64();
        assert!(after < steady && after > 20.0, "{steady} -> {after}");
        assert!(d.get(1).unwrap().status_text().contains(" left"));
    }

    #[test]
    fn times_left_read_well() {
        assert_eq!(eta_text(Duration::from_millis(300)), "1s");
        assert_eq!(eta_text(Duration::from_secs(12)), "12s");
        assert_eq!(eta_text(Duration::from_secs(59)), "59s");
        assert_eq!(eta_text(Duration::from_secs(60)), "1m");
        assert_eq!(eta_text(Duration::from_secs(130)), "3m");
        assert_eq!(eta_text(Duration::from_secs(3900)), "1h 5m");
    }

    #[test]
    fn cancel_from_the_panel_stops_and_retry_starts_over() {
        let body = bytes(30_000);
        let (url, _) = serve(body.clone(), true, Duration::ZERO);
        let folder = dir("retry");
        let mut d = Downloads::default();
        let staged = started_from(&mut d, &folder, 1, &url, &body[..5000]);
        let target = folder.join("retried.bin");
        d.answer(1, Some(target.clone()));
        assert!(d.cancel(1), "the engine's download must stop");
        d.apply(Download { id: 1, change: DownloadChange::Failed("cancelled".into()) });
        assert_eq!(d.get(1).unwrap().state, DownloadState::Cancelled);
        assert!(!staged.exists());
        assert!(d.progress().is_none());
        d.retry(1);
        d.cookies_answered(1, None);
        assert_eq!(d.progress().map(|p| p.0), Some(1));
        assert_eq!(settle_own(&mut d, 1), Outcome::Saved(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), body);
        d.clear_done();
        assert!(d.items().is_empty());
    }

    // --- Sessions: resume behind a login ---------------------------------

    use morph_core::download_sessions::LibrarySessions;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// What a gated server saw in one request.
    #[derive(Debug, Clone, Default)]
    struct Seen {
        cookie: Option<String>,
        range: Option<String>,
        if_range: Option<String>,
        referer: Option<String>,
        user_agent: Option<String>,
    }

    /// A server for `body` behind a login: without `Cookie: sid=<sid>` it
    /// answers 403. It honours `Range` only while `If-Range` (if sent)
    /// matches its current `ETag` (`etag`, changeable to simulate a new file).
    fn serve_gated(body: Arc<Mutex<Vec<u8>>>, sid: &str, etag: Arc<Mutex<String>>) -> (String, Arc<Mutex<Vec<Seen>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/private/file.bin", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let (log, want) = (seen.clone(), format!("sid={sid}"));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut r = Seen::default();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    let Some((name, value)) = line.split_once(':') else { continue };
                    let value = Some(value.trim().to_string());
                    match name.trim().to_ascii_lowercase().as_str() {
                        "cookie" => r.cookie = value,
                        "range" => r.range = value,
                        "if-range" => r.if_range = value,
                        "referer" => r.referer = value,
                        "user-agent" => r.user_agent = value,
                        _ => {}
                    }
                }
                log.lock().unwrap().push(r.clone());
                let mut out = stream;
                if !r.cookie.as_deref().unwrap_or("").split("; ").any(|c| c == want) {
                    let _ = out.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let body = body.lock().unwrap().clone();
                let tag = etag.lock().unwrap().clone();
                let same = r.if_range.as_deref().is_none_or(|v| v == tag);
                let start: usize =
                    r.range.as_deref().filter(|_| same).and_then(|v| v.strip_prefix("bytes=")?.trim_end_matches('-').parse().ok()).unwrap_or(0);
                let head = if start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nETag: {tag}\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len() - start,
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nETag: {tag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                };
                let _ = out.write_all(head.as_bytes());
                let _ = out.write_all(&body[start..]);
            }
        });
        (url, seen)
    }

    /// A session store in memory that counts what it holds.
    #[derive(Default)]
    struct MemStore(Mutex<HashMap<String, DownloadContext>>);

    impl SessionStore for MemStore {
        fn save(&self, key: &str, context: &DownloadContext) {
            self.0.lock().unwrap().insert(key.into(), context.clone());
        }
        fn load(&self, key: &str) -> Option<DownloadContext> {
            self.0.lock().unwrap().get(key).cloned()
        }
        fn delete(&self, key: &str) {
            self.0.lock().unwrap().remove(key);
        }
        fn clear(&self) {
            self.0.lock().unwrap().clear();
        }
    }

    impl MemStore {
        fn len(&self) -> usize {
            self.0.lock().unwrap().len()
        }
        fn cookies(&self) -> Vec<String> {
            self.0.lock().unwrap().values().flat_map(|c| c.cookies.iter().map(|k| format!("{}={}", k.name, k.value))).collect()
        }
    }

    fn cookie(name: &str, value: &str) -> morph_webview::Cookie {
        morph_webview::Cookie { name: name.into(), value: value.into(), domain: "127.0.0.1".into(), path: "/".into(), secure: false }
    }

    /// The engine's word on the request: Referer, User-Agent, the ETag.
    fn engine_context(d: &mut Downloads, id: u64, url: &str, etag: &str) {
        let context = morph_webview::RequestContext {
            final_url: url.into(),
            referer: Some("http://app.test/files".into()),
            user_agent: Some("Morph-Test/1".into()),
            headers: vec![("Referer".into(), "http://app.test/files".into()), ("Accept".into(), "*/*".into())],
            etag: Some(etag.into()),
            last_modified: None,
        };
        d.apply(Download { id, change: DownloadChange::Context(context) });
    }

    /// Start `id` from `url` with `kept` bytes in, answer its dialog, pause it
    /// and let the engine stop: ready to resume.
    fn paused_at(d: &mut Downloads, folder: &Path, id: u64, url: &str, kept: &[u8]) -> PathBuf {
        started_from(d, folder, id, url, kept);
        let target = folder.join(format!("file-{id}.bin"));
        d.answer(id, Some(target.clone()));
        assert!(d.pause(id));
        d.apply(Download { id, change: DownloadChange::Failed("cancelled".into()) });
        target
    }

    fn all_bytes(dir: &Path) -> Vec<u8> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(all_bytes(&p));
            } else if let Ok(b) = std::fs::read(&p) {
                out.extend(b);
            }
        }
        out
    }

    #[test]
    fn a_resume_behind_a_login_uses_the_saved_session_kept_encrypted() {
        let body = bytes(100_000);
        let (url, seen) = serve_gated(Arc::new(Mutex::new(body.clone())), "s3cr3t-session-value", Arc::new(Mutex::new("\"v1\"".into())));
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        morph_core::profiles::load_catalog(&home).unwrap();
        let profile = morph_core::profiles::create(&home, "Work").unwrap().id;
        morph_core::library_db::unlock(&home, &profile, &morph_core::morph_identity::profile_identity::LibraryKey::from_bytes([3; 32])).unwrap();
        let store = Arc::new(LibrarySessions::new(&home, &profile));
        let folder = dir("session");
        let mut d = Downloads::default();
        d.set_sessions(store.clone());

        started_from(&mut d, &folder, 1, &url, &body[..30_000]);
        engine_context(&mut d, 1, &url, "\"v1\"");
        // At the start the engine says which cookies it sent.
        d.cookies_answered(1, Some(vec![cookie("sid", "s3cr3t-session-value")]));
        let key = d.get(1).unwrap().session_key.clone();
        let saved = store.try_load(&key).unwrap().expect("the session is saved with the download");
        assert_eq!(saved.cookie_header().as_deref(), Some("sid=s3cr3t-session-value"));
        assert_eq!(saved.etag.as_deref(), Some("\"v1\""));
        assert!(saved.auth_headers.is_empty(), "only auth headers are kept");
        let target = folder.join("file.bin");
        d.answer(1, Some(target.clone()));
        assert!(d.pause(1));
        d.apply(Download { id: 1, change: DownloadChange::Failed("cancelled".into()) });
        // Nothing on disk shows the cookie: not the library, not the downloads folder.
        let needle = b"s3cr3t-session-value";
        let mut disk = all_bytes(&home);
        disk.extend(all_bytes(&folder));
        assert!(!disk.windows(needle.len()).any(|w| w == needle), "the cookie is readable on disk");

        // Resume: the engine can't say (a fresh start, its view closed): the saved session goes.
        d.get_mut(1).unwrap().context.cookies.clear();
        d.resume(1);
        assert!(d.awaiting_cookies(1));
        d.cookies_answered(1, None);
        assert_eq!(settle_own(&mut d, 1), Outcome::Saved(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), body);
        let r = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(r.cookie.as_deref(), Some("sid=s3cr3t-session-value"));
        assert_eq!(r.range.as_deref(), Some("bytes=30000-"));
        assert_eq!(r.if_range.as_deref(), Some("\"v1\""));
        assert_eq!(r.referer.as_deref(), Some("http://app.test/files"));
        assert_eq!(r.user_agent.as_deref(), Some("Morph-Test/1"));
        assert_eq!(store.try_load(&key).unwrap(), None, "completed: the session is deleted");
        assert_eq!(d.get(1).unwrap().context, DownloadContext::default(), "and gone from memory");
    }

    #[test]
    fn fresh_cookies_from_the_engine_win_over_the_saved_ones() {
        let body = bytes(60_000);
        let (url, seen) = serve_gated(Arc::new(Mutex::new(body.clone())), "new", Arc::new(Mutex::new("\"v1\"".into())));
        let folder = dir("fresh");
        let store = Arc::new(MemStore::default());
        let mut d = Downloads::default();
        d.set_sessions(store.clone());
        let target = paused_at(&mut d, &folder, 1, &url, &body[..20_000]);
        engine_context(&mut d, 1, &url, "\"v1\"");
        d.cookies_answered(1, Some(vec![cookie("sid", "old")]));
        assert_eq!(store.cookies(), ["sid=old"]);
        d.resume(1);
        d.cookies_answered(1, Some(vec![cookie("sid", "new")]));
        assert_eq!(settle_own(&mut d, 1), Outcome::Saved(target.clone()));
        assert_eq!(std::fs::read(&target).unwrap(), body);
        assert_eq!(seen.lock().unwrap().last().unwrap().cookie.as_deref(), Some("sid=new"));
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn without_the_session_a_resume_fails_clearly() {
        let body = bytes(60_000);
        let (url, _) = serve_gated(Arc::new(Mutex::new(body.clone())), "only-with-login", Arc::new(Mutex::new("\"v1\"".into())));
        let folder = dir("nosession");
        let mut d = Downloads::default();
        d.set_sessions(Arc::new(MemStore::default()));
        paused_at(&mut d, &folder, 1, &url, &body[..20_000]);
        d.resume(1);
        d.cookies_answered(1, None);
        let Outcome::Failed(why) = settle_own(&mut d, 1) else { panic!("the resume must fail") };
        assert!(why.contains("HTTP 403") && why.contains("sign in"), "{why}");
        assert!(matches!(&d.get(1).unwrap().state, DownloadState::Failed(w) if *w == why));

        // An expired saved session says so too.
        paused_at(&mut d, &folder, 2, &url, &body[..20_000]);
        d.cookies_answered(2, Some(vec![cookie("sid", "expired")]));
        d.resume(2);
        d.cookies_answered(2, None);
        let Outcome::Failed(why) = settle_own(&mut d, 2) else { panic!("the resume must fail") };
        assert!(why.contains("no longer works"), "{why}");
    }

    #[test]
    fn a_file_that_changed_on_the_server_restarts_cleanly() {
        let body = Arc::new(Mutex::new(bytes(50_000)));
        let etag = Arc::new(Mutex::new("\"v1\"".to_string()));
        let (url, seen) = serve_gated(body.clone(), "ok", etag.clone());
        let folder = dir("changed");
        let mut d = Downloads::default();
        d.set_sessions(Arc::new(MemStore::default()));
        let kept = body.lock().unwrap()[..25_000].to_vec();
        let target = paused_at(&mut d, &folder, 1, &url, &kept);
        engine_context(&mut d, 1, &url, "\"v1\"");
        d.cookies_answered(1, Some(vec![cookie("sid", "ok")]));
        let new: Vec<u8> = (0..70_000).map(|i| (i % 13) as u8).collect();
        *body.lock().unwrap() = new.clone();
        *etag.lock().unwrap() = "\"v2\"".into();
        d.resume(1);
        d.cookies_answered(1, None);
        assert_eq!(settle_own(&mut d, 1), Outcome::Saved(target.clone()));
        assert!(d.get(1).unwrap().restarted, "the tile says it started over");
        assert_eq!(std::fs::read(&target).unwrap(), new, "no old bytes mixed in");
        assert_eq!(seen.lock().unwrap().last().unwrap().if_range.as_deref(), Some("\"v1\""));
    }

    #[test]
    fn cancel_and_clear_delete_the_session() {
        let folder = dir("forget");
        let store = Arc::new(MemStore::default());
        let mut d = Downloads::default();
        d.set_sessions(store.clone());
        started_from(&mut d, &folder, 1, "http://127.0.0.1:9/a", b"x");
        d.cookies_answered(1, Some(vec![cookie("sid", "a")]));
        started_from(&mut d, &folder, 2, "http://127.0.0.1:9/b", b"y");
        d.cookies_answered(2, Some(vec![cookie("sid", "b")]));
        assert_eq!(store.len(), 2);
        d.cancel(1);
        assert_eq!(store.cookies(), ["sid=b"], "cancelled: its session is gone");
        d.apply(Download { id: 2, change: DownloadChange::Failed("net".into()) });
        assert_eq!(store.len(), 1, "a failed download keeps it for a retry");
        d.clear_done();
        assert_eq!(store.len(), 0, "cleared: gone");
        // A new store forgets rows an earlier run left.
        let stale = Arc::new(MemStore::default());
        stale.save("dl-old", &DownloadContext::default());
        d.set_sessions(stale.clone());
        assert_eq!(stale.len(), 0);
    }
}
