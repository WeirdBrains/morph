//! The relays Morph links devices through, and whether it is connected to
//! them right now (Settings → Relays, and a status line on the link screens).
//!
//! Morph runs no network node of its own outside device linking, so there is
//! no standing relay connection to report. The state comes from the link
//! nodes that are running (a shown offer or invite, a pairing) and from a
//! short check the user starts ([`Links::relay_check_start`]), which runs the
//! same node a link would for [`CHECK_FOR`] and then stops. With none of
//! those running the report says so ([`RelayReport::nodes`] is 0) instead of
//! guessing.
//!
//! A relay's display name comes from its `/.well-known/weirdnet-relay.json`,
//! fetched once in the background (short timeout, cached); until it arrives,
//! or if it never does, the relay is named by its host.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use morph_identity::device_link::{NetStatus, RelayProbe, RelayState, link_relays, net_config, relay_states};

use crate::device_link::Links;
use crate::{Error, Result};

/// How long a relay check keeps its node up.
pub const CHECK_FOR: Duration = Duration::from_secs(30);
/// Fetching a relay's info gives up after this.
const INFO_TIMEOUT: Duration = Duration::from_secs(4);
/// A fetched name is kept this long; a failed fetch is retried after
/// [`INFO_RETRY`].
const INFO_TTL: Duration = Duration::from_secs(6 * 3600);
const INFO_RETRY: Duration = Duration::from_secs(5 * 60);

/// One relay for a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayView {
    pub peer: String,
    /// Configured addresses (multiaddrs).
    pub addrs: Vec<String>,
    /// The DNS name in its addresses, if any.
    pub host: Option<String>,
    /// The name its well-known info gives, once fetched.
    pub name: Option<String>,
    pub connected: bool,
    pub reserved: bool,
    pub rtt_ms: Option<u64>,
}

/// Where a relay stands, for a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayStatus {
    /// No node is running: Morph is not connected to anything.
    NoNode,
    /// A node runs but holds no connection to it (yet).
    NotConnected,
    /// Connected, no circuit reservation (yet).
    Connected,
    /// Connected with a circuit reservation: other devices can reach this one through it.
    Reserved,
}

impl RelayStatus {
    /// A stable word for shells and tests.
    pub fn as_str(self) -> &'static str {
        match self {
            RelayStatus::NoNode => "no-node",
            RelayStatus::NotConnected => "not-connected",
            RelayStatus::Connected => "connected",
            RelayStatus::Reserved => "reserved",
        }
    }

    /// What a screen says about it.
    pub fn label(self) -> &'static str {
        match self {
            RelayStatus::NoNode => "Not connected",
            RelayStatus::NotConnected => "Connecting…",
            RelayStatus::Connected => "Connected, reserving a slot…",
            RelayStatus::Reserved => "Connected, slot reserved",
        }
    }
}

impl RelayView {
    /// Its name, else its host, else its short peer id.
    pub fn title(&self) -> String {
        if let Some(n) = &self.name {
            return n.clone();
        }
        if let Some(h) = &self.host {
            return h.clone();
        }
        let tail = self.peer.len().saturating_sub(8);
        format!("relay …{}", &self.peer[tail..])
    }
}

/// The configured relays and what the running nodes say about them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayReport {
    /// Running nodes the state came from (0: nothing runs).
    pub nodes: usize,
    /// A relay check is running.
    pub checking: bool,
    pub relays: Vec<RelayView>,
}

impl RelayReport {
    /// The report for `relays` (configured) given the statuses of the nodes
    /// that run; `names` looks up a relay's display name by host and peer.
    pub fn build(
        relays: &[morph_identity::device_link::Multiaddr],
        statuses: &[NetStatus],
        checking: bool,
        names: impl Fn(&str, &str) -> Option<String>,
    ) -> Self {
        let relays = relay_states(relays, statuses)
            .into_iter()
            .map(|RelayState { peer, addrs, connected, reserved, rtt_ms }| {
                let host = addrs.iter().find_map(|a| dns_host(a));
                let name = host.as_deref().and_then(|h| names(h, &peer));
                RelayView { peer, addrs, host, name, connected, reserved, rtt_ms }
            })
            .collect();
        RelayReport { nodes: statuses.len(), checking, relays }
    }

    /// Where `relay` stands.
    pub fn status(&self, relay: &RelayView) -> RelayStatus {
        if self.nodes == 0 {
            RelayStatus::NoNode
        } else if relay.reserved {
            RelayStatus::Reserved
        } else if relay.connected {
            RelayStatus::Connected
        } else {
            RelayStatus::NotConnected
        }
    }

