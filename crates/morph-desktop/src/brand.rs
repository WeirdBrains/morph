//! Morph's brand in the shell (docs/brand): the wordmark lockup and the
//! glitch rule, a thin line with cyan and magenta slices, as the Flutter
//! shell's `MorphGlitchRule` drew it.

use std::sync::{Arc, OnceLock};

use gpui::{Bounds, ImageFormat, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px, rgb, rgba};
use morph_ui::theme::Theme;

const WORDMARK: &[u8] = include_bytes!("../../../docs/brand/png/morph-wordmark.png");
const WORDMARK_LIGHT: &[u8] = include_bytes!("../../../docs/brand/png/morph-wordmark-light.png");

/// The lockup (rounded mark and "morph") for dark backgrounds, 896x256.
pub fn wordmark() -> Arc<gpui::Image> {
    static IMAGE: OnceLock<Arc<gpui::Image>> = OnceLock::new();
    IMAGE.get_or_init(|| Arc::new(gpui::Image::from_bytes(ImageFormat::Png, WORDMARK.to_vec()))).clone()
}

/// The lockup for light backgrounds, 896x256.
pub fn wordmark_light() -> Arc<gpui::Image> {
    static IMAGE: OnceLock<Arc<gpui::Image>> = OnceLock::new();
    IMAGE.get_or_init(|| Arc::new(gpui::Image::from_bytes(ImageFormat::Png, WORDMARK_LIGHT.to_vec()))).clone()
}

/// A dark background (the dark lockup), by its luminance.
pub fn is_dark(t: Theme) -> bool {
    let (r, g, b) = ((t.background >> 16) & 0xFF, (t.background >> 8) & 0xFF, t.background & 0xFF);
    r * 299 + g * 587 + b * 114 < 128_000
}

/// The start screen's loose glitch bars, as (dx, dy, width, height, cyan)
/// from the centre (the far-edge ticks are placed by [`start_bars`]), as the
/// phone app's `_StartGlitchPainter` draws them.
const START_BARS: [(f32, f32, f32, f32, bool); 6] = [
    (-420., -36., 72., 8., true),
    (-390., -20., 40., 5., false),
    (-430., 18., 56., 8., true),
    (340., -28., 80., 8., false),
    (360., -10., 36., 5., true),
    (348., 22., 64., 8., false),
];

/// Every start-screen bar in an area `w` x `h`: (x, y, width, height, cyan).
pub fn start_bars(w: f32, h: f32) -> Vec<(f32, f32, f32, f32, bool)> {
    let (cx, cy) = (w / 2., h / 2.);
    let mut bars: Vec<_> = START_BARS.iter().map(|&(dx, dy, bw, bh, c)| (cx + dx, cy + dy, bw, bh, c)).collect();
    bars.push((24., cy - 80., 48., 6., true));
    bars.push((w - 80., cy + 70., 52., 6., false));
    bars
}

/// What a tab with nothing in it shows (the phone app's start page): the
/// lockup, "A distributed web browser with local AI." and a dimmer "Type an
/// address.", centred, with cyan and magenta glitch bars around them.
pub fn start_screen(t: Theme) -> gpui::Div {
    let (cyan, magenta) = (t.accent << 8 | 0x8C, t.accent_alt << 8 | 0x8C);
    let bars = canvas(
        |_, _, _| {},
        move |bounds: Bounds<gpui::Pixels>, _, window, _| {
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            for (x, y, bw, bh, is_cyan) in start_bars(w, h) {
                let rect = Bounds::new(bounds.origin + point(px(x), px(y)), gpui::size(px(bw), px(bh)));
                window.paint_quad(gpui::fill(rect, rgba(if is_cyan { cyan } else { magenta })));
            }
        },
    )
    .absolute()
    .size_full();
    let lockup = if is_dark(t) { wordmark() } else { wordmark_light() };
    div().absolute().size_full().bg(rgb(t.background)).child(bars).child(
        div()
            .absolute()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(32.))
            .child(gpui::img(lockup).h(px(88.)).w(px(308.)))
            .child(div().h(px(28.)))
            .child(
                div().font_family("monospace").text_size(px(14.)).text_color(rgb(t.text_secondary)).child("A distributed web browser with local AI."),
            )
            .child(div().h(px(8.)))
            .child(div().font_family("monospace").text_size(px(13.)).text_color(rgba(t.text_secondary << 8 | 0xCC)).child("Type an address.")),
    )
}

/// The slices along a rule `width` wide, as (x0, x1, top, colour): cyan
/// and magenta pairs at 22% and 72% of the width.
pub fn slices(width: f32, t: Theme) -> [(f32, f32, bool, u32); 4] {
    let (mid, mid2) = (width * 0.22, width * 0.72);
    [
        (mid, mid + 28., true, t.accent),
        (mid + 18., mid + 52., false, t.accent_alt),
        (mid2, mid2 + 36., true, t.accent_alt),
        (mid2 - 16., mid2 + 12., false, t.accent),
    ]
}

/// The rule, 3 px tall and as wide as its parent.
pub fn glitch_rule(t: Theme) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<gpui::Pixels>, _, window, _| {
            let (x, y, w) = (bounds.origin.x, bounds.origin.y, f32::from(bounds.size.width));
            let line = Bounds::new(point(x, y + px(1.)), gpui::size(bounds.size.width, px(1.)));
            window.paint_quad(gpui::fill(line, gpui::rgba(0x2C2C3A99)));
            for (x0, x1, top, colour) in slices(w, t) {
                let mut path = PathBuilder::fill();
                let (top_y, bottom_y) = if top { (y, y + px(2.)) } else { (y + px(1.), y + px(3.)) };
                path.move_to(point(x + px(x0), top_y));
                path.line_to(point(x + px(x1), top_y));
                path.line_to(point(x + px(x1), bottom_y));
                path.line_to(point(x + px(x0), bottom_y));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(colour));
                }
            }
        },
    )
    .w_full()
    .h(px(3.))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wordmark_is_the_brand_png() {
        assert_eq!(&WORDMARK[1..4], b"PNG");
    }

    #[test]
    fn the_start_screen_follows_the_phone_app() {
        assert_eq!(&WORDMARK_LIGHT[1..4], b"PNG");
        assert!(is_dark(Theme::dark()));
        assert!(!is_dark(Theme::light()));
        let bars = start_bars(1000., 600.);
        assert_eq!(bars.len(), 8);
        assert_eq!(bars[0], (80., 264., 72., 8., true));
        assert_eq!(bars[7], (920., 370., 52., 6., false));
    }

    #[test]
    fn slices_sit_where_the_flutter_rule_put_them() {
        let s = slices(1000., Theme::dark());
        assert_eq!(s[0], (220., 248., true, 0x00E5FF));
        assert_eq!(s[1], (238., 272., false, 0xFF2BD6));
        assert_eq!(s[2], (720., 756., true, 0xFF2BD6));
        assert_eq!(s[3], (704., 732., false, 0x00E5FF));
    }
}
