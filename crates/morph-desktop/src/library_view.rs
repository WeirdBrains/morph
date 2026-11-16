//! The library panel (`morph.library`): bookmarks with folders, tags and
//! search, history, and the password prompt while the profile's library is
//! locked. It lives in the sidebar slot and opens pages by asking the shell
//! ([`ShellRequest::Open`]); it never touches the browser.

use std::path::Path;

use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton, SharedString, Subscription, Window, div, prelude::*, px, rgb};
use morph_ui::theme::Theme;

use crate::library::{self, Library, Row, Tab};
use crate::shell::{Page, ShellRequest};
use crate::text_input::{TextInput, TextInputEvent};
use crate::widgets::{button, chip, heading, icon};

/// What the edit row is changing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Edit {
    NewFolder,
    RenameFolder(String),
    RenameBookmark(String),
    Tags(String),
}

pub struct LibraryView {
    library: Library,
    theme: Theme,
    search: Entity<TextInput>,
    password: Entity<TextInput>,
    edit_input: Entity<TextInput>,
    editing: Option<Edit>,
    /// The bookmark whose "move to" list is open.
    moving: Option<String>,
    rows: Vec<Row>,
    tags: Vec<morph_core::bookmarks::TagCount>,
    page: Option<Page>,
    marked: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShellRequest> for LibraryView {}

impl LibraryView {
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        for input in [&self.search, &self.password, &self.edit_input] {
            input.update(cx, |i, cx| i.set_theme(theme, cx));
        }
        cx.notify();
    }

