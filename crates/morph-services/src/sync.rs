//! Library sync (bookmarks, history, settings and companion records) between a user's linked devices, over `weirdnet-sync`.
//!
//! The node respects Morph's discovery toggle and only *initiates* pulls
//! against roster members (and manual peers that name a roster device).
//! Discovery never authorizes a stranger. A first device (roster of one)
//! still listens so a later linked node can catch up through a relay.
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::companion_sync;
use morph_core::library_db;
use morph_core::network::NetworkSettings;
use morph_core::sync::{self as core, LinkSettings};
use morph_identity::profile_identity::{self, SecretString};
use morph_store::Library;
use weirdnet_net::user::NodeCert;
use weirdnet_net::{Keypair, Multiaddr, Node, NodeConfig, PeerId};
use weirdnet_sync::{Device, KeyStatus, PAGE_MAX, Roster, SyncState, node_config, pull_from_outcome, start_node};

use crate::home::{now_ms, profile_dir};
use crate::{Error, Result};

/// Counts the times synced data or revocations were applied to a profile (a pull that brought
/// something in, a revocation arriving or made here). Hosts that run things from synced config,
/// such as the MCP host, watch it and re-check at once instead of waiting for their next reload.
static APPLIED: AtomicU64 = AtomicU64::new(0);

/// The current count; it only goes up. Poll it and act when it changes.
pub fn applied_revision() -> u64 {
    APPLIED.load(Ordering::SeqCst)
}

fn note_applied() {
    APPLIED.fetch_add(1, Ordering::SeqCst);
}

static RELAY_OVERRIDE: Mutex<Option<Vec<Multiaddr>>> = Mutex::new(None);

/// Tests point sync at a local relay without touching `MORPH_LINK_RELAYS`.
pub fn set_relays_for_tests(addrs: Option<Vec<Multiaddr>>) {
    *RELAY_OVERRIDE.lock().unwrap_or_else(|p| p.into_inner()) = addrs;
}

pub(crate) fn configured_relays() -> Vec<Multiaddr> {
    if let Some(v) = RELAY_OVERRIDE.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        return v;
    }
    morph_identity::device_link::link_relays()
}

struct Running {
    /// The home and profile it started for, so [`restart_all`] can start it again.
    home: PathBuf,
    profile_id: String,
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
        // The key chain too, so this device knows whether keys have rotated
        // and shows that it waits for admission until its first sync.
        roster.chain.merge(&incoming.roster.chain);
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
/// Linking is the user's approval: once keys have rotated, the device is
/// admitted as soon as its first hello brings its device key.
pub fn adopt_peer_cert(home: &Path, profile_id: &str, cert: NodeCert) -> Result<()> {
    library_db::with(home, profile_id, |lib| {
        if let Some((mut st, _)) = offline_state(lib) {
            let _ = st.approve(cert.clone(), now_ms());
            core::save_roster(lib, &st.roster)?;
            core::save_epoch_keys(lib, &st.keys)?;
        } else {
            let mut roster = core::load_roster(lib)?;
            roster.upsert(cert);
            core::save_roster(lib, &roster)?;
        }
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
    // A locked profile never syncs (docs/profile-lock.md).
    if crate::profile_lock::is_locked(home, profile_id) {
        return Err(Error::Locked);
    }
    let built = library_db::with(home, profile_id, |lib| Ok(build_state(lib))).map_err(Error::failed)?;
    let Some(Some((state, node_key))) = built else {
        return Ok(());
    };
    let node = start_node(node_key, cfg, state.clone()).map_err(Error::failed)?;
    // On load: rows merged before a revocation arrived (or by an older build) are rebuilt.
    let _ = library_db::with(home, profile_id, |lib| {
        if core::revocations_unapplied(lib).unwrap_or(true) || companion_sync::revocations_pending(lib) {
            apply_revocations(lib, home, profile_id);
        }
        Ok(())
    });
    core::attach_live(dir.clone(), state.clone());
    let stopped = Arc::new(AtomicBool::new(false));
    let running = Running { home: home.to_path_buf(), profile_id: profile_id.to_string(), node: Arc::new(node), state, stopped: stopped.clone() };
    nodes().push((dir, running));
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
            seed: Some(seed),
            user_id,
            user_key: user.public(),
            our_cert,
            our_node: weirdnet_sync::NodeId::new(our),
            types: core::types_on(lib).ok()?,
            roster,
            log: core::load_log(lib).ok()?,
            clock: core::load_clock(lib).ok()?,
            device: Device::from_node_key(&node_key).ok(),
            keys: core::load_epoch_keys(lib).ok()?,
            // Start the key chain right after the first link, or once after an
            // upgrade from seed-only keys (once every device has a device key).
            auto_first_rotation: true,
            records: core::load_records(lib).ok()?,
        })),
        node_key,
    ))
    .map(|(st, key)| {
        if let Ok(mut s) = st.lock() {
            let _ = s.publish_device_key(now_ms());
            s.refresh_keys();
            companion_sync::configure(&mut s);
        }
        (st, key)
    })
}

