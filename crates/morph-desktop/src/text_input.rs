//! A one-line text input for the shell's own panels (find bar, library
//! search, profile names, the library password). It edits a
//! `morph_ui::text::TextField`, registers itself as the window's input
//! handler while focused (typing and input methods), and tells its owner
//! about edits, Enter and Escape through [`TextInputEvent`]. The address bar
//! keeps its own element (`address.rs`), which this one follows.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, point, prelude::*, px, relative, rgb, rgba,
    size,
};
use morph_ui::text::TextField;
use morph_ui::theme::Theme;
use unicode_segmentation::UnicodeSegmentation;

actions!(text_input, [Backspace, Delete, Left, Right, SelectLeft, SelectRight, SelectAll, Home, End, Paste, Copy, Cut, Submit, SubmitBack, Cancel]);

/// The key context the input's bindings apply in.
pub const CONTEXT: &str = "TextInput";

/// What the owner hears about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputEvent {
    /// The text changed (typed, pasted, deleted).
    Changed,
    /// Enter; `back` is Shift+Enter.
    Submit { back: bool },
    /// Escape.
    Cancel,
}

/// Keys for the input's editing actions, in its key context.
pub fn bindings() -> Vec<KeyBinding> {
    let c = Some(CONTEXT);
    vec![
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("shift-insert", Paste, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new("shift-enter", SubmitBack, c),
        KeyBinding::new("escape", Cancel, c),
    ]
    .into_iter()
    .chain(
        crate::shell::MAC
            .then(|| {
                [
                    KeyBinding::new("cmd-a", SelectAll, c),
                    KeyBinding::new("cmd-v", Paste, c),
                    KeyBinding::new("cmd-c", Copy, c),
                    KeyBinding::new("cmd-x", Cut, c),
                ]
            })
            .into_iter()
            .flatten(),
    )
    .collect()
}

pub struct TextInput {
    field: TextField,
    focus: FocusHandle,
    placeholder: SharedString,
    /// Shown as bullets and never copied (passwords).
    masked: bool,
    theme: Theme,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    selecting: bool,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, theme: Theme, cx: &mut Context<Self>) -> Self {
        Self {
            field: TextField::new(),
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            masked: false,
            theme,
            last_layout: None,
            last_bounds: None,
            selecting: false,
        }
    }

    /// A password input: bullets, no copy or cut.
    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    pub fn text(&self) -> &str {
        self.field.text()
    }

    /// Reuse the input for another field: its hint and whether it hides
    /// what is typed.
    pub fn configure(&mut self, placeholder: impl Into<SharedString>, masked: bool, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        self.masked = masked;
        cx.notify();
    }

    /// Replace the text, all of it selected. Does not emit `Changed`.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.field.set_text_selected(text);
        cx.notify();
    }

    /// Select all of the text, so typing replaces it.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.field.select_all();
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut TextField)) {
        let before = self.field.text().to_string();
        f(&mut self.field);
        if self.field.text() != before {
            cx.emit(TextInputEvent::Changed);
        }
        cx.notify();
    }

    /// What is drawn: the text, or bullets for a masked input.
    fn display(&self) -> String {
        if self.masked { "•".repeat(self.field.text().graphemes(true).count()) } else { self.field.text().to_string() }
    }

    /// `offset` in the field's text as an offset in [`Self::display`].
    fn display_offset(&self, offset: usize) -> usize {
        if self.masked { masked_offset(self.field.text(), offset) } else { offset }
    }

    fn field_offset(&self, offset: usize) -> usize {
        if self.masked { unmasked_offset(self.field.text(), offset) } else { offset }
    }

    fn index_for_point(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        if self.field.text().is_empty() || line.text.as_ref() != self.display() {
            return 0;
        }
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.field.text().len();
        }
        self.field_offset(line.closest_index_for_x(position.x - bounds.left()))
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.index_for_point(event.position);
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        if event.modifiers.shift {
            self.field.select_to(index);
        } else {
            self.field.move_to(index);
        }
        self.selecting = true;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            let index = self.index_for_point(event.position);
            self.field.select_to(index);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.edit(cx, |f| f.replace(None, &text));
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.masked
            && let Some(text) = self.field.selected_text()
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.masked
            && let Some(text) = self.field.selected_text()
        {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            self.edit(cx, |f| f.replace(None, ""));
        }
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let focused = self.focus.is_focused(window);
        div()
            .flex_1()
            .flex()
            .items_center()
            .h(px(28.))
            .px(px(8.))
            .rounded(px(6.))
            .bg(rgb(t.background))
            .border_1()
            .border_color(rgb(if focused { t.accent } else { t.surface_variant }))
            .text_color(rgb(t.text_primary))
            .text_size(px(13.))
            .line_height(px(18.))
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(|this, _: &Backspace, _, cx| this.edit(cx, |f| f.backspace())))
            .on_action(cx.listener(|this, _: &Delete, _, cx| this.edit(cx, |f| f.delete())))
            .on_action(cx.listener(|this, _: &Left, _, cx| this.edit(cx, |f| f.left())))
            .on_action(cx.listener(|this, _: &Right, _, cx| this.edit(cx, |f| f.right())))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.edit(cx, |f| f.select_left())))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.edit(cx, |f| f.select_right())))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.edit(cx, |f| f.select_all())))
            .on_action(cx.listener(|this, _: &Home, _, cx| this.edit(cx, |f| f.home())))
            .on_action(cx.listener(|this, _: &End, _, cx| this.edit(cx, |f| f.end())))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(|_, _: &Submit, _, cx| cx.emit(TextInputEvent::Submit { back: false })))
            .on_action(cx.listener(|_, _: &SubmitBack, _, cx| cx.emit(TextInputEvent::Submit { back: true })))
            .on_action(cx.listener(|_, _: &Cancel, _, cx| cx.emit(TextInputEvent::Cancel)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(InputElement { input: cx.entity() })
    }
}

