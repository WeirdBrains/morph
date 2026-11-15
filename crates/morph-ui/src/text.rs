//! Editing rules for a one-line text field (the address bar), kept out of
//! the toolkit so they can be tested. Offsets are UTF-8 byte offsets on
//! grapheme boundaries; the UTF-16 helpers serve platform input methods.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    content: String,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
}

impl TextField {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn selection(&self) -> Range<usize> {
        self.selected.clone()
    }

    pub fn is_reversed(&self) -> bool {
        self.reversed
    }

    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    /// Where the caret is: the moving end of the selection.
    pub fn cursor(&self) -> usize {
        if self.reversed { self.selected.start } else { self.selected.end }
    }

    /// Replace the whole text and select all of it, as an address bar does
    /// when a page loads or the field is focused.
    pub fn set_text_selected(&mut self, text: &str) {
        self.content = single_line(text);
        self.selected = 0..self.content.len();
        self.reversed = false;
        self.marked = None;
    }

    /// Replace `range` (or the marked text, else the selection) with `text`
    /// and put the caret after it.
    pub fn replace(&mut self, range: Option<Range<usize>>, text: &str) {
        let text = single_line(text);
        let range = self.clamp(range.or_else(|| self.marked.clone()).unwrap_or_else(|| self.selected.clone()));
        self.content.replace_range(range.clone(), &text);
        let end = range.start + text.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
    }

    /// Like [`Self::replace`] for input-method composition: the new text is
    /// marked, and `selected_in_new` (relative to it) becomes the selection.
    pub fn replace_and_mark(&mut self, range: Option<Range<usize>>, text: &str, selected_in_new: Option<Range<usize>>) {
        let text = single_line(text);
        let range = self.clamp(range.or_else(|| self.marked.clone()).unwrap_or_else(|| self.selected.clone()));
        self.content.replace_range(range.clone(), &text);
        let start = range.start;
        self.marked = (!text.is_empty()).then(|| start..start + text.len());
        self.selected = match selected_in_new {
            Some(sel) => start + sel.start.min(text.len())..start + sel.end.min(text.len()),
            None => start + text.len()..start + text.len(),
        };
        self.reversed = false;
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }

    pub fn backspace(&mut self) {
        if self.selected.is_empty() {
            let prev = self.previous_boundary(self.cursor());
            self.selected = prev..self.selected.end;
        }
        self.replace(None, "");
    }

    pub fn delete(&mut self) {
        if self.selected.is_empty() {
            let next = self.next_boundary(self.cursor());
            self.selected = self.selected.start..next;
        }
        self.replace(None, "");
    }

    pub fn left(&mut self) {
        let to = if self.selected.is_empty() { self.previous_boundary(self.cursor()) } else { self.selected.start };
        self.move_to(to);
    }

    pub fn right(&mut self) {
        let to = if self.selected.is_empty() { self.next_boundary(self.cursor()) } else { self.selected.end };
        self.move_to(to);
    }

    pub fn select_left(&mut self) {
        self.select_to(self.previous_boundary(self.cursor()));
    }

    pub fn select_right(&mut self) {
        self.select_to(self.next_boundary(self.cursor()));
    }

    pub fn home(&mut self) {
        self.move_to(0);
    }

    pub fn end(&mut self) {
        self.move_to(self.content.len());
    }

    pub fn select_all(&mut self) {
        self.selected = 0..self.content.len();
        self.reversed = false;
    }

    /// The selected text, if any.
    pub fn selected_text(&self) -> Option<&str> {
        (!self.selected.is_empty()).then(|| &self.content[self.selected.clone()])
    }

    pub fn move_to(&mut self, offset: usize) {
        let offset = self.snap(offset);
        self.selected = offset..offset;
        self.reversed = false;
    }

    /// Extend the selection's moving end to `offset`.
    pub fn select_to(&mut self, offset: usize) {
        let offset = self.snap(offset);
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
    }

    /// Set the selection from a UTF-16 range (platform input methods).
    pub fn set_selection_utf16(&mut self, range: Range<usize>) {
        self.selected = self.range_from_utf16(&range);
        self.reversed = false;
    }

    pub fn len_utf16(&self) -> usize {
        self.content.encode_utf16().count()
    }

    pub fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).rev().find_map(|(i, _)| (i < offset).then_some(i)).unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).find_map(|(i, _)| (i > offset).then_some(i)).unwrap_or(self.content.len())
    }

    /// The nearest grapheme boundary at or before `offset`.
    fn snap(&self, offset: usize) -> usize {
        if offset >= self.content.len() {
            return self.content.len();
        }
        self.content.grapheme_indices(true).map(|(i, _)| i).take_while(|&i| i <= offset).last().unwrap_or(0)
    }

    fn clamp(&self, range: Range<usize>) -> Range<usize> {
        let start = self.snap(range.start.min(range.end));
        let end = self.snap(range.end.max(range.start));
        start..end
    }
}

/// Address-bar text is one line: line breaks become spaces.
fn single_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::TextField;

    fn typed(text: &str) -> TextField {
        let mut field = TextField::new();
        for ch in text.chars() {
            field.replace(None, &ch.to_string());
        }
        field
    }

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut field = typed("helo");
        field.left();
        field.replace(None, "l");
        assert_eq!(field.text(), "hello");
        assert_eq!(field.cursor(), 4);
    }

    #[test]
    fn backspace_and_delete_remove_whole_graphemes() {
        let mut field = typed("ae\u{301}🙂z");
        field.left();
        field.backspace();
        assert_eq!(field.text(), "ae\u{301}z");
        field.home();
        field.right();
        field.delete();
        assert_eq!(field.text(), "az");
        field.home();
        field.backspace();
        assert_eq!(field.text(), "az", "nothing before the start");
    }

    #[test]
    fn loading_a_page_selects_its_address_so_typing_replaces_it() {
        let mut field = typed("old");
        field.set_text_selected("https://example.com/");
        assert_eq!(field.selected_text(), Some("https://example.com/"));
        field.replace(None, "n");
        assert_eq!(field.text(), "n");
    }

    #[test]
    fn shift_selection_can_reverse() {
        let mut field = typed("abcd");
        field.select_left();
        field.select_left();
        assert_eq!(field.selected_text(), Some("cd"));
        assert!(field.is_reversed());
        field.select_right();
        assert_eq!(field.selected_text(), Some("d"));
        field.select_all();
        assert_eq!(field.selected_text(), Some("abcd"));
    }

    #[test]
    fn pasted_line_breaks_become_spaces() {
        let mut field = TextField::new();
        field.replace(None, "how to\nfix it");
        assert_eq!(field.text(), "how to fix it");
    }

    #[test]
    fn composition_marks_text_and_utf16_offsets_count_surrogates() {
        let mut field = typed("A🙂Z");
        assert_eq!(field.len_utf16(), 4);
        field.replace_and_mark(Some(1..5), "é🙂", Some(1..3));
        assert_eq!(field.text(), "Aé🙂Z");
        assert_eq!(field.marked(), Some(1..7));
        field.replace_and_mark(None, "漢", None);
        assert_eq!(field.text(), "A漢Z");
        field.unmark();
        assert_eq!(field.marked(), None);
        field.set_selection_utf16(1..2);
        assert_eq!(field.selected_text(), Some("漢"));
        assert_eq!(field.range_to_utf16(&field.selection()), 1..2);
    }
}
