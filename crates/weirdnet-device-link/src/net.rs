//! Running a link over Weirdnet (`/weirdnet/link/1`, Noise-authenticated
//! request/response): the new device listens, the existing device dials it.
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use weirdnet_keys::Key32;
use weirdnet_net::node::{Handler, Node, NodeConfig};
use weirdnet_net::{Keypair, PeerId, now_ms};

use crate::session::{Host, HostStatus, Sender, make_transfer};
use crate::wire::{LinkReply, LinkRequest, Transfer};
use crate::{Error, LINK_PROFILE, Result, decode_link, offer_addrs};

/// How long the new device waits for its listeners before giving up.
const LISTEN_WAIT: Duration = Duration::from_secs(5);

struct HostHandler(Arc<OnceLock<Arc<Host>>>);

impl Handler<LinkRequest, LinkReply> for HostHandler {
    fn request(&self, from: &PeerId, req: LinkRequest) -> LinkReply {
        match self.0.get() {
            Some(host) => host.handle(from, req, now_ms()),
            None => LinkReply::refused("not ready"),
        }
    }
}

/// The new device: a node on its own key, listening, with the offer it shows.
/// Stops listening when dropped.
pub struct HostNode {
    node: Node<LinkRequest, LinkReply>,
    host: Arc<Host>,
}

impl HostNode {
    /// Starts listening as `node_key` and makes a fresh offer naming the
    /// addresses the node ended up on.
    pub fn start(scheme: &str, node_key: Keypair, cfg: NodeConfig, hint: &str) -> Result<Self> {
        let slot = Arc::new(OnceLock::new());
        let node = Node::start(&LINK_PROFILE, node_key.clone(), cfg, Arc::new(HostHandler(slot.clone())))?;
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
        let host = Arc::new(Host::new(scheme, &node_key, addrs, hint, now_ms())?);
        let _ = slot.set(host.clone());
        Ok(HostNode { node, host })
    }

    pub fn link(&self) -> &str {
        self.host.link()
    }

    pub fn expires_ms(&self) -> i64 {
        self.host.expires_ms()
    }

    pub fn status(&self) -> HostStatus {
        self.host.status(now_ms())
    }

    pub fn confirm(&self) {
        self.host.confirm();
    }

    pub fn take(&self) -> Result<Transfer> {
        self.host.take()
    }

    pub fn cancel(&self) {
        self.host.cancel();
    }

    pub fn peer_id(&self) -> PeerId {
        self.node.peer_id()
    }
}

impl Drop for HostNode {
    fn drop(&mut self) {
        self.host.cancel();
        self.node.halt();
    }
}

/// The existing device, paired with a scanned offer, waiting for the user to
/// compare codes. Holds a short-lived node on a throwaway key.
pub struct Pairing {
    node: Node<LinkRequest, LinkReply>,
    sender: Sender,
    sent: bool,
}

struct NoRequests;

impl Handler<LinkRequest, LinkReply> for NoRequests {
    fn request(&self, _from: &PeerId, _req: LinkRequest) -> LinkReply {
        LinkReply::refused("this device is not offering a link")
    }
}

impl Pairing {
    /// Reads `link`, dials the new device and pairs. On success both screens
    /// show [`Pairing::sas`].
    pub fn start(scheme: &str, link: &str, cfg: NodeConfig) -> Result<Self> {
        let offer = decode_link(scheme, link)?;
        let (sender, hello) = Sender::start(offer, now_ms())?;
        let node = Node::start(&LINK_PROFILE, Keypair::generate_ed25519(), cfg, Arc::new(NoRequests))?;
        let pairing = Pairing { node, sender, sent: false };
        pairing.ask(hello)?;
        Ok(pairing)
    }

    fn ask(&self, req: LinkRequest) -> Result<()> {
        let offer = self.sender.offer();
        match self.node.request(offer.peer(), &offer.dial_addrs(), req)? {
            LinkReply::Ok => Ok(()),
            LinkReply::Refused { reason } => Err(Error::Refused(reason)),
        }
    }

    /// The code to compare with the new device's screen.
    pub fn sas(&self) -> &str {
        self.sender.sas()
    }

    /// What the new device calls itself.
    pub fn device(&self) -> &str {
        &self.sender.offer().offer.hint
    }

    pub fn peer(&self) -> PeerId {
        self.sender.offer().peer()
    }

    /// The user confirmed the codes match: send the identity, once.
    pub fn send(&mut self, seed: &Key32, profile_name: &str, settings: &str) -> Result<()> {
        if self.sent {
            return Err(Error::Invalid("already sent"));
        }
        let t = make_transfer(seed, profile_name, settings, self.sender.offer(), now_ms())?;
        self.ask(self.sender.transfer(&t)?)?;
        self.sent = true;
        Ok(())
    }

    /// The codes differ (or the user gave up): tell the new device.
    pub fn cancel(&self) -> Result<()> {
        self.ask(self.sender.cancel())
    }
}

impl Drop for Pairing {
    fn drop(&mut self) {
        self.node.halt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEME: &str = "weirdnet-morph://";

    fn memory() -> NodeConfig {
        NodeConfig { listen: vec!["/memory/0".parse().unwrap()], memory: true, ..Default::default() }
    }

    fn dial_only() -> NodeConfig {
        NodeConfig { memory: true, ..Default::default() }
    }

    #[test]
    fn an_identity_moves_over_the_network_after_both_confirm() {
        let new_device = HostNode::start(SCHEME, Keypair::generate_ed25519(), memory(), "Morph on desk").unwrap();
        assert_eq!(new_device.status(), HostStatus::Waiting);
        let mut phone = Pairing::start(SCHEME, new_device.link(), dial_only()).unwrap();
        assert_eq!(phone.device(), "Morph on desk");
        assert_eq!(phone.peer(), new_device.peer_id());
        let HostStatus::Paired { sas } = new_device.status() else { panic!("not paired") };
        assert_eq!(sas, phone.sas());

        let seed = Key32::random().unwrap();
        new_device.confirm();
        phone.send(&seed, "Work", "{}").unwrap();
        assert!(phone.send(&seed, "Work", "{}").is_err(), "single send");
        let t = new_device.take().unwrap();
        assert_eq!(t.seed, seed.0.to_vec());
        assert_eq!(t.node_cert.peer_id, new_device.peer_id().to_string());
        assert_eq!(new_device.status(), HostStatus::Done);
    }

    #[test]
    fn a_cancel_from_the_phone_ends_the_offer() {
        let new_device = HostNode::start(SCHEME, Keypair::generate_ed25519(), memory(), "desk").unwrap();
        let phone = Pairing::start(SCHEME, new_device.link(), dial_only()).unwrap();
        phone.cancel().unwrap();
        assert_eq!(new_device.status(), HostStatus::Cancelled);
        assert!(matches!(Pairing::start(SCHEME, new_device.link(), dial_only()), Err(Error::Refused(_))));
    }
}
