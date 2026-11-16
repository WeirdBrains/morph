//! A `<select>`'s open list as the shell draws it (pages the shell draws):
//! which row is highlighted, and what keys do. Up and Down move over the
//! rows that can be chosen (the element shows the row as it moves, as
//! GTK's list did), Page Up and Page Down move a page, Home and End go to
//! the ends, Enter chooses, Escape cancels, and typing jumps to a row
//! starting with the letters typed (within a second of each other; the
//! same letter again goes to the next such row).

use std::time::{Duration, Instant};

use morph_webview::{MenuChoice, OptionMenu};

/// Rows a Page Up or Page Down moves.
pub const PAGE: usize = 10;
/// Letters typed further apart than this start a new search.
pub const TYPE_AHEAD: Duration = Duration::from_millis(1000);

/// A key for an open list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Char(char),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuState {
    pub menu: OptionMenu,
    /// The highlighted row, an index into `menu.items`.
    pub highlighted: Option<usize>,
    typed: String,
    typed_at: Option<Instant>,
}

impl MenuState {
    pub fn new(menu: OptionMenu) -> Self {
        let mut state = Self { menu, highlighted: None, typed: String::new(), typed_at: None };
        state.highlighted = state.menu.items.iter().position(|i| i.selected).filter(|&i| state.choosable(i)).or_else(|| state.first());
        state
    }

    /// Whether row `i` can be chosen.
    pub fn choosable(&self, i: usize) -> bool {
        self.menu.items.get(i).is_some_and(|item| item.enabled && !item.group_label)
    }

    fn first(&self) -> Option<usize> {
        (0..self.menu.items.len()).find(|&i| self.choosable(i))
    }

    fn last(&self) -> Option<usize> {
        (0..self.menu.items.len()).rev().find(|&i| self.choosable(i))
    }

    /// The choosable row `steps` rows down (or up) from the highlighted
    /// one, stopping at the ends.
    fn moved(&self, down: bool, steps: usize) -> Option<usize> {
        let Some(mut at) = self.highlighted else {
            return if down { self.first() } else { self.last() };
        };
        let mut left = steps;
        let mut i = at;
        while left > 0 {
            let next = if down { i.checked_add(1).filter(|&n| n < self.menu.items.len()) } else { i.checked_sub(1) };
            let Some(n) = next else { break };
            i = n;
            if self.choosable(i) {
                at = i;
                left -= 1;
            }
        }
        Some(at)
    }

    fn type_ahead(&mut self, c: char, now: Instant) -> Option<usize> {
        if self.typed_at.is_none_or(|t| now.duration_since(t) > TYPE_AHEAD) {
            self.typed.clear();
        }
        self.typed_at = Some(now);
        self.typed.extend(c.to_lowercase());
        let n = self.menu.items.len();
        let start = self.highlighted.unwrap_or(n.saturating_sub(1));
        // One letter repeated cycles through the rows starting with it.
        let repeat = self.typed.chars().count() > 1 && self.typed.chars().all(|t| Some(t) == self.typed.chars().next());
        let prefix = if repeat { self.typed.chars().take(1).collect::<String>() } else { self.typed.clone() };
        let from = if self.typed.chars().count() == 1 || repeat { 1 } else { 0 };
        (0..n).map(|k| (start + from + k) % n.max(1)).find(|&i| self.choosable(i) && self.menu.items[i].label.trim_start().to_lowercase().starts_with(&prefix))
    }

    /// Highlight row `i` (the pointer is over it); `None` for a row that
    /// cannot be chosen.
    pub fn hover(&mut self, i: usize) -> Option<MenuChoice> {
        if self.choosable(i) && self.highlighted != Some(i) {
            self.highlighted = Some(i);
            return Some(MenuChoice::Highlight(i));
        }
        None
    }

    /// The answer to a click on row `i`, if it can be chosen.
    pub fn click(&self, i: usize) -> Option<MenuChoice> {
        self.choosable(i).then_some(MenuChoice::Activate(i))
    }

