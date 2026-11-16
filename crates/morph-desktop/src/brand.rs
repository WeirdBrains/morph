//! Morph's brand in the shell (docs/brand): the wordmark lockup and the
//! glitch rule, a thin line with cyan and magenta slices, as the Flutter
//! shell's `MorphGlitchRule` drew it.

use std::sync::{Arc, OnceLock};

use gpui::{Bounds, ImageFormat, IntoElement, PathBuilder, Styled, canvas, point, px, rgb};
use morph_ui::theme::Theme;

const WORDMARK: &[u8] = include_bytes!("../../../docs/brand/png/morph-wordmark.png");

/// The lockup (rounded mark and "morph") for dark backgrounds, 896x256.
pub fn wordmark() -> Arc<gpui::Image> {
    static IMAGE: OnceLock<Arc<gpui::Image>> = OnceLock::new();
    IMAGE.get_or_init(|| Arc::new(gpui::Image::from_bytes(ImageFormat::Png, WORDMARK.to_vec()))).clone()
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
    fn slices_sit_where_the_flutter_rule_put_them() {
        let s = slices(1000., Theme::dark());
        assert_eq!(s[0], (220., 248., true, 0x00E5FF));
        assert_eq!(s[1], (238., 272., false, 0xFF2BD6));
        assert_eq!(s[2], (720., 756., true, 0xFF2BD6));
        assert_eq!(s[3], (704., 732., false, 0x00E5FF));
    }
}
