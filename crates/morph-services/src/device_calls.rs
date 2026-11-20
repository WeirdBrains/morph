//! Calls between the user's own linked devices, on this device's identity.
//!
//! [`crate::app_calls::NetAppCaller`] calls public Weirdnet apps from a throwaway key, so the
//! far end learns nothing about who calls. Calls to the user's *own* devices (Node companions,
//! inference offload, anything a device serves only to its siblings) go through a
//! [`DeviceCaller`] instead: it uses the profile's running sync node, so the caller is the
//! device's node key (its peer id is what Noise authenticates) and every call carries the
//! device's cert from the linked-devices roster ([`CERT_HEADER`]). The serving side
//! ([`DeviceCaller::serve`], [`DeviceCaller::accept_streams`]) runs
//! [`weirdnet_sync::Roster::check_caller`] against its own live roster before anything
//! reaches the handler: an unknown, revoked, foreign or not-yet-admitted device is refused
//! with the reason. The caller checks the far end the same way: only an active device of the
//! same identity is dialled.
//!
//! See `docs/net-app-caller-identity.md`.

use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use weirdnet_net::profile::WEIRDNET;
use weirdnet_net::stream::{Incoming, Limits, Stream};
use weirdnet_net::user::NodeCert;
use weirdnet_net::{AppError, AppHandler, AppRequest, AppResponse, Multiaddr, Node, PeerId};
pub use weirdnet_sync::CallerCheck;
use weirdnet_sync::SyncState;

/// The header a device call carries its cert in (JSON of [`NodeCert`]).
pub const CERT_HEADER: &str = "weirdnet-device-cert";
/// How long a device call may take to connect and get its status and headers.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeviceCallError {
    /// The profile is locked (its password has not been entered): no device calls either way.
    #[error("profile locked")]
    Locked,
    /// No sync node runs for this profile (locked, or it does not sync yet).
    #[error("this profile is not linked to other devices yet")]
    NotSyncing,
    /// The far end is not an active linked device of this identity.
    #[error("{peer} is not one of your linked devices ({reason})")]
    NotLinked { peer: String, reason: &'static str },
    /// The far end refused (for a device call, usually our device being revoked there).
    #[error("refused: {0}")]
    Refused(String),
    #[error("{0} is not reachable")]
    Unreachable(String),
    /// The far end is a linked device but does not accept the stream's protocol (a build from
    /// before the protocol was added): the caller may ask an older one instead.
    #[error("{0} is not reachable")]
    ProtocolNotSupported(String),
    #[error("{0}")]
    Failed(String),
}

impl From<AppError> for DeviceCallError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::Refused(r) => DeviceCallError::Refused(r),
            AppError::Unreachable(r) => DeviceCallError::Unreachable(r),
            AppError::TimedOut => DeviceCallError::Unreachable("timed out".into()),
            other => DeviceCallError::Failed(other.to_string()),
        }
    }
}

/// One of the user's linked, unrevoked devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedDevice {
    pub peer_id: String,
    pub label: String,
    pub this_device: bool,
}

type SyncNode = Node<Vec<u8>, Vec<u8>>;
type AddrsFor = dyn Fn(&NodeCert) -> Vec<Multiaddr> + Send + Sync;

/// The profile lock as read once: see [`DeviceCaller::lock_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Unlocked,
    Locked,
    /// The status could not be read (an I/O error).
    Unknown,
}

/// Calls to and from the user's linked devices on this device's identity (module docs).
#[derive(Clone)]
pub struct DeviceCaller {
    node: Arc<SyncNode>,
    state: Arc<Mutex<SyncState>>,
    addrs: Arc<AddrsFor>,
    /// The profile (home, id) it acts for: every call, stream and served request is refused
    /// while it is locked, even by a caller made before the lock.
    profile: Option<Arc<(std::path::PathBuf, String)>>,
}

impl DeviceCaller {
    /// The caller of a profile whose sync node runs (after unlock), reaching devices the way
    /// sync does (added peers, then relay circuits).
    pub fn for_profile(home: &Path, profile_id: &str) -> Result<Self, DeviceCallError> {
        if crate::profile_lock::is_locked(home, profile_id) {
            return Err(DeviceCallError::Locked);
        }
        let (node, state) = crate::sync::live(home, profile_id).ok_or(DeviceCallError::NotSyncing)?;
        let net = morph_core::network::load(home).unwrap_or_default();
        let addrs = move |cert: &NodeCert| crate::sync::addrs_for(cert, &net, &crate::sync::configured_relays());
        Ok(Self::without_profile(node, state, addrs).for_profile_at(home, profile_id))
    }

