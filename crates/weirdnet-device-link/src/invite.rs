//! The other direction: the **existing device** shows an invite as a QR code
//! or link (`<scheme>invite/…`) and the **new device** scans it. The invite
//! is only a meeting point: the new device answers it with its own signed
//! [`Offer`](crate::Offer) link over `/weirdnet/link-invite/1`, and from there
//! the link runs exactly as before: the existing device dials the offer,
//! both screens show the same six-digit code, and nothing moves until the
//! user confirms it on both. An invite holds no secret; whoever answers it
//! still has to show the same code.
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use weirdnet_keys::random_bytes;
use weirdnet_net::node::{Handler, Node, NodeConfig};
use weirdnet_net::profile::{Profile, WEIRDNET};
use weirdnet_net::{Keypair, Multiaddr, PeerId, now_ms};

use crate::compact::{Bytes, pack_addrs, unpack_addrs};
use crate::net::listen_addrs;
use crate::offer::{MAX_ADDRS, cbor, older_or_damaged, version_error};
use crate::wire::LinkReply;
use crate::{Error, OFFER_TTL_MS, Result, VERSION, decode_link};

/// The invite's own stream id: offers are pushed here, not on `/weirdnet/link/1`.
pub static INVITE_PROFILE: Profile = Profile { protocol: "/weirdnet/link-invite/1", thread: "weirdnet-link-invite", ..WEIRDNET };

/// Longest invite text accepted.
const MAX_INVITE: usize = 2048;

/// What the existing device shows. On the wire (v2) it is a CBOR array: the
/// peer id as bytes, addresses packed ([`crate::compact`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "InviteWire", try_from = "InviteWire")]
pub struct Invite {
    pub v: u8,
    /// The listening node's peer id (a throwaway key).
    pub peer: String,
    /// Where it listens, without the `/p2p/` suffix.
    pub addrs: Vec<String>,
    /// 16 random bytes the answer must carry.
    #[serde(with = "crate::bytes")]
    pub token: Vec<u8>,
    /// Unix ms after which the invite is dead.
    pub expires_ms: i64,
}

#[derive(Serialize, Deserialize)]
struct InviteWire(u8, Bytes, Vec<Bytes>, Bytes, i64);

impl From<Invite> for InviteWire {
    fn from(i: Invite) -> Self {
        let peer = i.peer.parse::<PeerId>().map(|p| p.to_bytes()).unwrap_or_default();
        InviteWire(i.v, Bytes(peer), pack_addrs(&i.addrs), Bytes(i.token), i.expires_ms)
    }
}

impl TryFrom<InviteWire> for Invite {
    type Error = &'static str;
    fn try_from(InviteWire(v, peer, addrs, token, expires_ms): InviteWire) -> std::result::Result<Self, &'static str> {
        let peer = PeerId::from_bytes(&peer.0).map_err(|_| "bad peer id")?.to_string();
        Ok(Invite { v, peer, addrs: unpack_addrs(&addrs), token: token.0, expires_ms })
    }
}

/// The new device's answer: its offer link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InviteRequest {
    Offer {
        #[serde(with = "crate::bytes")]
        token: Vec<u8>,
        link: String,
    },
}

/// `<scheme>invite/<base64url CBOR>`.
pub fn encode_invite(scheme: &str, invite: &Invite) -> Result<String> {
    Ok(format!("{scheme}invite/{}", URL_SAFE_NO_PAD.encode(cbor(invite)?)))
}

/// Reads an invite and checks its shape (expiry is the caller's, as for offers).
pub fn decode_invite(scheme: &str, text: &str) -> Result<Invite> {
    let text = text.trim();
    if text.len() > MAX_INVITE {
        return Err(Error::Invalid("the link is too long"));
    }
    let body = text.strip_prefix(&format!("{scheme}invite/")).ok_or(Error::Invalid("not a device invite"))?;
    let blob = URL_SAFE_NO_PAD.decode(body).map_err(|_| Error::Invalid("the link is damaged"))?;
    let invite: Invite = cbor4ii::serde::from_slice(&blob).map_err(|_| older_or_damaged(&blob))?;
    if invite.v != VERSION {
        return Err(version_error(invite.v));
    }
    if invite.token.len() != 16 || invite.addrs.is_empty() || invite.addrs.len() > MAX_ADDRS || invite.peer.parse::<PeerId>().is_err() {
        return Err(Error::Invalid("the link is damaged"));
    }
    Ok(invite)
}

