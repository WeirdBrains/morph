//! The profile's library (bookmarks, history) at launch.

use std::path::Path;

use morph_services::library::{self, LibraryOpen};

/// Open `profile`'s library with the key kept in the system keystore, if it
/// is encrypted. Returns whether history can be recorded this session: not
/// while the library stays locked (the password prompt is phase 3 work).
pub fn open_at_launch(home: &Path, profile: &str) -> bool {
    match library::open(home, profile) {
        Ok(LibraryOpen::Ready) => true,
        Ok(LibraryOpen::NeedsPassword { reason }) => {
            let why = reason.map(|r| format!(" ({r})")).unwrap_or_default();
            eprintln!("morph: the library of profile {profile} is locked{why}; history is off for this session");
            false
        }
        Err(e) => {
            eprintln!("morph: library of profile {profile}: {e}; history is off for this session");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_services::SecretString;

    #[test]
    fn history_follows_whether_the_library_opens() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::create(&home, "Work").unwrap();
        assert!(open_at_launch(&home, "p1"), "plaintext library");

        morph_services::identity::create(&home, "p1", &SecretString::from("long enough".to_string())).unwrap();
        library::lock(&home, "p1");
        assert!(open_at_launch(&home, "p1"), "the saved key opens it");

        library::lock(&home, "p1");
        morph_keystore::forget(&morph_core::profiles::profile_dir(&home, "p1")).unwrap();
        assert!(!open_at_launch(&home, "p1"), "locked without a key");
        assert!(!open_at_launch(&home, "nope"), "unknown profile");
    }
}
