use std::cell::RefCell;
use std::rc::Rc;

use morph_ui::actions::{Action, Keymap};
use morph_webview::fake::{Call, FakeBackend};
use morph_webview::{ErrorKind, Event, Frame, Input, LoadError, Modifiers, MouseButton, Rect, ViewId};

use super::{Browser, Config, Cover, FindBar, error_text};

fn browser(start: &str) -> (Browser, FakeBackend) {
    let fake = FakeBackend::new();
    let browser = Browser::new(Box::new(fake.clone()), Config { start_url: start.into(), ..Config::default() }).unwrap();
    (browser, fake)
}

fn type_into_address(browser: &mut Browser, text: &str) {
    browser.focus_address();
    browser.address_mut().replace(None, text);
    browser.submit_address();
}

#[test]
fn starts_with_one_tab_loading_the_start_address() {
    let (b, fake) = browser("example.com");
    let tabs = b.tabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(b.active_url(), "https://example.com");
    let created = fake.created();
    assert_eq!(created.len(), 1);
    match &fake.calls_for(created[0])[0] {
        Call::Create(spec) => {
            assert_eq!(spec.url, "https://example.com");
            assert!(spec.visible);
        }
        other => panic!("{other:?}"),
    }
    assert!(!b.address_focused());
}

#[test]
fn a_blank_start_focuses_the_address_bar() {
    let (b, _) = browser("");
    assert!(b.address_focused());
    assert_eq!(b.tabs()[0].label, "New tab");
}

#[test]
fn typing_a_phrase_searches_and_an_address_opens() {
    let (mut b, fake) = browser("");
    let view = fake.created()[0];
    type_into_address(&mut b, "what do i need to install to get youtube working");
    assert_eq!(b.active_url(), "https://duckduckgo.com/?q=what+do+i+need+to+install+to+get+youtube+working");
    type_into_address(&mut b, "instagram.com");
    let calls = fake.calls_for(view);
    assert!(calls.contains(&Call::Load("https://instagram.com".into())));
    assert_eq!(calls.last(), Some(&Call::Focus), "the page gets focus after Enter");
    assert!(!b.address_focused());
}

#[test]
fn blank_address_does_nothing() {
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    type_into_address(&mut b, "   ");
    assert!(!fake.calls_for(view).iter().any(|c| matches!(c, Call::Load(_))));
}

#[test]
fn page_events_update_the_tab_and_the_address_bar() {
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    fake.emit(view, Event::LoadStarted);
    fake.emit(view, Event::UrlChanged("https://example.com/".into()));
    fake.emit(view, Event::TitleChanged("Example Domain".into()));
    fake.emit(view, Event::History { can_back: true, can_forward: false });
    let polled = b.poll();
    assert!(polled.changed);
    assert!(b.toolbar().loading);
    assert!(b.toolbar().can_back);
    assert_eq!(b.tabs()[0].label, "Example Domain");
    assert_eq!(b.address().text(), "https://example.com/");
    fake.emit(view, Event::LoadFinished);
    b.poll();
    assert!(!b.toolbar().loading);
}

#[test]
fn typing_in_the_address_bar_is_not_overwritten_by_page_events() {
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    b.focus_address();
    b.address_mut().replace(None, "half typ");
    fake.emit(view, Event::UrlChanged("https://example.com/redirected".into()));
    b.poll();
    assert_eq!(b.address().text(), "half typ");
    b.blur_address();
    assert_eq!(b.address().text(), "https://example.com/redirected");
}

#[test]
fn only_the_active_tab_is_visible_and_closing_selects_the_next() {
    let (mut b, fake) = browser("a.test");
    b.new_tab("b.test");
    let [first, second] = fake.created()[..] else { panic!("two views") };
    assert_eq!(fake.last_visible(first), Some(false));
    assert_eq!(fake.last_visible(second), Some(true));
    assert!(b.tabs()[1].active);
    b.dispatch(Action::SelectTab(0));
    assert_eq!(fake.last_visible(first), Some(true));
    assert_eq!(fake.last_visible(second), Some(false));
    assert_eq!(b.address().text(), "https://a.test");
    b.dispatch(Action::CloseTab);
    assert_eq!(fake.calls_for(first).last(), Some(&Call::Dropped));
    assert_eq!(b.tabs().len(), 1);
    assert_eq!(b.active_url(), "https://b.test");
    assert_eq!(fake.last_visible(second), Some(true));
}

