//! `morph-ai`'s [`NodeRouter`] and [`LinkHandler`] over [`DeviceCaller`]: Node companions and
//! inference offload reach the user's other devices on this device's identity.
//!
//! - [`DeviceRouter::nodes`] is the live roster's linked devices ([`DeviceCaller::linked_devices`]),
//!   with what the roster does not know (platform, online, serves models) from a [`NodeInfo`]
//!   source the app fills from discovery and companion settings.
//! - [`DeviceRouter::open`] opens a stream ([`DeviceCaller::open_stream`]) and frames messages on
//!   it ([`frames`]): only an active linked device is dialled.
//! - [`DeviceRouter::serve`] accepts streams ([`DeviceCaller::accept_streams`], linked devices
//!   only) and runs a [`LinkHandler`] for each, concurrently, without a runtime of its own.
//!
//! The protocols that run over a [`Link`] are in [`protocols`]; that is the only part that
//! changes when Morph pins `weirdnet-agent` 0.2.0.

use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use futures::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use futures::stream::{FuturesUnordered, StreamExt};
use morph_ai::remote::{LinkRx, LinkTx};
use morph_ai::{Link, LinkHandler, LinkedNode, NodeRouter, Platform, RouteError};
use weirdnet_net::stream::{Limits, Stream};

use crate::device_calls::{DeviceCallError, DeviceCaller, LinkedDevice};

/// What the roster does not know about a linked device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeInfo {
    pub platform: Platform,
    pub online: bool,
    pub serves_models: bool,
}

impl Default for NodeInfo {
    /// Until discovery says otherwise: a desktop, assumed online (a dial finds out), that does
    /// not serve models (offload never picks a device on a guess).
    fn default() -> Self {
        Self { platform: Platform::Desktop, online: true, serves_models: false }
    }
}

type InfoFor = dyn Fn(&LinkedDevice) -> NodeInfo + Send + Sync;

/// [`NodeRouter`] on a [`DeviceCaller`].
#[derive(Clone)]
pub struct DeviceRouter {
    caller: DeviceCaller,
    info: Arc<InfoFor>,
    limits: Limits,
}

impl DeviceRouter {
    pub fn new(caller: DeviceCaller) -> Self {
        Self { caller, info: Arc::new(|_| NodeInfo::default()), limits: Limits::default() }
    }

    /// Where platform, online and serves-models come from.
    pub fn with_info(mut self, info: impl Fn(&LinkedDevice) -> NodeInfo + Send + Sync + 'static) -> Self {
        self.info = Arc::new(info);
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn caller(&self) -> &DeviceCaller {
        &self.caller
    }

    /// Serve `protocol` to linked devices: each accepted stream becomes a [`Link`] for
    /// `handler`, and sessions run concurrently. Streams are accepted from the moment this
    /// returns; drive the returned future (on any executor) to run the sessions. It ends when
    /// the node stops accepting.
    pub fn serve(&self, protocol: &str, handler: Arc<dyn LinkHandler>) -> Result<impl Future<Output = ()> + Send + 'static, RouteError> {
        let mut incoming = self.caller.accept_streams(protocol, self.limits).map_err(route_error)?;
        Ok(async move {
            let mut sessions = FuturesUnordered::new();
            loop {
                futures::select! {
                    s = incoming.next().fuse() => match s {
                        Some(stream) => {
                            let from = stream.peer().to_string();
                            let handler = handler.clone();
                            sessions.push(async move { handler.handle(from, frames(stream)).await });
                        }
                        None => break,
                    },
                    _ = sessions.select_next_some() => {}
                }
            }
            while sessions.next().await.is_some() {}
        })
    }
}

#[async_trait]
impl NodeRouter for DeviceRouter {
    fn nodes(&self) -> Vec<LinkedNode> {
        self.caller
            .linked_devices()
            .into_iter()
            .map(|d| {
                let i = if d.this_device { NodeInfo { online: true, ..(self.info)(&d) } } else { (self.info)(&d) };
                LinkedNode {
                    peer_id: d.peer_id,
                    label: d.label,
                    platform: i.platform,
                    online: i.online,
                    this_device: d.this_device,
                    serves_models: i.serves_models,
                }
            })
            .collect()
    }

    async fn open(&self, peer: &str, protocol: &str) -> Result<Link, RouteError> {
        let stream = self.caller.open_stream(peer, protocol, self.limits).await.map_err(route_error)?;
        Ok(frames(stream))
    }
}

/// `DeviceCallError` as the router reports it.
pub fn route_error(e: DeviceCallError) -> RouteError {
    match e {
        DeviceCallError::NotLinked { peer, .. } => RouteError::NotLinked(peer),
        DeviceCallError::Unreachable(p) => RouteError::Unreachable(p),
        other => RouteError::Failed(other.to_string()),
    }
}

