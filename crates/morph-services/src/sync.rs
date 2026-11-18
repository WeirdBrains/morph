//! Library sync (bookmarks, history and settings) between a user's linked devices, over `weirdnet-sync`.
//!
//! The node respects Morph's discovery toggle and only *initiates* pulls
//! against roster members (and manual peers that name a roster device).
//! Discovery never authorizes a stranger. A first device (roster of one)
//! still listens so a later linked node can catch up through a relay.
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use morph_core::library_db;
use morph_core::network::NetworkSettings;
use morph_core::sync::{self as core, LinkSettings};
use morph_identity::profile_identity::{self, SecretString};
use morph_store::Library;
use weirdnet_net::user::NodeCert;
use weirdnet_net::{Keypair, Multiaddr, Node, NodeConfig, PeerId};
use weirdnet_sync::{PAGE_MAX, Roster, SyncState, node_config, pull_from, start_node};

use crate::home::{now_ms, profile_dir};
use crate::{Error, Result};

static RELAY_OVERRIDE: Mutex<Option<Vec<Multiaddr>>> = Mutex::new(None);

/// Tests point sync at a local relay without touching `MORPH_LINK_RELAYS`.
pub fn set_relays_for_tests(addrs: Option<Vec<Multiaddr>>) {
    *RELAY_OVERRIDE.lock().unwrap_or_else(|p| p.into_inner()) = addrs;
}

fn configured_relays() -> Vec<Multiaddr> {
    if let Some(v) = RELAY_OVERRIDE.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        return v;
    }
    morph_identity::device_link::link_relays()
}

struct Running {
    node: Arc<Node<Vec<u8>, Vec<u8>>>,
    state: Arc<Mutex<SyncState>>,
    /// Set when the node stops, so its background sync pass ends.
    stopped: Arc<AtomicBool>,
}

/// How often a running node runs a sync pass against its roster.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(30);
/// First pass after start, once relay reservations had a moment.
const FIRST_PASS: Duration = Duration::from_secs(3);

static NODES: Mutex<Vec<(PathBuf, Running)>> = Mutex::new(Vec::new());

fn nodes() -> MutexGuard<'static, Vec<(PathBuf, Running)>> {
    NODES.lock().unwrap_or_else(|p| p.into_inner())
}

fn user_from_seed(seed: &[u8]) -> Option<(Keypair, String)> {
    if seed.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(seed);
    let key = Keypair::ed25519_from_bytes(&mut bytes).ok()?;
    let user_id = weirdnet_net::user::user_id(&key.public());
    Some((key, user_id))
}

/// Node settings for library sync: the same relays device linking uses,
/// Morph's discovery toggle and manual peers, and 256 KiB page caps.
pub fn sync_config(home: &Path) -> NodeConfig {
    let cfg = NodeConfig {
        listen: NodeConfig::default_listen(0),
        relays: configured_relays(),
        request_max: PAGE_MAX as u64,
        response_max: PAGE_MAX as u64,
        label: "morph-sync".into(),
        ..node_config(false)
    };
    match morph_core::network::load(home) {
        Ok(net) => crate::network::apply_to_config(cfg, &net),
        Err(_) => cfg,
    }
}

/// After a password is in hand: copy the seed and node key into `library.db`
/// (already encrypted) and enable library sync.
pub fn prepare_with_password(home: &Path, profile_id: &str, password: &SecretString) -> Result<()> {
    let dir = profile_dir(home, profile_id)?;
    let seed = profile_identity::open_seed(&dir, password).map_err(Error::failed)?;
    let Some(cert) = profile_identity::load_node_cert(&dir).map_err(Error::failed)? else {
        return Ok(());
    };
    let node_key = profile_identity::open_node_key(&dir, password).map_err(Error::failed)?;
    let node_bytes = node_key.to_protobuf_encoding().map_err(Error::failed)?;
    let our = node_key.public().to_peer_id().to_string();
    library_db::with(home, profile_id, |lib| {
        let mut roster = core::load_roster(lib)?;
        roster.upsert(cert.clone());
        core::bind(lib, &our, &seed.0, &node_bytes, &roster, now_ms())?;
        Ok(())
    })
    .map_err(Error::failed)?
    .ok_or_else(|| Error::failed("the library is not open"))?;
    Ok(())
}

