//! How a link node reaches its relays, and the limits it runs with.
//!
//! weirdnet-net 0.4 reserves a relay slot over the first configured address
//! of each relay only (its relay client dials that one address). A relay
//! usually has several: TCP, QUIC and, where it serves it, WebSocket over TLS
//! on port 443 for networks that only let HTTPS out. So a link node starts
//! with no relays in its [`NodeConfig`]; [`start`] connects to each relay
//! over all of its addresses at once (libp2p dials them side by side and
//! keeps the first that answers) and only then asks for the reservation,
//! which rides on that connection. If no address answers, the reservation
//! is still asked for, and weirdnet-net keeps retrying it as before.
//!
//! The circuits a node then listens on (and puts in its code) are the ones
//! the relay's reservation names, built from the addresses the relay
//! announces, whichever address carried the connection, as before. A code
//! only says which relay to go through; the other device reaches that relay
//! over whatever address works on its own network
//! ([`LinkNode::reach_relays_of`]).
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use weirdnet_net::addr::{addr_peer, circuit_relay};
use weirdnet_net::node::{Handler, Message, Node, NodeConfig};
use weirdnet_net::profile::Profile;
use weirdnet_net::{Keypair, Multiaddr, PeerId};

use crate::Result;

/// The largest request a link node takes. Link requests are a hello, a
/// sealed transfer (a 32-byte seed, a profile name, a node certificate and a
/// little settings JSON) or a cancel: a few KiB. Bulk data (identity sync)
/// belongs on a stream, not here. weirdnet-net's default is 8 MiB.
pub const REQUEST_MAX: u64 = 256 * 1024;
/// The largest response: always `Ok` or a short refusal.
pub const RESPONSE_MAX: u64 = 16 * 1024;
/// How long a request waits for its answer. The other side answers at once
/// (the code check happens between requests, not inside one), so this only
/// has to cover a slow relayed connection. weirdnet-net's default is 60 s.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a dialing node waits to reach the relay of a circuit before it
/// sends anyway (weirdnet-net then waits on its own dial, as it did before).
const RELAY_CONNECT_WAIT: Duration = Duration::from_secs(10);

/// `cfg` with the link limits wherever it leaves weirdnet-net's defaults.
pub fn with_link_limits(cfg: NodeConfig) -> NodeConfig {
    NodeConfig {
        request_max: if cfg.request_max == 0 { REQUEST_MAX } else { cfg.request_max },
        response_max: if cfg.response_max == 0 { RESPONSE_MAX } else { cfg.response_max },
        request_timeout: if cfg.request_timeout.is_zero() { REQUEST_TIMEOUT } else { cfg.request_timeout },
        ..cfg
    }
}

/// Relay addresses grouped by relay, both in configuration order. Addresses
/// without a `/p2p/` peer id are not relay addresses and are dropped, as are
/// repeats.
pub fn relay_groups(relays: &[Multiaddr]) -> Vec<(PeerId, Vec<Multiaddr>)> {
    let mut out: Vec<(PeerId, Vec<Multiaddr>)> = vec![];
    for a in relays {
        let Some(peer) = addr_peer(a) else { continue };
        match out.iter_mut().find(|(p, _)| *p == peer) {
            Some((_, addrs)) if addrs.contains(a) => {}
            Some((_, addrs)) => addrs.push(a.clone()),
            None => out.push((peer, vec![a.clone()])),
        }
    }
    out
}

/// A running link node and the relays it was configured with.
pub(crate) struct LinkNode<Q: Message, R: Message> {
    pub node: Arc<Node<Q, R>>,
    pub relays: Vec<(PeerId, Vec<Multiaddr>)>,
}

