//! Settings > Privacy and site data > Passwords: the password saver's
//! switch, "ask again before filling", and the saved logins (show behind a
//! fresh proof, copy, delete). See `docs/profile-lock.md`, "Password saver".
//! Only a profile with an identity has a saver; a system prompt (password
//! off) runs on its own thread so the window never waits on it.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use gpui::{ClipboardItem, Context, Entity, Focusable, Window, div, prelude::*, px, rgb};
use morph_services::password_saver::{self as saver, LoginInfo, Unavailable};
use morph_services::{SecretString, Zeroizing};

use super::{Item, SettingsView};
use crate::password_saver::{SystemOsAuth, fill_failure, site};
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::button;

/// How long a shown password stays on screen.
pub const SHOWN_FOR: Duration = Duration::from_secs(60);

/// A password being opened on its own thread.
type Opening = mpsc::Receiver<Result<Zeroizing<String>, String>>;

/// What the password field is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    /// Turning "ask again" off needs the profile password.
    AskAgainOff,
    /// Showing this login's password.
    Reveal(String),
}

pub struct State {
    pub logins: Vec<LoginInfo>,
    pub prompt: Option<Prompt>,
    password: Entity<TextInput>,
    /// The one password shown (login id), and since when.
    shown: Option<(String, Zeroizing<String>, Instant)>,
    revealing: Option<(String, Opening)>,
    /// A login whose Delete was pressed once and waits for the second press.
    confirm_delete: Option<String>,
    pub note: Option<String>,
    pub error: Option<String>,
}

impl State {
    pub fn new(theme: morph_ui::theme::Theme, cx: &mut Context<SettingsView>) -> Self {
        Self {
            logins: Vec::new(),
            prompt: None,
            password: cx.new(|cx| TextInput::new("Profile password", theme, cx).masked()),
            shown: None,
            revealing: None,
            confirm_delete: None,
            note: None,
            error: None,
        }
    }
}

/// Enter in the password field confirms, Esc cancels.
pub fn subscribe(state: &State, window: &mut Window, cx: &mut Context<SettingsView>) -> Vec<gpui::Subscription> {
    vec![cx.subscribe_in(&state.password, window, |this, _, event, window, cx| match event {
        TextInputEvent::Submit { .. } => this.passwords_confirm(window, cx),
        TextInputEvent::Cancel => this.passwords_cancel(cx),
        _ => {}
    })]
}

/// Why the saver is not offered, in words.
pub fn unavailable(u: Unavailable) -> &'static str {
    match u {
        Unavailable::NoIdentity => "This profile has no identity, so it cannot save passwords. Create or import one in Identity.",
        Unavailable::NoOsAuth => {
            "The profile password is off and this system cannot confirm it's you (no Touch ID or password prompt, Windows Hello or polkit), so saved passwords are not filled. Turn the profile password on to use them."
        }
    }
}

impl SettingsView {
    fn saver_offer(&self) -> Option<(String, saver::Offer)> {
        let id = self.profile_id()?;
        saver::offer(&self.home, &id, &SystemOsAuth).ok().map(|o| (id, o))
    }

    /// Read the saved logins again (the page opened, or one was saved elsewhere).
    pub(super) fn passwords_reload(&mut self) {
        self.passwords.logins = self.profile_id().and_then(|id| saver::list(&self.home, &id, &SystemOsAuth, None).ok()).unwrap_or_default();
        self.passwords.confirm_delete = None;
        if self.passwords.shown.as_ref().is_some_and(|(id, ..)| !self.passwords.logins.iter().any(|l| &l.id == id)) {
            self.passwords.shown = None;
        }
    }

