//! Settings > Privacy and site data > Profile password: whether the profile
//! asks for its password before it opens, changing the password, and Lock
//! now. Only a profile with an identity has a password; one without is not
//! encrypted and says so. See `docs/profile-lock.md`.

use gpui::{Context, Entity, Focusable, Window, div, prelude::*, px, rgb};
use morph_services::SecretString;
use morph_services::profile_lock::{self, LockStatus};
use morph_ui::actions::Action;

use super::{Item, SettingsView};
use crate::shell::ShellRequest;
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::button;

/// What the password form is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    TurnOff,
    TurnOn,
    Change,
}

/// Shown before turning the password off.
pub const OFF_WARNING: &str = "Anyone who can use your computer account will be able to open this profile, with its bookmarks, history, \
tabs and linked devices, without the password. The data stays encrypted; its key is kept in the system keystore instead.";

/// A profile without an identity.
pub const NO_IDENTITY: &str = "This profile has no identity, so it is not encrypted and not protected by a password. \
Create or import one in Identity to protect it.";

pub struct State {
    pub mode: Option<Mode>,
    current: Entity<TextInput>,
    new: Entity<TextInput>,
    confirm: Entity<TextInput>,
    pub note: Option<String>,
    pub error: Option<String>,
}

impl State {
    pub fn new(theme: morph_ui::theme::Theme, cx: &mut Context<SettingsView>) -> Self {
        Self {
            mode: None,
            current: cx.new(|cx| TextInput::new("Current password", theme, cx).masked()),
            new: cx.new(|cx| TextInput::new("New password", theme, cx).masked()),
            confirm: cx.new(|cx| TextInput::new("New password again", theme, cx).masked()),
            note: None,
            error: None,
        }
    }

    fn clear(&mut self, cx: &mut Context<SettingsView>) {
        for input in [&self.current, &self.new, &self.confirm] {
            input.update(cx, |i, cx| i.set_text("", cx));
        }
    }
}

/// Enter in any of the password fields confirms.
pub fn subscribe(state: &State, window: &mut Window, cx: &mut Context<SettingsView>) -> Vec<gpui::Subscription> {
    [&state.current, &state.new, &state.confirm]
        .into_iter()
        .map(|input| {
            cx.subscribe_in(input, window, |this, _, event, window, cx| match event {
                TextInputEvent::Submit { .. } => this.lock_confirm(window, cx),
                TextInputEvent::Cancel => this.lock_cancel(cx),
                _ => {}
            })
        })
        .collect()
}

/// The words for a failed password operation (never echoes a secret).
fn failure(e: &morph_services::Error) -> String {
    match e {
        morph_services::Error::WrongPassword => "That password is not right.".into(),
        e @ morph_services::Error::TooSoon { .. } => format!("Too many wrong passwords ({e})."),
        other => format!("Could not change it: {other}"),
    }
}

impl SettingsView {
    fn lock_status(&self) -> Option<(String, LockStatus)> {
        let id = self.profile_id()?;
        profile_lock::status(&self.home, &id).ok().map(|s| (id, s))
    }

