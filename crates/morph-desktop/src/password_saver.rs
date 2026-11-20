//! The password saver in the desktop shell (`docs/profile-lock.md`,
//! "Password saver"): the page bridge that hears login forms
//! (`morph_webview::login_form`), the system's authentication behind
//! `OsAuth`, and what a window offers for its tabs: save a login the user
//! just submitted, fill a saved one into the page it was saved for.
//!
//! Nothing here fills without the user pressing Fill, and a fill waits for
//! whatever the profile needs first (the profile password with "ask again",
//! the system's prompt with the password off), on a thread of its own so a
//! system prompt never blocks the window.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, mpsc};

use morph_core::tabs::TabId;
use morph_services::password_saver::{self as saver, Captured, LoginInfo, OsAuth, OsAuthError};
use morph_services::{SecretString, Zeroizing};
use morph_webview::bridge::Trust;
use morph_webview::login_form::{self, PageLogin};
use morph_webview::{BridgeContext, PageBridge};

/// Whether the saver is shown at all. It stays hidden until it is finished
/// (a preview may be cut from main before then): `MORPH_PASSWORD_SAVER=1`
/// turns it on for development and the headless checks. Hidden, pages get
/// no login script, windows no bar, and Settings no Passwords rows.
pub fn shown() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("MORPH_PASSWORD_SAVER").is_some_and(|v| v == "1"))
}

/// Logins the page bridge heard, until the window takes them.
pub type Inbox = Rc<RefCell<Vec<PageLogin>>>;

/// The most logins kept for the window between two polls.
const INBOX_MAX: usize = 16;

/// The page bridge of a window: Morph's page channel as before, plus the
/// saver's host script and its messages, which go to `inbox`.
pub fn page_bridge(inbox: Inbox) -> PageBridge {
    PageBridge::with_context(morph_core::page_bridge::BRIDGE_SCRIPT, Rc::new(move |ctx: &BridgeContext, body: &str| answer(&inbox, ctx, body)))
        .host_script(login_form::HOST_SCRIPT)
}

fn answer(inbox: &Inbox, ctx: &BridgeContext, body: &str) -> Option<String> {
    match ctx.trust {
        Trust::Page => morph_core::page_bridge::answer(body),
        Trust::Host if login_form::is_saver_message(body) => {
            if let Some(login) = login_form::parse(ctx, body) {
                let mut inbox = inbox.borrow_mut();
                if inbox.len() >= INBOX_MAX {
                    inbox.remove(0);
                }
                inbox.push(login);
            }
            login_form::reply(body)
        }
        Trust::Host => None,
    }
}

/// The system's authentication (`morph_os::os_auth`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemOsAuth;

impl OsAuth for SystemOsAuth {
    fn available(&self) -> bool {
        morph_os::os_auth::available()
    }

    fn authenticate(&self, reason: &str) -> Result<(), OsAuthError> {
        morph_os::os_auth::authenticate(reason).map_err(|e| match e {
            morph_os::os_auth::Failure::Denied => OsAuthError::Denied,
            morph_os::os_auth::Failure::Unavailable(_) => OsAuthError::Unavailable,
        })
    }
}

/// Ask the system once, off the UI thread, whether it can authenticate,
/// so the first offer does not wait for it.
pub fn warm_up() {
    std::thread::spawn(morph_os::os_auth::available);
}

/// The host part of an origin, for the bar's words.
pub fn site(origin: &str) -> &str {
    origin.split_once("://").map_or(origin, |(_, h)| h)
}

/// A fill offer: the page shows a login form and logins are saved for its origin.
pub struct FillOffer {
    pub tab: TabId,
    pub origin: String,
    pub logins: Vec<LoginInfo>,
    pub choice: usize,
    /// "Ask again": the profile password goes with the fill.
    pub ask_password: bool,
    /// The password is off: the system asks.
    pub os_auth: bool,
    pub error: Option<String>,
}

impl FillOffer {
    pub fn chosen(&self) -> &LoginInfo {
        &self.logins[self.choice.min(self.logins.len() - 1)]
    }
}

