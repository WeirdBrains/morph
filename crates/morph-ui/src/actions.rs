//! Named actions and the keymap. Menus, keybindings and help dispatch the
//! same names (collective rule), so the keymap is data: keystroke to action.

/// Every action a shell can dispatch, by stable name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    NewTab,
    /// Open a new browser window (same profile) with a fresh tab.
    NewWindow,
    CloseTab,
    NextTab,
    PrevTab,
    /// Select the tab at a 0-based index.
    SelectTab(usize),
    SelectLastTab,
    FocusAddress,
    /// Open the find bar beside the page.
    Find,
    /// Next match in the find bar (opens it when closed).
    FindNext,
    /// Previous match in the find bar (opens it when closed).
    FindPrevious,
    Reload,
    Stop,
    Back,
    Forward,
    ToggleFullscreen,
    Quit,
    /// Bookmark the page, or remove its bookmark.
    Bookmark,
    /// Open or close the library on its bookmarks.
    ShowBookmarks,
    /// Open or close the library on its history.
    ShowHistory,
    /// Open or close the profiles panel.
    ShowProfiles,
    /// Open or close the settings panel.
    ShowSettings,
    /// Open or close the identity panel.
    ShowIdentity,
    /// Show the About page (morph://about).
    ShowAbout,
    /// Show the preferences at the search engine section.
    ShowSearchSettings,
    /// Open or close the downloads panel (the toolbar's downloads button).
    ShowDownloads,
    /// Open or close the Companion panel (the right-hand slot).
    ToggleCompanions,
}

impl Action {
    /// The stable name, as `component.verb`.
    pub fn name(self) -> String {
        match self {
            Action::NewTab => "browser.new_tab".into(),
            Action::NewWindow => "shell.new_window".into(),
            Action::CloseTab => "browser.close_tab".into(),
            Action::NextTab => "browser.next_tab".into(),
            Action::PrevTab => "browser.prev_tab".into(),
            Action::SelectTab(i) => format!("browser.select_tab_{}", i + 1),
            Action::SelectLastTab => "browser.select_last_tab".into(),
            Action::FocusAddress => "browser.focus_address".into(),
            Action::Find => "browser.find".into(),
            Action::FindNext => "browser.find_next".into(),
            Action::FindPrevious => "browser.find_previous".into(),
            Action::Reload => "browser.reload".into(),
            Action::Stop => "browser.stop".into(),
            Action::Back => "browser.back".into(),
            Action::Forward => "browser.forward".into(),
            Action::ToggleFullscreen => "shell.toggle_fullscreen".into(),
            Action::Quit => "shell.quit".into(),
            Action::Bookmark => "library.bookmark".into(),
            Action::ShowBookmarks => "library.show_bookmarks".into(),
            Action::ShowHistory => "library.show_history".into(),
            Action::ShowProfiles => "shell.show_profiles".into(),
            Action::ShowSettings => "shell.show_settings".into(),
            Action::ShowIdentity => "shell.show_identity".into(),
            Action::ShowAbout => "shell.show_about".into(),
            Action::ShowSearchSettings => "shell.show_search_settings".into(),
            Action::ShowDownloads => "browser.show_downloads".into(),
            Action::ToggleCompanions => "shell.toggle_companions".into(),
        }
    }

    /// The action with this name, if there is one.
    pub fn from_name(name: &str) -> Option<Action> {
        let fixed = [
            Action::NewTab,
            Action::NewWindow,
            Action::CloseTab,
            Action::NextTab,
            Action::PrevTab,
            Action::SelectLastTab,
            Action::FocusAddress,
            Action::Find,
            Action::FindNext,
            Action::FindPrevious,
            Action::Reload,
            Action::Stop,
            Action::Back,
            Action::Forward,
            Action::ToggleFullscreen,
            Action::Quit,
            Action::Bookmark,
            Action::ShowBookmarks,
            Action::ShowHistory,
            Action::ShowProfiles,
            Action::ShowSettings,
            Action::ShowIdentity,
            Action::ShowAbout,
            Action::ShowSearchSettings,
            Action::ShowDownloads,
            Action::ToggleCompanions,
        ];
        if let Some(action) = fixed.into_iter().find(|a| a.name() == name) {
            return Some(action);
        }
        let n: usize = name.strip_prefix("browser.select_tab_")?.parse().ok()?;
        (1..=8).contains(&n).then(|| Action::SelectTab(n - 1))
    }
}