#[test]
fn closing_the_last_tab_leaves_a_blank_one() {
    let (mut b, fake) = browser("a.test");
    b.dispatch(Action::CloseTab);
    assert_eq!(b.tabs().len(), 1);
    assert_eq!(b.tabs()[0].label, "New tab");
    assert_eq!(fake.created().len(), 2);
    assert!(b.address_focused());
}

#[test]
fn tab_shortcuts_cycle_and_select_last() {
    let (mut b, _) = browser("a.test");
    b.new_tab("b.test");
    b.new_tab("c.test");
    b.dispatch(Action::NextTab);
    assert_eq!(b.active_url(), "https://a.test", "wraps around");
    b.dispatch(Action::PrevTab);
    assert_eq!(b.active_url(), "https://c.test");
    b.dispatch(Action::SelectTab(1));
    assert_eq!(b.active_url(), "https://b.test");
    b.dispatch(Action::SelectLastTab);
    assert_eq!(b.active_url(), "https://c.test");
    b.dispatch(Action::SelectTab(7));
    assert_eq!(b.active_url(), "https://c.test", "out of range does nothing");
    assert!(!b.dispatch(Action::Quit), "not the browser's action");
}

#[test]
fn navigation_actions_reach_the_active_page() {
    let (mut b, fake) = browser("a.test");
    let view = fake.created()[0];
    for action in [Action::Back, Action::Forward, Action::Reload, Action::Stop] {
        assert!(b.dispatch(action));
    }
    let calls = fake.calls_for(view);
    for call in [Call::Back, Call::Forward, Call::Reload, Call::Stop] {
        assert!(calls.contains(&call), "{call:?}");
    }
}

#[test]
fn the_page_area_follows_layout_and_hiding_the_component_hides_pages() {
    let (mut b, fake) = browser("a.test");
    let view = fake.created()[0];
    let area = Rect::new(0.0, 80.0, 1000.0, 620.0);
    b.set_page_area(area);
    assert_eq!(fake.last_bounds(view), Some(area));
    b.set_shown(false);
    assert_eq!(fake.last_visible(view), Some(false));
    b.set_shown(true);
    assert_eq!(fake.last_visible(view), Some(true));
}

#[test]
fn a_crashed_page_is_hidden_until_reloaded() {
    let (mut b, fake) = browser("a.test");
    let view = fake.created()[0];
    fake.emit(view, Event::Crashed);
    b.poll();
    assert!(b.toolbar().covered);
    assert_eq!(b.cover(), Some(&Cover::Crashed));
    assert_eq!(fake.last_visible(view), Some(false));
    b.dispatch(Action::Reload);
    assert!(!b.toolbar().covered);
    assert_eq!(fake.last_visible(view), Some(true));
}

fn not_found(url: &str) -> LoadError {
    LoadError { url: url.into(), kind: ErrorKind::NotFound, message: "Error resolving".into() }
}

#[test]
fn a_failed_load_shows_an_error_page_until_the_next_load() {
    let (mut b, fake) = browser("nope.test");
    let view = fake.created()[0];
    fake.emit(view, Event::LoadStarted);
    // WebKit puts the previous address back just before reporting the failure.
    fake.emit(view, Event::UrlChanged("https://before.test/".into()));
    fake.emit(view, Event::LoadFailed(not_found("https://nope.test/")));
    let host_focus = fake.focus_host_count();
    b.poll();
    assert_eq!(b.active_url(), "https://nope.test/", "the tab keeps the address that failed");
    assert_eq!(b.address().text(), "https://nope.test/");
    assert_eq!(b.cover(), Some(&Cover::Failed(not_found("https://nope.test/"))));
    assert!(fake.focus_host_count() > host_focus, "the hidden page does not keep the keyboard");
    assert!(!b.toolbar().loading);
    assert_eq!(fake.last_visible(view), Some(false), "the engine's own error text stays hidden");
    // Try again reloads and shows the page.
    b.dispatch(Action::Reload);
    assert_eq!(b.cover(), None);
    assert_eq!(fake.last_visible(view), Some(true));
    assert_eq!(fake.calls_for(view).last(), Some(&Call::Load("https://nope.test/".into())), "the failed address, not the page before it");
    // Going back from an error page starts a load, which clears it.
    fake.emit(view, Event::LoadFailed(not_found("https://nope.test/")));
    b.poll();
    fake.emit(view, Event::LoadStarted);
    b.poll();
    assert_eq!(b.cover(), None);
    assert_eq!(fake.last_visible(view), Some(true));
}

