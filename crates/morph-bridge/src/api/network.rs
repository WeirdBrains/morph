//! Settings → Relays: Weirdnet discovery and manually added peers, for every
//! profile (`network.json` in the Morph home). The logic lives in
//! `morph-core::network` and `morph-services::network`; changes restart the
//! running nodes. Turning discovery off keeps sharing, relays and sync with
//! added peers.
use crate::home::resolve as resolve_home;
use morph_core::network::{DiscoveryOverride, NetworkSettings};
use morph_services::network as net;

/// One manually added peer: a node id and the addresses that reach it.
pub struct NetworkPeerView {
    pub peer_id: String,
    pub addrs: Vec<String>,
}

/// The install's network settings as Settings shows them.
pub struct NetworkView {
    /// Whether discovery runs (the preference, unless an admin locked it off).
    pub discovery_enabled: bool,
    /// An admin override forces discovery off; the switch cannot turn it on.
    pub discovery_locked: bool,
    /// `env` or `policy` when locked, else `None`.
    pub lock_source: Option<String>,
    /// What the screen says under the switch (why it is locked, if it is).
    pub discovery_help: String,
    pub peers: Vec<NetworkPeerView>,
}

impl From<&NetworkSettings> for NetworkView {
    fn from(s: &NetworkSettings) -> Self {
        Self {
            discovery_enabled: s.discovery_enabled(),
            discovery_locked: s.discovery_locked(),
            lock_source: s.discovery_override.map(|o| {
                match o {
                    DiscoveryOverride::Env => "env",
                    DiscoveryOverride::Policy => "policy",
                }
                .into()
            }),
            discovery_help: net::discovery_help(s).into(),
            peers: s.peers().iter().map(|p| NetworkPeerView { peer_id: p.peer_id.clone(), addrs: p.addrs.clone() }).collect(),
        }
    }
}

/// The network settings now (defaults when nothing was saved).
pub fn load_network(home: Option<String>) -> Result<NetworkView, String> {
    let home = resolve_home(home)?;
    net::load(&home).map(|s| (&s).into()).map_err(|e| e.to_string())
}

/// Turn discovery on or off and restart running nodes. Locked: unchanged.
pub fn set_discovery(home: Option<String>, enabled: bool) -> Result<NetworkView, String> {
    let home = resolve_home(home)?;
    net::set_discovery(&home, enabled).map(|s| (&s).into()).map_err(|e| e.to_string())
}

/// Add a peer by multiaddr (ending in `/p2p/<id>`) or `<id> <addr>…`. The
/// error says what is wrong with the text.
pub fn add_network_peer(home: Option<String>, spec: String) -> Result<NetworkView, String> {
    let home = resolve_home(home)?;
    net::add_peer(&home, &spec).map(|s| (&s).into()).map_err(|e| e.to_string())
}

pub fn remove_network_peer(home: Option<String>, peer_id: String) -> Result<NetworkView, String> {
    let home = resolve_home(home)?;
    net::remove_peer(&home, &peer_id).map(|s| (&s).into()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: &str = "12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB";

    #[test]
    fn the_view_round_trips_discovery_and_peers() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h").display().to_string();
        let first = load_network(Some(home.clone())).unwrap();
        if first.discovery_locked {
            return;
        }
        assert!(first.discovery_enabled && first.lock_source.is_none() && first.peers.is_empty());
        let off = set_discovery(Some(home.clone()), false).unwrap();
        assert!(!off.discovery_enabled && !off.discovery_locked);

        assert!(add_network_peer(Some(home.clone()), "/ip4/127.0.0.1/tcp/1".into()).err().unwrap().contains("/p2p/"));
        let addr = format!("/ip4/192.168.1.5/tcp/4747/p2p/{PEER}");
        let added = add_network_peer(Some(home.clone()), addr.clone()).unwrap();
        assert_eq!(added.peers.len(), 1);
        assert_eq!(added.peers[0].peer_id, PEER);
        assert_eq!(added.peers[0].addrs, vec![addr]);
        assert!(!added.discovery_enabled, "adding a peer keeps discovery off");

        let again = load_network(Some(home.clone())).unwrap();
        assert_eq!(again.peers.len(), 1);
        assert!(remove_network_peer(Some(home.clone()), PEER.into()).unwrap().peers.is_empty());

        std::fs::write(std::path::Path::new(&home).join("network.policy.json"), "{\"discovery\": false}\n").unwrap();
        let locked = set_discovery(Some(home), true).unwrap();
        assert!(locked.discovery_locked && !locked.discovery_enabled);
        assert_eq!(locked.lock_source.as_deref(), Some("policy"));
        assert!(locked.discovery_help.starts_with("Turned off by a policy file"));
    }
}
