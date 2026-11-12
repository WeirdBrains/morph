//! Login over a QR code: move a profile's identity to another device of the
//! same person (`weirdnet-device-link/v1`, docs/identity-recovery.md §4).
//!
//! The **new device** ([`NewDevice`]) has no identity in the profile yet. It
//! listens on a fresh node key and shows a `weirdnet-morph://link/…` QR. The
//! **existing device** ([`AddDevice`]) unlocks its seed with the profile
//! password, scans or pastes the link and pairs. Both screens then show the
//! same six digits; only after the user confirms them on both does the seed
//! travel, sealed, to the new device, which seals it under its own new
//! password and keeps the node key and the node certificate the user key
//! signed for it:
//!
//! - `identity/node.key`: the node's Ed25519 key, sealed like `user.key`;
//! - `identity/node.cert.json`: its `NodeCert` (public).
use std::fs;
use std::path::{Path, PathBuf};

use weirdnet_device_link::{HostNode, HostStatus, Pairing};
use weirdnet_keys::{Key32, SecretString, password_seal};
use weirdnet_net::Keypair;
use weirdnet_net::node::NodeConfig;

use crate::profile_identity::{self, ProfileIdentity, Source, identity_dir};
use crate::{Error, Result};

/// The link scheme Morph shows and accepts.
pub const SCHEME: &str = "weirdnet-morph://";
const NODE_AAD: &[u8] = b"morph/identity/node-key/v1";

/// Where the link listens and dials: TCP and QUIC on every interface, on a
/// port the system picks. The offer carries the addresses, so no discovery
/// is needed on one network. Relays are not configured yet (LAN or direct
/// only).
pub fn net_config() -> NodeConfig {
    NodeConfig { listen: NodeConfig::default_listen(0), label: "morph-link".into(), ..Default::default() }
}

/// What the new device's screen should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// Showing the QR; nobody has scanned it yet.
    Waiting,
    /// Compare this code with the other device.
    Paired {
        sas: String,
    },
    /// The identity arrived; confirm the code here (if not yet) and choose a password.
    Received {
        sas: String,
    },
    Done,
    Cancelled,
    Expired,
    Failed(String),
}

impl From<HostStatus> for LinkState {
    fn from(s: HostStatus) -> Self {
        match s {
            HostStatus::Waiting => Self::Waiting,
            HostStatus::Paired { sas } => Self::Paired { sas },
            HostStatus::Received { sas } => Self::Received { sas },
            HostStatus::Done => Self::Done,
            HostStatus::Cancelled => Self::Cancelled,
            HostStatus::Expired => Self::Expired,
            HostStatus::Failed(why) => Self::Failed(why.to_string()),
        }
    }
}

fn link_error(e: weirdnet_device_link::Error) -> Error {
    Error::Message(e.to_string())
}

/// The new device's side of one link. Stops listening when dropped.
pub struct NewDevice {
    host: HostNode,
    node_key: Keypair,
}

impl std::fmt::Debug for NewDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewDevice").field("peer", &self.host.peer_id()).finish_non_exhaustive()
    }
}

impl NewDevice {
    /// Listens and makes a fresh offer; `device_name` is what the other screen
    /// shows ("Link Morph on laptop?").
    pub fn start(device_name: &str, cfg: NodeConfig) -> Result<Self> {
        let node_key = Keypair::generate_ed25519();
        let host = HostNode::start(SCHEME, node_key.clone(), cfg, device_name).map_err(link_error)?;
        Ok(Self { host, node_key })
    }

    /// The text the QR encodes (also shown for pasting).
    pub fn link(&self) -> &str {
        self.host.link()
    }

    pub fn expires_ms(&self) -> i64 {
        self.host.expires_ms()
    }

    pub fn state(&self) -> LinkState {
        self.host.status().into()
    }

    /// The user tapped "Codes match" here.
    pub fn confirm(&self) {
        self.host.confirm();
    }

    pub fn cancel(&self) {
        self.host.cancel();
    }

