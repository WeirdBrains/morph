//! Morph: the raw key behind the key event GPUI is dispatching.
//!
//! `gpui::Keystroke` names keys by what they type, so the keypad's `1` and
//! the top row's `1` (or the two Enter keys) look the same. An embedder that
//! forwards keys to another toolkit (Morph's off-screen web views on
//! Wayland) needs the hardware keycode; it reads it here while GPUI
//! dispatches the event.

use std::cell::Cell;

/// One key event as the keyboard sent it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawKey {
    /// The XKB keycode (the evdev code plus 8), as X11 and GDK number keys.
    pub keycode: u32,
    /// The keysym the key gives in the current state (an X keysym, which is
    /// also a GDK keyval).
    pub keysym: u32,
    /// The effective XKB modifier mask: Shift 1, Lock 2, Control 4, Mod1
    /// (Alt) 8, Mod2 (Num Lock) 16, Mod4 (Super) 64.
    pub mods: u32,
    /// The effective layout (group).
    pub group: u32,
}

thread_local! {
    static CURRENT: Cell<Option<RawKey>> = const { Cell::new(None) };
}

/// The raw key behind the key event GPUI is dispatching right now; `None`
/// outside a dispatch and for keys an input method made up.
pub fn current() -> Option<RawKey> {
    CURRENT.with(|c| c.get())
}

/// Run `f` (which dispatches a key event) with `key` as the current key.
#[cfg_attr(not(feature = "wayland"), allow(dead_code))]
pub(crate) fn dispatching<R>(key: RawKey, f: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|c| c.replace(Some(key)));
    let out = f();
    CURRENT.with(|c| c.set(previous));
    out
}
