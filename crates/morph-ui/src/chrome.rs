//! Where the shell's chrome goes, decided without drawing: the tab strip's
//! place around the page (from the profile's settings), the keys the
//! settings panel answers to, and which window buttons a frameless window
//! draws.

use morph_core::settings::{MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH, Settings, TabPosition, clamp_sidebar_width};

/// Width of a collapsed left or right tab list, in logical pixels.
pub const COLLAPSED_TAB_LIST_WIDTH: u32 = 48;

/// How much one press of the settings panel's − or + changes the tab list.
pub const TAB_LIST_WIDTH_STEP: u32 = 20;

/// Where the tabs are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabStrip {
    /// A row above the toolbar.
    Top,
    /// A row below the page.
    Bottom,
    /// A vertical list beside the toolbar and page, `width` wide;
    /// `collapsed` lists show one letter per tab.
    Side { left: bool, width: u32, collapsed: bool },
}

impl TabStrip {
    pub fn from_settings(settings: &Settings) -> Self {
        let side = |left| {
            let collapsed = settings.sidebar_collapsed;
            let width = if collapsed { COLLAPSED_TAB_LIST_WIDTH } else { clamp_sidebar_width(settings.sidebar_width) };
            TabStrip::Side { left, width, collapsed }
        };
        match settings.tab_position {
            TabPosition::Top => TabStrip::Top,
            TabPosition::Bottom => TabStrip::Bottom,
            TabPosition::Left => side(true),
            TabPosition::Right => side(false),
        }
    }

    /// Whether the row of tabs is the window's top edge (and so its caption).
    pub fn is_caption(self) -> bool {
        self == TabStrip::Top
    }
}

/// A change the settings panel makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsChange {
    Position(TabPosition),
    ToggleCollapsed,
    Wider,
    Narrower,
}

impl SettingsChange {
    /// The settings panel's keys: arrows move the tabs to that edge, `c`
    /// collapses the side list, `-` and `=` (or `+`) resize it.
    pub fn for_key(key: &str) -> Option<Self> {
        Some(match key {
            "up" => Self::Position(TabPosition::Top),
            "down" => Self::Position(TabPosition::Bottom),
            "left" => Self::Position(TabPosition::Left),
            "right" => Self::Position(TabPosition::Right),
            "c" => Self::ToggleCollapsed,
            "=" | "+" => Self::Wider,
            "-" => Self::Narrower,
            _ => return None,
        })
    }

    /// `settings` with the change made; widths stay in range.
    pub fn apply(self, settings: &Settings) -> Settings {
        let mut next = settings.clone();
        match self {
            Self::Position(position) => next.tab_position = position,
            Self::ToggleCollapsed => next.sidebar_collapsed = !next.sidebar_collapsed,
            Self::Wider => next.sidebar_width = (next.sidebar_width + TAB_LIST_WIDTH_STEP).min(MAX_SIDEBAR_WIDTH),
            Self::Narrower => next.sidebar_width = next.sidebar_width.saturating_sub(TAB_LIST_WIDTH_STEP).max(MIN_SIDEBAR_WIDTH),
        }
        next
    }
}

/// Which window buttons Morph draws in its frameless window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowButtons {
    /// Close only: tiling window managers place and size windows themselves.
    CloseOnly,
    /// Minimize, maximize and close.
    All,
}

/// Overrides the detection: `close` or `all`.
pub const WINDOW_BUTTONS_ENV: &str = "MORPH_WINDOW_BUTTONS";

/// Tiling compositors and window managers, matched against
/// `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP` and `DESKTOP_SESSION`.
pub const TILING_DESKTOP_IDS: &[&str] = &[
    "hyprland",
    "hypr",
    "sway",
    "i3",
    "i3-gaps",
    "niri",
    "river",
    "awesome",
    "bspwm",
    "dwm",
    "dwl",
    "qtile",
    "xmonad",
    "herbstluftwm",
    "herbstluft",
    "leftwm",
    "spectrwm",
    "wmii",
    "notion",
    "hikari",
    "newm",
    "stumpwm",
    "subtle",
    "dk",
];

/// Variables that mean a tiling compositor runs even when the desktop
/// names are empty or stale.
pub const TILING_ENV_MARKERS: &[&str] = &["HYPRLAND_INSTANCE_SIGNATURE", "SWAYSOCK", "I3SOCK", "NIRI_SOCKET", "BSPWM_SOCKET"];

/// Desktop names from the session variables: lower-cased, split on `:`,
/// `;` and spaces, with paths and a `.desktop` suffix removed.
pub fn desktop_tokens(env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION"] {
        let Some(raw) = env(key) else { continue };
        for part in raw.split([':', ';', ' ']) {
            let mut token = part.trim().to_ascii_lowercase();
            if let Some(base) = token.rsplit('/').next() {
                token = base.to_string();
            }
            if let Some(stripped) = token.strip_suffix(".desktop") {
                token = stripped.to_string();
            }
            if !token.is_empty() {
                out.push(token);
            }
        }
    }
    out
}

