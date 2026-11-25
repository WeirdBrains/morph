//! The asking side of remote inference: a [`ModelProvider`] that runs a companion's model turns
//! where the "Run on" setting says, and falls back as the design orders.
//!
//! - [`morph_ai::offload_plan`] decides, from the setting, the linked devices that report
//!   `serves_models` and whether the local model can run: This device runs here; Automatic runs
//!   here while the local model can, else on a serving device; a chosen device is asked first
//!   and alone.
//! - Turns that go out use [`protocols::remote_model`]: the plan's devices in order, then the
//!   local model if it is available (and the setting allows the fallback). A device that
//!   refuses, is busy, is offline or has no model is skipped.
//! - Ymir pool ([`crate::ymir_pool`]): turns go to the user's Ymir model cloud. When it can't be
//!   reached or fails before any text, the local model runs the turn (when it can) and the panel
//!   shows a note saying so; a node that dies mid-answer is resumed on another, also noted.
//! - The setting is read at every turn: a session's model follows a change at its next turn.
//! - [`RunsOn`] says which device a turn is on, for the working line ("Thinking on desktop…").

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use morph_ai::{LinkedNode, ModelChoice, ModelProvider, NodeRouter, OffloadPlan, OffloadSettings, Platform, offload_plan};
use weirdnet_agent::remote::infer::Route;
use weirdnet_agent::remote::pool_model::PoolModel;
use weirdnet_agent::session::AgentEvent;
use weirdnet_agent::tool::ToolSpec;
use weirdnet_agent::{CancelToken, DeltaSink, Message, ModelBackend, ModelError, ModelNotice, ModelTurn};

use crate::node_router::protocols;
use crate::ymir_pool::{FIRST_CATALOG, YmirPool};

/// The router of the profile's sync node while it runs, shared with the provider (the panel's
/// device tracking sets it; no router means no linked devices).
#[derive(Clone, Default)]
pub struct RouterSlot(Arc<Mutex<Option<Arc<dyn NodeRouter>>>>);

impl RouterSlot {
    pub fn set(&self, router: Option<Arc<dyn NodeRouter>>) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = router;
    }

    pub(crate) fn router(&self) -> Option<Arc<dyn NodeRouter>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn get(&self) -> Option<Arc<dyn NodeRouter>> {
        self.router()
    }
}

/// The name of the device a model turn is running on right now, if it is not this one.
#[derive(Clone, Default)]
pub struct RunsOn(Arc<Mutex<Option<String>>>);

impl RunsOn {
    pub fn get(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn set(&self, device: Option<String>) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = device;
    }
}

/// What the working line says while a turn runs on the pool.
pub const ON_THE_POOL: &str = "your Ymir pool";

type Settings = Arc<dyn Fn() -> OffloadSettings + Send + Sync>;
type Ready = Arc<dyn Fn(&ModelChoice) -> bool + Send + Sync>;

/// Opens models here, on linked devices or on the Ymir pool, per the "Run on" setting.
pub struct RoutedModels {
    inner: Arc<Inner>,
}

struct Inner {
    local: Arc<dyn ModelProvider>,
    local_ready: Ready,
    settings: Settings,
    router: RouterSlot,
    platform: Platform,
    runs_on: RunsOn,
    pool: Option<YmirPool>,
}

impl RoutedModels {
    /// `local_ready` says whether a model can run on this device now (downloaded, CPU supported);
    /// `settings` is read at every turn.
    pub fn new(
        local: Arc<dyn ModelProvider>,
        local_ready: impl Fn(&ModelChoice) -> bool + Send + Sync + 'static,
        settings: impl Fn() -> OffloadSettings + Send + Sync + 'static,
        router: RouterSlot,
        platform: Platform,
    ) -> Self {
        let inner = Inner {
            local,
            local_ready: Arc::new(local_ready),
            settings: Arc::new(settings),
            router,
            platform,
            runs_on: RunsOn::default(),
            pool: None,
        };
        Self { inner: Arc::new(inner) }
    }

