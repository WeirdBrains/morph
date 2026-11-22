//! The password saver's bar under the page ([`crate::password_saver`]):
//! "Save the password for amy on example.com?" after a login, and "Fill the
//! saved login?" on a page with a login form. Only for the tab shown; the
//! fill itself is two page actions pinned to the login's origin.

use std::path::Path;
use std::sync::Arc;

use gpui::{Context, Window, div, prelude::*, px, rgb};
use morph_core::tabs::TabId;
use morph_services::SecretString;
use morph_webview::login_form;
use morph_webview::page_action::ActionResult;
use morph_webview::page_read::Pending;

use super::{BrowserView, log_event};
use crate::password_saver::{Bar, Inbox, Saver, SystemOsAuth, site};
use crate::widgets::{button, chip};

/// A fill under way: its page actions, the password's last.
pub(super) struct Filling {
    username: String,
    origin: String,
    actions: Vec<Pending<ActionResult>>,
    results: Vec<ActionResult>,
}

impl BrowserView {
    /// Save and fill logins for `profile` in this window's tabs; `inbox` is
    /// where its page bridge puts what pages said ([`crate::password_saver::page_bridge`]).
    pub fn set_password_saver(&mut self, home: &Path, profile: &str, inbox: Inbox) {
        crate::password_saver::warm_up();
        self.saver = Some(Saver::new(home, profile, Arc::new(SystemOsAuth)));
        self.saver_inbox = Some(inbox);
    }

    /// From the poll loop: what pages said, a finished proof, a finished fill.
    pub(super) fn tick_saver(&mut self, cx: &mut Context<Self>) {
        let (Some(saver), Some(inbox)) = (self.saver.as_mut(), self.saver_inbox.as_ref()) else { return };
        let before = saver.bar.as_ref().map(Bar::tab);
        let heard: Vec<_> = inbox.borrow_mut().drain(..).collect();
        let browser = &self.browser;
        let active = browser.active_tab_id();
        let url_of = |tab: TabId| browser.tab_url(tab).map(str::to_string);
        for login in heard {
            let tab = browser.tab_of_view(login.view());
            saver.heard(login, tab, active, url_of);
        }
        if let Some(tab) = saver.bar.as_ref().map(Bar::tab) {
            saver.moved(tab, browser.tab_url(tab));
        }
        saver.shown(active, url_of);
        let ready = saver.poll();
        let changed = before != saver.bar.as_ref().map(Bar::tab) || ready.is_some();
        if let Some(ready) = ready {
            let page = self.browser.tab_url(ready.tab).unwrap_or_default().to_string();
            if login_form::fill_allowed(&ready.origin, &page) {
                let actions = login_form::fill_actions(&ready.origin, &ready.username, &ready.password)
                    .into_iter()
                    .map(|request| self.browser.tab_action(ready.tab, request))
                    .collect();
                self.saver_filling = Some(Filling { username: ready.username.clone(), origin: ready.origin.clone(), actions, results: Vec::new() });
            } else {
                self.toast("Not filled: the page is no longer the site the login was saved for.", None, cx);
            }
        }
        self.tick_filling(cx);
        if changed {
            cx.notify();
        }
    }

    fn tick_filling(&mut self, cx: &mut Context<Self>) {
        let Some(filling) = self.saver_filling.as_mut() else { return };
        while let Some(first) = filling.actions.first() {
            match first.try_take() {
                Some(result) => {
                    filling.actions.remove(0);
                    filling.results.push(result);
                }
                None => return,
            }
        }
        let Some(filling) = self.saver_filling.take() else { return };
        // The password's action is the last; the username's may find no field.
        let text = match filling.results.last() {
            Some(Ok(_)) => {
                log_event("password saver filled");
                let who = if filling.username.is_empty() { String::new() } else { format!(" for {}", filling.username) };
                format!("Filled the saved login{who} on {}", site(&filling.origin))
            }
            Some(Err(e)) => format!("Not filled: {e}"),
            None => return,
        };
        self.toast(&text, None, cx);
    }

