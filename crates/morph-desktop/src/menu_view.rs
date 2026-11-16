//! A list of rows the shell draws as a menu: a `<select>`'s options (pages
//! the shell draws), the main menu (☰) and the tab strip's context menu.
//! Rows highlight under the pointer and with the keys the owner handles;
//! long lists scroll, keeping the highlighted row in view.

use std::rc::Rc;

use gpui::{App, Div, MouseButton, ScrollHandle, SharedString, Stateful, Window, div, prelude::*, px, rgb};
use morph_ui::theme::Theme;

pub const ROW_HEIGHT: f32 = 26.;
/// Space above and below the rows inside the menu's border.
pub const PADDING: f32 = 4.;

/// One row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub label: SharedString,
    /// Shown on the right, dimmed (a shortcut).
    pub detail: SharedString,
    pub enabled: bool,
    /// A heading (`<optgroup>` label): never highlighted or chosen.
    pub header: bool,
    pub indent: bool,
    /// A thin line above the row.
    pub separator: bool,
    /// The element's current value (a check mark).
    pub checked: bool,
}

impl Row {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into(), enabled: true, ..Default::default() }
    }
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = detail.into();
        self
    }
    pub fn separated(mut self) -> Self {
        self.separator = true;
        self
    }
}

pub type RowHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// The height `rows` take, without a cap.
pub fn natural_height(rows: &[Row]) -> f32 {
    rows.iter().map(|r| ROW_HEIGHT + if r.separator { 9. } else { 0. }).sum::<f32>() + 2. * PADDING + 2.
}

