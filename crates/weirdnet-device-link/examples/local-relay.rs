//! A Weirdnet relay on this machine only, for headless checks of device
//! linking and relay status (scripts/xvfb-check.py): a circuit relay with no
//! caps and a names server, over TCP and QUIC on this machine's own IPv4
//! address (the one it would reach the internet from; 127.0.0.1 without
//! one), on ports the system picks. Not loopback, because link codes leave
//! loopback addresses out, circuits through a relay there included. Those addresses are also its external ones: a relay puts
//! its external addresses in every reservation, and a client turns down a
//! reservation that has none. Prints `relay <multiaddr>` for each address (ending in its
//! `/p2p/<peer>`), then `ready`, and runs until stdin closes.
//!
//! `cargo run -p weirdnet-device-link --example local-relay`
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use weirdnet_device_link::{LINK_PROFILE, LinkReply, LinkRequest};
use weirdnet_net::names::Names;
use weirdnet_net::node::{Handler, Node, NodeConfig, RelayLimits};
use weirdnet_net::{Keypair, PeerId};

struct Relay;

impl Handler<LinkRequest, LinkReply> for Relay {
    fn request(&self, _from: &PeerId, _req: LinkRequest) -> LinkReply {
        LinkReply::Refused { reason: "this is a relay".into() }
    }
}

/// This machine's IPv4 address towards the internet (a UDP socket
/// "connected" to a documentation address: nothing is sent).
fn own_ip() -> std::net::IpAddr {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr()));
    match probe {
        Ok(a) if !a.ip().is_unspecified() => a.ip(),
        _ => std::net::Ipv4Addr::LOCALHOST.into(),
    }
}

/// A port the system has free right now on `ip` (for TCP, or UDP).
fn free_port(ip: std::net::IpAddr, udp: bool) -> u16 {
    if udp {
        std::net::UdpSocket::bind((ip, 0)).and_then(|s| s.local_addr()).expect("a free UDP port").port()
    } else {
        std::net::TcpListener::bind((ip, 0)).and_then(|s| s.local_addr()).expect("a free TCP port").port()
    }
}

fn main() {
    let ip = own_ip();
    let listen: Vec<weirdnet_net::Multiaddr> = vec![
        format!("/ip4/{ip}/tcp/{}", free_port(ip, false)).parse().unwrap(),
        format!("/ip4/{ip}/udp/{}/quic-v1", free_port(ip, true)).parse().unwrap(),
    ];
    let cfg = NodeConfig {
        external: listen.clone(),
        listen,
        relay_server: true,
        relay_limits: RelayLimits::unlimited(),
        names_server: Some(Names::memory()),
        label: "morph-local-relay".into(),
        ..Default::default()
    };
    let node = Node::start(&LINK_PROFILE, Keypair::generate_ed25519(), cfg, Arc::new(Relay)).expect("the relay starts");
    let end = Instant::now() + Duration::from_secs(10);
    let mut addrs = vec![];
    while Instant::now() < end {
        addrs = node.status().listening;
        if addrs.iter().any(|a| a.contains("/tcp/")) && addrs.iter().any(|a| a.contains("/quic-v1")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut out = std::io::stdout().lock();
    for a in &addrs {
        let _ = writeln!(out, "relay {a}/p2p/{}", node.peer_id());
    }
    let _ = writeln!(out, "ready");
    let _ = out.flush();
    drop(out);
    for line in std::io::stdin().lock().lines() {
        if line.is_err() {
            break;
        }
    }
    node.stop();
}
