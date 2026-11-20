//! Login over a QR code: move a profile's identity to another device of the
//! same person (`weirdnet-device-link/v1`, docs/identity-recovery.md §4).
//!
//! The **new device** ([`NewDevice`]) has no identity in the profile yet. It
//! listens on a fresh node key and shows a `weirdnet-morph://link/…` QR. The
//! **existing device** ([`AddDevice`]) unlocks its seed with the profile
//! password, scans or pastes the link and pairs. Both screens then show the
//! same six digits; only after the user confirms them on both does the seed
//! travel, sealed, to the new device, which seals it under its own new
//! password and keeps the node key and the node certificate the user key
//! signed for it:
//!
//! - `identity/node.key`: the node's Ed25519 key, sealed like `user.key`;
//! - `identity/node.cert.json`: its `NodeCert` (public).
use std::fs;
use std::path::{Path, PathBuf};

use weirdnet_device_link::{HostNode, HostStatus, InviteHost, Pairing};
pub use weirdnet_device_link::{RelayProbe, RelayState, relay_groups, relay_states};
use weirdnet_keys::{Key32, SecretString, password_seal};
use weirdnet_net::Keypair;
pub use weirdnet_net::Multiaddr;
pub use weirdnet_net::node::NodeConfig;
pub use weirdnet_net::node::Status as NetStatus;

use crate::profile_identity::{self, ProfileIdentity, Source};
use crate::{Error, Result};

/// The link scheme Morph shows and accepts.
pub const SCHEME: &str = "weirdnet-morph://";

/// Ymir's link scheme: a Ymir node (`ymirctl pool link`) shows a new
/// device's code on it, made by the same weirdnet-device-link with only the
/// prefix changed (the prefix is not signed), so Morph adds it like any
/// device: profile password, then the six-digit check on both sides.
pub const YMIR_SCHEME: &str = "weirdnet-ymir://";

/// The schemes whose new-device codes (`<scheme>link/…`) Morph pairs with.
/// Morph's own offers and invites stay on [`SCHEME`].
pub const OFFER_SCHEMES: &[&str] = &[SCHEME, YMIR_SCHEME];

/// The app that showed a new device's code, by its scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferApp {
    Morph,
    Ymir,
}

/// Which of [`OFFER_SCHEMES`] `text` is a new device's code on (scheme and
/// `link/` in any case), and the code with them in lowercase, as
/// weirdnet-device-link reads it. Only the prefix is looked at here; the
/// pairing checks the rest (encoding, version, signature, size).
pub fn offer_scheme(text: &str) -> Option<(OfferApp, String)> {
    let text = text.trim();
    OFFER_SCHEMES.iter().find_map(|scheme| {
        let prefix = format!("{scheme}link/");
        let head = text.get(..prefix.len())?;
        if !head.eq_ignore_ascii_case(&prefix) {
            return None;
        }
        let app = if *scheme == YMIR_SCHEME { OfferApp::Ymir } else { OfferApp::Morph };
        Some((app, format!("{prefix}{}", &text[prefix.len()..])))
    })
}

/// Whether `text` is a new device's code Morph can pair with (Morph's or
/// Ymir's), by its prefix.
pub fn is_offer(text: &str) -> bool {
    offer_scheme(text).is_some()
}

/// What Add a device calls the new device: its own hint, said to be Ymir
/// when the code came from Ymir and the hint does not say so ("Ymir on
/// gpu-box"). The hint is the new device's own words, from the code.
pub fn device_label(app: OfferApp, hint: &str) -> String {
    let hint = hint.trim();
    match app {
        OfferApp::Morph => hint.to_string(),
        OfferApp::Ymir if hint.is_empty() || hint.eq_ignore_ascii_case("ymir") => "Ymir".to_string(),
        OfferApp::Ymir if hint.to_ascii_lowercase().contains("ymir") => hint.to_string(),
        OfferApp::Ymir => format!("Ymir on {hint}"),
    }
}
use crate::profile_identity::NODE_AAD;

/// The official WeirdBrains Weirdnet relay, built in on every platform: TCP
/// first (UDP is often blocked), then QUIC. Links dial both at once and keep
/// whichever answers first. Addresses it
/// advertises in its info (WebSocket over TLS on 443, once it serves that)
/// are added at run time, in morph-services:
/// <https://relay.net.weirdbrains.com/.well-known/weirdnet-relay.json>.
pub const PUBLIC_RELAYS: &[&str] = &[
    "/dns4/relay.net.weirdbrains.com/tcp/4747/p2p/12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB",
    "/dns4/relay.net.weirdbrains.com/udp/4747/quic-v1/p2p/12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB",
];

