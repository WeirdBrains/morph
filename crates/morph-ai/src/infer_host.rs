//! This device's local model, served to the user's other devices (`remote::infer`, infer/1 and
//! infer/2): the production [`InferHost`].
//!
//! Who may call is decided before this host sees a request: the app wraps it in a roster gate
//! (`morph-services` `RosterHost`: linked, non-revoked devices only, nobody while the profile
//! is locked). This host decides whether the *device* serves: the "Serve my models to my other
//! devices" setting (live, so turning it off refuses at once), the local task first, and
//! whether the model can run here at all. Every refusal is a clean [`RemoteError`] that makes
//! the asking device try another node or run the turn itself.

use std::sync::Arc;

use weirdnet_agent::remote::infer::InferHost;
use weirdnet_agent::{Caller, ModelBackend, ModelError, RemoteError};

use crate::model::{ModelChoice, ModelProvider, Platform};
use crate::roster::Roster;

type Flag = Arc<dyn Fn() -> bool + Send + Sync>;

/// The model this device serves: the default companion's when it runs a model here, else the
/// first local companion's, else the platform default. The one Settings > Agents shows.
pub fn serving_choice(roster: &Roster) -> ModelChoice {
    let local = |c: &crate::roster::CompanionProfile| c.identity.kind == weirdnet_agent::AgentKind::Local;
    roster
        .default_companion()
        .filter(|c| local(c))
        .or_else(|| roster.companions().iter().find(|c| local(c)))
        .and_then(|c| c.model.clone())
        .unwrap_or_else(|| ModelChoice::default_for(Platform::Desktop))
}

/// Serves `models`' model for `choice` to linked devices while `enabled`.
pub struct LocalInferHost {
    models: Arc<dyn ModelProvider>,
    choice: Arc<dyn Fn() -> ModelChoice + Send + Sync>,
    enabled: Flag,
    local_busy: Flag,
}

impl LocalInferHost {
    /// `enabled` is read at every request (the setting); `choice` is the model to serve.
    pub fn new(
        models: Arc<dyn ModelProvider>,
        choice: impl Fn() -> ModelChoice + Send + Sync + 'static,
        enabled: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self { models, choice: Arc::new(choice), enabled: Arc::new(enabled), local_busy: Arc::new(|| false) }
    }

    /// While `busy` says a task of this device is running, remote turns are turned away (the
    /// asker moves on) so serving never makes the user wait for their own model.
    pub fn with_local_priority(mut self, busy: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.local_busy = Arc::new(busy);
        self
    }

    fn serving(&self) -> Result<(), RemoteError> {
        if (self.enabled)() { Ok(()) } else { Err(RemoteError::refused("this device is not serving its models")) }
    }
}

fn model_error(e: ModelError) -> RemoteError {
    match e {
        ModelError::Unavailable(message) => RemoteError::Unavailable { message },
        ModelError::Failed(message) => RemoteError::Failed { message },
        other => RemoteError::Failed { message: other.to_string() },
    }
}

impl InferHost for LocalInferHost {
    fn authorize(&self, _caller: &Caller) -> Result<(), RemoteError> {
        self.serving()
    }