    pub fn new(home: &Path, profile: &str, theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new("Search", theme, cx));
        let password = cx.new(|cx| TextInput::new("Profile password", theme, cx).masked());
        let edit_input = cx.new(|cx| TextInput::new("", theme, cx));
        let subs = vec![
            cx.subscribe_in(&search, window, |this, input, event, _, cx| match event {
                TextInputEvent::Changed => {
                    let q = input.read(cx).text().to_string();
                    this.library.set_query(&q);
                    this.reload(cx);
                }
                TextInputEvent::Submit { .. } => {
                    if let Some(url) = this.first_url() {
                        cx.emit(ShellRequest::Open { url, new_tab: false });
                    }
                }
                TextInputEvent::Cancel => cx.emit(ShellRequest::ClosePanel),
                TextInputEvent::Tab { .. } => {}
            }),
            cx.subscribe_in(&password, window, |this, input, event, window, cx| match event {
                TextInputEvent::Submit { .. } => this.unlock(window, cx),
                TextInputEvent::Tab { .. } => {}
                TextInputEvent::Cancel => cx.emit(ShellRequest::ClosePanel),
                TextInputEvent::Changed => {
                    let _ = input;
                }
            }),
            cx.subscribe_in(&edit_input, window, |this, _, event, window, cx| match event {
                TextInputEvent::Submit { .. } => this.save_edit(window, cx),
                TextInputEvent::Cancel => {
                    this.editing = None;
                    cx.notify();
                }
                TextInputEvent::Changed => {}
                TextInputEvent::Tab { .. } => {}
            }),
        ];
        let mut view = Self {
            library: Library::new(home, profile),
            theme,
            search,
            password,
            edit_input,
            editing: None,
            moving: None,
            rows: Vec::new(),
            tags: Vec::new(),
            page: None,
            marked: false,
            focus: cx.focus_handle(),
            _subscriptions: subs,
        };
        view.reload(cx);
        view
    }

    pub fn is_locked(&self) -> bool {
        self.library.is_locked()
    }

    pub fn tab(&self) -> Tab {
        self.library.tab
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.library.tab = tab;
        self.editing = None;
        self.moving = None;
        self.reload(cx);
    }

    /// Re-read the lists from the profile.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.rows = self.library.rows();
        self.tags = if self.library.tab == Tab::Bookmarks { self.library.tags() } else { Vec::new() };
        self.marked = self.page.as_ref().is_some_and(|p| self.library.is_bookmarked(&p.url));
        cx.notify();
    }

    /// Put the keyboard in the panel: the password while locked, else search.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = if self.library.is_locked() { self.password.clone() } else { self.search.clone() };
        target.update(cx, |input, cx| input.select_all(cx));
        window.focus(&target.focus_handle(cx), cx);
    }

    pub fn set_page(&mut self, page: Option<Page>, cx: &mut Context<Self>) {
        self.page = page;
        self.reload(cx);
    }

    /// Whether the current page is bookmarked.
    pub fn marked(&self) -> bool {
        self.marked
    }

    /// Bookmark the current page or remove its bookmark (Ctrl+D).
    pub fn toggle_bookmark(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(page) = self.page.clone() else {
            return false;
        };
        self.library.toggle_bookmark(&page.url, &page.title);
        self.reload(cx);
        true
    }

    fn first_url(&self) -> Option<String> {
        self.rows.iter().find_map(|r| match r {
            Row::Bookmark(b) => Some(b.url.clone()),
            Row::Visit(v) => Some(v.url.clone()),
            Row::Folder { .. } => None,
        })
    }

    fn unlock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.password.read(cx).text().to_string();
        if text.is_empty() {
            return;
        }
        self.library.unlock(&text);
        self.password.update(cx, |p, cx| p.set_text("", cx));
        self.reload(cx);
        if self.library.is_locked() {
            return;
        }
        if std::env::var_os("MORPH_LOG_EVENTS").is_some_and(|v| v == "1") {
            println!("morph-event library unlocked");
        }
        // The password field is gone: the keyboard goes to the search.
        self.focus(window, cx);
        // History starts recording now that writes succeed.
        cx.emit(ShellRequest::LibraryUnlocked);
    }

    fn start_edit(&mut self, edit: Edit, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(edit);
        self.moving = None;
        self.edit_input.update(cx, |i, cx| i.set_text(text, cx));
        window.focus(&self.edit_input.focus_handle(cx), cx);
        cx.emit(ShellRequest::FocusShell);
        cx.notify();
    }

    fn save_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.edit_input.read(cx).text().to_string();
        match self.editing.take() {
            Some(Edit::NewFolder) => self.library.new_folder(&text),
            Some(Edit::RenameFolder(id)) => self.library.rename_folder(&id, &text),
            Some(Edit::RenameBookmark(id)) => self.library.rename_bookmark(&id, &text),
            Some(Edit::Tags(id)) => self.library.set_tags(&id, &text),
            None => {}
        }
        window.focus(&self.search.focus_handle(cx), cx);
        self.reload(cx);
    }

    fn open(&self, url: &str, new_tab: bool, cx: &mut Context<Self>) {
        cx.emit(ShellRequest::Open { url: url.to_string(), new_tab });
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let tab = self.library.tab;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(
                chip(t, "lib-bookmarks", "Bookmarks", tab == Tab::Bookmarks).on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Bookmarks, cx))),
            )
            .child(chip(t, "lib-history", "History", tab == Tab::History).on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::History, cx))))
            .child(div().flex_1())
            .child(icon(t, "lib-close", "×").on_click(cx.listener(|_, _, _, cx| cx.emit(ShellRequest::ClosePanel))))
    }

    fn locked(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().text_size(px(13.)).child("This profile's library is locked."))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(t.text_secondary))
                    .child("Enter the profile password to see bookmarks and history and to record new visits."),
            )
            .child(div().flex().child(self.password.clone()))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .child(button(t, "lib-unlock", "Unlock", true).on_click(cx.listener(|this, _, window, cx| this.unlock(window, cx)))),
            )
    }

    fn edit_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let t = self.theme;
        let label = match self.editing.as_ref()? {
            Edit::NewFolder => "New folder name",
            Edit::RenameFolder(_) => "Folder name",
            Edit::RenameBookmark(_) => "Bookmark title",
            Edit::Tags(_) => "Tags, separated by commas",
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .p(px(6.))
                .rounded(px(6.))
                .bg(rgb(t.surface_variant))
                .child(heading(t, label))
                .child(div().flex().child(self.edit_input.clone()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(6.))
                        .child(button(t, "edit-save", "Save", true).on_click(cx.listener(|this, _, w, cx| this.save_edit(w, cx))))
                        .child(button(t, "edit-cancel", "Cancel", false).on_click(cx.listener(|this, _, _, cx| {
                            this.editing = None;
                            cx.notify();
                        }))),
                ),
        )
    }

    fn bookmarks_tools(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let path = self.library.path();
        let searching = !self.library.query().is_empty() || self.library.tag().is_some();
        let place: SharedString = if searching {
            "Search results".into()
        } else if path.is_empty() {
            "Bookmarks".into()
        } else {
            format!("Bookmarks / {}", path.join(" / ")).into()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .when(self.library.folder().is_some() && !searching, |d| {
                        d.child(icon(t, "lib-up", "‹").on_click(cx.listener(|this, _, _, cx| {
                            this.library.up();
                            this.reload(cx);
                        })))
                    })
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(12.))
                            .text_color(rgb(t.text_secondary))
                            .child(place),
                    )
                    .when(!searching, |d| {
                        d.child(
                            button(t, "lib-new-folder", "+ Folder", false)
                                .on_click(cx.listener(|this, _, w, cx| this.start_edit(Edit::NewFolder, "", w, cx))),
                        )
                    }),
            )
            .when(!self.tags.is_empty(), |d| {
                d.child(div().flex().flex_row().flex_wrap().gap(px(4.)).children(self.tags.iter().enumerate().map(|(i, tag)| {
                    let name = tag.name.clone();
                    chip(t, ("lib-tag", i), format!("#{} {}", tag.name, tag.count), self.library.tag() == Some(tag.name.as_str())).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.library.toggle_tag(&name);
                            this.reload(cx);
                        }),
                    )
                })))
            })
    }

    fn row(&self, index: usize, row: &Row, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = self.theme;
        let line = |title: String, sub: String| {
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(13.)).child(title))
                .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(sub))
        };
        let base = div().flex().flex_row().items_center().gap(px(2.)).px(px(4.)).py(px(3.)).rounded(px(6.)).hover(|s| s.bg(rgb(t.surface_variant)));
        match row {
            Row::Folder { id, name, folders, bookmarks } => {
                let (open, rename, delete) = (id.clone(), id.clone(), id.clone());
                let name_for_edit = name.clone();
                let count = folders + bookmarks;
                base.id(("lib-row", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library.enter(&open);
                        this.reload(cx);
                    }))
                    .child(div().w(px(18.)).child("▸"))
                    .child(line(name.clone(), if count == 1 { "1 item".into() } else { format!("{count} items") }))
                    .child(icon(t, ("lib-rename-folder", index), "✎").on_click(cx.listener(move |this, _, w, cx| {
                        cx.stop_propagation();
                        this.start_edit(Edit::RenameFolder(rename.clone()), &name_for_edit, w, cx);
                    })))
                    .child(icon(t, ("lib-delete-folder", index), "×").on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.library.delete_folder(&delete);
                        this.reload(cx);
                    })))
                    .into_any_element()
            }
            Row::Bookmark(b) => {
                let (url, url_new, url_middle) = (b.url.clone(), b.url.clone(), b.url.clone());
                let (id_tags, id_rename, id_move, id_delete) = (b.id.clone(), b.id.clone(), b.id.clone(), b.id.clone());
                let tags_text = b.tags.join(", ");
                let title_text = b.title.clone();
                let title = if b.title.is_empty() { b.url.clone() } else { b.title.clone() };
                let sub = if b.tags.is_empty() { b.url.clone() } else { format!("{}  ·  #{}", b.url, b.tags.join(" #")) };
                let moving = self.moving.as_deref() == Some(b.id.as_str());
                let row = base
                    .id(("lib-row", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.open(&url, false, cx)))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, _, cx| {
                            this.open(&url_middle, true, cx);
                        }),
                    )
                    .child(div().w(px(18.)).text_color(rgb(t.accent)).child("★"))
                    .child(line(title, sub))
                    .child(icon(t, ("lib-new-tab", index), "⧉").on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.open(&url_new, true, cx);
                    })))
                    .child(icon(t, ("lib-tags", index), "#").on_click(cx.listener(move |this, _, w, cx| {
                        cx.stop_propagation();
                        this.start_edit(Edit::Tags(id_tags.clone()), &tags_text, w, cx);
                    })))
                    .child(icon(t, ("lib-rename", index), "✎").on_click(cx.listener(move |this, _, w, cx| {
                        cx.stop_propagation();
                        this.start_edit(Edit::RenameBookmark(id_rename.clone()), &title_text, w, cx);
                    })))
                    .child(icon(t, ("lib-move", index), "↦").on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.moving = if this.moving.as_deref() == Some(id_move.as_str()) { None } else { Some(id_move.clone()) };
                        cx.notify();
                    })))
                    .child(icon(t, ("lib-delete", index), "×").on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.library.delete_bookmark(&id_delete);
                        this.reload(cx);
                    })));
                if !moving {
                    return row.into_any_element();
                }
                let item = b.id.clone();
                let targets = self.library.folders();
                div()
                    .flex()
                    .flex_col()
                    .child(row)
                    .child(div().flex().flex_col().gap(px(2.)).pl(px(22.)).pb(px(4.)).child(heading(t, "Move to")).children(
                        targets.into_iter().enumerate().map(|(i, (folder, name))| {
                            let item = item.clone();
                            chip(t, ("lib-move-to", i), name, folder.as_deref() == b.folder_id.as_deref()).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.library.move_to(&item, folder.as_deref());
                                    this.moving = None;
                                    this.reload(cx);
                                },
                            ))
                        }),
                    ))
                    .into_any_element()
            }
            Row::Visit(v) => {
                let (url, url_middle, url_delete) = (v.url.clone(), v.url.clone(), v.url.clone());
                let title = if v.title.is_empty() { v.url.clone() } else { v.title.clone() };
                let sub = format!("{}  ·  {}", library::ago(v.visited_ms, library::now_ms()), v.url);
                base.id(("lib-row", index))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.open(&url, false, cx)))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, _, cx| {
                            this.open(&url_middle, true, cx);
                        }),
                    )
                    .child(line(title, sub))
                    .child(icon(t, ("lib-forget", index), "×").on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.library.delete_visit(&url_delete);
                        this.reload(cx);
                    })))
                    .into_any_element()
            }
        }
    }
}

