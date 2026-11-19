//! The real [`ModelProvider`]: Gemma 4 from `weirdnet-model`. Nothing downloads until the user
//! agrees in the Companion panel; then the download runs on its own thread with progress, and the
//! first question after it loads the model on a worker.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use weirdnet_agent::{ModelBackend, ModelError};
use weirdnet_model::catalog::{self, ModelSpec};
use weirdnet_model::{Consent, Engine, ModelStore, Worker, human};

use crate::model::{ModelChoice, ModelProvider};

/// Builds an inference engine for a model file (the app passes llama.cpp; tests pass a mock).
pub type EngineFactory = Arc<dyn Fn(&Path) -> weirdnet_model::Result<Box<dyn Engine>> + Send + Sync>;

/// Where a model stands, for the panel.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelStatus {
    /// Not on disk. `partial` bytes are kept from an interrupted download.
    Missing {
        label: String,
        size: u64,
        partial: u64,
        consent: Vec<String>,
    },
    Downloading {
        label: String,
        done: u64,
        total: u64,
    },
    Ready {
        label: String,
    },
    Failed {
        label: String,
        error: String,
    },
    /// Not a catalog model.
    Unsupported {
        label: String,
    },
    /// This CPU lacks instructions the build's llama.cpp needs, so no local model can run.
    CpuUnsupported {
        label: String,
        reason: String,
    },
}

/// Why this CPU cannot run local models: the instructions the build needs and the CPU lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuLimit {
    /// `is_x86_feature_detected!` names, e.g. `avx2`, `fma`.
    pub missing: Vec<String>,
}

impl CpuLimit {
    /// The limit a failed `weirdnet_model::cpu::check()` describes; `None` when it passed (or
    /// failed for another reason).
    pub fn from_check(check: weirdnet_model::Result<()>) -> Option<Self> {
        match check {
            Err(weirdnet_model::Error::UnsupportedCpu { missing }) => Some(Self { missing }),
            _ => None,
        }
    }

    /// One line for Settings: "This CPU lacks AVX2/FMA needed for local models".
    pub fn reason(&self) -> String {
        let names: Vec<String> = self.missing.iter().map(|m| m.to_uppercase()).collect();
        format!("This CPU lacks {} needed for local models", names.join("/"))
    }

    /// What to do instead, for the panel and Settings.
    pub const INSTEAD: &'static str = "Use a companion that runs on another device, or send turns to a linked device that serves models (offload).";
}

/// The limit of this machine, if any. `MORPH_FORCE_UNSUPPORTED_CPU=avx2,fma` forces one (to see the
/// screens without an old CPU).
pub fn cpu_limit() -> Option<CpuLimit> {
    if let Ok(forced) = std::env::var("MORPH_FORCE_UNSUPPORTED_CPU") {
        let missing: Vec<String> = forced.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if !missing.is_empty() {
            return Some(CpuLimit { missing });
        }
    }
    CpuLimit::from_check(weirdnet_model::cpu::check())
}

#[derive(Default)]
struct State {
    downloading: HashMap<String, (u64, u64)>,
    failed: HashMap<String, String>,
    cancel: HashMap<String, bool>,
    workers: HashMap<String, Arc<Worker>>,
    /// Set once a worker's model has loaded (the first question loads it).
    loaded: HashMap<String, Arc<AtomicBool>>,
}

/// Opens catalog models from a [`ModelStore`].
pub struct LocalModels {
    store: ModelStore,
    factory: EngineFactory,
    state: Arc<Mutex<State>>,
    cpu: Option<CpuLimit>,
}

fn spec_for(choice: &ModelChoice) -> Option<ModelSpec> {
    match choice {
        ModelChoice::Gemma4E4B => catalog::find(catalog::GEMMA_4_E4B),
        ModelChoice::Gemma4E2B => catalog::find(catalog::GEMMA_4_E2B),
        ModelChoice::Custom(id) => catalog::find(id),
    }
}

impl LocalModels {
    pub fn new(store: ModelStore, factory: EngineFactory) -> Self {
        Self { store, factory, state: Arc::default(), cpu: cpu_limit() }
    }

    /// As [`new`](Self::new) with the CPU limit chosen by the caller (tests).
    pub fn with_cpu_limit(mut self, cpu: Option<CpuLimit>) -> Self {
        self.cpu = cpu;
        self
    }

    /// Whether `choice` is loaded and ready to answer at once. `false` until the first question
    /// after launch has loaded it (which takes a while for a large model), so the panel can say
    /// "Loading…" rather than "Thinking…".
    pub fn is_loaded(&self, choice: &ModelChoice) -> bool {
        let Some(spec) = spec_for(choice) else { return false };
        self.state.lock().unwrap().loaded.get(&spec.id).is_some_and(|l| l.load(Ordering::SeqCst))
    }

    /// Why local models cannot run here, if they cannot.
    pub fn cpu_limit(&self) -> Option<&CpuLimit> {
        self.cpu.as_ref()
    }