/// Where the link listens and dials: TCP and QUIC on every interface, on a
/// port the system picks, plus a circuit reserved on the public relay. The
/// code carries the direct addresses and the circuit, so devices on one
/// network connect directly and devices on different networks (a phone on
/// mobile data, a guest Wi-Fi that isolates clients) connect through the
/// relay. `MORPH_LINK_RELAYS` (comma-separated multiaddrs ending in
/// `/p2p/<relay>`; empty for none) replaces the relay list.
/// Requests and answers are capped at the link limits
/// ([`weirdnet_device_link::reach`]), far below weirdnet-net's defaults.
pub fn net_config() -> NodeConfig {
    weirdnet_device_link::with_link_limits(NodeConfig {
        listen: NodeConfig::default_listen(0),
        relays: link_relays(),
        label: "morph-link".into(),
        ..Default::default()
    })
}

/// The relays links use: [`PUBLIC_RELAYS`], or `MORPH_LINK_RELAYS` when set.
pub fn link_relays() -> Vec<weirdnet_net::Multiaddr> {
    match std::env::var("MORPH_LINK_RELAYS") {
        Ok(v) => v.split(',').filter_map(|a| a.trim().parse().ok()).collect(),
        Err(_) => PUBLIC_RELAYS.iter().filter_map(|a| a.parse().ok()).collect(),
    }
}

/// What the new device's screen should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// Showing the QR; nobody has scanned it yet.
    Waiting,
    /// Compare this code with the other device.
    Paired {
        sas: String,
    },
    /// The identity arrived; confirm the code here (if not yet) and choose a password.
    Received {
        sas: String,
    },
    Done,
    Cancelled,
    Expired,
    Failed(String),
}

impl From<HostStatus> for LinkState {
    fn from(s: HostStatus) -> Self {
        match s {
            HostStatus::Waiting => Self::Waiting,
            HostStatus::Paired { sas } => Self::Paired { sas },
            HostStatus::Received { sas } => Self::Received { sas },
            HostStatus::Done => Self::Done,
            HostStatus::Cancelled => Self::Cancelled,
            HostStatus::Expired => Self::Expired,
            HostStatus::Failed(why) => Self::Failed(why.to_string()),
        }
    }
}

fn link_error(e: weirdnet_device_link::Error) -> Error {
    use weirdnet_device_link::Error as E;
    Error::Message(match e {
        E::OlderCode => "The other device runs an older Morph. Update it to link.".to_string(),
        E::NewerCode => "The other device runs a newer Morph. Update this device to link.".to_string(),
        e => e.to_string(),
    })
}

/// What [`NewDevice::finish`] stored, including the sync types and roster
/// snapshot that rode on the transfer.
#[derive(Debug)]
pub struct LinkedDevice {
    pub identity: ProfileIdentity,
    /// Types the existing device opted in (empty if the sender did not fill them).
    pub sync: Vec<String>,
    /// JSON snapshot of the roster (certs only, no secrets).
    pub settings: String,
}

/// The new device's side of one link. Stops listening when dropped.
pub struct NewDevice {
    host: HostNode,
    node_key: Keypair,
}

impl std::fmt::Debug for NewDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewDevice").field("peer", &self.host.peer_id()).finish_non_exhaustive()
    }
}

impl NewDevice {
    /// Listens and makes a fresh offer; `device_name` is what the other screen
    /// shows ("Link Morph on laptop?").
    pub fn start(device_name: &str, cfg: NodeConfig) -> Result<Self> {
        let node_key = Keypair::generate_ed25519();
        let host = HostNode::start(SCHEME, node_key.clone(), cfg, device_name).map_err(link_error)?;
        Ok(Self { host, node_key })
    }

    /// The text the QR encodes (also shown for pasting).
    pub fn link(&self) -> &str {
        self.host.link()
    }

    pub fn expires_ms(&self) -> i64 {
        self.host.expires_ms()
    }

    pub fn state(&self) -> LinkState {
        self.host.status().into()
    }

    /// The listening node's network status (connections, relay reservations).
    pub fn net_status(&self) -> NetStatus {
        self.host.net_status()
    }

