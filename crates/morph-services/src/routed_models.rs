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
//! - [`RunsOn`] says which device a turn is on, for the working line ("Thinking on my-desktop…").

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use morph_ai::{LinkedNode, ModelChoice, ModelProvider, NodeRouter, OffloadPlan, OffloadSettings, Platform, offload_plan};
use weirdnet_agent::remote::infer::Route;
use weirdnet_agent::tool::ToolSpec;
use weirdnet_agent::{CancelToken, DeltaSink, Message, ModelBackend, ModelError, ModelTurn};

use crate::node_router::protocols;

/// The router of the profile's sync node while it runs, shared with the provider (the panel's
/// device tracking sets it; no router means no linked devices).
#[derive(Clone, Default)]
pub struct RouterSlot(Arc<Mutex<Option<Arc<dyn NodeRouter>>>>);

impl RouterSlot {
    pub fn set(&self, router: Option<Arc<dyn NodeRouter>>) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = router;
    }

    fn get(&self) -> Option<Arc<dyn NodeRouter>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
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

type Settings = Arc<dyn Fn() -> OffloadSettings + Send + Sync>;
type Ready = Arc<dyn Fn(&ModelChoice) -> bool + Send + Sync>;

/// Opens models here or on linked devices, per the "Run on" setting.
pub struct RoutedModels {
    local: Arc<dyn ModelProvider>,
    local_ready: Ready,
    settings: Settings,
    router: RouterSlot,
    platform: Platform,
    runs_on: RunsOn,
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
        Self { local, local_ready: Arc::new(local_ready), settings: Arc::new(settings), router, platform, runs_on: RunsOn::default() }
    }

    pub fn runs_on(&self) -> RunsOn {
        self.runs_on.clone()
    }

    /// Where a turn with `choice` would run now.
    pub fn plan(&self, choice: &ModelChoice) -> (OffloadPlan, Vec<LinkedNode>) {
        let nodes = self.router.get().map(|r| r.nodes()).unwrap_or_default();
        (offload_plan(&(self.settings)(), self.platform, &nodes, (self.local_ready)(choice)), nodes)
    }
}

impl ModelProvider for RoutedModels {
    fn open(&self, choice: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError> {
        let (plan, nodes) = self.plan(choice);
        let Some(router) = self.router.get().filter(|_| !plan.runs_here()) else {
            return self.local.open(choice);
        };
        // The local model backs the turn up only when it can run here at all.
        let local = if (self.local_ready)(choice) { self.local.open(choice).ok() } else { None };
        let label = |peer: &str| nodes.iter().find(|n| n.peer_id == peer).map(|n| n.label.clone()).unwrap_or_else(|| peer.to_string());
        let first = plan.nodes.first().map(|p| label(p)).unwrap_or_default();
        let local = local.map(|inner| Arc::new(OnThisDevice { inner, runs_on: self.runs_on.clone() }) as Arc<dyn ModelBackend>);
        let remote = protocols::remote_model(router, &plan, local);
        let labels = nodes.iter().map(|n| (n.peer_id.clone(), n.label.clone())).collect();
        Ok(Arc::new(Remote { inner: remote, first, labels, runs_on: self.runs_on.clone() }))
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
            *self.settings.lock().unwrap() = OffloadSettings { mode, prefer: prefer.map(Into::into), fallback_local, serve: false };
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
        let labels = vec![("12D3KooWDesk".to_string(), "my-desktop".to_string())];
        let unavailable = |text: &str| explain(ModelError::Unavailable(text.into()), &labels).to_string();
        assert!(unavailable("12D3KooWDesk: node refused: revoked").starts_with("model unavailable: my-desktop no longer lets this device use it"));
        assert!(unavailable("12D3KooWDesk: node refused: profile-locked").contains("my-desktop is locked. Unlock it"));
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
        a.models.runs_on = runs_on;
        assert_eq!(a.ask(&rt).unwrap().0, "from the desk's model");
        assert_eq!(p.provider.seen.lock().unwrap().as_slice(), [Some("desk".to_string())]);
        assert_eq!(a.models.runs_on().get(), None, "cleared when the turn ended");
    }
}