    /// What `key` does: the page's answer, if any.
    pub fn key(&mut self, key: MenuKey, now: Instant) -> Option<MenuChoice> {
        let to = match key {
            MenuKey::Enter => return Some(self.highlighted.map_or(MenuChoice::Cancel, MenuChoice::Activate)),
            MenuKey::Escape => return Some(MenuChoice::Cancel),
            MenuKey::Up => self.moved(false, 1),
            MenuKey::Down => self.moved(true, 1),
            MenuKey::PageUp => self.moved(false, PAGE),
            MenuKey::PageDown => self.moved(true, PAGE),
            MenuKey::Home => self.first(),
            MenuKey::End => self.last(),
            MenuKey::Char(c) => self.type_ahead(c, now),
        }?;
        if self.highlighted == Some(to) {
            return None;
        }
        self.highlighted = Some(to);
        Some(MenuChoice::Highlight(to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_webview::{MenuItem, Rect};

    fn item(label: &str) -> MenuItem {
        MenuItem { label: label.into(), enabled: true, ..Default::default() }
    }

    /// Apple, Banana (selected), Cherry, Date (disabled), [Later], Elderberry, Fig
    fn fruit() -> MenuState {
        let mut items = vec![item("Apple"), item("Banana"), item("Cherry"), item("Date"), item("Later"), item("Elderberry"), item("Fig")];
        items[1].selected = true;
        items[3].enabled = false;
        items[4].group_label = true;
        items[5].group_child = true;
        items[6].group_child = true;
        MenuState::new(OptionMenu { items, anchor: Rect::new(10.0, 10.0, 100.0, 20.0) })
    }

    #[test]
    fn starts_on_the_selected_row() {
        assert_eq!(fruit().highlighted, Some(1));
        let mut m = fruit();
        m.menu.items[1].selected = false;
        assert_eq!(MenuState::new(m.menu).highlighted, Some(0));
    }

    #[test]
    fn arrows_skip_disabled_rows_and_group_labels_and_stop_at_the_ends() {
        let mut m = fruit();
        let now = Instant::now();
        assert_eq!(m.key(MenuKey::Down, now), Some(MenuChoice::Highlight(2)));
        assert_eq!(m.key(MenuKey::Down, now), Some(MenuChoice::Highlight(5)), "Date is disabled, Later a label");
        assert_eq!(m.key(MenuKey::Down, now), Some(MenuChoice::Highlight(6)));
        assert_eq!(m.key(MenuKey::Down, now), None, "the last row stays");
        assert_eq!(m.key(MenuKey::Up, now), Some(MenuChoice::Highlight(5)));
        assert_eq!(m.key(MenuKey::Up, now), Some(MenuChoice::Highlight(2)));
        assert_eq!(m.key(MenuKey::Home, now), Some(MenuChoice::Highlight(0)));
        assert_eq!(m.key(MenuKey::Up, now), None);
        assert_eq!(m.key(MenuKey::End, now), Some(MenuChoice::Highlight(6)));
    }

    #[test]
    fn page_keys_move_a_page_of_choosable_rows() {
        let items = (1..=60).map(|i| item(&format!("Item {i}"))).collect();
        let mut m = MenuState::new(OptionMenu { items, anchor: Rect::default() });
        let now = Instant::now();
        assert_eq!(m.key(MenuKey::PageDown, now), Some(MenuChoice::Highlight(10)));
        assert_eq!(m.key(MenuKey::PageDown, now), Some(MenuChoice::Highlight(20)));
        assert_eq!(m.key(MenuKey::PageUp, now), Some(MenuChoice::Highlight(10)));
        m.highlighted = Some(55);
        assert_eq!(m.key(MenuKey::PageDown, now), Some(MenuChoice::Highlight(59)));
    }

    #[test]
    fn enter_chooses_and_escape_cancels() {
        let mut m = fruit();
        let now = Instant::now();
        m.key(MenuKey::Down, now);
        assert_eq!(m.key(MenuKey::Enter, now), Some(MenuChoice::Activate(2)));
        assert_eq!(m.key(MenuKey::Escape, now), Some(MenuChoice::Cancel));
        let mut empty = MenuState::new(OptionMenu::default());
        assert_eq!(empty.key(MenuKey::Enter, now), Some(MenuChoice::Cancel));
    }

    #[test]
    fn typing_jumps_to_a_matching_row() {
        let mut m = fruit();
        let t = Instant::now();
        assert_eq!(m.key(MenuKey::Char('f'), t), Some(MenuChoice::Highlight(6)));
        assert_eq!(m.key(MenuKey::Char('E'), t + TYPE_AHEAD * 2), Some(MenuChoice::Highlight(5)), "a pause starts a new search");
        assert_eq!(m.key(MenuKey::Char('d'), t + TYPE_AHEAD * 4), None, "disabled rows are never found");
        let items = vec![item("Car"), item("Cat"), item("Cow"), item("Dog")];
        let mut m = MenuState::new(OptionMenu { items, anchor: Rect::default() });
        assert_eq!(m.highlighted, Some(0));
        assert_eq!(m.key(MenuKey::Char('c'), t), Some(MenuChoice::Highlight(1)), "one letter goes to the next row with it");
        assert_eq!(m.key(MenuKey::Char('c'), t), Some(MenuChoice::Highlight(2)));
        assert_eq!(m.key(MenuKey::Char('c'), t), Some(MenuChoice::Highlight(0)), "and wraps");
        let t2 = t + TYPE_AHEAD * 2;
        m.key(MenuKey::Char('c'), t2);
        assert_eq!(m.highlighted, Some(1));
        assert_eq!(m.key(MenuKey::Char('o'), t2), Some(MenuChoice::Highlight(2)), "\"co\" finds Cow");
    }

    #[test]
    fn the_pointer_highlights_and_clicks_choose_only_choosable_rows() {
        let mut m = fruit();
        assert_eq!(m.hover(3), None);
        assert_eq!(m.hover(4), None);
        assert_eq!(m.hover(5), Some(MenuChoice::Highlight(5)));
        assert_eq!(m.hover(5), None);
        assert_eq!(m.click(4), None);
        assert_eq!(m.click(0), Some(MenuChoice::Activate(0)));
    }
}