    /// The user tapped "Codes match" here.
    pub fn confirm(&self) {
        self.host.confirm();
    }

    pub fn cancel(&self) {
        self.host.cancel();
    }

    /// Takes the identity that arrived (after [`NewDevice::confirm`]) into
    /// `profile_dir`, sealed under the new `password`. Spends the offer.
    pub fn finish(&self, profile_dir: &Path, password: &SecretString, now_ms: i64) -> Result<LinkedDevice> {
        // Check everything that could still fail before taking: the offer is
        // single use, so a refused password must not spend it.
        profile_identity::check_password(password)?;
        if profile_identity::load(profile_dir)?.is_some() {
            return Err(Error::Message("this profile already has an identity".into()));
        }
        let transfer = self.host.take().map_err(link_error)?;
        let seed = Key32::from_slice(&transfer.seed)?;
        let identity = profile_identity::store(profile_dir, &seed, password, Source::Linked, now_ms)?;
        // The link already checked the cert names this node and the seed's user;
        // keep the node key and cert next to the user key.
        let node_key = self.node_key.to_protobuf_encoding().map_err(|e| Error::Message(format!("node key: {e}")))?;
        let node_key = zeroize::Zeroizing::new(node_key);
        let sealed = password_seal(password, &node_key, NODE_AAD)?;
        let sealed = serde_json::to_vec_pretty(&sealed).map_err(|e| Error::Message(e.to_string()))?;
        let cert = serde_json::to_vec_pretty(&transfer.node_cert).map_err(|e| Error::Message(e.to_string()))?;
        profile_identity::write_private(&node_key_path(profile_dir), &sealed)?;
        profile_identity::write_private(&node_cert_path(profile_dir), &cert)?;
        Ok(LinkedDevice { identity, sync: transfer.sync.clone(), settings: transfer.settings.clone() })
    }
}

pub fn node_key_path(profile_dir: &Path) -> PathBuf {
    profile_identity::node_key_path(profile_dir)
}

pub fn node_cert_path(profile_dir: &Path) -> PathBuf {
    profile_identity::node_cert_path(profile_dir)
}

/// The existing device's side: paired with a scanned link, holding the
/// unlocked seed until the user confirms the code (or drops this).
pub struct AddDevice {
    pairing: Pairing,
    seed: Key32,
    profile_name: String,
    device: String,
}

impl std::fmt::Debug for AddDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddDevice").field("peer", &self.pairing.peer()).finish_non_exhaustive()
    }
}

impl AddDevice {
    /// Checks the profile password first (nothing is dialled with a wrong
    /// one), then reads `link` (Morph's or Ymir's new-device code), dials
    /// the new device and pairs.
    pub fn start(profile_dir: &Path, profile_name: &str, password: &SecretString, link: &str, cfg: NodeConfig) -> Result<Self> {
        let seed = profile_identity::open_seed(profile_dir, password)?;
        let Some((app, link)) = offer_scheme(link) else {
            return Err(Error::Message("That is not a new device's code (weirdnet-morph://link/… or weirdnet-ymir://link/…).".into()));
        };
        let scheme = if app == OfferApp::Ymir { YMIR_SCHEME } else { SCHEME };
        let pairing = Pairing::start(scheme, &link, cfg).map_err(link_error)?;
        let device = device_label(app, pairing.device());
        Ok(Self { pairing, seed, profile_name: profile_name.to_string(), device })
    }

    /// The code to compare with the new device's screen.
    pub fn sas(&self) -> &str {
        self.pairing.sas()
    }

    /// What the new device calls itself ([`device_label`]).
    pub fn device(&self) -> &str {
        &self.device
    }

    /// The pairing node's network status (connections, relay reservations).
    pub fn net_status(&self) -> NetStatus {
        self.pairing.net_status()
    }

    /// The codes match: send the identity, once.
    pub fn send(&mut self) -> Result<()> {
        self.send_with("{}", &[]).map(|_| ())
    }

    /// Send the identity with a roster snapshot and the types to sync.
    /// Returns the new device's `NodeCert`.
    pub fn send_with(&mut self, settings: &str, sync: &[String]) -> Result<weirdnet_net::user::NodeCert> {
        self.pairing.send_with(&self.seed, &self.profile_name, settings, sync).map_err(link_error)
    }

