//! Small pieces the shell's panels share: buttons, chips and headings,
//! drawn in the theme's colours.

use gpui::{Div, ElementId, SharedString, Stateful, div, prelude::*, px, rgb};
use morph_ui::theme::Theme;

/// A text button; `primary` fills it with the accent colour.
pub fn button(t: Theme, id: impl Into<ElementId>, label: impl Into<SharedString>, primary: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .px(px(10.))
        .h(px(26.))
        .rounded(px(6.))
        .text_size(px(12.))
        .bg(rgb(if primary { t.accent } else { t.surface_variant }))
        .text_color(rgb(if primary { t.background } else { t.text_primary }))
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .child(label.into())
}

/// A small square button holding one glyph.
pub fn icon(t: Theme, id: impl Into<ElementId>, glyph: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(22.))
        .h(px(22.))
        .rounded(px(4.))
        .text_size(px(12.))
        .text_color(rgb(t.text_secondary))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.surface_variant)).text_color(rgb(t.text_primary)))
        .child(glyph.into())
}

/// A pill that can be selected (tab headers, tag filters, options).
pub fn chip(t: Theme, id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(8.))
        .py(px(2.))
        .rounded(px(10.))
        .text_size(px(12.))
        .border_1()
        .border_color(rgb(if selected { t.accent } else { t.surface_variant }))
        .bg(rgb(if selected { t.surface_variant } else { t.surface }))
        .text_color(rgb(if selected { t.text_primary } else { t.text_secondary }))
        .cursor_pointer()
        .child(label.into())
}

/// A section heading in a panel.
pub fn heading(t: Theme, text: impl Into<SharedString>) -> Div {
    div().text_size(px(11.)).text_color(rgb(t.text_secondary)).child(text.into())
}
