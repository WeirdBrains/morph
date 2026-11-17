//! Install-level Weirdnet network preferences: discovery and manually added
//! peers, stored as `network.json` under the Morph home (not per profile).
//!
//! The effective discovery flag is the stored preference (on by default),
//! unless an admin override turns it off: the environment variable
//! `MORPH_DISABLE_WEIRDNET_DISCOVERY`, or a policy file
//! (`MORPH_NETWORK_POLICY`, else `<home>/network.policy.json`, else
//! `/etc/morph/network.policy.json` on Unix). A locked-off setting is shown
//! in the UI but cannot be turned back on from there.
//!
//! Passing `discovery` and the peer list into `weirdnet-net::NodeConfig`
//! waits on a published weirdnet-net that has `DiscoveryMode` and `PeerHint`
//! (see morph-services' network adapter). Adding a peer address is not
//! trust; the roster and `NodeCert` authorize a node.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Environment variable that forces Weirdnet discovery off when set to a
/// truthy value (`1`, `true`, `yes`, `on`).
pub const DISABLE_DISCOVERY_ENV: &str = "MORPH_DISABLE_WEIRDNET_DISCOVERY";
/// Environment variable naming a policy file that can force discovery off.
pub const POLICY_ENV: &str = "MORPH_NETWORK_POLICY";

/// A peer to dial without discovering it: a node id and addresses that reach
/// it. Compatible with weirdnet-net's `PeerHint` JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerEntry {
    pub peer_id: String,
    pub addrs: Vec<String>,
}

/// What Morph stores in `network.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkPrefs {
    /// Whether the user wants Weirdnet discovery on. Default: true.
    #[serde(default = "default_true")]
    pub discovery: bool,
    /// Manually added peers (address hints only).
    #[serde(default)]
    pub peers: Vec<PeerEntry>,
}

fn default_true() -> bool {
    true
}

impl Default for NetworkPrefs {
    fn default() -> Self {
        Self { discovery: true, peers: Vec::new() }
    }
}

/// Why discovery is off, when it is not the user's stored preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryOverride {
    Env,
    Policy,
}

/// Preferences plus the effective discovery flag after admin overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkSettings {
    pub prefs: NetworkPrefs,
    /// `Some` when an admin override forces discovery off.
    pub discovery_override: Option<DiscoveryOverride>,
}

impl NetworkSettings {
    /// Whether discovery should be on (overrides win over the stored preference).
    pub fn discovery_enabled(&self) -> bool {
        self.discovery_override.is_none() && self.prefs.discovery
    }

    pub fn discovery_locked(&self) -> bool {
        self.discovery_override.is_some()
    }

    pub fn peers(&self) -> &[PeerEntry] {
        &self.prefs.peers
    }
}

/// `<home>/network.json`.
pub fn prefs_path(home: impl AsRef<Path>) -> PathBuf {
    home.as_ref().join("network.json")
}

/// `<home>/network.policy.json`.
pub fn policy_path(home: impl AsRef<Path>) -> PathBuf {
    home.as_ref().join("network.policy.json")
}

/// Load stored preferences (defaults if the file is missing) and apply overrides.
///
/// This never creates the home directory.
pub fn load(home: impl AsRef<Path>) -> Result<NetworkSettings, NetworkError> {
    let home = home.as_ref();
    let prefs = match fs::read_to_string(prefs_path(home)) {
        Ok(raw) => serde_json::from_str(&raw)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => NetworkPrefs::default(),
        Err(e) => return Err(e.into()),
    };
    Ok(NetworkSettings { prefs, discovery_override: discovery_override(home) })
}