    pub fn status(&self, choice: &ModelChoice) -> ModelStatus {
        let label = choice.label();
        if let Some(cpu) = &self.cpu {
            return ModelStatus::CpuUnsupported { label, reason: cpu.reason() };
        }
        let Some(spec) = spec_for(choice) else { return ModelStatus::Unsupported { label } };
        let st = self.state.lock().unwrap();
        if let Some((done, total)) = st.downloading.get(&spec.id) {
            return ModelStatus::Downloading { label, done: *done, total: *total };
        }
        if self.store.installed(&spec) {
            return ModelStatus::Ready { label };
        }
        if let Some(error) = st.failed.get(&spec.id) {
            return ModelStatus::Failed { label, error: error.clone() };
        }
        ModelStatus::Missing { label, size: spec.size, partial: self.store.partial_bytes(&spec), consent: spec.consent_text(self.store.hub()) }
    }

    /// The user agreed: download in the background. Does nothing if it is already running or done.
    pub fn start_download(&self, choice: &ModelChoice) {
        let Some(spec) = spec_for(choice) else { return };
        if self.cpu.is_some() {
            return; // a model this CPU cannot run is not worth gigabytes
        }
        {
            let mut st = self.state.lock().unwrap();
            if st.downloading.contains_key(&spec.id) || self.store.installed(&spec) {
                return;
            }
            st.failed.remove(&spec.id);
            st.cancel.insert(spec.id.clone(), false);
            st.downloading.insert(spec.id.clone(), (self.store.partial_bytes(&spec), spec.size));
        }
        let (store, state) = (self.store.clone(), self.state.clone());
        std::thread::Builder::new()
            .name("morph-model-download".into())
            .spawn(move || {
                let consent = Consent::given(&spec);
                let result = weirdnet_model::download(&store, &spec, &consent, &mut |p| {
                    let mut st = state.lock().unwrap();
                    st.downloading.insert(spec.id.clone(), (p.done, p.total));
                    !st.cancel.get(&spec.id).copied().unwrap_or(false)
                });
                let mut st = state.lock().unwrap();
                st.downloading.remove(&spec.id);
                if let Err(e) = result
                    && !st.cancel.get(&spec.id).copied().unwrap_or(false)
                {
                    st.failed.insert(spec.id.clone(), e.to_string());
                }
            })
            .expect("spawn the download thread");
    }

    /// Stops a running download; the partial file is kept for a resume.
    pub fn cancel_download(&self, choice: &ModelChoice) {
        if let Some(spec) = spec_for(choice) {
            self.state.lock().unwrap().cancel.insert(spec.id, true);
        }
    }
}

/// "Gemma 4 E4B is 4.6 GB" for the panel.
pub fn size_label(size: u64) -> String {
    human(size)
}