    fn saver_save(&mut self, cx: &mut Context<Self>) {
        let Some(saver) = self.saver.as_mut() else { return };
        let text = match saver.save() {
            Ok(text) => {
                log_event("password saver saved");
                text
            }
            Err(e) => e,
        };
        self.toast(&text, None, cx);
        cx.notify();
    }

    pub(super) fn saver_fill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.saver.as_ref().and_then(|s| s.bar.as_ref()).map(Bar::tab) else { return };
        let url = self.browser.tab_url(tab).unwrap_or_default().to_string();
        let typed = self.saver_input.read(cx).text().to_string();
        self.saver_input.update(cx, |i, cx| i.set_text("", cx));
        let password = (!typed.is_empty()).then(|| SecretString::from(typed));
        if let Some(saver) = self.saver.as_mut() {
            saver.start_fill(&url, password);
        }
        window.focus(&self.page_focus, cx);
        cx.notify();
    }

    pub(super) fn saver_dismiss(&mut self, cx: &mut Context<Self>) {
        if let Some(saver) = self.saver.as_mut() {
            saver.dismiss();
        }
        self.saver_input.update(cx, |i, cx| i.set_text("", cx));
        cx.notify();
    }

    fn saver_choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(saver) = self.saver.as_mut() {
            saver.choose(index);
        }
        cx.notify();
    }

    /// The bar, for the tab shown.
    pub(super) fn saver_bar(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let bar = self.saver.as_ref()?.bar.as_ref()?;
        if bar.tab() != self.browser.active_tab_id() {
            return None;
        }
        let t = self.theme;
        let text = |s: String| div().flex_1().min_w(px(0.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(s);
        let row = div()
            .id("password-saver")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(4.))
            .min_h(px(36.))
            .bg(rgb(t.surface))
            .border_t_1()
            .border_color(rgb(t.surface_variant))
            .text_size(px(13.))
            .text_color(rgb(t.text_primary));
        let not_now = button(t, "password-not-now", "Not now", false).on_click(cx.listener(|this, _, _, cx| this.saver_dismiss(cx)));
        let row = match bar {
            Bar::Save { username, origin, update, .. } => {
                let who = if username.is_empty() { String::new() } else { format!(" for {username}") };
                let ask = if *update { "Update the saved password" } else { "Save the password" };
                row.child(text(format!("{ask}{who} on {}?", site(origin))))
                    .child(
                        button(t, "password-save", if *update { "Update" } else { "Save" }, true)
                            .on_click(cx.listener(|this, _, _, cx| this.saver_save(cx))),
                    )
                    .child(not_now)
            }
            Bar::Fill(offer) => {
                let user = &offer.chosen().username;
                let who = if user.is_empty() { String::new() } else { format!(" for {user}") };
                let mut row = row.child(text(format!("Fill the saved login{who} on {}?", site(&offer.origin))));
                if offer.logins.len() > 1 {
                    for (i, login) in offer.logins.iter().enumerate().take(5) {
                        let label = if login.username.is_empty() { "(no username)".to_string() } else { login.username.clone() };
                        row = row.child(
                            chip(t, ("password-login", i), label, i == offer.choice)
                                .on_click(cx.listener(move |this, _, _, cx| this.saver_choose(i, cx))),
                        );
                    }
                }
                if offer.ask_password {
                    row = row.child(div().w(px(200.)).flex().child(self.saver_input.clone()));
                }
                if let Some(error) = &offer.error {
                    row = row.child(div().text_size(px(12.)).text_color(rgb(t.error)).child(error.clone()));
                }
                let label = if offer.os_auth { "Fill…" } else { "Fill" };
                row.child(button(t, "password-fill", label, true).on_click(cx.listener(|this, _, w, cx| this.saver_fill(w, cx)))).child(not_now)
            }
            Bar::Working { offer, .. } => row.child(text(if offer.os_auth {
                "Waiting for your system to confirm it's you…".to_string()
            } else {
                "Checking the profile password…".to_string()
            })),
        };
        Some(row.into_any_element())
    }
}
