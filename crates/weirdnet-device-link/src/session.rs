//! Both sides of a link, without the network: key agreement, SAS, the sealed
//! transfer and the offer's single-use rules.
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use weirdnet_keys::exchange::{ExchangeSecret, derive};
use weirdnet_keys::{Key32, open, random_bytes, seal};
use weirdnet_net::profile::WEIRDNET;
use weirdnet_net::user::user_id;
use weirdnet_net::{Keypair, PeerId};
use zeroize::Zeroizing;

use crate::offer::{Offer, VerifiedOffer, cbor, encode_link};
use crate::wire::{LinkReply, LinkRequest, Transfer};
use crate::{Error, MAX_ATTEMPTS, OFFER_TTL_MS, Result, VERSION};

const TRANSCRIPT_DOMAIN: &[u8] = b"weirdnet-device-link/v1";
const KEY_INFO: &[u8] = b"weirdnet-device-link/v1/key";
const SAS_INFO: &[u8] = b"weirdnet-device-link/v1/sas";
const TRANSFER_AAD: &[u8] = b"weirdnet-device-link/v1/transfer\n";

fn transcript(offer: &[u8], eph_existing: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(TRANSCRIPT_DOMAIN);
    h.update(offer);
    h.update(eph_existing);
    h.finalize().into()
}

/// The session key and the 6-digit code both screens show.
fn session_keys(shared: &[u8; 32], transcript: &[u8; 32]) -> Result<(Key32, String)> {
    let (key, sas) = derive(shared, transcript, KEY_INFO, SAS_INFO)?;
    Ok((key, format!("{:06}", u64::from_be_bytes(sas) % 1_000_000)))
}

fn aad(transcript: &[u8; 32]) -> Vec<u8> {
    [TRANSFER_AAD, transcript.as_slice()].concat()
}

/// The 32-byte seed's Ed25519 user key.
pub(crate) fn user_key(seed: &[u8]) -> Result<Keypair> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    if seed.len() != 32 {
        return Err(Error::Invalid("the identity did not check out"));
    }
    bytes.copy_from_slice(seed);
    Keypair::ed25519_from_bytes(bytes.as_mut()).map_err(|_| Error::Invalid("the identity did not check out"))
}

/// What the new device's screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStatus {
    /// Showing the code; nobody has scanned it yet.
    Waiting,
    /// Paired: compare this code with the other device.
    Paired {
        sas: String,
    },
    /// The identity arrived (after the other device confirmed the code).
    Received {
        sas: String,
    },
    /// Taken into this device; the offer is spent.
    Done,
    Cancelled,
    Expired,
    Failed(&'static str),
}

struct Paired {
    peer: PeerId,
    key: Key32,
    transcript: [u8; 32],
    sas: String,
}

struct HostInner {
    attempts: u32,
    paired: Option<Paired>,
    received: Option<Transfer>,
    confirmed: bool,
    end: Option<HostStatus>,
}

/// The new device's side: owns the ephemeral secret behind the offer.
pub struct Host {
    secret: ExchangeSecret,
    offer: VerifiedOffer,
    link: String,
    inner: Mutex<HostInner>,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host").field("peer", &self.offer.peer()).finish_non_exhaustive()
    }
}

impl Host {
    /// A fresh offer for `node_key` at `addrs`, valid for [`OFFER_TTL_MS`].
    pub fn new(scheme: &str, node_key: &Keypair, addrs: Vec<String>, hint: &str, now_ms: i64) -> Result<Self> {
        let secret = ExchangeSecret::generate();
        let nonce: [u8; 16] = random_bytes()?;
        let mut hint = hint.to_string();
        while hint.len() > crate::offer::MAX_HINT {
            hint.pop();
        }
        let offer = Offer {
            v: VERSION,
            node: node_key.public().encode_protobuf(),
            eph: secret.public().to_vec(),
            nonce: nonce.to_vec(),
            addrs,
            expires_ms: now_ms + OFFER_TTL_MS,
            hint,
        };
        let (link, offer) = encode_link(scheme, &offer, node_key)?;
        let inner = HostInner { attempts: 0, paired: None, received: None, confirmed: false, end: None };
        Ok(Host { secret, offer, link, inner: Mutex::new(inner) })
    }

    /// The link text (what the QR code encodes).
    pub fn link(&self) -> &str {
        &self.link
    }

