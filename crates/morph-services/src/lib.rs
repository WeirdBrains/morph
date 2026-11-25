//! Logic the desktop (GPUI) and mobile (Flutter) shells share, with no UI
//! toolkit: where the Morph home is, profiles and what deleting one cleans
//! up, opening and unlocking a profile's encrypted library with the key kept
//! in the system keystore, profile identities (create, import, restore) and
//! moving one between devices over a QR link.
//!
//! `morph-bridge` exposes these to Dart with its own string-typed views;
//! `morph-desktop` calls them directly. Secrets stay in Rust: passwords come
//! in as [`SecretString`], keys never leave.

pub mod app_calls;
pub mod companion_sync;
pub mod device_calls;
pub mod device_link;
pub mod home;
pub mod identity;
pub mod infer_serving;
pub mod library;
pub mod network;
pub mod node_router;
pub mod password_saver;
pub mod profile_io;
pub mod profile_lock;
pub mod profiles;
pub mod relays;
pub mod routed_models;
pub mod sync;
pub mod ymir_pool;

pub use morph_identity::profile_identity::{SecretString, Zeroizing};

/// Why a service call failed. The text is for the user and never holds a secret.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The profile has no Weirdnet identity yet (onboarding not done).
    #[error("this profile has no identity yet")]
    NoIdentity,
    /// A device-link session id that ended or never existed.
    #[error("this link has ended; start again")]
    LinkGone,
    /// The profile password was wrong (the count goes toward the backoff).
    #[error("that password is not right")]
    WrongPassword,
    /// Too many wrong passwords: wait before the next try.
    #[error("too many wrong passwords; try again in {} s", (wait_ms + 999) / 1000)]
    TooSoon { wait_ms: i64 },
    /// The profile is locked: it has an identity and has not been unlocked
    /// with its password in this process.
    #[error("profile locked: unlock it with its password first")]
    Locked,
    /// A failure from the layer below, as its message.
    #[error("{0}")]
    Failed(String),
}

impl Error {
    pub(crate) fn failed(e: impl std::fmt::Display) -> Self {
        Self::Failed(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