/// The profile's sync state without a running node, for revoking and approving.
fn offline_state(lib: &Library) -> Option<(SyncState, Keypair)> {
    let seed = core::load_seed(lib).ok().flatten()?;
    let node_key = Keypair::from_protobuf_encoding(&core::load_node_key(lib).ok().flatten()?).ok()?;
    let our = core::node_id(lib).ok().flatten()?;
    let (user, user_id) = user_from_seed(&seed)?;
    let roster = core::load_roster(lib).ok()?;
    let our_cert = roster.get(&our).cloned()?;
    let mut st = SyncState {
        seed: Some(seed),
        user_id,
        user_key: user.public(),
        our_cert,
        our_node: weirdnet_sync::NodeId::new(our),
        types: core::types_on(lib).ok()?,
        roster,
        log: weirdnet_sync::OpLog::new(),
        clock: None,
        device: Device::from_node_key(&node_key).ok(),
        keys: core::load_epoch_keys(lib).ok()?,
        auto_first_rotation: true,
        records: core::load_records(lib).ok()?,
    };
    st.publish_device_key(now_ms()).ok()?;
    st.refresh_keys();
    companion_sync::configure(&mut st);
    Some((st, user))
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

/// Start every running sync node under `home` again with [`sync_config`],
/// so a changed discovery toggle or peer list reaches it. Returns how many
/// restarted; a node whose library has closed meanwhile stays stopped.
pub fn restart_all(home: &Path) -> usize {
    // One restart at a time, so two quick changes never start a profile twice.
    static RESTART: Mutex<()> = Mutex::new(());
    let _one = RESTART.lock().unwrap_or_else(|p| p.into_inner());
    let running: Vec<String> = nodes().iter().filter(|(_, r)| r.home == home).map(|(_, r)| r.profile_id.clone()).collect();
    running.iter().filter(|profile_id| start(home, profile_id).is_ok() && is_running(home, profile_id)).count()
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
        if let Err(e) = companion_sync::backfill_with(lib, &companion_sync::store(&dir), companion_sync::secrets_for(&dir).as_deref(), now_ms()) {
            eprintln!("morph-sync: companions: {e}");
        }
        core::refresh_live(&dir, lib);
        Ok(())
    });
    let (peers, revoked_before) = {
        let st = state.lock().map_err(|_| Error::failed("sync state"))?;
        (st.roster.active().filter(|c| c.peer_id != our.to_string()).cloned().collect::<Vec<_>>(), revoked_ids(&st.roster))
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
            st.snapshot()
        };
        let mut n = 0usize;
        for typ in core::SUPPORTED_TYPES {
            if !work.types.iter().any(|t| t == typ) {
                continue;
            }
            // A pull refused part way (the peer rotated between its hello and a
            // later page: epoch-behind) has already applied the pages before it;
            // they count, so this tick materializes them.
            let pulled = pull_from_outcome(&mut work, node.as_ref(), peer, &addrs, typ);
            n += pulled.applied;
            if let Some(e) = pulled.error {
                eprintln!("morph-sync: {typ} pull from {peer} failed: {e}");
            }
            // The pull brings a type's chunks along; ask again for any a record still
            // misses (another device may have written them since).
            if !work.log.pending_blobs(typ).is_empty() {
                match weirdnet_sync::pull_chunks(&mut work, node.as_ref(), peer, &addrs, typ) {
                    Ok(got) => n += got,
                    Err(e) => eprintln!("morph-sync: {typ} chunks from {peer} failed: {e}"),
                }
            }
        }
        {
            let mut st = state.lock().map_err(|_| Error::failed("sync state"))?;
            n = n.max(st.merge_back(&work).entries);
        }
        let _ = library_db::with(home, profile_id, |lib| {
            let st = state.lock().expect("sync state");
            core::persist_log(lib, &st.log, st.clock.as_ref())?;
            core::save_records(lib, &st.records)?;
            core::save_roster(lib, &st.roster)?;
            core::save_epoch_keys(lib, &st.keys)?;
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
    // Companion records: merge what came in, and void a revoked device's grants as soon as a
    // revocation arrives from another device.
    // `revocations_pending` also catches one that reached the library without new ops, or
    // whose void failed on an earlier pass.
    let revoked_now = state.lock().map(|st| revoked_ids(&st.roster)).unwrap_or_default();
    let _ = library_db::with(home, profile_id, |lib| {
        if revoked_now != revoked_before || core::revocations_unapplied(lib).unwrap_or(true) {
            // Rebuild every type without what revoked devices wrote late (all of it, for the
            // security-sensitive companion types).
            apply_revocations(lib, home, profile_id);
        } else if (applied > 0 || companion_sync::revocations_pending(lib))
            && let Err(e) = companion_sync::materialize_with(lib, &companion_sync::store(&dir), companion_sync::secrets_for(&dir).as_deref())
        {
            eprintln!("morph-sync: companions: {e}");
        }
        Ok(())
    });
    if applied > 0 || state.lock().map(|st| revoked_ids(&st.roster)).unwrap_or_default() != revoked_before {
        note_applied();
    }
    Ok(applied)
}

/// Rematerialize every type against the roster's current revocations, then record them as
/// applied. Run when the revocations change, when sync starts, and after a revoke here.
fn apply_revocations(lib: &mut Library, home: &Path, profile_id: &str) {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    if let Err(e) = core::materialize_all(lib, now_ms()) {
        eprintln!("morph-sync: revocations: {e}");
        return;
    }
    if let Err(e) = morph_core::settings_sync::materialize(lib, home, profile_id) {
        eprintln!("morph-sync: settings: {e}");
    }
    if let Err(e) = companion_sync::materialize_with(lib, &companion_sync::store(&dir), companion_sync::secrets_for(&dir).as_deref()) {
        eprintln!("morph-sync: companions: {e}");
        return;
    }
    if let Err(e) = core::mark_revocations_applied(lib) {
        eprintln!("morph-sync: revocations: {e}");
    }
}

/// Whether the stdio MCP server `server_id` of a profile may start a process on this device:
/// its synced config was written here or by a linked device that is not revoked (in the
/// library's roster and, while sync runs, the live one). Fails closed when the library is
/// locked or unreadable. The MCP host checks this before it starts a local command, on top of
/// the per-machine approval.
pub fn mcp_may_start(home: &Path, profile_id: &str, server_id: &str) -> bool {
    let live_revoked: Vec<String> =
        live(home, profile_id).map(|(_, st)| st.lock().map(|st| revoked_ids(&st.roster)).unwrap_or_default()).unwrap_or_default();
    let trusted = library_db::with(home, profile_id, |lib| {
        let writer_ok = companion_sync::mcp_writer_trusted(lib, server_id).unwrap_or(false);
        let writer = companion_sync::mcp_writer(lib, server_id).ok().flatten();
        Ok(writer_ok && writer.is_none_or(|w| !live_revoked.contains(&w)))
    });
    // No library for the profile: nothing ever synced into it, so the config was written here.
    matches!(trusted, Ok(Some(true)) | Ok(None))
}

/// [`mcp_may_start`] for the profile whose companions directory is `companions`
/// (`<home>/profiles/<id>/companions`).
pub fn mcp_may_start_in(companions: &Path, server_id: &str) -> bool {
    let Some(profile) = companions.parent() else { return false };
    let (Some(id), Some(home)) = (profile.file_name().and_then(|n| n.to_str()), profile.parent().and_then(Path::parent)) else { return false };
    mcp_may_start(home, id, server_id)
}

fn revoked_ids(roster: &Roster) -> Vec<String> {
    roster.certs.iter().filter(|c| !c.active()).map(|c| c.peer_id.clone()).collect()
}

/// Addresses we may dial for a roster member: matching manual peers, plus
/// relays (for a circuit). Discovered strangers are never listed.
pub(crate) fn addrs_for(cert: &NodeCert, net: &NetworkSettings, relays: &[Multiaddr]) -> Vec<Multiaddr> {
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

/// The running sync node of a profile and its live state, for calls between linked devices
/// on this device's identity ([`crate::device_calls`]).
pub(crate) type Live = (Arc<Node<Vec<u8>, Vec<u8>>>, Arc<Mutex<SyncState>>);

/// See [`Live`].
pub(crate) fn live(home: &Path, profile_id: &str) -> Option<Live> {
    let dir = morph_core::profiles::profile_dir(home, profile_id);
    nodes().iter().find(|(p, _)| p == &dir).map(|(_, r)| (r.node.clone(), r.state.clone()))
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
    /// This device: keys rotated without it (restored from the recovery words,
    /// or linked while offline), so another device has to re-admit it.
    pub needs_admission: bool,
    /// Another device, linked here after keys rotated: it is admitted on its
    /// first sync with this one.
    pub awaiting_first_sync: bool,
    /// It has published its device key (it runs a Morph with key rotation and
    /// has synced since). A revoke now leaves devices without one out of the
    /// new key.
    pub has_key: bool,
}

impl DeviceView {
    /// Its state in Linked devices: "This device", "Linked" or "Revoked".
    pub fn state_label(&self) -> &'static str {
        match (self.this_device, self.revoked_ms.is_some()) {
            (_, true) => "Revoked",
            (true, false) => "This device",
            (false, false) => "Linked",
        }
    }

    /// One line for the device's state in Linked devices, if it needs one.
    pub fn key_note(&self) -> Option<&'static str> {
        if self.revoked_ms.is_some() {
            None
        } else if self.needs_admission {
            Some("Needs re-admission from another device")
        } else if self.awaiting_first_sync {
            Some("Waiting for its first sync")
        } else {
            None
        }
    }
}

