//! The open windows and their tabs, kept so the next launch can bring
//! them back (`<home>/profiles/<id>/tabs.json`).
//!
//! Each profile keeps the windows that showed it. Only addresses are kept
//! (no page state, no form data), and only ones worth reopening: blank
//! tabs, `data:` and `about:` pages are left out. A window with nothing
//! left to reopen is dropped.
use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file format version this build writes.
pub const VERSION: u32 = 1;

/// One window: its tabs' addresses, left to right, and which is active.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSession {
    pub tabs: Vec<String>,
    /// Index into `tabs`.
    #[serde(default)]
    pub active: usize,
}

impl WindowSession {
    /// The window as it should be saved: only the tabs worth reopening,
    /// with the active index moved to match. `None` when nothing is left.
    pub fn kept(tabs: &[String], active: usize) -> Option<Self> {
        let mut kept = Vec::new();
        let mut new_active = 0;
        for (i, url) in tabs.iter().enumerate() {
            if !is_restorable(url) {
                continue;
            }
            if i <= active {
                new_active = kept.len();
            }
            kept.push(url.clone());
        }
        (!kept.is_empty()).then_some(Self { tabs: kept, active: new_active })
    }

    /// The active index, clamped to the tabs there are.
    pub fn active_index(&self) -> usize {
        self.active.min(self.tabs.len().saturating_sub(1))
    }
}

/// A profile's saved windows, in the order they were opened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub windows: Vec<WindowSession>,
}

impl Session {
    pub fn new(windows: Vec<WindowSession>) -> Self {
        Self { version: VERSION, windows }
    }
}

/// Whether a tab at `url` is worth reopening at the next launch.
pub fn is_restorable(url: &str) -> bool {
    let url = url.trim();
    !(url.is_empty() || url.starts_with("data:") || url.starts_with("about:") || url.starts_with("blob:") || url.starts_with("javascript:"))
}

/// The profile's saved windows (none when there is no file or it cannot
/// be read: a bad file never keeps Morph from starting).
pub fn load(home: impl AsRef<Path>, profile: &str) -> Session {
    let path = crate::profiles::tabs_path(home, profile);
    let Ok(body) = fs::read(&path) else {
        return Session::default();
    };
    match serde_json::from_slice::<Session>(&body) {
        Ok(mut session) => {
            session.windows.retain(|w| !w.tabs.is_empty());
            session
        }
        Err(_) => Session::default(),
    }
}

/// Replace the profile's saved windows (atomic). No windows removes the file.
pub fn save(home: impl AsRef<Path>, profile: &str, session: &Session) -> std::io::Result<()> {
    let path = crate::profiles::tabs_path(home, profile);
    if session.windows.is_empty() {
        return match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(&Session::new(session.windows.clone()))?;
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&body)?;
        file.write_all(b"\n")?;
    }
    fs::rename(tmp, path)
}

/// The windows open app-wide, each keyed by `K` and showing one profile,
/// and the windows a profile switch put aside: together they are what each
/// profile's `tabs.json` holds.
///
/// A window that switches from profile A to B leaves its tabs with A (put
/// aside, still saved in A's file, so a crash or quit keeps them) and takes
/// B's windows put aside before, if any. Switching back to A brings A's
/// windows back as they were.
#[derive(Debug, Clone)]
pub struct OpenWindows<K> {
    windows: Vec<OpenWindow<K>>,
    aside: Vec<(String, WindowSession)>,
}

#[derive(Debug, Clone)]
struct OpenWindow<K> {
    key: K,
    profile: String,
    session: Option<WindowSession>,
}

impl<K> Default for OpenWindows<K> {
    fn default() -> Self {
        Self { windows: Vec::new(), aside: Vec::new() }
    }
}

/// What a window's switch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switched {
    /// The profile the window showed until now.
    pub from: String,
    /// The windows of the new profile to show: the first in the window
    /// that switched, the others in new windows (each opened with
    /// [`OpenWindows::add`], which takes it from the ones put aside).
    pub windows: Vec<WindowSession>,
}

