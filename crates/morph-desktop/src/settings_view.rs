//! The settings panel (`morph.settings`): where the tabs go and how the
//! side tab list looks, saved in the shown profile's `settings.json`. A
//! change asks the shell ([`ShellRequest::SettingsChanged`]) to pass the new
//! settings to the components.

use std::path::{Path, PathBuf};

use gpui::{App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Window, div, prelude::*, px, rgb};
use morph_core::settings::{self, Settings, TabPosition};
use morph_ui::chrome::SettingsChange;
use morph_ui::theme::Theme;

use crate::default_browser::{self, Status};
use crate::shell::ShellRequest;
use crate::widgets::{button, chip, heading, icon};

pub struct SettingsView {
    home: PathBuf,
    settings: Settings,
    theme: Theme,
    focus: FocusHandle,
    error: Option<String>,
    /// Whether Morph opens links, read when the panel opens.
    browser: Option<Status>,
    browser_note: Option<String>,
    /// A running `xdg-settings` (never on the UI thread).
    browser_check: Option<default_browser::Check>,
}

impl EventEmitter<ShellRequest> for SettingsView {}

impl SettingsView {
    pub fn new(home: &Path, theme: Theme, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            home: home.to_path_buf(),
            settings: Settings::default(),
            theme,
            focus: cx.focus_handle(),
            error: None,
            browser: None,
            browser_note: None,
            browser_check: None,
        };
        view.reload();
        view
    }

    /// Read the shown profile's settings again (after a profile switch).
    pub fn reload(&mut self) {
        match settings::load(&self.home) {
            Ok(s) => {
                self.settings = s;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Could not read the settings: {e}")),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The panel opened: it takes the keyboard (arrows move the tabs).
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.browser_check.is_none() {
            self.browser_check = Some(default_browser::Check::status(default_browser::system));
        }
        self.browser_note = None;
        window.focus(&self.focus, cx);
        cx.emit(ShellRequest::FocusShell);
        cx.notify();
    }

    fn change(&mut self, change: SettingsChange, cx: &mut Context<Self>) {
        let next = change.apply(&self.settings);
        if next == self.settings {
            return;
        }
        match settings::save(&self.home, &next) {
            Ok(()) => {
                self.settings = next;
                self.error = None;
                cx.emit(ShellRequest::SettingsChanged);
            }
            Err(e) => self.error = Some(format!("Could not save the settings: {e}")),
        }
        cx.notify();
    }

    fn make_default(&mut self, cx: &mut Context<Self>) {
        self.browser_check = Some(default_browser::Check::make_default(default_browser::system));
        self.browser_note = Some("Asking the desktop…".into());
        cx.notify();
    }

    /// Pick up a finished `xdg-settings`; called by the host's tick.
    pub fn poll(&mut self, cx: &mut Context<Self>) {
        let Some((status, made)) = self.browser_check.as_ref().and_then(|c| c.take()) else {
            return;
        };
        self.browser_check = None;
        self.browser = Some(status);
        self.browser_note = made.map(|made| match made {
            Ok(()) => "Links from other apps now open in Morph.".into(),
            Err(e) => format!("Could not make Morph the default: {e}"),
        });
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.modifiers.control || k.modifiers.alt || k.modifiers.platform {
            return;
        }
        if k.key == "escape" {
            cx.emit(ShellRequest::ClosePanel);
            cx.stop_propagation();
            return;
        }
        let key = k.key_char.as_deref().filter(|c| matches!(*c, "+" | "=" | "-")).unwrap_or(k.key.as_str());
        if key == "d" {
            self.make_default(cx);
            cx.stop_propagation();
            return;
        }
        if let Some(change) = SettingsChange::for_key(key) {
            self.change(change, cx);
            cx.stop_propagation();
        }
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let s = self.settings.clone();
        let positions =
            [(TabPosition::Top, "Top ↑"), (TabPosition::Bottom, "Bottom ↓"), (TabPosition::Left, "Left ←"), (TabPosition::Right, "Right →")];
        let side = matches!(s.tab_position, TabPosition::Left | TabPosition::Right);
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(10.))
            .bg(rgb(t.surface))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(div().flex_1().text_size(px(13.)).child("Settings"))
                    .child(icon(t, "settings-close", "×").on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::ClosePanel)))),
            )
            .child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("For this profile. Arrow keys move the tabs."))
            .child(heading(t, "Tabs"))
            .child(div().flex().flex_row().flex_wrap().gap(px(6.)).children(positions.into_iter().enumerate().map(|(i, (position, label))| {
                chip(t, ("settings-position", i), label, s.tab_position == position)
                    .on_click(cx.listener(move |this, _, _, cx| this.change(SettingsChange::Position(position), cx)))
            })))
            .when(side, |d| {
                d.child(heading(t, "Side tab list"))
                    .child(
                        div().flex().flex_row().gap(px(6.)).child(
                            chip(t, "settings-collapsed", if s.sidebar_collapsed { "Collapsed (c)" } else { "Expanded (c)" }, s.sidebar_collapsed)
                                .on_click(cx.listener(|this, _, _, cx| this.change(SettingsChange::ToggleCollapsed, cx))),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                button(t, "settings-narrower", "−", false)
                                    .on_click(cx.listener(|this, _, _, cx| this.change(SettingsChange::Narrower, cx))),
                            )
                            .child(div().text_size(px(12.)).child(format!("{} px wide", s.sidebar_width)))
                            .child(
                                button(t, "settings-wider", "+", false)
                                    .on_click(cx.listener(|this, _, _, cx| this.change(SettingsChange::Wider, cx))),
                            ),
                    )
            })
            .child(heading(t, "Default browser"))
            .child(div().text_size(px(12.)).child(match &self.browser {
                Some(Status::Default) => "Morph opens links from other apps.".to_string(),
                Some(Status::Other(id)) => format!("Links open in {id}."),
                Some(Status::Unknown(e)) => format!("Unknown: {e}"),
                None => String::new(),
            }))
            .when(self.browser != Some(Status::Default), |d| {
                d.child(
                    div().flex().flex_row().child(
                        button(t, "settings-default-browser", "Make Morph the default (d)", true)
                            .on_click(cx.listener(|this, _, _, cx| this.make_default(cx))),
                    ),
                )
            })
            .children(self.browser_note.clone().map(|n| div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(n)))
            .children(self.error.clone().map(|e| div().text_size(px(12.)).text_color(rgb(t.error)).child(e)))
    }
}