#[test]
fn back_from_an_error_page_shows_the_page_under_it() {
    let (mut b, fake) = browser("a.test");
    let view = fake.created()[0];
    type_into_address(&mut b, "nope.test");
    fake.emit(view, Event::LoadStarted);
    fake.emit(view, Event::UrlChanged("https://a.test/".into()));
    fake.emit(view, Event::LoadFailed(not_found("https://nope.test")));
    b.poll();
    // Trying again fails again; Back still means the page before.
    b.reload();
    fake.emit(view, Event::LoadStarted);
    fake.emit(view, Event::UrlChanged("https://a.test/".into()));
    fake.emit(view, Event::LoadFailed(not_found("https://nope.test")));
    b.poll();
    let backs = fake.calls_for(view).iter().filter(|c| **c == Call::Back).count();
    b.dispatch(Action::Back);
    assert_eq!(b.cover(), None);
    assert_eq!(b.active_url(), "https://a.test/");
    assert_eq!(b.address().text(), "https://a.test/");
    assert_eq!(fake.last_visible(view), Some(true));
    assert_eq!(fake.calls_for(view).iter().filter(|c| **c == Call::Back).count(), backs, "the engine's Back would skip a page");
    b.dispatch(Action::Back);
    assert_eq!(fake.calls_for(view).last(), Some(&Call::Back));
}

#[test]
fn an_error_in_a_background_tab_keeps_it_hidden_when_selected() {
    let (mut b, fake) = browser("a.test");
    b.new_tab("nope.test");
    let second = fake.created()[1];
    b.dispatch(Action::SelectTab(0));
    fake.emit(second, Event::LoadFailed(not_found("https://nope.test/")));
    b.poll();
    assert_eq!(b.cover(), None, "the active tab is fine");
    b.dispatch(Action::SelectTab(1));
    assert!(matches!(b.cover(), Some(Cover::Failed(_))));
    assert_eq!(fake.last_visible(second), Some(false));
}

#[test]
fn error_pages_explain_what_went_wrong() {
    let (title, detail) = error_text(&not_found("https://exampel.com/x"));
    assert_eq!(title, "Can't find exampel.com");
    assert!(detail.contains("misspelled"));
    let cert =
        LoadError { url: "https://self-signed.test:8443/".into(), kind: ErrorKind::Certificate, message: "The certificate has expired.".into() };
    let (title, detail) = error_text(&cert);
    assert_eq!(title, "This connection isn't private");
    assert!(detail.starts_with("self-signed.test sent"), "{detail}");
    assert!(detail.contains("expired. Check the address"), "{detail}");
    let other = LoadError { url: "about:x".into(), kind: ErrorKind::Other, message: String::new() };
    assert_eq!(error_text(&other), ("This page couldn't load".into(), "Try again in a moment.".into()));
    let slow = LoadError { url: "http://user@slow.test:81".into(), kind: ErrorKind::TimedOut, message: String::new() };
    assert_eq!(error_text(&slow).0, "slow.test took too long to respond");
}

#[test]
fn a_hung_page_can_be_waited_for_or_reloaded() {
    let (mut b, fake) = browser("busy.test");
    let view = fake.created()[0];
    fake.emit(view, Event::Unresponsive(true));
    b.poll();
    assert_eq!(b.cover(), Some(&Cover::Unresponsive));
    assert_eq!(fake.last_visible(view), Some(false));
    // Wait shows the page and does not cover it again while still hung.
    b.wait();
    assert_eq!(b.cover(), None);
    assert_eq!(fake.last_visible(view), Some(true));
    fake.emit(view, Event::Unresponsive(true));
    b.poll();
    assert_eq!(b.cover(), None);
    // Once it answered, a new hang covers it again.
    fake.emit(view, Event::Unresponsive(false));
    fake.emit(view, Event::Unresponsive(true));
    b.poll();
    assert_eq!(b.cover(), Some(&Cover::Unresponsive));
    // Reload ends the hung process and reloads; its crash report is expected.
    b.dispatch(Action::Reload);
    let calls = fake.calls_for(view);
    assert!(calls.ends_with(&[Call::Terminate, Call::Bounds(Rect::default()), Call::Visible(true), Call::Reload]), "{calls:?}");
    fake.emit(view, Event::Crashed);
    b.poll();
    assert_eq!(b.cover(), None, "the deliberate end is not a crash");
    fake.emit(view, Event::Crashed);
    b.poll();
    assert_eq!(b.cover(), Some(&Cover::Crashed), "a later crash is");
}