    /// "Run on: Ymir pool" goes to `pool`, reached through this provider's router.
    pub fn with_pool(self, pool: YmirPool) -> Self {
        pool.attach(self.inner.router.clone());
        let Inner { local, local_ready, settings, router, platform, runs_on, .. } = match Arc::try_unwrap(self.inner) {
            Ok(inner) => inner,
            Err(_) => unreachable!("with_pool is called on a new provider"),
        };
        Self { inner: Arc::new(Inner { local, local_ready, settings, router, platform, runs_on, pool: Some(pool) }) }
    }

    pub fn runs_on(&self) -> RunsOn {
        self.inner.runs_on.clone()
    }

    /// Where a turn with `choice` would run now (linked devices; the pool is not part of it).
    pub fn plan(&self, choice: &ModelChoice) -> (OffloadPlan, Vec<LinkedNode>) {
        self.inner.plan(&(self.inner.settings)(), choice)
    }
}

impl Inner {
    fn plan(&self, settings: &OffloadSettings, choice: &ModelChoice) -> (OffloadPlan, Vec<LinkedNode>) {
        let nodes = self.router.get().map(|r| r.nodes()).unwrap_or_default();
        (offload_plan(settings, self.platform, &nodes, (self.local_ready)(choice)), nodes)
    }

    /// The local model wrapped so the working line says the turn is here, when it can run.
    fn local_fallback(&self, choice: &ModelChoice) -> Option<Arc<dyn ModelBackend>> {
        let inner = (self.local_ready)(choice).then(|| self.local.open(choice).ok()).flatten()?;
        Some(Arc::new(OnThisDevice { inner, runs_on: self.runs_on.clone() }))
    }

    /// A model for the turns of `settings`.
    fn open_for(&self, settings: &OffloadSettings, choice: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError> {
        if settings.pool
            && let Some(pool) = &self.pool
        {
            // m3talsmith's call (2026-11-25): when the pool can't take a turn, this device's model
            // runs it automatically, with a note in the panel.
            let local = self.local_fallback(choice);
            let model = pool.model(settings.pool_model.as_deref());
            let model = match &local {
                Some(l) => model.with_fallback(l.clone()),
                None => model,
            };
            return Ok(Arc::new(OnThePool { model, pool: pool.clone(), local, runs_on: self.runs_on.clone(), name: Mutex::default() }));
        }
        let (plan, nodes) = self.plan(settings, choice);
        let Some(router) = self.router.get().filter(|_| !plan.runs_here()) else {
            return self.local.open(choice);
        };
        // The local model backs the turn up only when it can run here at all.
        let local = self.local_fallback(choice);
        let label = |peer: &str| nodes.iter().find(|n| n.peer_id == peer).map(|n| n.label.clone()).unwrap_or_else(|| peer.to_string());
        let first = plan.nodes.first().map(|p| label(p)).unwrap_or_default();
        let remote = protocols::remote_model(router, &plan, local);
        let labels = nodes.iter().map(|n| (n.peer_id.clone(), n.label.clone())).collect();
        Ok(Arc::new(Remote { inner: remote, first, labels, runs_on: self.runs_on.clone() }))
    }
}

impl ModelProvider for RoutedModels {
    fn open(&self, choice: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError> {
        let settings = (self.inner.settings)();
        let first = self.inner.open_for(&settings, choice)?;
        Ok(Arc::new(PerTurn { inner: self.inner.clone(), choice: choice.clone(), current: Mutex::new((settings, first)) }))
    }
}

/// A session's model: the one its "Run on" setting gives, opened again when the setting changes.
struct PerTurn {
    inner: Arc<Inner>,
    choice: ModelChoice,
    current: Mutex<(OffloadSettings, Arc<dyn ModelBackend>)>,
}

impl PerTurn {
    fn model(&self) -> Result<Arc<dyn ModelBackend>, ModelError> {
        let settings = (self.inner.settings)();
        let mut current = self.current.lock().unwrap_or_else(|p| p.into_inner());
        if current.0 != settings {
            *current = (settings.clone(), self.inner.open_for(&settings, &self.choice)?);
        }
        Ok(current.1.clone())
    }
}

#[async_trait]
impl ModelBackend for PerTurn {
    fn name(&self) -> String {
        self.current.lock().unwrap_or_else(|p| p.into_inner()).1.name()
    }

