//! Serving this device's model to the user's other devices: registers [`InferService`] for
//! infer/1 and infer/2 on a [`DeviceRouter`] and runs the sessions until the handle is dropped.
//!
//! Registration is [`DeviceRouter::serve`] (accepting streams from linked devices only), and
//! the host is always behind [`RosterHost`](crate::node_router::protocols::RosterHost): a
//! caller must be a current, non-revoked linked device and the profile unlocked, before the
//! host's own rules (the setting, the model) apply. Dropping the [`InferServing`] stops
//! accepting and cancels the turns in flight; the app drops it when the setting goes off, the
//! profile locks or the sync node restarts.

use futures::future::{AbortHandle, Abortable, join};
use morph_ai::RouteError;
use weirdnet_agent::remote::infer::{Admission, InferHost};

use crate::node_router::{DeviceRouter, protocols};

/// Inference being served on a router; dropping it stops serving.
pub struct InferServing {
    abort: AbortHandle,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl InferServing {
    /// Whether the sessions are still being driven (not once the node stopped accepting).
    pub fn running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}

impl Drop for InferServing {
    fn drop(&mut self) {
        self.abort.abort();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Serve `make()`'s host on `router` for infer/1 and infer/2, `admission` at a time (it is
/// shared by both protocols). The router's caller holds the profile, and its lock refuses everything.
/// Fails when the router cannot accept (for example, a locked profile, or the protocol is
/// already served on this node).
pub fn serve_inference<H: InferHost + 'static>(
    router: &DeviceRouter,
    make: impl Fn() -> H,
    admission: Admission,
) -> Result<InferServing, RouteError> {
    let gate = router.caller().clone();
    let v1 = protocols::InferService::new(make(), gate.clone(), admission.clone());
    let v2 = protocols::InferService::new(make(), gate, admission).v2();
    let first = router.serve(protocols::INFER, std::sync::Arc::new(v1))?;
    let second = router.serve(protocols::INFER_V2, std::sync::Arc::new(v2))?;
    let (abort, registration) = AbortHandle::new_pair();
    let thread = std::thread::Builder::new()
        .name("morph-infer-serving".into())
        .spawn(move || {
            // The agent protocols time and wait on tokio.
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_time().build() else { return };
            let _ = rt.block_on(Abortable::new(join(first, second), registration));
        })
        .map_err(|e| RouteError::Failed(e.to_string()))?;
    Ok(InferServing { abort, thread: Some(thread) })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::device_calls::DeviceCaller;
    use morph_ai::{LocalInferHost, ModelChoice, ModelProvider, OffloadPlan};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use weirdnet_agent::remote::infer::Route;
    use weirdnet_agent::testing::ScriptedModel;
    use weirdnet_agent::{CancelToken, DeltaSink, Message, ModelBackend, ModelError, ModelTurn};
    use weirdnet_net::Keypair;
    use weirdnet_net::Multiaddr;
    use weirdnet_net::profile::WEIRDNET;
    use weirdnet_net::user::NodeCert;
    use weirdnet_sync::{Roster, SyncState, node_config, start_node};

    pub(crate) fn said(text: &str) -> Arc<dyn ModelBackend> {
        Arc::new(ScriptedModel::new(vec![ModelTurn { text: text.into(), tool_calls: vec![] }]))
    }

    /// The desk's model provider: ready, or refusing with a reason (no download, unsupported CPU).
    pub(crate) struct Desk {
        pub(crate) refusal: Mutex<Option<String>>,
        /// A real provider to serve instead of the scripted model (the ignored Gemma test).
        pub(crate) real: Mutex<Option<Arc<dyn ModelProvider>>>,
        /// A marker the desk's model reads while it runs a turn (tests of the working line).
        pub(crate) peek: Mutex<Option<crate::routed_models::RunsOn>>,
        pub(crate) seen: Arc<Mutex<Vec<Option<String>>>>,
    }

    /// The desk's model: answers, and notes what the marker said while it did.
    struct Peeking(Option<crate::routed_models::RunsOn>, Arc<Mutex<Vec<Option<String>>>>);

    #[async_trait::async_trait]
    impl ModelBackend for Peeking {
        fn name(&self) -> String {
            "desk-model".into()
        }

        async fn complete(
            &self,
            _m: &[Message],
            _t: &[weirdnet_agent::tool::ToolSpec],
            d: &DeltaSink,
            _c: &CancelToken,
        ) -> Result<ModelTurn, ModelError> {
            if let Some(r) = &self.0 {
                self.1.lock().unwrap().push(r.get());
            }
            for piece in "from the desk's model".split_inclusive(' ') {
                d.text(piece);
            }
            Ok(ModelTurn { text: "from the desk's model".into(), tool_calls: vec![] })
        }
    }

    impl ModelProvider for Desk {
        fn open(&self, choice: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError> {
            if let Some(real) = &*self.real.lock().unwrap() {
                return real.open(choice);
            }
            match &*self.refusal.lock().unwrap() {
                None => Ok(Arc::new(Peeking(self.peek.lock().unwrap().clone(), self.seen.clone()))),
                Some(why) => Err(ModelError::Unavailable(why.clone())),
            }
        }
    }

    struct Device {
        key: Keypair,
        cert: NodeCert,
    }

    fn device(user: &Keypair, label: &str) -> Device {
        let key = Keypair::generate_ed25519();
        let cert = Roster::self_certify(&WEIRDNET, user, &key.public().to_peer_id().to_string(), label, 1).unwrap();
        Device { key, cert }
    }

    fn up(user: &Keypair, me: &Device, others: &[&NodeCert], server: Option<Multiaddr>) -> (DeviceCaller, Vec<String>) {
        let mut st = SyncState::new(vec![9; 32], user.public(), me.cert.clone(), Some(&me.key), vec!["bookmarks".into()]).unwrap();
        st.auto_first_rotation = false;
        for c in others {
            st.roster.upsert((*c).clone());
        }
        let st = Arc::new(Mutex::new(st));
        let node = Arc::new(start_node(me.key.clone(), node_config(true), st.clone()).unwrap());
        let listening = node.status().listening.clone();
        (DeviceCaller::new(node, st, move |_| server.clone().into_iter().collect()), listening)
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
    }

    /// A desk serving (or not yet) and a phone linked to it.
    pub(crate) struct Pair {
        pub(crate) desk: DeviceRouter,
        pub(crate) phone: Arc<dyn morph_ai::NodeRouter>,
        pub(crate) desk_id: String,
        pub(crate) provider: Arc<Desk>,
        pub(crate) on: Arc<AtomicBool>,
        pub(crate) busy: Arc<AtomicBool>,
    }

    pub(crate) fn pair() -> Pair {
        let user = Keypair::generate_ed25519();
        let (a, b) = (device(&user, "desk"), device(&user, "phone"));
        let (server, listening) = up(&user, &a, &[&b.cert], None);
        let at: Multiaddr = format!("{}/p2p/{}", listening[0], server.peer_id()).parse().unwrap();
        let (phone, _) = up(&user, &b, &[&a.cert], Some(at));
        Pair {
            desk: DeviceRouter::new(server),
            // The directory (capabilities) says the desk serves models, as it does once it asked.
            phone: Arc::new(DeviceRouter::new(phone).with_info(|_| crate::node_router::NodeInfo {
                platform: morph_ai::Platform::Desktop,
                online: true,
                serves_models: true,
            })),
            desk_id: a.cert.peer_id,
            provider: Arc::new(Desk { refusal: Mutex::new(None), real: Mutex::default(), peek: Mutex::default(), seen: Arc::default() }),
            on: Arc::new(AtomicBool::new(true)),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    impl Pair {
        pub(crate) fn serve(&self) -> Result<InferServing, RouteError> {
            let (provider, on, busy) = (self.provider.clone(), self.on.clone(), self.busy.clone());
            serve_inference(
                &self.desk,
                move || {
                    let (on, busy) = (on.clone(), busy.clone());
                    LocalInferHost::new(provider.clone(), || ModelChoice::Gemma4E4B, move || on.load(Ordering::SeqCst))
                        .with_local_priority(move || busy.load(Ordering::SeqCst))
                },
                Admission::with_queue(1, 2),
            )
        }

        /// The phone asks the desk for a turn, with its own model as the fallback.
        fn ask(&self, rt: &tokio::runtime::Runtime) -> (String, Option<Route>) {
            let plan = OffloadPlan { nodes: vec![self.desk_id.clone()], fallback_local: true };
            let model = protocols::remote_model(self.phone.clone(), &plan, Some(said("from the phone")));
            let turn = rt.block_on(model.complete(&[Message::user("hi")], &[], &DeltaSink::discard(), &CancelToken::new())).unwrap();
            (turn.text, model.last_route())
        }
    }

    #[test]
    fn a_linked_device_gets_its_turn_served_when_the_setting_is_on() {
        let (rt, p) = (rt(), pair());
        let _serving = p.serve().unwrap();
        let (text, route) = p.ask(&rt);
        assert_eq!(text, "from the desk's model");
        assert!(matches!(&route, Some(Route::Node { node, .. }) if *node == p.desk_id), "{route:?}");
    }

    #[test]
    fn it_refuses_when_the_setting_is_off_and_serves_again_when_it_is_turned_on() {
        let (rt, p) = (rt(), pair());
        let _serving = p.serve().unwrap();
        p.on.store(false, Ordering::SeqCst);
        let (text, route) = p.ask(&rt);
        assert_eq!((text.as_str(), route), ("from the phone", Some(Route::Fallback)));
        p.on.store(true, Ordering::SeqCst);
        assert_eq!(p.ask(&rt).0, "from the desk's model");
    }

    #[test]
    fn with_no_model_or_an_unsupported_cpu_the_turn_runs_on_the_phone() {
        let (rt, p) = (rt(), pair());
        let _serving = p.serve().unwrap();
        *p.provider.refusal.lock().unwrap() = Some("Gemma 4 E4B is not downloaded yet".into());
        assert_eq!(p.ask(&rt), ("from the phone".into(), Some(Route::Fallback)));
        *p.provider.refusal.lock().unwrap() = Some("This CPU lacks AVX2/FMA needed for local models".into());
        assert_eq!(p.ask(&rt), ("from the phone".into(), Some(Route::Fallback)));
        *p.provider.refusal.lock().unwrap() = None;
        assert_eq!(p.ask(&rt).0, "from the desk's model");
    }

    #[test]
    fn the_desks_own_task_goes_first() {
        let (rt, p) = (rt(), pair());
        let _serving = p.serve().unwrap();
        p.busy.store(true, Ordering::SeqCst);
        assert_eq!(p.ask(&rt), ("from the phone".into(), Some(Route::Fallback)));
        p.busy.store(false, Ordering::SeqCst);
        assert_eq!(p.ask(&rt).0, "from the desk's model");
    }

    #[test]
    fn dropping_the_handle_stops_serving_and_serving_can_start_again() {
        let (rt, p) = (rt(), pair());
        let serving = p.serve().unwrap();
        assert!(serving.running());
        assert_eq!(p.ask(&rt).0, "from the desk's model");
        drop(serving);
        assert_eq!(p.ask(&rt), ("from the phone".into(), Some(Route::Fallback)), "nothing accepts the stream any more");
        let again = p.serve().expect("the protocols are free again");
        assert_eq!(p.ask(&rt).0, "from the desk's model");
        drop(again);
    }

    #[test]
    fn serving_twice_on_one_router_is_refused() {
        let p = pair();
        let _first = p.serve().unwrap();
        assert!(p.serve().is_err());
    }

    /// A real Gemma over the loopback, with the timing:
    /// `cargo test -p morph-services --release -- --ignored --nocapture real_gemma`
    /// (needs the model downloaded in the default models directory).
    #[test]
    #[ignore = "needs the Gemma 4 E4B model on disk and takes a while"]
    fn real_gemma_over_the_loopback() {
        use std::time::Instant;
        let factory: morph_ai::EngineFactory = Arc::new(|path| {
            weirdnet_model::llama::LlamaEngine::load(path, weirdnet_model::llama::LlamaOptions::default())
                .map(|e| Box::new(e) as Box<dyn weirdnet_model::Engine>)
        });
        let models = Arc::new(morph_ai::LocalModels::new(weirdnet_model::ModelStore::from_env(), factory));
        let (rt, p) = (rt(), pair());
        *p.provider.real.lock().unwrap() = Some(models);
        let _serving = p.serve().unwrap();
        let plan = OffloadPlan { nodes: vec![p.desk_id.clone()], fallback_local: false };
        let model = protocols::remote_model(p.phone.clone(), &plan, None);
        let t0 = Instant::now();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = DeltaSink::new(Some(tx));
        let messages = [Message::user("Reply with one short sentence: what is the capital of France?")];
        let (turn, first) = rt.block_on(async {
            let first = tokio::spawn(async move {
                rx.recv().await;
                t0.elapsed()
            });
            let turn = model.complete(&messages, &[], &sink, &CancelToken::new()).await.expect("the remote turn");
            drop(sink);
            (turn, first.await.unwrap())
        });
        println!("REAL GEMMA: total {:?}, first delta {:?}, route {:?}, text {:?}", t0.elapsed(), first, model.last_route(), turn.text);
        assert!(turn.text.to_lowercase().contains("paris"), "{}", turn.text);
    }
}