/// The buttons for this session; `env` reads an environment variable.
pub fn window_buttons(env: &dyn Fn(&str) -> Option<String>) -> WindowButtons {
    match env(WINDOW_BUTTONS_ENV).map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("close") => return WindowButtons::CloseOnly,
        Some("all") => return WindowButtons::All,
        _ => {}
    }
    if TILING_ENV_MARKERS.iter().any(|m| env(m).is_some_and(|v| !v.is_empty())) {
        return WindowButtons::CloseOnly;
    }
    if desktop_tokens(env).iter().any(|t| TILING_DESKTOP_IDS.contains(&t.as_str())) {
        return WindowButtons::CloseOnly;
    }
    WindowButtons::All
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_core::settings::DEFAULT_SIDEBAR_WIDTH;

    fn with(position: TabPosition, collapsed: bool, width: u32) -> Settings {
        Settings { tab_position: position, sidebar_collapsed: collapsed, sidebar_width: width }
    }

    #[test]
    fn each_position_places_the_tabs() {
        assert_eq!(TabStrip::from_settings(&Settings::default()), TabStrip::Top);
        assert!(TabStrip::from_settings(&Settings::default()).is_caption());
        assert_eq!(TabStrip::from_settings(&with(TabPosition::Bottom, false, 220)), TabStrip::Bottom);
        assert_eq!(TabStrip::from_settings(&with(TabPosition::Left, false, 300)), TabStrip::Side { left: true, width: 300, collapsed: false });
        assert_eq!(
            TabStrip::from_settings(&with(TabPosition::Right, true, 300)),
            TabStrip::Side { left: false, width: COLLAPSED_TAB_LIST_WIDTH, collapsed: true }
        );
        assert!(!TabStrip::from_settings(&with(TabPosition::Right, false, 220)).is_caption());
    }

    #[test]
    fn a_stored_width_out_of_range_is_clamped() {
        let strip = TabStrip::from_settings(&with(TabPosition::Left, false, 9000));
        assert_eq!(strip, TabStrip::Side { left: true, width: MAX_SIDEBAR_WIDTH, collapsed: false });
    }

    #[test]
    fn panel_keys_change_the_settings() {
        let s = Settings::default();
        assert_eq!(SettingsChange::for_key("left").unwrap().apply(&s).tab_position, TabPosition::Left);
        assert_eq!(SettingsChange::for_key("right").unwrap().apply(&s).tab_position, TabPosition::Right);
        assert_eq!(SettingsChange::for_key("down").unwrap().apply(&s).tab_position, TabPosition::Bottom);
        assert_eq!(SettingsChange::for_key("up").unwrap().apply(&with(TabPosition::Left, false, 220)).tab_position, TabPosition::Top);
        assert!(SettingsChange::for_key("c").unwrap().apply(&s).sidebar_collapsed);
        assert_eq!(SettingsChange::for_key("=").unwrap().apply(&s).sidebar_width, DEFAULT_SIDEBAR_WIDTH + TAB_LIST_WIDTH_STEP);
        assert_eq!(SettingsChange::for_key("-").unwrap().apply(&s).sidebar_width, DEFAULT_SIDEBAR_WIDTH - TAB_LIST_WIDTH_STEP);
        assert_eq!(SettingsChange::for_key("x"), None);
    }

    #[test]
    fn resizing_stops_at_the_limits() {
        assert_eq!(SettingsChange::Wider.apply(&with(TabPosition::Left, false, MAX_SIDEBAR_WIDTH - 5)).sidebar_width, MAX_SIDEBAR_WIDTH);
        assert_eq!(SettingsChange::Narrower.apply(&with(TabPosition::Left, false, MIN_SIDEBAR_WIDTH + 5)).sidebar_width, MIN_SIDEBAR_WIDTH);
    }

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
    }

    #[test]
    fn floating_desktops_get_every_button() {
        assert_eq!(window_buttons(&env(&[])), WindowButtons::All);
        assert_eq!(window_buttons(&env(&[("XDG_CURRENT_DESKTOP", "GNOME")])), WindowButtons::All);
        assert_eq!(window_buttons(&env(&[("XDG_CURRENT_DESKTOP", "KDE"), ("DESKTOP_SESSION", "plasma")])), WindowButtons::All);
    }

    #[test]
    fn tiling_desktops_get_close_only() {
        assert_eq!(window_buttons(&env(&[("XDG_CURRENT_DESKTOP", "Hyprland")])), WindowButtons::CloseOnly);
        assert_eq!(window_buttons(&env(&[("XDG_CURRENT_DESKTOP", "sway:wlroots")])), WindowButtons::CloseOnly);
        assert_eq!(window_buttons(&env(&[("DESKTOP_SESSION", "/usr/share/xsessions/i3.desktop")])), WindowButtons::CloseOnly);
        assert_eq!(window_buttons(&env(&[("SWAYSOCK", "/run/user/1000/sway.sock")])), WindowButtons::CloseOnly);
        assert_eq!(window_buttons(&env(&[("SWAYSOCK", "")])), WindowButtons::All, "an empty marker is ignored");
    }

    #[test]
    fn the_override_wins() {
        assert_eq!(window_buttons(&env(&[("XDG_CURRENT_DESKTOP", "Hyprland"), ("MORPH_WINDOW_BUTTONS", "all")])), WindowButtons::All);
        assert_eq!(window_buttons(&env(&[("MORPH_WINDOW_BUTTONS", " Close ")])), WindowButtons::CloseOnly);
        assert_eq!(window_buttons(&env(&[("XDG_CURRENT_DESKTOP", "niri"), ("MORPH_WINDOW_BUTTONS", "bogus")])), WindowButtons::CloseOnly);
    }

    #[test]
    fn desktop_names_are_tokenized() {
        let tokens = desktop_tokens(&env(&[("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"), ("DESKTOP_SESSION", "/a/b/i3.desktop")]));
        assert_eq!(tokens, ["ubuntu", "gnome", "i3"]);
    }
}