    pub fn expires_ms(&self) -> i64 {
        self.offer.offer.expires_ms
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HostInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn status(&self, now_ms: i64) -> HostStatus {
        let mut s = self.lock();
        self.expire(&mut s, now_ms);
        if let Some(end) = &s.end {
            return end.clone();
        }
        match (&s.paired, &s.received) {
            (Some(p), Some(_)) => HostStatus::Received { sas: p.sas.clone() },
            (Some(p), None) => HostStatus::Paired { sas: p.sas.clone() },
            _ => HostStatus::Waiting,
        }
    }

    fn expire(&self, s: &mut HostInner, now_ms: i64) {
        // An identity that already arrived stays takeable; the offer only had to
        // be alive for the pairing and the transfer.
        if s.end.is_none() && s.received.is_none() && self.offer.expired(now_ms) {
            s.end = Some(HostStatus::Expired);
            s.paired = None;
        }
    }

    /// Answers a request from `from` (the peer id Noise authenticated).
    pub fn handle(&self, from: &PeerId, req: LinkRequest, now_ms: i64) -> LinkReply {
        let mut s = self.lock();
        self.expire(&mut s, now_ms);
        if let Some(end) = &s.end {
            return LinkReply::refused(match end {
                HostStatus::Expired => "the code expired",
                HostStatus::Done => "the code was already used",
                _ => "the code is no longer valid",
            });
        }
        let nonce = match &req {
            LinkRequest::Hello { nonce, .. } | LinkRequest::Transfer { nonce, .. } | LinkRequest::Cancel { nonce } => nonce,
        };
        if *nonce != self.offer.offer.nonce {
            return LinkReply::refused("unknown code");
        }
        match req {
            LinkRequest::Hello { eph, .. } => {
                if s.received.is_some() {
                    return LinkReply::refused("the code was already used");
                }
                s.attempts += 1;
                if s.attempts > MAX_ATTEMPTS {
                    s.end = Some(HostStatus::Failed("too many pairing attempts"));
                    s.paired = None;
                    return LinkReply::refused("too many pairing attempts");
                }
                let Ok(eph) = <[u8; 32]>::try_from(eph.as_slice()) else {
                    return LinkReply::refused("bad key");
                };
                let Some(shared) = self.secret.agree(&eph) else {
                    return LinkReply::refused("bad key");
                };
                let transcript = transcript(&self.offer.bytes, &eph);
                match session_keys(&shared, &transcript) {
                    Ok((key, sas)) => {
                        s.paired = Some(Paired { peer: *from, key, transcript, sas });
                        s.confirmed = false;
                        LinkReply::Ok
                    }
                    Err(_) => LinkReply::refused("bad key"),
                }
            }
            LinkRequest::Transfer { sealed, .. } => {
                let Some(p) = s.paired.as_ref().filter(|p| p.peer == *from) else {
                    return LinkReply::refused("not paired");
                };
                if s.received.is_some() {
                    return LinkReply::refused("the code was already used");
                }
                let transfer = open(&p.key, &sealed, &aad(&p.transcript))
                    .map_err(|_| Error::Invalid("could not open"))
                    .and_then(|plain| cbor4ii::serde::from_slice::<Transfer>(&plain).map_err(|_| Error::Invalid("could not read")));
                let transfer = match transfer {
                    Ok(t) => t,
                    Err(_) => return LinkReply::refused("could not open the transfer"),
                };
                if let Err(reason) = self.check(&transfer) {
                    s.end = Some(HostStatus::Failed(reason));
                    s.paired = None;
                    return LinkReply::refused(reason);
                }
                s.received = Some(transfer);
                LinkReply::Ok
            }
            LinkRequest::Cancel { .. } => {
                if s.paired.as_ref().is_some_and(|p| p.peer == *from) {
                    s.end = Some(HostStatus::Cancelled);
                    s.paired = None;
                    s.received = None;
                    LinkReply::Ok
                } else {
                    LinkReply::refused("not paired")
                }
            }
        }
    }

    fn check(&self, t: &Transfer) -> std::result::Result<(), &'static str> {
        const BAD: &str = "the identity did not check out";
        if t.v != VERSION {
            return Err("the other device runs a newer version");
        }
        let user = user_key(&t.seed).map_err(|_| BAD)?;
        let user_public = user.public();
        if user_id(&user_public) != t.user_id || t.node_cert.peer_id != self.offer.peer().to_string() {
            return Err(BAD);
        }
        if !WEIRDNET.verify_cert(&user_public, &t.node_cert) || !t.node_cert.active() {
            return Err(BAD);
        }
        Ok(())
    }

    /// The user tapped "Matches" on this device.
    pub fn confirm(&self) {
        let mut s = self.lock();
        if s.paired.is_some() {
            s.confirmed = true;
        }
    }

