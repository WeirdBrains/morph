//! Which relays a link node is using, read from the node's own status: never
//! guessed. A relay is one peer id with one or more configured addresses
//! (`/…/tcp/…/p2p/RELAY`, `/…/quic-v1/p2p/RELAY`, `/…/tcp/443/tls/ws/p2p/RELAY`).
//!
//! weirdnet-net 0.4 reports the peers a node is connected to, the relays
//! holding a reservation for it and its listen addresses, among them
//! `/…/p2p/RELAY/p2p-circuit` while a reservation is held. It does not
//! report ping round trips, so [`RelayState::rtt_ms`] is what a
//! [`RelayProbe`] measures itself: the best of a few names lookups sent to
//! the relay over the connection it holds (a request and its answer, so one
//! round trip plus the relay's lookup).
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use weirdnet_net::addr::addr_peer;
use weirdnet_net::names::{NamesRequest, NamesResponse};
use weirdnet_net::node::{Handler, NodeConfig, Status};
use weirdnet_net::{Keypair, Multiaddr, PeerId};

use crate::reach::{self, LinkNode};
use crate::wire::{LinkReply, LinkRequest};
use crate::{LINK_PROFILE, Result};

/// One configured relay as the running nodes see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayState {
    /// The relay's peer id.
    pub peer: String,
    /// Its configured addresses, in configuration order.
    pub addrs: Vec<String>,
    /// A node holds a connection to it.
    pub connected: bool,
    /// A node holds a circuit reservation on it (it is connected, and listens
    /// through it or the relay accepted a reservation on this connection).
    pub reserved: bool,
    /// The round trip of a request to it, in milliseconds, when a relay
    /// check measured one (see the module docs).
    pub rtt_ms: Option<u64>,
}

/// The configured relays (grouped by peer, addresses without a `/p2p/` peer
/// id dropped) as `statuses` (one per running node) report them, with the
/// round trips (`(peer, ms)`) a [`RelayProbe`] measured. With no statuses
/// everything reads as not connected.
pub fn relay_states(relays: &[Multiaddr], statuses: &[Status], round_trips: &[(String, u64)]) -> Vec<RelayState> {
    let mut out: Vec<RelayState> = vec![];
    for addr in relays {
        let Some(peer) = addr_peer(addr) else { continue };
        let peer = peer.to_string();
        match out.iter_mut().find(|r| r.peer == peer) {
            Some(r) => r.addrs.push(addr.to_string()),
            None => out.push(RelayState { peer, addrs: vec![addr.to_string()], connected: false, reserved: false, rtt_ms: None }),
        }
    }
    for r in &mut out {
        let circuit = format!("/p2p/{}/p2p-circuit", r.peer);
        for s in statuses {
            let connected = s.connected.contains(&r.peer);
            // Only trust a reservation while the connection that carried it is up.
            let reserved = connected && (s.listening.iter().any(|a| a.contains(&circuit)) || s.reservations.contains(&r.peer));
            r.connected |= connected;
            r.reserved |= reserved;
        }
        if r.connected {
            r.rtt_ms = round_trips.iter().filter(|(p, _)| *p == r.peer).map(|(_, ms)| *ms).min();
        }
    }
    out
}

struct NoRequests;

impl Handler<LinkRequest, LinkReply> for NoRequests {
    fn request(&self, _from: &PeerId, _req: LinkRequest) -> LinkReply {
        LinkReply::refused("this device is only checking its relays")
    }
}

/// How long a probe waits for a relay connection before it gives up
/// measuring that relay's round trip.
const RTT_WAIT: Duration = Duration::from_secs(20);
/// Lookups per measurement; the best one counts.
const RTT_TRIES: usize = 3;
/// The user a round-trip lookup asks for: no one (a well-formed answer of
/// "not found", or a refusal, is all it waits for).
const RTT_LOOKUP: &str = "morph-relay-check";

/// A short-lived node on a throwaway key that only reaches the configured
/// relays (and reserves a slot on them, as a link would), so a settings page
/// can show real relay state while no link runs, and measures each relay's
/// round trip once connected. Stops when dropped.
pub struct RelayProbe {
    node: LinkNode<LinkRequest, LinkReply>,
    rtts: Arc<Mutex<HashMap<PeerId, u64>>>,
}

