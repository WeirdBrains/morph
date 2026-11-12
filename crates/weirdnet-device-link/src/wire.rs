//! Messages on `/weirdnet/link/1` and the sealed transfer.
use serde::{Deserialize, Serialize};
use weirdnet_keys::Sealed;
use weirdnet_net::user::NodeCert;
use zeroize::Zeroize;

/// From the existing device to the new one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LinkRequest {
    /// Start pairing: the existing device's ephemeral X25519 key.
    Hello {
        #[serde(with = "crate::bytes")]
        nonce: Vec<u8>,
        #[serde(with = "crate::bytes")]
        eph: Vec<u8>,
    },
    /// The sealed [`Transfer`], after the user confirmed the SAS.
    Transfer {
        #[serde(with = "crate::bytes")]
        nonce: Vec<u8>,
        sealed: Sealed,
    },
    /// The user said the codes do not match (or gave up).
    Cancel {
        #[serde(with = "crate::bytes")]
        nonce: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkReply {
    Ok,
    Refused { reason: String },
}

impl LinkReply {
    pub(crate) fn refused(reason: &str) -> Self {
        LinkReply::Refused { reason: reason.to_string() }
    }
}

/// What moves to the new device. The seed is wiped on drop and never shown by
/// `Debug`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub v: u8,
    /// The 32-byte user-key seed (what the recovery words encode).
    #[serde(with = "crate::bytes")]
    pub seed: Vec<u8>,
    pub user_id: String,
    pub profile_name: String,
    /// App settings worth carrying over (JSON; `{}` when none).
    pub settings: String,
    /// The new device's node certificate, signed by the user key.
    pub node_cert: NodeCert,
    /// Data types the user chose to sync (none yet).
    pub sync: Vec<String>,
}

impl Drop for Transfer {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

impl std::fmt::Debug for Transfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transfer").field("user_id", &self.user_id).field("profile_name", &self.profile_name).finish_non_exhaustive()
    }
}
