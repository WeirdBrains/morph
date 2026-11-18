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
}