    /// Takes the identity that arrived (after [`NewDevice::confirm`]) into
    /// `profile_dir`, sealed under the new `password`. Spends the offer.
    pub fn finish(&self, profile_dir: &Path, password: &SecretString, now_ms: i64) -> Result<ProfileIdentity> {
        // Check everything that could still fail before taking: the offer is
        // single use, so a refused password must not spend it.
        profile_identity::check_password(password)?;
        if profile_identity::load(profile_dir)?.is_some() {
            return Err(Error::Message("this profile already has an identity".into()));
        }
        let transfer = self.host.take().map_err(link_error)?;
        let seed = Key32::from_slice(&transfer.seed)?;
        let identity = profile_identity::store(profile_dir, &seed, password, Source::Linked, now_ms)?;
        // The link already checked the cert names this node and the seed's user;
        // keep the node key and cert next to the user key.
        let node_key = self.node_key.to_protobuf_encoding().map_err(|e| Error::Message(format!("node key: {e}")))?;
        let node_key = zeroize::Zeroizing::new(node_key);
        let sealed = password_seal(password, &node_key, NODE_AAD)?;
        let sealed = serde_json::to_vec_pretty(&sealed).map_err(|e| Error::Message(e.to_string()))?;
        let cert = serde_json::to_vec_pretty(&transfer.node_cert).map_err(|e| Error::Message(e.to_string()))?;
        profile_identity::write_private(&node_key_path(profile_dir), &sealed)?;
        profile_identity::write_private(&node_cert_path(profile_dir), &cert)?;
        Ok(identity)
    }
}

pub fn node_key_path(profile_dir: &Path) -> PathBuf {
    identity_dir(profile_dir).join("node.key")
}

pub fn node_cert_path(profile_dir: &Path) -> PathBuf {
    identity_dir(profile_dir).join("node.cert.json")
}

/// The existing device's side: paired with a scanned link, holding the
/// unlocked seed until the user confirms the code (or drops this).
pub struct AddDevice {
    pairing: Pairing,
    seed: Key32,
    profile_name: String,
}

impl std::fmt::Debug for AddDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddDevice").field("peer", &self.pairing.peer()).finish_non_exhaustive()
    }
}

impl AddDevice {
    /// Checks the profile password first (nothing is dialled with a wrong
    /// one), then reads `link`, dials the new device and pairs.
    pub fn start(profile_dir: &Path, profile_name: &str, password: &SecretString, link: &str, cfg: NodeConfig) -> Result<Self> {
        let seed = profile_identity::open_seed(profile_dir, password)?;
        let pairing = Pairing::start(SCHEME, link.trim(), cfg).map_err(link_error)?;
        Ok(Self { pairing, seed, profile_name: profile_name.to_string() })
    }

    /// The code to compare with the new device's screen.
    pub fn sas(&self) -> &str {
        self.pairing.sas()
    }

    /// What the new device calls itself.
    pub fn device(&self) -> &str {
        self.pairing.device()
    }

    /// The codes match: send the identity, once.
    pub fn send(&mut self) -> Result<()> {
        self.pairing.send(&self.seed, &self.profile_name, "{}").map_err(link_error)
    }

    /// The codes differ, or the user gave up.
    pub fn cancel(&self) -> Result<()> {
        self.pairing.cancel().map_err(link_error)
    }
}