/// One binding: a keystroke in GPUI's notation (`ctrl-t`, `ctrl-shift-tab`,
/// `alt-left`) and the action it dispatches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub keystroke: String,
    pub action: Action,
}

/// The keymap: bindings in priority order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Default for Keymap {
    /// The Flutter shell's shortcuts on main (`shell_shortcuts.dart`), plus
    /// the usual back, forward, reload and stop keys.
    fn default() -> Self {
        let fixed = [
            ("ctrl-t", Action::NewTab),
            ("ctrl-n", Action::NewWindow),
            ("ctrl-w", Action::CloseTab),
            ("ctrl-tab", Action::NextTab),
            ("ctrl-shift-tab", Action::PrevTab),
            ("ctrl-l", Action::FocusAddress),
            ("ctrl-f", Action::Find),
            ("f3", Action::FindNext),
            ("shift-f3", Action::FindPrevious),
            ("ctrl-g", Action::FindNext),
            ("ctrl-shift-g", Action::FindPrevious),
            ("ctrl-r", Action::Reload),
            ("f5", Action::Reload),
            ("alt-left", Action::Back),
            ("alt-right", Action::Forward),
            ("f11", Action::ToggleFullscreen),
            ("ctrl-q", Action::Quit),
            ("ctrl-d", Action::Bookmark),
            ("ctrl-shift-o", Action::ShowBookmarks),
            ("ctrl-h", Action::ShowHistory),
            ("ctrl-shift-m", Action::ShowProfiles),
            ("ctrl-,", Action::ShowSettings),
            ("ctrl-shift-l", Action::ShowIdentity),
            ("ctrl-shift-y", Action::ShowDownloads),
            ("ctrl-shift-k", Action::ToggleCompanions),
        ];
        let mut bindings: Vec<Binding> = fixed.into_iter().map(|(k, a)| Binding { keystroke: k.into(), action: a }).collect();
        bindings.extend((1..=8).map(|n| Binding { keystroke: format!("ctrl-{n}"), action: Action::SelectTab(n - 1) }));
        bindings.push(Binding { keystroke: "ctrl-9".into(), action: Action::SelectLastTab });
        // Second keys other browsers have, after the first so menus show
        // the first.
        let more = [("ctrl-shift-r", Action::Reload), ("shift-f5", Action::Reload), ("ctrl-f5", Action::Reload)];
        bindings.extend(more.into_iter().map(|(k, a)| Binding { keystroke: k.into(), action: a }));
        Self { bindings }
    }
}

/// Keystrokes that keep Ctrl on macOS: switching tabs is Ctrl+Tab there too.
pub const MAC_CTRL_EXCEPTIONS: [&str; 2] = ["ctrl-tab", "ctrl-shift-tab"];

/// Keys macOS browsers use instead: Cmd+H hides the app, so history is
/// Cmd+Y; Option+arrows move by word in text fields, so back and forward
/// are Cmd+[ and Cmd+]; F11 shows the desktop, so full screen is Ctrl+Cmd+F;
/// downloads are Cmd+Shift+J there, as in Chrome.
pub const MAC_RENAMES: [(&str, &str); 5] =
    [("ctrl-h", "cmd-y"), ("alt-left", "cmd-["), ("alt-right", "cmd-]"), ("f11", "ctrl-cmd-f"), ("ctrl-shift-y", "cmd-shift-j")];

/// Keys only one side has (Windows and Linux, or macOS), added after the
/// shared ones.
const PC_ONLY: [(&str, Action); 5] = [
    ("ctrl-pagedown", Action::NextTab),
    ("ctrl-pageup", Action::PrevTab),
    ("ctrl-f4", Action::CloseTab),
    ("f6", Action::FocusAddress),
    ("alt-d", Action::FocusAddress),
];
const MAC_ONLY: [(&str, Action); 2] = [("cmd-alt-right", Action::NextTab), ("cmd-alt-left", Action::PrevTab)];