/// Starts a node with the link limits and reaches every relay in `cfg` over
/// all of its addresses, then reserves a slot on it (in the background:
/// this returns once the node listens).
pub(crate) fn start<Q: Message, R: Message>(
    profile: &'static Profile,
    key: Keypair,
    cfg: NodeConfig,
    handler: Arc<dyn Handler<Q, R>>,
) -> Result<LinkNode<Q, R>> {
    let mut cfg = with_link_limits(cfg);
    let relays = relay_groups(&std::mem::take(&mut cfg.relays));
    let node = Arc::new(Node::start(profile, key, cfg, handler)?);
    for (peer, addrs) in &relays {
        let (n, peer, addrs) = (node.clone(), *peer, addrs.clone());
        let first = addrs[0].clone();
        let spawned = std::thread::Builder::new().name("weirdnet-link-relay".into()).spawn(move || {
            let _ = n.connect(peer, &addrs);
            let _ = n.add_relay(addrs[0].clone());
        });
        if spawned.is_err() {
            let _ = node.add_relay(first);
        }
    }
    Ok(LinkNode { node, relays })
}

impl<Q: Message, R: Message> LinkNode<Q, R> {
    /// Before dialing `targets`: connect to the relay of each circuit among
    /// them over every address this node knows for it (its own
    /// configuration's and the circuit's), waiting at most
    /// [`RELAY_CONNECT_WAIT`] in all. Without this weirdnet-net would dial
    /// the relay over the circuit's one address, which may be the one
    /// transport this network blocks.
    pub fn reach_relays_of(&self, targets: &[Multiaddr]) {
        let mut seen: Vec<PeerId> = vec![];
        let (tx, rx) = mpsc::channel::<()>();
        for (relay, raddr) in targets.iter().filter_map(circuit_relay) {
            if seen.contains(&relay) || relay == self.node.peer_id() {
                continue;
            }
            seen.push(relay);
            let mut addrs = self.relays.iter().find(|(p, _)| *p == relay).map(|(_, a)| a.clone()).unwrap_or_default();
            if !addrs.contains(&raddr) {
                addrs.push(raddr);
            }
            let (n, tx) = (self.node.clone(), tx.clone());
            let _ = std::thread::Builder::new().name("weirdnet-link-relay".into()).spawn(move || {
                let _ = n.connect(relay, &addrs);
                let _ = tx.send(());
            });
        }
        drop(tx);
        let end = Instant::now() + RELAY_CONNECT_WAIT;
        for _ in 0..seen.len() {
            if rx.recv_timeout(end.saturating_duration_since(Instant::now())).is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELAY: &str = "12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB";

    fn m(a: &str) -> Multiaddr {
        a.parse().unwrap()
    }

    #[test]
    fn relays_group_by_peer_in_order_without_repeats() {
        let other = Keypair::generate_ed25519().public().to_peer_id();
        let addrs = [
            m(&format!("/dns4/relay.example/tcp/4747/p2p/{RELAY}")),
            m(&format!("/dns4/other.example/tcp/4747/p2p/{other}")),
            m(&format!("/dns4/relay.example/udp/4747/quic-v1/p2p/{RELAY}")),
            m(&format!("/dns4/relay.example/tcp/443/tls/ws/p2p/{RELAY}")),
            m(&format!("/dns4/relay.example/tcp/4747/p2p/{RELAY}")),
            m("/ip4/192.0.2.1/tcp/1"),
        ];
        let g = relay_groups(&addrs);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].0.to_string(), RELAY);
        assert_eq!(g[0].1, vec![addrs[0].clone(), addrs[2].clone(), addrs[3].clone()]);
        assert_eq!(g[1].1, vec![addrs[1].clone()]);
    }

    #[test]
    fn link_limits_fill_only_what_is_left_at_the_default() {
        let c = with_link_limits(NodeConfig::default());
        assert_eq!((c.request_max, c.response_max, c.request_timeout), (REQUEST_MAX, RESPONSE_MAX, REQUEST_TIMEOUT));
        let c = with_link_limits(NodeConfig { request_max: 7, request_timeout: Duration::from_secs(3), ..Default::default() });
        assert_eq!((c.request_max, c.response_max, c.request_timeout), (7, RESPONSE_MAX, Duration::from_secs(3)));
    }
}
