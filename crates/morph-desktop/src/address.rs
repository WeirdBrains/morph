//! The address bar's text element: draws the browser's `TextField` and
//! registers it as the window's input handler (typing and input methods),
//! after GPUI's `examples/input.rs`.

use gpui::{
    App, Bounds, ElementId, ElementInputHandler, Entity, GlobalElementId, LayoutId, PaintQuad, Pixels, ShapedLine, SharedString, Style, TextRun,
    UnderlineStyle, Window, fill, point, prelude::*, px, relative, rgb, rgba, size,
};

use crate::browser_view::BrowserView;

pub struct AddressElement {
    pub view: Entity<BrowserView>,
}

pub struct Prepainted {
    /// How far the text is scrolled left, so the cursor stays in view.
    scroll: Pixels,
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl Element for AddressElement {
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
        _id: Option<&GlobalElementId>,
        _inspector: Option<&gpui::InspectorElementId>,
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
        _id: Option<&GlobalElementId>,
        _inspector: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepainted {
        let view = self.view.read(cx);
        let field = view.browser().address();
        let focused = view.browser().address_focused();
        let content: SharedString = field.text().to_string().into();
        let selection = field.selection();
        let cursor = field.cursor();
        let marked = field.marked();
        let theme = view.theme();
        let style = window.text_style();
        let (text, color) = if content.is_empty() {
            (SharedString::from("Search or enter address"), rgb(theme.text_secondary).into())
        } else {
            (content, rgb(theme.text_primary).into())
        };
        let run = TextRun { len: text.len(), font: style.font(), color, background_color: None, underline: None, strikethrough: None };
        let runs = match marked.filter(|_| !field.text().is_empty()) {
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
        // A long address scrolls inside the bar (to the cursor while
        // editing, to its start otherwise) and is clipped at its edge, so
        // it never runs under the chips beside the bar.
        let room = (bounds.size.width - px(2.)).max(px(0.));
        let most = (line.width - room).max(px(0.));
        let scroll = if !focused || field.text().is_empty() {
            px(0.)
        } else {
            let x = line.x_for_index(cursor);
            let old = view.address_scroll.get();
            let s = if x < old {
                x
            } else if x > old + room {
                x - room
            } else {
                old
            };
            s.clamp(px(0.), most)
        };
        view.address_scroll.set(scroll);
        let bounds = Bounds::new(point(bounds.left() - scroll, bounds.top()), bounds.size);
        let (selection, cursor) = if !focused {
            (None, None)
        } else if selection.is_empty() || field.text().is_empty() {
            let x = if field.text().is_empty() { px(0.) } else { line.x_for_index(cursor) };
            (None, Some(fill(Bounds::new(point(bounds.left() + x, bounds.top()), size(px(2.), bounds.size.height)), rgb(theme.accent))))
        } else {
            let from = point(bounds.left() + line.x_for_index(selection.start), bounds.top());
            let to = point(bounds.left() + line.x_for_index(selection.end), bounds.bottom());
            (Some(fill(Bounds::from_corners(from, to), rgba((theme.accent << 8) | 0x55))), None)
        };
        Prepainted { scroll, line: Some(line), cursor, selection }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut (),
        prepainted: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.view.read(cx).address_focus().clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        let Some(line) = prepainted.line.take() else {
            return;
        };
        // Where the text starts (left of the bar when scrolled), for hit
        // tests; as wide again as the scroll so the bar's right part counts.
        let text_bounds =
            Bounds::new(point(bounds.left() - prepainted.scroll, bounds.top()), size(bounds.size.width + prepainted.scroll, bounds.size.height));
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(selection) = prepainted.selection.take() {
                window.paint_quad(selection);
            }
            let _ = line.paint(text_bounds.origin, window.line_height(), gpui::TextAlign::Left, None, window, cx);
            if let Some(cursor) = prepainted.cursor.take() {
                window.paint_quad(cursor);
            }
        });
        if crate::browser_view::log_layout() {
            let chip = self.view.read(cx).button_bounds.borrow().get("profile").map(|b| f32::from(b.left()));
            println!(
                "morph-event address text_w={:.0} bar_w={:.0} scroll={:.0} right={:.0} chip_left={:.0}",
                f32::from(line.width),
                f32::from(bounds.size.width),
                f32::from(prepainted.scroll),
                f32::from(bounds.right()),
                chip.unwrap_or(-1.)
            );
        }
        self.view.update(cx, |view, _| view.set_address_layout(line, text_bounds));
    }
}