    /// The identity, once it arrived and the user confirmed the code here. Spends
    /// the offer; later calls fail.
    pub fn take(&self) -> Result<Transfer> {
        let mut s = self.lock();
        if !s.confirmed {
            return Err(Error::Invalid("confirm the code first"));
        }
        let t = s.received.take().ok_or(Error::Invalid("nothing has arrived yet"))?;
        s.paired = None;
        s.end = Some(HostStatus::Done);
        Ok(t)
    }

    /// The user said the codes differ, or closed the screen.
    pub fn cancel(&self) {
        let mut s = self.lock();
        if s.end.is_none() {
            s.end = Some(HostStatus::Cancelled);
        }
        s.paired = None;
        s.received = None;
    }
}

/// The existing device's side of one pairing.
pub struct Sender {
    offer: VerifiedOffer,
    eph: [u8; 32],
    key: Key32,
    transcript: [u8; 32],
    sas: String,
}

impl std::fmt::Debug for Sender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender").field("peer", &self.offer.peer()).finish_non_exhaustive()
    }
}

impl Sender {
    /// Starts pairing with a scanned offer; send the returned Hello.
    pub fn start(offer: VerifiedOffer, now_ms: i64) -> Result<(Self, LinkRequest)> {
        if offer.expired(now_ms) {
            return Err(Error::Expired);
        }
        let secret = ExchangeSecret::generate();
        let eph = secret.public();
        let shared = secret.agree(&offer.eph()).ok_or(Error::Invalid("the link is damaged"))?;
        let transcript = transcript(&offer.bytes, &eph);
        let (key, sas) = session_keys(&shared, &transcript)?;
        let hello = LinkRequest::Hello { nonce: offer.offer.nonce.clone(), eph: eph.to_vec() };
        Ok((Sender { offer, eph, key, transcript, sas }, hello))
    }

    pub fn sas(&self) -> &str {
        &self.sas
    }

    pub fn offer(&self) -> &VerifiedOffer {
        &self.offer
    }

    /// Our ephemeral public key (tests).
    pub fn eph(&self) -> [u8; 32] {
        self.eph
    }

    /// The sealed transfer request; only after the user confirmed the SAS.
    pub fn transfer(&self, t: &Transfer) -> Result<LinkRequest> {
        let plain = Zeroizing::new(cbor(t)?);
        let sealed = seal(&self.key, &plain, &aad(&self.transcript))?;
        Ok(LinkRequest::Transfer { nonce: self.offer.offer.nonce.clone(), sealed })
    }

    pub fn cancel(&self) -> LinkRequest {
        LinkRequest::Cancel { nonce: self.offer.offer.nonce.clone() }
    }
}

