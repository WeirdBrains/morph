//! The address bar's suggestions: while text is typed, pages from history and
//! bookmarks that match (best first) and a "search for" row drop down below
//! the bar; the best match completes inline as selected text, which Right
//! accepts and typing, Backspace or Escape drop.

use std::path::Path;
use std::rc::Rc;

use gpui::{MouseButton, MouseDownEvent, SharedString, Window, anchored, deferred, div, point, prelude::*, px, rgb};
use morph_core::suggest::{self, Index, Source, Suggestion};

use super::{BrowserView, Context};

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Page(Suggestion),
    /// Search the configured engine for this text.
    Search(String),
}

/// The list under the address bar while the user types.
pub struct Suggestions {
    pub rows: Vec<Row>,
    /// The row the keyboard is on.
    pub selected: Option<usize>,
    /// Where the inline completion starts in the bar's text (it runs to the
    /// end), while it is still the bar's selection.
    pub completion: Option<usize>,
}

/// History and bookmarks of a profile, read when the user starts typing.
pub fn load_index(home: &Path, profile: &str) -> Index {
    let history = morph_core::library::history(home, profile, "", 0).unwrap_or_default();
    let bookmarks: Vec<(String, String)> =
        morph_core::bookmarks::bookmarks(home, profile).unwrap_or_default().into_iter().map(|b| (b.url, b.title)).collect();
    Index::build(&history, &bookmarks)
}

impl BrowserView {
    /// Where history and bookmarks come from (the window's profile).
    pub fn set_suggest_source(&mut self, home: &Path, profile: &str) {
        self.suggest_source = Some((home.to_path_buf(), profile.to_string()));
    }

    /// After an edit changed the bar's text: refresh the list and, when the
    /// user added text at the end, complete it inline.
    pub(super) fn text_changed(&mut self, typed_before: usize, complete: bool, cx: &mut Context<Self>) {
        let field = self.browser.address();
        let text = field.text().to_string();
        let grew = text.len() > typed_before;
        let at_end = field.cursor() == text.len() && field.selection().is_empty() && field.marked().is_none();
        if text.trim().is_empty() || !self.browser.address_focused() {
            self.suggest = None;
            return;
        }
        if self.suggest_index.is_none() {
            let started = std::time::Instant::now();
            if let Some((home, profile)) = &self.suggest_source {
                self.suggest_index = Some(Rc::new(load_index(home, profile)));
            }
            if let Some(index) = &self.suggest_index {
                super::log_event(&format!("suggestions index pages={} loaded_ms={}", index.len(), started.elapsed().as_millis()));
            }
        }
        let now = crate::library::now_ms();
        let mut rows: Vec<Row> = match &self.suggest_index {
            Some(index) => index.suggest(&text, now, suggest::MAX_SUGGESTIONS).into_iter().map(Row::Page).collect(),
            None => Vec::new(),
        };
        rows.push(Row::Search(text.trim().to_string()));
        let mut completion = None;
        if grew
            && complete
            && at_end
            && let Some(full) = self.suggest_index.as_ref().and_then(|i| i.complete(&text, now))
            && let Some(rest) = full.strip_prefix(&text)
            && !rest.is_empty()
        {
            let rest = rest.to_string();
            completion = Some(text.len());
            self.browser.address_mut().append_selected(&rest);
        }
        self.open_address_menu();
        self.suggest = Some(Suggestions { rows, selected: None, completion });
        self.log_suggestions();
        cx.notify();
    }

    /// The bar's text without the inline completion.
    pub(super) fn typed_text(&self) -> String {
        let text = self.browser.address().text();
        match self.suggest.as_ref().and_then(|s| s.completion) {
            Some(start) if self.completion_selected(start) => text[..start].to_string(),
            _ => text.to_string(),
        }
    }

    /// Whether the inline completion from `start` is still the selection.
    fn completion_selected(&self, start: usize) -> bool {
        let field = self.browser.address();
        field.selection() == (start..field.text().len())
    }

    /// Forget the inline completion once it is no longer the selection
    /// (Right accepted it, or the caret moved off it).
    pub(super) fn settle_completion(&mut self) {
        if let Some(start) = self.suggest.as_ref().and_then(|s| s.completion)
            && !self.completion_selected(start)
            && let Some(s) = self.suggest.as_mut()
        {
            s.completion = None;
        }
    }

    pub(super) fn log_suggestions(&self) {
        if !super::log_events() {
            return;
        }
        let Some(s) = &self.suggest else {
            return;
        };
        let urls: Vec<&str> = s
            .rows
            .iter()
            .map(|r| match r {
                Row::Page(p) => p.url.as_str(),
                Row::Search(_) => "search",
            })
            .collect();
        let field = self.browser.address();
        println!(
            "morph-event suggestions text={:?} selection={}..{} completion={} rows={:?}",
            field.text(),
            field.selection().start,
            field.selection().end,
            s.completion.is_some(),
            urls
        );
    }

