//! One conversation per companion, run from the panel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use morph_ai::{ModelChoice, ModelProvider, Roster};
use weirdnet_agent::{AgentConfig, AgentId, AgentSession, Approver, CancelToken, Outcome, SharedPolicy, ToolRegistry};

use crate::view::{ViewUpdate, translate};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("no companion {0}")]
    UnknownCompanion(String),
    #[error("{0} has no model to run")]
    NoModel(String),
    #[error("could not open the model: {0}")]
    Model(String),
    #[error("{0} is already working on a task")]
    Busy(String),
}

/// The sessions of every companion. Sessions open lazily and keep their conversation, so
/// switching companions in the panel and back picks up where each left off.
pub struct Sessions {
    models: Arc<dyn ModelProvider>,
    tools: Arc<ToolRegistry>,
    policy: SharedPolicy,
    approver: Arc<dyn Approver>,
    open: HashMap<AgentId, AgentSession>,
    running: Running,
}

type Running = Arc<Mutex<HashMap<AgentId, CancelToken>>>;

/// Stops tasks from another thread while `Sessions::run` holds the sessions.
#[derive(Clone)]
pub struct Canceller(Running);

impl Canceller {
    /// Stops a companion's running task. Returns whether one was running.
    pub fn cancel(&self, companion: &AgentId) -> bool {
        match self.0.lock().unwrap().get(companion) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

impl Sessions {
    pub fn new(models: Arc<dyn ModelProvider>, tools: Arc<ToolRegistry>, policy: SharedPolicy, approver: Arc<dyn Approver>) -> Self {
        Self { models, tools, policy, approver, open: HashMap::new(), running: Running::default() }
    }

    /// Runs one task for a companion, handing each view update to `on_update` as it happens.
    pub async fn run(
        &mut self,
        roster: &Roster,
        companion: &AgentId,
        input: &str,
        mut on_update: impl FnMut(ViewUpdate),
    ) -> Result<Outcome, SessionError> {
        let profile = roster.get(companion).ok_or_else(|| SessionError::UnknownCompanion(companion.to_string()))?;
        let name = profile.name().to_string();
        if !self.open.contains_key(companion) {
            let choice: &ModelChoice = profile.model.as_ref().ok_or_else(|| SessionError::NoModel(name.clone()))?;
            let model = self.models.open(choice).map_err(|e| SessionError::Model(e.to_string()))?;
            let session =
                AgentSession::new(AgentConfig::new(profile.identity.clone()), model, self.tools.clone(), self.policy.clone(), self.approver.clone());
            self.open.insert(companion.clone(), session);
        }
        let cancel = {
            let mut running = self.running.lock().unwrap();
            if running.contains_key(companion) {
                return Err(SessionError::Busy(name));
            }
            let c = CancelToken::new();
            running.insert(companion.clone(), c.clone());
            c
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let session = self.open.get_mut(companion).expect("opened above");
        let work = session.run(input, Some(tx), cancel);
        let drain = async {
            while let Some(event) = rx.recv().await {
                for update in translate(&name, &event) {
                    on_update(update);
                }
            }
        };
        let (outcome, ()) = tokio::join!(work, drain);
        self.running.lock().unwrap().remove(companion);
        Ok(outcome)
    }

    /// A handle that stops tasks from another thread.
    pub fn canceller(&self) -> Canceller {
        Canceller(self.running.clone())
    }

    /// Stops a companion's running task. Returns whether one was running.
    pub fn cancel(&self, companion: &AgentId) -> bool {
        self.canceller().cancel(companion)
    }

    pub fn is_running(&self, companion: &AgentId) -> bool {
        self.running.lock().unwrap().contains_key(companion)
    }

    /// Forgets a companion's conversation (when it is removed or the user clears it).
    pub fn close(&mut self, companion: &AgentId) {
        self.open.remove(companion);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weirdnet_agent::testing::{FnTool, ScriptedApprover, ScriptedModel, call};
    use weirdnet_agent::{ApprovalAnswer, LocalTools, ModelBackend, ModelError, ModelTurn, Policy, Scope, StopReason, Tier, ToolOutput, ToolSpec};

    struct Provider(Mutex<Vec<ModelTurn>>);
    impl ModelProvider for Provider {
        fn open(&self, _: &ModelChoice) -> Result<Arc<dyn ModelBackend>, ModelError> {
            Ok(Arc::new(ScriptedModel::new(std::mem::take(&mut *self.0.lock().unwrap()))))
        }
    }

    fn turn(text: &str, calls: Vec<weirdnet_agent::ToolCall>) -> ModelTurn {
        ModelTurn { text: text.into(), tool_calls: calls }
    }

    async fn registry() -> Arc<ToolRegistry> {
        let spec = ToolSpec::new("open_url", "Open a URL").tier(Tier::Navigate);
        let tool = FnTool::new(spec, |_| Ok(ToolOutput::text("opened"))).with_target(|_| weirdnet_agent::CallTarget::url("https://tokio.rs/x"));
        let mut r = ToolRegistry::new();
        r.add_source(Arc::new(LocalTools::new("browser").with(tool)));
        r.refresh().await.unwrap();
        Arc::new(r)
    }

    #[tokio::test]
    async fn runs_a_task_and_streams_card_text_and_finish() {
        let roster = Roster::first_run(ModelChoice::Gemma4E4B);
        let provider = Provider(Mutex::new(vec![turn("", vec![call("c1", "open_url", serde_json::json!({}))]), turn("Done.", vec![])]));
        let approver = Arc::new(ScriptedApprover::new(vec![ApprovalAnswer::Approve { scope: Scope::Once }]));
        let mut sessions = Sessions::new(Arc::new(provider), registry().await, Policy::new().shared(), approver);
        let mut updates = Vec::new();
        let id = AgentId::new("local");
        let outcome = sessions.run(&roster, &id, "open tokio", |u| updates.push(u)).await.unwrap();
        assert_eq!(outcome.stop, StopReason::Completed);
        assert_eq!(outcome.text, "Done.");
        assert!(matches!(updates.first(), Some(ViewUpdate::TaskStarted { .. })));
        assert!(updates.iter().any(|u| matches!(u, ViewUpdate::Approval(c) if c.title.contains("Local wants to open"))));
        assert!(updates.iter().any(|u| matches!(u, ViewUpdate::ToolDone { is_error: false, .. })));
        assert!(matches!(updates.last(), Some(ViewUpdate::Finished { status, .. }) if status == "done"));
        assert!(!sessions.is_running(&id));
        assert!(!sessions.cancel(&id));
    }

    #[tokio::test]
    async fn unknown_companion_and_missing_model_are_errors() {
        let mut roster = Roster::first_run(ModelChoice::Gemma4E4B);
        roster
            .add(morph_ai::CompanionProfile {
                identity: weirdnet_agent::AgentIdentity::new("ext", "Ext", weirdnet_agent::AgentKind::External),
                model: None,
            })
            .unwrap();
        let approver = Arc::new(ScriptedApprover::new(vec![]));
        let mut sessions = Sessions::new(Arc::new(Provider(Mutex::new(vec![]))), registry().await, Policy::new().shared(), approver);
        let e = sessions.run(&roster, &AgentId::new("nope"), "x", |_| {}).await.unwrap_err();
        assert_eq!(e, SessionError::UnknownCompanion("nope".into()));
        let e = sessions.run(&roster, &AgentId::new("ext"), "x", |_| {}).await.unwrap_err();
        assert_eq!(e, SessionError::NoModel("Ext".into()));
    }

    #[tokio::test]
    async fn conversation_persists_between_tasks_until_closed() {
        let roster = Roster::first_run(ModelChoice::Gemma4E4B);
        let provider = Provider(Mutex::new(vec![turn("one", vec![]), turn("two", vec![])]));
        let approver = Arc::new(ScriptedApprover::new(vec![]));
        let mut sessions = Sessions::new(Arc::new(provider), registry().await, Policy::new().shared(), approver);
        let id = AgentId::new("local");
        assert_eq!(sessions.run(&roster, &id, "a", |_| {}).await.unwrap().text, "one");
        assert_eq!(sessions.run(&roster, &id, "b", |_| {}).await.unwrap().text, "two");
        sessions.close(&id);
    }
}
