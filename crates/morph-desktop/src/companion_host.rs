//! What the Companion panel runs tasks on: a worker thread with its own small runtime, the
//! approval hand-off between the agent and the card the user answers, and the plain messages
//! the panel polls. No GPUI here, so it tests without a window.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use morph_ai::{ModelProvider, Roster};
use morph_companion::{Canceller, SessionError, Sessions, ViewUpdate};
use weirdnet_agent::{AgentId, ApprovalAnswer, ApprovalPrompt, Approver, Outcome, SharedPolicy, ToolRegistry, ToolSpec};

/// What the worker tells the panel.
#[derive(Debug)]
pub enum Msg {
    Update(AgentId, ViewUpdate),
    Done(AgentId, Result<Box<Outcome>, SessionError>),
}

enum Cmd {
    Run { roster: Roster, companion: AgentId, input: String },
}

type Slot = Arc<Mutex<Option<tokio::sync::oneshot::Sender<ApprovalAnswer>>>>;

/// Asks the user through the panel: `decide` waits until [`Worker::answer`] is called.
struct PanelApprover {
    slot: Slot,
}

#[async_trait]
impl Approver for PanelApprover {
    async fn decide(&self, _prompt: &ApprovalPrompt, _tool: &ToolSpec) -> ApprovalAnswer {
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.slot.lock().unwrap() = Some(tx);
        rx.await.unwrap_or(ApprovalAnswer::Deny)
    }
}

pub struct Worker {
    cmd: Sender<Cmd>,
    msgs: Receiver<Msg>,
    canceller: Canceller,
    approval: Slot,
}

impl Worker {
    pub fn start(models: Arc<dyn ModelProvider>, policy: SharedPolicy) -> Self {
        let (cmd, cmd_rx) = mpsc::channel::<Cmd>();
        let (msg_tx, msgs) = mpsc::channel::<Msg>();
        let (can_tx, can_rx) = mpsc::channel();
        let approval: Slot = Arc::default();
        let approver = Arc::new(PanelApprover { slot: approval.clone() });
        std::thread::Builder::new()
            .name("morph-companions".into())
            .spawn(move || {
                // Tools arrive with the browser and research hosts; until then the registry is empty.
                let mut sessions = Sessions::new(models, Arc::new(ToolRegistry::new()), policy, approver);
                let _ = can_tx.send(sessions.canceller());
                let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_time().build() else {
                    return;
                };
                while let Ok(Cmd::Run { roster, companion, input }) = cmd_rx.recv() {
                    let tx = msg_tx.clone();
                    let id = companion.clone();
                    let result = rt.block_on(sessions.run(&roster, &companion, &input, |u| {
                        let _ = tx.send(Msg::Update(id.clone(), u));
                    }));
                    let _ = msg_tx.send(Msg::Done(companion, result.map(Box::new)));
                }
            })
            .expect("spawn the companion worker");
        let canceller = can_rx.recv().expect("the worker reports its canceller");
        Self { cmd, msgs, canceller, approval }
    }

    pub fn run(&self, roster: Roster, companion: AgentId, input: String) {
        let _ = self.cmd.send(Cmd::Run { roster, companion, input });
    }

    pub fn cancel(&self, companion: &AgentId) -> bool {
        // A card waiting for an answer is declined so the task can end.
        self.answer(ApprovalAnswer::Deny);
        self.canceller.cancel(companion)
    }

    /// The user's answer to the waiting card. Returns whether one was waiting.
    pub fn answer(&self, answer: ApprovalAnswer) -> bool {
        match self.approval.lock().unwrap().take() {
            Some(tx) => tx.send(answer).is_ok(),
            None => false,
        }
    }

    /// Everything the worker has sent since the last call.
    pub fn poll(&self) -> Vec<Msg> {
        self.msgs.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use morph_ai::{ModelChoice, UnavailableModels};
    use weirdnet_agent::Policy;

    use super::*;

    fn wait_for(worker: &Worker, want: impl Fn(&[Msg]) -> bool) -> Vec<Msg> {
        let mut got = Vec::new();
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            got.extend(worker.poll());
            if want(&got) {
                return got;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    }

    #[test]
    fn a_task_without_a_model_reports_why_and_the_worker_stays_usable() {
        let worker = Worker::start(Arc::new(UnavailableModels::default()), Policy::new().shared());
        let roster = Roster::first_run(ModelChoice::Gemma4E4B);
        for _ in 0..2 {
            worker.run(roster.clone(), AgentId::new("local"), "hi".into());
            let got = wait_for(&worker, |m| m.iter().any(|x| matches!(x, Msg::Done(..))));
            let done = got.iter().find_map(|m| if let Msg::Done(_, r) = m { Some(r) } else { None }).expect("done");
            let err = done.as_ref().expect_err("no model");
            assert!(err.to_string().contains("no local model is set up yet"), "{err}");
        }
    }

    #[test]
    fn answering_with_nothing_waiting_is_a_no_op_and_cancel_of_idle_is_false() {
        let worker = Worker::start(Arc::new(UnavailableModels::default()), Policy::new().shared());
        assert!(!worker.answer(ApprovalAnswer::Deny));
        assert!(!worker.cancel(&AgentId::new("local")));
    }

    #[test]
    fn an_unknown_companion_is_an_error_not_a_hang() {
        let worker = Worker::start(Arc::new(UnavailableModels::default()), Policy::new().shared());
        let roster = Roster::first_run(ModelChoice::Gemma4E4B);
        worker.run(roster, AgentId::new("nobody"), "x".into());
        let got = wait_for(&worker, |m| m.iter().any(|x| matches!(x, Msg::Done(..))));
        assert!(matches!(got.last(), Some(Msg::Done(_, Err(SessionError::UnknownCompanion(_))))));
    }
}
