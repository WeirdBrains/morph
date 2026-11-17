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
pub mod device_link;
pub mod home;
pub mod identity;
pub mod library;
pub mod network;
pub mod profiles;
pub mod relays;

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