#[test]
fn a_page_that_answers_again_is_uncovered() {
    let (mut b, fake) = browser("busy.test");
    let view = fake.created()[0];
    fake.emit(view, Event::Unresponsive(true));
    fake.emit(view, Event::Unresponsive(false));
    b.poll();
    assert_eq!(b.cover(), None);
    assert_eq!(fake.last_visible(view), Some(true));
}

#[test]
fn find_searches_the_active_page_and_escape_returns_to_it() {
    let (mut b, fake) = browser("a.test");
    let view = fake.created()[0];
    let host_focus = fake.focus_host_count();
    assert!(b.dispatch(Action::Find));
    assert_eq!(b.find(), Some(&FindBar::default()));
    assert!(fake.focus_host_count() > host_focus, "the find bar takes the keyboard from the page");
    b.set_find_query("morph");
    assert_eq!(fake.calls_for(view).last(), Some(&Call::Find("morph".into())));
    fake.emit(view, Event::FindResult { matches: 3 });
    b.poll();
    assert_eq!(b.find().unwrap().matches, Some(3));
    assert!(b.dispatch(Action::FindNext));
    assert_eq!(fake.calls_for(view).last(), Some(&Call::FindNext(true)));
    b.find_next(false);
    assert_eq!(fake.calls_for(view).last(), Some(&Call::FindNext(false)));
    // The same text again searches nothing new; an empty one clears.
    b.set_find_query("morph");
    assert_eq!(fake.calls_for(view).last(), Some(&Call::FindNext(false)));
    b.set_find_query("");
    assert_eq!(fake.calls_for(view).last(), Some(&Call::FindClear));
    assert_eq!(b.find().unwrap().matches, None);
    b.set_find_query("x");
    b.close_find();
    assert_eq!(b.find(), None);
    let calls = fake.calls_for(view);
    assert_eq!(&calls[calls.len() - 2..], &[Call::FindClear, Call::Focus]);
}

#[test]
fn find_next_opens_a_closed_find_bar_and_keeps_its_text_when_reopened() {
    let (mut b, fake) = browser("a.test");
    let view = fake.created()[0];
    assert!(b.dispatch(Action::FindPrevious));
    assert!(b.find().is_some());
    assert!(!fake.calls_for(view).iter().any(|c| matches!(c, Call::FindNext(_))), "nothing to search yet");
    b.set_find_query("tab");
    b.dispatch(Action::Find);
    assert_eq!(b.find().unwrap().query, "tab");
}

#[test]
fn find_follows_the_active_tab() {
    let (mut b, fake) = browser("a.test");
    b.new_tab("b.test");
    let (first, second) = (fake.created()[0], fake.created()[1]);
    b.dispatch(Action::Find);
    b.set_find_query("word");
    fake.emit(second, Event::FindResult { matches: 2 });
    b.poll();
    b.dispatch(Action::SelectTab(0));
    assert_eq!(b.find().unwrap().matches, None);
    assert_eq!(fake.calls_for(first).last(), Some(&Call::Find("word".into())));
    assert_eq!(fake.calls_for(second).last(), Some(&Call::FindClear));
    // Results from a background tab are ignored.
    fake.emit(second, Event::FindResult { matches: 9 });
    b.poll();
    assert_eq!(b.find().unwrap().matches, None);
}

#[test]
fn opening_find_leaves_the_address_bar() {
    let (mut b, _) = browser("a.test");
    b.focus_address();
    b.dispatch(Action::Find);
    assert!(!b.address_focused());
}

#[test]
fn popups_are_counted_per_tab() {
    let (mut b, fake) = browser("a.test");
    let view = fake.created()[0];
    fake.emit(view, Event::PopupOpened { url: "https://login.test/".into() });
    b.poll();
    assert_eq!(b.toolbar().popups, 1);
    fake.emit(view, Event::PopupClosed);
    fake.emit(view, Event::PopupClosed);
    b.poll();
    assert_eq!(b.toolbar().popups, 0);
}

#[test]
fn events_from_closed_tabs_are_ignored_and_shortcuts_pass_through() {
    let (mut b, fake) = browser("a.test");
    b.new_tab("b.test");
    let first = fake.created()[0];
    b.select_index(0);
    b.dispatch(Action::CloseTab);
    fake.emit(first, Event::PopupClosed);
    fake.emit(ViewId(99), Event::LoadFinished);
    fake.emit(fake.created()[1], Event::Shortcut("ctrl-t".into()));
    let polled = b.poll();
    assert!(!polled.changed);
    assert_eq!(polled.shortcuts, vec!["ctrl-t".to_string()]);
}

