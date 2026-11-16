//! Compact address and byte encodings for the codes a phone scans (offers and
//! invites, v2), so their QR codes stay small enough to read off a laptop
//! screen.
//!
//! An address is its binary multiaddr, except a circuit through one of the
//! [`KNOWN_RELAYS`] (`<relay>/p2p-circuit`), which is the relay's one-byte
//! index; the reader rebuilds the circuit from its own copy of the list. Both
//! sides ship the same list. An index the reader does not know is dropped
//! like any other address it cannot use; the peer id that is dialled comes
//! from the signed or checked part of the code, never from the relay entry,
//! so a wrong relay can at worst fail to connect.
use serde::{Deserialize, Serialize};
use weirdnet_net::Multiaddr;

/// The relays both sides know by index: the WeirdBrains public relay, TCP
/// then QUIC. Append only; never reorder (old indices must keep meaning).
pub const KNOWN_RELAYS: &[&str] = &[
    "/dns4/relay.net.weirdbrains.com/tcp/4747/p2p/12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB",
    "/dns4/relay.net.weirdbrains.com/udp/4747/quic-v1/p2p/12D3KooWHp56KezbVCubjuznJGqyn9xTe8PWePfuwv4YQUGxaRBB",
];

/// A byte string on the wire (CBOR bytes, not an array of integers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Bytes(#[serde(with = "crate::bytes")] pub Vec<u8>);

/// `addr` (text, no trailing `/p2p/` of the node itself) for the wire.
pub fn pack_addr(addr: &str) -> Option<Vec<u8>> {
    if let Some(relay) = addr.strip_suffix("/p2p-circuit")
        && let Some(i) = KNOWN_RELAYS.iter().position(|r| *r == relay)
    {
        return Some(vec![i as u8]);
    }
    let m: Multiaddr = addr.parse().ok()?;
    Some(m.to_vec())
}

/// An address read off the wire, as text; `None` when it is not one.
pub fn unpack_addr(bytes: &[u8]) -> Option<String> {
    if let [i] = bytes {
        return KNOWN_RELAYS.get(*i as usize).map(|r| format!("{r}/p2p-circuit"));
    }
    Multiaddr::try_from(bytes.to_vec()).ok().map(|m| m.to_string())
}

pub(crate) fn pack_addrs(addrs: &[String]) -> Vec<Bytes> {
    addrs.iter().filter_map(|a| pack_addr(a)).map(Bytes).collect()
}

pub(crate) fn unpack_addrs(addrs: &[Bytes]) -> Vec<String> {
    addrs.iter().filter_map(|b| unpack_addr(&b.0)).collect()
}

/// The `v` of an older (v1, a CBOR map) code, to say "update Morph" rather
/// than "damaged".
pub(crate) fn map_version(blob: &[u8]) -> Option<u8> {
    #[derive(Deserialize)]
    struct V {
        v: u8,
    }
    cbor4ii::serde::from_slice::<V>(blob).ok().map(|v| v.v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_known_relay_is_one_byte_and_comes_back_whole() {
        for (i, r) in KNOWN_RELAYS.iter().enumerate() {
            let circuit = format!("{r}/p2p-circuit");
            assert_eq!(pack_addr(&circuit), Some(vec![i as u8]));
            assert_eq!(unpack_addr(&[i as u8]).as_deref(), Some(circuit.as_str()));
            assert!(r.parse::<Multiaddr>().is_ok());
        }
        assert_eq!(unpack_addr(&[200]), None, "an index this side does not know");
    }

    #[test]
    fn other_addresses_round_trip_in_binary() {
        for a in [
            "/ip4/192.168.50.2/tcp/41234",
            "/ip4/100.101.102.103/udp/41234/quic-v1",
            "/ip6/2600:1700::1/tcp/4001",
            "/dns4/relay.example.com/tcp/4747/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp/p2p-circuit",
        ] {
            let b = pack_addr(a).unwrap();
            assert!(b.len() > 1);
            assert_eq!(unpack_addr(&b).as_deref(), Some(a));
        }
        assert_eq!(pack_addr("/ip4/192.168.50.2/tcp/41234").unwrap().len(), 8, "ip4 + tcp: 8 bytes");
        assert_eq!(pack_addr("not an address"), None);
        assert_eq!(unpack_addr(&[0xff, 0xff, 0xff]), None);
    }
}