/// After restoring an identity from its recovery words, shards or a backup:
/// this install has a new node key, so once keys have rotated it reads
/// nothing until another device admits it (Linked devices shows "Needs
/// re-admission from another device" meanwhile).
pub const RESTORED_NOTE: &str = "If you use this identity on other devices, link this one again from one of them so it can read what they sync; until then Linked devices shows it waiting for re-admission.";

/// What to tell the user before revoking `peer_id`: the other active devices
/// that have not synced since updating Morph are left out of the new key and
/// will need re-admission (linking them again from a device that has it).
pub fn revoke_warning(devices: &[DeviceView], peer_id: &str) -> Option<String> {
    let left_out: Vec<&str> = devices
        .iter()
        .filter(|d| d.peer_id != peer_id && !d.this_device && d.revoked_ms.is_none() && !d.has_key)
        .map(|d| if d.label.is_empty() { "an unnamed device" } else { d.label.as_str() })
        .collect();
    if left_out.is_empty() {
        return None;
    }
    let who = left_out.join(", ");
    let verb = if left_out.len() == 1 { "has" } else { "have" };
    Some(format!(
        "{who} {verb} not synced since updating Morph. Revoking now leaves {} out of the new key; link {} again afterwards.",
        if left_out.len() == 1 { "it" } else { "them" },
        if left_out.len() == 1 { "it" } else { "them" }
    ))
}