    /// The codes differ, or the user gave up.
    pub fn cancel(&self) -> Result<()> {
        self.pairing.cancel().map_err(link_error)
    }
}

/// The existing device's invite: a QR the new device scans when it is the
/// one with a camera (or the link pasted there). The new device answers with
/// its own offer link, which [`Invite::take_offer`] hands back for
/// [`AddDevice::start`]; the code check is the same.
pub struct Invite(InviteHost);

impl std::fmt::Debug for Invite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invite").finish_non_exhaustive()
    }
}

impl Invite {
    pub fn start(cfg: NodeConfig) -> Result<Self> {
        InviteHost::start(SCHEME, cfg).map(Invite).map_err(link_error)
    }

    pub fn link(&self) -> &str {
        self.0.link()
    }

    pub fn expires_ms(&self) -> i64 {
        self.0.expires_ms()
    }

    pub fn expired(&self) -> bool {
        self.0.expired()
    }

    /// The new device's offer link, once it answered.
    pub fn take_offer(&self) -> Option<String> {
        self.0.take_offer()
    }

    /// The listening node's network status (connections, relay reservations).
    pub fn net_status(&self) -> NetStatus {
        self.0.net_status()
    }
}

/// Whether `text` is an existing device's invite (not a new device's link).
pub fn is_invite(text: &str) -> bool {
    weirdnet_device_link::is_invite(SCHEME, text)
}

/// Whether `text` reads as an existing device's invite for Morph: its shape
/// and version (weirdnet-device-link's `decode_invite`). What an opened
/// `weirdnet-morph://invite/…` link is checked with before it is used.
pub fn check_invite(text: &str) -> Result<()> {
    weirdnet_device_link::decode_invite(SCHEME, text.trim()).map(|_| ()).map_err(link_error)
}

/// The new device answers `invite` with its offer.
pub fn answer_invite(invite: &str, offer_link: &str, cfg: NodeConfig) -> Result<()> {
    weirdnet_device_link::answer_invite(SCHEME, invite.trim(), offer_link, cfg).map_err(link_error)
}

