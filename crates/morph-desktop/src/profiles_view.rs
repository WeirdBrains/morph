//! The profiles panel (`morph.profiles`): switch, create, rename and delete
//! profiles. Switching asks the shell ([`ShellRequest::SwitchProfile`]),
//! which rebuilds the window's components for the profile.

use std::path::Path;

use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window, div, prelude::*, px, rgb};
use morph_ui::theme::Theme;

use crate::profiles::Profiles;
use crate::shell::ShellRequest;
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::{button, heading, icon};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Edit {
    New,
    Rename(String),
}

pub struct ProfilesView {
    profiles: Profiles,
    theme: Theme,
    input: Entity<TextInput>,
    editing: Option<Edit>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShellRequest> for ProfilesView {}

impl ProfilesView {
    pub fn new(home: &Path, shown: &str, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Profile name", theme, cx));
        let sub = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
            TextInputEvent::Submit { .. } => this.save(window, cx),
            TextInputEvent::Cancel => {
                if this.editing.take().is_none() {
                    cx.emit(ShellRequest::ClosePanel);
                }
                cx.notify();
            }
            TextInputEvent::Changed => {}
        });
        Self { profiles: Profiles::new(home, shown), theme, input, editing: None, focus: cx.focus_handle(), _subscriptions: vec![sub] }
    }

    pub fn shown_name(&self) -> String {
        self.profiles.shown_name()
    }

    /// The panel opened: start a new profile's name, the likeliest action
    /// besides clicking one.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start(Edit::New, "", window, cx);
    }

    /// Make `id` current; returns whether it worked.
    pub fn switch(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        self.editing = None;
        cx.notify();
        self.profiles.switch(id)
    }

    fn start(&mut self, edit: Edit, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(edit);
        self.input.update(cx, |i, cx| i.set_text(text, cx));
        window.focus(&self.input.focus_handle(cx), cx);
        cx.emit(ShellRequest::FocusShell);
        cx.notify();
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.input.read(cx).text().to_string();
        match self.editing.clone() {
            Some(Edit::New) => {
                if let Some(id) = self.profiles.create(&name) {
                    self.editing = None;
                    cx.emit(ShellRequest::SwitchProfile(id));
                }
            }
            Some(Edit::Rename(id)) => {
                self.profiles.rename(&id, &name);
                if self.profiles.error.is_none() {
                    self.editing = None;
                    cx.emit(ShellRequest::ProfileRenamed);
                }
            }
            None => {}
        }
        cx.notify();
    }
}

impl Focusable for ProfilesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ProfilesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let entries = self.profiles.entries();
        let rows = entries.into_iter().enumerate().map(|(i, e)| {
            let (switch, rename, delete) = (e.profile.id.clone(), e.profile.id.clone(), e.profile.id.clone());
            let name = e.profile.name.clone();
            div()
                .id(("profile-row", i))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .px(px(6.))
                .py(px(4.))
                .rounded(px(6.))
                .hover(|s| s.bg(rgb(t.surface_variant)))
                .when(!e.current, |d| {
                    d.cursor_pointer().on_click(cx.listener(move |_, _, _, cx| cx.emit(ShellRequest::SwitchProfile(switch.clone()))))
                })
                .child(div().w(px(18.)).text_color(rgb(t.accent)).child(if e.current { "●" } else { "" }))
                .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(13.)).child(e.profile.name.clone()))
                .when(e.current, |d| d.child(div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child("this window")))
                .child(icon(t, ("profile-rename", i), "✎").on_click(cx.listener(move |this, _, w, cx| {
                    cx.stop_propagation();
                    this.start(Edit::Rename(rename.clone()), &name, w, cx);
                })))
                .when(!e.current, |d| {
                    d.child(icon(t, ("profile-delete", i), "×").on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.profiles.delete(&delete);
                        cx.notify();
                    })))
                })
        });
        let editing = self.editing.clone();
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(10.))
            .bg(rgb(t.surface))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(div().flex_1().text_size(px(13.)).child("Profiles"))
                    .child(icon(t, "profiles-close", "×").on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::ClosePanel)))),
            )
            .child(
                div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child("Each profile keeps its own tabs, cookies, bookmarks and history."),
            )
            .child(div().flex().flex_col().gap(px(1.)).children(rows))
            .child(match editing {
                Some(edit) => div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .p(px(6.))
                    .rounded(px(6.))
                    .bg(rgb(t.surface_variant))
                    .child(heading(t, if edit == Edit::New { "New profile" } else { "Rename profile" }))
                    .child(div().flex().child(self.input.clone()))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(6.))
                            .child(
                                button(t, "profile-save", if edit == Edit::New { "Create and switch" } else { "Save" }, true)
                                    .on_click(cx.listener(|this, _, w, cx| this.save(w, cx))),
                            )
                            .child(button(t, "profile-cancel", "Cancel", false).on_click(cx.listener(|this, _, _, cx| {
                                this.editing = None;
                                cx.notify();
                            }))),
                    )
                    .into_any_element(),
                None => div()
                    .flex()
                    .flex_row()
                    .child(button(t, "profile-new", "+ New profile", false).on_click(cx.listener(|this, _, w, cx| this.start(Edit::New, "", w, cx))))
                    .into_any_element(),
            })
            .children(self.profiles.error.clone().map(|e| div().text_size(px(12.)).text_color(rgb(t.error)).child(e)))
    }
}