/// Merge a transfer's roster snapshot and opt-ins into the new device's library.
pub fn apply_link_snapshot(home: &Path, profile_id: &str, types: &[String], settings: &str) -> Result<()> {
    let incoming: LinkSettings = core::decode_link_settings(settings);
    library_db::with(home, profile_id, |lib| {
        let mut roster = core::load_roster(lib)?;
        roster.epoch = roster.epoch.max(incoming.roster.epoch);
        for c in incoming.roster.certs {
            roster.upsert(c);
        }
        core::save_roster(lib, &roster)?;
        if !types.is_empty() {
            core::set_types(lib, types)?;
        }
        core::refresh_live(&morph_core::profiles::profile_dir(home, profile_id), lib);
        Ok(())
    })
    .map_err(Error::failed)?
    .ok_or_else(|| Error::failed("the library is not open"))?;
    Ok(())
}

/// Put the new device's cert on the existing device's roster (after send).
pub fn adopt_peer_cert(home: &Path, profile_id: &str, cert: NodeCert) -> Result<()> {
    library_db::with(home, profile_id, |lib| {
        let mut roster = core::load_roster(lib)?;
        roster.upsert(cert);
        core::save_roster(lib, &roster)?;
        core::refresh_live(&morph_core::profiles::profile_dir(home, profile_id), lib);
        Ok(())
    })
    .map_err(Error::failed)?
    .ok_or_else(|| Error::failed("the library is not open"))?;
    Ok(())
}

/// Settings JSON and opt-ins to put on a transfer from this device.
pub fn link_payload(home: &Path, profile_id: &str) -> Result<(String, Vec<String>)> {
    let mut types = Vec::new();
    let mut roster = Roster::new();
    let taken = library_db::with(home, profile_id, |lib| {
        types = core::types_on(lib)?;
        roster = core::load_roster(lib)?;
        Ok(())
    })
    .map_err(Error::failed)?;
    if taken.is_none() {
        return Ok((core::encode_link_settings(&Roster::new()), core::DEFAULT_TYPES.iter().map(|s| (*s).to_string()).collect()));
    }
    if types.is_empty() {
        types = core::DEFAULT_TYPES.iter().map(|s| (*s).to_string()).collect();
    }
    let dir = profile_dir(home, profile_id)?;
    if let Ok(Some(cert)) = profile_identity::load_node_cert(&dir) {
        roster.upsert(cert);
    }
    Ok((core::encode_link_settings(&roster), types))
}

static AUTOSTART: AtomicBool = AtomicBool::new(false);

/// Let unlocks start sync nodes and their periodic passes. The shells call
/// this once at start-up; library code and other crates' tests never start
/// a networked node by accident.
pub fn enable_autostart() {
    AUTOSTART.store(true, Ordering::Relaxed);
}

fn autostart() -> bool {
    AUTOSTART.load(Ordering::Relaxed) || std::env::var_os("MORPH_SYNC_TEST").is_some()
}

/// Start a sync node for an unlocked library, if we have a seed and node key.
pub fn start(home: &Path, profile_id: &str) -> Result<()> {
    start_with(home, profile_id, sync_config(home))
}

/// [`start`] with an explicit node config (tests pass a local relay and discovery off).
pub fn start_with(home: &Path, profile_id: &str, cfg: NodeConfig) -> Result<()> {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    stop(home, profile_id);
    let built = library_db::with(home, profile_id, |lib| Ok(build_state(lib))).map_err(Error::failed)?;
    let Some(Some((state, node_key))) = built else {
        return Ok(());
    };
    let node = start_node(node_key, cfg, state.clone()).map_err(Error::failed)?;
    core::attach_live(dir.clone(), state.clone());
    let stopped = Arc::new(AtomicBool::new(false));
    nodes().push((dir, Running { node: Arc::new(node), state, stopped: stopped.clone() }));
    if autostart() {
        spawn_passes(home.to_path_buf(), profile_id.to_string(), stopped);
    }
    Ok(())
}

/// Run [`tick`] soon after start and then every [`SYNC_INTERVAL`] until the
/// node stops. Production only; tests drive `tick` themselves.
fn spawn_passes(home: PathBuf, profile_id: String, stopped: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new().name("morph-sync".into()).spawn(move || {
        let mut wait = FIRST_PASS;
        loop {
            let step = Duration::from_millis(250);
            let mut slept = Duration::ZERO;
            while slept < wait {
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(step);
                slept += step;
            }
            if stopped.load(Ordering::Relaxed) {
                return;
            }
            let _ = tick(&home, &profile_id);
            wait = SYNC_INTERVAL;
        }
    });
}

