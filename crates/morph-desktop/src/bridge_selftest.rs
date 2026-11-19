//! `MORPH_TEST_BRIDGE=1` (scripts/desktop-smoke.py): the page bridge's
//! reads, actions and throwaway host pages on this platform's real engine,
//! from the shell's own poll loop. Once the smoke page answered the bridge
//! (title `morph-smoke pong`), it reads the tab, fills and clicks in it,
//! then loads `/cookie` in a throwaway and a signed-in host page, and prints
//! one `morph-test-bridge ...` line per step for the smoke script to check.
//! Nothing runs without the variable.

use std::time::{Duration, Instant};

use morph_browser::{Browser, HostPageId, HostPageRequest, HostPageState};
use morph_webview::page_action::{Action, ActionRequest, ActionResult, Locator};
use morph_webview::page_read::{Pending, ReadResult};
use morph_webview::{ReadRequest, Want};

enum Step {
    /// Read the tab's title again at this time, until the page is ready.
    Wait(Instant),
    Ready(Pending<ReadResult>),
    Fill(Pending<ActionResult>),
    Click(Pending<ActionResult>),
    Clicked(Pending<ReadResult>),
    Open {
        id: HostPageId,
        loaded: Pending<HostPageState>,
        signed_in: bool,
    },
    ReadHost {
        id: HostPageId,
        read: Pending<ReadResult>,
        signed_in: bool,
    },
    Done,
}

pub struct BridgeSelfTest {
    step: Step,
    origin: String,
    give_up: Instant,
}

fn title() -> ReadRequest {
    ReadRequest::new(Want { title: true, ..Want::default() })
}

fn say(line: impl AsRef<str>) {
    println!("morph-test-bridge {}", line.as_ref());
}

impl BridgeSelfTest {
    /// The test, when `MORPH_TEST_BRIDGE=1`.
    pub fn from_env() -> Option<Self> {
        std::env::var_os("MORPH_TEST_BRIDGE").is_some_and(|v| v == "1").then(|| Self {
            step: Step::Wait(Instant::now()),
            origin: String::new(),
            give_up: Instant::now() + Duration::from_secs(90),
        })
    }

    /// One step from the poll loop; `true` when it has finished.
    pub fn tick(&mut self, browser: &mut Browser) -> bool {
        if Instant::now() > self.give_up {
            say("gave up (timed out)");
            return true;
        }
        let step = std::mem::replace(&mut self.step, Step::Done);
        self.step = match step {
            Step::Wait(at) if Instant::now() >= at => Step::Ready(browser.read_active_tab(title())),
            Step::Ready(read) => match read.try_take() {
                None => Step::Ready(read),
                Some(Ok(page)) if page.title.as_deref() == Some("morph-smoke pong") => {
                    let isolation = page.isolation.map(|i| format!("{i:?}")).unwrap_or_default();
                    say(format!("read isolation={isolation} title={}", page.title.unwrap_or_default()));
                    self.origin = page.origin;
                    let fill = Action::Fill { target: Locator::role("textbox", Some("Smoke field")), value: "filled-by-morph".into() };
                    Step::Fill(browser.active_tab_action(ActionRequest::new(fill).on_origin(self.origin.clone())))
                }
                Some(_) => Step::Wait(Instant::now() + Duration::from_millis(300)),
            },
            Step::Fill(action) => match action.try_take() {
                None => Step::Fill(action),
                Some(result) => {
                    match &result {
                        Ok(out) => say(format!(
                            "fill isolation={:?} value={}",
                            out.isolation,
                            out.element.as_ref().and_then(|e| e.value.clone()).unwrap_or_default()
                        )),
                        Err(e) => say(format!("fill error={e}")),
                    }
                    Step::Click(browser.active_tab_action(
                        ActionRequest::new(Action::Click(Locator::role("button", Some("Smoke button")))).on_origin(self.origin.clone()),
                    ))
                }
            },
            Step::Click(action) => match action.try_take() {
                None => Step::Click(action),
                Some(result) => {
                    if let Err(e) = &result {
                        say(format!("click error={e}"));
                    }
                    Step::Clicked(browser.read_active_tab(title()))
                }
            },
            Step::Clicked(read) => match read.try_take() {
                None => Step::Clicked(read),
                Some(result) => {
                    match result {
                        Ok(page) => say(format!("click title={}", page.title.unwrap_or_default())),
                        Err(e) => say(format!("click read error={e}")),
                    }
                    self.open(browser, false)
                }
            },
            Step::Open { id, loaded, signed_in } => match loaded.try_take() {
                None => Step::Open { id, loaded, signed_in },
                Some(HostPageState::Loaded) => Step::ReadHost { id, read: browser.read_host_page(id, title()), signed_in },
                Some(state) => {
                    say(format!("{} error={state:?}", if signed_in { "signed-in" } else { "ephemeral" }));
                    browser.close_host_page(id);
                    if signed_in { Step::Done } else { self.open(browser, true) }
                }
            },
            Step::ReadHost { id, read, signed_in } => match read.try_take() {
                None => Step::ReadHost { id, read, signed_in },
                Some(result) => {
                    let what = if signed_in { "signed-in" } else { "ephemeral" };
                    match result {
                        Ok(page) => say(format!("{what} title={}", page.title.unwrap_or_default())),
                        Err(e) => say(format!("{what} error={e}")),
                    }
                    browser.close_host_page(id);
                    if signed_in { Step::Done } else { self.open(browser, true) }
                }
            },
            other => other,
        };
        if matches!(self.step, Step::Done) {
            say("done");
            return true;
        }
        false
    }

    fn open(&self, browser: &mut Browser, signed_in: bool) -> Step {
        let url = format!("{}/cookie", self.origin);
        let request = if signed_in { HostPageRequest::signed_in(url) } else { HostPageRequest::new(url) };
        match browser.open_host_page_with(request) {
            Ok(id) => Step::Open { id, loaded: browser.host_page_loaded(id), signed_in },
            Err(e) => {
                say(format!("{} error={e}", if signed_in { "signed-in" } else { "ephemeral" }));
                Step::Done
            }
        }
    }
}
