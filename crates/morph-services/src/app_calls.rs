//! Calling Weirdnet apps for `weirdnet-<app>://` addresses, over
//! weirdnet-net's app calls (`/weirdnet/app/1`).
//!
//! [`NetAppCaller`] is the [`AppCaller`] both shells use. On its first call
//! it starts a node of its own on a throwaway key (it only calls; it serves
//! nothing), which reaches the same relays device links use
//! ([`crate::relays::relay_addrs`], every address of each at once) and asks
//! them, as names servers, where users are. The node then stays up for the
//! next call, and is started again if the relay list changes.
//!
//! One call: [`Node::resolve_app`] turns the locator (a peer id, a 32-hex
//! user id, or a name bound to a user by a `_weirdnet.<name>` TXT record)
//! into the peers that serve the app, connecting to each (directly, else
//! through a relay circuit); then [`Node::app_call_via`] sends the request
//! to the first that answers. Both block, so the shells call this off the UI
//! thread ([`morph_core::app_call::fetch`]). The body streams: reading it
//! waits for the next chunk, and dropping it resets the call.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use morph_core::app_call::{self, AppCallError, AppCaller, AppRequest, AppResponse};
use morph_core::network::{self as netprefs, NetworkSettings};
use weirdnet_net::profile::{Profile, WEIRDNET};
use weirdnet_net::{AppBody, AppError, Multiaddr, Node, NodeConfig, PeerId};

/// How long a call may take to connect and get the app's status and headers.
/// After that only the idle timeout applies ([`IDLE_TIMEOUT`]).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a body may go without a byte before the call is given up.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long finding and connecting to a peer may wait on one request.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// The app-call node's identifiers: Weirdnet's own (so names servers and
/// the DHT are the ones Weirdnet apps use), on a thread of its own name.
static PROFILE: Profile = Profile { thread: "morph-app-calls", ..WEIRDNET };

/// The node an app caller runs, for `relays` (the device-link relays).
pub fn caller_config(relays: Vec<Multiaddr>) -> NodeConfig {
    NodeConfig {
        listen: NodeConfig::default_listen(0),
        names_servers: relays.clone(),
        relays,
        label: "morph-app-calls".into(),
        request_timeout: CONNECT_TIMEOUT,
        app_idle_timeout: IDLE_TIMEOUT,
        app_body_max: app_call::BODY_MAX + 1,
        ..Default::default()
    }
}

/// Answers nothing: the app-call node takes no requests.
struct NoRequests;

impl weirdnet_net::Handler<(), ()> for NoRequests {
    fn request(&self, _from: &PeerId, _req: ()) {}
}

/// The node in use and the relays it was started with.
struct Running {
    relays: Vec<Multiaddr>,
    discovery: bool,
    peers: Vec<netprefs::PeerEntry>,
    node: Arc<Node<(), ()>>,
}

/// Calls Weirdnet apps over weirdnet-net (see the module docs).
pub struct NetAppCaller {
    config: Box<dyn Fn() -> NodeConfig + Send + Sync>,
    /// Overlay `network.json` (and dial manual peers). Off in tests that
    /// pass their own [`NetAppCaller::with_config`].
    use_install_network: bool,
    running: Mutex<Option<Running>>,
}

impl Default for NetAppCaller {
    fn default() -> Self {
        Self::new()
    }
}

impl NetAppCaller {
    /// A caller over the device-link relays (`MORPH_LINK_RELAYS` or the
    /// public relay, with the addresses each advertises).
    pub fn new() -> Self {
        let mut s = Self::with_config(|| caller_config(crate::relays::relay_addrs()));
        s.use_install_network = true;
        s
    }

    /// A caller whose node runs with `config()` (tests: an in-memory network).
    pub fn with_config(config: impl Fn() -> NodeConfig + Send + Sync + 'static) -> Self {
        Self { config: Box::new(config), use_install_network: false, running: Mutex::new(None) }
    }

    fn install_network() -> NetworkSettings {
        crate::home::resolve(None).ok().and_then(|home| netprefs::load(&home).ok()).unwrap_or_default()
    }

    /// The running node, started (again) when there is none or the relays
    /// changed.
    fn node(&self) -> Result<Arc<Node<(), ()>>, AppCallError> {
        let mut cfg = (self.config)();
        let net = if self.use_install_network { Self::install_network() } else { NetworkSettings::default() };
        if self.use_install_network {
            cfg = crate::network::apply_to_config(cfg, &net);
        }
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(r) = running.as_ref()
            && r.relays == cfg.relays
            && r.discovery == net.discovery_enabled()
            && r.peers == net.peers()
            && r.node.running()
        {
            return Ok(r.node.clone());
        }
        *running = None;
        let relays = cfg.relays.clone();
        let node = Node::start(&PROFILE, weirdnet_net::Keypair::generate_ed25519(), cfg, Arc::new(NoRequests))
            .map_err(|e| AppCallError::Unreachable(format!("Weirdnet could not start: {e}")))?;
        if self.use_install_network {
            crate::network::dial_manual_peers(&node, net.peers());
        }
        let node = Arc::new(node);
        *running = Some(Running { relays, discovery: net.discovery_enabled(), peers: net.peers().to_vec(), node: node.clone() });
        Ok(node)
    }