fn build_state(lib: &Library) -> Option<(Arc<Mutex<SyncState>>, Keypair)> {
    let seed = core::load_seed(lib).ok().flatten()?;
    let node_bytes = core::load_node_key(lib).ok().flatten()?;
    let our = core::node_id(lib).ok().flatten()?;
    if !core::any_on(lib) {
        return None;
    }
    let node_key = Keypair::from_protobuf_encoding(&node_bytes).ok()?;
    let (user, user_id) = user_from_seed(&seed)?;
    let roster = core::load_roster(lib).ok()?;
    let our_cert = roster.get(&our).cloned()?;
    Some((
        Arc::new(Mutex::new(SyncState {
            seed,
            user_id,
            user_key: user.public(),
            our_cert,
            our_node: weirdnet_sync::NodeId::new(our),
            types: core::types_on(lib).ok()?,
            roster,
            log: core::load_log(lib).ok()?,
            clock: core::load_clock(lib).ok()?,
        })),
        node_key,
    ))
}

/// Stop a running sync node, if any.
pub fn stop(home: &Path, profile_id: &str) {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    core::detach_live(&dir);
    let running = {
        let mut g = nodes();
        g.iter().position(|(p, _)| p == &dir).map(|i| g.remove(i).1)
    };
    if let Some(running) = running {
        running.stopped.store(true, Ordering::Relaxed);
        running.node.halt();
    }
}

/// Hello and pull every opted-in type from each other roster member, then rematerialize.
pub fn tick(home: &Path, profile_id: &str) -> Result<usize> {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    let net = morph_core::network::load(home).unwrap_or_default();
    let relays = configured_relays();
    let (state, node, our) = {
        let g = nodes();
        let Some(r) = g.iter().find(|(p, _)| p == &dir).map(|(_, r)| r) else { return Ok(0) };
        (r.state.clone(), r.node.clone(), r.node.peer_id())
    };
    let _ = library_db::with(home, profile_id, |lib| {
        morph_core::settings_sync::backfill(lib, home, profile_id)?;
        core::refresh_live(&dir, lib);
        Ok(())
    });
    let peers = {
        let st = state.lock().map_err(|_| Error::failed("sync state"))?;
        st.roster.active().filter(|c| c.peer_id != our.to_string()).cloned().collect::<Vec<_>>()
    };
    let mut applied = 0usize;
    for cert in peers {
        let Ok(peer) = PeerId::from_str(&cert.peer_id) else { continue };
        let addrs = addrs_for(&cert, &net, &relays);
        // Pull into a copy, not the live state: our handler needs the live
        // state to answer, and the peer may be pulling from us at the same
        // time. Holding the lock across the network would stall both sides.
        let mut work = {
            let st = state.lock().map_err(|_| Error::failed("sync state"))?;
            snapshot(&st)
        };
        let mut n = 0usize;
        for typ in core::SUPPORTED_TYPES {
            if !work.types.iter().any(|t| t == typ) {
                continue;
            }
            match pull_from(&mut work, node.as_ref(), peer, &addrs, typ) {
                Ok(got) => n += got,
                Err(e) => eprintln!("morph-sync: {typ} pull from {peer} failed: {e}"),
            }
        }
        {
            let mut st = state.lock().map_err(|_| Error::failed("sync state"))?;
            merge_back(&mut st, &work);
        }
        let _ = library_db::with(home, profile_id, |lib| {
            let mut st = state.lock().expect("sync state");
            if n > 0 {
                core::observe_remote(&mut st, now_ms().max(0) as u64);
            }
            core::persist_log(lib, &st.log, st.clock.as_ref())?;
            core::save_roster(lib, &st.roster)?;
            drop(st);
            if n > 0 {
                core::materialize_all(lib, now_ms())?;
                if let Err(e) = morph_core::settings_sync::materialize(lib, home, profile_id) {
                    eprintln!("morph-sync: settings: {e}");
                }
            }
            Ok(())
        });
        applied += n;
    }
    Ok(applied)
}

fn snapshot(st: &SyncState) -> SyncState {
    SyncState {
        seed: st.seed.clone(),
        user_id: st.user_id.clone(),
        user_key: st.user_key.clone(),
        our_cert: st.our_cert.clone(),
        our_node: st.our_node.clone(),
        types: st.types.clone(),
        roster: st.roster.clone(),
        log: st.log.clone(),
        clock: st.clock.clone(),
    }
}

