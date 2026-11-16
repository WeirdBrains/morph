//! The offer a new device shows as a QR code: public keys, addresses, an expiry,
//! signed by the new device's node key. Nothing in it is secret.
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use weirdnet_net::{Keypair, Multiaddr, PeerId, PublicKey};

use crate::{Error, Result, VERSION};

/// Signature domain for offers.
const OFFER_DOMAIN: &[u8] = b"weirdnet-device-link/v1/offer\n";
/// Most addresses an offer carries, so the QR code stays easy to scan.
pub const MAX_ADDRS: usize = 6;
/// Longest device hint, in bytes.
pub const MAX_HINT: usize = 64;
/// Longest link text accepted, so a pasted blob cannot be huge.
const MAX_LINK: usize = 4096;

/// What the new device offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub v: u8,
    /// The new device's node public key (libp2p protobuf).
    #[serde(with = "crate::bytes")]
    pub node: Vec<u8>,
    /// The new device's ephemeral X25519 public key.
    #[serde(with = "crate::bytes")]
    pub eph: Vec<u8>,
    /// 16 random bytes naming this offer.
    #[serde(with = "crate::bytes")]
    pub nonce: Vec<u8>,
    /// Where the new device listens, without the `/p2p/` suffix.
    pub addrs: Vec<String>,
    /// Unix ms after which the offer is dead.
    pub expires_ms: i64,
    /// A label for the new device ("Morph on my-desktop").
    pub hint: String,
}

#[derive(Serialize, Deserialize)]
struct Signed {
    /// The CBOR bytes of the [`Offer`], exactly as signed.
    #[serde(with = "crate::bytes")]
    offer: Vec<u8>,
    #[serde(with = "crate::bytes")]
    sig: Vec<u8>,
}

/// An offer whose signature checked out, with the exact bytes the transcript binds.
#[derive(Debug, Clone)]
pub struct VerifiedOffer {
    pub offer: Offer,
    pub(crate) bytes: Vec<u8>,
    node: PublicKey,
}

impl VerifiedOffer {
    /// The new device's peer id (its node key).
    pub fn peer(&self) -> PeerId {
        self.node.to_peer_id()
    }

    /// The new device's addresses, ready to dial (`/p2p/PEER` added).
    pub fn dial_addrs(&self) -> Vec<Multiaddr> {
        let peer = self.peer();
        self.offer.addrs.iter().filter_map(|a| format!("{a}/p2p/{peer}").parse().ok()).collect()
    }

    pub fn expired(&self, now_ms: i64) -> bool {
        now_ms > self.offer.expires_ms
    }

    pub(crate) fn eph(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&self.offer.eph);
        out
    }
}

/// Signs `offer` with the new device's node key and returns the link text
/// (`<scheme>link/<base64url CBOR>`), e.g. `weirdnet-morph://link/…`.
pub fn encode_link(scheme: &str, offer: &Offer, node_key: &Keypair) -> Result<(String, VerifiedOffer)> {
    check_shape(offer)?;
    if node_key.public().encode_protobuf() != offer.node {
        return Err(Error::Invalid("the offer names another node key"));
    }
    let bytes = cbor(offer)?;
    let sig = node_key.sign(&signed_bytes(&bytes)).map_err(|_| Error::Invalid("could not sign the offer"))?;
    let blob = cbor(&Signed { offer: bytes.clone(), sig })?;
    let verified = VerifiedOffer { offer: offer.clone(), bytes, node: node_key.public() };
    Ok((format!("{scheme}link/{}", URL_SAFE_NO_PAD.encode(blob)), verified))
}