impl<K: PartialEq> OpenWindows<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Window `key` opened showing `profile` with `session` (a saved window
    /// it restores, so the session keeps it before the window reports its
    /// tabs). A window put aside with the same tabs is taken back.
    pub fn add(&mut self, key: K, profile: &str, session: Option<WindowSession>) {
        if let Some(session) = &session
            && let Some(at) = self.aside.iter().position(|(p, w)| p == profile && w == session)
        {
            self.aside.remove(at);
        }
        self.windows.retain(|w| w.key != key);
        self.windows.push(OpenWindow { key, profile: profile.to_string(), session });
    }

    /// The window shows `session` now. Returns its profile, to save.
    pub fn update(&mut self, key: &K, session: Option<WindowSession>) -> Option<&str> {
        let w = self.windows.iter_mut().find(|w| w.key == *key)?;
        w.session = session;
        Some(&w.profile)
    }

    /// The window closed while others stay open: its tabs leave the
    /// session. Returns its profile, to save.
    pub fn remove(&mut self, key: &K) -> Option<String> {
        let at = self.windows.iter().position(|w| w.key == *key)?;
        Some(self.windows.remove(at).profile)
    }

    /// The profile window `key` shows.
    pub fn profile(&self, key: &K) -> Option<&str> {
        self.windows.iter().find(|w| w.key == *key).map(|w| w.profile.as_str())
    }

    /// Window `key`, showing `leaving` (its tabs as they are now, taken
    /// before its pages go), switches to profile `to`. Its tabs are put
    /// aside for the profile it leaves; it takes the first window `to` had
    /// put aside. `None` when there is no such window or it shows `to`
    /// already. Save both profiles afterwards.
    pub fn switch(&mut self, key: &K, leaving: Option<WindowSession>, to: &str) -> Option<Switched> {
        let w = self.windows.iter_mut().find(|w| w.key == *key)?;
        if w.profile == to {
            return None;
        }
        let from = std::mem::replace(&mut w.profile, to.to_string());
        if let Some(leaving) = leaving {
            self.aside.push((from.clone(), leaving));
        }
        let windows: Vec<WindowSession> = self.aside.iter().filter(|(p, _)| p == to).map(|(_, s)| s.clone()).collect();
        // The first one is the switching window's now; the others stay
        // aside (and saved) until their windows open.
        if let Some(at) = self.aside.iter().position(|(p, _)| p == to) {
            self.aside.remove(at);
        }
        w.session = windows.first().cloned();
        Some(Switched { from, windows })
    }

    /// What `profile`'s `tabs.json` should hold: its open windows, then the
    /// ones put aside, in the order they were.
    pub fn session(&self, profile: &str) -> Session {
        let open = self.windows.iter().filter(|w| w.profile == profile).filter_map(|w| w.session.clone());
        let aside = self.aside.iter().filter(|(p, _)| p == profile).map(|(_, s)| s.clone());
        Session::new(open.chain(aside).collect())
    }

    /// Write `profile`'s windows to its `tabs.json`.
    pub fn save(&self, home: impl AsRef<Path>, profile: &str) -> std::io::Result<()> {
        save(home, profile, &self.session(profile))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn only_tabs_worth_reopening_are_kept_and_the_active_one_follows() {
        let w = WindowSession::kept(&s(&["", "https://a.example/", "data:text/html,x", "https://b.example/"]), 3).unwrap();
        assert_eq!(w.tabs, s(&["https://a.example/", "https://b.example/"]));
        assert_eq!(w.active, 1);
        // The active tab was dropped: the one before it is active.
        let w = WindowSession::kept(&s(&["https://a.example/", "about:blank", "https://b.example/"]), 1).unwrap();
        assert_eq!(w.active, 0);
        assert_eq!(WindowSession::kept(&s(&["", "about:blank"]), 0), None);
        assert!(is_restorable("morph://settings"));
    }

    #[test]
    fn windows_round_trip_per_profile_and_none_removes_the_file() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(load(home.path(), "default"), Session::default());
        let session = Session::new(vec![
            WindowSession { tabs: s(&["https://a.example/", "https://b.example/"]), active: 1 },
            WindowSession { tabs: s(&["https://c.example/"]), active: 0 },
        ]);
        save(home.path(), "default", &session).unwrap();
        assert_eq!(load(home.path(), "default"), session);
        assert_eq!(load(home.path(), "other").windows.len(), 0, "profiles keep their own windows");
        save(home.path(), "default", &Session::default()).unwrap();
        assert!(!crate::profiles::tabs_path(home.path(), "default").exists());
        save(home.path(), "default", &Session::default()).unwrap();
    }

    #[test]
    fn a_bad_file_restores_nothing() {
        let home = tempfile::tempdir().unwrap();
        let path = crate::profiles::tabs_path(home.path(), "default");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{not json").unwrap();
        assert_eq!(load(home.path(), "default"), Session::default());
        fs::write(&path, br#"{"version":1,"windows":[{"tabs":[],"active":0},{"tabs":["https://a.example/"],"active":4}]}"#).unwrap();
        let loaded = load(home.path(), "default");
        assert_eq!(loaded.windows.len(), 1);
        assert_eq!(loaded.windows[0].active_index(), 0);
    }

    fn window(tabs: &[&str], active: usize) -> WindowSession {
        WindowSession { tabs: s(tabs), active }
    }

    /// m3talsmith's report: switching profiles lost the tabs. A window with A's
    /// three tabs switches to B and back: A has the same tabs and active
    /// tab, on disk after every step (a crash or quit keeps them).
    #[test]
    fn a_profile_switch_and_back_keeps_the_tabs() {
        let home = tempfile::tempdir().unwrap();
        let a = window(&["https://a.example/", "https://b.example/", "https://c.example/"], 1);
        let mut open = OpenWindows::new();
        open.add(1, "A", None);
        assert_eq!(open.update(&1, Some(a.clone())), Some("A"));
        open.save(home.path(), "A").unwrap();

        let to_b = open.switch(&1, Some(a.clone()), "B").unwrap();
        assert_eq!(to_b, Switched { from: "A".into(), windows: vec![] }, "B had nothing put aside: a blank window");
        open.save(home.path(), "A").unwrap();
        open.save(home.path(), "B").unwrap();
        assert_eq!(load(home.path(), "A").windows, vec![a.clone()], "A's tabs stay saved while B shows");
        assert_eq!(open.profile(&1), Some("B"));
        // B's blank tab reports no session; A's file must not change.
        assert_eq!(open.update(&1, None), Some("B"));
        open.save(home.path(), "B").unwrap();
        assert_eq!(load(home.path(), "A").windows, vec![a.clone()]);
        let b = window(&["https://work.example/"], 0);
        open.update(&1, Some(b.clone()));
        open.save(home.path(), "B").unwrap();

        let back = open.switch(&1, Some(b.clone()), "A").unwrap();
        assert_eq!(back, Switched { from: "B".into(), windows: vec![a.clone()] });
        open.save(home.path(), "A").unwrap();
        open.save(home.path(), "B").unwrap();
        assert_eq!(load(home.path(), "A").windows, vec![a.clone()]);
        assert_eq!(load(home.path(), "A").windows[0].active_index(), 1);
        assert_eq!(load(home.path(), "B").windows, vec![b.clone()], "and B's tabs wait for the next switch");

        // To B once more: B's tabs come back too, and the same session
        // twice is not a switch.
        assert_eq!(open.switch(&1, Some(a.clone()), "B").unwrap().windows, vec![b]);
        assert_eq!(open.switch(&1, None, "B"), None);
        assert_eq!(open.switch(&2, None, "A"), None, "no such window");
    }

    /// Two windows of A both switch to B; switching one back brings both
    /// of A's windows back (the second in a new window, which `add` takes
    /// from the ones put aside), and none is saved twice.
    #[test]
    fn every_window_of_a_profile_comes_back() {
        let home = tempfile::tempdir().unwrap();
        let w1 = window(&["https://one.example/", "https://two.example/"], 0);
        let w2 = window(&["https://three.example/"], 0);
        let mut open = OpenWindows::new();
        open.add(1, "A", Some(w1.clone()));
        open.add(2, "A", Some(w2.clone()));
        open.save(home.path(), "A").unwrap();
        assert_eq!(load(home.path(), "A").windows, vec![w1.clone(), w2.clone()]);

        open.switch(&1, Some(w1.clone()), "B").unwrap();
        open.update(&1, None);
        open.save(home.path(), "A").unwrap();
        // A's file lists the open window first, then the one put aside.
        assert_eq!(load(home.path(), "A").windows, vec![w2.clone(), w1.clone()]);
        open.switch(&2, Some(w2.clone()), "B").unwrap();
        open.update(&2, None);
        open.save(home.path(), "A").unwrap();
        open.save(home.path(), "B").unwrap();
        assert_eq!(load(home.path(), "A").windows, vec![w1.clone(), w2.clone()]);
        assert_eq!(load(home.path(), "B").windows, vec![]);

        let back = open.switch(&1, None, "A").unwrap();
        assert_eq!(back.windows, vec![w1.clone(), w2.clone()]);
        open.save(home.path(), "A").unwrap();
        assert_eq!(load(home.path(), "A").windows, vec![w1.clone(), w2.clone()], "the second is saved while its window opens");
        open.add(3, "A", Some(w2.clone()));
        open.save(home.path(), "A").unwrap();
        assert_eq!(load(home.path(), "A").windows, vec![w1.clone(), w2.clone()], "and once it opened, not twice");
        assert_eq!(open.session("A").windows.len(), 2);

        // Closing a window takes its tabs out; the other window stays.
        assert_eq!(open.remove(&3).as_deref(), Some("A"));
        open.save(home.path(), "A").unwrap();
        assert_eq!(load(home.path(), "A").windows, vec![w1]);
    }
}
