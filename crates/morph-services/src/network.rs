//! Apply Morph's install-level network settings to a weirdnet-net node.
//!
//! `NodeConfig::discovery` and `NodeConfig::peers` come from
//! [`NetworkSettings`]. Adding an address does not grant trust; the roster
//! and `NodeCert` do.
use morph_core::network::NetworkSettings;
use weirdnet_net::{DiscoveryMode, NodeConfig, PeerHint};

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