/// Reads and checks a link: its scheme, encoding, signature, version and shape.
/// Expiry is checked separately ([`VerifiedOffer::expired`]) so the caller can
/// say "this code expired" rather than "invalid".
pub fn decode_link(scheme: &str, text: &str) -> Result<VerifiedOffer> {
    let text = text.trim();
    if text.len() > MAX_LINK {
        return Err(Error::Invalid("the link is too long"));
    }
    let prefix = format!("{scheme}link/");
    let body = text.strip_prefix(&prefix).ok_or(Error::Invalid("not a device link"))?;
    let blob = URL_SAFE_NO_PAD.decode(body).map_err(|_| Error::Invalid("the link is damaged"))?;
    let signed: Signed = cbor4ii::serde::from_slice(&blob).map_err(|_| Error::Invalid("the link is damaged"))?;
    let offer: Offer = cbor4ii::serde::from_slice(&signed.offer).map_err(|_| Error::Invalid("the link is damaged"))?;
    if offer.v != VERSION {
        return Err(Error::Invalid("this link is from a newer version"));
    }
    check_shape(&offer)?;
    let node = PublicKey::try_decode_protobuf(&offer.node).map_err(|_| Error::Invalid("the link is damaged"))?;
    if !node.verify(&signed_bytes(&signed.offer), &signed.sig) {
        return Err(Error::Invalid("the link's signature does not match"));
    }
    Ok(VerifiedOffer { offer, bytes: signed.offer, node })
}

fn signed_bytes(offer: &[u8]) -> Vec<u8> {
    [OFFER_DOMAIN, offer].concat()
}

fn check_shape(offer: &Offer) -> Result<()> {
    if offer.eph.len() != 32 || offer.nonce.len() != 16 {
        return Err(Error::Invalid("the link is damaged"));
    }
    if offer.addrs.is_empty() || offer.addrs.len() > MAX_ADDRS || offer.hint.len() > MAX_HINT {
        return Err(Error::Invalid("the link is damaged"));
    }
    Ok(())
}

pub(crate) fn cbor<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    cbor4ii::serde::to_vec(Vec::new(), value).map_err(|_| Error::Invalid("could not encode"))
}

/// Most relay circuits an offer carries; the rest of [`MAX_ADDRS`] is direct
/// addresses.
pub const MAX_CIRCUITS: usize = 2;

/// The addresses worth putting in an offer: no wildcard or link-local ones, the
/// node's own trailing `/p2p/` dropped, loopback only when nothing else is
/// there (tests and single-machine checks), at most [`MAX_ADDRS`]. Direct
/// addresses come first, IPv4 before IPv6; relay circuits
/// (`…/p2p/RELAY/p2p-circuit`) keep their relay id and always get up to
/// [`MAX_CIRCUITS`] places, so a device on another network can still dial in.
pub fn offer_addrs(reachable: &[String]) -> Vec<String> {
    let mut direct: Vec<String> = Vec::new();
    let mut circuits: Vec<String> = Vec::new();
    for a in reachable {
        let a = a.trim();
        let base = match a.rsplit_once("/p2p/") {
            Some((head, id)) if !id.contains('/') => head,
            _ => a,
        };
        if base.is_empty() || base.parse::<Multiaddr>().is_err() {
            continue;
        }
        let circuit = base.contains("/p2p-circuit");
        let base = base.to_string();
        let list = if circuit { &mut circuits } else { &mut direct };
        if !list.contains(&base) {
            list.push(base);
        }
    }
    let loopback = |a: &String| a.starts_with("/ip4/127.") || a.starts_with("/ip6/::1/");
    let mut out: Vec<String> = direct.iter().filter(|a| !loopback(a)).cloned().collect();
    if out.is_empty() {
        out = direct;
    }
    out.sort_by_key(|a| !a.starts_with("/ip4/"));
    circuits.sort_by_key(|a| a.contains("/quic"));
    circuits.truncate(MAX_CIRCUITS);
    out.truncate(MAX_ADDRS - circuits.len());
    out.extend(circuits);
    out
}