/// Largest message on a link (the same as `weirdnet-agent` `remote::codec::MAX_FRAME`).
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

/// A stream as a message [`Link`]: each message is a u32 big-endian length, then that many
/// bytes (the `weirdnet-agent` `remote::codec` framing, so its protocols run unchanged).
pub fn frames(stream: Stream) -> Link {
    let (r, w) = stream.split();
    Link { tx: Box::new(FrameWriter(w)), rx: Box::new(FrameReader { r, buf: Vec::new() }) }
}

struct FrameWriter(WriteHalf<Stream>);

#[async_trait]
impl LinkTx for FrameWriter {
    async fn send(&mut self, message: Vec<u8>) -> Result<(), RouteError> {
        if message.len() > MAX_FRAME {
            return Err(RouteError::Failed(format!("message of {} bytes is over the {MAX_FRAME}-byte limit", message.len())));
        }
        let mut out = Vec::with_capacity(4 + message.len());
        out.extend_from_slice(&(message.len() as u32).to_be_bytes());
        out.extend_from_slice(&message);
        self.0.write_all(&out).await.map_err(|_| RouteError::Closed)
    }
}

struct FrameReader {
    r: ReadHalf<Stream>,
    buf: Vec<u8>,
}

/// One whole message off the front of `buf`, if there is one.
fn take_frame(buf: &mut Vec<u8>) -> Result<Option<Vec<u8>>, RouteError> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > MAX_FRAME {
        return Err(RouteError::Failed(format!("message of {len} bytes is over the {MAX_FRAME}-byte limit")));
    }
    if buf.len() < 4 + len {
        return Ok(None);
    }
    let msg = buf[4..4 + len].to_vec();
    buf.drain(..4 + len);
    Ok(Some(msg))
}

#[async_trait]
impl LinkRx for FrameReader {
    /// Cancel-safe: bytes are buffered across calls and a pending read consumes nothing.
    async fn recv(&mut self) -> Result<Option<Vec<u8>>, RouteError> {
        let mut chunk = [0u8; 16 * 1024];
        loop {
            if let Some(m) = take_frame(&mut self.buf)? {
                return Ok(Some(m));
            }
            match self.r.read(&mut chunk).await {
                Ok(0) if self.buf.is_empty() => return Ok(None),
                Ok(0) => return Err(RouteError::Closed),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(_) => return Err(RouteError::Closed),
            }
        }
    }
}

/// The protocols Morph runs over a [`Link`], and the glue to `weirdnet-agent`.
///
/// Morph pins `weirdnet-agent` `=0.1.0`, which has no `remote` module yet. With 0.2.0 the
/// glue is: a [`Link`]'s `tx` / `rx` as `remote::FrameTx` / `remote::FrameRx` (both carry
/// whole messages, same framing), a `remote::Dialer` that calls [`NodeRouter::open`] with
/// [`INFER`] or [`TASK`], and [`LinkHandler`]s that call `remote::infer::serve` and
/// `remote::task::serve`. Nothing outside this module changes.
pub mod protocols {
    /// `weirdnet-agent` `remote::infer::PROTOCOL`: model turns for inference offload.
    pub const INFER: &str = "/weirdnet-agent/infer/1";
    /// `weirdnet-agent` `remote::task::PROTOCOL`: Node companion tasks.
    pub const TASK: &str = "/weirdnet-agent/task/1";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_calls::DeviceCaller;
    use std::sync::{Arc, Mutex};
    use weirdnet_net::Keypair;
    use weirdnet_net::Multiaddr;
    use weirdnet_net::profile::WEIRDNET;
    use weirdnet_net::user::NodeCert;
    use weirdnet_sync::{Roster, SyncState, node_config, start_node};

    struct Device {
        key: Keypair,
        cert: NodeCert,
    }

    fn device(user: &Keypair, label: &str) -> Device {
        let key = Keypair::generate_ed25519();
        let cert = Roster::self_certify(&WEIRDNET, user, &key.public().to_peer_id().to_string(), label, 1).unwrap();
        Device { key, cert }
    }

    type Up = (DeviceCaller, Arc<Mutex<SyncState>>, Vec<String>);

    /// A device's caller, its live state and the addresses its node listens on.
    fn up(user: &Keypair, me: &Device, others: &[&NodeCert], server: Option<Multiaddr>) -> Up {
        let mut st = SyncState::new(vec![9; 32], user.public(), me.cert.clone(), Some(&me.key), vec!["bookmarks".into()]).unwrap();
        st.auto_first_rotation = false;
        for c in others {
            st.roster.upsert((*c).clone());
        }
        let st = Arc::new(Mutex::new(st));
        let node = Arc::new(start_node(me.key.clone(), node_config(true), st.clone()).unwrap());
        let listening = node.status().listening.clone();
        (DeviceCaller::new(node, st.clone(), move |_| server.clone().into_iter().collect()), st, listening)
    }