    /// Item::ProfilePassword or Item::ChangePassword was pressed.
    pub(super) fn lock_activate(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, status)) = self.lock_status() else { return };
        if !status.has_identity {
            cx.emit(ShellRequest::Run(Action::ShowIdentity));
            return;
        }
        let mode = match item {
            Item::ChangePassword => Mode::Change,
            _ if status.require_password && status.policy_locked => return,
            _ if status.require_password => Mode::TurnOff,
            _ => Mode::TurnOn,
        };
        self.lock.mode = Some(mode);
        self.lock.error = None;
        self.lock.note = None;
        self.lock.clear(cx);
        window.focus(&self.lock.current.focus_handle(cx), cx);
        cx.notify();
    }

    pub(super) fn lock_cancel(&mut self, cx: &mut Context<Self>) {
        self.lock.mode = None;
        self.lock.clear(cx);
        cx.notify();
    }

    pub(super) fn lock_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(mode), Some((id, _))) = (self.lock.mode, self.lock_status()) else { return };
        let current = SecretString::from(self.lock.current.read(cx).text().to_string());
        let result = match mode {
            Mode::TurnOff => profile_lock::set_require_password(&self.home, &id, &current, false).map(|_| "Password turned off for this profile."),
            Mode::TurnOn => profile_lock::set_require_password(&self.home, &id, &current, true).map(|_| "This profile asks for its password again."),
            Mode::Change => {
                let (new, again) = (self.lock.new.read(cx).text().to_string(), self.lock.confirm.read(cx).text().to_string());
                if new != again {
                    self.lock.error = Some("The new passwords differ.".into());
                    cx.notify();
                    return;
                }
                profile_lock::change_password(&self.home, &id, &current, &SecretString::from(new)).map(|_| "Password changed.")
            }
        };
        self.lock.clear(cx);
        match result {
            Ok(note) => {
                self.lock.mode = None;
                self.lock.error = None;
                self.lock.note = Some(note.into());
                window.focus(&self.focus, cx);
            }
            Err(e) => self.lock.error = Some(failure(&e)),
        }
        cx.notify();
    }

    pub(super) fn lock_rows(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let Some((_, status)) = self.lock_status() else { return Vec::new() };
        if !status.has_identity {
            return vec![
                self.row(
                    Item::ProfilePassword,
                    "Profile password",
                    Some(NO_IDENTITY.into()),
                    button(t, "settings-lock-identity", "Open Identity", false)
                        .on_click(cx.listener(|this, _, w, cx| this.lock_activate(Item::ProfilePassword, w, cx)))
                        .into_any_element(),
                    cx,
                ),
            ];
        }
        let (help, label) = match (status.require_password, status.policy_locked) {
            (true, true) => ("On, and kept on by your administrator's policy.".to_string(), None),
            (true, false) => ("On: Morph asks for the password every time this profile opens.".to_string(), Some("Turn off…")),
            (false, _) => (format!("Off. {OFF_WARNING}"), Some("Turn on…")),
        };
        let toggle = match label {
            Some(label) => button(t, "settings-lock-toggle", label, false)
                .on_click(cx.listener(|this, _, w, cx| this.lock_activate(Item::ProfilePassword, w, cx)))
                .into_any_element(),
            None => div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("Required").into_any_element(),
        };
        let mut rows = vec![
            self.row(Item::ProfilePassword, "Require password to open this profile", Some(help), toggle, cx),
            self.row(
                Item::ChangePassword,
                "Change password",
                Some("You need the current password. A forgotten password cannot be recovered on this device: delete the profile, or link it again from one of your other devices.".into()),
                div()
                    .flex()
                    .flex_row()
                    .gap(px(6.))
                    .child(
                        button(t, "settings-lock-change", "Change…", false)
                            .on_click(cx.listener(|this, _, w, cx| this.lock_activate(Item::ChangePassword, w, cx))),
                    )
                    .child(
                        button(t, "settings-lock-now", "Lock now", false)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::Run(Action::LockProfile)))),
                    )
                    .into_any_element(),
                cx,
            ),
        ];
        if let Some(mode) = self.lock.mode {
            let title = match mode {
                Mode::TurnOff => "Turn the profile password off",
                Mode::TurnOn => "Turn the profile password on",
                Mode::Change => "Change the profile password",
            };
            let form = div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(8.))
                .bg(rgb(t.surface))
                .child(div().text_size(px(13.)).child(title))
                .when(mode == Mode::TurnOff, |d| d.child(div().text_size(px(12.)).text_color(rgb(t.error)).child(OFF_WARNING)))
                .child(div().flex().child(self.lock.current.clone()))
                .when(mode == Mode::Change, |d| {
                    d.child(div().flex().child(self.lock.new.clone())).child(div().flex().child(self.lock.confirm.clone()))
                })
                .children(self.lock.error.clone().map(|e| div().text_size(px(12.)).text_color(rgb(t.error)).child(e)))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(6.))
                        .child(
                            button(t, "settings-lock-confirm", if mode == Mode::TurnOff { "Turn off" } else { "Confirm" }, true)
                                .on_click(cx.listener(|this, _, w, cx| this.lock_confirm(w, cx))),
                        )
                        .child(button(t, "settings-lock-cancel", "Cancel", false).on_click(cx.listener(|this, _, _, cx| this.lock_cancel(cx)))),
                );
            rows.push(form.into_any_element());
        }
        if let Some(note) = self.lock.note.clone() {
            rows.push(div().px(px(12.)).text_size(px(12.)).text_color(rgb(t.text_secondary)).child(note).into_any_element());
        }
        rows.push(
            div()
                .px(px(12.))
                .text_size(px(12.))
                .text_color(rgb(t.text_secondary))
                .child(
                    "Cookies and site storage are protected only by your system's file permissions and disk encryption, not by the profile password.",
                )
                .into_any_element(),
        );
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_never_echo_a_secret() {
        assert_eq!(failure(&morph_services::Error::WrongPassword), "That password is not right.");
        assert!(failure(&morph_services::Error::TooSoon { wait_ms: 1500 }).contains("try again in 2 s"));
    }
}
