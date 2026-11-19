//! A Morph home for headless checks of the profile lock
//! (scripts/xvfb-check.py --only profile-lock): the `default` profile with no
//! identity, and a profile `Locked` with an identity sealed under the
//! password in `MORPH_SEED_PASSWORD`, made current. Prints `ready`.
//!
//! Refuses to run without an explicit `MORPH_HOME` (never the real
//! `~/.morph`) and without `MORPH_KEYSTORE=off` (never the real keystore).
//!
//! `MORPH_HOME=/tmp/x MORPH_KEYSTORE=off MORPH_SEED_PASSWORD=… cargo run -p morph-services --example seed-locked-profile`
use std::path::PathBuf;

use morph_services::SecretString;

fn main() {
    let home = std::env::var_os("MORPH_HOME").map(PathBuf::from).filter(|h| !h.as_os_str().is_empty()).expect("set MORPH_HOME to a test home");
    assert_eq!(std::env::var("MORPH_KEYSTORE").as_deref(), Ok("off"), "set MORPH_KEYSTORE=off");
    let password = SecretString::from(std::env::var("MORPH_SEED_PASSWORD").expect("set MORPH_SEED_PASSWORD"));
    morph_core::profiles::load_catalog(&home).expect("catalog");
    let locked = morph_core::profiles::create(&home, "Locked").expect("profile").id;
    morph_services::identity::create(&home, &locked, &password).expect("identity");
    morph_services::profile_lock::lock(&home, &locked);
    println!("profile {locked}");
    println!("ready");
}