/// The menu box: `width` wide, at most `max_height` tall (scrolling past
/// that). `on_hover` and `on_click` get the row index.
#[allow(clippy::too_many_arguments)]
pub fn menu(
    t: Theme,
    id: &'static str,
    rows: &[Row],
    highlighted: Option<usize>,
    width: f32,
    max_height: f32,
    scroll: &ScrollHandle,
    on_hover: RowHandler,
    on_click: RowHandler,
) -> Stateful<Div> {
    let list = div().id(SharedString::from(format!("{id}-rows"))).flex().flex_col().overflow_y_scroll().track_scroll(scroll).max_h(px(max_height - 2.)).py(px(PADDING));
    let list = list.children(rows.iter().enumerate().map(|(i, row)| {
        let lit = highlighted == Some(i) && row.enabled && !row.header;
        let (hover, click) = (on_hover.clone(), on_click.clone());
        let choosable = row.enabled && !row.header;
        let text = if row.header || !row.enabled { t.text_secondary } else { t.text_primary };
        let line = div()
            .id(SharedString::from(format!("{id}-row-{i}")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .h(px(ROW_HEIGHT))
            .px(px(10.))
            .pl(px(if row.indent { 26. } else { 10. }))
            .text_size(px(13.))
            .text_color(rgb(text))
            .when(lit, |d| d.bg(rgb(t.selection)))
            .when(row.header, |d| d.text_size(px(11.)))
            .when(choosable, |d| d.cursor_pointer())
            .on_mouse_move(move |_, window, cx| hover(i, window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| click(i, window, cx))
            .child(div().w(px(10.)).text_color(rgb(t.accent)).child(if row.checked { "✓" } else { "" }))
            .child(div().flex_1().overflow_hidden().whitespace_nowrap().child(row.label.clone()))
            .when(!row.detail.is_empty(), |d| d.child(div().text_size(px(12.)).text_color(rgb(t.text_secondary)).child(row.detail.clone())));
        div()
            .flex()
            .flex_col()
            .when(row.separator, |d| d.child(div().h(px(1.)).my(px(4.)).bg(rgb(t.surface_variant))))
            .child(line)
    }));
    div()
        .id(id)
        .occlude()
        .w(px(width))
        .bg(rgb(t.surface))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(6.))
        .shadow_lg()
        .overflow_hidden()
        .child(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_count_rows_separators_and_padding() {
        let rows = vec![Row::new("a"), Row::new("b").separated()];
        assert_eq!(natural_height(&rows), 2. * ROW_HEIGHT + 9. + 2. * PADDING + 2.);
    }
}

/// The list key a keystroke is, if any.
pub fn menu_key(keystroke: &gpui::Keystroke) -> Option<morph_browser::MenuKey> {
    use morph_browser::MenuKey;
    let m = &keystroke.modifiers;
    if m.control || m.alt || m.platform {
        return None;
    }
    Some(match keystroke.key.as_str() {
        "up" => MenuKey::Up,
        "down" => MenuKey::Down,
        "pageup" => MenuKey::PageUp,
        "pagedown" => MenuKey::PageDown,
        "home" => MenuKey::Home,
        "end" => MenuKey::End,
        "enter" | "space" => MenuKey::Enter,
        "escape" => MenuKey::Escape,
        "tab" => MenuKey::Escape,
        _ => {
            let text = keystroke.key_char.as_deref().unwrap_or(&keystroke.key);
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if !c.is_control() => MenuKey::Char(c),
                _ => return None,
            }
        }
    })
}

/// A keystroke in GPUI notation (`ctrl-shift-o`) as a menu shows it:
/// `Ctrl+Shift+O`, or on macOS `⌘⇧O`.
pub fn shortcut_label(keystroke: &str, mac: bool) -> String {
    let parts: Vec<&str> = keystroke.split('-').collect();
    let (mods, key) = parts.split_at(parts.len().saturating_sub(1));
    let key = match key.first().copied().unwrap_or("") {
        "," => ",".to_string(),
        "left" => if mac { "←".into() } else { "Left".into() },
        "right" => if mac { "→".into() } else { "Right".into() },
        "tab" => "Tab".into(),
        "escape" => "Esc".into(),
        k if k.len() == 1 => k.to_uppercase(),
        k => {
            let mut c = k.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    };
    if mac {
        let mut out = String::new();
        for m in mods {
            out.push_str(match *m {
                "ctrl" => "⌃",
                "alt" => "⌥",
                "shift" => "⇧",
                "cmd" | "super" => "⌘",
                _ => "",
            });
        }
        out + &key
    } else {
        let mut out: Vec<String> = mods
            .iter()
            .map(|m| match *m {
                "ctrl" => "Ctrl".to_string(),
                "alt" => "Alt".to_string(),
                "shift" => "Shift".to_string(),
                "cmd" | "super" => "Super".to_string(),
                other => other.to_string(),
            })
            .collect();
        out.push(key);
        out.join("+")
    }
}

/// Where a list for `anchor` (an element, in window coordinates) opens in
/// a page area: below it if it fits or there is more room there, else
/// above. Returns the top-left corner, the width and the height cap.
pub fn place(anchor: (f32, f32, f32, f32), area: (f32, f32, f32, f32), natural: f32) -> (f32, f32, f32, f32) {
    let (ax, ay, aw, ah) = anchor;
    let (x0, y0, w0, h0) = area;
    let width = aw.max(120.).min(w0);
    let below = (y0 + h0) - (ay + ah);
    let above = ay - y0;
    let x = ax.min(x0 + w0 - width).max(x0);
    if natural <= below || below >= above {
        (x, ay + ah, width, natural.min(below.max(ROW_HEIGHT * 3.)))
    } else {
        let height = natural.min(above);
        (x, ay - height, width, height)
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use morph_browser::MenuKey;

    #[test]
    fn shortcut_labels_read_like_menus_do() {
        assert_eq!(shortcut_label("ctrl-shift-o", false), "Ctrl+Shift+O");
        assert_eq!(shortcut_label("ctrl-,", false), "Ctrl+,");
        assert_eq!(shortcut_label("cmd-shift-o", true), "⌘⇧O");
        assert_eq!(shortcut_label("cmd-t", true), "⌘T");
        assert_eq!(shortcut_label("f3", false), "F3");
        assert_eq!(shortcut_label("alt-left", false), "Alt+Left");
    }

    #[test]
    fn lists_open_below_unless_there_is_more_room_above() {
        let area = (0., 100., 800., 600.);
        let (x, y, w, h) = place((40., 140., 160., 30.), area, 200.);
        assert_eq!((x, y, w, h), (40., 170., 160., 200.));
        let (_, y, _, h) = place((40., 640., 160., 30.), area, 300.);
        assert_eq!((y, h), (340., 300.), "near the bottom it opens upwards");
        let (x, _, w, _) = place((760., 140., 60., 30.), area, 100.);
        assert_eq!((x, w), (680., 120.), "kept inside the page, at least 120 wide");
        let (_, _, _, h) = place((40., 140., 160., 30.), area, 5000.);
        assert_eq!(h, 530., "long lists scroll");
    }

    #[test]
    fn keys_map_to_list_keys() {
        let k = |s: &str| gpui::Keystroke::parse(s).unwrap();
        assert_eq!(menu_key(&k("down")), Some(MenuKey::Down));
        assert_eq!(menu_key(&k("pageup")), Some(MenuKey::PageUp));
        assert_eq!(menu_key(&k("enter")), Some(MenuKey::Enter));
        assert_eq!(menu_key(&k("escape")), Some(MenuKey::Escape));
        assert_eq!(menu_key(&k("b")), Some(MenuKey::Char('b')));
        assert_eq!(menu_key(&k("ctrl-t")), None, "the shell's shortcuts stay the shell's");
    }
}