    /// One line for a link screen or the top of the settings section.
    pub fn summary(&self) -> String {
        if self.relays.is_empty() {
            return "No relays set: devices link only on the same network.".into();
        }
        if self.nodes == 0 {
            return "Not connected to a relay. Morph connects only while linking a device or checking.".into();
        }
        let line = |r: &RelayView| {
            let mut s = format!("{}: {}", r.title(), self.status(r).label().to_lowercase());
            if let Some(ms) = r.rtt_ms {
                s.push_str(&format!(" ({ms} ms)"));
            }
            s
        };
        match self.relays.as_slice() {
            [one] => format!("Relay {}", line(one)),
            many => {
                let reserved = many.iter().filter(|r| r.reserved).count();
                let connected = many.iter().filter(|r| r.connected).count();
                format!("Relays: {connected} of {} connected, {reserved} with a slot reserved", many.len())
            }
        }
    }
}

/// The host of a `/dns4/HOST/…`, `/dns6/…` or `/dns/…` address.
pub fn dns_host(addr: &str) -> Option<String> {
    let mut parts = addr.split('/');
    while let Some(p) = parts.next() {
        if matches!(p, "dns" | "dns4" | "dns6") {
            return parts.next().filter(|h| !h.is_empty()).map(str::to_string);
        }
    }
    None
}

/// The display name in a relay's well-known info, if it names `peer`.
pub fn info_name(json: &str, peer: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("peer_id").and_then(|p| p.as_str()) != Some(peer) {
        return None;
    }
    let name = v.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(name.chars().filter(|c| !c.is_control()).take(64).collect())
}

struct Cached {
    at: Instant,
    name: Option<String>,
    pending: bool,
}

fn info_cache() -> &'static Mutex<HashMap<String, Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The cached display name of the relay at `host` (`peer`); never blocks. A
/// missing or stale entry starts one background fetch; the name shows on
/// the next report. `MORPH_RELAY_INFO=0` turns fetching off.
pub fn relay_name(host: &str, peer: &str) -> Option<String> {
    let key = format!("{host} {peer}");
    let mut cache = info_cache().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(c) = cache.get(&key) {
        let ttl = if c.name.is_some() { INFO_TTL } else { INFO_RETRY };
        if c.pending || c.at.elapsed() < ttl {
            return c.name.clone();
        }
    }
    if std::env::var_os("MORPH_RELAY_INFO").is_some_and(|v| v == "0") {
        return None;
    }
    let previous = cache.get(&key).and_then(|c| c.name.clone());
    cache.insert(key.clone(), Cached { at: Instant::now(), name: previous.clone(), pending: true });
    let (host_key, peer_key) = (host, peer);
    let (host, peer) = (host.to_string(), peer.to_string());
    let spawned = std::thread::Builder::new().name("morph-relay-info".into()).spawn(move || {
        let name = fetch_name(&host, &peer);
        let mut cache = info_cache().lock().unwrap_or_else(|p| p.into_inner());
        let keep = name.or_else(|| cache.get(&key).and_then(|c| c.name.clone()));
        cache.insert(key, Cached { at: Instant::now(), name: keep, pending: false });
    });
    if spawned.is_err()
        && let Some(c) = cache.get_mut(&format!("{host_key} {peer_key}"))
    {
        c.pending = false;
    }
    previous
}

fn fetch_name(host: &str, peer: &str) -> Option<String> {
    if !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return None;
    }
    let url = format!("https://{host}/.well-known/weirdnet-relay.json");
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(INFO_TIMEOUT)).build().into();
    let mut resp = agent.get(&url).call().ok()?;
    let body = resp.body_mut().with_config().limit(64 * 1024).read_to_string().ok()?;
    info_name(&body, peer)
}

impl Links {
    /// Start a relay check: a node like a link's that only reaches the
    /// relays, for [`CHECK_FOR`]. Blocks while the node starts (call it off
    /// the UI thread). A check that runs already is kept.
    pub fn relay_check_start(&self) -> Result<()> {
        if self.check_running() {
            return Ok(());
        }
        let probe = RelayProbe::start(net_config()).map_err(Error::failed)?;
        *self.probe_slot() = Some((probe, Instant::now() + CHECK_FOR));
        Ok(())
    }

    /// Stop a running relay check.
    pub fn relay_check_stop(&self) {
        self.probe_slot().take();
    }

    fn check_running(&self) -> bool {
        let mut slot = self.probe_slot();
        if slot.as_ref().is_some_and(|(_, until)| Instant::now() > *until) {
            *slot = None;
        }
        slot.is_some()
    }

