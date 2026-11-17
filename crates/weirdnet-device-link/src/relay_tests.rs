//! Links and relay checks through a relay of their own (in memory), with no
//! direct address between the two devices, and relays whose first address
//! goes nowhere: the second one has to carry the connection.
use std::sync::Arc;
use std::time::{Duration, Instant};

use weirdnet_keys::Key32;
use weirdnet_net::names::Names;
use weirdnet_net::node::{Handler, Node, NodeConfig, RelayLimits};
use weirdnet_net::{Keypair, Multiaddr, PeerId};

use crate::wire::{LinkReply, LinkRequest};
use crate::{HostNode, HostStatus, InviteHost, LINK_PROFILE, Pairing, RelayProbe, answer_invite, decode_link, relay_states};

const SCHEME: &str = "weirdnet-morph://";

struct Nothing;

impl Handler<LinkRequest, LinkReply> for Nothing {
    fn request(&self, _from: &PeerId, _req: LinkRequest) -> LinkReply {
        LinkReply::refused("a relay")
    }
}

/// A relay (with a names server, as the public one runs) and its address.
fn relay() -> (Node<LinkRequest, LinkReply>, Multiaddr) {
    let cfg = NodeConfig {
        listen: vec!["/memory/0".parse().unwrap()],
        memory: true,
        relay_server: true,
        relay_limits: RelayLimits::unlimited(),
        names_server: Some(Names::memory()),
        ..Default::default()
    };
    let node = Node::start(&LINK_PROFILE, Keypair::generate_ed25519(), cfg, Arc::new(Nothing)).unwrap();
    let addr = node.status().listening[0].parse::<Multiaddr>().unwrap().with(multiaddr::Protocol::P2p(node.peer_id()));
    (node, addr)
}

/// An address of the same relay that nothing listens on.
fn dead(relay: &Multiaddr) -> Multiaddr {
    let peer = weirdnet_net::addr::addr_peer(relay).unwrap();
    format!("/memory/{}/p2p/{peer}", 9_000_000_000u64 + u64::from(std::process::id())).parse().unwrap()
}

/// No listen address of its own: reachable only through `relays`.
fn behind_nat(relays: Vec<Multiaddr>) -> NodeConfig {
    NodeConfig { memory: true, relays, ..Default::default() }
}

fn wait(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn a_link_goes_through_the_relay_alone() {
    let (_relay, at) = relay();
    let new_device = HostNode::start(SCHEME, Keypair::generate_ed25519(), behind_nat(vec![at.clone()]), "desk").unwrap();
    let offer = decode_link(SCHEME, new_device.link()).unwrap();
    assert!(!offer.offer.addrs.is_empty() && offer.offer.addrs.iter().all(|a| a.contains("/p2p-circuit")), "{:?}", offer.offer.addrs);
    let mut phone = Pairing::start(SCHEME, new_device.link(), behind_nat(vec![at])).unwrap();
    let HostStatus::Paired { sas } = new_device.status() else { panic!("not paired") };
    assert_eq!(sas, phone.sas());
    new_device.confirm();
    let seed = Key32::random().unwrap();
    phone.send(&seed, "Work", "{}").unwrap();
    assert_eq!(new_device.take().unwrap().seed, seed.0.to_vec());
}

#[test]
fn a_relay_is_reached_over_its_second_address_when_the_first_goes_nowhere() {
    let (_relay, at) = relay();
    let relays = vec![dead(&at), at.clone()];
    let new_device = HostNode::start(SCHEME, Keypair::generate_ed25519(), behind_nat(relays.clone()), "desk").unwrap();
    let states = relay_states(&relays, &[new_device.net_status()], &[]);
    assert_eq!(states.len(), 1, "one relay, two addresses");
    assert!(states[0].connected && states[0].reserved, "{states:?}");
    // The phone reaches the relay over whichever of its addresses works.
    let mut phone = Pairing::start(SCHEME, new_device.link(), behind_nat(relays)).unwrap();
    new_device.confirm();
    phone.send(&Key32::random().unwrap(), "Work", "{}").unwrap();
    assert!(new_device.take().is_ok());
}

#[test]
fn an_invite_is_answered_through_the_relay_alone() {
    let (_relay, at) = relay();
    let relays = vec![dead(&at), at];
    let invite = InviteHost::start(SCHEME, behind_nat(relays.clone())).unwrap();
    let new_device = HostNode::start(SCHEME, Keypair::generate_ed25519(), behind_nat(relays.clone()), "desk").unwrap();
    answer_invite(SCHEME, invite.link(), new_device.link(), behind_nat(relays)).unwrap();
    assert_eq!(invite.take_offer().as_deref(), Some(new_device.link()));
}

#[test]
fn a_relay_check_reserves_and_measures_the_round_trip() {
    let (_relay, at) = relay();
    let relays = vec![dead(&at), at];
    let probe = RelayProbe::start(behind_nat(relays.clone())).unwrap();
    assert!(wait(|| relay_states(&relays, &[probe.net_status()], &[])[0].reserved), "{:?}", probe.net_status());
    assert!(wait(|| !probe.round_trips().is_empty()), "no round trip measured");
    let s = &relay_states(&relays, &[probe.net_status()], &probe.round_trips())[0];
    assert!(s.rtt_ms.is_some_and(|ms| ms < 5_000), "{s:?}");
}

#[test]
fn without_a_names_server_there_is_no_round_trip_but_the_rest_works() {
    let cfg = NodeConfig {
        listen: vec!["/memory/0".parse().unwrap()],
        memory: true,
        relay_server: true,
        relay_limits: RelayLimits::unlimited(),
        ..Default::default()
    };
    let relay = Node::<LinkRequest, LinkReply>::start(&LINK_PROFILE, Keypair::generate_ed25519(), cfg, Arc::new(Nothing)).unwrap();
    let at = relay.status().listening[0].parse::<Multiaddr>().unwrap().with(multiaddr::Protocol::P2p(relay.peer_id()));
    let relays = vec![at];
    let probe = RelayProbe::start(behind_nat(relays.clone())).unwrap();
    assert!(wait(|| relay_states(&relays, &[probe.net_status()], &[])[0].reserved));
    std::thread::sleep(Duration::from_millis(500));
    let s = &relay_states(&relays, &[probe.net_status()], &probe.round_trips())[0];
    assert!(s.connected && s.rtt_ms.is_none(), "{s:?}");
}

/// The public relay (weirdnet-relay 0.1.0 when this was written) takes a 0.4
/// node's reservation and answers its names lookups:
/// `cargo test -p weirdnet-device-link -- --ignored`.
#[test]
#[ignore = "needs the public relay"]
fn the_public_relay_reserves_and_answers_lookups() {
    let relays: Vec<Multiaddr> = crate::KNOWN_RELAYS.iter().map(|a| a.parse().unwrap()).collect();
    let probe = RelayProbe::start(behind_nat_on_the_internet(relays.clone())).unwrap();
    assert!(wait(|| relay_states(&relays, &[probe.net_status()], &[])[0].reserved), "{:?}", probe.net_status());
    assert!(wait(|| !probe.round_trips().is_empty()), "no round trip");
}

fn behind_nat_on_the_internet(relays: Vec<Multiaddr>) -> NodeConfig {
    NodeConfig { relays, ..Default::default() }
}