/// Byte offset in the bullet string for byte offset `offset` in `text`.
fn masked_offset(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true).take_while(|(i, _)| *i < offset).count() * '•'.len_utf8()
}

/// Byte offset in `text` for byte offset `offset` in its bullet string.
fn unmasked_offset(text: &str, offset: usize) -> usize {
    let n = offset / '•'.len_utf8();
    text.grapheme_indices(true).nth(n).map(|(i, _)| i).unwrap_or(text.len())
}

/// Byte offset of UTF-16 offset `utf16` within `text`, clamped to its end.
fn utf16_to_byte(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        if self.masked {
            return None;
        }
        let range = self.field.range_from_utf16(&range_utf16);
        actual.replace(self.field.range_to_utf16(&range));
        self.field.text().get(range).map(str::to_string)
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.field.range_to_utf16(&self.field.selection()), reversed: self.field.is_reversed() })
    }

    fn set_selected_text_range(&mut self, range_utf16: Range<usize>, _: &mut Window, cx: &mut Context<Self>) {
        self.field.set_selection_utf16(range_utf16);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.field.len_utf16())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.field.marked().map(|r| self.field.range_to_utf16(&r))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.field.unmark();
        cx.notify();
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let range = range_utf16.map(|r| self.field.range_from_utf16(&r));
        self.edit(cx, |f| f.replace(range, text));
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|r| self.field.range_from_utf16(&r));
        let selected = selected_utf16.map(|r| utf16_to_byte(text, r.start)..utf16_to_byte(text, r.end));
        self.edit(cx, |f| f.replace_and_mark(range, text, selected));
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let range = self.field.range_from_utf16(&range_utf16);
        let (start, end) = (self.display_offset(range.start), self.display_offset(range.end));
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(start), bounds.top()),
            point(bounds.left() + line.x_for_index(end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let local = self.last_bounds?.localize(&p)?;
        let line = self.last_layout.as_ref()?;
        Some(self.field.offset_to_utf16(self.field_offset(line.closest_index_for_x(local.x))))
    }
}

struct InputElement {
    input: Entity<TextInput>,
}

struct Prepainted {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl gpui::Element for InputElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepainted {
        let input = self.input.read(cx);
        let focused = input.focus.is_focused(window);
        let field = &input.field;
        let theme = input.theme;
        let style = window.text_style();
        let empty = field.text().is_empty();
        let (text, color): (SharedString, gpui::Hsla) = if empty {
            (input.placeholder.clone(), rgb(theme.text_secondary).into())
        } else {
            (input.display().into(), rgb(theme.text_primary).into())
        };
        let run = TextRun { len: text.len(), font: style.font(), color, background_color: None, underline: None, strikethrough: None };
        let runs = match field.marked().filter(|_| !empty && !input.masked) {
            Some(m) => vec![
                TextRun { len: m.start, ..run.clone() },
                TextRun {
                    len: m.end - m.start,
                    underline: Some(UnderlineStyle { color: Some(run.color), thickness: px(1.), wavy: false }),
                    ..run.clone()
                },
                TextRun { len: text.len() - m.end, ..run },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            None => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window.text_system().shape_line(text, font_size, &runs, None);
        let selection = field.selection();
        let (selection, cursor) = if !focused {
            (None, None)
        } else if selection.is_empty() || empty {
            let x = if empty { px(0.) } else { line.x_for_index(input.display_offset(field.cursor())) };
            (None, Some(fill(Bounds::new(point(bounds.left() + x, bounds.top()), size(px(2.), bounds.size.height)), rgb(theme.accent))))
        } else {
            let from = point(bounds.left() + line.x_for_index(input.display_offset(selection.start)), bounds.top());
            let to = point(bounds.left() + line.x_for_index(input.display_offset(selection.end)), bounds.bottom());
            (Some(fill(Bounds::from_corners(from, to), rgba((theme.accent << 8) | 0x55))), None)
        };
        Prepainted { line: Some(line), cursor, selection }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepainted: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx);
        if let Some(selection) = prepainted.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepainted.line.take() else {
            return;
        };
        let _ = line.paint(bounds.origin, window.line_height(), gpui::TextAlign::Left, None, window, cx);
        if let Some(cursor) = prepainted.cursor.take() {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_offsets_count_graphemes_as_bullets() {
        let text = "aé🙂b";
        assert_eq!(masked_offset(text, 0), 0);
        assert_eq!(masked_offset(text, 1), 3);
        assert_eq!(masked_offset(text, 3), 6);
        assert_eq!(masked_offset(text, text.len()), 12);
        assert_eq!(unmasked_offset(text, 0), 0);
        assert_eq!(unmasked_offset(text, 6), 3);
        assert_eq!(unmasked_offset(text, 12), text.len());
        assert_eq!(unmasked_offset(text, 99), text.len());
    }

    #[test]
    fn utf16_offsets_map_to_byte_offsets() {
        assert_eq!(utf16_to_byte("é🙂", 1), 2);
        assert_eq!(utf16_to_byte("é🙂", 9), 6);
    }
}