    async fn complete(&self, messages: &[Message], tools: &[ToolSpec], deltas: &DeltaSink, cancel: &CancelToken) -> Result<ModelTurn, ModelError> {
        self.model()?.complete(messages, tools, deltas, cancel).await
    }
}

/// A turn on the Ymir pool, with this device's model as the fallback when it can run.
struct OnThePool {
    model: PoolModel,
    pool: YmirPool,
    local: Option<Arc<dyn ModelBackend>>,
    runs_on: RunsOn,
    /// `model@node` of the last turn, with the node's device name.
    name: Mutex<Option<String>>,
}

#[async_trait]
impl ModelBackend for OnThePool {
    fn name(&self) -> String {
        self.name.lock().unwrap_or_else(|p| p.into_inner()).clone().unwrap_or_else(|| self.model.name())
    }

    async fn complete(&self, messages: &[Message], tools: &[ToolSpec], deltas: &DeltaSink, cancel: &CancelToken) -> Result<ModelTurn, ModelError> {
        self.pool.settled(FIRST_CATALOG).await;
        self.runs_on.set(Some(ON_THE_POOL.into()));
        // The pool's notices name nodes by peer id: pass them on with the device's name.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let ours = DeltaSink::new(Some(tx));
        let forward = |event: AgentEvent| match event {
            AgentEvent::TextDelta(t) => deltas.text(t),
            AgentEvent::ReasoningDelta(t) => deltas.reasoning(t),
            AgentEvent::ModelNotice(ModelNotice::Continued { node, offset }) => {
                deltas.notice(ModelNotice::Continued { node: self.pool.label(&node), offset })
            }
            AgentEvent::ModelNotice(n) => deltas.notice(n),
            _ => {}
        };
        let turn = {
            let run = self.model.complete(messages, tools, &ours, cancel);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    turn = &mut run => break turn,
                    Some(event) = rx.recv() => forward(event),
                }
            }
        };
        drop(ours);
        while let Ok(event) = rx.try_recv() {
            forward(event);
        }
        self.runs_on.set(None);
        let name = match self.model.last_route() {
            Some(Route::Node { node, model }) => Some(format!("{model}@{}", self.pool.label(&node))),
            Some(Route::Fallback) => self.local.as_ref().map(|l| l.name()),
            _ => None,
        };
        *self.name.lock().unwrap_or_else(|p| p.into_inner()) = name;
        turn.map_err(|e| match e {
            ModelError::PoolUnreachable(why) => ModelError::PoolUnreachable(format!(
                "{why}. This device has no model that can run here, so nothing answered; download one in Settings, Agents, or check that your Ymir inference node is on"
            )),
            e => e,
        })
    }
}

/// The local fallback: while it runs the turn is on this device.
struct OnThisDevice {
    inner: Arc<dyn ModelBackend>,
    runs_on: RunsOn,
}

#[async_trait]
impl ModelBackend for OnThisDevice {
    fn name(&self) -> String {
        self.inner.name()
    }

    async fn complete(&self, messages: &[Message], tools: &[ToolSpec], deltas: &DeltaSink, cancel: &CancelToken) -> Result<ModelTurn, ModelError> {
        self.runs_on.set(None);
        self.inner.complete(messages, tools, deltas, cancel).await
    }
}

/// A turn that goes to linked devices first; it marks which device it is on while it runs.
struct Remote {
    inner: weirdnet_agent::remote::infer::RemoteModel,
    first: String,
    labels: Vec<(String, String)>,
    runs_on: RunsOn,
}

#[async_trait]
impl ModelBackend for Remote {
    /// `model@device` of the last turn, as the trace records it.
    fn name(&self) -> String {
        match self.inner.last_route() {
            Some(Route::Node { node, model }) => {
                let device = self.labels.iter().find(|(p, _)| *p == node).map(|(_, l)| l.as_str()).unwrap_or(&node);
                format!("{model}@{device}")
            }
            _ => self.inner.name(),
        }
    }