/// Fold what a pull into a copy learned back into the live state: new log
/// entries, cursors that moved forward, roster changes (a revocation is never
/// undone) and the clock.
fn merge_back(st: &mut SyncState, work: &SyncState) {
    for (nid, seq) in work.log.cursors() {
        let ops: Vec<_> = work.log.iter_node(&nid).cloned().collect();
        if let (Some(first), Some(last)) = (ops.first().map(|e| e.seq), ops.last().map(|e| e.seq)) {
            st.log.integrate(&weirdnet_sync::LogPage { node_id: nid.clone(), from_seq: first, to_seq: last, ops });
        }
        if seq > st.log.cursor(&nid) {
            st.log.set_cursor(&nid, seq);
        }
    }
    let key = st.user_key.clone();
    st.roster.merge(&work.roster, &key, &weirdnet_net::WEIRDNET);
    if let Some(c) = &work.clock
        && st.clock.as_ref().is_none_or(|l| c.cmp_total(l).is_gt())
    {
        st.clock = Some(c.clone());
    }
}

/// Addresses we may dial for a roster member: matching manual peers, plus
/// relays (for a circuit). Discovered strangers are never listed.
fn addrs_for(cert: &NodeCert, net: &NetworkSettings, relays: &[Multiaddr]) -> Vec<Multiaddr> {
    let mut addrs = Vec::new();
    for p in net.peers() {
        if p.peer_id == cert.peer_id {
            for a in &p.addrs {
                if let Ok(m) = a.parse() {
                    addrs.push(m);
                }
            }
        }
    }
    // Relay multiaddrs are the relay's listen addresses, not the peer's.
    // A catch-up goes through a circuit: /.../p2p/<relay>/p2p-circuit/p2p/<peer>.
    if let Ok(peer) = PeerId::from_str(&cert.peer_id) {
        for r in relays {
            let s = r.to_string();
            let base = s.split_once("/p2p-circuit").map(|(h, _)| h).unwrap_or(&s);
            if let Ok(m) = format!("{base}/p2p-circuit/p2p/{peer}").parse() {
                addrs.push(m);
            }
        }
    }
    addrs
}

/// The running node's status, if any (tests wait for a relay reservation).
pub fn net_status(home: &Path, profile_id: &str) -> Option<weirdnet_net::Status> {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    nodes().iter().find(|(p, _)| p == &dir).map(|(_, r)| r.node.status())
}

/// Bind after a password unlock; start the node once [`enable_autostart`]
/// ran (or in tests that set `MORPH_SYNC_TEST`).
pub fn on_password(home: &Path, profile_id: &str, password: &SecretString) -> Result<()> {
    prepare_with_password(home, profile_id, password)?;
    if autostart() {
        start(home, profile_id)?;
    }
    Ok(())
}

/// Start after a keystore-only open, if the seed is already in the library.
pub fn on_open(home: &Path, profile_id: &str) -> Result<()> {
    if autostart() {
        start(home, profile_id)?;
    }
    Ok(())
}

/// Close the sync node with the library.
pub fn on_lock(home: &Path, profile_id: &str) {
    stop(home, profile_id);
}

/// One linked device, from the profile's roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceView {
    pub peer_id: String,
    pub label: String,
    pub issued_ms: i64,
    pub revoked_ms: Option<i64>,
    /// This install's own node.
    pub this_device: bool,
}

/// The profile's linked devices (this one first, then by link time). Empty
/// when the library is locked or the profile does not sync yet.
pub fn devices(home: &Path, profile_id: &str) -> Result<Vec<DeviceView>> {
    let listed = library_db::with(home, profile_id, |lib| {
        let ours = core::node_id(lib)?.unwrap_or_default();
        let roster = core::load_roster(lib)?;
        let mut out: Vec<DeviceView> = roster
            .certs
            .iter()
            .map(|c| DeviceView {
                peer_id: c.peer_id.clone(),
                label: c.label.clone(),
                issued_ms: c.issued_at,
                revoked_ms: c.revoked_at,
                this_device: c.peer_id == ours,
            })
            .collect();
        out.sort_by_key(|d| (!d.this_device, d.issued_ms));
        Ok(out)
    })
    .map_err(Error::failed)?;
    Ok(listed.unwrap_or_default())
}

/// Revoke a linked device: sign its revocation with the user key (from the
/// unlocked library), bump the roster epoch, and stop answering it at once.
/// Other devices learn the revocation on their next sync pass with this one.
/// This device cannot revoke itself.
pub fn revoke_device(home: &Path, profile_id: &str, peer_id: &str, now_ms: i64) -> Result<()> {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    let outcome = library_db::with(home, profile_id, |lib| {
        if core::node_id(lib)?.as_deref() == Some(peer_id) {
            return Ok(Err("this device cannot revoke itself".to_string()));
        }
        let Some((user, _)) = core::load_seed(lib)?.and_then(|seed| user_from_seed(&seed)) else {
            return Ok(Err("this profile does not sync yet".to_string()));
        };
        let mut roster = core::load_roster(lib)?;
        match roster.get(peer_id) {
            None => return Ok(Err("no such device".to_string())),
            Some(c) if !c.active() => return Ok(Ok(())),
            Some(_) => {}
        }
        if let Err(e) = roster.revoke(&weirdnet_net::WEIRDNET, &user, peer_id, now_ms) {
            return Ok(Err(e.to_string()));
        }
        core::save_roster(lib, &roster)?;
        core::refresh_live(&dir, lib);
        Ok(Ok(()))
    })
    .map_err(Error::failed)?;
    match outcome {
        None => Err(Error::failed("the library is locked")),
        Some(r) => r.map_err(Error::failed),
    }
}