/// Write the stored preference (not the override). Refuses to store a
/// discovery value of `true` when an override has locked it off — the file
/// may still keep the last preference; only `prefs` is written as given
/// except discovery is forced to `false` when locked, so a future unlock
/// does not surprise a corporate machine by turning discovery back on.
pub fn save(home: impl AsRef<Path>, prefs: &NetworkPrefs) -> Result<(), NetworkError> {
    let home = home.as_ref();
    let mut prefs = prefs.clone();
    if discovery_override(home).is_some() {
        prefs.discovery = false;
    }
    if let Err(e) = fs::create_dir_all(home)
        && e.kind() != std::io::ErrorKind::AlreadyExists
    {
        return Err(e.into());
    }
    let path = prefs_path(home);
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(&prefs)?;
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&body)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn set_discovery(home: impl AsRef<Path>, on: bool) -> Result<NetworkSettings, NetworkError> {
    let home = home.as_ref();
    if discovery_override(home).is_some() {
        return load(home);
    }
    let mut settings = load(home)?;
    settings.prefs.discovery = on;
    save(home, &settings.prefs)?;
    load(home)
}

/// Validate and add a peer. `spec` is a multiaddr that ends in `/p2p/<id>`,
/// or `peer_id` plus one or more addresses (whitespace or comma separated).
pub fn add_peer(home: impl AsRef<Path>, spec: &str) -> Result<NetworkSettings, NetworkError> {
    let home = home.as_ref();
    let entry = parse_peer(spec)?;
    let mut settings = load(home)?;
    if let Some(existing) = settings.prefs.peers.iter_mut().find(|p| p.peer_id == entry.peer_id) {
        for a in entry.addrs {
            if !existing.addrs.contains(&a) {
                existing.addrs.push(a);
            }
        }
    } else {
        settings.prefs.peers.push(entry);
    }
    save(home, &settings.prefs)?;
    load(home)
}

pub fn remove_peer(home: impl AsRef<Path>, peer_id: &str) -> Result<NetworkSettings, NetworkError> {
    let home = home.as_ref();
    let mut settings = load(home)?;
    let id = peer_id.trim();
    settings.prefs.peers.retain(|p| p.peer_id != id);
    save(home, &settings.prefs)?;
    load(home)
}

/// Parse a typed peer: `/ip4/…/p2p/<id>`, or `<id> /ip4/…`, or `<id>,/ip4/…`.
pub fn parse_peer(spec: &str) -> Result<PeerEntry, NetworkError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(NetworkError::Invalid("enter a peer id and an address, or a multiaddr ending in /p2p/<id>".into()));
    }
    let parts: Vec<&str> =
        if spec.starts_with('/') { vec![spec] } else { spec.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).collect() };
    if parts.is_empty() {
        return Err(NetworkError::Invalid("enter a peer id and an address, or a multiaddr ending in /p2p/<id>".into()));
    }
    if parts.len() == 1 && parts[0].starts_with('/') {
        return peer_from_addr(parts[0]);
    }
    let (peer_id, addrs) = if parts[0].starts_with('/') {
        return Err(NetworkError::Invalid("a peer id has to come before extra addresses".into()));
    } else {
        let id = normalize_peer_id(parts[0])?;
        let addrs = if parts.len() == 1 { Vec::new() } else { parts[1..].to_vec() };
        (id, addrs)
    };
    if addrs.is_empty() {
        return Err(NetworkError::Invalid("a peer needs at least one address".into()));
    }
    let mut out = Vec::new();
    for a in addrs {
        let full = normalize_addr(a, Some(&peer_id))?;
        if !out.contains(&full) {
            out.push(full);
        }
    }
    Ok(PeerEntry { peer_id, addrs: out })
}

fn peer_from_addr(addr: &str) -> Result<PeerEntry, NetworkError> {
    let id = addr_peer_id(addr).ok_or_else(|| NetworkError::Invalid(format!("{addr} has no /p2p/ peer id")))?;
    let full = normalize_addr(addr, Some(&id))?;
    Ok(PeerEntry { peer_id: id, addrs: vec![full] })
}