/// What the window offers, for one tab.
pub enum Bar {
    /// The user submitted a login that is not saved (or saved with another password).
    Save {
        tab: TabId,
        url: String,
        origin: String,
        username: String,
        password: SecretString,
        update: bool,
    },
    Fill(FillOffer),
    /// The proofs and the saved password, on their own thread.
    Working {
        offer: FillOffer,
        rx: mpsc::Receiver<Result<Zeroizing<String>, String>>,
    },
}

impl Bar {
    pub fn tab(&self) -> TabId {
        match self {
            Bar::Save { tab, .. } => *tab,
            Bar::Fill(offer) | Bar::Working { offer, .. } => offer.tab,
        }
    }
}

/// A saved password ready to go into a tab (the page actions run next).
pub struct Ready {
    pub tab: TabId,
    pub origin: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

/// One window's saver: the profile it saves for, and what it offers now.
pub struct Saver {
    home: PathBuf,
    profile: String,
    os: Arc<dyn OsAuth>,
    pub bar: Option<Bar>,
    /// Tabs (not shown when their form appeared) with a login form, by origin.
    waiting: HashMap<TabId, String>,
}

impl Saver {
    pub fn new(home: &Path, profile: &str, os: Arc<dyn OsAuth>) -> Self {
        Self { home: home.to_path_buf(), profile: profile.to_string(), os, bar: None, waiting: HashMap::new() }
    }

    fn offer(&self) -> Option<saver::Offer> {
        saver::offer(&self.home, &self.profile, self.os.as_ref()).ok().filter(saver::Offer::active)
    }

    /// Take one login the bridge heard in `tab` (`None`: not a tab's page),
    /// while `active` is shown and `url_of` reads a tab's address.
    pub fn heard(&mut self, login: PageLogin, tab: Option<TabId>, active: TabId, url_of: impl Fn(TabId) -> Option<String>) {
        let Some(tab) = tab else { return };
        match login {
            PageLogin::Form { origin, .. } => {
                if tab == active {
                    self.offer_fill(tab, &origin, &url_of);
                } else {
                    self.waiting.insert(tab, origin);
                }
            }
            PageLogin::Submitted { url, origin, username, password, .. } => {
                if matches!(self.bar, Some(Bar::Working { .. })) || self.offer().is_none() {
                    return;
                }
                let password = SecretString::from(password);
                let update = match saver::captured(&self.home, &self.profile, self.os.as_ref(), &url, &username, &password) {
                    Ok(Captured::New) => false,
                    Ok(Captured::Changed(_)) => true,
                    Ok(Captured::Same) | Err(_) => return,
                };
                self.waiting.remove(&tab);
                self.bar = Some(Bar::Save { tab, url, origin, username, password, update });
            }
        }
    }

    /// The window now shows `active`: a form that appeared while it was in
    /// the background gets its offer now.
    pub fn shown(&mut self, active: TabId, url_of: impl Fn(TabId) -> Option<String>) {
        if self.bar.is_none()
            && let Some(origin) = self.waiting.remove(&active)
        {
            self.offer_fill(active, &origin, &url_of);
        }
    }

    /// Tab `tab` is at `url` now: a fill offer for another origin goes, and
    /// so does everything for a closed tab (`url` `None`).
    pub fn moved(&mut self, tab: TabId, url: Option<&str>) {
        let stale = |origin: &str| url.is_none_or(|u| !saver::page_matches(origin, u));
        if self.waiting.get(&tab).is_some_and(|o| stale(o)) {
            self.waiting.remove(&tab);
        }
        let drop_bar = match &self.bar {
            Some(Bar::Fill(offer)) if offer.tab == tab => stale(&offer.origin),
            Some(bar) if bar.tab() == tab => url.is_none(),
            _ => false,
        };
        if drop_bar {
            self.bar = None;
        }
    }

    /// Not now.
    pub fn dismiss(&mut self) {
        if !matches!(self.bar, Some(Bar::Working { .. })) {
            self.bar = None;
        }
    }

