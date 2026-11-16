//! The other direction: the **existing device** shows an invite as a QR code
//! or link (`<scheme>invite/…`) and the **new device** scans it. The invite
//! is only a meeting point: the new device answers it with its own signed
//! [`Offer`](crate::Offer) link over `/weirdnet/link-invite/1`, and from there
//! the link runs exactly as before: the existing device dials the offer,
//! both screens show the same six-digit code, and nothing moves until the
//! user confirms it on both. An invite holds no secret; whoever answers it
//! still has to show the same code.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use weirdnet_keys::random_bytes;
use weirdnet_net::node::{Handler, Node, NodeConfig};
use weirdnet_net::profile::{Profile, WEIRDNET};
use weirdnet_net::{Keypair, Multiaddr, PeerId, now_ms};

use crate::offer::{MAX_ADDRS, cbor};
use crate::wire::LinkReply;
use crate::{Error, OFFER_TTL_MS, Result, VERSION, decode_link, offer_addrs};

/// The invite's own stream id: offers are pushed here, not on `/weirdnet/link/1`.
pub static INVITE_PROFILE: Profile = Profile { protocol: "/weirdnet/link-invite/1", thread: "weirdnet-link-invite", ..WEIRDNET };

/// How long the existing device waits for its listeners.
const LISTEN_WAIT: Duration = Duration::from_secs(5);
/// Longest invite text accepted.
const MAX_INVITE: usize = 2048;

/// What the existing device shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    let invite: Invite = cbor4ii::serde::from_slice(&blob).map_err(|_| Error::Invalid("the link is damaged"))?;
    if invite.v != VERSION {
        return Err(Error::Invalid("this link is from a newer version"));
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
        let node = Node::start(&INVITE_PROFILE, key, cfg, inbox.clone())?;
        let deadline = Instant::now() + LISTEN_WAIT;
        let addrs = loop {
            let addrs = offer_addrs(&node.reachable_addrs());
            if !addrs.is_empty() {
                break addrs;
            }
            if Instant::now() > deadline {
                node.halt();
                return Err(Error::Net("network: could not listen for the other device".into()));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
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
