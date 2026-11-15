//! The Morph home and profile directories.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{Error, Result};

/// The Morph home: `explicit` if given, else `MORPH_HOME`, else `~/.morph`.
/// Does not create it.
pub fn resolve(explicit: Option<&Path>) -> Result<PathBuf> {
    Ok(morph_identity::Paths::resolve(explicit.map(Path::to_path_buf)).map_err(Error::failed)?.home)
}

/// The directory of profile `profile_id`, which must be in the catalog.
pub fn profile_dir(home: &Path, profile_id: &str) -> Result<PathBuf> {
    morph_core::profiles::require(home, profile_id).map_err(Error::failed)?;
    Ok(morph_core::profiles::profile_dir(home, profile_id))
}

/// Milliseconds since the Unix epoch, for timestamps the services write.
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_home_wins_and_is_not_created() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        assert_eq!(resolve(Some(&home)).unwrap(), home);
        assert!(!home.exists());
    }

    #[test]
    fn only_catalogued_profiles_have_a_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::load_catalog(&home).unwrap();
        assert!(profile_dir(&home, "default").unwrap().ends_with("profiles/default"));
        assert!(profile_dir(&home, "nope").is_err());
        assert!(now_ms() > 0);
    }
}
