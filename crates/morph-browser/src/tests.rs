use std::cell::RefCell;
use std::rc::Rc;

use morph_ui::actions::{Action, Keymap};
use morph_webview::fake::{Call, FakeBackend};
use morph_webview::{Event, Rect, ViewId};

use super::{Browser, Config};

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
    assert!(b.toolbar().crashed);
    assert_eq!(fake.last_visible(view), Some(false));
    b.dispatch(Action::Reload);
    assert!(!b.toolbar().crashed);
    assert_eq!(fake.last_visible(view), Some(true));
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