    /// Item::PasswordSaver, Item::AskAgain or Item::SavedPasswords was pressed.
    pub(super) fn passwords_activate(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, offer)) = self.saver_offer() else { return };
        self.passwords.error = None;
        self.passwords.note = None;
        let result = match item {
            Item::PasswordSaver => saver::set_enabled(&self.home, &id, !offer.enabled).map(|_| {
                if offer.enabled {
                    "Morph no longer offers to save or fill passwords. Saved ones stay."
                } else {
                    "Morph offers to save and fill passwords."
                }
            }),
            Item::AskAgain if offer.ask_again => {
                self.passwords.prompt = Some(Prompt::AskAgainOff);
                self.passwords.password.update(cx, |i, cx| i.set_text("", cx));
                window.focus(&self.passwords.password.focus_handle(cx), cx);
                cx.notify();
                return;
            }
            Item::AskAgain => saver::set_ask_again(&self.home, &id, true, None).map(|_| "Morph asks for the profile password before every fill."),
            _ => {
                self.passwords_reload();
                cx.notify();
                return;
            }
        };
        match result {
            Ok(note) => self.passwords.note = Some(note.into()),
            Err(e) => self.passwords.error = Some(format!("Could not change it: {e}")),
        }
        cx.notify();
    }

    pub(super) fn passwords_cancel(&mut self, cx: &mut Context<Self>) {
        self.passwords.prompt = None;
        self.passwords.password.update(cx, |i, cx| i.set_text("", cx));
        cx.notify();
    }

    pub(super) fn passwords_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(prompt), Some(id)) = (self.passwords.prompt.clone(), self.profile_id()) else { return };
        let typed = SecretString::from(self.passwords.password.read(cx).text().to_string());
        self.passwords.password.update(cx, |i, cx| i.set_text("", cx));
        match prompt {
            Prompt::AskAgainOff => match saver::set_ask_again(&self.home, &id, false, Some(&typed)) {
                Ok(()) => {
                    self.passwords.prompt = None;
                    self.passwords.error = None;
                    self.passwords.note = Some("Filling no longer asks for the profile password.".into());
                    window.focus(&self.focus, cx);
                }
                Err(e) => self.passwords.error = Some(fill_failure(&e)),
            },
            Prompt::Reveal(login) => {
                self.passwords.prompt = None;
                self.passwords_start_reveal(login, Some(typed));
                window.focus(&self.focus, cx);
            }
        }
        cx.notify();
    }

    fn passwords_show(&mut self, login: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.profile_id() else { return };
        self.passwords.error = None;
        match saver::reveal_requirement(&self.home, &id, &SystemOsAuth) {
            Ok(r) if r.password => {
                self.passwords.prompt = Some(Prompt::Reveal(login));
                self.passwords.password.update(cx, |i, cx| i.set_text("", cx));
                window.focus(&self.passwords.password.focus_handle(cx), cx);
            }
            Ok(_) => self.passwords_start_reveal(login, None),
            Err(e) => self.passwords.error = Some(fill_failure(&e)),
        }
        cx.notify();
    }

    /// Prove and open on a thread; [`Self::passwords_poll`] picks it up.
    fn passwords_start_reveal(&mut self, login: String, password: Option<SecretString>) {
        let Some(id) = self.profile_id() else { return };
        let (tx, rx) = mpsc::channel();
        let (home, login_id) = (self.home.clone(), login.clone());
        std::thread::spawn(move || {
            let _ = tx.send(saver::reveal(&home, &id, &SystemOsAuth, &login_id, password.as_ref()).map_err(|e| fill_failure(&e)));
        });
        self.passwords.shown = None;
        self.passwords.revealing = Some((login, rx));
    }

    /// From the host's tick: a finished reveal, and hiding a shown password in time.
    pub(super) fn passwords_poll(&mut self, cx: &mut Context<Self>) {
        if self.passwords.shown.as_ref().is_some_and(|(_, _, at)| at.elapsed() >= SHOWN_FOR) {
            self.passwords.shown = None;
            cx.notify();
        }
        let Some((_, rx)) = &self.passwords.revealing else { return };
        let got = match rx.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("Not shown.".into()),
        };
        let Some((login, _)) = self.passwords.revealing.take() else { return };
        match got {
            Ok(password) => self.passwords.shown = Some((login, password, Instant::now())),
            Err(why) => self.passwords.error = Some(why),
        }
        cx.notify();
    }

    fn passwords_hide(&mut self, cx: &mut Context<Self>) {
        self.passwords.shown = None;
        cx.notify();
    }

    fn passwords_copy(&mut self, cx: &mut Context<Self>) {
        if let Some((_, password, _)) = &self.passwords.shown {
            cx.write_to_clipboard(ClipboardItem::new_string(password.to_string()));
            self.passwords.note = Some("Copied the password.".into());
        }
        cx.notify();
    }

    /// First press arms the button, the second deletes.
    fn passwords_delete(&mut self, login: String, cx: &mut Context<Self>) {
        if self.passwords.confirm_delete.as_deref() != Some(login.as_str()) {
            self.passwords.confirm_delete = Some(login);
            cx.notify();
            return;
        }
        let Some(id) = self.profile_id() else { return };
        match saver::delete(&self.home, &id, &login) {
            Ok(_) => self.passwords.note = Some("Deleted the saved login.".into()),
            Err(e) => self.passwords.error = Some(format!("Could not delete it: {e}")),
        }
        self.passwords_reload();
        cx.notify();
    }

    pub(super) fn passwords_rows(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let Some((_, offer)) = self.saver_offer() else { return Vec::new() };
        if offer.requirement == Err(Unavailable::NoIdentity) {
            return vec![self.row(Item::PasswordSaver, "Passwords", Some(unavailable(Unavailable::NoIdentity).into()), div().into_any_element(), cx)];
        }
        let help = match (&offer.requirement, offer.enabled) {
            (_, false) => "Off: Morph does not offer to save or fill passwords. Saved ones stay below.".to_string(),
            (Err(u), true) => unavailable(*u).to_string(),
            (Ok(r), true) if r.os_auth => "On. The profile password is off, so your system confirms it's you before every fill.".to_string(),
            (Ok(_), true) => "On. Saved passwords are sealed with this profile's identity and open only while it is unlocked; only https sites, and only the exact site they were saved on.".to_string(),
        };
        let toggle = button(t, "settings-passwords-toggle", if offer.enabled { "Turn off" } else { "Turn on" }, false)
            .on_click(cx.listener(|this, _, w, cx| this.passwords_activate(Item::PasswordSaver, w, cx)));
        let ask = button(t, "settings-passwords-ask", if offer.ask_again { "Turn off…" } else { "Turn on" }, false)
            .on_click(cx.listener(|this, _, w, cx| this.passwords_activate(Item::AskAgain, w, cx)));
        let mut rows = vec![
            self.row(Item::PasswordSaver, "Offer to save passwords", Some(help), toggle.into_any_element(), cx),
            self.row(
                Item::AskAgain,
                "Ask for the profile password before filling",
                Some(if offer.ask_again {
                    "On: every fill asks for the profile password (not a second password). Turning it off needs the profile password.".into()
                } else {
                    "Off: an unlocked profile fills without asking again.".into()
                }),
                ask.into_any_element(),
                cx,
            ),
        ];
        if self.passwords.prompt == Some(Prompt::AskAgainOff) {
            rows.push(self.passwords_form("Enter the profile password to stop asking before fills", cx));
        }
        let count = self.passwords.logins.len();
        rows.push(
            self.row(
                Item::SavedPasswords,
                &format!("Saved passwords ({count})"),
                Some(if count == 0 {
                    "None yet. Morph offers to save a password after you sign in on an https site.".into()
                } else {
                    "Showing a password asks for the profile password, or your system's confirmation when the password is off.".into()
                }),
                button(t, "settings-passwords-reload", "Refresh", false)
                    .on_click(cx.listener(|this, _, w, cx| this.passwords_activate(Item::SavedPasswords, w, cx)))
                    .into_any_element(),
                cx,
            ),
        );
        let logins = self.passwords.logins.clone();
        for (i, login) in logins.iter().enumerate() {
            rows.push(self.passwords_login_row(i, login, cx));
            if self.passwords.prompt == Some(Prompt::Reveal(login.id.clone())) {
                rows.push(self.passwords_form("Enter the profile password to show this password", cx));
            }
        }
        for (text, error) in [(self.passwords.error.clone(), true), (self.passwords.note.clone(), false)] {
            if let Some(text) = text {
                rows.push(
                    div()
                        .px(px(12.))
                        .text_size(px(12.))
                        .text_color(rgb(if error { t.error } else { t.text_secondary }))
                        .child(text)
                        .into_any_element(),
                );
            }
        }
        rows
    }

    fn passwords_login_row(&self, i: usize, login: &LoginInfo, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let shown = self.passwords.shown.as_ref().filter(|(id, ..)| *id == login.id).map(|(_, p, _)| p.to_string());
        let revealing = self.passwords.revealing.as_ref().is_some_and(|(id, _)| *id == login.id);
        let user = if login.username.is_empty() { "(no username)".to_string() } else { login.username.clone() };
        let secret = match (&shown, revealing) {
            (Some(p), _) => p.clone(),
            (None, true) => "Waiting…".into(),
            (None, false) => "••••••••".into(),
        };
        let id = login.id.clone();
        let mut controls = div().flex().flex_row().gap(px(6.));
        if shown.is_some() {
            controls = controls
                .child(button(t, ("settings-password-copy", i), "Copy", false).on_click(cx.listener(|this, _, _, cx| this.passwords_copy(cx))))
                .child(button(t, ("settings-password-hide", i), "Hide", false).on_click(cx.listener(|this, _, _, cx| this.passwords_hide(cx))));
        } else {
            let show = id.clone();
            controls = controls.child(
                button(t, ("settings-password-show", i), "Show…", false)
                    .on_click(cx.listener(move |this, _, w, cx| this.passwords_show(show.clone(), w, cx))),
            );
        }
        let armed = self.passwords.confirm_delete.as_deref() == Some(id.as_str());
        controls = controls.child(
            button(t, ("settings-password-delete", i), if armed { "Confirm delete" } else { "Delete" }, armed)
                .on_click(cx.listener(move |this, _, _, cx| this.passwords_delete(id.clone(), cx))),
        );
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(6.))
            .rounded(px(8.))
            .bg(rgb(t.surface))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .child(div().text_size(px(13.)).child(site(&login.origin).to_string()))
                    .child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(format!("{user} · {secret}"))),
            )
            .child(controls)
            .into_any_element()
    }

    fn passwords_form(&self, title: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(8.))
            .bg(rgb(t.surface))
            .child(div().text_size(px(13.)).child(title.to_string()))
            .child(div().flex().child(self.passwords.password.clone()))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(6.))
                    .child(
                        button(t, "settings-passwords-confirm", "Confirm", true)
                            .on_click(cx.listener(|this, _, w, cx| this.passwords_confirm(w, cx))),
                    )
                    .child(button(t, "settings-passwords-cancel", "Cancel", false).on_click(cx.listener(|this, _, _, cx| this.passwords_cancel(cx)))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reasons_name_the_way_out() {
        assert!(unavailable(Unavailable::NoIdentity).contains("Identity"));
        assert!(unavailable(Unavailable::NoOsAuth).contains("Turn the profile password on"));
    }
}