/// Whether `text` is an invite (rather than an offer) for `scheme`.
pub fn is_invite(scheme: &str, text: &str) -> bool {
    text.trim().starts_with(&format!("{scheme}invite/"))
}

struct Inbox {
    scheme: String,
    token: Vec<u8>,
    expires_ms: i64,
    offer: Mutex<Option<String>>,
}

impl Handler<InviteRequest, LinkReply> for Inbox {
    fn request(&self, _from: &PeerId, req: InviteRequest) -> LinkReply {
        let InviteRequest::Offer { token, link } = req;
        if token != self.token {
            return LinkReply::refused("not this invite");
        }
        if now_ms() > self.expires_ms {
            return LinkReply::refused("this invite expired");
        }
        if decode_link(&self.scheme, &link).is_err() {
            return LinkReply::refused("not a device link");
        }
        let mut offer = self.offer.lock().unwrap_or_else(|p| p.into_inner());
        if offer.is_some() {
            return LinkReply::refused("this invite was already answered");
        }
        *offer = Some(link);
        LinkReply::Ok
    }
}

/// The existing device, listening for an answer to its invite. Stops when
/// dropped.
pub struct InviteHost {
    node: Node<InviteRequest, LinkReply>,
    inbox: Arc<Inbox>,
    link: String,
}

impl InviteHost {
    pub fn start(scheme: &str, cfg: NodeConfig) -> Result<Self> {
        let token: [u8; 16] = random_bytes()?;
        let expires_ms = now_ms() + OFFER_TTL_MS;
        let inbox = Arc::new(Inbox { scheme: scheme.to_string(), token: token.to_vec(), expires_ms, offer: Mutex::new(None) });
        let key = Keypair::generate_ed25519();
        let peer = key.public().to_peer_id();
        let want_relay = !cfg.relays.is_empty();
        let node = Node::start(&INVITE_PROFILE, key, cfg, inbox.clone())?;
        let addrs = listen_addrs(&node, want_relay).ok_or_else(|| Error::Net("network: could not listen for the other device".into()))?;
        let invite = Invite { v: VERSION, peer: peer.to_string(), addrs, token: token.to_vec(), expires_ms };
        let link = encode_invite(scheme, &invite)?;
        Ok(InviteHost { node, inbox, link })
    }

    /// The text to show as a QR code and to copy.
    pub fn link(&self) -> &str {
        &self.link
    }

    pub fn expires_ms(&self) -> i64 {
        self.inbox.expires_ms
    }

    pub fn expired(&self) -> bool {
        now_ms() > self.inbox.expires_ms
    }

    /// The offer link the new device sent, once (then pair with it as if it
    /// had been scanned).
    pub fn take_offer(&self) -> Option<String> {
        self.inbox.offer.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    /// The node's network status (connections, relay reservations).
    pub fn net_status(&self) -> weirdnet_net::node::Status {
        self.node.status()
    }
}

impl Drop for InviteHost {
    fn drop(&mut self) {
        self.node.halt();
    }
}

/// The new device: answer `invite` with its own `offer_link`.
pub fn answer_invite(scheme: &str, invite: &str, offer_link: &str, cfg: NodeConfig) -> Result<()> {
    let invite = decode_invite(scheme, invite)?;
    if now_ms() > invite.expires_ms {
        return Err(Error::Expired);
    }
    let peer: PeerId = invite.peer.parse().map_err(|_| Error::Invalid("the link is damaged"))?;
    let addrs: Vec<Multiaddr> = invite.addrs.iter().filter_map(|a| format!("{a}/p2p/{peer}").parse().ok()).collect();
    let node = Node::<InviteRequest, LinkReply>::start(&INVITE_PROFILE, Keypair::generate_ed25519(), cfg, Arc::new(NoAnswers))?;
    let reply = node.request(peer, &addrs, InviteRequest::Offer { token: invite.token, link: offer_link.to_string() });
    node.halt();
    match reply? {
        LinkReply::Ok => Ok(()),
        LinkReply::Refused { reason } => Err(Error::Refused(reason)),
    }
}

struct NoAnswers;

impl Handler<InviteRequest, LinkReply> for NoAnswers {
    fn request(&self, _from: &PeerId, _req: InviteRequest) -> LinkReply {
        LinkReply::refused("this device is not inviting")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostNode, HostStatus, Pairing};
    use weirdnet_keys::Key32;

