use std::cell::RefCell;
use std::rc::Rc;

use morph_ui::actions::{Action, Keymap};
use morph_webview::fake::{Call, FakeBackend};
use morph_webview::{ErrorKind, Event, Frame, Input, LoadError, Modifiers, MouseButton, Rect, ViewId};

use super::{ABOUT_URL, Browser, Config, Cover, FindBar, Internal, SETTINGS_URL, error_text, start_address};

fn browser(start: &str) -> (Browser, FakeBackend) {
    let fake = FakeBackend::new();
    let browser = Browser::new(Box::new(fake.clone()), Config { start_url: start.into(), ..Config::default() }).unwrap();
    (browser, fake)
}

fn type_into_address(browser: &mut Browser, text: &str) {
    browser.focus_address();
    browser.address_mut().replace(None, text);
    let _ = browser.submit_address();
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
    assert_eq!(b.active_url(), "https://www.google.com/search?q=what+do+i+need+to+install+to+get+youtube+working");
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
fn closing_the_last_tab_asks_to_quit() {
    let (mut b, fake) = browser("a.test");
    b.new_tab("b.test");
    b.dispatch(Action::CloseTab);
    assert!(!b.take_quit(), "other tabs are left");
    assert_eq!(b.tabs().len(), 1);
    let only = b.tabs()[0].id;
    b.close_tab(only);
    assert!(b.take_quit());
    assert!(!b.take_quit(), "asked once");
    assert_eq!(b.tabs().len(), 1, "the tab stays until the shell quits");
    assert_eq!(fake.created().len(), 2);
}

#[test]
fn tabs_search_with_their_own_engine_and_new_tabs_use_the_default() {
    let (mut b, _) = browser("example.com");
    assert_eq!(b.tab_engine().id, "google");
    b.set_tab_engine("duckduckgo");
    type_into_address(&mut b, "rust lang");
    assert_eq!(b.active_url(), "https://duckduckgo.com/?q=rust+lang");
    b.new_tab("");
    assert_eq!(b.tab_engine().id, "google", "a new tab uses the default");
    type_into_address(&mut b, "rust lang");
    assert_eq!(b.active_url(), "https://www.google.com/search?q=rust+lang");
    b.dispatch(Action::PrevTab);
    assert_eq!(b.tab_engine().id, "duckduckgo", "the override stays with its tab");
    b.set_default_engine("duckduckgo");
    b.dispatch(Action::NewTab);
    assert_eq!(b.tab_engine().id, "duckduckgo");
}

#[test]
fn settings_open_as_a_page_and_an_open_one_is_reused() {
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    b.dispatch(Action::ShowSettings);
    assert_eq!(b.tabs().len(), 2);
    assert_eq!(b.active_url(), SETTINGS_URL);
    assert_eq!(b.active_title(), "Settings");
    assert_eq!(b.cover(), Some(&Cover::Settings));
    b.dispatch(Action::PrevTab);
    assert_eq!(b.cover(), None);
    assert_eq!(fake.last_visible(view), Some(true));
    b.dispatch(Action::ShowSettings);
    assert_eq!(b.tabs().len(), 2, "the open settings tab is selected");
    assert_eq!(b.cover(), Some(&Cover::Settings));
    b.focus_address();
    b.address_mut().replace(None, "morph://settings");
    assert_eq!(b.submit_address(), None, "morph://settings shows here, not in a panel");
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

fn select_menu() -> morph_webview::OptionMenu {
    let item = |label: &str, selected| morph_webview::MenuItem { label: label.into(), enabled: true, selected, ..Default::default() };
    morph_webview::OptionMenu {
        items: vec![item("Apple", false), item("Banana", true), item("Cherry", false)],
        anchor: Rect::new(40.0, 40.0, 160.0, 30.0),
    }
}

fn menu_calls(fake: &FakeBackend, view: ViewId) -> Vec<morph_webview::MenuChoice> {
    fake.calls_for(view).into_iter().filter_map(|c| if let Call::OptionMenu(m) = c { Some(m) } else { None }).collect()
}

#[test]
fn a_select_list_opens_for_the_shell_and_keys_choose_from_it() {
    use super::MenuKey;
    use morph_webview::MenuChoice;
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    assert!(!b.menu_key(MenuKey::Down), "no list, the key is the page's");
    fake.emit(view, Event::OptionMenu(select_menu()));
    b.poll();
    let menu = b.option_menu().expect("open");
    assert_eq!(menu.highlighted, Some(1));
    assert_eq!(menu.menu.anchor, Rect::new(40.0, 40.0, 160.0, 30.0));
    assert!(b.menu_key(MenuKey::Down));
    assert!(b.menu_key(MenuKey::Enter));
    assert_eq!(menu_calls(&fake, view), vec![MenuChoice::Highlight(2), MenuChoice::Activate(2)]);
    assert!(b.option_menu().is_none(), "choosing closes it");
}

#[test]
fn a_click_chooses_and_a_click_outside_or_a_new_page_cancels() {
    use morph_webview::MenuChoice;
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    fake.emit(view, Event::OptionMenu(select_menu()));
    b.poll();
    b.menu_hover(0);
    b.menu_click(0);
    fake.emit(view, Event::OptionMenu(select_menu()));
    b.poll();
    b.menu_cancel();
    fake.emit(view, Event::OptionMenu(select_menu()));
    b.poll();
    b.load("https://example.org");
    assert_eq!(menu_calls(&fake, view), vec![MenuChoice::Highlight(0), MenuChoice::Activate(0), MenuChoice::Cancel, MenuChoice::Cancel]);
    assert!(b.option_menu().is_none());
}

#[test]
fn the_page_closing_its_list_or_a_tab_switch_drops_it() {
    use morph_webview::MenuChoice;
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    fake.emit(view, Event::OptionMenu(select_menu()));
    b.poll();
    fake.emit(view, Event::OptionMenuClosed);
    b.poll();
    assert!(b.option_menu().is_none());
    fake.emit(view, Event::OptionMenu(select_menu()));
    b.poll();
    b.dispatch(Action::NewTab);
    assert!(b.option_menu().is_none());
    assert_eq!(menu_calls(&fake, view), vec![MenuChoice::Cancel]);
}

#[test]
fn tooltips_follow_the_page() {
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    fake.emit(view, Event::Tooltip(Some("Sort order".into())));
    b.poll();
    assert_eq!(b.tooltip(), Some("Sort order"));
    fake.emit(view, Event::Tooltip(None));
    b.poll();
    assert_eq!(b.tooltip(), None);
}

#[test]
fn morph_addresses_name_morphs_pages() {
    assert_eq!(Internal::parse("morph://about"), Some(Internal::About));
    assert_eq!(Internal::parse(" MORPH://Settings/ "), Some(Internal::Settings));
    assert_eq!(Internal::parse("morph:history"), Some(Internal::History));
    assert_eq!(Internal::parse("morph://nope"), None);
    assert_eq!(Internal::parse("morphology"), None);
    for i in [Internal::About, Internal::Bookmarks, Internal::History, Internal::Identity, Internal::Profiles, Internal::Settings] {
        assert_eq!(Internal::parse(i.url()), Some(i));
    }
    assert_eq!(Internal::Bookmarks.action(), Action::ShowBookmarks);
}

#[test]
fn panel_addresses_go_to_the_shell_and_about_shows_in_the_tab() {
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    b.focus_address();
    b.address_mut().replace(None, "morph://bookmarks");
    assert_eq!(b.submit_address(), Some(Internal::Bookmarks));
    assert!(!b.address_focused());
    assert_eq!(b.active_url(), "https://example.com", "the tab stays where it was");
    b.focus_address();
    b.address_mut().replace(None, "morph://about");
    assert_eq!(b.submit_address(), None);
    assert_eq!(b.active_url(), ABOUT_URL);
    assert_eq!(b.active_title(), "About Morph");
    assert_eq!(b.cover(), Some(&Cover::About));
    assert_eq!(fake.last_visible(view), Some(false), "the page hides under it");
    fake.emit(view, Event::TitleChanged("Example".into()));
    b.poll();
    assert_eq!(b.active_title(), "About Morph", "the page beneath does not rename the tab");
    b.back();
    assert_eq!(b.cover(), None);
    assert_eq!(b.active_url(), "https://example.com");
    assert_eq!(b.active_title(), "Example", "Back gives the tab the page's title again");
    assert_eq!(fake.last_visible(view), Some(true));
}

#[test]
fn the_about_action_opens_a_tab_unless_the_tab_is_blank() {
    let (mut b, _) = browser("");
    b.dispatch(Action::ShowAbout);
    assert_eq!(b.tabs().len(), 1);
    assert_eq!(b.active_url(), ABOUT_URL);
    let (mut b, _) = browser("example.com");
    b.dispatch(Action::ShowAbout);
    assert_eq!(b.tabs().len(), 2);
    assert_eq!(b.active_url(), ABOUT_URL);
    b.load("https://example.org");
    assert_eq!(b.cover(), None, "loading an address leaves the About page");
}

#[test]
fn the_tab_menu_duplicates_reloads_and_closes_the_others() {
    let (mut b, fake) = browser("example.com");
    b.new_tab("example.org");
    b.new_tab("example.net");
    let ids: Vec<_> = b.tabs().iter().map(|t| t.id).collect();
    b.duplicate_tab(ids[0]);
    assert_eq!(b.tabs().len(), 4);
    assert_eq!(b.active_url(), "https://example.com");
    b.reload_tab(ids[1]);
    assert_eq!(b.active_url(), "https://example.org");
    assert!(fake.calls_for(fake.created()[1]).contains(&Call::Reload));
    b.close_other_tabs(ids[1]);
    assert_eq!(b.tabs().len(), 1);
    assert_eq!(b.active_url(), "https://example.org");
}

#[test]
fn icons_are_kept_per_site_and_shared_by_its_tabs() {
    let (mut b, fake) = browser("https://a.test/x");
    let view = fake.created()[0];
    fake.emit(view, Event::UrlChanged("https://a.test/x".into()));
    fake.emit(view, Event::Favicon(vec![1, 2, 3]));
    b.poll();
    assert_eq!(b.tabs()[0].icon.as_deref(), Some(&vec![1, 2, 3]));
    b.new_tab("https://a.test/other");
    let second = fake.created()[1];
    fake.emit(second, Event::UrlChanged("https://a.test/other".into()));
    b.poll();
    assert_eq!(b.tabs()[1].icon.as_deref(), Some(&vec![1, 2, 3]), "same site, same icon");
    fake.emit(second, Event::UrlChanged("https://b.test/".into()));
    b.poll();
    assert_eq!(b.tabs()[1].icon, None, "another site has none yet");
    b.clear_site_data();
    assert_eq!(b.tabs()[0].icon, None);
    assert_eq!(super::origin("https://User@A.test:8080/p?q#h").as_deref(), Some("https://a.test:8080"));
    assert_eq!(super::origin("file:///tmp/x"), None);
}

#[test]
fn a_page_asking_for_files_is_answered_once() {
    let (mut b, fake) = browser("https://a.test/");
    let view = fake.created()[0];
    assert_eq!(b.take_file_request(), None);
    fake.emit(view, Event::FileChooser { multiple: true });
    b.poll();
    assert_eq!(b.take_file_request(), Some(true));
    assert_eq!(b.take_file_request(), None, "the dialog is already open");
    b.choose_files(Some(vec!["/tmp/a.txt".into()]));
    assert!(fake.calls_for(view).contains(&Call::ChooseFiles(Some(vec!["/tmp/a.txt".into()]))));
    b.choose_files(None);
    assert_eq!(fake.calls_for(view).iter().filter(|c| matches!(c, Call::ChooseFiles(_))).count(), 1, "answered once");
}

#[test]
fn a_start_page_keeps_morph_pages_and_links_and_searches_only_phrases() {
    let engine = morph_core::search::engine("duckduckgo");
    assert_eq!(start_address("morph://about", engine), ABOUT_URL);
    assert_eq!(start_address(" MORPH:settings/ ", engine), SETTINGS_URL);
    assert_eq!(start_address("morph://history", engine), "morph://history");
    assert_eq!(start_address("weirdnet-morph://peer/site", engine), "weirdnet-morph://peer/site");
    assert_eq!(start_address("file:///home/me/start.html", engine), "file:///home/me/start.html");
    assert_eq!(start_address("about:blank", engine), "about:blank");
    assert_eq!(start_address("example.com", engine), "https://example.com");
    assert_eq!(start_address("  ", engine), "");
    assert!(start_address("rust gpui", engine).starts_with("https://duckduckgo.com/?q=rust+gpui"));
    // Unknown morph:// pages and Weirdnet app links open Morph's error
    // page, never a search.
    assert_eq!(start_address("morph://nope", engine), "morph://nope");
    assert_eq!(start_address("weirdnet-muninn://alice/notes", engine), "weirdnet-muninn://alice/notes");
}

#[test]
fn a_morph_start_page_opens_as_morphs_own_page_not_a_search() {
    let (b, fake) = browser("morph://about");
    assert_eq!(b.active_url(), ABOUT_URL);
    assert_eq!(b.cover(), Some(&Cover::About));
    assert!(!fake.calls().iter().any(|c| format!("{c:?}").contains("duckduckgo")), "nothing searched");
    let (b, _) = browser("morph:settings");
    assert_eq!(b.cover(), Some(&Cover::Settings));
}

#[test]
fn a_tab_with_nothing_in_it_shows_the_start_screen() {
    let (mut b, fake) = browser("");
    let view = fake.created()[0];
    assert_eq!(b.cover(), Some(&Cover::Start));
    assert_eq!(fake.last_visible(view), Some(false), "the empty web view stays hidden");
    assert!(b.tabs()[0].own);

    // An address replaces it with the page.
    type_into_address(&mut b, "example.com");
    assert_eq!(b.cover(), None);
    assert_eq!(fake.last_visible(view), Some(true));

    // A new blank tab gets it too; About over it goes back to it.
    b.new_tab("");
    let blank = *fake.created().last().unwrap();
    assert_eq!(b.cover(), Some(&Cover::Start));
    assert_eq!(fake.last_visible(blank), Some(false));
    b.show_about(true);
    assert_eq!(b.cover(), Some(&Cover::About));
    b.dispatch(Action::Back);
    assert_eq!(b.cover(), Some(&Cover::Start));
    assert_eq!(fake.last_visible(blank), Some(false));
    assert_eq!(b.active_url(), "");

    // Reload does not uncover the empty view.
    b.dispatch(Action::Reload);
    assert_eq!(b.cover(), Some(&Cover::Start));
    assert_eq!(fake.last_visible(blank), Some(false));

    // Closing the page tab leaves the blank one; closing everything but a
    // page tab leaves that page.
    b.dispatch(Action::SelectTab(0));
    assert_eq!(b.cover(), None);
    b.dispatch(Action::SelectTab(1));
    assert_eq!(b.cover(), Some(&Cover::Start));
}

/// The engine never got a load for `url`.
fn never_loaded(fake: &FakeBackend, url: &str) -> bool {
    fake.created().iter().all(|v| {
        fake.calls_for(*v).iter().all(|c| match c {
            Call::Load(u) => u != url,
            Call::Create(spec) => spec.url != url,
            _ => true,
        })
    })
}

#[test]
fn an_unknown_morph_page_gets_morphs_error_page_listing_the_real_ones() {
    let (mut b, fake) = browser("example.com");
    let view = fake.created()[0];
    fake.emit(view, Event::TitleChanged("Example".into()));
    b.poll();
    type_into_address(&mut b, "morph://nope");
    assert_eq!(b.active_url(), "morph://nope");
    let Some(Cover::Failed(error)) = b.cover().cloned() else { panic!("{:?}", b.cover()) };
    let (title, body) = error_text(&error);
    assert_eq!(title, "morph://nope is not a Morph page");
    for page in morph_core::address::PAGES {
        assert!(body.contains(&format!("morph://{}", page.name)), "{}", page.name);
    }
    assert_eq!(b.active_title(), title);
    assert!(never_loaded(&fake, "morph://nope"));
    assert!(
        fake.created().iter().all(|v| fake.calls_for(*v).iter().all(|c| !matches!(c, Call::Load(u) if u.contains("duckduckgo")))),
        "not a search"
    );
    b.back();
    assert_eq!(b.active_url(), "https://example.com", "Back shows the page under it");
    assert_eq!(b.active_title(), "Example", "with its title");
    assert_eq!(b.cover(), None);
}

/// Poll until every app call has finished.
fn settle(b: &mut Browser) {
    for _ in 0..400 {
        b.poll();
        if b.app_loads.is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("an app call never finished");
}

#[test]
fn a_weirdnet_app_link_without_a_network_says_so_instead_of_searching() {
    for url in ["weirdnet-muninn://home.example.com/notes/a.md?token=x", "Weirdnet-Notes://alice", "weirdnet-morph://peer/site"] {
        let (mut b, fake) = browser("example.com");
        type_into_address(&mut b, url);
        assert_eq!(b.active_url(), url);
        assert!(b.toolbar().loading, "{url}: the tab loads while the call runs");
        settle(&mut b);
        assert!(!b.toolbar().loading);
        let Some(Cover::Failed(error)) = b.cover().cloned() else { panic!("{url}: {:?}", b.cover()) };
        let (title, body) = error_text(&error);
        assert_eq!(title, "App calls aren't available", "{url}");
        assert!(!body.contains("token=x"), "no query on the page");
        assert!(never_loaded(&fake, url), "{url} never reaches the engine (or an OS handler through it)");
        // Reload asks again, still without the engine.
        b.reload();
        settle(&mut b);
        assert!(matches!(b.cover(), Some(Cover::Failed(_))));
        assert!(never_loaded(&fake, url));
    }
    let (mut b, _) = browser("example.com");
    type_into_address(&mut b, "weirdnet://internal/x");
    let Some(Cover::Failed(error)) = b.cover().cloned() else { panic!() };
    assert_eq!(error_text(&error).0, "weirdnet:// can't be opened");
    type_into_address(&mut b, "weirdnet-No.Good://alice/x");
    let Some(Cover::Failed(error)) = b.cover().cloned() else { panic!("an invalid app name fails at once: {:?}", b.cover()) };
    assert_eq!(error_text(&error).0, "Not a valid Weirdnet app name");
}

#[test]
fn app_links_open_in_new_tabs_and_at_start_without_the_engine() {
    let (mut b, fake) = browser("weirdnet-notes://alice/x");
    assert_eq!(b.active_url(), "weirdnet-notes://alice/x");
    settle(&mut b);
    assert!(matches!(b.cover(), Some(Cover::Failed(_))));
    b.new_tab("morph://nope");
    assert_eq!(b.active_url(), "morph://nope");
    assert!(matches!(b.cover(), Some(Cover::Failed(_))));
    b.load("weirdnet-notes://bob/");
    assert_eq!(b.active_url(), "weirdnet-notes://bob/");
    settle(&mut b);
    for url in ["weirdnet-notes://alice/x", "morph://nope", "weirdnet-notes://bob/"] {
        assert!(never_loaded(&fake, url), "{url}");
    }
}

/// A caller that records requests and answers from a fixed list.
struct Scripted {
    seen: std::sync::Mutex<Vec<morph_core::app_call::AppRequest>>,
    answer: fn(&morph_core::app_call::AppRequest) -> Result<morph_core::app_call::AppResponse, morph_core::app_call::AppCallError>,
    /// Held until released, to see a call under way.
    gate: std::sync::Mutex<()>,
}

impl morph_core::app_call::AppCaller for Scripted {
    fn call(&self, r: morph_core::app_call::AppRequest) -> Result<morph_core::app_call::AppResponse, morph_core::app_call::AppCallError> {
        self.seen.lock().unwrap().push(r.clone());
        let _open = self.gate.lock().unwrap();
        (self.answer)(&r)
    }
}

/// Pages the browser recorded in history: (address, title).
type Visits = Rc<RefCell<Vec<(String, String)>>>;

fn with_caller(
    start: &str,
    answer: fn(&morph_core::app_call::AppRequest) -> Result<morph_core::app_call::AppResponse, morph_core::app_call::AppCallError>,
) -> (Browser, FakeBackend, std::sync::Arc<Scripted>, Visits) {
    let caller = std::sync::Arc::new(Scripted { seen: Default::default(), answer, gate: Default::default() });
    let fake = FakeBackend::new();
    let visits: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
    let sink = visits.clone();
    let config = Config {
        start_url: start.into(),
        app_caller: Some(caller.clone()),
        history: Some(Box::new(move |u: &str, t: &str| sink.borrow_mut().push((u.into(), t.into())))),
        ..Config::default()
    };
    let b = Browser::new(Box::new(fake.clone()), config).unwrap();
    (b, fake, caller, visits)
}

fn html_answer(r: &morph_core::app_call::AppRequest) -> Result<morph_core::app_call::AppResponse, morph_core::app_call::AppCallError> {
    let body = format!("<html><head><title>Notes of {}</title></head><body>{}</body></html>", r.locator, r.path).into_bytes();
    Ok(morph_core::app_call::AppResponse {
        status: 200,
        headers: vec![("content-type".into(), "text/html".into())],
        body: Box::new(std::io::Cursor::new(body)),
    })
}

#[test]
fn a_plugged_in_app_caller_gets_the_request_and_its_error_shows() {
    let (mut b, _, caller, _) = with_caller("example.com", |_| Err(morph_core::app_call::AppCallError::NotFound("alice".into())));
    type_into_address(&mut b, "weirdnet-notes://alice/a?b=1");
    settle(&mut b);
    let r = caller.seen.lock().unwrap().pop().unwrap();
    assert_eq!((r.app.as_str(), r.locator.as_str(), r.method.as_str(), r.path.as_str()), ("weirdnet-notes", "alice", "GET", "/a?b=1"));
    let Some(Cover::Failed(error)) = b.cover().cloned() else { panic!() };
    assert_eq!(error_text(&error).0, "Couldn't find alice");
    assert!(error_text(&error).1.contains("not sent to a search engine"));
    assert_eq!(b.active_title(), "Couldn't find alice");
    b.back();
    assert_eq!(b.active_url(), "https://example.com", "Back shows the page under it");
    assert_eq!(b.cover(), None);
}

#[test]
fn an_apps_answer_loads_in_the_tab_under_the_app_address() {
    let (mut b, fake, _, visits) = with_caller("example.com", html_answer);
    let view = fake.created()[0];
    type_into_address(&mut b, "weirdnet-notes://Alice/dir/a?x=1");
    settle(&mut b);
    let Some(Call::Load(data)) = fake.calls_for(view).into_iter().rev().find(|c| matches!(c, Call::Load(_))) else { panic!() };
    assert!(data.starts_with("data:text/html;charset=utf-8;morph-app="), "{}", &data[..50]);
    assert_eq!(b.cover(), None);
    assert_eq!(b.active_url(), "weirdnet-notes://Alice/dir/a?x=1");
    assert_eq!(b.active_title(), "Notes of Alice");
    // The engine reports the data: address; the tab keeps the app's.
    fake.emit(view, Event::LoadStarted);
    fake.emit(view, Event::UrlChanged(data.clone()));
    fake.emit(view, Event::TitleChanged("Notes of Alice".into()));
    fake.emit(view, Event::LoadFinished);
    b.poll();
    assert_eq!(b.active_url(), "weirdnet-notes://Alice/dir/a?x=1");
    assert_eq!(b.address().text(), "weirdnet-notes://Alice/dir/a?x=1");
    assert_eq!(
        visits.borrow().last().cloned(),
        Some(("weirdnet-notes://Alice/dir/a?x=1".into(), "Notes of Alice".into())),
        "history has the app address"
    );
    // Reload calls the app again rather than reloading the copy.
    let loads = fake.calls_for(view).iter().filter(|c| matches!(c, Call::Load(_))).count();
    b.reload();
    assert!(b.toolbar().loading);
    settle(&mut b);
    assert_eq!(fake.calls_for(view).iter().filter(|c| matches!(c, Call::Load(_))).count(), loads + 1);
    assert!(!fake.calls_for(view).contains(&Call::Reload));
    // A link on the page to another app address is called too.
    fake.emit(
        view,
        Event::LoadFailed(LoadError { url: "weirdnet-notes://Alice/dir/b".into(), kind: ErrorKind::Other, message: "unsupported".into() }),
    );
    b.poll();
    assert_eq!(b.active_url(), "weirdnet-notes://Alice/dir/b");
    settle(&mut b);
    assert_eq!(b.cover(), None);
    assert_eq!(b.active_title(), "Notes of Alice");
}

#[test]
fn stop_and_going_elsewhere_drop_a_call_under_way() {
    let (mut b, _, caller, _) = with_caller("example.com", html_answer);
    let held = caller.gate.lock().unwrap();
    type_into_address(&mut b, "weirdnet-notes://alice/slow");
    assert_eq!(b.active_url(), "weirdnet-notes://alice/slow");
    assert!(b.toolbar().loading && b.app_call_progress() == Some(0));
    b.dispatch(Action::Stop);
    assert_eq!(b.active_url(), "https://example.com", "Stop puts the address back");
    assert!(!b.toolbar().loading && b.app_call_progress().is_none());
    type_into_address(&mut b, "weirdnet-notes://alice/slow");
    type_into_address(&mut b, "example.org");
    assert!(b.app_loads.is_empty(), "a new address drops the call");
    drop(held);
    for _ in 0..20 {
        b.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(b.active_url(), "https://example.org");
    assert_eq!(b.cover(), None);
}

#[test]
fn settings_parts_have_their_own_morph_pages() {
    let (mut b, _) = browser("example.com");
    for (text, internal, section) in [
        ("morph://relays", Internal::Relays, Some("relays")),
        ("morph://downloads", Internal::Downloads, Some("downloads")),
        ("morph://search", Internal::Search, Some("search")),
        ("morph://history", Internal::History, None),
        ("morph://identity", Internal::Identity, None),
    ] {
        b.focus_address();
        b.address_mut().replace(None, text);
        assert_eq!(b.submit_address(), Some(internal), "{text}");
        assert_eq!(internal.settings_section(), section);
        assert_eq!(internal.url(), text);
    }
    assert_eq!(b.active_url(), "https://example.com");
}

#[test]
fn link_clicks_open_tabs_and_shift_clicks_ask_for_a_window() {
    let (mut b, fake) = browser("https://a.test/");
    let view = fake.created()[0];
    fake.emit(view, Event::OpenLink { url: "https://b.test/".into(), target: morph_webview::LinkTarget::NewTab });
    b.poll();
    assert_eq!(b.tabs().len(), 2);
    assert_eq!(b.active_url(), "https://b.test/");
    assert!(b.take_new_windows().is_empty());
    fake.emit(view, Event::OpenLink { url: "https://c.test/".into(), target: morph_webview::LinkTarget::NewWindow });
    b.poll();
    assert_eq!(b.tabs().len(), 2, "a window, not a tab");
    assert_eq!(b.take_new_windows(), vec!["https://c.test/".to_string()]);
    assert!(b.take_new_windows().is_empty(), "taken once");
}

#[test]
fn the_shell_gets_the_link_menu_of_the_active_page_only() {
    let (mut b, fake) = browser("https://a.test/");
    let first = fake.created()[0];
    b.new_tab("https://b.test/");
    fake.emit(first, Event::LinkMenu { url: "https://x.test/".into(), x: 1.0, y: 2.0 });
    b.poll();
    assert_eq!(b.take_link_menu(), None, "a background tab's menu is dropped");
    let second = fake.created()[1];
    fake.emit(second, Event::LinkMenu { url: "https://y.test/".into(), x: 3.0, y: 4.0 });
    b.poll();
    assert_eq!(b.take_link_menu(), Some(("https://y.test/".into(), 3.0, 4.0)));
}

#[test]
fn a_detached_tab_leaves_with_its_history_and_starts_another_window_from_it() {
    let (mut b, fake) = browser("https://a.test/");
    assert_eq!(b.detach_tab(b.tabs()[0].id), None, "the only tab stays (its window would move)");
    b.new_tab("https://b.test/");
    b.set_tab_engine("duckduckgo");
    let moved = b.tabs()[1].id;
    let tab = b.detach_tab(moved).unwrap();
    assert_eq!(tab.url, "https://b.test/");
    assert_eq!(tab.session.as_deref(), Some(&b"fake-session:https://b.test/"[..]));
    assert_eq!(tab.engine.as_deref(), Some("duckduckgo"));
    assert_eq!(b.tabs().len(), 1);
    assert!(!b.take_quit(), "moving a tab out never quits");
    assert!(fake.calls_for(fake.created()[1]).contains(&Call::Dropped), "its page here is gone");
    // The other window starts from that history.
    let other = FakeBackend::new();
    let b2 = Browser::new(Box::new(other.clone()), Config { start_url: tab.url.clone(), start_session: tab.session.clone(), ..Config::default() })
        .unwrap();
    assert_eq!(b2.active_url(), "https://b.test/");
    match &other.calls_for(other.created()[0])[0] {
        Call::Create(spec) => assert_eq!(spec.session, tab.session),
        c => panic!("{c:?}"),
    }
}

#[test]
fn own_pages_move_by_address_and_a_saved_window_reopens_in_order() {
    let (mut b, _) = browser("https://a.test/");
    b.new_tab("morph://about");
    let about = b.tabs()[1].id;
    let tab = b.detach_tab(about).unwrap();
    assert_eq!(tab.url, "morph://about");
    assert_eq!(tab.session, None);
    let (mut r, fake) = browser("https://a.test/");
    r.restore_tabs(&["https://b.test/".into(), "https://c.test/".into()], 1);
    let (urls, active) = r.tab_urls();
    assert_eq!(urls, vec!["https://a.test/", "https://b.test/", "https://c.test/"]);
    assert_eq!(active, 1);
    assert_eq!(fake.created().len(), 3);
}
