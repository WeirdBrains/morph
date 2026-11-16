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

impl Theme {
    /// A light counterpart with the same roles.
    pub const fn light() -> Self {
        Self {
            background: 0xF7F7FA,
            surface: 0xFFFFFF,
            surface_variant: 0xE6E7EE,
            text_primary: 0x15161C,
            text_secondary: 0x55576A,
            accent: 0x0077A8,
            accent_alt: 0xC2189B,
            border: 0x8C8EA0,
            selection: 0xB8E6F5,
            error: 0xC62842,
        }
    }

    /// The theme a profile's setting names.
    pub fn for_choice(choice: morph_core::settings::ThemeChoice) -> Self {
        match choice {
            morph_core::settings::ThemeChoice::Dark => Self::dark(),
            morph_core::settings::ThemeChoice::Light => Self::light(),
        }
    }

    /// Whether this is a dark theme (pages get dark form controls).
    pub fn is_dark(&self) -> bool {
        let r = (self.background >> 16) & 0xFF;
        let g = (self.background >> 8) & 0xFF;
        let b = self.background & 0xFF;
        r + g + b < 0x180
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

    #[test]
    fn both_themes_keep_text_readable_on_their_background() {
        use super::Theme;
        assert!(Theme::dark().is_dark());
        assert!(!Theme::light().is_dark());
        assert_eq!(Theme::for_choice(morph_core::settings::ThemeChoice::Light), Theme::light());
        assert_eq!(Theme::for_choice(morph_core::settings::ThemeChoice::Dark), Theme::default());
    }
}