    /// Up and Down in the list. Returns false when there is no list.
    pub(super) fn step_suggestions(&mut self, down: bool, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.suggest.as_mut() else {
            return false;
        };
        s.selected = super::step(s.selected, s.rows.len(), down);
        cx.notify();
        true
    }

    /// Escape: close the list and drop the inline completion, leaving the
    /// typed text and the caret in the bar. Returns false when no list shows.
    pub(super) fn dismiss_suggestions(&mut self, cx: &mut Context<Self>) -> bool {
        if self.suggest.is_none() {
            return false;
        }
        let typed = self.typed_text();
        let end = self.browser.address().text().len();
        if typed.len() < end {
            self.browser.address_mut().replace(Some(typed.len()..end), "");
        }
        self.suggest = None;
        self.close_address_menu(cx);
        super::log_event("suggestions closed");
        cx.notify();
        true
    }

    /// Open the highlighted row (Enter). Returns false when none is.
    pub(super) fn open_selected_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(row) = self.suggest.as_ref().and_then(|s| s.selected.and_then(|i| s.rows.get(i).cloned())) else {
            return false;
        };
        self.open_suggestion(row, window, cx);
        true
    }

    pub(super) fn open_suggestion(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        let target = match row {
            Row::Page(page) => page.url,
            Row::Search(query) => self.browser.tab_engine().url(&query),
        };
        super::log_event(&format!("suggestion opened {target}"));
        self.suggest = None;
        self.close_address_menu(cx);
        self.browser.address_mut().set_text_selected(&target);
        let internal = self.browser.submit_address();
        self.sync_focus(window, cx);
        if let Some(internal) = internal {
            self.open_internal(internal, cx);
        }
        cx.notify();
    }

    /// The list, below the address bar, with a backdrop that closes it on a
    /// click elsewhere.
    pub(super) fn suggestions_overlay(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = self.theme;
        let (Some(list), Some(bar)) = (&self.suggest, self.button_bounds.borrow().get("address").copied()) else {
            return Vec::new();
        };
        let engine = self.browser.tab_engine();
        let backdrop = div().id("suggestions-backdrop").absolute().top_0().left_0().size_full().on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                if !bar.contains(&event.position) {
                    this.suggest = None;
                    this.leave_address(window, cx);
                }
            }),
        );
        let rows = list.rows.iter().enumerate().map(|(i, row)| {
            let (glyph, title, detail): (SharedString, SharedString, SharedString) = match row {
                Row::Page(p) => {
                    let glyph = if p.source == Source::Bookmark { "★" } else { "↺" };
                    let shown = suggest::key(&p.url);
                    let title = if p.title.is_empty() { shown.clone() } else { p.title.clone() };
                    (glyph.into(), title.into(), if p.title.is_empty() { SharedString::default() } else { shown.into() })
                }
                Row::Search(q) => (engine.mark.into(), format!("Search {} for “{q}”", engine.name).into(), SharedString::default()),
            };
            let open = row.clone();
            div()
                .id(("suggestion", i))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .h(px(34.))
                .px(px(14.))
                .text_size(px(13.))
                .text_color(rgb(t.text_primary))
                .cursor_pointer()
                .when(list.selected == Some(i), |d| d.bg(rgb(t.surface_variant)))
                .hover(|s| s.bg(rgb(t.surface_variant)))
                .on_click(cx.listener(move |this: &mut Self, _: &gpui::ClickEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_suggestion(open.clone(), window, cx);
                }))
                .child(div().w(px(16.)).flex_none().text_color(rgb(t.accent)).child(glyph))
                .child(
                    div().flex_none().max_w(px(f32::from(bar.size.width) * 0.55)).overflow_hidden().text_ellipsis().whitespace_nowrap().child(title),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_size(px(12.))
                        .text_color(rgb(t.text_secondary))
                        .child(detail),
                )
        });
        let top = bar.bottom() + px(4.);
        self.log_once(
            "suggestions-box",
            format!(
                "suggestions box left={:.0} top={:.0} width={:.0} bar_bottom={:.0} rows={}",
                f32::from(bar.left()),
                f32::from(top),
                f32::from(bar.size.width),
                f32::from(bar.bottom()),
                list.rows.len()
            ),
        );
        let panel = div()
            .id("suggestions")
            .occlude()
            .w(bar.size.width)
            .py(px(4.))
            .rounded(px(10.))
            .bg(rgb(t.surface))
            .border_1()
            .border_color(rgb(t.border))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(rows);
        vec![
            deferred(backdrop).with_priority(1).into_any_element(),
            deferred(anchored().position(point(bar.left(), top)).snap_to_window_with_margin(px(4.)).child(panel)).with_priority(2).into_any_element(),
        ]
    }
}
