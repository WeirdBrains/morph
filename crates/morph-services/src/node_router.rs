//! `morph-ai`'s [`NodeRouter`] and [`LinkHandler`] over [`DeviceCaller`]: Node companions and
//! inference offload reach the user's other devices on this device's identity.
//!
//! - [`DeviceRouter::nodes`] is the live roster's linked devices ([`DeviceCaller::linked_devices`]),
//!   with what the roster does not know (platform, online, serves models) from a [`NodeInfo`]
//!   source the app fills from discovery and companion settings.
//! - [`DeviceRouter::open`] opens a stream ([`DeviceCaller::open_stream`]) and frames messages on
//!   it ([`frames`]): only an active linked device is dialled.
//! - [`for_profile`] is the app's router: it serves this device's [`Capabilities`] to linked
//!   devices on [`DEVICE_APP`] and fills [`NodeInfo`] from theirs ([`NodeDirectory`]).
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

/// The app each device serves to its linked siblings (only) with what it can do.
pub const DEVICE_APP: &str = "morph-device";

/// What a device says about itself on [`DEVICE_APP`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub platform: Platform,
    /// It has a local model it serves to the user's other devices.
    pub serves_models: bool,
}

/// [`Capabilities`] on the wire.
#[derive(serde::Serialize, serde::Deserialize)]
struct CapsWire {
    platform: String,
    serves_models: bool,
}

impl Capabilities {
    fn to_json(self) -> Vec<u8> {
        let platform = match self.platform {
            Platform::Phone => "phone",
            Platform::Desktop => "desktop",
        };
        serde_json::to_vec(&CapsWire { platform: platform.into(), serves_models: self.serves_models }).unwrap_or_default()
    }

    /// Unknown platforms read as desktop.
    fn from_json(bytes: &[u8]) -> Option<Self> {
        let w: CapsWire = serde_json::from_slice(bytes).ok()?;
        Some(Self { platform: if w.platform == "phone" { Platform::Phone } else { Platform::Desktop }, serves_models: w.serves_models })
    }
}

/// Answer [`DEVICE_APP`] on this device: `GET /caps` gives `mine()` as JSON. Linked devices
/// only ([`DeviceCaller::serve`] refuses the rest before this runs).
pub fn serve_capabilities(caller: &DeviceCaller, mine: impl Fn() -> Capabilities + Send + Sync + 'static) -> Result<(), DeviceCallError> {
    let mine = Arc::new(mine);
    let handler = move |_peer: weirdnet_net::PeerId, req: weirdnet_net::AppRequest| {
        let caps = mine();
        async move {
            if req.path_with_query != "/caps" {
                return Ok(weirdnet_net::AppResponse::new(404));
            }
            Ok(weirdnet_net::AppResponse::new(200).body(caps.to_json()))
        }
    };
    caller.serve(DEVICE_APP, Arc::new(handler))
}

/// What the router knows about each linked device, from asking it ([`NodeDirectory::refresh`]).
/// A device that answered is online with what it said; one that did not is offline; one not
/// asked yet gets [`NodeInfo::default`].
#[derive(Clone, Default)]
pub struct NodeDirectory {
    known: Arc<std::sync::Mutex<std::collections::HashMap<String, Option<Capabilities>>>>,
}

impl NodeDirectory {
    pub fn info(&self, device: &LinkedDevice) -> NodeInfo {
        match self.known.lock().unwrap_or_else(|p| p.into_inner()).get(&device.peer_id) {
            Some(Some(c)) => NodeInfo { platform: c.platform, online: true, serves_models: c.serves_models },
            Some(None) => NodeInfo { online: false, ..NodeInfo::default() },
            None => NodeInfo::default(),
        }
    }

    /// Ask every other linked device what it can do (each within `timeout`), and forget
    /// devices that are no longer linked.
    pub async fn refresh(&self, caller: &DeviceCaller, timeout: std::time::Duration) {
        let others: Vec<LinkedDevice> = caller.linked_devices().into_iter().filter(|d| !d.this_device).collect();
        let asks = others.iter().map(|d| async move {
            let caps = match caller.call(&d.peer_id, weirdnet_net::AppRequest::new(DEVICE_APP, "GET", "/caps"), timeout).await {
                Ok(r) if r.status == 200 => r.body.collect().await.ok().and_then(|b| Capabilities::from_json(&b)),
                _ => None,
            };
            (d.peer_id.clone(), caps)
        });
        let found = futures::future::join_all(asks).await;
        let mut known = self.known.lock().unwrap_or_else(|p| p.into_inner());
        known.clear();
        known.extend(found);
    }