    fn rt<F: std::future::Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }

    /// Echoes each message back with the caller's peer id in front, then ends.
    struct Echo;

    #[async_trait]
    impl LinkHandler for Echo {
        async fn handle(&self, from: String, mut link: Link) {
            while let Ok(Some(m)) = link.rx.recv().await {
                let mut out = format!("{from}:").into_bytes();
                out.extend(m);
                if link.tx.send(out).await.is_err() {
                    break;
                }
            }
        }
    }

    #[test]
    fn frames_split_and_join_across_reads() {
        let mut buf = Vec::new();
        for m in [b"one".to_vec(), Vec::new(), vec![7; 70_000]] {
            buf.extend_from_slice(&(m.len() as u32).to_be_bytes());
            buf.extend(m);
        }
        let mut partial = buf[..5].to_vec();
        assert_eq!(take_frame(&mut partial).unwrap(), None, "a part of a message waits for the rest");
        assert_eq!(take_frame(&mut buf).unwrap().unwrap(), b"one");
        assert_eq!(take_frame(&mut buf).unwrap().unwrap(), b"");
        assert_eq!(take_frame(&mut buf).unwrap().unwrap().len(), 70_000);
        assert!(buf.is_empty());
        let mut big = ((MAX_FRAME + 1) as u32).to_be_bytes().to_vec();
        assert!(take_frame(&mut big).is_err(), "an oversized length is refused before buffering it");
    }

    #[test]
    fn nodes_are_the_linked_devices_with_their_info() {
        let user = Keypair::generate_ed25519();
        let (a, b, c) = (device(&user, "Desk"), device(&user, "Phone"), device(&user, "Gone"));
        let (caller, st, _) = up(&user, &a, &[&b.cert, &c.cert], None);
        let phone = b.cert.peer_id.clone();
        let router = DeviceRouter::new(caller).with_info(move |d| {
            if d.peer_id == phone {
                NodeInfo { platform: Platform::Phone, online: false, serves_models: false }
            } else {
                NodeInfo { serves_models: true, ..NodeInfo::default() }
            }
        });
        let mut nodes = router.nodes();
        nodes.sort_by(|x, y| x.label.cmp(&y.label));
        assert_eq!(nodes.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(), ["Desk", "Gone", "Phone"]);
        assert!(nodes[0].this_device && nodes[0].online);
        assert_eq!((nodes[2].platform, nodes[2].online), (Platform::Phone, false));
        // A revoked device leaves the list at once.
        {
            let mut s = st.lock().unwrap();
            let mut gone = s.roster.get(&c.cert.peer_id).cloned().unwrap();
            gone.revoked_at = Some(2);
            s.roster.upsert(gone);
        }
        assert!(router.nodes().iter().all(|n| n.label != "Gone"));
    }

    #[test]
    fn a_link_carries_messages_both_ways_between_linked_devices_only() {
        let user = Keypair::generate_ed25519();
        let (a, b, c) = (device(&user, "a"), device(&user, "b"), device(&user, "c"));
        let (server, _, listening) = up(&user, &a, &[&b.cert], None);
        let at: Multiaddr = format!("{}/p2p/{}", listening[0], server.peer_id()).parse().unwrap();
        let served = DeviceRouter::new(server.clone());
        let serving = served.serve(protocols::TASK, Arc::new(Echo)).unwrap();
        std::thread::spawn(move || rt(serving));

        let (from_b, _, _) = up(&user, &b, &[&a.cert], Some(at.clone()));
        let b_router = DeviceRouter::new(from_b);
        let mut link = rt(b_router.open(&a.cert.peer_id, protocols::TASK)).unwrap();
        rt(link.tx.send(b"hello".to_vec())).unwrap();
        rt(link.tx.send(vec![1; 200_000])).unwrap();
        assert_eq!(rt(link.rx.recv()).unwrap().unwrap(), format!("{}:hello", b.cert.peer_id).into_bytes());
        let big = rt(link.rx.recv()).unwrap().unwrap();
        assert_eq!(big.len(), b.cert.peer_id.len() + 1 + 200_000, "a message larger than a stream frame arrives whole");

        // The router refuses to dial a device that is not linked.
        assert_eq!(rt(b_router.open(&c.cert.peer_id, protocols::TASK)).err(), Some(RouteError::NotLinked(c.cert.peer_id.clone())));

        // A device A never linked is reset before the handler sees it.
        let (from_c, _, _) = up(&user, &c, &[&a.cert], Some(at));
        let mut cl = rt(DeviceRouter::new(from_c).open(&a.cert.peer_id, protocols::TASK)).unwrap();
        let _ = rt(cl.tx.send(b"let me in".to_vec()));
        assert!(!matches!(rt(cl.rx.recv()), Ok(Some(_))), "no reply to an unlinked device");
    }
}