    const SCHEME: &str = "weirdnet-morph://";

    fn memory() -> NodeConfig {
        NodeConfig { listen: vec!["/memory/0".parse().unwrap()], memory: true, ..Default::default() }
    }

    fn dial_only() -> NodeConfig {
        NodeConfig { memory: true, ..Default::default() }
    }

    #[test]
    fn an_invite_round_trips_and_rejects_damage() {
        let invite = Invite {
            v: VERSION,
            peer: Keypair::generate_ed25519().public().to_peer_id().to_string(),
            addrs: vec!["/ip4/192.168.1.2/udp/4001/quic-v1".into()],
            token: vec![7; 16],
            expires_ms: 1,
        };
        let text = encode_invite(SCHEME, &invite).unwrap();
        assert!(is_invite(SCHEME, &text));
        assert!(!is_invite(SCHEME, "weirdnet-morph://link/abc"));
        assert_eq!(decode_invite(SCHEME, &text).unwrap(), invite);
        assert!(decode_invite(SCHEME, &text[..text.len() - 4]).is_err());
        assert!(decode_invite(SCHEME, "weirdnet-morph://link/abc").is_err());
        assert!(decode_invite(SCHEME, &"x".repeat(5000)).is_err());
    }

    /// What my-desktop listened on (LAN, Tailscale, Docker, IPv6, the
    /// public relay's circuit), as libp2p reports it.
    fn realistic_reachable(peer: &PeerId) -> Vec<String> {
        let relay = crate::KNOWN_RELAYS[0];
        [
            "/ip4/192.168.50.2/tcp/41234".to_string(),
            "/ip4/192.168.50.2/udp/41234/quic-v1".into(),
            "/ip4/100.101.102.103/tcp/41234".into(),
            "/ip4/100.101.102.103/udp/41234/quic-v1".into(),
            "/ip4/172.17.0.1/tcp/41234".into(),
            "/ip4/127.0.0.1/tcp/41234".into(),
            "/ip6/fe80::1/tcp/41235".into(),
            "/ip6/2600:1700:abcd:1234:5678:9abc:def0:1234/tcp/41235".into(),
            format!("{relay}/p2p-circuit"),
            format!("{relay}/p2p-circuit/p2p/{peer}"),
        ]
        .to_vec()
    }

    /// A phone has to read the code off a laptop screen: v1 made a version 18
    /// QR; v2 is about 110 bytes of CBOR (version 9 at M).
    #[test]
    fn a_realistic_invite_is_small_and_a_low_qr_version() {
        let key = Keypair::generate_ed25519();
        let peer = key.public().to_peer_id();
        let addrs = crate::offer_addrs(&realistic_reachable(&peer));
        assert_eq!(addrs.len(), 4, "{addrs:?}");
        assert_eq!(addrs[0], "/ip4/192.168.50.2/tcp/41234", "the LAN first");
        assert_eq!(addrs[1], "/ip4/100.101.102.103/tcp/41234", "another host before another transport");
        assert!(addrs.iter().all(|a| !a.contains("172.17.") && !a.contains("127.0.0.1") && !a.contains("fe80")));
        let invite = Invite { v: VERSION, peer: peer.to_string(), addrs, token: vec![9; 16], expires_ms: now_ms() + OFFER_TTL_MS };
        let raw = cbor(&invite).unwrap();
        assert!(raw.len() < 120, "{} bytes", raw.len());
        let text = encode_invite(SCHEME, &invite).unwrap();
        let qr = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M).unwrap();
        assert!(matches!(qr.version(), qrcode::Version::Normal(v) if v <= 9), "{:?} for {} chars", qr.version(), text.len());
        println!("invite: {} bytes CBOR, {} chars, QR {:?} at M", raw.len(), text.len(), qr.version());
        assert_eq!(decode_invite(SCHEME, &text).unwrap(), invite);
    }

