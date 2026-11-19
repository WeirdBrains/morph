//! Draws a Companion answer's [`Markdown`] blocks with the brand tokens. Text is laid out word by
//! word in wrapping rows so mixed styles (bold, code, links) flow like a paragraph. Links open in a
//! new tab through `ShellRequest::Open`; footnote markers focus their footnote below the answer.

use gpui::{ClipboardItem, FontWeight, SharedString, div, prelude::*, px, rgb};
use morph_companion::{Block, Markdown, Span};
use morph_ui::theme::Theme;

use crate::companion_view::CompanionView;
use crate::shell::ShellRequest;
use gpui::Context;

/// All blocks of `doc`. `entry` keeps element ids unique across answers.
pub fn blocks(t: Theme, doc: &Markdown, entry: usize, cx: &mut Context<CompanionView>) -> Vec<gpui::AnyElement> {
    let mut next = 0usize;
    doc.blocks().map(|b| block(t, b, entry, &mut next, cx)).collect()
}

fn block(t: Theme, b: &Block, entry: usize, next: &mut usize, cx: &mut Context<CompanionView>) -> gpui::AnyElement {
    match b {
        Block::Paragraph(spans) => inline(t, spans, 13., false, entry, next, cx).into_any_element(),
        Block::Heading(level, spans) => {
            let size = match level {
                1 => 17.,
                2 => 15.,
                _ => 13.5,
            };
            inline(t, spans, size, true, entry, next, cx).mt(px(4.)).into_any_element()
        }
        Block::Code { lang, text, closed } => code_block(t, lang.as_deref(), text, *closed, entry, next, cx).into_any_element(),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .children(items.iter().enumerate().map(|(i, item)| {
                let marker = match start {
                    Some(n) => format!("{}.", n + i as u64),
                    None => "•".to_string(),
                };
                let inner: Vec<gpui::AnyElement> = item.iter().map(|b| block(t, b, entry, next, cx)).collect();
                div()
                    .flex()
                    .flex_row()
                    .gap(px(6.))
                    .child(div().w(px(20.)).flex_none().text_size(px(13.)).text_color(rgb(t.accent)).child(marker))
                    .child(div().flex_1().min_w(px(0.)).flex().flex_col().gap(px(3.)).children(inner))
            }))
            .into_any_element(),
        Block::Quote(inner) => {
            let inner: Vec<gpui::AnyElement> = inner.iter().map(|b| block(t, b, entry, next, cx)).collect();
            div()
                .pl(px(8.))
                .border_l_2()
                .border_color(rgb(t.border))
                .text_color(rgb(t.text_secondary))
                .flex()
                .flex_col()
                .gap(px(3.))
                .children(inner)
                .into_any_element()
        }
        Block::Rule => div().h(px(1.)).w_full().bg(rgb(t.border)).into_any_element(),
    }
}

/// Split into words that keep their trailing whitespace, so rows can wrap between them.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_space = false;
    for c in text.chars() {
        if c == '\n' {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push("\n".into());
            in_space = false;
            continue;
        }
        if c.is_whitespace() {
            in_space = true;
        } else if in_space {
            if cur.chars().any(|c| !c.is_whitespace()) {
                out.push(std::mem::take(&mut cur));
            }
            in_space = false;
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn inline(t: Theme, spans: &[Span], size: f32, heading: bool, entry: usize, next: &mut usize, cx: &mut Context<CompanionView>) -> gpui::Div {
    let mut row = div().flex().flex_row().flex_wrap().text_size(px(size)).text_color(rgb(t.text_primary));
    for s in spans {
        for w in words(&s.text) {
            if w == "\n" {
                row = row.child(div().w_full().h(px(0.)));
                continue;
            }
            *next += 1;
            let id = ("md", entry * 100_000 + *next);
            let mut d = div().id(id).child(SharedString::from(w));
            if s.bold || heading {
                d = d.font_weight(FontWeight::BOLD);
            }
            if s.italic {
                d = d.italic();
            }
            if s.code {
                d = d.font_family("monospace").text_size(px(size - 1.)).bg(rgb(t.background)).text_color(rgb(t.accent_alt));
            }
            if s.dim {
                d = d.text_size(px(size - 2.)).text_color(rgb(t.text_secondary));
            }
            if let Some(link) = &s.link {
                let url = link.url.clone();
                d = d
                    .text_color(rgb(t.accent))
                    .underline()
                    .cursor_pointer()
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(ShellRequest::Open { url: url.clone(), new_tab: true })));
            }
            if let Some(n) = s.footnote {
                d = d.text_color(rgb(t.accent)).cursor_pointer().on_click(cx.listener(move |this, _, _, cx| this.focus_footnote(n, cx)));
            }
            row = row.child(d);
        }
    }
    row
}

fn code_block(t: Theme, lang: Option<&str>, text: &str, closed: bool, entry: usize, next: &mut usize, cx: &mut Context<CompanionView>) -> gpui::Div {
    *next += 1;
    let copy = text.to_string();
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .child(div().flex_1().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(match (lang, closed) {
            (Some(l), true) => l.to_string(),
            (Some(l), false) => format!("{l} …"),
            (None, true) => String::new(),
            (None, false) => "…".to_string(),
        }))
        .child(
            div()
                .id(("md-copy", entry * 100_000 + *next))
                .px(px(6.))
                .rounded(px(6.))
                .text_size(px(11.))
                .text_color(rgb(t.accent))
                .border_1()
                .border_color(rgb(t.surface_variant))
                .cursor_pointer()
                .child("Copy")
                .on_click(cx.listener(move |_, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())))),
        );
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(8.))
        .rounded(px(6.))
        .bg(rgb(t.background))
        .border_1()
        .border_color(rgb(t.surface_variant))
        .child(header)
        .child(
            div()
                .flex()
                .flex_col()
                .overflow_hidden()
                .font_family("monospace")
                .text_size(px(12.))
                .text_color(rgb(t.text_primary))
                .children(text.split('\n').map(|l| div().whitespace_nowrap().min_h(px(15.)).child(SharedString::from(l.replace(' ', "\u{a0}"))))),
        )
}

#[cfg(test)]
mod tests {
    use super::words;

    #[test]
    fn words_keep_their_trailing_space_and_split_lines() {
        assert_eq!(words("Two plus  two"), ["Two ", "plus  ", "two"]);
        assert_eq!(words("a\nb"), ["a", "\n", "b"]);
        assert_eq!(words(" lead"), [" lead"]);
        assert!(words("").is_empty());
    }
}