fn normalize_peer_id(s: &str) -> Result<String, NetworkError> {
    let s = s.trim();
    if let Some(id) = addr_peer_id(s) {
        return Ok(id);
    }
    if !is_plausible_peer_id(s) {
        return Err(NetworkError::Invalid(format!("not a peer id: {s}")));
    }
    Ok(s.to_string())
}

fn normalize_addr(addr: &str, expect: Option<&str>) -> Result<String, NetworkError> {
    let addr = addr.trim();
    if !addr.starts_with('/') || addr.len() < 4 {
        return Err(NetworkError::Invalid(format!("{addr} is not a multiaddr")));
    }
    if addr.contains(' ') || !addr[1..].contains('/') {
        return Err(NetworkError::Invalid(format!("{addr} is not a multiaddr")));
    }
    if let Some(found) = addr_peer_id(addr) {
        if let Some(expect) = expect
            && found != expect
        {
            return Err(NetworkError::Invalid(format!("{addr} is for {found}, not {expect}")));
        }
        return Ok(addr.to_string());
    }
    let Some(id) = expect else {
        return Err(NetworkError::Invalid(format!("{addr} has no /p2p/ peer id")));
    };
    Ok(format!("{addr}/p2p/{id}"))
}

fn addr_peer_id(addr: &str) -> Option<String> {
    let needle = "/p2p/";
    let i = addr.rfind(needle)?;
    let rest = &addr[i + needle.len()..];
    let id = rest.split('/').next().unwrap_or("").trim();
    is_plausible_peer_id(id).then(|| id.to_string())
}