    /// A caller over a running sync node and its live state, acting for a fresh unlocked
    /// temp profile (tests; production uses [`DeviceCaller::for_profile`]). `addrs` gives a
    /// device's addresses (an in-memory listener).
    #[cfg(test)]
    pub(crate) fn new(
        node: Arc<SyncNode>,
        state: Arc<Mutex<SyncState>>,
        addrs: impl Fn(&NodeCert) -> Vec<Multiaddr> + Send + Sync + 'static,
    ) -> Self {
        let home = tempfile::tempdir().unwrap().keep();
        Self::without_profile(node, state, addrs).for_profile_at(&home, "default")
    }

    /// A caller with no profile: it fails closed, refusing everything as locked until
    /// [`DeviceCaller::for_profile_at`] gives it one.
    fn without_profile(
        node: Arc<SyncNode>,
        state: Arc<Mutex<SyncState>>,
        addrs: impl Fn(&NodeCert) -> Vec<Multiaddr> + Send + Sync + 'static,
    ) -> Self {
        Self { node, state, addrs: Arc::new(addrs), profile: None }
    }

    /// This caller, acting for the profile `(home, id)`: refused while it is locked (tests;
    /// [`DeviceCaller::for_profile`] sets it).
    fn for_profile_at(mut self, home: &Path, profile_id: &str) -> Self {
        self.profile = Some(Arc::new((home.to_path_buf(), profile_id.to_string())));
        self
    }

    #[cfg(test)]
    pub(crate) fn for_locked_profile(self, home: &Path, profile_id: &str) -> Self {
        self.for_profile_at(home, profile_id)
    }

    /// Whether the profile this caller acts for is unlocked: the lock check served requests
    /// make, so a host takes it from the caller rather than being handed a profile.
    pub fn check_unlocked(&self) -> Result<(), DeviceCallError> {
        self.unlocked()
    }

    /// What the lock is right now, telling a real lock from a failed read of it
    /// ([`DeviceCaller::check_unlocked`] fails closed on both).
    pub fn lock_state(&self) -> LockState {
        match &self.profile {
            // No profile refuses as locked, as `check_unlocked` does.
            None => LockState::Locked,
            Some(p) => match crate::profile_lock::status(&p.0, &p.1) {
                Ok(s) if s.unlocked => LockState::Unlocked,
                Ok(_) => LockState::Locked,
                Err(_) => LockState::Unknown,
            },
        }
    }

    /// Refuse while the profile is locked.
    fn unlocked(&self) -> Result<(), DeviceCallError> {
        match &self.profile {
            Some(p) if !crate::profile_lock::is_locked(&p.0, &p.1) => Ok(()),
            // Locked, or no profile to check: fail closed.
            _ => Err(DeviceCallError::Locked),
        }
    }