    fn open(&self, _caller: &Caller, model: Option<&str>) -> Result<Arc<dyn ModelBackend>, RemoteError> {
        self.serving()?;
        let choice = (self.choice)();
        // A request names a model by id or label; a device serves the one Settings selects.
        if let Some(asked) = model.filter(|m| !m.is_empty())
            && !asked.eq_ignore_ascii_case(&choice.label())
            && !matches!(&choice, ModelChoice::Custom(id) if id == asked)
        {
            return Err(RemoteError::unavailable(format!("this device serves {}, not {asked}", choice.label())));
        }
        if (self.local_busy)() {
            return Err(RemoteError::Busy);
        }
        // The provider refuses cleanly when this CPU cannot run the model or it is not downloaded.
        self.models.open(&choice).map_err(model_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use weirdnet_agent::ModelTurn;
    use weirdnet_agent::testing::ScriptedModel;

    struct Provider {
        result: Mutex<Result<(), ModelError>>,
        opened: Mutex<Vec<ModelChoice>>,
    }

    impl Provider {
        fn new(result: Result<(), ModelError>) -> Arc<Self> {
            Arc::new(Self { result: Mutex::new(result), opened: Mutex::default() })
        }
    }

    impl ModelProvider for Provider {
        fn open(&self, choice: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError> {
            self.opened.lock().unwrap().push(choice.clone());
            match &*self.result.lock().unwrap() {
                Ok(()) => Ok(Arc::new(ScriptedModel::new(vec![ModelTurn { text: "served".into(), tool_calls: vec![] }]))),
                Err(ModelError::Unavailable(m)) => Err(ModelError::Unavailable(m.clone())),
                Err(e) => Err(ModelError::Failed(e.to_string())),
            }
        }
    }

    fn caller() -> Caller {
        Caller::new("12D3KooWPhone")
    }

    fn host(p: &Arc<Provider>, on: &Arc<AtomicBool>) -> LocalInferHost {
        let on = on.clone();
        LocalInferHost::new(p.clone(), || ModelChoice::Gemma4E2B, move || on.load(Ordering::SeqCst))
    }

    #[test]
    fn it_serves_the_selected_model_while_the_setting_is_on() {
        let (p, on) = (Provider::new(Ok(())), Arc::new(AtomicBool::new(true)));
        let h = host(&p, &on);
        assert!(h.authorize(&caller()).is_ok());
        assert!(h.open(&caller(), None).is_ok());
        assert!(h.open(&caller(), Some("Gemma 4 E2B")).is_ok(), "by label");
        assert_eq!(p.opened.lock().unwrap().as_slice(), [ModelChoice::Gemma4E2B, ModelChoice::Gemma4E2B]);
        let err = h.open(&caller(), Some("some/other-model")).err().unwrap();
        assert!(matches!(err, RemoteError::Unavailable { ref message } if message.contains("Gemma 4 E2B")), "{err:?}");
    }

    #[test]
    fn it_refuses_at_once_when_the_setting_is_off() {
        let (p, on) = (Provider::new(Ok(())), Arc::new(AtomicBool::new(false)));
        let h = host(&p, &on);
        assert!(matches!(h.authorize(&caller()), Err(RemoteError::Refused { .. })));
        assert!(matches!(h.open(&caller(), None), Err(RemoteError::Refused { .. })));
        assert!(p.opened.lock().unwrap().is_empty(), "the model is not even loaded");
        on.store(true, Ordering::SeqCst);
        assert!(h.open(&caller(), None).is_ok());
        on.store(false, Ordering::SeqCst);
        assert!(h.open(&caller(), None).is_err(), "turned off again: refused again");
    }

    #[test]
    fn a_model_that_cannot_run_here_is_refused_cleanly() {
        let on = Arc::new(AtomicBool::new(true));
        let missing = Provider::new(Err(ModelError::Unavailable("Gemma 4 E2B is not downloaded yet".into())));
        let err = host(&missing, &on).open(&caller(), None).err().unwrap();
        assert!(matches!(err, RemoteError::Unavailable { ref message } if message.contains("not downloaded")), "{err:?}");
        let cpu = Provider::new(Err(ModelError::Unavailable("This CPU lacks AVX2/FMA needed for local models".into())));
        let err = host(&cpu, &on).open(&caller(), None).err().unwrap();
        assert!(matches!(err, RemoteError::Unavailable { ref message } if message.contains("AVX2")), "{err:?}");
    }

    #[test]
    fn this_devices_own_task_goes_first() {
        let (p, on, busy) = (Provider::new(Ok(())), Arc::new(AtomicBool::new(true)), Arc::new(AtomicBool::new(true)));
        let b = busy.clone();
        let h = host(&p, &on).with_local_priority(move || b.load(Ordering::SeqCst));
        assert!(matches!(h.open(&caller(), None), Err(RemoteError::Busy)));
        busy.store(false, Ordering::SeqCst);
        assert!(h.open(&caller(), None).is_ok());
    }

    #[test]
    fn the_served_model_is_the_default_local_companions() {
        use crate::roster::CompanionProfile;
        let first = Roster::first_run(ModelChoice::Gemma4E4B);
        assert_eq!(serving_choice(&first), ModelChoice::Gemma4E4B);
        let mut r = first;
        r.set_model(&weirdnet_agent::AgentId::new("local"), ModelChoice::Gemma4E2B).unwrap();
        assert_eq!(serving_choice(&r), ModelChoice::Gemma4E2B);
        let _ = CompanionProfile::local("x", "X", ModelChoice::Gemma4E4B);
        assert_eq!(serving_choice(&Roster::default()), ModelChoice::Gemma4E4B, "no companions: the platform default");
    }
}
