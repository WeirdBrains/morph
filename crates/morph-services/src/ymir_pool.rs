//! "Run on: Ymir pool": companions' model turns on the user's [Ymir](https://github.com/WeirdBrains/ymir)
//! model cloud, through `weirdnet-agent`'s [`PoolModel`] and [`PoolWatch`].
//!
//! Ymir's inference nodes are linked devices in the same roster as Morph's, so they are reached
//! the same way as any linked device: through the panel's [`NodeRouter`] ([`RouterDialer`]).
//! Which linked devices are inference nodes is learned by asking them for the pool catalog
//! (catalog/1): Morph devices don't serve it, so turns only ever go to the nodes that answered.
//!
//! One [`YmirPool`] per profile ([`for_profile`]), shared by the Companion panel (turns) and
//! Settings > Agents (the model picker). It follows the catalog on a small runtime of its own
//! once something asks for it, so the picker's state stays live between turns.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use morph_ai::NodeRouter;
use weirdnet_agent::remote::pool::{NodeSource, Pool, PoolOptions};
use weirdnet_agent::remote::pool_model::{CatalogNodes, PoolModel, PoolStatus, PoolWatch};
use weirdnet_agent::remote::{Channel, Dialer, RemoteError};

use crate::node_router::protocols::RouterDialer;
use crate::routed_models::RouterSlot;

/// How long a turn waits for the first catalog before it counts the pool as unreachable.
pub const FIRST_CATALOG: Duration = Duration::from_secs(5);

/// The routers of the profile's open panels; the first that has one is used.
#[derive(Clone, Default)]
struct Routers(Arc<Mutex<Vec<RouterSlot>>>);

impl Routers {
    fn get(&self) -> Option<Arc<dyn NodeRouter>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).iter().find_map(RouterSlot::router)
    }
}

/// Linked devices that may be inference nodes: the other online ones that don't serve Morph's
/// own models (those are Morph desktops).
struct Candidates(Routers);

#[async_trait]
impl NodeSource for Candidates {
    async fn nodes(&self) -> Vec<String> {
        let Some(router) = self.0.get() else { return Vec::new() };
        router.nodes().into_iter().filter(|n| !n.this_device && n.online && !n.serves_models).map(|n| n.peer_id).collect()
    }
}

/// Dials through whichever panel router is up.
struct Dial(Routers);

#[async_trait]
impl Dialer for Dial {
    async fn open(&self, node: &str, protocol: &str) -> Result<Channel, RemoteError> {
        match self.0.get() {
            Some(router) => RouterDialer(router).open(node, protocol).await,
            None => Err(RemoteError::Unreachable { message: "this profile's sync node is not running".into() }),
        }
    }
}

struct Running {
    watch: PoolWatch,
    /// Keeps the runtime's thread going; dropped with the pool.
    _stop: tokio::sync::oneshot::Sender<()>,
}

struct Shared {
    routers: Routers,
    running: OnceLock<Option<Running>>,
}

/// The profile's Ymir pool. Cheap to clone.
#[derive(Clone)]
pub struct YmirPool(Arc<Shared>);

impl Default for YmirPool {
    fn default() -> Self {
        Self(Arc::new(Shared { routers: Routers::default(), running: OnceLock::new() }))
    }
}

impl YmirPool {
    /// Reach the pool through this panel's router too.
    pub fn attach(&self, slot: RouterSlot) {
        self.0.routers.0.lock().unwrap_or_else(|p| p.into_inner()).push(slot);
    }

    fn options() -> PoolOptions {
        // Ymir's inference nodes speak infer/2; infer/1 would only reach Morph devices.
        PoolOptions::default().with_infer_v1_fallback(false)
    }

    /// Starts following the catalog (once). `None` if the runtime could not start.
    fn running(&self) -> Option<&Running> {
        self.0
            .running
            .get_or_init(|| {
                let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().ok()?;
                let probe = Pool::new(Arc::new(Dial(self.0.routers.clone())), Candidates(self.0.routers.clone())).with_options(Self::options());
                let watch = {
                    let _in = rt.enter();
                    PoolWatch::spawn(probe)
                };
                let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
                std::thread::Builder::new()
                    .name("morph-ymir-pool".into())
                    .spawn(move || {
                        let _ = rt.block_on(stopped);
                    })
                    .ok()?;
                Some(Running { watch, _stop: stop })
            })
            .as_ref()
    }

    /// The pool as the picker shows it now (starts following it on first use).
    pub fn status(&self) -> PoolStatus {
        self.running().map(|r| r.watch.status()).unwrap_or_else(|| PoolStatus::Searching { last: None })
    }

    /// The nodes turns go to: those that answered the catalog.
    fn nodes(&self) -> CatalogNodes {
        self.running().map(|r| r.watch.nodes()).unwrap_or_default()
    }

    /// Waits up to `wait` for an answer from the pool before a turn: the first catalog after
    /// start, or, when the pool was unreachable, one more round asked now (not after the backoff),
    /// so a turn right after the inference node came back goes there instead of falling back.
    pub async fn settled(&self, wait: Duration) {
        let Some(running) = self.running() else { return };
        let mut rx = running.watch.receiver();
        let unreachable = matches!(&*rx.borrow_and_update(), PoolStatus::Unreachable { .. });
        if unreachable {
            running.watch.retry();
            let _ = tokio::time::timeout(wait, rx.changed()).await;
            return;
        }
        let _ = tokio::time::timeout(wait, rx.wait_for(|s| !matches!(s, PoolStatus::Searching { last: None }))).await;
    }

    /// A model for one turn on the pool: `model` (a catalog id) or the pool's choice.
    pub fn model(&self, model: Option<&str>) -> PoolModel {
        let pool = Pool::new(Arc::new(Dial(self.0.routers.clone())), self.nodes()).with_options(Self::options());
        let m = PoolModel::new(pool);
        match model {
            Some(id) => m.with_model(id),
            None => m,
        }
    }

    /// The label of a linked device (a pool node's peer id), for notices.
    pub fn label(&self, peer: &str) -> String {
        self.0.routers.get().and_then(|r| r.nodes().into_iter().find(|n| n.peer_id == peer).map(|n| n.label)).unwrap_or_else(|| "another node".into())
    }
}

static POOLS: Mutex<Vec<(PathBuf, String, YmirPool)>> = Mutex::new(Vec::new());

/// The Ymir pool of profile `profile` under `home` (one per process).
pub fn for_profile(home: &Path, profile: &str) -> YmirPool {
    let mut pools = POOLS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, _, p)) = pools.iter().find(|(h, id, _)| h == home && id == profile) {
        return p.clone();
    }
    let pool = YmirPool::default();
    pools.push((home.to_path_buf(), profile.to_string(), pool.clone()));
    pool
}