    /// The built-in relay travels as one byte and the reader rebuilds its
    /// full circuit (relay address and peer id) from its own list.
    #[test]
    fn the_known_relay_circuit_is_rebuilt_by_the_reader() {
        let key = Keypair::generate_ed25519();
        let circuit = format!("{}/p2p-circuit", crate::KNOWN_RELAYS[0]);
        let invite =
            Invite { v: VERSION, peer: key.public().to_peer_id().to_string(), addrs: vec![circuit.clone()], token: vec![1; 16], expires_ms: 5 };
        let raw = cbor(&invite).unwrap();
        assert!(!raw.windows(4).any(|w| w == b"dns4" || w == b"12D3"), "no relay text on the wire");
        let read = decode_invite(SCHEME, &encode_invite(SCHEME, &invite).unwrap()).unwrap();
        assert_eq!(read.addrs, vec![circuit]);
    }

    /// A v1 code (a CBOR map) says to update rather than "damaged".
    #[test]
    fn an_old_invite_says_update() {
        #[derive(Serialize)]
        struct V1 {
            v: u8,
            peer: String,
        }
        let blob = cbor(&V1 { v: 1, peer: "x".into() }).unwrap();
        let text = format!("{SCHEME}invite/{}", URL_SAFE_NO_PAD.encode(blob));
        let err = decode_invite(SCHEME, &text).unwrap_err().to_string();
        assert!(err.contains("older Morph"), "{err}");
    }

    #[test]
    fn the_new_device_scans_the_invite_and_the_identity_moves_after_both_confirm() {
        // Existing device: shows the invite.
        let existing = InviteHost::start(SCHEME, memory()).unwrap();
        assert!(existing.take_offer().is_none());
        // New device: scans it and answers with its own offer.
        let new_device = HostNode::start(SCHEME, Keypair::generate_ed25519(), memory(), "Morph on desk").unwrap();
        answer_invite(SCHEME, existing.link(), new_device.link(), dial_only()).unwrap();
        // A second answer (anyone else's) is refused.
        let other = HostNode::start(SCHEME, Keypair::generate_ed25519(), memory(), "intruder").unwrap();
        assert!(matches!(answer_invite(SCHEME, existing.link(), other.link(), dial_only()), Err(Error::Refused(_))));
        // Existing device: pairs with the offer it got, as if scanned.
        let offer = existing.take_offer().unwrap();
        assert!(existing.take_offer().is_none(), "taken once");
        let mut pairing = Pairing::start(SCHEME, &offer, dial_only()).unwrap();
        assert_eq!(pairing.device(), "Morph on desk");
        let HostStatus::Paired { sas } = new_device.status() else { panic!("not paired") };
        assert_eq!(sas, pairing.sas());
        let seed = Key32::random().unwrap();
        new_device.confirm();
        pairing.send(&seed, "Work", "{}").unwrap();
        assert_eq!(new_device.take().unwrap().seed, seed.0.to_vec());
    }

    #[test]
    fn a_wrong_token_or_a_non_link_is_refused() {
        let existing = InviteHost::start(SCHEME, memory()).unwrap();
        let mut invite = decode_invite(SCHEME, existing.link()).unwrap();
        invite.token = vec![0; 16];
        let forged = encode_invite(SCHEME, &invite).unwrap();
        let new_device = HostNode::start(SCHEME, Keypair::generate_ed25519(), memory(), "desk").unwrap();
        assert!(matches!(answer_invite(SCHEME, &forged, new_device.link(), dial_only()), Err(Error::Refused(_))));
        assert!(matches!(answer_invite(SCHEME, existing.link(), "hello", dial_only()), Err(Error::Refused(_))));
        assert!(existing.take_offer().is_none());
    }
}