impl ModelProvider for LocalModels {
    fn open(&self, choice: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError> {
        let label = choice.label();
        if let Some(cpu) = &self.cpu {
            return Err(ModelError::Unavailable(format!("{label}: {}. {}", cpu.reason(), CpuLimit::INSTEAD)));
        }
        let spec = spec_for(choice).ok_or_else(|| ModelError::Unavailable(format!("{label} is not a model Morph can run locally")))?;
        if !self.store.installed(&spec) {
            let why = match self.status(choice) {
                ModelStatus::Downloading { done, total, .. } => format!("is still downloading ({} of {})", human(done), human(total)),
                ModelStatus::Failed { error, .. } => format!("could not be downloaded: {error}"),
                _ => format!("is not downloaded yet ({}). Use Download in the Companion panel", human(spec.size)),
            };
            return Err(ModelError::Unavailable(format!("{label} {why}")));
        }
        let worker = {
            let mut st = self.state.lock().unwrap();
            let loaded = st.loaded.entry(spec.id.clone()).or_default().clone();
            st.workers
                .entry(spec.id.clone())
                .or_insert_with(|| {
                    let (path, factory) = (self.store.path(&spec), self.factory.clone());
                    Arc::new(Worker::spawn(move || {
                        let engine = factory(&path);
                        loaded.store(engine.is_ok(), Ordering::SeqCst);
                        engine
                    }))
                })
                .clone()
        };
        // Gemma 4 often calls tools in its own syntax rather than the one weirdnet-model asks for.
        Ok(Arc::new(crate::NativeCalls::new(Arc::new(crate::local_chat::LocalChat::new(spec.id, worker)))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;
    impl Engine for Fixed {
        fn generate(
            &mut self,
            _req: &weirdnet_model::GenerateRequest,
            on_text: &mut dyn FnMut(&str) -> bool,
        ) -> weirdnet_model::Result<weirdnet_model::Stats> {
            on_text("hi ");
            on_text("there");
            Ok(weirdnet_model::Stats { prompt_tokens: 1, generated_tokens: 2, stop: weirdnet_model::worker::StopCause::End })
        }
    }

    fn models(dir: &Path) -> LocalModels {
        // Port 9 refuses connections, so a download fails fast without touching the network.
        LocalModels::new(ModelStore::new(dir).with_hub("http://127.0.0.1:9"), Arc::new(|_| Ok(Box::new(Fixed)))).with_cpu_limit(None)
    }

    fn weak_cpu() -> Option<CpuLimit> {
        CpuLimit::from_check(Err(weirdnet_model::Error::UnsupportedCpu { missing: vec!["avx2".into(), "fma".into()] }))
    }

    fn install(dir: &Path, choice: &ModelChoice) {
        let spec = spec_for(choice).unwrap();
        let f = std::fs::File::create(dir.join(&spec.file)).unwrap();
        f.set_len(spec.size).unwrap();
    }

    #[test]
    fn a_missing_model_says_so_with_its_size_and_consent_lines() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path());
        match m.status(&ModelChoice::Gemma4E4B) {
            ModelStatus::Missing { size, partial, consent, .. } => {
                assert!(size > 4_000_000_000);
                assert_eq!(partial, 0);
                assert!(!consent.is_empty());
            }
            other => panic!("{other:?}"),
        }
        let err = m.open(&ModelChoice::Gemma4E4B).err().expect("not installed").to_string();
        assert!(err.contains("not downloaded yet") && err.contains("Gemma 4 E4B"), "{err}");
    }

    #[test]
    fn an_installed_model_opens_and_answers_through_the_agent_backend() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), &ModelChoice::Gemma4E2B);
        let m = models(dir.path());
        assert_eq!(m.status(&ModelChoice::Gemma4E2B), ModelStatus::Ready { label: "Gemma 4 E2B".into() });
        assert!(!m.is_loaded(&ModelChoice::Gemma4E2B), "nothing loads before a question");
        let backend = m.open(&ModelChoice::Gemma4E2B).unwrap();
        assert!(!m.is_loaded(&ModelChoice::Gemma4E2B), "opening does not load it either");
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let turn = rt
            .block_on(backend.complete(
                &[weirdnet_agent::Message::user("x")],
                &[],
                &weirdnet_agent::DeltaSink::discard(),
                &weirdnet_agent::CancelToken::new(),
            ))
            .unwrap();
        assert_eq!(turn.text, "hi there");
        assert!(m.is_loaded(&ModelChoice::Gemma4E2B), "the first question loaded it");
        assert!(!m.is_loaded(&ModelChoice::Gemma4E4B));
    }

    #[test]
    fn a_failed_cpu_check_says_what_is_missing_and_a_passing_one_is_no_limit() {
        let limit = weak_cpu().expect("an UnsupportedCpu error is a limit");
        assert_eq!(limit.reason(), "This CPU lacks AVX2/FMA needed for local models");
        assert!(CpuLimit::from_check(Ok(())).is_none());
        assert!(
            CpuLimit::from_check(Err(weirdnet_model::Error::Cancelled { done: "0".into(), total: "1".into() })).is_none(),
            "another failure is not a CPU limit"
        );
    }

    #[test]
    fn on_a_cpu_that_cannot_run_them_local_models_are_unavailable_not_offered_for_download() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path()).with_cpu_limit(weak_cpu());
        for choice in [ModelChoice::Gemma4E4B, ModelChoice::Gemma4E2B] {
            assert!(
                matches!(m.status(&choice), ModelStatus::CpuUnsupported { reason, .. } if reason == "This CPU lacks AVX2/FMA needed for local models"),
                "{choice:?}"
            );
            let err = m.open(&choice).err().expect("no local model opens").to_string();
            assert!(err.contains("AVX2/FMA") && err.contains("another device"), "{err}");
            m.start_download(&choice);
        }
        assert!(matches!(m.status(&ModelChoice::Gemma4E4B), ModelStatus::CpuUnsupported { .. }), "start_download did nothing: no download began");
        // The same models are offered normally on a CPU that can run them.
        assert!(matches!(models(dir.path()).status(&ModelChoice::Gemma4E4B), ModelStatus::Missing { .. }));
    }

    #[test]
    fn opening_twice_reuses_one_worker() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), &ModelChoice::Gemma4E2B);
        let m = models(dir.path());
        m.open(&ModelChoice::Gemma4E2B).unwrap();
        m.open(&ModelChoice::Gemma4E2B).unwrap();
        assert_eq!(m.state.lock().unwrap().workers.len(), 1);
    }

    #[test]
    fn an_unreachable_hub_becomes_a_failed_status_and_can_retry() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path());
        m.start_download(&ModelChoice::Gemma4E2B);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if matches!(m.status(&ModelChoice::Gemma4E2B), ModelStatus::Failed { .. }) {
                break;
            }
            assert!(std::time::Instant::now() < end, "{:?}", m.status(&ModelChoice::Gemma4E2B));
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let err = m.open(&ModelChoice::Gemma4E2B).err().unwrap().to_string();
        assert!(err.contains("could not be downloaded"), "{err}");
        m.start_download(&ModelChoice::Gemma4E2B);
    }

    #[test]
    fn custom_ids_outside_the_catalog_are_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path());
        assert!(matches!(m.status(&ModelChoice::Custom("org/m".into())), ModelStatus::Unsupported { .. }));
        assert!(m.open(&ModelChoice::Custom("org/m".into())).is_err());
    }
}
