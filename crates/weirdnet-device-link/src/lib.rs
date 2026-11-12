//! `weirdnet-device-link/v1`: move a Weirdnet user identity to another device of
//! the same person without typing its recovery words.
//!
//! The **new device** (no identity yet) shows an [`Offer`] as a QR code or link:
//! its node key, a fresh X25519 key, its addresses and a 120 s expiry, signed by
//! the node key. The **existing device** (holds the identity) scans it, connects
//! over Weirdnet on `/weirdnet/link/1`, and both derive a key and a 6-digit
//! short authentication string (SAS) from the X25519 agreement and a transcript
//! hash. Only after the user confirms the SAS matches on the existing device does
//! it send the [`Transfer`] (seed, profile name, a node certificate for the new
//! device), sealed with that key. The offer is single use and dies after three
//! pairing attempts, a cancel, a transfer or its expiry.
//!
//! Nothing secret is in the offer, and nothing here logs secrets.
mod bytes;
pub mod net;
pub mod offer;
pub mod session;
pub mod wire;

pub use net::{HostNode, Pairing};
pub use offer::{Offer, VerifiedOffer, decode_link, encode_link, offer_addrs};
pub use session::{Host, HostStatus, Sender};
pub use wire::{LinkReply, LinkRequest, Transfer};

use weirdnet_net::profile::{Profile, WEIRDNET};

/// Protocol version in offers and transfers.
pub const VERSION: u8 = 1;
/// How long an offer lives.
pub const OFFER_TTL_MS: i64 = 120_000;
/// Pairing attempts an offer accepts before it dies.
pub const MAX_ATTEMPTS: u32 = 3;

/// The Weirdnet network with this protocol's own stream id, so other nodes
/// ignore it (and it ignores them).
pub static LINK_PROFILE: Profile = Profile { protocol: "/weirdnet/link/1", thread: "weirdnet-link", ..WEIRDNET };

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("this code expired; show a new one on the other device")]
    Expired,
    #[error("the other device refused: {0}")]
    Refused(String),
    #[error("network: {0}")]
    Net(String),
    #[error("keys: {0}")]
    Keys(String),
}

impl From<weirdnet_keys::KeysError> for Error {
    fn from(e: weirdnet_keys::KeysError) -> Self {
        Error::Keys(e.to_string())
    }
}

impl From<weirdnet_net::NetError> for Error {
    fn from(e: weirdnet_net::NetError) -> Self {
        Error::Net(e.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