    fn offer_fill(&mut self, tab: TabId, origin: &str, url_of: &impl Fn(TabId) -> Option<String>) {
        if matches!(self.bar, Some(Bar::Save { .. } | Bar::Working { .. })) {
            return;
        }
        let Some(url) = url_of(tab).filter(|u| saver::page_matches(origin, u)) else { return };
        let Some(offer) = self.offer() else { return };
        let Ok(requirement) = offer.requirement else { return };
        let logins = match saver::list(&self.home, &self.profile, self.os.as_ref(), Some(&url)) {
            Ok(l) if !l.is_empty() => l,
            _ => return,
        };
        self.bar = Some(Bar::Fill(FillOffer {
            tab,
            origin: origin.to_string(),
            logins,
            choice: 0,
            ask_password: requirement.password,
            os_auth: requirement.os_auth,
            error: None,
        }));
    }

    /// Save (Save or Update pressed). The words for the result.
    pub fn save(&mut self) -> Result<String, String> {
        let Some(Bar::Save { url, origin, username, password, update, .. }) = self.bar.take() else {
            return Err("Nothing to save.".into());
        };
        saver::save(&self.home, &self.profile, self.os.as_ref(), &url, &username, &password)
            .map_err(|e| format!("Could not save the password: {e}"))?;
        let who = if username.is_empty() { String::new() } else { format!(" for {username}") };
        Ok(format!("{} the password{who} on {}", if update { "Updated" } else { "Saved" }, site(&origin)))
    }

    /// Pick another of the saved logins in the fill bar.
    pub fn choose(&mut self, index: usize) {
        if let Some(Bar::Fill(offer)) = &mut self.bar
            && index < offer.logins.len()
        {
            offer.choice = index;
        }
    }

    /// Fill pressed: prove what the profile needs (with `password`, typed
    /// and not empty, when "ask again" is on) and open the saved password,
    /// on a thread. `page_url` is the tab's address now.
    pub fn start_fill(&mut self, page_url: &str, password: Option<SecretString>) {
        let Some(Bar::Fill(mut offer)) = self.bar.take() else { return };
        let missing = offer.ask_password && password.is_none();
        let error = if !saver::page_matches(&offer.origin, page_url) {
            Some("This page is no longer the site the login was saved for.")
        } else if missing {
            Some("Enter the profile password to fill this login.")
        } else {
            None
        };
        if let Some(error) = error {
            offer.error = Some(error.into());
            self.bar = Some(Bar::Fill(offer));
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (home, profile, os, url, id) =
            (self.home.clone(), self.profile.clone(), self.os.clone(), page_url.to_string(), offer.chosen().id.clone());
        std::thread::spawn(move || {
            let got = saver::fill(&home, &profile, os.as_ref(), &id, &url, password.as_ref()).map_err(|e| fill_failure(&e));
            let _ = tx.send(got);
        });
        offer.error = None;
        self.bar = Some(Bar::Working { offer, rx });
    }

    /// Whether a fill's proofs came back: the password to put in the page,
    /// or the fill bar again with why not.
    pub fn poll(&mut self) -> Option<Ready> {
        let Some(Bar::Working { rx, .. }) = &self.bar else { return None };
        let got = match rx.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("The fill stopped.".into()),
        };
        let Some(Bar::Working { mut offer, .. }) = self.bar.take() else { return None };
        match got {
            Ok(password) => Some(Ready { tab: offer.tab, origin: offer.origin.clone(), username: offer.chosen().username.clone(), password }),
            Err(why) => {
                offer.error = Some(why);
                self.bar = Some(Bar::Fill(offer));
                None
            }
        }
    }