/// A transfer for the new device in `offer`: the seed, and a node certificate
/// for it signed by the user key.
pub fn make_transfer(seed: &Key32, profile_name: &str, settings: &str, offer: &VerifiedOffer, now_ms: i64) -> Result<Transfer> {
    let user = user_key(&seed.0)?;
    let uid = user_id(&user.public());
    let node_cert = WEIRDNET.certify(&user, &uid, &offer.peer().to_string(), &offer.offer.hint, now_ms)?;
    Ok(Transfer {
        v: VERSION,
        seed: seed.0.to_vec(),
        user_id: uid,
        profile_name: profile_name.to_string(),
        settings: settings.to_string(),
        node_cert,
        sync: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEME: &str = "weirdnet-morph://";

    fn new_host(now: i64) -> (Host, Keypair) {
        let node = Keypair::generate_ed25519();
        let host = Host::new(SCHEME, &node, vec!["/ip4/192.168.1.5/tcp/4747".into()], "Morph on desk", now).unwrap();
        (host, node)
    }

    fn scan(host: &Host, now: i64) -> (Sender, LinkRequest) {
        Sender::start(crate::decode_link(SCHEME, host.link()).unwrap(), now).unwrap()
    }

    #[test]
    fn both_sides_show_the_same_code_and_the_identity_moves() {
        let (host, _) = new_host(0);
        let phone = Keypair::generate_ed25519().public().to_peer_id();
        assert_eq!(host.status(1), HostStatus::Waiting);
        let (sender, hello) = scan(&host, 1);
        assert_eq!(host.handle(&phone, hello, 2), LinkReply::Ok);
        let HostStatus::Paired { sas } = host.status(3) else { panic!("not paired") };
        assert_eq!(sas, sender.sas());
        assert_eq!(sas.len(), 6);

        let seed = Key32::random().unwrap();
        let t = make_transfer(&seed, "Work", "{}", sender.offer(), 4).unwrap();
        let uid = t.user_id.clone();
        assert!(host.take().is_err(), "nothing yet");
        assert_eq!(host.handle(&phone, sender.transfer(&t).unwrap(), 5), LinkReply::Ok);
        assert!(matches!(host.status(6), HostStatus::Received { .. }));
        assert!(host.take().is_err(), "not confirmed here yet");
        host.confirm();
        let got = host.take().unwrap();
        assert_eq!(got.seed, seed.0.to_vec());
        assert_eq!(got.user_id, uid);
        assert_eq!(got.profile_name, "Work");
        assert_eq!(host.status(7), HostStatus::Done);
        // Single use.
        assert!(host.take().is_err());
        let (again, hello) = scan(&host, 8);
        assert!(matches!(host.handle(&phone, hello, 9), LinkReply::Refused { .. }));
        assert!(matches!(host.handle(&phone, again.transfer(&t).unwrap(), 9), LinkReply::Refused { .. }));
    }

    #[test]
    fn a_transfer_from_another_peer_or_key_is_refused() {
        let (host, _) = new_host(0);
        let phone = Keypair::generate_ed25519().public().to_peer_id();
        let thief = Keypair::generate_ed25519().public().to_peer_id();
        let (sender, hello) = scan(&host, 1);
        host.handle(&phone, hello, 1);
        let seed = Key32::random().unwrap();
        let t = make_transfer(&seed, "Work", "{}", sender.offer(), 1).unwrap();
        assert!(matches!(host.handle(&thief, sender.transfer(&t).unwrap(), 2), LinkReply::Refused { .. }));
        // A second scanner re-pairs; the first sender's key no longer opens.
        let (other, hello) = scan(&host, 2);
        assert_ne!(other.sas(), sender.sas());
        host.handle(&thief, hello, 2);
        assert!(matches!(host.handle(&thief, sender.transfer(&t).unwrap(), 3), LinkReply::Refused { .. }));
    }

    #[test]
    fn a_certificate_for_another_node_or_user_is_refused() {
        let (host, _) = new_host(0);
        let phone = Keypair::generate_ed25519().public().to_peer_id();
        let (sender, hello) = scan(&host, 1);
        host.handle(&phone, hello, 1);
        let seed = Key32::random().unwrap();
        let mut t = make_transfer(&seed, "Work", "{}", sender.offer(), 1).unwrap();
        t.user_id = "someone-else".into();
        assert!(matches!(host.handle(&phone, sender.transfer(&t).unwrap(), 2), LinkReply::Refused { .. }));
        assert_eq!(host.status(3), HostStatus::Failed("the identity did not check out"));
    }

    #[test]
    fn three_attempts_then_the_code_dies() {
        let (host, _) = new_host(0);
        let phone = Keypair::generate_ed25519().public().to_peer_id();
        for _ in 0..MAX_ATTEMPTS {
            let (_, hello) = scan(&host, 1);
            assert_eq!(host.handle(&phone, hello, 1), LinkReply::Ok);
        }
        let (_, hello) = scan(&host, 1);
        assert!(matches!(host.handle(&phone, hello, 1), LinkReply::Refused { .. }));
        assert_eq!(host.status(1), HostStatus::Failed("too many pairing attempts"));
    }

    #[test]
    fn codes_expire_and_cancel() {
        let (host, _) = new_host(0);
        let link = crate::decode_link(SCHEME, host.link()).unwrap();
        assert!(matches!(Sender::start(link, OFFER_TTL_MS + 1), Err(Error::Expired)));
        let phone = Keypair::generate_ed25519().public().to_peer_id();
        let (_, hello) = scan(&host, 1);
        assert!(matches!(host.handle(&phone, hello, OFFER_TTL_MS + 1), LinkReply::Refused { .. }));
        assert_eq!(host.status(OFFER_TTL_MS + 1), HostStatus::Expired);

        let (host, _) = new_host(0);
        let (sender, hello) = scan(&host, 1);
        host.handle(&phone, hello, 1);
        assert_eq!(host.handle(&phone, sender.cancel(), 2), LinkReply::Ok);
        assert_eq!(host.status(2), HostStatus::Cancelled);
        let (host, _) = new_host(0);
        host.cancel();
        assert_eq!(host.status(1), HostStatus::Cancelled);
    }

    #[test]
    fn transfers_do_not_show_the_seed() {
        let (host, _) = new_host(0);
        let (sender, _) = scan(&host, 1);
        let t = make_transfer(&Key32([9; 32]), "Work", "{}", sender.offer(), 1).unwrap();
        assert!(!format!("{t:?}").contains("9, 9"));
        assert!(!format!("{host:?} {sender:?}").contains("secret"));
    }
}