/// Whether a sync node is running for this profile (tests).
pub fn is_running(home: &Path, profile_id: &str) -> bool {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    nodes().iter().any(|(p, _)| p == &dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_core::bookmarks::{self, BookmarkEdit};
    use morph_core::library as hist;
    use morph_core::network::NetworkPrefs;
    use morph_core::settings::ThemeChoice;
    use morph_identity::device_link::{AddDevice, NewDevice};
    use morph_identity::profile_identity::SecretString;
    use std::time::{Duration, Instant};
    use weirdnet_net::names::Names;
    use weirdnet_net::node::{Handler, RelayLimits};
    use weirdnet_net::{DiscoveryMode, Keypair, PeerId};
    use weirdnet_sync::SYNC_PROFILE;

    struct Refuse;

    impl Handler<Vec<u8>, Vec<u8>> for Refuse {
        fn request(&self, _from: &PeerId, _req: Vec<u8>) -> Vec<u8> {
            Vec::new()
        }
    }

    fn own_ip() -> std::net::IpAddr {
        let probe = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr()));
        match probe {
            Ok(a) if !a.ip().is_unspecified() => a.ip(),
            _ => std::net::Ipv4Addr::LOCALHOST.into(),
        }
    }

    fn free_port(ip: std::net::IpAddr, udp: bool) -> u16 {
        if udp {
            std::net::UdpSocket::bind((ip, 0)).and_then(|s| s.local_addr()).expect("udp").port()
        } else {
            std::net::TcpListener::bind((ip, 0)).and_then(|s| s.local_addr()).expect("tcp").port()
        }
    }

    struct LocalRelay {
        node: Node<Vec<u8>, Vec<u8>>,
        addrs: Vec<Multiaddr>,
    }

    impl Drop for LocalRelay {
        fn drop(&mut self) {
            self.node.halt();
        }
    }

    fn start_local_relay() -> LocalRelay {
        let ip = own_ip();
        let listen: Vec<Multiaddr> = vec![
            format!("/ip4/{ip}/tcp/{}", free_port(ip, false)).parse().unwrap(),
            format!("/ip4/{ip}/udp/{}/quic-v1", free_port(ip, true)).parse().unwrap(),
        ];
        let cfg = NodeConfig {
            external: listen.clone(),
            listen: listen.clone(),
            relay_server: true,
            relay_limits: RelayLimits::unlimited(),
            names_server: Some(Names::memory()),
            discovery: DiscoveryMode::Disabled,
            label: "morph-sync-test-relay".into(),
            ..Default::default()
        };
        let node = Node::start(&SYNC_PROFILE, Keypair::generate_ed25519(), cfg, Arc::new(Refuse)).expect("relay");
        let end = Instant::now() + Duration::from_secs(10);
        let mut raw = vec![];
        while Instant::now() < end {
            raw = node.status().listening;
            if raw.iter().any(|a| a.contains("/tcp/")) && raw.iter().any(|a| a.contains("/quic-v1")) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let addrs: Vec<Multiaddr> = raw.into_iter().map(|a| format!("{a}/p2p/{}", node.peer_id()).parse().unwrap()).collect();
        assert!(addrs.iter().any(|a| a.to_string().contains("/tcp/")), "relay tcp {addrs:?}");
        LocalRelay { node, addrs }
    }

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn home() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        morph_core::profiles::create(&home, "Work").unwrap();
        morph_core::network::save(&home, &NetworkPrefs { discovery: false, peers: vec![] }).unwrap();
        (tmp, home)
    }

    fn link_cfg(relays: &[Multiaddr]) -> NodeConfig {
        weirdnet_device_link::with_link_limits(NodeConfig {
            listen: NodeConfig::default_listen(0),
            relays: relays.to_vec(),
            discovery: DiscoveryMode::Disabled,
            label: "morph-link-test".into(),
            ..Default::default()
        })
    }

    fn sync_cfg(relays: &[Multiaddr]) -> NodeConfig {
        NodeConfig {
            listen: NodeConfig::default_listen(0),
            relays: relays.to_vec(),
            discovery: DiscoveryMode::Disabled,
            request_max: PAGE_MAX as u64,
            response_max: PAGE_MAX as u64,
            label: "morph-sync-test".into(),
            ..node_config(false)
        }
    }

    fn wait_link(new: &NewDevice, done: impl Fn(&morph_identity::device_link::LinkState) -> bool) -> morph_identity::device_link::LinkState {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let s = new.state();
            if done(&s) || Instant::now() > end {
                return s;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_reserved(home: &Path) {
        let end = Instant::now() + Duration::from_secs(15);
        while Instant::now() < end {
            if net_status(home, "p1").is_some_and(|s| !s.reservations.is_empty()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let st = net_status(home, "p1");
        panic!("no relay reservation: {st:?}");
    }

    fn tick_until(home_a: &Path, home_b: &Path, pred: impl Fn() -> bool) {
        let end = Instant::now() + Duration::from_secs(30);
        while Instant::now() < end {
            // Both sides pull at once, as two running devices would.
            std::thread::scope(|s| {
                s.spawn(|| tick(home_a, "p1"));
                s.spawn(|| tick(home_b, "p1"));
            });
            if pred() {
                return;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        panic!("sync did not catch up in time");
    }

    fn by_url(home: &Path, url: &str) -> morph_core::bookmarks::Bookmark {
        bookmarks::tree(home, "p1").unwrap().bookmarks.into_iter().find(|b| b.url == url).expect(url)
    }

    #[test]
    fn a_pull_copy_folds_back_without_undoing_a_revocation() {
        let user = Keypair::generate_ed25519();
        let uid = weirdnet_net::user::user_id(&user.public());
        let me = Keypair::generate_ed25519().public().to_peer_id().to_string();
        let other = Keypair::generate_ed25519().public().to_peer_id().to_string();
        let mine = weirdnet_sync::Roster::self_certify(&weirdnet_net::WEIRDNET, &user, &me, "me", 1).unwrap();
        let theirs = weirdnet_sync::Roster::self_certify(&weirdnet_net::WEIRDNET, &user, &other, "other", 1).unwrap();
        let mut roster = Roster::new();
        roster.upsert(mine.clone());
        roster.upsert(theirs);
        let mut live = SyncState {
            seed: vec![1; 32],
            user_id: uid,
            user_key: user.public(),
            our_cert: mine,
            our_node: weirdnet_sync::NodeId::new(me.clone()),
            types: vec!["bookmarks".into()],
            roster,
            log: weirdnet_sync::OpLog::new(),
            clock: None,
        };
        let mut work = snapshot(&live);
        // While the copy pulls, the live state revokes `other` and writes locally.
        live.roster.revoke(&weirdnet_net::WEIRDNET, &user, &other, 5).unwrap();
        live.local_op(10, "bookmarks", "mine", vec![1]);
        let peer = weirdnet_sync::NodeId::new(other.clone());
        work.log.integrate(&weirdnet_sync::LogPage {
            node_id: peer.clone(),
            from_seq: 1,
            to_seq: 1,
            ops: vec![weirdnet_sync::LogEntry {
                node_id: peer.clone(),
                seq: 1,
                hlc: weirdnet_sync::Hlc::new(50, 0, peer.clone()),
                typ: "bookmarks".into(),
                record_id: "theirs".into(),
                op: vec![2],
            }],
        });
        work.log.set_cursor(&peer, 1);
        work.clock = Some(weirdnet_sync::Hlc::new(60, 0, live.our_node.clone()));
        merge_back(&mut live, &work);
        assert!(live.log.get(&peer, 1).is_some(), "pulled ops land");
        assert!(live.log.get(&live.our_node, 1).is_some(), "local writes stay");
        assert_eq!(live.log.cursor(&peer), 1);
        assert!(!live.roster.get(&other).unwrap().active(), "the revocation stands");
        assert_eq!(live.clock.as_ref().unwrap().physical_ms, 60);
    }

    fn visits(home: &Path) -> Vec<morph_core::library::HistoryEntry> {
        hist::history(home, "p1", "", 0).unwrap()
    }

    fn has_visit(home: &Path, url: &str) -> bool {
        visits(home).iter().any(|e| e.url == url)
    }

    fn has_url(home: &Path, url: &str) -> bool {
        bookmarks::tree(home, "p1").unwrap().bookmarks.iter().any(|b| b.url == url)
    }

    /// Two Morph homes, discovery off, linked over a local relay, then
    /// bookmarks travel both ways (concurrent title vs folder move, a delete,
    /// a later folder move), then history (a pre-link visit, visits both ways,
    /// delete by URL, clear-all and a visit after the clear), then settings
    /// (a pre-link theme beats fresh defaults, a change comes back, layout
    /// stays local), then revoking B cuts it off both ways.
    #[test]
    fn two_homes_sync_the_library_over_a_local_relay() {
        let relay = start_local_relay();
        set_relays_for_tests(Some(relay.addrs.clone()));
        let cfg = link_cfg(&relay.addrs);
        let scfg = sync_cfg(&relay.addrs);

        let (_ta, a) = home();
        let (_tb, b) = home();
        crate::identity::create(&a, "p1", &pw("old password")).unwrap();

        bookmarks::add_bookmark(&a, "p1", "https://one.example/", "One", 10).unwrap();
        let (folder, _) = bookmarks::create_folder(&a, "p1", "Work", None, 11).unwrap();
        bookmarks::add_bookmark(&a, "p1", "https://two.example/", "Two", 12).unwrap();
        hist::record_visit(&a, "p1", "https://visit-a.example/", "Visit A", 13).unwrap();
        let mut prefs = morph_core::settings::load(&a).unwrap();
        prefs.theme = ThemeChoice::Light;
        prefs.sidebar_width = 300;
        morph_core::settings::save(&a, &prefs).unwrap();

        let new = NewDevice::start("Morph test phone", cfg.clone()).unwrap();
        let mut add = AddDevice::start(&morph_core::profiles::profile_dir(&a, "p1"), "Work", &pw("old password"), new.link(), cfg).unwrap();
        match wait_link(&new, |s| matches!(s, morph_identity::device_link::LinkState::Paired { .. })) {
            morph_identity::device_link::LinkState::Paired { sas } => assert_eq!(sas, add.sas()),
            other => panic!("{other:?}"),
        }
        new.confirm();
        let (settings, types) = link_payload(&a, "p1").unwrap();
        let cert = add.send_with(&settings, &types).unwrap();
        adopt_peer_cert(&a, "p1", cert).unwrap();
        match wait_link(&new, |s| matches!(s, morph_identity::device_link::LinkState::Received { .. })) {
            morph_identity::device_link::LinkState::Received { .. } => {}
            other => panic!("{other:?}"),
        }
        let linked = new.finish(&morph_core::profiles::profile_dir(&b, "p1"), &pw("new password"), 20).unwrap();
        // Link nodes used the same keys the sync nodes will; stop them first.
        drop(new);
        drop(add);
        crate::library::adopt_new_identity(
            &b,
            "p1",
            &linked.identity.user_id,
            &morph_identity::profile_identity::library_key(&morph_core::profiles::profile_dir(&b, "p1"), &pw("new password")).unwrap(),
        );
        on_password(&b, "p1", &pw("new password")).unwrap();
        apply_link_snapshot(&b, "p1", &linked.sync, &linked.settings).unwrap();
        assert_eq!(linked.sync, types);

        start_with(&a, "p1", scfg.clone()).unwrap();
        start_with(&b, "p1", scfg.clone()).unwrap();
        assert!(is_running(&a, "p1") && is_running(&b, "p1"));
        wait_reserved(&a);
        wait_reserved(&b);

        tick_until(&a, &b, || has_url(&b, "https://one.example/") && has_url(&b, "https://two.example/"));
        assert_eq!(by_url(&b, "https://one.example/").title, "One");

        // Concurrent: A renames "One", B moves it into Work, same wall time.
        let one = by_url(&a, "https://one.example/").id;
        assert_eq!(one, by_url(&b, "https://one.example/").id);
        bookmarks::update_bookmark(&a, "p1", &one, BookmarkEdit { title: Some("Renamed".into()), url: None, tags: None }, 50).unwrap();
        bookmarks::move_item(&b, "p1", &one, Some(&folder), 0, 50).unwrap();

        tick_until(&a, &b, || {
            let aa = by_url(&a, "https://one.example/");
            let bb = by_url(&b, "https://one.example/");
            aa.title == "Renamed"
                && bb.title == "Renamed"
                && aa.folder_id.as_deref() == Some(folder.as_str())
                && bb.folder_id.as_deref() == Some(folder.as_str())
        });

        // Delete on B, A catches up.
        let two = by_url(&b, "https://two.example/").id;
        bookmarks::delete_bookmark(&b, "p1", &two, 80).unwrap();
        tick_until(&a, &b, || !has_url(&a, "https://two.example/") && !has_url(&b, "https://two.example/"));

        // Folder move the other way: B creates a folder, A already has "One" in Work;
        // A moves it to the new folder after it arrives? Simpler: B adds a bookmark
        // in a new folder, A gets both the folder and the bookmark.
        let (other, _) = bookmarks::create_folder(&b, "p1", "Later", None, 90).unwrap();
        bookmarks::add_bookmark(&b, "p1", "https://three.example/", "Three", 91).unwrap();
        let three = by_url(&b, "https://three.example/").id;
        bookmarks::move_item(&b, "p1", &three, Some(&other), 0, 92).unwrap();
        tick_until(&a, &b, || {
            has_url(&a, "https://three.example/") && by_url(&a, "https://three.example/").folder_id.as_deref() == Some(other.as_str())
        });

        // History: A's pre-link visit reaches B, then visits go both ways.
        tick_until(&a, &b, || has_visit(&b, "https://visit-a.example/"));
        assert_eq!(visits(&b)[0].title, "Visit A");
        hist::record_visit(&b, "p1", "https://visit-b.example/", "Visit B", 100).unwrap();
        hist::record_visit(&a, "p1", "https://visit-a2.example/", "Visit A2", 101).unwrap();
        tick_until(&a, &b, || {
            has_visit(&a, "https://visit-b.example/") && has_visit(&b, "https://visit-a2.example/") && visits(&a).len() == 3 && visits(&b).len() == 3
        });

        // Delete by URL on A, B drops it.
        hist::delete_history_url(&a, "p1", "https://visit-b.example").unwrap();
        tick_until(&a, &b, || !has_visit(&b, "https://visit-b.example/") && visits(&b).len() == 2);

        // Clear on B, A empties; a later visit on A survives on both.
        hist::clear_history(&b, "p1").unwrap();
        tick_until(&a, &b, || visits(&a).is_empty());
        hist::record_visit(&a, "p1", "https://after-clear.example/", "After", 120).unwrap();
        tick_until(&a, &b, || has_visit(&b, "https://after-clear.example/") && visits(&b).len() == 1);
        assert_eq!(visits(&a).len(), 1);

        // Settings: A's pre-link theme wins over B's fresh defaults; B's home
        // page comes back to A; layout stays per device.
        tick_until(&a, &b, || morph_core::settings::load(&b).unwrap().theme == ThemeChoice::Light);
        assert_ne!(morph_core::settings::load(&b).unwrap().sidebar_width, 300, "layout does not sync");
        let mut prefs = morph_core::settings::load(&b).unwrap();
        prefs.home_page = "https://home.example/".into();
        prefs.search_engine = "duckduckgo".into();
        morph_core::settings::save(&b, &prefs).unwrap();
        tick_until(&a, &b, || {
            let s = morph_core::settings::load(&a).unwrap();
            s.home_page == "https://home.example/" && s.search_engine == "duckduckgo"
        });
        assert_eq!(morph_core::settings::load(&a).unwrap().theme, ThemeChoice::Light);
        assert_eq!(morph_core::settings::load(&a).unwrap().sidebar_width, 300);

        // Devices: both list each other; A revokes B, and B's later changes
        // no longer reach A, nor A's reach B.
        let listed = devices(&a, "p1").unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed[0].this_device && !listed[1].this_device);
        let b_peer = listed[1].peer_id.clone();
        assert!(revoke_device(&a, "p1", &listed[0].peer_id, 200).is_err(), "no self-revocation");
        revoke_device(&a, "p1", &b_peer, 200).unwrap();
        assert_eq!(devices(&a, "p1").unwrap()[1].revoked_ms, Some(200));
        revoke_device(&a, "p1", &b_peer, 201).unwrap();
        assert_eq!(devices(&a, "p1").unwrap()[1].revoked_ms, Some(200), "revoking twice is a no-op");
        hist::record_visit(&b, "p1", "https://after-revoke-b.example/", "", 210).unwrap();
        hist::record_visit(&a, "p1", "https://after-revoke-a.example/", "", 211).unwrap();
        for _ in 0..5 {
            std::thread::scope(|s| {
                s.spawn(|| tick(&a, "p1"));
                s.spawn(|| tick(&b, "p1"));
            });
        }
        assert!(!has_visit(&a, "https://after-revoke-b.example/"), "a revoked device's changes stay out");
        assert!(!has_visit(&b, "https://after-revoke-a.example/"), "a revoked device stops receiving");

        stop(&a, "p1");
        stop(&b, "p1");
        set_relays_for_tests(None);
    }
}