/// `keystroke` on the platform: on macOS Morph's Ctrl shortcuts use Cmd
/// (`ctrl-shift-o` is `cmd-shift-o`), except [`MAC_CTRL_EXCEPTIONS`].
pub fn platform_keystroke(keystroke: &str, mac: bool) -> String {
    if let Some((_, to)) = MAC_RENAMES.iter().find(|(from, _)| mac && *from == keystroke) {
        return to.to_string();
    }
    match keystroke.strip_prefix("ctrl-") {
        Some(rest) if mac && !MAC_CTRL_EXCEPTIONS.contains(&keystroke) => format!("cmd-{rest}"),
        _ => keystroke.to_string(),
    }
}

impl Keymap {
    /// The keymap as bound on the platform ([`platform_keystroke`]): the one
    /// place the macOS Cmd rule lives.
    pub fn for_platform(&self, mac: bool) -> Keymap {
        let mut bindings: Vec<Binding> =
            self.bindings.iter().map(|b| Binding { keystroke: platform_keystroke(&b.keystroke, mac), action: b.action }).collect();
        for (k, a) in if mac { &MAC_ONLY[..] } else { &PC_ONLY[..] } {
            if !bindings.iter().any(|b| b.keystroke == *k) {
                bindings.push(Binding { keystroke: k.to_string(), action: *a });
            }
        }
        Keymap { bindings }
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// The action bound to `keystroke`, if any.
    pub fn action_for(&self, keystroke: &str) -> Option<Action> {
        self.bindings.iter().find(|b| b.keystroke == keystroke).map(|b| b.action)
    }

    /// The first keystroke bound to `action`, for menus and help.
    pub fn keystroke_for(&self, action: Action) -> Option<&str> {
        self.bindings.iter().find(|b| b.action == action).map(|b| b.keystroke.as_str())
    }

    /// Bind `keystroke` to `action`, replacing whatever it did before.
    pub fn bind(&mut self, keystroke: &str, action: Action) {
        self.bindings.retain(|b| b.keystroke != keystroke);
        self.bindings.insert(0, Binding { keystroke: keystroke.into(), action });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_macos_every_shortcut_uses_cmd_but_tab_switching() {
        let mac = Keymap::default().for_platform(true);
        for b in mac.bindings() {
            assert!(
                !b.keystroke.starts_with("ctrl-") || b.keystroke.contains("cmd-") || MAC_CTRL_EXCEPTIONS.contains(&b.keystroke.as_str()),
                "{} still uses Ctrl on macOS",
                b.keystroke
            );
        }
        assert_eq!(mac.action_for("cmd-t"), Some(Action::NewTab));
        assert_eq!(mac.action_for("cmd-n"), Some(Action::NewWindow));
        assert_eq!(mac.action_for("cmd-shift-l"), Some(Action::ShowIdentity));
        assert_eq!(mac.action_for("cmd-,"), Some(Action::ShowSettings));
        assert_eq!(mac.action_for("cmd-9"), Some(Action::SelectLastTab));
        assert_eq!(mac.action_for("ctrl-tab"), Some(Action::NextTab));
        assert_eq!(mac.action_for("ctrl-t"), None);
    }

    #[test]
    fn the_shortcut_audit_keys_each_platform_expects() {
        let mac = Keymap::default().for_platform(true);
        assert_eq!(mac.action_for("cmd-y"), Some(Action::ShowHistory));
        assert_eq!(mac.action_for("cmd-h"), None, "Cmd+H hides the app");
        assert_eq!(mac.action_for("cmd-["), Some(Action::Back));
        assert_eq!(mac.action_for("cmd-]"), Some(Action::Forward));
        assert_eq!(mac.action_for("alt-left"), None, "Option+Left moves by word");
        assert_eq!(mac.action_for("ctrl-cmd-f"), Some(Action::ToggleFullscreen));
        assert_eq!(mac.action_for("cmd-alt-right"), Some(Action::NextTab));
        assert_eq!(mac.action_for("cmd-shift-r"), Some(Action::Reload));
        assert_eq!(mac.action_for("f6"), None);
        assert_eq!(mac.action_for("cmd-shift-j"), Some(Action::ShowDownloads));
        assert_eq!(mac.action_for("cmd-shift-y"), None);
        let pc = Keymap::default().for_platform(false);
        assert_eq!(pc.action_for("ctrl-h"), Some(Action::ShowHistory));
        assert_eq!(pc.action_for("alt-left"), Some(Action::Back));
        assert_eq!(pc.action_for("ctrl-pagedown"), Some(Action::NextTab));
        assert_eq!(pc.action_for("ctrl-pageup"), Some(Action::PrevTab));
        assert_eq!(pc.action_for("ctrl-f4"), Some(Action::CloseTab));
        assert_eq!(pc.action_for("f6"), Some(Action::FocusAddress));
        assert_eq!(pc.action_for("alt-d"), Some(Action::FocusAddress));
        assert_eq!(pc.action_for("shift-f5"), Some(Action::Reload));
        assert_eq!(pc.action_for("ctrl-shift-y"), Some(Action::ShowDownloads));
        // Menus keep showing the first key.
        assert_eq!(pc.keystroke_for(Action::Reload), Some("ctrl-r"));
        assert_eq!(pc.keystroke_for(Action::NextTab), Some("ctrl-tab"));
        // No key does two things.
        for keymap in [&mac, &pc] {
            let mut seen = std::collections::HashSet::new();
            for b in keymap.bindings() {
                assert!(seen.insert(b.keystroke.clone()), "{} bound twice", b.keystroke);
            }
        }
    }

    #[test]
    fn default_keys_match_the_flutter_shell() {
        let keys = Keymap::default();
        assert_eq!(keys.action_for("ctrl-t"), Some(Action::NewTab));
        assert_eq!(keys.action_for("ctrl-n"), Some(Action::NewWindow));
        assert_eq!(keys.action_for("ctrl-w"), Some(Action::CloseTab));
        assert_eq!(keys.action_for("ctrl-l"), Some(Action::FocusAddress));
        assert_eq!(keys.action_for("ctrl-r"), Some(Action::Reload));
        assert_eq!(keys.action_for("ctrl-f"), Some(Action::Find));
        assert_eq!(keys.action_for("f3"), Some(Action::FindNext));
        assert_eq!(keys.action_for("shift-f3"), Some(Action::FindPrevious));
        assert_eq!(keys.action_for("ctrl-g"), Some(Action::FindNext));
        assert_eq!(keys.action_for("ctrl-shift-g"), Some(Action::FindPrevious));
        assert_eq!(keys.action_for("ctrl-tab"), Some(Action::NextTab));
        assert_eq!(keys.action_for("ctrl-shift-tab"), Some(Action::PrevTab));
        assert_eq!(keys.action_for("ctrl-1"), Some(Action::SelectTab(0)));
        assert_eq!(keys.action_for("ctrl-8"), Some(Action::SelectTab(7)));
        assert_eq!(keys.action_for("ctrl-9"), Some(Action::SelectLastTab));
        assert_eq!(keys.action_for("ctrl-d"), Some(Action::Bookmark));
        assert_eq!(keys.action_for("ctrl-shift-o"), Some(Action::ShowBookmarks));
        assert_eq!(keys.action_for("ctrl-h"), Some(Action::ShowHistory));
        assert_eq!(keys.action_for("ctrl-shift-m"), Some(Action::ShowProfiles));
        assert_eq!(keys.action_for("ctrl-,"), Some(Action::ShowSettings));
        assert_eq!(keys.action_for("ctrl-shift-l"), Some(Action::ShowIdentity));
        assert_eq!(keys.action_for("ctrl-shift-k"), Some(Action::ToggleCompanions));
        assert_eq!(Action::ToggleCompanions.name(), "shell.toggle_companions");
        assert_eq!(keys.action_for("ctrl-x"), None);
    }

    #[test]
    fn names_round_trip() {
        for binding in Keymap::default().bindings() {
            let name = binding.action.name();
            assert_eq!(Action::from_name(&name), Some(binding.action), "{name}");
        }
        assert_eq!(Action::from_name("browser.select_tab_9"), None);
        assert_eq!(Action::from_name("nope"), None);
    }

    #[test]
    fn rebinding_a_key_replaces_it_and_menus_see_the_new_key() {
        let mut keys = Keymap::default();
        keys.bind("ctrl-r", Action::Stop);
        assert_eq!(keys.action_for("ctrl-r"), Some(Action::Stop));
        assert_eq!(keys.keystroke_for(Action::Reload), Some("f5"));
        assert_eq!(keys.keystroke_for(Action::Stop), Some("ctrl-r"));
    }
}