/// Whether `profile_dir` holds a node key that came with a link.
pub fn has_linked_node(profile_dir: &Path) -> bool {
    fs::metadata(node_key_path(profile_dir)).is_ok() && fs::metadata(node_cert_path(profile_dir)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_identity::{create, load, unlock};
    use std::time::{Duration, Instant};

    #[test]
    fn code_version_errors_name_morph() {
        let older = link_error(weirdnet_device_link::Error::OlderCode).to_string();
        let newer = link_error(weirdnet_device_link::Error::NewerCode).to_string();
        assert!(older.contains("older Morph"), "{older}");
        assert!(newer.contains("newer Morph"), "{newer}");
    }

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    #[test]
    fn the_public_relays_are_full_relay_addresses() {
        for a in PUBLIC_RELAYS {
            let m: weirdnet_net::Multiaddr = a.parse().unwrap();
            assert!(a.contains("/p2p/12D3Koo") && weirdnet_net::addr::addr_peer(&m).is_some(), "{a}");
        }
    }

    fn memory() -> NodeConfig {
        NodeConfig { listen: vec!["/memory/0".parse().unwrap()], memory: true, ..Default::default() }
    }

    fn wait_for(new: &NewDevice, want: fn(&LinkState) -> bool) -> LinkState {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = new.state();
            if want(&s) || Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_profile_moves_to_a_new_device_over_a_link() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let fresh = tmp.path().join("fresh");
        let made = create(&old, &pw("old password"), 1).unwrap();

        let new = NewDevice::start("Morph on laptop", memory()).unwrap();
        assert!(new.link().starts_with("weirdnet-morph://link/"));
        assert_eq!(new.state(), LinkState::Waiting);

        let mut add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        assert_eq!(add.device(), "Morph on laptop");
        let LinkState::Paired { sas } = wait_for(&new, |s| matches!(s, LinkState::Paired { .. })) else { panic!("not paired") };
        assert_eq!(sas, add.sas());
        assert_eq!(sas.len(), 6);

        new.confirm();
        add.send().unwrap();
        assert!(matches!(new.state(), LinkState::Received { .. }));
        let linked = new.finish(&fresh, &pw("new password"), 2).unwrap();

        assert_eq!(linked.identity.user_id, made.identity.user_id);
        assert_eq!(linked.identity.source, Source::Linked);
        assert_eq!(load(&fresh).unwrap().unwrap(), linked.identity);
        assert_eq!(unlock(&fresh, &pw("new password")).unwrap().user_id, made.identity.user_id);
        assert!(unlock(&fresh, &pw("old password")).is_err());
        assert!(has_linked_node(&fresh));
        let cert: weirdnet_net::user::NodeCert = serde_json::from_slice(&fs::read(node_cert_path(&fresh)).unwrap()).unwrap();
        assert_eq!(cert.user_id, made.identity.user_id);
        assert_eq!(new.state(), LinkState::Done);
        // Spent: a second take fails, and nothing secret is in the clear.
        assert!(new.finish(&tmp.path().join("again"), &pw("new password"), 3).is_err());
        for entry in fs::read_dir(profile_identity::identity_dir(&fresh)).unwrap() {
            let body = fs::read_to_string(entry.unwrap().path()).unwrap();
            assert!(!body.contains(made.words.as_str()));
            assert!(!body.contains("new password"));
        }
    }

    #[test]
    fn offer_schemes_are_morph_and_ymir_only() {
        assert_eq!(offer_scheme(" weirdnet-morph://link/abc ").unwrap(), (OfferApp::Morph, "weirdnet-morph://link/abc".into()));
        assert_eq!(offer_scheme("weirdnet-ymir://link/abc").unwrap(), (OfferApp::Ymir, "weirdnet-ymir://link/abc".into()));
        assert_eq!(offer_scheme("WEIRDNET-YMIR://Link/AbC").unwrap(), (OfferApp::Ymir, "weirdnet-ymir://link/AbC".into()));
        for not in [
            "weirdnet-ymir://invite/abc",
            "weirdnet-morph://invite/abc",
            "weirdnet://link/abc",
            "weirdnet-bragi://link/abc",
            "ymir://link/a",
            "",
            "weirdnet-ymir://lin",
        ] {
            assert!(!is_offer(not), "{not}");
        }
        // Multi-byte text at the cut does not panic.
        assert!(!is_offer("weirdnet-ymir://linké/x"));
        assert!(!is_offer("ééééééééééééééééééééé"));
    }

    #[test]
    fn a_ymir_code_says_ymir() {
        assert_eq!(device_label(OfferApp::Ymir, "ymir"), "Ymir");
        assert_eq!(device_label(OfferApp::Ymir, ""), "Ymir");
        assert_eq!(device_label(OfferApp::Ymir, "gpu-box"), "Ymir on gpu-box");
        assert_eq!(device_label(OfferApp::Ymir, "Ymir on gpu-box"), "Ymir on gpu-box");
        assert_eq!(device_label(OfferApp::Morph, "Morph on laptop"), "Morph on laptop");
        assert_eq!(device_label(OfferApp::Morph, "ymir"), "ymir", "a Morph code is not called Ymir");
    }

    /// A Ymir node's code, made exactly as `ymirctl pool link` makes it
    /// (weirdnet-device-link's `HostNode` on `weirdnet-ymir://`, hint from
    /// its node name), is added like a Morph device: password, six digits
    /// on both sides, then the identity and a cert for the node's key.
    #[test]
    fn a_ymir_node_links_like_any_device() {
        use weirdnet_device_link::HostNode;
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let made = create(&old, &pw("old password"), 1).unwrap();
        let node_key = Keypair::generate_ed25519();
        let ymir = HostNode::start(YMIR_SCHEME, node_key.clone(), memory(), "gpu-box").unwrap();
        assert!(ymir.link().starts_with("weirdnet-ymir://link/"));
        assert!(!is_invite(ymir.link()));

        // A wrong password never dials Ymir either.
        assert!(AddDevice::start(&old, "Work", &pw("wrong password"), ymir.link(), memory()).is_err());
        assert_eq!(ymir.status(), HostStatus::Waiting);

        let mut add = AddDevice::start(&old, "Work", &pw("old password"), ymir.link(), memory()).unwrap();
        assert_eq!(add.device(), "Ymir on gpu-box");
        let deadline = Instant::now() + Duration::from_secs(10);
        let sas = loop {
            if let HostStatus::Paired { sas } = ymir.status() {
                break sas;
            }
            assert!(Instant::now() < deadline, "Ymir never paired");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(sas, add.sas());
        // Nothing moves until Ymir's side confirms too.
        assert!(ymir.take().is_err());
        ymir.confirm();
        let cert = add.send_with("{}", &["bookmarks".into()]).unwrap();
        assert_eq!(cert.peer_id, node_key.public().to_peer_id().to_string());
        assert_eq!(cert.user_id, made.identity.user_id);
        assert_eq!(cert.label, "gpu-box", "the signed label is Ymir's own hint");
        let t = ymir.take().unwrap();
        assert_eq!(t.node_cert, cert);
        assert_eq!(t.sync, ["bookmarks"]);
    }

    #[test]
    fn a_tampered_or_foreign_ymir_code_is_refused() {
        use weirdnet_device_link::HostNode;
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        create(&old, &pw("old password"), 1).unwrap();
        let ymir = HostNode::start(YMIR_SCHEME, Keypair::generate_ed25519(), memory(), "ymir").unwrap();
        let link = ymir.link().to_string();
        // A flipped character breaks the encoding or the signature.
        let body = &link["weirdnet-ymir://link/".len()..];
        let flip = |c: char| if c == 'A' { 'B' } else { 'A' };
        let mid = body.len() / 2;
        let tampered: String = format!("weirdnet-ymir://link/{}{}{}", &body[..mid], flip(body[mid..].chars().next().unwrap()), &body[mid + 1..]);
        for bad in [tampered, "weirdnet-ymir://link/".into(), "weirdnet-ymir://link/!!!".into(), format!("weirdnet-ymir://link/{}", "A".repeat(5000))]
        {
            assert!(AddDevice::start(&old, "Work", &pw("old password"), &bad, memory()).is_err(), "{bad:.60}");
        }
        // A Ymir invite or bare weirdnet:// is not a new device's code.
        let invite = link.replace("weirdnet-ymir://link/", "weirdnet-ymir://invite/");
        let bare = link.replace("weirdnet-ymir://", "weirdnet://");
        for not in [invite, bare] {
            let e = AddDevice::start(&old, "Work", &pw("old password"), &not, memory()).unwrap_err().to_string();
            assert!(e.contains("not a new device's code"), "{e}");
        }
        assert_eq!(ymir.status(), HostStatus::Waiting);
    }

    #[test]
    fn a_short_password_does_not_spend_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let fresh = tmp.path().join("fresh");
        create(&old, &pw("old password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        let mut add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        wait_for(&new, |s| matches!(s, LinkState::Paired { .. }));
        new.confirm();
        add.send().unwrap();
        assert!(new.finish(&fresh, &pw("short"), 2).is_err());
        assert!(matches!(new.state(), LinkState::Received { .. }));
        assert_eq!(new.finish(&fresh, &pw("long enough"), 2).unwrap().identity.source, Source::Linked);
    }

    #[test]
    fn a_wrong_password_never_dials() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        create(&old, &pw("old password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        assert!(AddDevice::start(&old, "Work", &pw("wrong password"), new.link(), memory()).is_err());
        assert_eq!(new.state(), LinkState::Waiting);
    }

    #[test]
    fn nothing_is_taken_without_confirming_here() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        create(&old, &pw("old password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        let mut add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        wait_for(&new, |s| matches!(s, LinkState::Paired { .. }));
        add.send().unwrap();
        let fresh = tmp.path().join("fresh");
        assert!(new.finish(&fresh, &pw("new password"), 2).is_err());
        assert!(load(&fresh).unwrap().is_none());
    }

    #[test]
    fn a_cancel_from_the_existing_device_ends_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        create(&old, &pw("old password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        let add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        add.cancel().unwrap();
        assert_eq!(wait_for(&new, |s| *s == LinkState::Cancelled), LinkState::Cancelled);
    }

    #[test]
    fn a_profile_with_an_identity_is_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let fresh = tmp.path().join("fresh");
        create(&old, &pw("old password"), 1).unwrap();
        let other = create(&fresh, &pw("fresh password"), 1).unwrap();
        let new = NewDevice::start("Morph", memory()).unwrap();
        let mut add = AddDevice::start(&old, "Work", &pw("old password"), new.link(), memory()).unwrap();
        wait_for(&new, |s| matches!(s, LinkState::Paired { .. }));
        new.confirm();
        add.send().unwrap();
        assert!(new.finish(&fresh, &pw("new password"), 2).is_err());
        assert_eq!(load(&fresh).unwrap().unwrap().user_id, other.identity.user_id);
    }
}
