//! Identity facts that are safe to show in the UI. Secrets never cross this boundary.

use morph_identity::{UserIdentity, identity_status};

/// Public identity status for a Morph home. Never creates the directory.
pub struct IdentityStatusView {
    pub home: String,
    pub home_exists: bool,
    pub profile: String,
}

/// Resolve the data dir (`home`, else `MORPH_HOME`, else `~/.morph`) without creating it.
pub fn identity_dir_info(home: Option<String>) -> Result<IdentityStatusView, String> {
    let path = home.filter(|h| !h.is_empty());
    let status = identity_status(path.as_deref().map(std::path::Path::new)).map_err(|e| e.to_string())?;
    Ok(IdentityStatusView { home: status.home, home_exists: status.home_exists, profile: status.profile })
}

/// An ephemeral user identity for the M1 wiring check. The key never leaves Rust.
pub struct EphemeralUser {
    pub user_id: String,
    pub fingerprint: String,
}

/// Generate a new user key in memory and return only its public id and fingerprint.
pub fn new_ephemeral_user() -> Result<EphemeralUser, String> {
    let user = UserIdentity::generate().map_err(|e| e.to_string())?;
    let fingerprint = user.fingerprint();
    Ok(EphemeralUser { user_id: user.user_id, fingerprint })
}

#[cfg(test)]
mod tests {
    #[test]
    fn dir_info_does_not_create() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let info = super::identity_dir_info(Some(home.clone())).unwrap();
        assert!(!info.home_exists);
        assert!(!std::path::Path::new(&home).exists());
        assert_eq!(info.profile, "weirdnet/keys/v1");
    }

    #[test]
    fn ephemeral_user_has_a_public_id() {
        let a = super::new_ephemeral_user().unwrap();
        let b = super::new_ephemeral_user().unwrap();
        assert_eq!(a.user_id.len(), 32);
        assert_ne!(a.user_id, b.user_id);
    }
}