    fn with_state<T>(&self, f: impl FnOnce(&SyncState) -> T) -> T {
        f(&self.state.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// This device's peer id (its node key).
    pub fn peer_id(&self) -> String {
        self.node.peer_id().to_string()
    }

    /// This device's cert from the roster.
    pub fn cert(&self) -> NodeCert {
        self.with_state(|st| st.roster.get(&st.our_cert.peer_id).cloned().unwrap_or_else(|| st.our_cert.clone()))
    }

    /// Linked, unrevoked devices, this one included and marked.
    pub fn linked_devices(&self) -> Vec<LinkedDevice> {
        self.with_state(|st| {
            st.roster
                .active()
                .map(|c| LinkedDevice { peer_id: c.peer_id.clone(), label: c.label.clone(), this_device: c.peer_id == st.our_cert.peer_id })
                .collect()
        })
    }

    /// Check `peer` against the live roster (the same rule both ends use).
    pub fn check(&self, peer: &str, presented: Option<&NodeCert>) -> CallerCheck {
        self.with_state(|st| st.roster.check_caller(&WEIRDNET, &st.user_key, peer, presented))
    }

    /// The far end, if it is another linked device: its peer id and addresses.
    fn target(&self, peer: &str) -> Result<(PeerId, Vec<Multiaddr>), DeviceCallError> {
        let check = self.check(peer, None);
        if let Some(reason) = check.reason() {
            return Err(DeviceCallError::NotLinked { peer: peer.to_string(), reason });
        }
        if peer == self.peer_id() {
            return Err(DeviceCallError::NotLinked { peer: peer.to_string(), reason: "this-device" });
        }
        let cert =
            self.with_state(|st| st.roster.get(peer).cloned()).ok_or(DeviceCallError::NotLinked { peer: peer.into(), reason: "unknown-device" })?;
        let id = PeerId::from_str(peer).map_err(|e| DeviceCallError::Failed(e.to_string()))?;
        Ok((id, (self.addrs)(&cert)))
    }

    /// An app call to a linked device, carrying this device's cert.
    pub async fn call(&self, peer: &str, req: AppRequest, timeout: Duration) -> Result<AppResponse, DeviceCallError> {
        self.unlocked()?;
        let (id, addrs) = self.target(peer)?;
        let cert = serde_json::to_string(&self.cert()).map_err(|e| DeviceCallError::Failed(e.to_string()))?;
        let mut req = req;
        req.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(CERT_HEADER));
        req.headers.push((CERT_HEADER.into(), cert));
        Ok(self.node.app_call_via(id, &addrs, req, timeout).await?)
    }

    /// A raw stream to a linked device on `protocol` (for message channels such as
    /// `morph-ai` `NodeRouter::open`). The far end checks us by peer id when it accepts.
    pub async fn open_stream(&self, peer: &str, protocol: &str, limits: Limits) -> Result<Stream, DeviceCallError> {
        self.unlocked()?;
        let (id, addrs) = self.target(peer)?;
        self.node.open_stream_async(id, &addrs, protocol, limits).await.map_err(|e| open_error(e.to_string()))
    }

    /// Serve `app` to linked devices only: every call is checked against the live roster
    /// (and the cert it carries) before `handler` sees it; others get `Refused(reason)`.
    pub fn serve(&self, app: &str, handler: Arc<dyn AppHandler>) -> Result<(), DeviceCallError> {
        self.unlocked()?;
        let gate = self.clone();
        let gated = move |peer: PeerId, req: AppRequest| {
            let presented = req.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(CERT_HEADER)).map(|(_, v)| serde_json::from_str::<NodeCert>(v));
            let check = match presented {
                Some(Err(_)) => CallerCheck::WrongUser,
                Some(Ok(c)) => gate.check(&peer.to_string(), Some(&c)),
                None => gate.check(&peer.to_string(), None),
            };
            let handler = handler.clone();
            let locked = gate.unlocked().is_err();
            async move {
                if locked {
                    return Err(AppError::Refused("profile locked".into()));
                }
                match check.reason() {
                    Some(reason) => Err(AppError::Refused(reason.into())),
                    None => handler.handle(peer, req).await,
                }
            }
        };
        self.node.serve_app(app, Arc::new(gated)).map_err(DeviceCallError::from)
    }

    /// Accept streams on `protocol` from linked devices only (others are reset unread).
    pub fn accept_streams(&self, protocol: &str, limits: Limits) -> Result<LinkedStreams, DeviceCallError> {
        self.unlocked()?;
        let incoming = self.node.accept_streams(protocol, limits).map_err(|e| DeviceCallError::Failed(e.to_string()))?;
        Ok(LinkedStreams { incoming, gate: self.clone() })
    }
}

/// A failed [`DeviceCaller::open_stream`] as a [`DeviceCallError`]. weirdnet-net 0.8 reports a
/// protocol the far end does not accept only in its message ("the peer does not accept
/// {protocol}"), so this is the one place that reads it; callers match the variant.
fn open_error(message: String) -> DeviceCallError {
    if message.contains("does not accept") { DeviceCallError::ProtocolNotSupported(message) } else { DeviceCallError::Unreachable(message) }
}

/// Incoming streams, filtered to the user's linked devices.
pub struct LinkedStreams {
    incoming: Incoming,
    gate: DeviceCaller,
}

