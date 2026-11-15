//! Keys for pages the shell draws ([`crate::Backend::draws_pages`]): GPUI's
//! key names as X keysyms (GDK keyvals) and modifier masks. Kept free of
//! GTK so it is tested everywhere.

use crate::Modifiers;

/// X11/GDK modifier mask bits.
pub const SHIFT: u32 = 1;
pub const LOCK: u32 = 1 << 1;
pub const CONTROL: u32 = 1 << 2;
pub const MOD1: u32 = 1 << 3;
pub const MOD2: u32 = 1 << 4;
pub const MOD4: u32 = 1 << 6;
/// GDK's pointer button masks (`GDK_BUTTON1_MASK` and on).
pub const BUTTON1: u32 = 1 << 8;

/// The X11/GDK modifier mask for `modifiers`.
pub fn state(modifiers: Modifiers) -> u32 {
    let mut state = 0;
    if modifiers.shift {
        state |= SHIFT;
    }
    if modifiers.control {
        state |= CONTROL;
    }
    if modifiers.alt {
        state |= MOD1;
    }
    if modifiers.super_key {
        state |= MOD4;
    }
    state
}

/// The keysym for a character: Latin-1 keysyms are the code point, others
/// are the code point plus `0x0100_0000`.
pub fn keysym_for_char(c: char) -> u32 {
    let cp = c as u32;
    if (0x20..=0x7e).contains(&cp) || (0xa0..=0xff).contains(&cp) { cp } else { 0x0100_0000 + cp }
}

/// The keysym for a GPUI key name (`Keystroke::key`, such as `pageup`, `a`
/// or `f5`), with Shift held or not. Used when the raw key is not known
/// (an input method, or GPUI's X11 platform).
pub fn keysym(key: &str, shift: bool) -> Option<u32> {
    let named = match key {
        "enter" => 0xff0d,
        "escape" => 0xff1b,
        "backspace" => 0xff08,
        "tab" if shift => 0xfe20, // ISO_Left_Tab, as X11 reports Shift+Tab
        "tab" => 0xff09,
        "space" => 0x20,
        "delete" => 0xffff,
        "insert" => 0xff63,
        "home" => 0xff50,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "pageup" => 0xff55,
        "pagedown" => 0xff56,
        "end" => 0xff57,
        "menu" => 0xff67,
        "print" => 0xff61,
        "pause" => 0xff13,
        "scroll_lock" => 0xff14,
        "num_lock" => 0xff7f,
        "back" => 0x1008ff26,
        "forward" => 0x1008ff27,
        // The keypad keys whose names say so (GPUI drops the `kp_`).
        "add" => 0xffab,
        "subtract" => 0xffad,
        "multiply" => 0xffaa,
        "divide" => 0xffaf,
        "decimal" => 0xffae,
        "separator" => 0xffac,
        "begin" => 0xff9d,
        _ => 0,
    };
    if named != 0 {
        return Some(named);
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
        && (1..=35).contains(&n)
    {
        return Some(0xffbe + n - 1);
    }
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let c = if shift && c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c };
    Some(keysym_for_char(c))
}

/// Modifier keys as GPUI reports them (a change of held modifiers, no key):
/// keysym and keycode (the left-hand key on a PC keyboard) and its mask.
pub const MODIFIER_KEYS: [(u32, u32, u32); 4] = [
    (0xffe1, 50, SHIFT),   // Shift_L
    (0xffe3, 37, CONTROL), // Control_L
    (0xffe9, 64, MOD1),    // Alt_L
    (0xffeb, 133, MOD4),   // Super_L
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_have_their_keysyms() {
        assert_eq!(keysym("pagedown", false), Some(0xff56));
        assert_eq!(keysym("enter", false), Some(0xff0d));
        assert_eq!(keysym("tab", true), Some(0xfe20));
        assert_eq!(keysym("f5", false), Some(0xffc2));
        assert_eq!(keysym("f12", false), Some(0xffc9));
        assert_eq!(keysym("add", false), Some(0xffab));
    }

    #[test]
    fn characters_are_their_keysyms() {
        assert_eq!(keysym("a", false), Some(0x61));
        assert_eq!(keysym("a", true), Some(0x41));
        assert_eq!(keysym("1", true), Some(0x31));
        assert_eq!(keysym(",", false), Some(0x2c));
        assert_eq!(keysym("é", false), Some(0xe9));
        assert_eq!(keysym("ж", false), Some(0x0100_0436));
        assert_eq!(keysym("nonsense", false), None);
    }

    #[test]
    fn modifiers_make_the_x11_mask() {
        assert_eq!(state(Modifiers { shift: true, control: true, ..Default::default() }), SHIFT | CONTROL);
        assert_eq!(state(Modifiers { alt: true, super_key: true, ..Default::default() }), MOD1 | MOD4);
    }
}