    /// Wait for a fill started with [`Self::start_fill`] (tests).
    #[cfg(test)]
    fn wait(&mut self) -> Option<Ready> {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < end {
            if !matches!(self.bar, Some(Bar::Working { .. })) {
                return None;
            }
            if let Some(ready) = self.poll() {
                return Some(ready);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }
}

/// The words for a refused fill (never a secret).
pub fn fill_failure(e: &morph_services::Error) -> String {
    match e {
        morph_services::Error::WrongPassword => "That password is not right.".into(),
        e @ morph_services::Error::TooSoon { .. } => format!("Too many wrong passwords ({e})."),
        morph_services::Error::Locked => "The profile is locked.".into(),
        other => format!("Not filled: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_webview::ViewId;
    use morph_webview::bridge::{FrameKind, ViewBridge};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The system's prompt, faked: whether it is there, whether the user passes, how often it asked.
    struct FakeOs {
        available: bool,
        pass: bool,
        asked: AtomicUsize,
    }

    impl OsAuth for FakeOs {
        fn available(&self) -> bool {
            self.available
        }
        fn authenticate(&self, _reason: &str) -> Result<(), OsAuthError> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            if self.pass { Ok(()) } else { Err(OsAuthError::Denied) }
        }
    }

    fn os(available: bool, pass: bool) -> Arc<FakeOs> {
        Arc::new(FakeOs { available, pass, asked: 0.into() })
    }

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    const PROFILE_PASSWORD: &str = "profile password";
    const TAB: TabId = TabId(1);
    const OTHER: TabId = TabId(2);

    fn profile() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        morph_services::identity::create(&home, "default", &pw(PROFILE_PASSWORD)).unwrap();
        (tmp, home)
    }

    fn submitted(url: &str, user: &str, password: &str) -> PageLogin {
        PageLogin::Submitted {
            view: ViewId(1),
            origin: morph_webview::bridge::origin_of(url),
            url: url.into(),
            username: user.into(),
            password: password.into(),
        }
    }

    fn form(url: &str) -> PageLogin {
        PageLogin::Form { view: ViewId(1), origin: morph_webview::bridge::origin_of(url) }
    }

    fn at(url: &'static str) -> impl Fn(TabId) -> Option<String> + Copy {
        move |_| Some(url.to_string())
    }

    #[test]
    fn the_bridge_keeps_the_page_channel_and_takes_only_host_logins() {
        let inbox = Inbox::default();
        let view = ViewBridge::new(ViewId(4), true, false);
        let page = view.context("https://example.com/", FrameKind::Unknown, Trust::Page);
        assert!(answer(&inbox, &page, r#"{"id":"1","type":"ping"}"#).unwrap().contains("pong"));
        let forged = r#"{"id":"2","type":"host.saver.submit","payload":{"username":"amy","password":"x"}}"#;
        answer(&inbox, &page, forged);
        assert!(inbox.borrow().is_empty(), "a page cannot claim a login");
        let host = view.context("https://example.com/login", FrameKind::Main, Trust::Host);
        assert_eq!(answer(&inbox, &host, forged).as_deref(), Some(r#"{"id":"2","type":"ok"}"#));
        assert!(matches!(&inbox.borrow()[0], PageLogin::Submitted { username, .. } if username == "amy"));
        assert_eq!(answer(&inbox, &host, r#"{"id":"3","type":"host.other"}"#), None);
        for i in 0..40 {
            answer(&inbox, &host, &format!(r#"{{"id":"{i}","type":"host.saver.form"}}"#));
        }
        assert_eq!(inbox.borrow().len(), INBOX_MAX, "a page cannot fill memory");
    }

    #[test]
    fn a_new_login_is_offered_saved_once_and_then_left_alone() {
        let (_tmp, home) = profile();
        let mut saver = Saver::new(&home, "default", os(false, false));
        let url = "https://example.com/login";
        saver.heard(submitted(url, "amy", "s3cret"), Some(TAB), TAB, at(url));
        assert!(matches!(&saver.bar, Some(Bar::Save { update: false, username, .. }) if username == "amy"));
        assert_eq!(saver.save().unwrap(), "Saved the password for amy on example.com");
        assert!(saver.bar.is_none());
        saver.heard(submitted(url, "amy", "s3cret"), Some(TAB), TAB, at(url));
        assert!(saver.bar.is_none(), "the same login again: nothing to ask");
        saver.heard(submitted(url, "amy", "n3wer"), Some(TAB), TAB, at(url));
        assert!(matches!(&saver.bar, Some(Bar::Save { update: true, .. })));
        saver.dismiss();
        assert!(saver.bar.is_none());
        // Not a tab's page (a host page): nothing.
        saver.heard(submitted(url, "bob", "x"), None, TAB, at(url));
        assert!(saver.bar.is_none());
    }

    #[test]
    fn a_login_form_offers_the_saved_login_and_the_fill_opens_it() {
        let (_tmp, home) = profile();
        let mut saver = Saver::new(&home, "default", os(false, false));
        let url = "https://example.com/login";
        saver.heard(form(url), Some(TAB), TAB, at(url));
        assert!(saver.bar.is_none(), "nothing saved yet");
        saver::save(&home, "default", &saver::NoOsAuth, url, "amy", &pw("s3cret")).unwrap();
        saver::save(&home, "default", &saver::NoOsAuth, url, "bob", &pw("hunter2")).unwrap();
        saver::save(&home, "default", &saver::NoOsAuth, "https://other.example/", "eve", &pw("x")).unwrap();
        saver.heard(form(url), Some(TAB), TAB, at(url));
        let Some(Bar::Fill(offer)) = &saver.bar else { panic!("no fill offer") };
        assert_eq!(offer.logins.len(), 2, "only this origin's logins");
        assert!(!offer.ask_password && !offer.os_auth);
        saver.choose(1);
        let chosen = match &saver.bar {
            Some(Bar::Fill(offer)) => offer.chosen().username.clone(),
            _ => unreachable!(),
        };
        saver.start_fill("https://evil.test/", None);
        assert!(matches!(&saver.bar, Some(Bar::Fill(o)) if o.error.is_some()), "the tab went elsewhere: refused before anything opens");
        saver.start_fill(url, None);
        let ready = saver.wait().expect("filled");
        assert_eq!((ready.username.as_str(), ready.origin.as_str(), ready.tab), (chosen.as_str(), "https://example.com", TAB));
        assert_eq!(ready.password.as_str(), if chosen == "amy" { "s3cret" } else { "hunter2" });
        assert!(saver.bar.is_none());
    }

    #[test]
    fn http_pages_and_other_origins_get_no_offer() {
        let (_tmp, home) = profile();
        saver::save(&home, "default", &saver::NoOsAuth, "https://example.com/", "amy", &pw("s3cret")).unwrap();
        let mut saver = Saver::new(&home, "default", os(false, false));
        saver.heard(form("http://example.com/"), Some(TAB), TAB, at("http://example.com/"));
        assert!(saver.bar.is_none());
        saver.heard(form("https://example.com/"), Some(TAB), TAB, at("https://example.com.evil.test/"));
        assert!(saver.bar.is_none(), "the form's origin must be the tab's");
        saver.heard(submitted("http://example.com/", "amy", "x"), Some(TAB), TAB, at("http://example.com/"));
        assert!(saver.bar.is_none(), "nothing saved from http");
        saver.heard(form("https://example.com/"), Some(TAB), TAB, at("https://example.com/"));
        assert!(saver.bar.is_some());
        saver.moved(TAB, Some("https://elsewhere.example/"));
        assert!(saver.bar.is_none(), "the offer goes when the tab leaves the site");
    }

    #[test]
    fn ask_again_needs_the_profile_password_for_the_fill() {
        let (_tmp, home) = profile();
        let url = "https://example.com/";
        saver::save(&home, "default", &saver::NoOsAuth, url, "amy", &pw("s3cret")).unwrap();
        saver::set_ask_again(&home, "default", true, None).unwrap();
        let mut saver = Saver::new(&home, "default", os(false, false));
        saver.heard(form(url), Some(TAB), TAB, at(url));
        assert!(matches!(&saver.bar, Some(Bar::Fill(o)) if o.ask_password));
        saver.start_fill(url, None);
        assert!(matches!(&saver.bar, Some(Bar::Fill(o)) if o.error.as_deref() == Some("Enter the profile password to fill this login.")));
        saver.start_fill(url, Some(pw("wrong one")));
        assert!(saver.wait().is_none());
        assert!(matches!(&saver.bar, Some(Bar::Fill(o)) if o.error.as_deref() == Some("That password is not right.")));
        // The wrong-password backoff of the lock screen applies: wait it out.
        let wait = morph_core::profile_lock::wait_ms(&home, "default", crate::library::now_ms());
        std::thread::sleep(std::time::Duration::from_millis(wait as u64 + 50));
        saver.start_fill(url, Some(pw(PROFILE_PASSWORD)));
        assert_eq!(saver.wait().expect("filled").password.as_str(), "s3cret");
    }

    #[test]
    fn with_the_password_off_the_system_asks_or_nothing_is_offered() {
        let (_tmp, home) = profile();
        let url = "https://example.com/";
        saver::save(&home, "default", &saver::NoOsAuth, url, "amy", &pw("s3cret")).unwrap();
        morph_services::profile_lock::set_require_password(&home, "default", &pw(PROFILE_PASSWORD), false).unwrap();

        let mut none = Saver::new(&home, "default", os(false, false));
        none.heard(form(url), Some(TAB), TAB, at(url));
        none.heard(submitted(url, "bob", "x"), Some(TAB), TAB, at(url));
        assert!(none.bar.is_none(), "no system authentication: no saver");

        let denied = os(true, false);
        let mut saver = Saver::new(&home, "default", denied.clone());
        saver.heard(form(url), Some(TAB), TAB, at(url));
        assert!(matches!(&saver.bar, Some(Bar::Fill(o)) if o.os_auth && !o.ask_password));
        saver.start_fill(url, None);
        assert!(saver.wait().is_none(), "cancelled: nothing opened");
        assert!(matches!(&saver.bar, Some(Bar::Fill(o)) if o.error.is_some()));
        assert_eq!(denied.asked.load(Ordering::SeqCst), 1);

        let passes = os(true, true);
        let mut saver = Saver::new(&home, "default", passes.clone());
        saver.heard(form(url), Some(TAB), TAB, at(url));
        saver.start_fill(url, None);
        assert_eq!(saver.wait().expect("filled").password.as_str(), "s3cret");
        assert_eq!(passes.asked.load(Ordering::SeqCst), 1, "asked once for the fill");
    }

    #[test]
    fn a_background_tab_gets_its_offer_when_shown_and_the_switch_turns_it_all_off() {
        let (_tmp, home) = profile();
        let url = "https://example.com/";
        saver::save(&home, "default", &saver::NoOsAuth, url, "amy", &pw("s3cret")).unwrap();
        let mut saver = Saver::new(&home, "default", os(false, false));
        saver.heard(form(url), Some(OTHER), TAB, at(url));
        assert!(saver.bar.is_none());
        saver.shown(OTHER, at(url));
        assert!(matches!(&saver.bar, Some(b) if b.tab() == OTHER));
        saver.moved(OTHER, None);
        assert!(saver.bar.is_none(), "a closed tab's offer goes");

        saver::set_enabled(&home, "default", false).unwrap();
        saver.heard(form(url), Some(TAB), TAB, at(url));
        saver.heard(submitted(url, "bob", "x"), Some(TAB), TAB, at(url));
        assert!(saver.bar.is_none(), "the saver is off");
    }

    #[test]
    fn a_profile_without_an_identity_never_offers() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        let mut saver = Saver::new(&home, "default", os(true, true));
        saver.heard(submitted("https://example.com/", "amy", "x"), Some(TAB), TAB, at("https://example.com/"));
        saver.heard(form("https://example.com/"), Some(TAB), TAB, at("https://example.com/"));
        assert!(saver.bar.is_none());
    }

    #[test]
    fn sites_read_plainly() {
        assert_eq!(site("https://example.com:8443"), "example.com:8443");
        assert_eq!(site("example.com"), "example.com");
    }
}
