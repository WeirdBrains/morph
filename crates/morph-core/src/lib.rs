//! Morph core wiring: version, a tick event, the page-bridge message contract, and the
//! shell rules every front end shares (tabs, the address bar, profiles, the library).
//!
//! Domain crates stay testable without Flutter. The `flutter_rust_bridge` surface lives
//! in `morph-bridge`.
#![forbid(unsafe_code)]

pub mod address;
pub mod app_call;
pub mod bookmarks;
pub mod download_auth;
pub mod download_sessions;
pub mod history_sync;
pub mod library;
pub mod library_db;
pub mod network;
pub mod omnibox;
pub mod page_bridge;
pub mod profiles;
pub mod scheme;
pub mod search;
pub mod session;
pub mod settings;
pub mod settings_sync;
pub mod suggest;
pub mod sync;
pub mod tabs;

pub use morph_identity::{self, IdentityStatus, MasterBackup, MemoryIdentityStore, Paths, UserIdentity};

/// The `morph-core` crate version, returned to Dart for the M1 wiring check.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_semver() {
        let v = super::version();
        assert!(v.split('.').count() >= 3, "{v}");
        assert_eq!(v, "0.1.0");
    }
}