    /// The configured relays as the running link nodes and relay check see
    /// them now. Cheap; never touches the network itself.
    pub fn relays(&self) -> RelayReport {
        let checking = self.check_running();
        let mut statuses = self.net_statuses();
        if let Some((probe, _)) = self.probe_slot().as_ref() {
            statuses.push(probe.net_status());
        }
        RelayReport::build(&link_relays(), &statuses, checking, relay_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELAY: &str = "12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB";

    fn configured() -> Vec<morph_identity::device_link::Multiaddr> {
        morph_identity::device_link::PUBLIC_RELAYS.iter().map(|a| a.parse().unwrap()).collect()
    }

    fn no_names(_: &str, _: &str) -> Option<String> {
        None
    }

    #[test]
    fn nothing_running_says_so_and_never_claims_a_connection() {
        let r = RelayReport::build(&configured(), &[], false, no_names);
        assert_eq!(r.nodes, 0);
        assert_eq!(r.relays.len(), 1, "TCP and QUIC are one relay");
        let relay = &r.relays[0];
        assert_eq!(relay.peer, RELAY);
        assert_eq!(relay.addrs.len(), 2);
        assert_eq!(relay.host.as_deref(), Some("relay.net.weirdbrains.com"));
        assert_eq!(r.status(relay), RelayStatus::NoNode);
        assert!(r.summary().starts_with("Not connected"), "{}", r.summary());
    }

    #[test]
    fn states_map_from_the_node_status() {
        let idle = NetStatus::default();
        let r = RelayReport::build(&configured(), std::slice::from_ref(&idle), false, no_names);
        assert_eq!(r.status(&r.relays[0]), RelayStatus::NotConnected);
        assert_eq!(r.summary(), "Relay relay.net.weirdbrains.com: connecting…");

        let connected = NetStatus { connected: vec![RELAY.into()], ..Default::default() };
        let r = RelayReport::build(&configured(), std::slice::from_ref(&connected), true, no_names);
        assert_eq!(r.status(&r.relays[0]), RelayStatus::Connected);
        assert!(r.checking);

        let reserved = NetStatus { listening: vec![format!("{}/p2p-circuit", configured()[0])], ..connected };
        let r = RelayReport::build(&configured(), &[idle, reserved], false, |h, p| {
            (h == "relay.net.weirdbrains.com" && p == RELAY).then(|| "WeirdBrains public relay".into())
        });
        assert_eq!(r.nodes, 2);
        assert_eq!(r.status(&r.relays[0]), RelayStatus::Reserved);
        assert_eq!(r.summary(), "Relay WeirdBrains public relay: connected, slot reserved");
        assert_eq!(RelayStatus::Reserved.as_str(), "reserved");
    }

    #[test]
    fn no_relays_and_several_relays_read_plainly() {
        let r = RelayReport::build(&[], &[NetStatus::default()], false, no_names);
        assert!(r.summary().starts_with("No relays set"));
        let other = "12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp";
        let two: Vec<_> = [configured()[0].to_string(), format!("/ip4/192.0.2.7/tcp/4001/p2p/{other}")].iter().map(|a| a.parse().unwrap()).collect();
        let s = NetStatus { connected: vec![RELAY.into()], reservations: vec![RELAY.into()], ..Default::default() };
        let r = RelayReport::build(&two, &[s], false, no_names);
        assert_eq!(r.summary(), "Relays: 1 of 2 connected, 1 with a slot reserved");
        assert_eq!(r.relays[1].host, None);
        assert!(r.relays[1].title().starts_with("relay …"));
    }

    #[test]
    fn the_info_name_must_name_the_same_relay() {
        let json = format!(r#"{{"v":1,"name":"  WeirdBrains public relay ","peer_id":"{RELAY}","limits":{{}}}}"#);
        assert_eq!(info_name(&json, RELAY).as_deref(), Some("WeirdBrains public relay"));
        assert_eq!(info_name(&json, "12D3KooWother"), None, "another relay's info");
        assert_eq!(info_name("not json", RELAY), None);
        assert_eq!(info_name(&format!(r#"{{"name":"","peer_id":"{RELAY}"}}"#), RELAY), None);
        assert_eq!(dns_host("/dns6/r.example/udp/1/quic-v1/p2p/x").as_deref(), Some("r.example"));
        assert_eq!(dns_host("/ip4/1.2.3.4/tcp/1"), None);
    }

    #[test]
    fn stopping_without_a_check_is_harmless() {
        let links = Links::default();
        assert!(!links.check_running());
        links.relay_check_stop();
        assert!(!links.check_running());
    }

    /// The public relay's info over HTTPS: `cargo test -p morph-services -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn the_public_relay_names_itself() {
        assert_eq!(fetch_name("relay.net.weirdbrains.com", RELAY).as_deref(), Some("WeirdBrains public relay"));
        assert_eq!(fetch_name("relay.net.weirdbrains.com", "12D3KooWother"), None);
        assert_eq!(fetch_name("bad host/", RELAY), None);
    }
}
