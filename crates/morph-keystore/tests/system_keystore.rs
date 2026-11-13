//! Against the real OS keystore (Keychain, Credential Manager, Secret Service,
//! Android Keystore), under a throwaway service name so no real profile's entry
//! is touched. Ignored by default: it needs a usable keystore and, on Linux, a
//! session bus. Run with:
//!
//! ```text
//! MORPH_KEYSTORE_SERVICE=org.weirdbrains.morph.test cargo test -p morph-keystore --test system_keystore -- --ignored
//! ```
//!
//! Every entry it saves is forgotten again, and the test checks that.

use morph_identity::profile_identity::LibraryKey;

#[test]
#[ignore = "uses the real OS keystore"]
fn the_system_keystore_keeps_and_forgets_keys() {
    let service = morph_keystore::service();
    assert!(service.starts_with("org.weirdbrains.morph.test"), "set MORPH_KEYSTORE_SERVICE=org.weirdbrains.morph.test (got {service})");
    let home = tempfile::tempdir().unwrap();
    let a = home.path().join("a");
    let b = home.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let key_a = LibraryKey::from_bytes([0xa5; 32]);
    let key_b = LibraryKey::from_bytes([0x5a; 32]);

    let result = std::panic::catch_unwind(|| {
        assert!(morph_keystore::load(&a, "alice").unwrap().is_none());
        morph_keystore::save(&a, "alice", &key_a).unwrap();
        morph_keystore::save(&b, "bob", &key_b).unwrap();
        assert_eq!(morph_keystore::load(&a, "alice").unwrap().unwrap().expose(), key_a.expose());
        assert_eq!(morph_keystore::load(&b, "bob").unwrap().unwrap().expose(), key_b.expose());

        // Saving again replaces the entry.
        morph_keystore::save(&a, "alice", &key_b).unwrap();
        assert_eq!(morph_keystore::load(&a, "alice").unwrap().unwrap().expose(), key_b.expose());

        // A key kept for another identity is never handed out, and is removed.
        assert!(morph_keystore::load(&b, "mallory").unwrap().is_none());
        assert!(morph_keystore::load(&b, "bob").unwrap().is_none());

        morph_keystore::forget(&a).unwrap();
        assert!(morph_keystore::load(&a, "alice").unwrap().is_none());
        morph_keystore::forget(&a).unwrap();
    });
    // Leave nothing behind, even if an assertion failed.
    let _ = morph_keystore::forget(&a);
    let _ = morph_keystore::forget(&b);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
