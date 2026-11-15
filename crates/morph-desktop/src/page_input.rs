//! GPUI input as input for pages the shell draws (`morph_webview`'s
//! off-screen backend on Wayland): keys with the hardware keycode GPUI's
//! Wayland platform reports (`gpui_linux::raw_key`, patched in
//! third_party/), pointer buttons, wheel steps and modifier keys.

use gpui::{Keystroke, Modifiers, MouseButton, NavigationDirection, ScrollDelta};

/// The key the keyboard sent behind a GPUI key event (Linux: GPUI's
/// platform patched in third_party/). Other platforms have none; the
/// shell draws pages only on Wayland.
#[cfg(target_os = "linux")]
pub use gpui_linux::raw_key::RawKey;

#[cfg(not(target_os = "linux"))]
#[derive(Debug, Clone, Copy)]
pub struct RawKey {
    pub keycode: u32,
    pub keysym: u32,
    pub mods: u32,
    pub group: u32,
}

/// The raw key behind the key event being handled, if the platform says.
pub fn current_raw_key() -> Option<RawKey> {
    #[cfg(target_os = "linux")]
    return gpui_linux::raw_key::current();
    #[cfg(not(target_os = "linux"))]
    None
}
use morph_webview::keys;
use morph_webview::{Input, Key};

/// The page-side modifiers for GPUI's.
pub fn modifiers(m: &Modifiers) -> morph_webview::Modifiers {
    morph_webview::Modifiers { shift: m.shift, control: m.control, alt: m.alt, super_key: m.platform }
}

pub fn button(button: MouseButton) -> morph_webview::MouseButton {
    match button {
        MouseButton::Left => morph_webview::MouseButton::Left,
        MouseButton::Right => morph_webview::MouseButton::Right,
        MouseButton::Middle => morph_webview::MouseButton::Middle,
        MouseButton::Navigate(NavigationDirection::Back) => morph_webview::MouseButton::Back,
        MouseButton::Navigate(NavigationDirection::Forward) => morph_webview::MouseButton::Forward,
    }
}

/// A key event for the page. With the raw key (from the keyboard) it is
/// exactly what the keyboard sent; without one (an input method's text) a
/// single character goes as its keysym and longer text as text.
pub fn key(keystroke: &Keystroke, pressed: bool, raw: Option<RawKey>) -> Option<Input> {
    if let Some(raw) = raw {
        return Some(Input::Key(Key { pressed, keycode: raw.keycode, keyval: raw.keysym, state: raw.mods, group: raw.group, is_modifier: false }));
    }
    if pressed
        && let Some(text) = keystroke.key_char.as_deref()
        && text.chars().count() > 1
    {
        return Some(Input::Text(text.to_string()));
    }
    let keyval = keys::keysym(&keystroke.key, keystroke.modifiers.shift)?;
    Some(Input::Key(Key { pressed, keycode: 0, keyval, state: keys::state(modifiers(&keystroke.modifiers)), group: 0, is_modifier: false }))
}

/// Presses and releases of modifier keys between two modifier states (GPUI
/// reports modifier keys as a change of state, not as keys).
pub fn modifier_keys(before: &Modifiers, after: &Modifiers) -> Vec<Input> {
    let held = |m: &Modifiers| [m.shift, m.control, m.alt, m.platform];
    let (was, now) = (held(before), held(after));
    let mut state = keys::state(modifiers(before));
    let mut out = Vec::new();
    for (i, &(keyval, keycode, mask)) in keys::MODIFIER_KEYS.iter().enumerate() {
        if was[i] == now[i] {
            continue;
        }
        out.push(Input::Key(Key { pressed: now[i], keycode, keyval, state, group: 0, is_modifier: true }));
        state = if now[i] { state | mask } else { state & !mask };
    }
    out
}

/// A wheel or touchpad scroll as page input: GPUI's deltas are positive
/// for scrolling up, three lines a wheel step, and three times the
/// compositor's pixels.
pub fn scroll(delta: &ScrollDelta) -> (f64, f64, bool) {
    match delta {
        ScrollDelta::Lines(p) => (-f64::from(p.x) / 3.0, -f64::from(p.y) / 3.0, false),
        ScrollDelta::Pixels(p) => (-f64::from(f32::from(p.x)) / 3.0, -f64::from(f32::from(p.y)) / 3.0, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    fn stroke(key: &str, key_char: Option<&str>) -> Keystroke {
        Keystroke { modifiers: Modifiers::default(), key: key.into(), key_char: key_char.map(Into::into) }
    }

    #[test]
    fn a_raw_key_goes_as_the_keyboard_sent_it() {
        let raw = RawKey { keycode: 87, keysym: 0xffb1, mods: 16, group: 0 };
        let Some(Input::Key(k)) = key(&stroke("1", Some("1")), true, Some(raw)) else { panic!() };
        assert_eq!((k.keycode, k.keyval, k.state, k.pressed), (87, 0xffb1, 16, true));
    }

    #[test]
    fn without_a_raw_key_the_name_gives_the_keysym() {
        let Some(Input::Key(k)) = key(&stroke("pagedown", None), true, None) else { panic!() };
        assert_eq!((k.keycode, k.keyval), (0, 0xff56));
        assert_eq!(key(&stroke("あ", Some("あい")), true, None), Some(Input::Text("あい".into())));
    }

    #[test]
    fn modifier_changes_become_modifier_keys() {
        let none = Modifiers::default();
        let ctrl = Modifiers { control: true, ..Default::default() };
        let keys = modifier_keys(&none, &ctrl);
        assert_eq!(keys.len(), 1);
        let Input::Key(k) = &keys[0] else { panic!() };
        assert!(k.pressed && k.is_modifier && k.keycode == 37 && k.state == 0);
        let Input::Key(k) = &modifier_keys(&ctrl, &none)[0] else { panic!() };
        assert!(!k.pressed && k.state == keys::CONTROL);
    }

    #[test]
    fn scrolling_down_is_positive() {
        assert_eq!(scroll(&ScrollDelta::Lines(point(0.0, -3.0))), (0.0, 1.0, false));
        assert_eq!(scroll(&ScrollDelta::Pixels(point(px(0.0), px(-30.0)))), (0.0, 10.0, true));
    }
}