impl LinkedStreams {
    /// The next stream from a linked device, with the check it passed.
    pub async fn next(&mut self) -> Option<Stream> {
        loop {
            let s = self.incoming.next().await?;
            if self.gate.unlocked().is_ok() && self.gate.check(&s.peer().to_string(), None) == CallerCheck::Linked {
                return Some(s);
            }
            s.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weirdnet_net::Keypair;
    use weirdnet_sync::{Roster, node_config, start_node};

    struct Device {
        key: Keypair,
        cert: NodeCert,
    }

    fn device(user: &Keypair, label: &str) -> Device {
        let key = Keypair::generate_ed25519();
        let cert = Roster::self_certify(&WEIRDNET, user, &key.public().to_peer_id().to_string(), label, 1).unwrap();
        Device { key, cert }
    }

    fn state(user: &Keypair, me: &Device, others: &[&NodeCert]) -> Arc<Mutex<SyncState>> {
        let mut st = SyncState::new(vec![9; 32], user.public(), me.cert.clone(), Some(&me.key), vec!["bookmarks".into()]).unwrap();
        st.auto_first_rotation = false;
        for c in others {
            st.roster.upsert((*c).clone());
        }
        Arc::new(Mutex::new(st))
    }

    fn up(user: &Keypair, me: &Device, others: &[&NodeCert], server: Option<Multiaddr>) -> DeviceCaller {
        let st = state(user, me, others);
        let node = Arc::new(start_node(me.key.clone(), node_config(true), st.clone()).unwrap());
        DeviceCaller::new(node, st, move |_| server.clone().into_iter().collect())
    }

    /// A caller with no profile is refused everything, as a locked one is.
    #[test]
    fn a_caller_with_no_profile_is_refused() {
        let user = Keypair::generate_ed25519();
        let (a, b) = (device(&user, "a"), device(&user, "b"));
        let st = state(&user, &a, &[&b.cert]);
        let node = Arc::new(start_node(a.key.clone(), node_config(true), st.clone()).unwrap());
        let caller = DeviceCaller::without_profile(node, st, |_| Vec::new());
        assert!(matches!(caller.check_unlocked(), Err(DeviceCallError::Locked)));
        let r = rt(caller.call(&b.cert.peer_id, AppRequest::new("morph-device-test", "GET", "/"), CALL_TIMEOUT));
        assert!(matches!(r, Err(DeviceCallError::Locked)), "{r:?}");
    }

    fn rt<F: std::future::Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }

    #[test]
    fn the_caller_check_follows_the_roster() {
        let user = Keypair::generate_ed25519();
        let (a, b, c) = (device(&user, "a"), device(&user, "b"), device(&user, "c"));
        let mut r = Roster::new();
        r.upsert(a.cert.clone());
        r.upsert(b.cert.clone());
        let k = user.public();
        assert_eq!(r.check_caller(&WEIRDNET, &k, &b.cert.peer_id, None), CallerCheck::Linked);
        assert_eq!(r.check_caller(&WEIRDNET, &k, &b.cert.peer_id, Some(&b.cert)), CallerCheck::Linked);
        assert_eq!(r.check_caller(&WEIRDNET, &k, &b.cert.peer_id, Some(&a.cert)), CallerCheck::WrongUser, "another device's cert");
        assert_eq!(r.check_caller(&WEIRDNET, &k, &c.cert.peer_id, Some(&c.cert)), CallerCheck::Unknown, "a minted cert alone");
        let stranger = device(&Keypair::generate_ed25519(), "x");
        assert_eq!(r.check_caller(&WEIRDNET, &k, &stranger.cert.peer_id, Some(&stranger.cert)), CallerCheck::WrongUser);
        let stale = r.clone();
        let revoked = r.revoke(&WEIRDNET, &user, &b.cert.peer_id, 5).unwrap();
        assert_eq!(r.check_caller(&WEIRDNET, &k, &b.cert.peer_id, None), CallerCheck::Revoked);
        assert_eq!(stale.check_caller(&WEIRDNET, &k, &b.cert.peer_id, Some(&revoked)), CallerCheck::Revoked);
    }

    /// A serves an app to its linked devices: B (linked) gets through, D (revoked on A) and
    /// C (same user, unknown to A) are refused, and nothing reaches the handler for them.
    #[test]
    fn a_linked_device_is_served_and_revoked_or_unknown_ones_are_refused() {
        let user = Keypair::generate_ed25519();
        let (a, b, c, d) = (device(&user, "a"), device(&user, "b"), device(&user, "c"), device(&user, "d"));
        let server = up(&user, &a, &[&b.cert, &d.cert], None);
        {
            let mut st = server.state.lock().unwrap();
            st.roster.revoke(&WEIRDNET, &user, &d.cert.peer_id, 5).unwrap();
        }
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        server
            .serve(
                "morph-device-test",
                Arc::new(move |peer: PeerId, _req: AppRequest| {
                    log.lock().unwrap().push(peer.to_string());
                    async move { Ok(AppResponse::new(200).body("hi")) }
                }),
            )
            .unwrap();
        let at: Multiaddr = format!("{}/p2p/{}", server.node.status().listening[0], server.peer_id()).parse().unwrap();
        let a_cert = a.cert.clone();
        let call = |caller: &DeviceCaller| rt(caller.call(&a_cert.peer_id, AppRequest::new("morph-device-test", "GET", "/"), CALL_TIMEOUT));

        let from_b = up(&user, &b, &[&a.cert], Some(at.clone()));
        assert_eq!(call(&from_b).unwrap().status, 200, "a linked device is served");

        // D still lists A, so it dials; A refuses it.
        let from_d = up(&user, &d, &[&a.cert], Some(at.clone()));
        assert_eq!(call(&from_d).unwrap_err(), DeviceCallError::Refused("revoked".into()));

        // C is the same user's device, but A never linked it.
        let from_c = up(&user, &c, &[&a.cert], Some(at.clone()));
        assert_eq!(call(&from_c).unwrap_err(), DeviceCallError::Refused("unknown-device".into()));

        assert_eq!(*seen.lock().unwrap(), std::slice::from_ref(&b.cert.peer_id), "only the linked call reached the handler");

        // The caller side refuses to dial anything but a linked device.
        let err = rt(from_b.call(&c.cert.peer_id, AppRequest::new("morph-device-test", "GET", "/"), CALL_TIMEOUT)).unwrap_err();
        assert_eq!(err, DeviceCallError::NotLinked { peer: c.cert.peer_id.clone(), reason: "unknown-device" });
        assert_eq!(from_b.linked_devices().len(), 2);
        assert!(from_b.linked_devices().iter().any(|l| l.this_device && l.peer_id == b.cert.peer_id));
    }

    #[test]
    fn streams_are_accepted_from_linked_devices_only() {
        let user = Keypair::generate_ed25519();
        let (a, b, c) = (device(&user, "a"), device(&user, "b"), device(&user, "c"));
        let server = up(&user, &a, &[&b.cert], None);
        let mut incoming = server.accept_streams("/morph/device-test/1", Limits::default()).unwrap();
        let at: Multiaddr = format!("{}/p2p/{}", server.node.status().listening[0], server.peer_id()).parse().unwrap();
        let from_c = up(&user, &c, &[&a.cert], Some(at.clone()));
        let from_b = up(&user, &b, &[&a.cert], Some(at));
        // C's stream opens at the transport but is reset by the gate; B's comes through.
        let _c = rt(from_c.open_stream(&a.cert.peer_id, "/morph/device-test/1", Limits::default()));
        let _b = rt(from_b.open_stream(&a.cert.peer_id, "/morph/device-test/1", Limits::default())).unwrap();
        let got = rt(incoming.next()).unwrap();
        assert_eq!(got.peer().to_string(), b.cert.peer_id);
    }

    #[test]
    fn a_protocol_the_far_end_does_not_accept_is_its_own_error() {
        let user = Keypair::generate_ed25519();
        let (a, b) = (device(&user, "a"), device(&user, "b"));
        let server = up(&user, &a, &[&b.cert], None);
        let _incoming = server.accept_streams("/morph/device-test/1", Limits::default()).unwrap();
        let at: Multiaddr = format!("{}/p2p/{}", server.node.status().listening[0], server.peer_id()).parse().unwrap();
        let from_b = up(&user, &b, &[&a.cert], Some(at));
        let err = rt(from_b.open_stream(&a.cert.peer_id, "/morph/device-test/2", Limits::default())).unwrap_err();
        let DeviceCallError::ProtocolNotSupported(m) = &err else { panic!("{err:?}") };
        // The text is what it was when this was an `Unreachable`.
        assert_eq!(err.to_string(), DeviceCallError::Unreachable(m.clone()).to_string());
        assert!(m.contains("/morph/device-test/2"), "{m}");
        // Anything else that stops a stream opening is still unreachable.
        assert!(matches!(open_error("network: cannot reach it in time".into()), DeviceCallError::Unreachable(_)));
    }
}