impl Focusable for LibraryView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let root = div().track_focus(&self.focus).size_full().flex().flex_col().gap(px(8.)).p(px(10.)).bg(rgb(t.surface)).child(self.header(cx));
        if self.library.is_locked() {
            return root
                .child(self.locked(cx))
                .children(self.library.error.clone().map(|e| div().text_size(px(12.)).text_color(rgb(t.error)).child(e)));
        }
        let tab = self.library.tab;
        let empty: SharedString = match tab {
            _ if !self.library.query().is_empty() || self.library.tag().is_some() => "Nothing matches.".into(),
            Tab::Bookmarks if self.library.folder().is_some() => "This folder is empty.".into(),
            Tab::Bookmarks => "No bookmarks yet. Press Ctrl+D or the star to bookmark a page.".into(),
            Tab::History => "No history yet.".into(),
        };
        let rows: Vec<gpui::AnyElement> = self.rows.clone().iter().enumerate().map(|(i, r)| self.row(i, r, cx)).collect();
        root.child(div().flex().child(self.search.clone()))
            .when(tab == Tab::Bookmarks, |d| d.child(self.bookmarks_tools(cx)))
            .when(tab == Tab::History && !self.rows.is_empty(), |d| {
                d.child(div().flex().flex_row().child(button(t, "lib-clear-history", "Clear history", false).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.library.clear_history();
                        this.reload(cx);
                    },
                ))))
            })
            .children(self.edit_row(cx))
            .children(self.library.notice.clone().map(|n| div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(n)))
            .children(self.library.error.clone().map(|e| div().text_size(px(12.)).text_color(rgb(t.error)).child(e)))
            .child(
                div()
                    .id("lib-rows")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .when(rows.is_empty(), |d| d.child(div().pt(px(12.)).text_size(px(12.)).text_color(rgb(t.text_secondary)).child(empty)))
                    .children(rows),
            )
    }
}
