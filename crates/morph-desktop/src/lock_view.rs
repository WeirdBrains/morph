//! The lock screen a window shows instead of a locked profile: nothing of
//! the profile (pages, tabs, panels, cookies) is built until its password
//! unlocks it. See `docs/profile-lock.md`.

use std::path::{Path, PathBuf};

use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window, div, prelude::*, px, rgb};
use morph_services::SecretString;
use morph_ui::theme::Theme;

use crate::shell::ShellRequest;
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::button;

/// What a forgotten password leaves, said on the lock screen.
pub const FORGOT: &str = "A forgotten password cannot be recovered on this device. You can delete this profile from another profile, \
or link it again from one of your other devices (Settings > Identity on that device).";

/// The lock screen's state, free of GPUI so it is tested here.
pub struct ProfileGate {
    home: PathBuf,
    profile: String,
    /// The last failure to show (never echoes what came back).
    pub error: Option<String>,
}

impl ProfileGate {
    pub fn new(home: &Path, profile: &str) -> Self {
        Self { home: home.to_path_buf(), profile: profile.to_string(), error: None }
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Unlock with `password`. `true` when the profile is open now.
    pub fn unlock(&mut self, password: &str) -> bool {
        if password.is_empty() {
            return false;
        }
        match morph_services::library::unlock(&self.home, &self.profile, &SecretString::from(password.to_string())) {
            Ok(_) => {
                self.error = None;
                true
            }
            Err(e @ morph_services::Error::TooSoon { .. }) => {
                self.error = Some(format!("Too many wrong passwords. {}", capitalize_try_again(&e.to_string())));
                false
            }
            Err(_) => {
                self.error = Some("That password is not right.".into());
                false
            }
        }
    }

    /// The other profiles, to switch to instead: (id, name).
    pub fn others(&self) -> Vec<(String, String)> {
        morph_core::profiles::list(&self.home).unwrap_or_default().into_iter().filter(|p| p.id != self.profile).map(|p| (p.id, p.name)).collect()
    }

    pub fn name(&self) -> String {
        morph_core::profiles::list(&self.home).unwrap_or_default().into_iter().find(|p| p.id == self.profile).map(|p| p.name).unwrap_or_default()
    }
}

/// "try again in 4 s" out of the service's message.
fn capitalize_try_again(message: &str) -> String {
    match message.find("try again") {
        Some(at) => format!("T{}.", &message[at + 1..]),
        None => String::new(),
    }
}

pub struct LockView {
    gate: ProfileGate,
    theme: Theme,
    password: Entity<TextInput>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShellRequest> for LockView {}

impl LockView {
    pub fn new(home: &Path, profile: &str, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let password = cx.new(|cx| TextInput::new("Profile password", theme, cx).masked());
        let subs = vec![cx.subscribe_in(&password, window, |this, _, event, window, cx| {
            if let TextInputEvent::Submit { .. } = event {
                this.unlock(window, cx);
            }
        })];
        Self { gate: ProfileGate::new(home, profile), theme, password, focus: cx.focus_handle(), _subscriptions: subs }
    }

    /// Put the keyboard in the password field.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.password.focus_handle(cx), cx);
    }

    fn unlock(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.password.read(cx).text().to_string();
        let opened = self.gate.unlock(&text);
        self.password.update(cx, |p, cx| p.set_text("", cx));
        cx.notify();
        if opened {
            if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
                println!("morph-event profile unlocked");
            }
            cx.emit(ShellRequest::ProfileUnlocked(self.gate.profile().to_string()));
        }
    }
}

impl Focusable for LockView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LockView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let others = self.gate.others();
        div().track_focus(&self.focus).size_full().flex().items_center().justify_center().bg(rgb(t.background)).child(
            div()
                .w(px(380.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(20.))
                .rounded(px(10.))
                .bg(rgb(t.surface))
                .child(div().text_size(px(16.)).child(format!("{} is locked", self.gate.name())))
                .child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("Enter the profile password to open it."))
                .child(div().flex().child(self.password.clone()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .child(button(t, "lock-unlock", "Unlock", true).on_click(cx.listener(|this, _, window, cx| this.unlock(window, cx)))),
                )
                .children(self.gate.error.clone().map(|e| div().text_size(px(12.)).text_color(rgb(t.error)).child(e)))
                .child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(FORGOT))
                .when(!others.is_empty(), |d| {
                    d.child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child("Or open another profile:")).child(
                        div().flex().flex_row().flex_wrap().gap(px(6.)).children(others.into_iter().map(|(id, name)| {
                            let target = id.clone();
                            button(t, gpui::SharedString::from(format!("lock-switch-{id}")), name, false)
                                .on_click(cx.listener(move |_, _, _, cx| cx.emit(ShellRequest::SwitchProfile(target.clone()))))
                        })),
                    )
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_needs_the_password_and_backs_off() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        morph_services::identity::create(&home, "default", &SecretString::from("profile password".to_string())).unwrap();
        morph_services::profile_lock::lock(&home, "default");
        let mut gate = ProfileGate::new(&home, "default");
        assert!(!gate.unlock(""));
        for _ in 0..morph_core::profile_lock::FREE_ATTEMPTS {
            assert!(!gate.unlock("wrong password"));
            assert_eq!(gate.error.as_deref(), Some("That password is not right."));
        }
        assert!(!gate.unlock("profile password"), "waits even for the right one");
        assert!(gate.error.as_deref().is_some_and(|e| e.starts_with("Too many wrong passwords. Try again in")), "{:?}", gate.error);
        assert!(morph_services::profile_lock::is_locked(&home, "default"));
    }

    #[test]
    fn the_right_password_opens_it() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        morph_core::profiles::create(&home, "Work").unwrap();
        morph_services::identity::create(&home, "default", &SecretString::from("profile password".to_string())).unwrap();
        morph_services::profile_lock::lock(&home, "default");
        let mut gate = ProfileGate::new(&home, "default");
        assert_eq!(gate.others(), vec![("p1".to_string(), "Work".to_string())]);
        assert!(gate.unlock("profile password"));
        assert!(!morph_services::profile_lock::is_locked(&home, "default"));
    }
}