impl RelayProbe {
    pub fn start(cfg: NodeConfig) -> Result<Self> {
        let node = reach::start(&LINK_PROFILE, Keypair::generate_ed25519(), cfg, Arc::new(NoRequests))?;
        let rtts: Arc<Mutex<HashMap<PeerId, u64>>> = Arc::default();
        for (relay, _) in &node.relays {
            let (n, relay, rtts) = (node.node.clone(), *relay, rtts.clone());
            let _ = std::thread::Builder::new().name("weirdnet-relay-rtt".into()).spawn(move || {
                let end = Instant::now() + RTT_WAIT;
                while n.running() && !n.status().connected.contains(&relay.to_string()) {
                    if Instant::now() > end {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let mut best: Option<u64> = None;
                for _ in 0..RTT_TRIES {
                    let t = Instant::now();
                    let answered = matches!(
                        n.names_request(relay, &[], NamesRequest::Discover { user_id: RTT_LOOKUP.into() }),
                        Ok(NamesResponse::Found(_) | NamesResponse::Refused(_))
                    );
                    if !answered {
                        break;
                    }
                    let ms = t.elapsed().as_millis() as u64;
                    best = Some(best.map_or(ms, |b| b.min(ms)));
                }
                if let Some(ms) = best {
                    rtts.lock().unwrap_or_else(|p| p.into_inner()).insert(relay, ms);
                }
            });
        }
        Ok(Self { node, rtts })
    }

    pub fn net_status(&self) -> Status {
        self.node.node.status()
    }

    /// Round trips measured so far, `(relay peer id, ms)`.
    pub fn round_trips(&self) -> Vec<(String, u64)> {
        self.rtts.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|(p, ms)| (p.to_string(), *ms)).collect()
    }
}

impl Drop for RelayProbe {
    fn drop(&mut self) {
        self.node.node.halt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELAY: &str = "12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB";

    fn relays() -> Vec<Multiaddr> {
        [
            format!("/dns4/relay.example/tcp/4747/p2p/{RELAY}"),
            format!("/dns4/relay.example/udp/4747/quic-v1/p2p/{RELAY}"),
            "/ip4/192.0.2.1/tcp/1".into(), // no peer id: not a relay address
        ]
        .iter()
        .map(|a| a.parse().unwrap())
        .collect()
    }

    #[test]
    fn addresses_of_one_relay_group_and_nothing_running_reads_not_connected() {
        let states = relay_states(&relays(), &[], &[]);
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].peer, RELAY);
        assert_eq!(states[0].addrs.len(), 2);
        assert!(states[0].addrs[1].contains("quic-v1"));
        assert!(!states[0].connected && !states[0].reserved && states[0].rtt_ms.is_none());
        assert!(relay_states(&[], &[Status::default()], &[]).is_empty());
    }

    #[test]
    fn connected_and_reserved_come_from_the_status() {
        let connected = Status { connected: vec![RELAY.into()], ..Default::default() };
        let s = &relay_states(&relays(), std::slice::from_ref(&connected), &[])[0];
        assert!(s.connected && !s.reserved, "connected, no reservation yet");

        let listening = Status { listening: vec![format!("/dns4/relay.example/tcp/4747/p2p/{RELAY}/p2p-circuit")], ..connected.clone() };
        assert!(relay_states(&relays(), &[listening], &[])[0].reserved, "listening through the relay");

        let accepted = Status { reservations: vec![RELAY.into()], ..connected };
        assert!(relay_states(&relays(), &[accepted], &[])[0].reserved, "reservation accepted");

        // An old reservation on a dropped connection is not a reservation.
        let stale = Status { reservations: vec![RELAY.into()], ..Default::default() };
        let s = &relay_states(&relays(), &[stale], &[])[0];
        assert!(!s.connected && !s.reserved);
    }

    #[test]
    fn any_running_node_counts() {
        let idle = Status::default();
        let busy = Status { connected: vec![RELAY.into()], reservations: vec![RELAY.into()], ..Default::default() };
        let s = &relay_states(&relays(), &[idle, busy], &[])[0];
        assert!(s.connected && s.reserved);
        let other = Status { connected: vec!["12D3KooWSomeoneElse".into()], ..Default::default() };
        assert!(!relay_states(&relays(), &[other], &[])[0].connected);
    }
}