    /// [`Self::refresh`] on this thread (for apps without an async runtime at hand).
    pub fn refresh_blocking(&self, caller: &DeviceCaller, timeout: std::time::Duration) {
        futures::executor::block_on(self.refresh(caller, timeout));
    }
}

/// The app's router for a profile whose sync node runs: it serves this device's
/// capabilities, and fills [`NodeInfo`] from what the others said (call
/// [`NodeDirectory::refresh`] when the panel opens or the devices change).
pub fn for_profile(
    home: &std::path::Path,
    profile_id: &str,
    mine: impl Fn() -> Capabilities + Send + Sync + 'static,
) -> Result<(DeviceRouter, NodeDirectory), DeviceCallError> {
    let caller = DeviceCaller::for_profile(home, profile_id)?;
    serve_capabilities(&caller, mine)?;
    let directory = NodeDirectory::default();
    let d = directory.clone();
    Ok((DeviceRouter::new(caller).with_info(move |dev| d.info(dev)), directory))
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

/// `weirdnet-agent` `remote::infer` and `remote::task` over [`NodeRouter`] links: a
/// [`remote::Dialer`] for the asking side ([`RemoteModel`], [`remote::task::run`]) and
/// [`LinkHandler`]s for the serving side. This is the only code that knows the protocols.
pub mod protocols {
    use std::sync::Arc;

    use async_trait::async_trait;
    use morph_ai::remote::{LinkRx, LinkTx};
    use morph_ai::{Link, LinkHandler, NodeRouter, OffloadPlan, RouteError};
    use weirdnet_agent::ModelBackend;
    use weirdnet_agent::remote::infer::{Admission, InferHost, RemoteModel};
    use weirdnet_agent::remote::task::TaskHost;
    use weirdnet_agent::remote::{self, Channel, Dialer, FrameRx, FrameTx, RemoteError};

    /// Model turns for inference offload.
    pub const INFER: &str = remote::infer::PROTOCOL;
    /// Node companion tasks.
    pub const TASK: &str = remote::task::PROTOCOL;

    fn remote_error(e: RouteError) -> RemoteError {
        match e {
            RouteError::NotLinked(p) => RemoteError::Refused { message: format!("{p} is not one of your linked devices") },
            RouteError::Unreachable(p) => RemoteError::Unreachable { message: p },
            RouteError::Closed => RemoteError::Closed,
            RouteError::Failed(m) => RemoteError::Failed { message: m },
        }
    }

    struct Tx(Box<dyn LinkTx>);
    struct Rx(Box<dyn LinkRx>);

    #[async_trait]
    impl FrameTx for Tx {
        async fn send(&mut self, message: Vec<u8>) -> Result<(), RemoteError> {
            self.0.send(message).await.map_err(remote_error)
        }
    }

    #[async_trait]
    impl FrameRx for Rx {
        /// Cancel-safe because `LinkRx::recv` is.
        async fn recv(&mut self) -> Result<Option<Vec<u8>>, RemoteError> {
            self.0.recv().await.map_err(remote_error)
        }
    }

    /// A link as a `weirdnet-agent` channel (both carry whole messages).
    pub fn channel(link: Link) -> Channel {
        Channel::new(Box::new(Tx(link.tx)), Box::new(Rx(link.rx)))
    }

    /// Dials the user's linked devices through a [`NodeRouter`].
    pub struct RouterDialer(pub Arc<dyn NodeRouter>);

    #[async_trait]
    impl Dialer for RouterDialer {
        async fn open(&self, node: &str, protocol: &str) -> Result<Channel, RemoteError> {
            self.0.open(node, protocol).await.map(channel).map_err(remote_error)
        }
    }

    /// Inference offload as planned: the plan's devices in order, then `local` when the plan
    /// falls back (or runs here).
    pub fn remote_model(router: Arc<dyn NodeRouter>, plan: &OffloadPlan, local: Option<Arc<dyn ModelBackend>>) -> RemoteModel {
        let m = RemoteModel::new(Arc::new(RouterDialer(router)), plan.nodes.clone());
        match local {
            Some(l) if plan.fallback_local => m.with_fallback(l),
            _ => m,
        }
    }

    /// Serves [`INFER`]: model turns from the user's other devices, `admission` at a time.
    pub struct InferService {
        pub host: Arc<dyn InferHost>,
        pub admission: Admission,
    }

    #[async_trait]
    impl LinkHandler for InferService {
        async fn handle(&self, from: String, link: Link) {
            if let Err(e) = remote::infer::serve(channel(link), self.host.as_ref(), &self.admission).await {
                eprintln!("morph-agent: inference for {from}: {e}");
            }
        }
    }

    /// Serves [`TASK`]: Node companion tasks, cards answered on the asking device.
    pub struct TaskService(pub Arc<dyn TaskHost>);

    #[async_trait]
    impl LinkHandler for TaskService {
        async fn handle(&self, from: String, link: Link) {
            if let Err(e) = remote::task::serve(channel(link), self.0.as_ref()).await {
                eprintln!("morph-agent: task for {from}: {e}");
            }
        }
    }
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
    fn node_info_comes_from_what_each_linked_device_says_about_itself() {
        let user = Keypair::generate_ed25519();
        let (a, b, c) = (device(&user, "desk"), device(&user, "phone"), device(&user, "gone"));
        let (desk, _, listening) = up(&user, &a, &[&b.cert, &c.cert], None);
        let at: Multiaddr = format!("{}/p2p/{}", listening[0], desk.peer_id()).parse().unwrap();
        serve_capabilities(&desk, || Capabilities { platform: Platform::Desktop, serves_models: true }).unwrap();
        // The phone knows the desk at `at`; C is linked but never answers (no node at that address for it).
        let (phone, _, _) = up(&user, &b, &[&a.cert, &c.cert], Some(at));
        let directory = NodeDirectory::default();
        let d = directory.clone();
        let router = DeviceRouter::new(phone.clone()).with_info(move |dev| d.info(dev));
        assert!(router.nodes().iter().all(|n| !n.serves_models), "nothing is offloaded to on a guess");
        rt(directory.refresh(&phone, std::time::Duration::from_secs(3)));
        let nodes = router.nodes();
        let by = |label: &str| nodes.iter().find(|n| n.label == label).unwrap().clone();
        assert_eq!((by("desk").platform, by("desk").online, by("desk").serves_models), (Platform::Desktop, true, true));
        assert!(!by("gone").online && !by("gone").serves_models);
        assert!(by("phone").this_device && by("phone").online);
        // So offload from the phone picks the desk.
        let plan = morph_ai::offload_plan(&morph_ai::OffloadSettings::default(), Platform::Phone, &nodes);
        assert_eq!(plan.nodes, std::slice::from_ref(&a.cert.peer_id));
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

    mod agent_protocols {
        use super::*;
        use morph_ai::OffloadPlan;
        use weirdnet_agent::remote::RemoteError;
        use weirdnet_agent::remote::infer::{Admission, InferHost, Route};
        use weirdnet_agent::remote::task::{TaskHost, TaskRequest};
        use weirdnet_agent::testing::{ScriptedApprover, ScriptedModel};
        use weirdnet_agent::{
            AgentConfig, AgentId, AgentIdentity, AgentKind, AgentSession, Approver, CancelToken, DeltaSink, Message, ModelBackend, ModelTurn, Policy,
            StopReason, ToolRegistry,
        };

        use super::protocols::{InferService, RouterDialer, TaskService, remote_model};

        fn said(text: &str) -> Arc<dyn ModelBackend> {
            Arc::new(ScriptedModel::new(vec![ModelTurn { text: text.into(), tool_calls: vec![] }]))
        }

        struct Models;
        impl InferHost for Models {
            fn open(&self, _model: Option<&str>) -> Result<Arc<dyn ModelBackend>, RemoteError> {
                Ok(said("from the desktop"))
            }
        }

        struct Companions;
        impl TaskHost for Companions {
            fn session(&self, request: &TaskRequest, approver: Arc<dyn Approver>) -> Result<AgentSession, RemoteError> {
                if request.companion != AgentId::new("atlas") {
                    return Err(RemoteError::Refused { message: format!("{} does not run here", request.companion) });
                }
                let identity = AgentIdentity::new("atlas", "Atlas", AgentKind::Node).with_node("desk");
                Ok(AgentSession::new(
                    AgentConfig::new(identity),
                    said("Atlas answered on the desktop"),
                    Arc::new(ToolRegistry::new()),
                    Policy::new().shared(),
                    approver,
                ))
            }
        }

        fn tokio() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
        }

        /// A serving desk (A) and an asking phone (B), linked; C is the user's but never linked.
        fn devices(rt: &tokio::runtime::Runtime) -> (Arc<dyn NodeRouter>, String, String) {
            let user = Keypair::generate_ed25519();
            let (a, b, c) = (device(&user, "desk"), device(&user, "phone"), device(&user, "c"));
            let (server, _, listening) = up(&user, &a, &[&b.cert], None);
            let at: Multiaddr = format!("{}/p2p/{}", listening[0], server.peer_id()).parse().unwrap();
            let desk = DeviceRouter::new(server);
            rt.spawn(desk.serve(protocols::INFER, Arc::new(InferService { host: Arc::new(Models), admission: Admission::new(2) })).unwrap());
            rt.spawn(desk.serve(protocols::TASK, Arc::new(TaskService(Arc::new(Companions)))).unwrap());
            let (phone, _, _) = up(&user, &b, &[&a.cert, &c.cert], Some(at));
            (Arc::new(DeviceRouter::new(phone)), a.cert.peer_id, c.cert.peer_id)
        }

        #[test]
        fn a_model_turn_runs_on_the_linked_desktop_and_falls_back_here_when_it_cannot() {
            let rt = tokio();
            let (router, desk, unlinked) = devices(&rt);
            let plan = OffloadPlan { nodes: vec![desk.clone()], fallback_local: true };
            let model = remote_model(router.clone(), &plan, Some(said("from this phone")));
            let turn = rt.block_on(model.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new())).unwrap();
            assert_eq!(turn.text, "from the desktop");
            assert!(matches!(model.last_route(), Some(Route::Node { node, .. }) if node == desk));

            // C is the user's device but not linked here, so the dial is refused and the turn runs locally.
            let plan = OffloadPlan { nodes: vec![unlinked], fallback_local: true };
            let model = remote_model(router.clone(), &plan, Some(said("from this phone")));
            let turn = rt.block_on(model.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new())).unwrap();
            assert_eq!(turn.text, "from this phone");
            assert_eq!(model.last_route(), Some(Route::Fallback));

            // Without a fallback, the error comes back.
            let plan = OffloadPlan { nodes: vec!["12D3KooWNotADevice".into()], fallback_local: false };
            let model = remote_model(router, &plan, Some(said("unused")));
            assert!(rt.block_on(model.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new())).is_err());
        }

        #[test]
        fn a_node_companion_runs_its_task_on_the_linked_desktop() {
            let rt = tokio();
            let (router, desk, _) = devices(&rt);
            let dialer = RouterDialer(router);
            let approver = ScriptedApprover::new(vec![]);
            let req = TaskRequest::new(AgentId::new("atlas"), "what is tokio", vec![Message::user("earlier")]);
            let got = rt.block_on(weirdnet_agent::remote::task::run(&dialer, &desk, req, None, &approver, &CancelToken::new())).unwrap();
            assert_eq!(got.outcome.stop, StopReason::Completed);
            assert_eq!(got.outcome.text, "Atlas answered on the desktop");
            assert_eq!(got.messages.first(), Some(&Message::user("earlier")), "the history went over and came back");

            // A companion that does not run there is refused by the host.
            let req = TaskRequest::new(AgentId::new("local"), "q", vec![]);
            let err = rt.block_on(weirdnet_agent::remote::task::run(&dialer, &desk, req, None, &approver, &CancelToken::new())).unwrap_err();
            assert!(matches!(err, RemoteError::Refused { .. }), "{err:?}");
        }
    }
}
