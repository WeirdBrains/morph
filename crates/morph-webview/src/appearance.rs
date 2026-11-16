//! Light or dark for pages on Linux. WebKitGTK answers
//! `prefers-color-scheme` (and draws form controls in pages that ask for
//! `color-scheme: dark`) from GTK's `gtk-application-prefer-dark-theme`,
//! which GTK 3 does not take from the desktop's setting. Morph reads the
//! desktop's choice, the XDG desktop portal's `org.freedesktop.appearance
//! color-scheme`, as Chrome and Firefox do, then GNOME's own key, then the
//! GTK theme, and otherwise follows its own (dark) theme; `MORPH_COLOR_SCHEME`
//! (`dark` or `light`) overrides them all. A change on the portal applies at
//! once to open pages.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
}

/// What the scheme is decided from, most important first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sources {
    /// `MORPH_COLOR_SCHEME`.
    pub env: Option<String>,
    /// The portal's `color-scheme`: 0 no preference, 1 dark, 2 light.
    pub portal: Option<u32>,
    /// GNOME's `org.gnome.desktop.interface color-scheme`
    /// (`prefer-dark`, `prefer-light`, `default`).
    pub gnome: Option<String>,
    /// GTK's own setting, from `settings.ini` or `GTK_THEME`.
    pub gtk_prefer_dark: bool,
    /// The GTK theme's name (`Adwaita-dark`, or `GTK_THEME=Adwaita:dark`).
    pub gtk_theme: Option<String>,
}

/// Morph's own theme, for when the desktop says nothing.
pub const SHELL_SCHEME: Scheme = Scheme::Dark;

/// The scheme `sources` ask for.
pub fn pick(sources: &Sources) -> Scheme {
    match sources.env.as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("dark") => return Scheme::Dark,
        Some("light") => return Scheme::Light,
        _ => {}
    }
    match sources.portal {
        Some(1) => return Scheme::Dark,
        Some(2) => return Scheme::Light,
        _ => {}
    }
    match sources.gnome.as_deref() {
        Some("prefer-dark") => return Scheme::Dark,
        Some("prefer-light") => return Scheme::Light,
        _ => {}
    }
    if sources.gtk_prefer_dark || sources.gtk_theme.as_deref().is_some_and(|t| t.to_ascii_lowercase().contains("dark")) {
        return Scheme::Dark;
    }
    SHELL_SCHEME
}

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";

/// A `u32` from a portal answer, which older portals wrap twice.
fn unwrap_u32(value: &glib::Variant) -> Option<u32> {
    let mut v = value.clone();
    for _ in 0..3 {
        if let Some(n) = v.get::<u32>() {
            return Some(n);
        }
        if v.type_() != glib::VariantTy::VARIANT {
            return None;
        }
        v = v.as_variant()?;
    }
    None
}

fn read_portal(bus: &gio::DBusConnection) -> Option<u32> {
    let call = |method: &str| {
        bus.call_sync(
            Some(PORTAL),
            PORTAL_PATH,
            SETTINGS,
            method,
            Some(&("org.freedesktop.appearance", "color-scheme").to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            500,
            gio::Cancellable::NONE,
        )
        .ok()
    };
    let answer = call("ReadOne").or_else(|| call("Read"))?;
    unwrap_u32(&answer.child_value(0))
}

fn read_gnome() -> Option<String> {
    let source = gio::SettingsSchemaSource::default()?;
    let schema = source.lookup("org.gnome.desktop.interface", true)?;
    if !schema.has_key("color-scheme") {
        return None;
    }
    Some(gio::Settings::new("org.gnome.desktop.interface").string("color-scheme").to_string())
}

/// What GTK itself was told (before Morph changes it).
fn gtk_sources(settings: &gtk::Settings) -> (bool, Option<String>) {
    let theme = std::env::var("GTK_THEME").ok().filter(|t| !t.is_empty()).or_else(|| settings.gtk_theme_name().map(|s| s.to_string()));
    (settings.is_gtk_application_prefer_dark_theme(), theme)
}

fn apply(settings: &gtk::Settings, scheme: Scheme) {
    let dark = scheme == Scheme::Dark;
    if settings.is_gtk_application_prefer_dark_theme() != dark {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
}

/// Decide the scheme and apply it, and follow the portal from now on. Call
/// on the GTK thread after `gtk::init`. Returns the scheme chosen.
pub fn start() -> Scheme {
    let Some(settings) = gtk::Settings::default() else {
        return SHELL_SCHEME;
    };
    let (gtk_prefer_dark, gtk_theme) = gtk_sources(&settings);
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok();
    let base = Sources {
        env: std::env::var("MORPH_COLOR_SCHEME").ok(),
        portal: bus.as_ref().and_then(read_portal),
        gnome: read_gnome(),
        gtk_prefer_dark,
        gtk_theme,
    };
    let scheme = pick(&base);
    apply(&settings, scheme);
    if let Some(bus) = bus {
        let id = bus.signal_subscribe(
            Some(PORTAL),
            Some(SETTINGS),
            Some("SettingChanged"),
            Some(PORTAL_PATH),
            Some("org.freedesktop.appearance"),
            gio::DBusSignalFlags::NONE,
            move |_, _, _, _, _, params| {
                if params.child_value(1).get::<String>().as_deref() != Some("color-scheme") {
                    return;
                }
                let sources = Sources { portal: unwrap_u32(&params.child_value(2)), ..base.clone() };
                if let Some(settings) = gtk::Settings::default() {
                    apply(&settings, pick(&sources));
                }
            },
        );
        // The subscription lasts as long as Morph does: the id is never
        // unsubscribed and the connection is kept alive.
        let _ = id;
        std::mem::forget(bus);
    }
    scheme
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_wins_then_the_portal() {
        let s = Sources { env: Some("light".into()), portal: Some(1), ..Default::default() };
        assert_eq!(pick(&s), Scheme::Light);
        let s = Sources { env: Some("nonsense".into()), portal: Some(1), gnome: Some("prefer-light".into()), ..Default::default() };
        assert_eq!(pick(&s), Scheme::Dark);
        assert_eq!(pick(&Sources { portal: Some(2), gtk_prefer_dark: true, ..Default::default() }), Scheme::Light);
    }

    #[test]
    fn no_portal_preference_falls_through_to_gnome_and_gtk() {
        assert_eq!(pick(&Sources { portal: Some(0), gnome: Some("prefer-light".into()), ..Default::default() }), Scheme::Light);
        assert_eq!(pick(&Sources { gnome: Some("default".into()), gtk_theme: Some("Adwaita:dark".into()), ..Default::default() }), Scheme::Dark);
        assert_eq!(pick(&Sources { gtk_prefer_dark: true, ..Default::default() }), Scheme::Dark);
    }

    #[test]
    fn with_nothing_said_pages_follow_morphs_theme() {
        assert_eq!(pick(&Sources::default()), SHELL_SCHEME);
        assert_eq!(pick(&Sources { gtk_theme: Some("Adwaita".into()), ..Default::default() }), SHELL_SCHEME);
    }

    #[test]
    fn portal_answers_are_unwrapped() {
        assert_eq!(unwrap_u32(&1u32.to_variant()), Some(1));
        assert_eq!(unwrap_u32(&glib::Variant::from_variant(&2u32.to_variant())), Some(2));
        let twice = glib::Variant::from_variant(&glib::Variant::from_variant(&1u32.to_variant()));
        assert_eq!(unwrap_u32(&twice), Some(1));
        assert_eq!(unwrap_u32(&"x".to_variant()), None);
    }
}