    /// Stop the node (it starts again on the next call).
    pub fn stop(&self) {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}

/// The one caller the shells share in a process.
pub fn shared() -> Arc<NetAppCaller> {
    static SHARED: std::sync::OnceLock<Arc<NetAppCaller>> = std::sync::OnceLock::new();
    SHARED.get_or_init(|| Arc::new(NetAppCaller::new())).clone()
}

/// The locator as weirdnet-net takes it: a peer id as typed (base58 is
/// case-sensitive), names and user ids in lowercase.
pub fn locator(typed: &str) -> String {
    let typed = typed.trim();
    if typed.parse::<PeerId>().is_ok() { typed.to_string() } else { typed.to_ascii_lowercase() }
}

/// What a weirdnet-net app-call error means for the call to `request`.
pub fn map_error(error: AppError, request: &AppRequest) -> AppCallError {
    match error {
        AppError::NotFound => AppCallError::NotFound(request.locator.clone()),
        AppError::NoHandler => AppCallError::NoHandler(request.app.clone()),
        AppError::Unreachable(why) => AppCallError::Unreachable(why),
        AppError::TimedOut => AppCallError::TimedOut,
        AppError::Refused(why) => AppCallError::Refused(why),
        AppError::TooLarge => AppCallError::TooLarge,
        AppError::Protocol(why) => AppCallError::Protocol(why),
    }
}

/// Of two failures, the one that says more: reaching a peer that then
/// failed beats not finding one.
fn rank(e: &AppCallError) -> u8 {
    match e {
        AppCallError::NotFound(_) => 0,
        AppCallError::Unreachable(_) | AppCallError::TimedOut => 1,
        AppCallError::NoHandler(_) => 2,
        _ => 3,
    }
}

impl AppCaller for NetAppCaller {
    fn call(&self, request: AppRequest) -> Result<AppResponse, AppCallError> {
        let app = request.app_name().to_string();
        if !app_call::valid_app_name(&app) {
            return Err(AppCallError::InvalidApp(app));
        }
        let node = self.node()?;
        let peers = node.resolve_app(&app, &locator(&request.locator)).map_err(|e| map_error(e, &request))?;
        let mut last = AppCallError::NotFound(request.locator.clone());
        for (peer, addrs) in peers {
            let mut req = weirdnet_net::AppRequest::new(&app, &request.method, &request.path);
            for (k, v) in &request.headers {
                req = req.header(k, v);
            }
            req = req.body(request.body.clone());
            match futures::executor::block_on(node.app_call_via(peer, &addrs, req, CALL_TIMEOUT)) {
                Ok(resp) => {
                    let who = AppRequest { body: vec![], ..request.clone() };
                    let body = Box::new(BodyReader { body: resp.body, chunk: vec![], at: 0, who, _node: node.clone() });
                    return Ok(AppResponse { status: resp.status, headers: resp.headers, body });
                }
                Err(e) => {
                    let e = map_error(e, &request);
                    if rank(&e) >= rank(&last) {
                        last = e;
                    }
                }
            }
        }
        Err(last)
    }
}

/// A streamed body as [`Read`]: each read waits for the next chunk.
/// Holds the node so it outlives the call.
struct BodyReader {
    body: AppBody,
    chunk: Vec<u8>,
    at: usize,
    /// The call, for its errors (no body).
    who: AppRequest,
    _node: Arc<Node<(), ()>>,
}

impl Read for BodyReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.at >= self.chunk.len() {
            match futures::executor::block_on(self.body.next()) {
                None => return Ok(0),
                Some(Ok(chunk)) => {
                    self.chunk = chunk;
                    self.at = 0;
                }
                Some(Err(e)) => return Err(app_call::io_error(map_error(e, &self.who))),
            }
        }
        let n = buf.len().min(self.chunk.len() - self.at);
        buf[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;
    use weirdnet_net::names::Names;
    use weirdnet_net::node::RelayLimits;

    fn req(app: &str, locator: &str) -> AppRequest {
        app_call::request(&format!("weirdnet-{app}://{locator}/x")).unwrap()
    }

    #[test]
    fn every_weirdnet_error_maps_to_a_page() {
        let r = req("notes", "alice.example");
        assert_eq!(map_error(AppError::NotFound, &r), AppCallError::NotFound("alice.example".into()));
        assert_eq!(map_error(AppError::NoHandler, &r), AppCallError::NoHandler("weirdnet-notes".into()));
        assert_eq!(map_error(AppError::Unreachable("no route".into()), &r), AppCallError::Unreachable("no route".into()));
        assert_eq!(map_error(AppError::TimedOut, &r), AppCallError::TimedOut);
        assert_eq!(map_error(AppError::Refused("members only".into()), &r), AppCallError::Refused("members only".into()));
        assert_eq!(map_error(AppError::TooLarge, &r), AppCallError::TooLarge);
        assert_eq!(map_error(AppError::Protocol("bad".into()), &r), AppCallError::Protocol("bad".into()));
        let titles: Vec<String> = [AppError::NotFound, AppError::NoHandler, AppError::Unreachable("x".into()), AppError::TimedOut]
            .into_iter()
            .map(|e| app_call::error_text("weirdnet-notes://alice.example/x", &map_error(e, &r)).0)
            .collect();
        assert_eq!(
            titles,
            [
                "Couldn't find alice.example",
                "alice.example doesn't serve weirdnet-notes://",
                "Couldn't reach alice.example",
                "The app didn't answer in time"
            ]
        );
        assert!(rank(&AppCallError::NoHandler("x".into())) > rank(&AppCallError::NotFound("x".into())));
    }

    #[test]
    fn peer_ids_keep_their_case_and_names_fold() {
        let peer = weirdnet_net::Keypair::generate_ed25519().public().to_peer_id().to_string();
        assert_eq!(locator(&peer), peer);
        assert_eq!(locator(" Home.Example.COM "), "home.example.com");
        assert_eq!(locator("ABCDEF0123456789ABCDEF0123456789"), "abcdef0123456789abcdef0123456789");
    }

    fn wait(mut f: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// An in-memory relay with a names server, an app behind it, and a
    /// caller that reaches the app only through the relay.
    #[test]
    fn calls_go_through_the_relay_to_the_app_and_errors_say_why() {
        let relay_cfg = NodeConfig {
            listen: vec!["/memory/0".parse().unwrap()],
            memory: true,
            relay_server: true,
            relay_limits: RelayLimits::unlimited(),
            names_server: Some(Names::memory()),
            ..Default::default()
        };
        let relay = Node::<(), ()>::start(&WEIRDNET, weirdnet_net::Keypair::generate_ed25519(), relay_cfg, Arc::new(NoRequests)).unwrap();
        let at: Multiaddr = format!("{}/p2p/{}", relay.status().listening[0], relay.peer_id()).parse().unwrap();
        let dead: Multiaddr = format!("/memory/{}/p2p/{}", 9_100_000_000u64 + u64::from(std::process::id()), relay.peer_id()).parse().unwrap();
        let relays = vec![dead, at];
        let server_cfg = NodeConfig { memory: true, relays: relays.clone(), ..Default::default() };
        let server = Node::<(), ()>::start(&WEIRDNET, weirdnet_net::Keypair::generate_ed25519(), server_cfg, Arc::new(NoRequests)).unwrap();
        server
            .serve_app(
                "morph-test",
                Arc::new(|_peer: PeerId, req: weirdnet_net::AppRequest| async move {
                    Ok(match req.path_with_query.as_str() {
                        "/hello" => weirdnet_net::AppResponse::new(200)
                            .header("content-type", "text/html")
                            .body("<html><head><title>Hello app</title></head><body>hi</body></html>"),
                        "/data.json" => weirdnet_net::AppResponse::new(200).header("content-type", "application/json").body(r#"{"ok":true}"#),
                        "/big" => weirdnet_net::AppResponse::new(200)
                            .header("content-type", "text/plain")
                            .body(vec![b'a'; app_call::BODY_MAX as usize + 10]),
                        "/refuse" => return Err(AppError::Refused("members only".into())),
                        _ => weirdnet_net::AppResponse::new(404),
                    })
                }),
            )
            .unwrap();
        assert!(wait(|| !server.status().reservations.is_empty()), "the app never reserved a slot on the relay");
        let caller_relays = relays.clone();
        let caller = NetAppCaller::with_config(move || NodeConfig {
            memory: true,
            names_servers: caller_relays.clone(),
            relays: caller_relays.clone(),
            request_timeout: Duration::from_secs(5),
            ..Default::default()
        });
        let peer = server.peer_id().to_string();
        let stop = AtomicBool::new(false);
        let open = |path: &str| app_call::fetch(&format!("weirdnet-morph-test://{peer}{path}"), &caller, &stop, &mut |_| {});

        let page = open("/hello").unwrap();
        assert_eq!((page.title.as_str(), page.kind, page.status), ("Hello app", "html", 200));
        assert!(app_call::page_token(&page.data_url).is_some());
        assert_eq!(open("/data.json").unwrap().kind, "json");
        assert_eq!(open("/missing"), Err(AppCallError::Status(404)));
        assert_eq!(open("/refuse"), Err(AppCallError::Refused("members only".into())));
        assert_eq!(open("/big"), Err(AppCallError::TooLarge));

        let other = app_call::fetch(&format!("weirdnet-other-app://{peer}/"), &caller, &stop, &mut |_| {});
        assert_eq!(other, Err(AppCallError::NoHandler("weirdnet-other-app".into())));
        let nobody = "0123456789abcdef0123456789abcdef";
        let missing = app_call::fetch(&format!("weirdnet-morph-test://{nobody}/"), &caller, &stop, &mut |_| {});
        assert_eq!(missing, Err(AppCallError::NotFound(nobody.into())));
    }
}