/// Whether `addrs` (from [`offer_addrs`]) include a relay circuit.
pub fn has_circuit(addrs: &[String]) -> bool {
    addrs.iter().any(|a| a.contains("/p2p-circuit"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(key: &Keypair) -> Offer {
        Offer {
            v: VERSION,
            node: key.public().encode_protobuf(),
            eph: vec![7; 32],
            nonce: vec![1; 16],
            addrs: vec!["/ip4/192.168.1.5/tcp/4747".into()],
            expires_ms: 1_000,
            hint: "Morph on test".into(),
        }
    }

    #[test]
    fn a_link_round_trips_and_names_its_node() {
        let key = Keypair::generate_ed25519();
        let (link, made) = encode_link("weirdnet-morph://", &offer(&key), &key).unwrap();
        assert!(link.starts_with("weirdnet-morph://link/"));
        assert!(link.len() < 400, "{} chars", link.len());
        let read = decode_link("weirdnet-morph://", &format!("  {link}\n")).unwrap();
        assert_eq!(read.offer, made.offer);
        assert_eq!(read.peer(), key.public().to_peer_id());
        assert_eq!(read.dial_addrs()[0].to_string(), format!("/ip4/192.168.1.5/tcp/4747/p2p/{}", read.peer()));
        assert!(!read.expired(1_000));
        assert!(read.expired(1_001));
    }

    #[test]
    fn tampered_or_foreign_links_are_refused() {
        let key = Keypair::generate_ed25519();
        let (link, _) = encode_link("weirdnet-morph://", &offer(&key), &key).unwrap();
        assert!(decode_link("weirdnet-muninn://", &link).is_err());
        let mut chars: Vec<char> = link.chars().collect();
        let i = chars.len() - 10;
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        let damaged: String = chars.into_iter().collect();
        assert!(decode_link("weirdnet-morph://", &damaged).is_err());
        // Signed by a key other than the one it names.
        let other = Keypair::generate_ed25519();
        assert!(encode_link("weirdnet-morph://", &offer(&key), &other).is_err());
        assert!(decode_link("weirdnet-morph://", "weirdnet-morph://link/!!").is_err());
        assert!(decode_link("weirdnet-morph://", &"x".repeat(5000)).is_err());
    }

    #[test]
    fn offers_carry_few_useful_addresses() {
        let peer = Keypair::generate_ed25519().public().to_peer_id();
        let addrs: Vec<String> = [
            "/ip6/2001:db8::1/tcp/1",
            "/ip4/127.0.0.1/tcp/1",
            "/ip4/192.168.1.5/tcp/1",
            "/ip4/192.168.1.5/udp/1/quic-v1",
            "/ip4/10.0.0.2/tcp/1",
            "/ip4/10.0.0.3/tcp/1",
            "/ip4/10.0.0.4/tcp/1",
            "/ip4/10.0.0.5/tcp/1",
        ]
        .iter()
        .map(|a| format!("{a}/p2p/{peer}"))
        .collect();
        let out = offer_addrs(&addrs);
        assert_eq!(out.len(), MAX_ADDRS);
        assert_eq!(out[0], "/ip4/192.168.1.5/tcp/1");
        assert!(out.iter().all(|a| !a.contains("127.0.0.1") && !a.contains("/p2p/")));
        assert_eq!(offer_addrs(&[format!("/ip4/127.0.0.1/tcp/9/p2p/{peer}")]), vec!["/ip4/127.0.0.1/tcp/9"]);
    }

    #[test]
    fn relay_circuits_keep_their_relay_and_always_fit() {
        let peer = Keypair::generate_ed25519().public().to_peer_id();
        let relay = Keypair::generate_ed25519().public().to_peer_id();
        let circuit = format!("/dns4/relay.example.com/tcp/4747/p2p/{relay}/p2p-circuit");
        let mut addrs: Vec<String> = (2..10).map(|i| format!("/ip4/10.0.0.{i}/tcp/1/p2p/{peer}")).collect();
        // libp2p reports a circuit with or without this node's own id at the end
        addrs.push(circuit.clone());
        addrs.push(format!("{circuit}/p2p/{peer}"));
        let out = offer_addrs(&addrs);
        assert_eq!(out.len(), MAX_ADDRS);
        assert_eq!(out.last().unwrap(), &circuit, "{out:?}");
        assert_eq!(out.iter().filter(|a| a.contains("p2p-circuit")).count(), 1);
        assert!(has_circuit(&out));

        let o = Offer { addrs: out, ..offer(&Keypair::generate_ed25519()) };
        let key = Keypair::generate_ed25519();
        let o = Offer { node: key.public().encode_protobuf(), ..o };
        let (link, _) = encode_link("weirdnet-morph://", &o, &key).unwrap();
        let read = decode_link("weirdnet-morph://", &link).unwrap();
        let dial = read.dial_addrs();
        assert_eq!(dial.last().unwrap().to_string(), format!("{circuit}/p2p/{}", read.peer()));
    }
}
