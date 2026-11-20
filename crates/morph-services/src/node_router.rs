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
//!   only) and runs a [`LinkHandler`] for each, concurrently, without a runtime of its own. A
//!   stream that sends nothing within [`FIRST_MESSAGE`] is closed, and a device gets at most
//!   [`MAX_SESSIONS_PER_PEER`] sessions of one protocol at once (more are reset).
//!
//! The protocols that run over a [`Link`] are in [`protocols`]; that is the only part that
//! changes when Morph pins `weirdnet-agent` 0.2.0.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

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
    first_message: Duration,
    per_peer: usize,
}

/// How long a served stream may stay silent before its first message; then it is closed, so a
/// linked device cannot hold sessions open without using them.
pub const FIRST_MESSAGE: Duration = Duration::from_secs(10);

/// Sessions one device may have open at once on one served protocol; a stream over it is reset
/// as it is accepted.
pub const MAX_SESSIONS_PER_PEER: usize = 16;

impl DeviceRouter {
    pub fn new(caller: DeviceCaller) -> Self {
        Self {
            caller,
            info: Arc::new(|_| NodeInfo::default()),
            limits: Limits::default(),
            first_message: FIRST_MESSAGE,
            per_peer: MAX_SESSIONS_PER_PEER,
        }
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

    /// What [`Self::serve`] allows each session: `first_message` to say something (else it is
    /// closed) and `per_peer` sessions open at once from one device (more are reset).
    /// Defaults: [`FIRST_MESSAGE`], [`MAX_SESSIONS_PER_PEER`].
    pub fn with_session_limits(mut self, first_message: Duration, per_peer: usize) -> Self {
        self.first_message = first_message;
        self.per_peer = per_peer.max(1);
        self
    }

    pub fn caller(&self) -> &DeviceCaller {
        &self.caller
    }

    /// Serve `protocol` to linked devices: each accepted stream becomes a [`Link`] for
    /// `handler`, and sessions run concurrently. Streams are accepted from the moment this
    /// returns; drive the returned future (on any executor) to run the sessions. It ends when
    /// the node stops accepting.
    ///
    /// The handler sees a stream only once its first message arrived (within the
    /// [`Self::with_session_limits`] time, else the stream is closed unseen), and a device with
    /// that many sessions of this protocol open has further streams reset as they arrive.
    pub fn serve(&self, protocol: &str, handler: Arc<dyn LinkHandler>) -> Result<impl Future<Output = ()> + Send + 'static, RouteError> {
        let mut incoming = self.caller.accept_streams(protocol, self.limits).map_err(route_error)?;
        let (first_message, per_peer) = (self.first_message, self.per_peer);
        Ok(async move {
            let mut sessions = FuturesUnordered::new();
            let mut open: HashMap<String, usize> = HashMap::new();
            loop {
                futures::select! {
                    s = incoming.next().fuse() => match s {
                        Some(stream) => {
                            let from = stream.peer().to_string();
                            let n = open.entry(from.clone()).or_default();
                            if *n >= per_peer {
                                stream.reset();
                                continue;
                            }
                            *n += 1;
                            let handler = handler.clone();
                            sessions.push(async move {
                                session(handler, from.clone(), stream, first_message).await;
                                from
                            });
                        }
                        None => break,
                    },
                    from = sessions.select_next_some() => {
                        if let Some(n) = open.get_mut(&from) {
                            *n -= 1;
                            if *n == 0 {
                                open.remove(&from);
                            }
                        }
                    }
                }
            }
            while sessions.next().await.is_some() {}
        })
    }
}

/// One served stream: wait up to `first_message` for its first message, then hand the link
/// (that message still unread) to `handler`. A stream that ends or stays silent is dropped.
async fn session(handler: Arc<dyn LinkHandler>, from: String, stream: Stream, first_message: Duration) {
    let Link { tx, mut rx } = frames(stream);
    let first = futures::select! {
        m = rx.recv().fuse() => m,
        _ = futures_timer::Delay::new(first_message).fuse() => return,
    };
    let Ok(Some(first)) = first else { return };
    handler.handle(from, Link { tx, rx: Box::new(Prefetched { first: Some(first), rest: rx }) }).await;
}

/// A link's receiving half with its first message already read.
struct Prefetched {
    first: Option<Vec<u8>>,
    rest: Box<dyn LinkRx>,
}