/// The profile's linked devices (this one first, then by link time). Empty
/// when the library is locked or the profile does not sync yet.
pub fn devices(home: &Path, profile_id: &str) -> Result<Vec<DeviceView>> {
    let listed = library_db::with(home, profile_id, |lib| {
        let ours = core::node_id(lib)?.unwrap_or_default();
        let roster = core::load_roster(lib)?;
        let st = offline_state(lib).map(|(st, _)| st);
        let ours_waiting = st.as_ref().is_some_and(|st| st.key_status() == KeyStatus::NeedsAdmission);
        let chain = st.as_ref().map(|st| &st.roster.chain).unwrap_or(&roster.chain);
        let mut out: Vec<DeviceView> = roster
            .certs
            .iter()
            .map(|c| DeviceView {
                peer_id: c.peer_id.clone(),
                label: c.label.clone(),
                issued_ms: c.issued_at,
                revoked_ms: c.revoked_at,
                this_device: c.peer_id == ours,
                needs_admission: c.peer_id == ours && ours_waiting,
                awaiting_first_sync: c.peer_id != ours && chain.approved.contains(&c.peer_id),
                has_key: chain.device(&c.peer_id).is_some(),
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
        let Some((mut st, user)) = offline_state(lib) else {
            return Ok(Err("this profile does not sync yet".to_string()));
        };
        match st.roster.get(peer_id) {
            None => return Ok(Err("no such device".to_string())),
            Some(c) if !c.active() => return Ok(Ok(())),
            Some(_) => {}
        }
        // Revoke and rotate: the revoked device keeps the seed, not the new key.
        if let Err(e) = st.revoke(&user, peer_id, now_ms) {
            return Ok(Err(e.to_string()));
        }
        core::save_roster(lib, &st.roster)?;
        core::save_epoch_keys(lib, &st.keys)?;
        // Its grants and Node companions are void now, not on the next pass.
        if let Err(e) = companion_sync::after_revoke(lib, &companion_sync::store(&dir), companion_sync::secrets_for(&dir).as_deref(), now_ms) {
            eprintln!("morph-sync: companions after revoke: {e}");
        }
        apply_revocations(lib, home, profile_id);
        core::refresh_live(&dir, lib);
        // The live roster has the revocation now: hosts re-check what they run.
        note_applied();
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

    /// Generous: a shared CI runner, with the first key rotation and the
    /// other tests' nodes running at once, can take a while to settle.
    fn tick_until(home_a: &Path, home_b: &Path, pred: impl Fn() -> bool) {
        let end = Instant::now() + Duration::from_secs(90);
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
        for h in [home_a, home_b] {
            let r = library_db::with(h, "p1", |lib| core::load_roster(lib)).unwrap().unwrap();
            let v = r.chain_view();
            let marks: Vec<String> = bookmarks::tree(h, "p1").unwrap().bookmarks.iter().map(|b| b.url.clone()).collect();
            eprintln!(
                "DBG2 home={} head={:?} status={:?} keys={} marks={marks:?}",
                h.display(),
                v.head,
                library_db::with(h, "p1", |lib| Ok(offline_state(lib).map(|(st, _)| st.key_status()))).unwrap(),
                library_db::with(h, "p1", |lib| Ok(core::load_epoch_keys(lib)?.len())).unwrap().unwrap()
            );
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
            seed: Some(vec![1; 32]),
            user_id: uid,
            user_key: user.public(),
            our_cert: mine,
            our_node: weirdnet_sync::NodeId::new(me.clone()),
            types: vec!["bookmarks".into()],
            roster,
            log: weirdnet_sync::OpLog::new(),
            clock: None,
            device: None,
            keys: Default::default(),
            auto_first_rotation: false,
            records: Default::default(),
        };
        let mut work = live.snapshot();
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
                value: None,
            }],
        });
        work.log.set_cursor(&peer, 1);
        work.clock = Some(weirdnet_sync::Hlc::new(60, 0, live.our_node.clone()));
        assert_eq!(live.merge_back(&work).entries, 1, "the pulled op is new to the live log");
        assert!(live.log.get(&peer, 1).is_some(), "pulled ops land");
        assert_eq!(live.merge_back(&work).entries, 0, "nothing is new the second time");
        assert!(live.log.get(&live.our_node, 1).is_some(), "local writes stay");
        assert_eq!(live.log.cursor(&peer), 1);
        assert!(!live.roster.get(&other).unwrap().active(), "the revocation stands");
        assert!(live.clock.as_ref().unwrap().physical_ms >= 60, "the clock moves past the pulled one");
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
    /// The relay tests share the relay override and the node registry.
    static RELAY_TESTS: Mutex<()> = Mutex::new(());

    #[test]
    fn two_homes_sync_the_library_over_a_local_relay() {
        let _one = RELAY_TESTS.lock().unwrap_or_else(|p| p.into_inner());
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

        // A network settings change restarts B's node from network.json
        // (discovery stays off); it reaches the relay again and keeps syncing.
        assert_eq!(restart_all(&b), 1);
        assert_eq!(restart_all(&tempfile::tempdir().unwrap().path().join("none")), 0, "other homes are left alone");
        assert!(is_running(&b, "p1"));
        wait_reserved(&b);

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

        // Companion records: B's companion and grants reach A, A's chat reaches B.
        use morph_ai::AiStore;
        use weirdnet_agent::{AgentId, AllowRule, CompanionScope, SiteLevel};
        let store_a = companion_sync::store(&morph_core::profiles::profile_dir(&a, "p1"));
        let store_b = companion_sync::store(&morph_core::profiles::profile_dir(&b, "p1"));
        let mut roster_b = morph_ai::Roster::first_run(morph_ai::ModelChoice::Gemma4E2B);
        roster_b.add(morph_ai::CompanionProfile::local("atlas", "Atlas", morph_ai::ModelChoice::Gemma4E2B)).unwrap();
        store_b.save_roster(&roster_b).unwrap();
        let mut policy_b = weirdnet_agent::Policy::new();
        policy_b.set_site_level(&AgentId::new("atlas"), "tokio.rs", SiteLevel::Act);
        policy_b.add_allow(CompanionScope::All, AllowRule::Research { provider: None }, 5);
        store_b.save_policy(&policy_b).unwrap();
        let mut chats_a = morph_ai::ChatLog::default();
        chats_a.push(morph_ai::ChatTurn::new(AgentId::new("local"), "what is tokio", "an async runtime", 7));
        store_a.save_chats(&chats_a).unwrap();
        tick_until(&a, &b, || {
            let roster = store_a.load_roster().ok().flatten();
            let policy = store_a.load_policy().ok().flatten();
            let chats = store_b.load_chats().ok().flatten();
            roster.is_some_and(|r| r.get(&AgentId::new("atlas")).is_some())
                && policy.is_some_and(|p| p.site_level(&AgentId::new("atlas"), "tokio.rs") == SiteLevel::Act && p.allows().len() == 1)
                && chats.is_some_and(|c| c.turns.len() == 1)
        });

        // Devices: both list each other; A revokes B, and B's later changes
        // no longer reach A, nor A's reach B.
        let listed = devices(&a, "p1").unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed[0].this_device && !listed[1].this_device);
        let b_peer = listed[1].peer_id.clone();
        assert!(revoke_device(&a, "p1", &listed[0].peer_id, 200).is_err(), "no self-revocation");
        let before = applied_revision();
        revoke_device(&a, "p1", &b_peer, 200).unwrap();
        assert!(applied_revision() > before, "a revocation made here tells the hosts watching it");
        assert_eq!(devices(&a, "p1").unwrap()[1].revoked_ms, Some(200));
        let policy_a = store_a.load_policy().unwrap().unwrap();
        assert!(policy_a.allows().is_empty(), "the revoked device's standing allow is void at once");
        assert_eq!(policy_a.site_level(&AgentId::new("atlas"), "tokio.rs"), SiteLevel::Ask, "and its site grant");
        assert!(
            store_a.load_roster().unwrap().unwrap().get(&AgentId::new("atlas")).is_none(),
            "every companion op it wrote is void, a Local companion too"
        );
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
        // The revoke rotated: A holds a random epoch key, and B (which still has the
        // seed) was not sealed a copy.
        let keys_of = |home: &Path| {
            library_db::with(home, "p1", |lib| Ok(core::load_epoch_keys(lib)?.ids().map(|(id, _)| id.to_string()).collect::<Vec<_>>()))
                .unwrap()
                .unwrap()
        };
        let head = library_db::with(&a, "p1", |lib| Ok(core::load_roster(lib)?.head_id())).unwrap().unwrap().expect("a rotation");
        assert!(keys_of(&a).contains(&head));
        assert!(!keys_of(&b).contains(&head), "the revoked device got no copy of the new key");
        let a_status = library_db::with(&a, "p1", |lib| Ok(offline_state(lib).map(|(st, _)| st.key_status()))).unwrap().unwrap();
        assert!(matches!(a_status, Some(KeyStatus::Current { .. })), "{a_status:?}");

        stop(&a, "p1");
        stop(&b, "p1");
        set_relays_for_tests(None);
    }

    /// Link a new home `b` to `a` through the device-link flow, as the
    /// Identity screen does on both devices.
    fn link_home(a: &Path, b: &Path, label: &str, cfg: &NodeConfig) {
        let new = NewDevice::start(label, cfg.clone()).unwrap();
        let mut add = AddDevice::start(&morph_core::profiles::profile_dir(a, "p1"), "Work", &pw("old password"), new.link(), cfg.clone()).unwrap();
        assert!(matches!(
            wait_link(&new, |s| matches!(s, morph_identity::device_link::LinkState::Paired { .. })),
            morph_identity::device_link::LinkState::Paired { .. }
        ));
        new.confirm();
        let (settings, types) = link_payload(a, "p1").unwrap();
        let cert = add.send_with(&settings, &types).unwrap();
        adopt_peer_cert(a, "p1", cert).unwrap();
        assert!(matches!(
            wait_link(&new, |s| matches!(s, morph_identity::device_link::LinkState::Received { .. })),
            morph_identity::device_link::LinkState::Received { .. }
        ));
        let linked = new.finish(&morph_core::profiles::profile_dir(b, "p1"), &pw("new password"), 20).unwrap();
        drop(new);
        drop(add);
        crate::library::adopt_new_identity(
            b,
            "p1",
            &linked.identity.user_id,
            &morph_identity::profile_identity::library_key(&morph_core::profiles::profile_dir(b, "p1"), &pw("new password")).unwrap(),
        );
        on_password(b, "p1", &pw("new password")).unwrap();
        apply_link_snapshot(b, "p1", &linked.sync, &linked.settings).unwrap();
    }

    fn tick_all(homes: &[&Path], pred: impl Fn() -> bool) {
        let end = Instant::now() + Duration::from_secs(180);
        while Instant::now() < end {
            std::thread::scope(|s| {
                for h in homes {
                    s.spawn(move || tick(h, "p1"));
                }
            });
            if pred() {
                return;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        for h in homes {
            let r = library_db::with(h, "p1", |lib| core::load_roster(lib)).unwrap().unwrap();
            let marks: Vec<String> = bookmarks::tree(h, "p1").unwrap().bookmarks.iter().map(|b| b.url.clone()).collect();
            let cursors =
                library_db::with(h, "p1", |lib| Ok(lib.sync_meta(core::META_TYPE_CURSORS)?.map(|b| String::from_utf8_lossy(&b).into_owned())))
                    .unwrap()
                    .unwrap();
            eprintln!("DBG home={} marks={marks:?} cursors={cursors:?}", h.display());
            let v = r.chain_view();
            eprintln!(
                "DBG head={:?} root={:?} sticky={:?} status={:?} rots={:?} devs={} certs={} appr={:?}",
                v.head,
                v.root,
                r.chain.root,
                key_status(h),
                r.chain
                    .rotations
                    .iter()
                    .map(|x| (x.id(), x.epoch, x.parent.clone(), x.by.clone(), x.keys.iter().map(|k| k.peer_id.clone()).collect::<Vec<_>>()))
                    .collect::<Vec<_>>(),
                r.chain.devices.len(),
                r.certs.len(),
                r.chain.approved
            );
        }
        panic!("sync did not settle in time");
    }

    fn key_status(home: &Path) -> Option<KeyStatus> {
        library_db::with(home, "p1", |lib| Ok(offline_state(lib).map(|(st, _)| st.key_status()))).unwrap().flatten()
    }

    fn head_of(home: &Path) -> Option<String> {
        library_db::with(home, "p1", |lib| Ok(core::load_roster(lib)?.head_id())).unwrap().flatten()
    }

    fn view(home: &Path, peer: &str) -> DeviceView {
        devices(home, "p1").unwrap().into_iter().find(|d| d.peer_id == peer).expect("listed")
    }

    /// Three linked homes: the first rotation starts by itself; revoking C
    /// rotates, B catches up and C is shut out; a device linked afterwards
    /// (as a restored install is) waits for admission, gets it on its first
    /// sync, and catches up on the whole library.
    #[test]
    fn three_homes_rotate_on_revoke_and_re_admit_a_restored_device() {
        let _one = RELAY_TESTS.lock().unwrap_or_else(|p| p.into_inner());
        let relay = start_local_relay();
        set_relays_for_tests(Some(relay.addrs.clone()));
        let cfg = link_cfg(&relay.addrs);
        let scfg = sync_cfg(&relay.addrs);
        let (_ta, a) = home();
        let (_tb, b) = home();
        let (_tc, c) = home();
        crate::identity::create(&a, "p1", &pw("old password")).unwrap();
        bookmarks::add_bookmark(&a, "p1", "https://before.example/", "Before", 10).unwrap();
        link_home(&a, &b, "Phone", &cfg);
        link_home(&a, &c, "Laptop", &cfg);
        for h in [&a, &b, &c] {
            start_with(h, "p1", scfg.clone()).unwrap();
            wait_reserved(h);
        }
        // Every device publishes its key on its first sync; then the first
        // rotation starts once, and all three hold the key.
        tick_all(&[&a, &b, &c], || {
            let heads: Vec<_> = [&a, &b, &c].iter().map(|h| head_of(h)).collect();
            heads[0].is_some()
                && heads.iter().all(|h| *h == heads[0])
                && [&a, &b, &c].iter().all(|h| matches!(key_status(h), Some(KeyStatus::Current { .. })))
                && has_url(&c, "https://before.example/")
        });
        let first = head_of(&a).unwrap();
        assert!([&b, &c].iter().all(|h| head_of(h).as_deref() == Some(first.as_str())));

        let peers: Vec<DeviceView> = devices(&a, "p1").unwrap();
        let c_peer = peers.iter().find(|d| d.label == "Laptop").unwrap().peer_id.clone();
        assert!(peers.iter().all(|d| d.has_key), "everyone synced since updating");
        assert_eq!(revoke_warning(&peers, &c_peer), None);
        revoke_device(&a, "p1", &c_peer, now_ms()).unwrap();
        let second = head_of(&a).unwrap();
        assert_ne!(second, first, "revoking rotated");
        bookmarks::add_bookmark(&a, "p1", "https://after.example/", "After", 100).unwrap();
        tick_all(&[&a, &b, &c], || has_url(&b, "https://after.example/") && head_of(&b).as_deref() == Some(second.as_str()));
        for _ in 0..3 {
            std::thread::scope(|s| {
                s.spawn(|| tick(&a, "p1"));
                s.spawn(|| tick(&b, "p1"));
                s.spawn(|| tick(&c, "p1"));
            });
        }
        assert!(!has_url(&c, "https://after.example/"), "the revoked device gets nothing new");
        assert!(matches!(key_status(&b), Some(KeyStatus::Current { .. })));
        let c_keys = library_db::with(&c, "p1", |lib| Ok(core::load_epoch_keys(lib)?.ids().map(|(id, _)| id.to_string()).collect::<Vec<_>>()))
            .unwrap()
            .unwrap();
        assert!(!c_keys.contains(&second), "the revoked device holds no copy of the new key");

        // A reinstall restored from the recovery words comes back by linking.
        let (_td, d) = home();
        link_home(&a, &d, "Phone (restored)", &cfg);
        let d_peer = devices(&d, "p1").unwrap().into_iter().find(|x| x.this_device).unwrap().peer_id;
        assert_eq!(key_status(&d), Some(KeyStatus::NeedsAdmission));
        assert_eq!(view(&d, &d_peer).key_note(), Some("Needs re-admission from another device"));
        assert_eq!(view(&a, &d_peer).key_note(), Some("Waiting for its first sync"));
        // Before D's first sync A has no device key for it, so revoking B now
        // would leave D out of the new key: Linked devices warns first.
        let b_peer = devices(&a, "p1").unwrap().into_iter().find(|x| x.label == "Phone").unwrap().peer_id;
        let w = revoke_warning(&devices(&a, "p1").unwrap(), &b_peer).expect("a warning before the revoke");
        assert!(w.starts_with("Phone (restored) has not synced since updating Morph"), "{w}");
        start_with(&d, "p1", scfg.clone()).unwrap();
        wait_reserved(&d);
        tick_all(&[&a, &b, &d], || has_url(&d, "https://after.example/") && has_url(&d, "https://before.example/"));
        assert!(matches!(key_status(&d), Some(KeyStatus::Current { .. })));
        assert_eq!(view(&d, &d_peer).key_note(), None);
        tick_all(&[&a, &b, &d], || view(&a, &d_peer).key_note().is_none());

        for h in [&a, &b, &c, &d] {
            stop(h, "p1");
        }
        set_relays_for_tests(None);
    }
}
