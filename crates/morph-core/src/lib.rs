//! Morph core wiring: version, a tick event, and the page-bridge message contract.
//!
//! Domain crates stay testable without Flutter. The `flutter_rust_bridge` surface lives
//! in `morph-bridge`.
#![forbid(unsafe_code)]

pub mod page_bridge;
pub mod settings;
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