#[async_trait]
impl LinkRx for Prefetched {
    /// Cancel-safe: the first message is taken whole, and then the inner `recv` is.
    async fn recv(&mut self) -> Result<Option<Vec<u8>>, RouteError> {
        if let Some(m) = self.first.take() {
            return Ok(Some(m));
        }
        self.rest.recv().await
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
        DeviceCallError::ProtocolNotSupported(m) => RouteError::Unsupported(m),
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
    use std::time::Duration;

    use async_trait::async_trait;
    use morph_ai::remote::{LinkRx, LinkTx};
    use morph_ai::{Link, LinkHandler, NodeRouter, OffloadPlan, RouteError};
    use weirdnet_agent::Caller;
    use weirdnet_agent::remote::infer::v2::{EmbedBackend, Finish, InferBackend, Output, Request};
    use weirdnet_agent::remote::infer::{Admission, InferHost, RemoteModel};
    use weirdnet_agent::remote::task::TaskHost;
    use weirdnet_agent::remote::{self, Channel, Dialer, FrameRx, FrameTx, RemoteError};
    use weirdnet_agent::{CancelToken, DeltaSink, Message, ModelBackend, ModelError, ModelTurn, ToolSpec};

    use crate::device_calls::DeviceCaller;

    /// Model turns for inference offload.
    pub const INFER: &str = remote::infer::PROTOCOL;
    /// Model turns, infer/2: queueing, reasoning, tools, sampling (the pool asks it first).
    pub const INFER_V2: &str = remote::infer::v2::PROTOCOL;
    /// Node companion tasks.
    pub const TASK: &str = remote::task::PROTOCOL;

    fn remote_error(e: RouteError) -> RemoteError {
        match e {
            RouteError::NotLinked(p) => RemoteError::Refused { message: format!("{p} is not one of your linked devices") },
            // A device from before infer/2 does not accept its protocol: the pool then asks it infer/1.
            RouteError::Unsupported(m) => RemoteError::Unsupported { message: m },
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

    /// How often a served stream is checked against the roster and the lock while it runs.
    const WATCH: Duration = Duration::from_millis(250);

    /// The gate: the profile is unlocked, and `caller` is a current linked device.
    fn admit(gate: &DeviceCaller, caller: &Caller) -> Result<(), RemoteError> {
        if gate.check_unlocked().is_err() {
            return Err(RemoteError::refused("profile-locked"));
        }
        let presented = match caller.cert_bytes.as_deref().map(weirdnet_agent::serve::decode_cert) {
            None => None,
            Some(Ok(cert)) => Some(cert),
            Some(Err(_)) => return Err(RemoteError::refused("wrong-user")),
        };
        match gate.check(&caller.peer, presented.as_ref()).reason() {
            None => Ok(()),
            Some(reason) => Err(RemoteError::refused(reason)),
        }
    }

    /// Drives `served` for `caller`'s stream, but stops it, running or still queued, within
    /// [`WATCH`] of the profile locking or the caller leaving the roster (revoked, removed).
    /// Needs a runtime with a timer.
    async fn watched(
        gate: &DeviceCaller,
        caller: &Caller,
        served: impl std::future::Future<Output = Result<(), RemoteError>>,
    ) -> Result<(), RemoteError> {
        // Nothing starts for a caller that is already gone: `remote::task::serve` has no
        // authorize hook, and the watcher below would only look after its first tick.
        admit(gate, caller)?;
        let gone = async {
            loop {
                tokio::time::sleep(WATCH).await;
                if let Err(e) = admit(gate, caller) {
                    return e;
                }
            }
        };
        futures::pin_mut!(served, gone);
        match futures::future::select(served, gone).await {
            futures::future::Either::Left((done, _)) => done,
            futures::future::Either::Right((why, _)) => Err(why),
        }
    }

    /// A backend that checks the caller again whenever it is asked to run: a turn waits in the
    /// admission queue after it was authorized, and the caller may be gone by its turn.
    struct Rechecked<B: ?Sized> {
        inner: Arc<B>,
        gate: DeviceCaller,
        caller: Caller,
    }

    impl<B: ?Sized> Rechecked<B> {
        fn wrap(inner: Arc<B>, gate: &DeviceCaller, caller: &Caller) -> Arc<Self> {
            Arc::new(Self { inner, gate: gate.clone(), caller: caller.clone() })
        }
    }

    #[async_trait]
    impl ModelBackend for Rechecked<dyn ModelBackend> {
        fn name(&self) -> String {
            self.inner.name()
        }

        async fn complete(&self, m: &[Message], t: &[ToolSpec], d: &DeltaSink, c: &CancelToken) -> Result<ModelTurn, ModelError> {
            admit(&self.gate, &self.caller).map_err(|e| ModelError::Unavailable(e.to_string()))?;
            self.inner.complete(m, t, d, c).await
        }
    }

    #[async_trait]
    impl InferBackend for Rechecked<dyn InferBackend> {
        fn name(&self) -> String {
            self.inner.name()
        }

        fn resumes(&self) -> bool {
            self.inner.resumes()
        }

        async fn infer(&self, request: &Request, out: &Output, cancel: &CancelToken) -> Result<Finish, RemoteError> {
            admit(&self.gate, &self.caller)?;
            self.inner.infer(request, out, cancel).await
        }
    }

    #[async_trait]
    impl EmbedBackend for Rechecked<dyn EmbedBackend> {
        fn name(&self) -> String {
            self.inner.name()
        }

        async fn embed(&self, input: &str, cancel: &CancelToken) -> Result<Vec<f32>, RemoteError> {
            admit(&self.gate, &self.caller)?;
            self.inner.embed(input, cancel).await
        }
    }

    /// Wraps an [`InferHost`] so only this user's current, non-revoked linked devices may use
    /// it, and nobody may while the profile is locked. `authorize` runs first on every request
    /// (the agent crate calls it before anything else) and `open` and friends check again, so
    /// a host cannot be reached around the gate. The rule is the one [`DeviceCaller`] applies to
    /// streams and app calls: [`weirdnet_sync::Roster::check_caller`] on the live roster.
    pub(crate) struct RosterHost<H> {
        inner: H,
        gate: DeviceCaller,
    }

    impl<H: InferHost> RosterHost<H> {
        /// The profile's lock is the `gate`'s own ([`DeviceCaller::check_unlocked`]), so there is
        /// no profile to forget to pass.
        pub(crate) fn new(inner: H, gate: DeviceCaller) -> Self {
            Self { inner, gate }
        }

        /// The gate: the profile is unlocked, and `caller` (the Noise-authenticated peer, plus
        /// the certificate it presented, if any) is a current linked device.
        pub(crate) fn check(&self, caller: &Caller) -> Result<(), RemoteError> {
            admit(&self.gate, caller)
        }
    }

    impl<H: InferHost> InferHost for RosterHost<H> {
        fn authorize(&self, caller: &Caller) -> Result<(), RemoteError> {
            self.check(caller)?;
            self.inner.authorize(caller)
        }

        fn open(&self, caller: &Caller, model: Option<&str>) -> Result<Arc<dyn ModelBackend>, RemoteError> {
            self.check(caller)?;
            Ok(Rechecked::wrap(self.inner.open(caller, model)?, &self.gate, caller))
        }

        fn open_request(
            &self,
            caller: &Caller,
            request: &remote::infer::v2::Request,
        ) -> Result<Arc<dyn remote::infer::v2::InferBackend>, RemoteError> {
            self.check(caller)?;
            Ok(Rechecked::wrap(self.inner.open_request(caller, request)?, &self.gate, caller))
        }

        fn open_embed(&self, caller: &Caller, model: Option<&str>) -> Result<Arc<dyn remote::infer::v2::EmbedBackend>, RemoteError> {
            self.check(caller)?;
            Ok(Rechecked::wrap(self.inner.open_embed(caller, model)?, &self.gate, caller))
        }
    }

    /// Serves [`INFER`]: model turns from the user's other devices, `admission` at a time. The
    /// host is always behind a [`RosterHost`], so there is no way to serve it ungated.
    pub struct InferService {
        host: Arc<dyn InferHost>,
        gate: DeviceCaller,
        admission: Admission,
        /// Serve infer/2 on this stream (register the service under [`INFER_V2`]), else infer/1.
        v2: bool,
    }

    impl InferService {
        pub fn new<H: InferHost + 'static>(host: H, gate: DeviceCaller, admission: Admission) -> Self {
            Self { host: Arc::new(RosterHost::new(host, gate.clone())), gate, admission, v2: false }
        }

        /// The same service for [`INFER_V2`] streams.
        pub fn v2(mut self) -> Self {
            self.v2 = true;
            self
        }
    }

    #[async_trait]
    impl LinkHandler for InferService {
        /// `from` is the stream's Noise-authenticated peer id (never one the caller sent). The
        /// link carries no certificate, so the peer id is checked against the roster alone.
        async fn handle(&self, from: String, link: Link) {
            let caller = Caller::new(from.clone());
            let served = if self.v2 {
                watched(&self.gate, &caller, remote::infer::v2::serve(channel(link), caller.clone(), self.host.as_ref(), &self.admission)).await
            } else {
                watched(&self.gate, &caller, remote::infer::serve(channel(link), &caller, self.host.as_ref(), &self.admission)).await
            };
            if let Err(e) = served {
                eprintln!("morph-agent: inference for {from}: {e}");
            }
        }
    }

    /// Serves [`TASK`]: Node companion tasks, cards answered on the asking device.
    pub struct TaskService {
        host: Arc<dyn TaskHost>,
        gate: DeviceCaller,
    }

    impl TaskService {
        /// `gate` is checked while a task runs, not only when its stream is accepted: a revoke or a
        /// lock ends the task within 250 ms.
        pub fn new(host: Arc<dyn TaskHost>, gate: DeviceCaller) -> Self {
            Self { host, gate }
        }
    }

    #[async_trait]
    impl LinkHandler for TaskService {
        async fn handle(&self, from: String, link: Link) {
            let caller = Caller::new(from.clone());
            if let Err(e) = watched(&self.gate, &caller, remote::task::serve(channel(link), self.host.as_ref())).await {
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
        let plan = morph_ai::offload_plan(&morph_ai::OffloadSettings::default(), Platform::Phone, &nodes, true);
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

    /// `recv` on `link`, but `None` when nothing came within `wait` (a test never hangs).
    fn recv_within(link: &mut Link, wait: std::time::Duration) -> Option<Result<Option<Vec<u8>>, RouteError>> {
        rt(async {
            futures::select! {
                m = link.rx.recv().fuse() => Some(m),
                _ = futures_timer::Delay::new(wait).fuse() => None,
            }
        })
    }

    /// Counts the sessions that reached the handler, then echoes.
    struct Counted(Arc<std::sync::atomic::AtomicUsize>);

    #[async_trait]
    impl LinkHandler for Counted {
        async fn handle(&self, from: String, link: Link) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Echo.handle(from, link).await
        }
    }

    #[test]
    fn a_stream_that_says_nothing_is_closed_before_the_handler_sees_it() {
        let user = Keypair::generate_ed25519();
        let (a, b) = (device(&user, "a"), device(&user, "b"));
        let (server, _, listening) = up(&user, &a, &[&b.cert], None);
        let at: Multiaddr = format!("{}/p2p/{}", listening[0], server.peer_id()).parse().unwrap();
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let served = DeviceRouter::new(server).with_session_limits(std::time::Duration::from_millis(400), MAX_SESSIONS_PER_PEER);
        let serving = served.serve(protocols::TASK, Arc::new(Counted(seen.clone()))).unwrap();
        std::thread::spawn(move || rt(serving));

        let (from_b, _, _) = up(&user, &b, &[&a.cert], Some(at));
        let b_router = DeviceRouter::new(from_b);
        let mut silent = rt(b_router.open(&a.cert.peer_id, protocols::TASK)).unwrap();
        let started = std::time::Instant::now();
        let got = recv_within(&mut silent, std::time::Duration::from_secs(20)).expect("the silent stream is closed, not left open");
        assert!(!matches!(got, Ok(Some(_))), "{got:?}");
        assert!(started.elapsed() >= std::time::Duration::from_millis(300), "closed only after the first-message time");
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 0, "the handler never saw it");

        // A stream that speaks in time is served, its first message included.
        let mut talking = rt(b_router.open(&a.cert.peer_id, protocols::TASK)).unwrap();
        rt(talking.tx.send(b"hi".to_vec())).unwrap();
        let reply = recv_within(&mut talking, std::time::Duration::from_secs(20)).unwrap().unwrap().unwrap();
        assert_eq!(reply, format!("{}:hi", b.cert.peer_id).into_bytes());
        rt(talking.tx.send(b"again".to_vec())).unwrap();
        let reply = recv_within(&mut talking, std::time::Duration::from_secs(20)).unwrap().unwrap().unwrap();
        assert_eq!(reply, format!("{}:again", b.cert.peer_id).into_bytes());
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_device_over_its_session_cap_is_refused_and_others_are_not() {
        let user = Keypair::generate_ed25519();
        let (a, b, d) = (device(&user, "a"), device(&user, "b"), device(&user, "d"));
        let (server, _, listening) = up(&user, &a, &[&b.cert, &d.cert], None);
        let at: Multiaddr = format!("{}/p2p/{}", listening[0], server.peer_id()).parse().unwrap();
        let served = DeviceRouter::new(server).with_session_limits(std::time::Duration::from_secs(30), 2);
        let serving = served.serve(protocols::TASK, Arc::new(Echo)).unwrap();
        std::thread::spawn(move || rt(serving));
        let wait = std::time::Duration::from_secs(20);
        let echoes = |link: &mut Link, peer: &str, text: &str| {
            if let Err(e) = rt(link.tx.send(text.as_bytes().to_vec())) {
                panic!("send {text}: {e:?}");
            }
            let got = recv_within(link, wait);
            assert!(matches!(&got, Some(Ok(Some(m))) if *m == format!("{peer}:{text}").into_bytes()), "{text}: {got:?}");
        };

        let (from_b, _, _) = up(&user, &b, &[&a.cert], Some(at.clone()));
        let b_router = DeviceRouter::new(from_b);
        let mut one = rt(b_router.open(&a.cert.peer_id, protocols::TASK)).unwrap();
        let mut two = rt(b_router.open(&a.cert.peer_id, protocols::TASK)).unwrap();
        echoes(&mut one, &b.cert.peer_id, "one");
        echoes(&mut two, &b.cert.peer_id, "two");

        // Another device has its own allowance.
        let (from_d, _, _) = up(&user, &d, &[&a.cert], Some(at));
        let d_router = DeviceRouter::new(from_d);
        let mut theirs = rt(d_router.open(&a.cert.peer_id, protocols::TASK)).unwrap();
        echoes(&mut theirs, &d.cert.peer_id, "mine");

        // A third session from B is reset at once.
        let mut three = rt(b_router.open(&a.cert.peer_id, protocols::TASK)).unwrap();
        let _ = rt(three.tx.send(b"three".to_vec()));
        let got = recv_within(&mut three, wait).expect("the extra stream is reset, not left hanging");
        assert!(!matches!(got, Ok(Some(_))), "{got:?}");

        // When one of B's sessions ends, B may open another.
        drop(one);
        let deadline = std::time::Instant::now() + wait;
        loop {
            let mut again = rt(b_router.open(&a.cert.peer_id, protocols::TASK)).unwrap();
            let _ = rt(again.tx.send(b"again".to_vec()));
            if let Some(Ok(Some(reply))) = recv_within(&mut again, std::time::Duration::from_secs(5)) {
                assert_eq!(reply, format!("{}:again", b.cert.peer_id).into_bytes());
                break;
            }
            assert!(std::time::Instant::now() < deadline, "the ended session's place never freed up");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        echoes(&mut two, &b.cert.peer_id, "still");
    }

    mod agent_protocols {
        use super::*;
        use morph_ai::OffloadPlan;
        use weirdnet_agent::Caller;
        use weirdnet_agent::remote::RemoteError;
        use weirdnet_agent::remote::infer::{Admission, InferHost, Route};
        use weirdnet_agent::remote::task::{TaskHost, TaskRequest};
        use weirdnet_agent::testing::{ScriptedApprover, ScriptedModel};
        use weirdnet_agent::{
            AgentConfig, AgentId, AgentIdentity, AgentKind, AgentSession, Approver, CancelToken, DeltaSink, Message, ModelBackend, ModelTurn, Policy,
            StopReason, ToolRegistry,
        };

        use super::protocols::{InferService, RosterHost, RouterDialer, TaskService, remote_model};

        fn said(text: &str) -> Arc<dyn ModelBackend> {
            Arc::new(ScriptedModel::new(vec![ModelTurn { text: text.into(), tool_calls: vec![] }]))
        }

        struct Models;
        impl InferHost for Models {
            /// The roster gate is `RosterHost`'s (what the service wraps this in); the model host itself admits.
            fn authorize(&self, _caller: &Caller) -> Result<(), RemoteError> {
                Ok(())
            }
            fn open(&self, _caller: &Caller, _model: Option<&str>) -> Result<Arc<dyn ModelBackend>, RemoteError> {
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
            devices_serving(rt, true)
        }

        /// `v2`: the desk also serves infer/2 (else it is a 0.3-era node: infer/1 only).
        fn devices_serving(rt: &tokio::runtime::Runtime, v2: bool) -> (Arc<dyn NodeRouter>, String, String) {
            let user = Keypair::generate_ed25519();
            let (a, b, c) = (device(&user, "desk"), device(&user, "phone"), device(&user, "c"));
            let (server, _, listening) = up(&user, &a, &[&b.cert], None);
            let at: Multiaddr = format!("{}/p2p/{}", listening[0], server.peer_id()).parse().unwrap();
            let desk = DeviceRouter::new(server.clone());
            if v2 {
                rt.spawn(desk.serve(protocols::INFER_V2, Arc::new(InferService::new(Models, server.clone(), Admission::new(2)).v2())).unwrap());
            }
            rt.spawn(desk.serve(protocols::INFER, Arc::new(InferService::new(Models, server.clone(), Admission::new(2)))).unwrap());
            rt.spawn(desk.serve(protocols::TASK, Arc::new(TaskService::new(Arc::new(Companions), server.clone()))).unwrap());
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

        /// The host's roster gate: A serves, B is linked, D was revoked, C is the user's but unknown, X is another user's.
        fn gate_fixture() -> (RosterHost<Models>, Device, Device, Device, Device) {
            let user = Keypair::generate_ed25519();
            let (a, b, c, d) = (device(&user, "desk"), device(&user, "phone"), device(&user, "c"), device(&user, "d"));
            let (server, st, _) = up(&user, &a, &[&b.cert, &d.cert], None);
            st.lock().unwrap().roster.revoke(&WEIRDNET, &user, &d.cert.peer_id, 5).unwrap();
            (RosterHost::new(Models, server), a, b, c, d)
        }

        #[test]
        fn a_linked_device_may_use_the_inference_host() {
            let (host, _a, b, _c, _d) = gate_fixture();
            let caller = Caller::new(b.cert.peer_id.clone());
            assert!(host.authorize(&caller).is_ok());
            assert!(host.open(&caller, None).is_ok());
            // With its own certificate presented, too.
            let with_cert = Caller::new(b.cert.peer_id.clone()).with_cert(weirdnet_agent::serve::encode_cert(&b.cert));
            assert!(host.authorize(&with_cert).is_ok());
        }

        #[test]
        fn a_device_that_is_not_in_the_roster_is_refused() {
            let (host, a, _b, c, _d) = gate_fixture();
            let refused = |caller: Caller| match host.authorize(&caller) {
                Err(RemoteError::Refused { message }) => message,
                other => panic!("not refused: {other:?}"),
            };
            assert_eq!(refused(Caller::new(c.cert.peer_id.clone())), "unknown-device", "the user's own, never linked here");
            assert_eq!(refused(Caller::new("12D3KooWNotADevice")), "unknown-device");
            assert!(host.open(&Caller::new(c.cert.peer_id.clone()), None).is_err(), "open is gated too, not only authorize");
            // A minted cert does not make a device a member; nor does a cert that is not the caller's.
            let minted = Caller::new(c.cert.peer_id.clone()).with_cert(weirdnet_agent::serve::encode_cert(&c.cert));
            assert_eq!(refused(minted), "unknown-device");
            let borrowed = Caller::new(c.cert.peer_id.clone()).with_cert(weirdnet_agent::serve::encode_cert(&a.cert));
            assert_eq!(refused(borrowed), "wrong-user");
            let garbage = Caller::new(c.cert.peer_id.clone()).with_cert(vec![1, 2, 3]);
            assert_eq!(refused(garbage), "wrong-user");
            // Another user's device, with its own valid cert.
            let stranger = device(&Keypair::generate_ed25519(), "x");
            let theirs = Caller::new(stranger.cert.peer_id.clone()).with_cert(weirdnet_agent::serve::encode_cert(&stranger.cert));
            assert_eq!(refused(theirs), "wrong-user");
        }

        #[test]
        fn a_revoked_device_is_refused() {
            let (host, _a, _b, _c, d) = gate_fixture();
            let caller = Caller::new(d.cert.peer_id.clone());
            assert!(matches!(host.authorize(&caller), Err(RemoteError::Refused { message }) if message == "revoked"));
            assert!(host.open(&caller, None).is_err());
            let with_cert = Caller::new(d.cert.peer_id.clone()).with_cert(weirdnet_agent::serve::encode_cert(&d.cert));
            assert!(matches!(host.authorize(&with_cert), Err(RemoteError::Refused { message }) if message == "revoked"));
        }

        #[test]
        fn a_locked_profile_refuses_even_a_linked_device() {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("h");
            morph_core::profiles::load_catalog(&home).unwrap();
            crate::identity::create(&home, "default", &morph_identity::profile_identity::SecretString::from("a long password".to_string())).unwrap();
            let user = Keypair::generate_ed25519();
            let (a, b) = (device(&user, "desk"), device(&user, "phone"));
            let (server, _, _) = up(&user, &a, &[&b.cert], None);
            let host = RosterHost::new(Models, server.for_locked_profile(&home, "default"));
            let caller = Caller::new(b.cert.peer_id.clone());
            assert!(host.authorize(&caller).is_ok(), "unlocked: a member gets in");
            crate::profile_lock::lock(&home, "default");
            assert!(matches!(host.authorize(&caller), Err(RemoteError::Refused { message }) if message == "profile-locked"));
            assert!(host.open(&caller, None).is_err());
        }

        /// A link whose far end says nothing, counting how often the serving side waits on it.
        struct Quiet(Arc<std::sync::atomic::AtomicUsize>);
        struct Mute(Arc<std::sync::atomic::AtomicUsize>);

        #[async_trait]
        impl LinkTx for Mute {
            async fn send(&mut self, _message: Vec<u8>) -> Result<(), RouteError> {
                Ok(())
            }
        }

        #[async_trait]
        impl LinkRx for Quiet {
            async fn recv(&mut self) -> Result<Option<Vec<u8>>, RouteError> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::future::pending().await
            }
        }

        fn quiet_link() -> (Link, Arc<std::sync::atomic::AtomicUsize>) {
            let waits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            (Link { tx: Box::new(Mute(waits.clone())), rx: Box::new(Quiet(waits.clone())) }, waits)
        }

        /// How long `handle` takes to return for `peer` (None: still running after a second) and
        /// whether it ever waited for the caller's request.
        fn handle_task(rt: &tokio::runtime::Runtime, service: &TaskService, peer: &str) -> (Option<()>, usize) {
            let (link, waits) = quiet_link();
            let done =
                rt.block_on(async { tokio::time::timeout(std::time::Duration::from_secs(1), service.handle(peer.to_string(), link)).await.ok() });
            (done, waits.load(std::sync::atomic::Ordering::SeqCst))
        }

        #[test]
        fn a_caller_revoked_or_locked_before_its_task_starts_never_starts_it() {
            let rt = tokio();
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("h");
            morph_core::profiles::load_catalog(&home).unwrap();
            crate::identity::create(&home, "default", &morph_identity::profile_identity::SecretString::from("a long password".to_string())).unwrap();
            let user = Keypair::generate_ed25519();
            let (a, b, d) = (device(&user, "desk"), device(&user, "phone"), device(&user, "gone"));
            let (server, st, _) = up(&user, &a, &[&b.cert, &d.cert], None);
            let server = server.for_locked_profile(&home, "default");
            let service = TaskService::new(Arc::new(Companions), server);

            // A member is waited for: the task would start on its first message.
            let (done, waits) = handle_task(&rt, &service, &b.cert.peer_id);
            assert!(done.is_none() && waits >= 1, "a member's task runs (waits for its request): {done:?} {waits}");

            // Revoked between the accept and the first message: returns at once, reads nothing.
            drop(st.lock().unwrap().roster.revoke(&WEIRDNET, &user, &d.cert.peer_id, 5).unwrap());
            let (done, waits) = handle_task(&rt, &service, &d.cert.peer_id);
            assert!(done.is_some() && waits == 0, "revoked: {done:?} {waits}");

            // Locked: nobody's task starts.
            crate::profile_lock::lock(&home, "default");
            let (done, waits) = handle_task(&rt, &service, &b.cert.peer_id);
            assert!(done.is_some() && waits == 0, "locked: {done:?} {waits}");
        }

        /// A host whose turns take half a minute (or until they are stopped) and count their starts.
        struct SlowHost(Arc<std::sync::atomic::AtomicUsize>);

        struct Slow(Arc<std::sync::atomic::AtomicUsize>);

        #[async_trait]
        impl ModelBackend for Slow {
            fn name(&self) -> String {
                "slow".into()
            }
            async fn complete(
                &self,
                _m: &[Message],
                _t: &[weirdnet_agent::tool::ToolSpec],
                _d: &DeltaSink,
                _c: &CancelToken,
            ) -> Result<ModelTurn, weirdnet_agent::ModelError> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                Ok(ModelTurn { text: "too late".into(), tool_calls: vec![] })
            }
        }

        impl InferHost for SlowHost {
            fn authorize(&self, _caller: &Caller) -> Result<(), RemoteError> {
                Ok(())
            }
            fn open(&self, _caller: &Caller, _model: Option<&str>) -> Result<Arc<dyn ModelBackend>, RemoteError> {
                Ok(Arc::new(Slow(self.0.clone())))
            }
        }

        /// A companion task host whose model takes half a minute.
        struct SlowTasks(Arc<std::sync::atomic::AtomicUsize>);

        impl TaskHost for SlowTasks {
            fn session(&self, _request: &TaskRequest, approver: Arc<dyn Approver>) -> Result<AgentSession, RemoteError> {
                let identity = AgentIdentity::new("atlas", "Atlas", AgentKind::Node).with_node("desk");
                Ok(AgentSession::new(
                    AgentConfig::new(identity),
                    Arc::new(Slow(self.0.clone())),
                    Arc::new(ToolRegistry::new()),
                    Policy::new().shared(),
                    approver,
                ))
            }
        }

        /// A desk serving slow turns one at a time with two queued, a phone linked to it, and
        /// what it takes to revoke the phone or lock the profile.
        struct Slowed {
            rt: tokio::runtime::Runtime,
            phone: Arc<dyn NodeRouter>,
            desk: String,
            started: Arc<std::sync::atomic::AtomicUsize>,
            revoke: Box<dyn Fn()>,
            _tmp: tempfile::TempDir,
            home: std::path::PathBuf,
        }

        fn slowed() -> Slowed {
            let tmp = tempfile::tempdir().unwrap();
            let home = tmp.path().join("h");
            morph_core::profiles::load_catalog(&home).unwrap();
            crate::identity::create(&home, "default", &morph_identity::profile_identity::SecretString::from("a long password".to_string())).unwrap();
            let rt = tokio();
            let user = Keypair::generate_ed25519();
            let (a, b) = (device(&user, "desk"), device(&user, "phone"));
            let (server, st, listening) = up(&user, &a, &[&b.cert], None);
            let server = server.for_locked_profile(&home, "default");
            let server2 = server.clone();
            let at: Multiaddr = format!("{}/p2p/{}", listening[0], server.peer_id()).parse().unwrap();
            let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let desk = DeviceRouter::new(server.clone());
            rt.spawn(
                desk.serve(protocols::INFER_V2, Arc::new(InferService::new(SlowHost(started.clone()), server, Admission::with_queue(1, 2)).v2()))
                    .unwrap(),
            );
            rt.spawn(desk.serve(protocols::TASK, Arc::new(TaskService::new(Arc::new(SlowTasks(started.clone())), server2))).unwrap());
            let (phone, _, _) = up(&user, &b, &[&a.cert], Some(at));
            let peer = b.cert.peer_id.clone();
            let revoke = Box::new(move || drop(st.lock().unwrap().roster.revoke(&WEIRDNET, &user, &peer, 9).unwrap()));
            Slowed { rt, phone: Arc::new(DeviceRouter::new(phone)), desk: a.cert.peer_id, started, revoke, _tmp: tmp, home }
        }

        impl Slowed {
            /// Three turns from the phone: one runs, two wait their turn. Returns how long it took
            /// them all to end (error or answer) after `stop` ran once the first had started.
            fn three_turns_then(&self, stop: impl FnOnce()) -> (std::time::Duration, Vec<Result<ModelTurn, weirdnet_agent::ModelError>>) {
                let plan = OffloadPlan { nodes: vec![self.desk.clone()], fallback_local: false };
                self.rt.block_on(async {
                    let turns: Vec<_> = (0..3)
                        .map(|_| {
                            let model = remote_model(self.phone.clone(), &plan, None);
                            tokio::spawn(async move { model.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new()).await })
                        })
                        .collect();
                    while self.started.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    // Let the other two reach the queue.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let at = std::time::Instant::now();
                    stop();
                    let mut ended = Vec::new();
                    for t in turns {
                        ended.push(tokio::time::timeout(std::time::Duration::from_secs(10), t).await.expect("the turn ended").unwrap());
                    }
                    (at.elapsed(), ended)
                })
            }
        }

        #[test]
        fn a_revoked_device_loses_its_running_and_queued_turns_at_once() {
            let s = slowed();
            let (took, ended) = s.three_turns_then(|| (s.revoke)());
            assert!(took < std::time::Duration::from_secs(3), "stopped in {took:?}, not after the 30 s turns");
            assert!(ended.iter().all(|t| t.is_err()), "{ended:?}");
            assert_eq!(s.started.load(std::sync::atomic::Ordering::SeqCst), 1, "a queued turn never started after the revoke");
        }

        #[test]
        fn locking_the_profile_stops_its_running_and_queued_turns_at_once() {
            let s = slowed();
            let (took, ended) = s.three_turns_then(|| crate::profile_lock::lock(&s.home, "default"));
            assert!(took < std::time::Duration::from_secs(3), "stopped in {took:?}, not after the 30 s turns");
            assert!(ended.iter().all(|t| t.is_err()), "{ended:?}");
            assert_eq!(s.started.load(std::sync::atomic::Ordering::SeqCst), 1);
        }

        #[test]
        fn a_revoke_or_a_lock_ends_a_running_companion_task() {
            for lock in [false, true] {
                let s = slowed();
                let dialer = RouterDialer(s.phone.clone());
                let result = s.rt.block_on(async {
                    let req = TaskRequest::new(AgentId::new("atlas"), "slowly", vec![]);
                    let approver = ScriptedApprover::new(vec![]);
                    let cancel = CancelToken::new();
                    let task = weirdnet_agent::remote::task::run(&dialer, &s.desk, req, None, &approver, &cancel);
                    let stop = async {
                        while s.started.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        }
                        if lock {
                            crate::profile_lock::lock(&s.home, "default")
                        } else {
                            (s.revoke)()
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    };
                    let at = std::time::Instant::now();
                    futures::pin_mut!(task, stop);
                    match futures::future::select(task, stop).await {
                        futures::future::Either::Left((r, _)) => Some((r, at.elapsed())),
                        futures::future::Either::Right(_) => None,
                    }
                });
                let (r, _) = result.unwrap_or_else(|| panic!("the task kept running after the {}", if lock { "lock" } else { "revoke" }));
                assert!(r.is_err(), "{r:?}");
            }
        }

        #[test]
        fn a_backend_opened_before_a_revoke_refuses_to_run_after_it() {
            // What a queued request holds when its turn comes: checked again, not trusted from before.
            let user = Keypair::generate_ed25519();
            let (a, b) = (device(&user, "desk"), device(&user, "phone"));
            let (server, st, _) = up(&user, &a, &[&b.cert], None);
            let host = RosterHost::new(SlowHost(Arc::default()), server);
            let caller = Caller::new(b.cert.peer_id.clone());
            let backend = host.open(&caller, None).unwrap();
            st.lock().unwrap().roster.revoke(&WEIRDNET, &user, &b.cert.peer_id, 9).unwrap();
            let r = rt(backend.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new()));
            assert!(matches!(r, Err(weirdnet_agent::ModelError::Unavailable(m)) if m.contains("revoked")), "refused at run time");
        }

        #[test]
        fn a_node_from_before_infer_2_is_asked_infer_1() {
            let rt = tokio();
            let (router, desk, _) = devices_serving(&rt, false);
            let plan = OffloadPlan { nodes: vec![desk.clone()], fallback_local: false };
            let model = remote_model(router, &plan, None);
            let turn = rt.block_on(model.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new())).unwrap();
            assert_eq!(turn.text, "from the desktop");
            assert!(matches!(model.last_route(), Some(Route::Node { node, .. }) if node == desk));
        }

        /// Refuses infer/2 dials with the typed not-supported error, its text saying nothing
        /// of it, and passes the rest to the real router.
        struct WithoutInferV2(Arc<dyn NodeRouter>);

        #[async_trait]
        impl NodeRouter for WithoutInferV2 {
            fn nodes(&self) -> Vec<LinkedNode> {
                self.0.nodes()
            }
            async fn open(&self, peer: &str, protocol: &str) -> Result<Link, RouteError> {
                if protocol == protocols::INFER_V2 {
                    return Err(RouteError::Unsupported("infer/2".into()));
                }
                self.0.open(peer, protocol).await
            }
        }

        #[test]
        fn the_infer_1_fallback_follows_the_typed_not_supported_error() {
            let rt = tokio();
            let (router, desk, _) = devices(&rt);
            let plan = OffloadPlan { nodes: vec![desk.clone()], fallback_local: false };
            let model = remote_model(Arc::new(WithoutInferV2(router)), &plan, None);
            let turn = rt.block_on(model.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new())).unwrap();
            assert_eq!(turn.text, "from the desktop", "infer/1 answered");
            assert!(matches!(model.last_route(), Some(Route::Node { node, .. }) if node == desk));
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
