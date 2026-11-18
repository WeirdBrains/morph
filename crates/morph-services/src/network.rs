//! Apply Morph's install-level network settings to a weirdnet-net node.
//!
//! `NodeConfig::discovery` and `NodeConfig::peers` come from
//! [`NetworkSettings`]. Adding an address does not grant trust; the roster
//! and `NodeCert` do.
use std::path::Path;

use morph_core::network::{self as netprefs, DiscoveryOverride, NetworkSettings};
use weirdnet_net::{DiscoveryMode, NodeConfig, PeerHint};

use crate::{Error, Result};

/// What Settings says about the discovery switch, for both shells.
pub fn discovery_help(net: &NetworkSettings) -> &'static str {
    match net.discovery_override {
        Some(DiscoveryOverride::Env) => "Turned off by MORPH_DISABLE_WEIRDNET_DISCOVERY. Sharing and sync with relays and added peers still work.",
        Some(DiscoveryOverride::Policy) => "Turned off by a policy file. Sharing and sync with relays and added peers still work.",
        None => {
            "Find peers on the local network and the DHT. Turn this off on corporate networks. Sharing and sync with relays and added peers still work."
        }
    }
}

/// The install's network settings (defaults when `network.json` is missing).
pub fn load(home: &Path) -> Result<NetworkSettings> {
    netprefs::load(home).map_err(Error::failed)
}

/// Store the discovery preference and restart running nodes with it. A
/// locked-off setting stays off; the returned settings say so.
pub fn set_discovery(home: &Path, on: bool) -> Result<NetworkSettings> {
    let net = netprefs::set_discovery(home, on).map_err(Error::failed)?;
    apply_running(home);
    Ok(net)
}

/// Validate `spec` (a multiaddr ending in `/p2p/<id>`, or a peer id and
/// addresses) the way weirdnet-net will dial it, store it, and restart
/// running nodes. The error text is for the user.
pub fn add_peer(home: &Path, spec: &str) -> Result<NetworkSettings> {
    let entry = netprefs::parse_peer(spec).map_err(Error::failed)?;
    PeerHint::parse(&entry.peer_id, &entry.addrs).map_err(Error::failed)?;
    let net = netprefs::add_peer(home, spec).map_err(Error::failed)?;
    apply_running(home);
    Ok(net)
}

/// Forget a manually added peer and restart running nodes.
pub fn remove_peer(home: &Path, peer_id: &str) -> Result<NetworkSettings> {
    let net = netprefs::remove_peer(home, peer_id).map_err(Error::failed)?;
    apply_running(home);
    Ok(net)
}

/// Make running nodes follow `network.json`: the app-call node starts again
/// on its next call, and sync nodes under `home` restart on a background
/// thread (starting a node must not hold up a UI thread).
pub fn apply_running(home: &Path) {
    crate::app_calls::shared().stop();
    let home = home.to_path_buf();
    let _ = std::thread::Builder::new().name("morph-net-apply".into()).spawn(move || crate::sync::restart_all(&home));
}

/// Overlay Morph's network preferences onto `cfg`.
///
/// Sets `discovery` from [`NetworkSettings::discovery_enabled`] and `peers`
/// from each stored entry via [`PeerHint::parse`]. Invalid entries are
/// skipped (they should already have been rejected when added). The node
/// dials those peers itself, including when discovery is off.
pub fn apply_to_config(cfg: NodeConfig, net: &NetworkSettings) -> NodeConfig {
    NodeConfig {
        discovery: DiscoveryMode::from_enabled(net.discovery_enabled()),
        peers: net.peers().iter().filter_map(|p| PeerHint::parse(&p.peer_id, &p.addrs).ok()).collect(),
        ..cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_core::network::{NetworkPrefs, PeerEntry};

    const PEER: &str = "12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB";

    #[test]
    fn apply_to_config_sets_discovery_and_peers() {
        let addr = format!("/ip4/127.0.0.1/tcp/4747/p2p/{PEER}");
        let net = NetworkSettings {
            prefs: NetworkPrefs { discovery: false, peers: vec![PeerEntry { peer_id: PEER.into(), addrs: vec![addr.clone()] }] },
            discovery_override: None,
        };
        let cfg = apply_to_config(NodeConfig { mdns: true, ..Default::default() }, &net);
        assert!(!cfg.discovery.is_enabled());
        assert_eq!(cfg.peers.len(), 1);
        assert_eq!(cfg.peers[0].peer_id, PEER);
        assert_eq!(cfg.peers[0].addrs, vec![addr]);
    }

    #[test]
    fn add_peer_validates_like_weirdnet_and_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        assert!(add_peer(home, "").is_err());
        assert!(add_peer(home, "/ip4/127.0.0.1/tcp/1").unwrap_err().to_string().contains("/p2p/"));
        // Plausible to morph-core's shape check, but not base58.
        let fake = "12D3KooW00000000000000000000000000000000000000000000";
        assert!(add_peer(home, &format!("/ip4/127.0.0.1/tcp/1/p2p/{fake}")).is_err());
        assert!(load(home).unwrap().peers().is_empty(), "nothing invalid is stored");

        let addr = format!("/ip4/127.0.0.1/tcp/4747/p2p/{PEER}");
        let net = add_peer(home, &addr).unwrap();
        assert_eq!(net.peers().len(), 1);
        assert_eq!(net.peers()[0].addrs, vec![addr]);
        assert!(remove_peer(home, PEER).unwrap().peers().is_empty());
    }

    #[test]
    fn set_discovery_stores_the_preference_unless_locked() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        if load(home).unwrap().discovery_locked() {
            return;
        }
        let off = set_discovery(home, false).unwrap();
        assert!(!off.discovery_enabled() && !off.discovery_locked());
        assert!(discovery_help(&off).contains("corporate"));
        assert!(set_discovery(home, true).unwrap().discovery_enabled());

        std::fs::write(netprefs::policy_path(home), "{\"discovery\": false}\n").unwrap();
        let locked = set_discovery(home, true).unwrap();
        assert!(!locked.discovery_enabled() && locked.discovery_locked());
        assert!(discovery_help(&locked).starts_with("Turned off by"));
    }

    #[test]
    fn apply_to_config_skips_a_bad_peer() {
        let net = NetworkSettings {
            prefs: NetworkPrefs { discovery: true, peers: vec![PeerEntry { peer_id: "nope".into(), addrs: vec!["/ip4/127.0.0.1/tcp/1".into()] }] },
            ..Default::default()
        };
        let cfg = apply_to_config(NodeConfig::default(), &net);
        assert!(cfg.discovery.is_enabled());
        assert!(cfg.peers.is_empty());
    }
}
