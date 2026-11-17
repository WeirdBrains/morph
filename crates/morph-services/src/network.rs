//! Apply Morph's install-level network settings to a weirdnet-net node.
//!
//! `weirdnet-net` 0.5.0 (the version Morph is allowed to depend on) has no
//! `DiscoveryMode` or `PeerHint` on `NodeConfig`. Those landed on
//! weirdnet-net main in 89e16a4 and will be usable once m3talsmith publishes
//! **0.6.0** (safer: new public fields on `NodeConfig` and `Status`; 0.5.1
//! is enough if callers keep using `..Default::default()`). Until then this
//! adapter:
//! - records the intended discovery flag but cannot pass it through
//!   (`TODO` below);
//! - dials manually configured peers with `Node::connect` after the node
//!   starts (the same addresses `NodeConfig::peers` will take later).
//!
//! Adding an address does not grant trust; the roster and `NodeCert` do.
use morph_core::network::{NetworkSettings, PeerEntry};
use weirdnet_net::{Multiaddr, Node, NodeConfig, PeerId};

/// Overlay Morph's network preferences onto `cfg`.
///
/// TODO(weirdnet-net >= 0.6.0): set `cfg.discovery` from
/// `NetworkSettings::discovery_enabled` (`DiscoveryMode::Enabled` /
/// `Disabled`) and `cfg.peers` from `PeerHint::parse`. Then drop
/// [`dial_manual_peers`].
pub fn apply_to_config(cfg: NodeConfig, net: &NetworkSettings) -> NodeConfig {
    let _discovery = net.discovery_enabled();
    let _peers = net.peers();
    // Cannot set NodeConfig::{discovery, peers} until Morph's weirdnet-net
    // dependency is bumped to the published crate that has those fields.
    cfg
}

/// Dial each manual peer with the 0.5.0 API (`Node::connect`). Failures
/// are ignored (the peer may be offline); the caller can try again.
///
/// TODO(weirdnet-net >= 0.6.0): delete this once `NodeConfig::peers` is
/// set in [`apply_to_config`] and the node dials them itself.
pub fn dial_manual_peers<Q, R>(node: &Node<Q, R>, peers: &[PeerEntry])
where
    Q: weirdnet_net::Message,
    R: weirdnet_net::Message,
{
    for peer in peers {
        let Ok(id) = peer.peer_id.parse::<PeerId>() else { continue };
        let addrs: Vec<Multiaddr> = peer.addrs.iter().filter_map(|a| a.parse().ok()).collect();
        if addrs.is_empty() {
            continue;
        }
        let _ = node.connect(id, &addrs);
    }
}