/// Whether `profile_dir` holds a node key that came with a link.
pub fn has_linked_node(profile_dir: &Path) -> bool {
    fs::metadata(node_key_path(profile_dir)).is_ok() && fs::metadata(node_cert_path(profile_dir)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_identity::{create, load, unlock};
    use std::time::{Duration, Instant};

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn memory() -> NodeConfig {
        NodeConfig { listen: vec!["/memory/0".parse().unwrap()], memory: true, ..Default::default() }
    }

    fn wait_for(new: &NewDevice, want: fn(&LinkState) -> bool) -> LinkState {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = new.state();
            if want(&s) || Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_profile_moves_to_a_new_device_over_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let fresh = tmp.path().join("fresh");
        let made = create(&old, &pw("old password"), 1).unwrap();

        let new = NewDevice::start("Morph on laptop", memory()).unwrap();
        assert!(new.link().starts_with("weirdnet-morph://link/"));
        assert_eq!(new.state(), LinkState::Waiting);

        let mut add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        assert_eq!(add.device(), "Morph on laptop");
        let LinkState::Paired { sas } = wait_for(&new, |s| matches!(s, LinkState::Paired { .. })) else { panic!("not paired") };
        assert_eq!(sas, add.sas());
        assert_eq!(sas.len(), 6);

        new.confirm();
        add.send().unwrap();
        assert!(matches!(new.state(), LinkState::Received { .. }));
        let linked = new.finish(&fresh, &pw("new password"), 2).unwrap();

        assert_eq!(linked.user_id, made.identity.user_id);
        assert_eq!(linked.source, Source::Linked);
        assert_eq!(load(&fresh).unwrap().unwrap(), linked);
        assert_eq!(unlock(&fresh, &pw("new password")).unwrap().user_id, made.identity.user_id);
        assert!(unlock(&fresh, &pw("old password")).is_err());
        assert!(has_linked_node(&fresh));
        let cert: weirdnet_net::user::NodeCert = serde_json::from_slice(&fs::read(node_cert_path(&fresh)).unwrap()).unwrap();
        assert_eq!(cert.user_id, made.identity.user_id);
        assert_eq!(new.state(), LinkState::Done);
        // Spent: a second take fails, and nothing secret is in the clear.
        assert!(new.finish(&tmp.path().join("again"), &pw("new password"), 3).is_err());
        for entry in fs::read_dir(identity_dir(&fresh)).unwrap() {
            let body = fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(!body.contains(made.words.as_str()));
            assert!(!body.contains("new password"));
        }
    }

    #[test]
    fn a_short_password_does_not_spend_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let fresh = tmp.path().join("fresh");
        create(&old, &pw("old password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        let mut add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        wait_for(&new, |s| matches!(s, LinkState::Paired { .. }));
        new.confirm();
        add.send().unwrap();
        assert!(new.finish(&fresh, &pw("short"), 2).is_err());
        assert!(matches!(new.state(), LinkState::Received { .. }));
        assert_eq!(new.finish(&fresh, &pw("long enough"), 2).unwrap().source, Source::Linked);
    }

    #[test]
    fn a_wrong_password_never_dials() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        create(&old, &pw("old password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        assert!(AddDevice::start(&old, "Work", &pw("wrong password"), new.link(), memory()).is_err());
        assert_eq!(new.state(), LinkState::Waiting);
    }

    #[test]
    fn nothing_is_taken_without_confirming_here() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        create(&old, &pw("old password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        let mut add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        wait_for(&new, |s| matches!(s, LinkState::Paired { .. }));
        add.send().unwrap();
        let fresh = tmp.path().join("fresh");
        assert!(new.finish(&fresh, &pw("new password"), 2).is_err());
        assert!(load(&fresh).unwrap().is_none());
    }

    #[test]
    fn a_cancel_from_the_existing_device_ends_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        create(&old, &pw("old password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        let add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        add.cancel().unwrap();
        assert_eq!(wait_for(&new, |s| *s == LinkState::Cancelled), LinkState::Cancelled);
    }

    #[test]
    fn a_profile_with_an_identity_is_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let fresh = tmp.path().join("fresh");
        create(&old, &pw("old password"), 1).unwrap();
        let other = create(&fresh, &pw("fresh password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        let mut add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        wait_for(&new, |s| matches!(s, LinkState::Paired { .. }));
        new.confirm();
        add.send().unwrap();
        assert!(new.finish(&fresh, &pw("new password"), 2).is_err());
        assert_eq!(load(&fresh).unwrap().unwrap().user_id, other.identity.user_id);
    }
}
