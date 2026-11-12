//! Version facts for the Flutter shell.

/// The `morph-core` version. A Dart→Rust call of this is part of the M1 wiring check.
#[flutter_rust_bridge::frb(sync)]
pub fn core_version() -> String {
    morph_core::version().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn core_version_matches_crate() {
        assert_eq!(super::core_version(), morph_core::version());
    }
}