#[test]
fn finished_loads_are_recorded_as_history_but_not_blank_pages() {
    let seen: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
    let sink = seen.clone();
    let fake = FakeBackend::new();
    let mut b = Browser::new(
        Box::new(fake.clone()),
        Config { start_url: "a.test".into(), history: Some(Box::new(move |u, t| sink.borrow_mut().push((u.into(), t.into())))), ..Config::default() },
    )
    .unwrap();
    let view = fake.created()[0];
    fake.emit(view, Event::UrlChanged("https://a.test/".into()));
    fake.emit(view, Event::LoadFinished);
    fake.emit(view, Event::TitleChanged("Alpha".into()));
    b.poll();
    assert_eq!(*seen.borrow(), vec![("https://a.test/".into(), "".into()), ("https://a.test/".into(), "Alpha".into())]);
    b.load("about:blank");
    fake.emit(view, Event::UrlChanged("about:blank".into()));
    fake.emit(view, Event::LoadFinished);
    b.poll();
    assert_eq!(seen.borrow().len(), 2);
}

#[test]
fn focusing_the_address_bar_takes_focus_from_the_page() {
    let (mut b, fake) = browser("a.test");
    b.set_shortcuts(&Keymap::default().bindings().iter().map(|k| k.keystroke.clone()).collect::<Vec<_>>());
    b.dispatch(Action::FocusAddress);
    assert!(b.address_focused());
    assert_eq!(b.address().selected_text(), Some("https://a.test"));
    assert_eq!(fake.focus_host_count(), 1);
}

#[test]
fn clicking_into_the_page_blurs_the_address_bar() {
    let (mut b, fake) = browser("a.test");
    let view = fake.created()[0];
    b.focus_address();
    b.address_mut().replace(None, "draft");
    fake.emit(view, Event::Focused);
    b.poll();
    assert!(!b.address_focused());
    assert_eq!(b.address().text(), "https://a.test");
}

fn drawing_browser(start: &str) -> (Browser, FakeBackend) {
    let fake = FakeBackend::drawing();
    let b = Browser::new(Box::new(fake.clone()), Config { start_url: start.into(), ..Config::default() }).unwrap();
    (b, fake)
}

fn frame(width: u32) -> Frame {
    Frame { width, height: 1, pixels: vec![0; width as usize * 4] }
}

#[test]
fn drawn_pages_hand_out_the_active_tabs_frames_and_input() {
    let (mut b, fake) = drawing_browser("a.test");
    assert!(b.draws_pages());
    let first = fake.created()[0];
    fake.push_frame(first, frame(3));
    assert_eq!(b.take_frame(), Some(frame(3)));
    assert_eq!(b.take_frame(), None, "nothing new");
    let click = Input::PointerButton { x: 1.0, y: 2.0, button: MouseButton::Left, pressed: true, modifiers: Modifiers::default() };
    b.page_input(click.clone());
    assert_eq!(fake.inputs(first), vec![click.clone()]);
    b.new_tab("b.test");
    let second = fake.created()[1];
    fake.push_frame(first, frame(4));
    assert_eq!(b.take_frame(), None, "a background tab's frame is not drawn");
    b.page_input(click.clone());
    assert_eq!(fake.inputs(second), vec![click]);
}

#[test]
fn a_covered_page_gets_no_input_and_draws_nothing() {
    let (mut b, fake) = drawing_browser("a.test");
    let view = fake.created()[0];
    fake.emit(view, Event::Crashed);
    b.poll();
    fake.push_frame(view, frame(2));
    assert_eq!(b.take_frame(), None);
    b.page_input(Input::PointerLeave);
    assert!(fake.inputs(view).is_empty());
}

#[test]
fn the_keyboard_follows_page_and_shell_focus() {
    let (mut b, fake) = drawing_browser("a.test");
    b.focus_page();
    assert!(b.page_has_keyboard());
    b.focus_address();
    assert!(!b.page_has_keyboard());
    b.close_find();
    b.dispatch(Action::Find);
    assert!(!b.page_has_keyboard(), "the find bar takes the keyboard");
    b.close_find();
    assert!(b.page_has_keyboard(), "closing it gives it back");
    let _ = fake;
}

#[test]
fn window_backends_leave_the_keyboard_to_the_window_system() {
    let (mut b, _fake) = browser("a.test");
    assert!(!b.draws_pages());
    b.focus_page();
    assert!(!b.page_has_keyboard());
    assert_eq!(b.take_frame(), None);
}