/// libp2p peer ids are base58btc, typically starting with `12D3KooW` (Ed25519)
/// or `Qm` (legacy). We only check shape here; weirdnet-net will parse them
/// strictly once Morph depends on a published release that has `PeerHint`.
fn is_plausible_peer_id(s: &str) -> bool {
    let s = s.trim();
    s.len() >= 32 && s.len() <= 128 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn discovery_override(home: &Path) -> Option<DiscoveryOverride> {
    override_from(std::env::var(DISABLE_DISCOVERY_ENV).ok().as_deref(), &policy_files(home))
}

/// Resolve an override from an env value and policy files (missing files are ignored).
fn override_from(env: Option<&str>, policy_files: &[PathBuf]) -> Option<DiscoveryOverride> {
    if env.is_some_and(is_truthy) {
        return Some(DiscoveryOverride::Env);
    }
    if policy_files.iter().any(|p| policy_file_disables(p)) {
        return Some(DiscoveryOverride::Policy);
    }
    None
}

fn is_truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

#[derive(Debug, Deserialize)]
struct PolicyFile {
    #[serde(default)]
    discovery: Option<bool>,
    #[serde(default)]
    disable_discovery: Option<bool>,
}

fn policy_file_disables(path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(path) else { return false };
    let Ok(p) = serde_json::from_str::<PolicyFile>(&raw) else { return false };
    p.disable_discovery == Some(true) || p.discovery == Some(false)
}

fn policy_files(home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(p) = std::env::var(POLICY_ENV) {
        let p = PathBuf::from(p);
        if !p.as_os_str().is_empty() {
            paths.push(p);
        }
    }
    paths.push(policy_path(home));
    #[cfg(unix)]
    {
        paths.push(PathBuf::from("/etc/morph/network.policy.json"));
    }
    paths
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("read or write network settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse network settings: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: &str = "12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB";

    #[test]
    fn missing_file_is_discovery_on_and_no_peers() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load(tmp.path()).unwrap();
        // A process-wide admin env would hide this; CI and the box do not set it.
        if std::env::var(DISABLE_DISCOVERY_ENV).ok().as_deref().is_some_and(is_truthy) {
            assert!(!s.discovery_enabled() && s.discovery_locked());
            return;
        }
        assert!(s.discovery_enabled() && !s.discovery_locked());
        assert!(s.peers().is_empty());
        assert!(!tmp.path().join("network.json").exists());
    }

    #[test]
    fn prefs_round_trip_and_default_missing_discovery_is_on() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        if override_from(std::env::var(DISABLE_DISCOVERY_ENV).ok().as_deref(), &policy_files(home)).is_some() {
            return;
        }
        set_discovery(home, false).unwrap();
        let s = load(home).unwrap();
        assert!(!s.discovery_enabled());
        assert!(!s.prefs.discovery);
        fs::write(prefs_path(home), "{}\n").unwrap();
        assert!(load(home).unwrap().discovery_enabled());
    }

    #[test]
    fn env_and_policy_helpers_force_discovery_off() {
        assert_eq!(override_from(Some("1"), &[]), Some(DiscoveryOverride::Env));
        assert_eq!(override_from(Some("TRUE"), &[]), Some(DiscoveryOverride::Env));
        assert_eq!(override_from(Some("yes"), &[]), Some(DiscoveryOverride::Env));
        assert_eq!(override_from(Some("on"), &[]), Some(DiscoveryOverride::Env));
        assert_eq!(override_from(Some("0"), &[]), None);
        assert_eq!(override_from(Some("false"), &[]), None);
        assert_eq!(override_from(None, &[]), None);

        let tmp = tempfile::tempdir().unwrap();
        let policy = tmp.path().join("corp.json");
        fs::write(&policy, "{\"discovery\": false}\n").unwrap();
        assert_eq!(override_from(None, std::slice::from_ref(&policy)), Some(DiscoveryOverride::Policy));
        fs::write(&policy, "{\"disable_discovery\": true}\n").unwrap();
        assert_eq!(override_from(None, std::slice::from_ref(&policy)), Some(DiscoveryOverride::Policy));
        fs::write(&policy, "{\"discovery\": true}\n").unwrap();
        assert_eq!(override_from(None, &[policy]), None);
    }

    #[test]
    fn home_policy_file_forces_discovery_off_and_locks_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        set_discovery(home, true).unwrap();
        fs::write(policy_path(home), "{\"discovery\": false}\n").unwrap();
        let s = NetworkSettings { prefs: load(home).unwrap().prefs, discovery_override: override_from(None, &[policy_path(home)]) };
        assert!(!s.discovery_enabled());
        assert_eq!(s.discovery_override, Some(DiscoveryOverride::Policy));
        // save() consults real policy files under home, so this locks discovery off in the file.
        let mut prefs = s.prefs.clone();
        prefs.discovery = true;
        save(home, &prefs).unwrap();
        assert!(!load(home).unwrap().prefs.discovery, "save under a policy keeps discovery off");
    }

    #[test]
    fn peers_are_validated_merged_and_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        assert!(parse_peer("").is_err());
        assert!(parse_peer("/ip4/127.0.0.1/tcp/1").is_err());
        assert!(parse_peer("nope /ip4/127.0.0.1/tcp/1").is_err());
        let addr = format!("/ip4/127.0.0.1/tcp/4747/p2p/{PEER}");
        let s = add_peer(home, &addr).unwrap();
        assert_eq!(s.peers().len(), 1);
        assert_eq!(s.peers()[0].peer_id, PEER);
        assert_eq!(s.peers()[0].addrs, vec![addr.clone()]);
        add_peer(home, &format!("{PEER} /ip4/10.0.0.2/tcp/9")).unwrap();
        let s = load(home).unwrap();
        assert_eq!(s.peers().len(), 1);
        assert_eq!(s.peers()[0].addrs.len(), 2);
        remove_peer(home, PEER).unwrap();
        assert!(load(home).unwrap().peers().is_empty());
    }

    #[test]
    fn a_mismatched_peer_in_the_address_is_rejected() {
        let other = "12D3KooWQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ";
        let addr = format!("/ip4/127.0.0.1/tcp/1/p2p/{other}");
        let err = parse_peer(&format!("{PEER} {addr}")).unwrap_err();
        assert!(err.to_string().contains("not"), "{err}");
    }
}