    async fn complete(&self, messages: &[Message], tools: &[ToolSpec], deltas: &DeltaSink, cancel: &CancelToken) -> Result<ModelTurn, ModelError> {
        self.runs_on.set(Some(self.first.clone()));
        let turn = self.inner.complete(messages, tools, deltas, cancel).await;
        self.runs_on.set(None);
        turn.map_err(|e| explain(e, &self.labels))
    }
}

/// A linked device's refusal in words the user can act on, not "node refused: revoked".
fn explain(error: ModelError, labels: &[(String, String)]) -> ModelError {
    let (ModelError::Unavailable(text) | ModelError::Failed(text)) = &error else { return error };
    let device =
        |text: &str| labels.iter().find(|(p, _)| text.contains(p.as_str())).map(|(_, l)| l.clone()).unwrap_or_else(|| "The other device".into());
    if text.contains("refused: revoked") {
        ModelError::Unavailable(format!("{} no longer lets this device use it: this device was revoked from your devices.", device(text)))
    } else if text.contains("refused: profile-locked") {
        ModelError::Unavailable(format!("{} is locked. Unlock it to run turns there.", device(text)))
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infer_serving::tests::{Pair, pair, said};
    use morph_ai::OffloadMode;
    use std::sync::atomic::{AtomicBool, Ordering};
    use weirdnet_agent::ModelError;

    /// The phone's own model: works, or refuses like an unsupported CPU would.
    struct Own(Arc<AtomicBool>);

    impl ModelProvider for Own {
        fn open(&self, _choice: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError> {
            if self.0.load(Ordering::SeqCst) {
                Ok(said("from the phone's own model"))
            } else {
                Err(ModelError::Unavailable("This CPU lacks AVX2/FMA needed for local models".into()))
            }
        }
    }

    struct Asker {
        models: RoutedModels,
        own_ready: Arc<AtomicBool>,
        settings: Arc<Mutex<OffloadSettings>>,
    }

    fn asker(p: &Pair, local_ready: bool) -> Asker {
        let own_ready = Arc::new(AtomicBool::new(local_ready));
        let settings = Arc::new(Mutex::new(OffloadSettings::default()));
        let slot = RouterSlot::default();
        slot.set(Some(p.phone.clone()));
        let (ready, s) = (own_ready.clone(), settings.clone());
        let models = RoutedModels::new(
            Arc::new(Own(own_ready.clone())),
            move |_| ready.load(Ordering::SeqCst),
            move || s.lock().unwrap().clone(),
            slot,
            Platform::Desktop,
        );
        Asker { models, own_ready, settings }
    }

    impl Asker {
        fn set(&self, mode: OffloadMode, prefer: Option<&str>, fallback_local: bool) {
            *self.settings.lock().unwrap() = OffloadSettings { mode, prefer: prefer.map(Into::into), fallback_local, ..Default::default() };
        }

        /// One turn through the provider, and the model name the trace would record.
        fn ask(&self, rt: &tokio::runtime::Runtime) -> Result<(String, String), String> {
            let model = self.models.open(&ModelChoice::Gemma4E4B).map_err(|e| e.to_string())?;
            let turn =
                rt.block_on(model.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new())).map_err(|e| e.to_string())?;
            Ok((turn.text, model.name()))
        }
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
    }

    #[test]
    fn a_refusal_for_revoked_or_locked_reads_as_a_reason() {
        let labels = vec![("12D3KooWDesk".to_string(), "desktop".to_string())];
        let unavailable = |text: &str| explain(ModelError::Unavailable(text.into()), &labels).to_string();
        assert!(unavailable("12D3KooWDesk: node refused: revoked").starts_with("model unavailable: desktop no longer lets this device use it"));
        assert!(unavailable("12D3KooWDesk: node refused: profile-locked").contains("desktop is locked. Unlock it"));
        assert!(
            explain(ModelError::Failed("12D3KooWDesk: node refused: revoked".into()), &labels).to_string().contains("revoked from your devices"),
            "also after text streamed"
        );
        assert_eq!(unavailable("12D3KooWDesk: node busy"), "model unavailable: 12D3KooWDesk: node busy", "others are left as they are");
        assert_eq!(explain(ModelError::Cancelled, &labels), ModelError::Cancelled);
    }

    #[test]
    fn the_turn_goes_to_the_linked_device_when_the_local_model_cannot_run() {
        let (rt, p) = (rt(), pair());
        let _serving = p.serve().unwrap();
        let a = asker(&p, false);
        let (text, name) = a.ask(&rt).unwrap();
        assert_eq!(text, "from the desk's model");
        assert!(name.ends_with("@desk"), "the trace names the device: {name}");
        assert_eq!(a.models.runs_on().get(), None, "nothing is running now");
    }

    #[test]
    fn automatic_runs_here_while_the_local_model_can_and_the_choice_is_respected() {
        let (rt, p) = (rt(), pair());
        let _serving = p.serve().unwrap();
        let a = asker(&p, true);
        assert_eq!(a.ask(&rt).unwrap().0, "from the phone's own model", "Automatic: here while it can");
        a.set(OffloadMode::Always, Some(&p.desk_id), true);
        assert_eq!(a.ask(&rt).unwrap().0, "from the desk's model", "Run on the desk, though the local model could run");
        a.set(OffloadMode::Never, Some(&p.desk_id), true);
        assert_eq!(a.ask(&rt).unwrap().0, "from the phone's own model", "This device");
        a.own_ready.store(false, Ordering::SeqCst);
        let err = a.ask(&rt).unwrap_err();
        assert!(err.contains("AVX2"), "This device with no model says why, and does not go elsewhere: {err}");
        a.set(OffloadMode::Always, Some("12D3KooWSomeoneElse"), true);
        assert!(a.ask(&rt).is_err(), "a chosen device that is not serving is not swapped for another");
    }

    #[test]
    fn it_falls_back_to_the_local_model_when_the_device_refuses_or_is_busy() {
        let (rt, p) = (rt(), pair());
        let _serving = p.serve().unwrap();
        let a = asker(&p, true);
        a.set(OffloadMode::Always, Some(&p.desk_id), true);
        p.on.store(false, Ordering::SeqCst);
        assert_eq!(a.ask(&rt).unwrap().0, "from the phone's own model", "the desk refuses: serving is off");
        p.on.store(true, Ordering::SeqCst);
        p.busy.store(true, Ordering::SeqCst);
        assert_eq!(a.ask(&rt).unwrap().0, "from the phone's own model", "the desk is busy with its own task");
        p.busy.store(false, Ordering::SeqCst);
        assert_eq!(a.ask(&rt).unwrap().0, "from the desk's model");
        // No fallback allowed, or no local model: the refusal is the answer.
        p.on.store(false, Ordering::SeqCst);
        a.set(OffloadMode::Always, Some(&p.desk_id), false);
        assert!(a.ask(&rt).is_err());
        a.set(OffloadMode::Auto, None, true);
        a.own_ready.store(false, Ordering::SeqCst);
        assert!(a.ask(&rt).is_err(), "nothing here, nothing there");
    }

    #[test]
    fn the_working_line_knows_the_device_while_its_turn_runs() {
        let (rt, p) = (rt(), pair());
        let runs_on = RunsOn::default();
        *p.provider.peek.lock().unwrap() = Some(runs_on.clone());
        let _serving = p.serve().unwrap();
        let a = asker(&p, false);
        // The asker shares its marker with the test's desk, which reads it mid-turn.
        let mut a = a;
        a.models.inner = Arc::new(Inner { runs_on, ..Arc::try_unwrap(a.models.inner).ok().unwrap() });
        assert_eq!(a.ask(&rt).unwrap().0, "from the desk's model");
        assert_eq!(p.provider.seen.lock().unwrap().as_slice(), [Some("desk".to_string())]);
        assert_eq!(a.models.runs_on().get(), None, "cleared when the turn ended");
    }

    /// A Ymir inference node behind a test router: it serves catalog/1 and infer/2; the other
    /// linked device ("desk", a Morph desktop) serves neither.
    mod pool {
        use super::*;
        use morph_ai::remote::{LinkRx, LinkTx};
        use morph_ai::{Link, RouteError};
        use weirdnet_agent::remote::catalog::{self, CatalogFeed, CatalogModel, ModelState, Purpose};
        use weirdnet_agent::remote::infer::v2::{self, Finish, InferBackend, Output};
        use weirdnet_agent::remote::infer::{Admission, InferHost};
        use weirdnet_agent::remote::{Caller, FrameRx, FrameTx, RemoteError, memory_pair};

        struct Says(&'static str);

        #[async_trait]
        impl InferBackend for Says {
            fn name(&self) -> String {
                "qwen3-8b".into()
            }

            async fn infer(&self, request: &v2::Request, out: &Output, _cancel: &CancelToken) -> Result<Finish, RemoteError> {
                out.text(format!("{} ({})", self.0, request.model.as_deref().unwrap_or("pool's choice")));
                Ok(Finish::new("stop"))
            }
        }

        struct Node;

        impl InferHost for Node {
            fn authorize(&self, _caller: &Caller) -> Result<(), RemoteError> {
                Ok(())
            }

            fn open(&self, _caller: &Caller, _model: Option<&str>) -> Result<Arc<dyn ModelBackend>, RemoteError> {
                Err(RemoteError::unavailable("infer/2 only"))
            }

            fn open_request(&self, _caller: &Caller, _request: &v2::Request) -> Result<Arc<dyn InferBackend>, RemoteError> {
                Ok(Arc::new(Says("from the pool")))
            }
        }

        struct Tx(Box<dyn FrameTx>);
        struct Rx(Box<dyn FrameRx>);

        #[async_trait]
        impl LinkTx for Tx {
            async fn send(&mut self, message: Vec<u8>) -> Result<(), RouteError> {
                self.0.send(message).await.map_err(|_| RouteError::Closed)
            }
        }

        #[async_trait]
        impl LinkRx for Rx {
            async fn recv(&mut self) -> Result<Option<Vec<u8>>, RouteError> {
                self.0.recv().await.map_err(|_| RouteError::Closed)
            }
        }

        struct Linked {
            /// The Ymir node is up.
            up: Arc<AtomicBool>,
            feed: CatalogFeed,
        }

        #[async_trait]
        impl NodeRouter for Linked {
            fn nodes(&self) -> Vec<LinkedNode> {
                let node = |id: &str, label: &str, this_device| LinkedNode {
                    peer_id: id.into(),
                    label: label.into(),
                    platform: Platform::Desktop,
                    online: true,
                    this_device,
                    serves_models: false,
                };
                vec![node("me", "this laptop", true), node("desk", "desk", false), node("12D3KooWYmir", "gpu box", false)]
            }

            async fn open(&self, peer: &str, protocol: &str) -> Result<Link, RouteError> {
                if peer != "12D3KooWYmir" || !self.up.load(Ordering::SeqCst) {
                    return Err(if peer == "desk" { RouteError::Unsupported(protocol.into()) } else { RouteError::Unreachable(peer.into()) });
                }
                let (ours, theirs) = memory_pair();
                let feed = self.feed.clone();
                let protocol = protocol.to_string();
                tokio::spawn(async move {
                    if protocol == catalog::PROTOCOL {
                        let _ = catalog::serve(theirs, Caller::new("me"), &feed).await;
                    } else {
                        let _ = v2::serve(theirs, Caller::new("me"), &Node, &Admission::new(2)).await;
                    }
                });
                Ok(Link { tx: Box::new(Tx(ours.tx)), rx: Box::new(Rx(ours.rx)) })
            }
        }

        struct Fixture {
            models: RoutedModels,
            pool: YmirPool,
            up: Arc<AtomicBool>,
            own_ready: Arc<AtomicBool>,
            settings: Arc<Mutex<OffloadSettings>>,
        }

        fn fixture(up: bool, local_ready: bool) -> Fixture {
            let up = Arc::new(AtomicBool::new(up));
            let feed = CatalogFeed::new(Arc::new(|_: &Caller| Ok(())));
            feed.publish(vec![CatalogModel::new("qwen3-8b", Purpose::Chat, ModelState::Warm).with_speed(42.0)]);
            let slot = RouterSlot::default();
            slot.set(Some(Arc::new(Linked { up: up.clone(), feed })));
            let own_ready = Arc::new(AtomicBool::new(local_ready));
            let settings = Arc::new(Mutex::new(OffloadSettings { pool: true, ..Default::default() }));
            let (ready, s) = (own_ready.clone(), settings.clone());
            let pool = YmirPool::default();
            let models = RoutedModels::new(
                Arc::new(Own(own_ready.clone())),
                move |_| ready.load(Ordering::SeqCst),
                move || s.lock().unwrap().clone(),
                slot,
                Platform::Desktop,
            )
            .with_pool(pool.clone());
            Fixture { models, pool, up, own_ready, settings }
        }

        fn turn(rt: &tokio::runtime::Runtime, model: &Arc<dyn ModelBackend>) -> (Result<String, ModelError>, Vec<ModelNotice>) {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let r = rt.block_on(model.complete(&[Message::user("hi")], &[], &DeltaSink::new(Some(tx)), &CancelToken::new())).map(|t| t.text);
            let mut notices = Vec::new();
            while let Ok(e) = rx.try_recv() {
                if let AgentEvent::ModelNotice(n) = e {
                    notices.push(n);
                }
            }
            (r, notices)
        }

        #[test]
        fn turns_go_to_the_pool_and_follow_the_setting_from_turn_to_turn() {
            let (rt, f) = (rt(), fixture(true, true));
            let model = f.models.open(&ModelChoice::Gemma4E4B).unwrap();
            let (text, notices) = turn(&rt, &model);
            assert_eq!(text.unwrap(), "from the pool (pool's choice)");
            assert!(notices.is_empty());
            assert_eq!(model.name(), "qwen3-8b@gpu box", "the trace names the pool node's device");
            assert!(f.pool.status().is_live());
            f.settings.lock().unwrap().pool_model = Some("qwen3-8b".into());
            assert_eq!(turn(&rt, &model).0.unwrap(), "from the pool (qwen3-8b)", "the chosen pool model");
            f.settings.lock().unwrap().pool = false;
            assert_eq!(turn(&rt, &model).0.unwrap(), "from the phone's own model", "Run on changed: the same session runs here now");
            assert_eq!(f.models.runs_on().get(), None);
        }

        #[test]
        fn an_unreachable_pool_falls_back_to_this_device_with_a_notice() {
            let (rt, f) = (rt(), fixture(false, true));
            let model = f.models.open(&ModelChoice::Gemma4E4B).unwrap();
            let (text, notices) = turn(&rt, &model);
            assert_eq!(text.unwrap(), "from the phone's own model");
            assert!(matches!(&notices[..], [ModelNotice::FellBack { reason, .. }] if reason.contains("unreachable")), "{notices:?}");
            assert!(matches!(f.pool.status(), weirdnet_agent::remote::pool_model::PoolStatus::Unreachable { .. }));
        }

        #[test]
        fn with_no_local_model_an_unreachable_pool_says_what_to_do() {
            let (rt, f) = (rt(), fixture(false, false));
            let model = f.models.open(&ModelChoice::Gemma4E4B).unwrap();
            let err = turn(&rt, &model).0.unwrap_err();
            assert!(matches!(&err, ModelError::PoolUnreachable(why) if why.contains("Ymir inference node")), "{err:?}");
            f.up.store(true, Ordering::SeqCst);
            f.own_ready.store(true, Ordering::SeqCst);
            assert_eq!(turn(&rt, &model).0.unwrap(), "from the pool (pool's choice)", "the pool came up: the next turn goes there");
        }
    }
}
