//! Colour roles, as `0xRRGGBB`, from the Flutter shell's dark theme
//! (`app/lib/src/theme/morph_theme.dart` on main). `weirdnet-themes` feeds
//! these later.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub background: u32,
    pub surface: u32,
    pub surface_variant: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub accent: u32,
    pub accent_alt: u32,
    pub border: u32,
    pub selection: u32,
    pub error: u32,
}

impl Theme {
    pub const fn dark() -> Self {
        Self {
            background: 0x0D0D12,
            surface: 0x16161D,
            surface_variant: 0x1E1E28,
            text_primary: 0xF5F6FA,
            text_secondary: 0xA8AAB8,
            accent: 0x00E5FF,
            accent_alt: 0xFF2BD6,
            border: 0x707284,
            selection: 0x0A434D,
            error: 0xFF4D6A,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dark_theme_uses_the_brand_cyan_on_charcoal() {
        let t = super::Theme::default();
        assert_eq!(t.background, 0x0D0D12);
        assert_eq!(t.accent, 0x00E5FF);
    }
}
