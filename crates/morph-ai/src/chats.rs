//! Saved companion chats: each finished task's question and answer. They sync across the
//! user's devices (`morph-services` `companion_sync`), so a chat started on the phone is there on
//! the desktop.

use serde::{Deserialize, Serialize};
use weirdnet_agent::AgentId;

/// Answers longer than this are clipped in the synced copy (a sync op must fit one page).
pub const MAX_SYNCED_ANSWER: usize = 48 * 1024;

/// One question and its answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    pub id: String,
    pub companion: AgentId,
    pub question: String,
    /// The answer as Markdown.
    pub answer: String,
    /// Cited URLs, in footnote order.
    #[serde(default)]
    pub sources: Vec<String>,
    pub at_ms: u64,
    /// Device the task ran on (peer id), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

impl ChatTurn {
    pub fn new(companion: AgentId, question: impl Into<String>, answer: impl Into<String>, at_ms: u64) -> Self {
        Self { id: crate::ids::new_id("chat"), companion, question: question.into(), answer: answer.into(), sources: Vec::new(), at_ms, device: None }
    }

    /// The copy that syncs: the answer clipped to [`MAX_SYNCED_ANSWER`] bytes on a char boundary.
    pub fn for_sync(&self) -> ChatTurn {
        let mut t = self.clone();
        if t.answer.len() > MAX_SYNCED_ANSWER {
            let mut cut = MAX_SYNCED_ANSWER;
            while !t.answer.is_char_boundary(cut) {
                cut -= 1;
            }
            t.answer.truncate(cut);
            t.answer.push_str("\n\n*(clipped on this device; the full answer is on the device that ran it)*");
        }
        t
    }
}

/// Every saved turn, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChatLog {
    #[serde(default)]
    pub turns: Vec<ChatTurn>,
}

impl ChatLog {
    pub fn push(&mut self, turn: ChatTurn) {
        self.turns.retain(|t| t.id != turn.id);
        self.turns.push(turn);
        self.sort();
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.turns.len();
        self.turns.retain(|t| t.id != id);
        self.turns.len() != before
    }

    /// Clears one companion's chat.
    pub fn clear_companion(&mut self, companion: &AgentId) {
        self.turns.retain(|t| &t.companion != companion);
    }

    pub fn for_companion<'a>(&'a self, companion: &'a AgentId) -> impl Iterator<Item = &'a ChatTurn> {
        self.turns.iter().filter(move |t| &t.companion == companion)
    }

    pub fn sort(&mut self) {
        self.turns.sort_by(|a, b| a.at_ms.cmp(&b.at_ms).then_with(|| a.id.cmp(&b.id)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_stay_in_time_order_and_per_companion() {
        let (a, b) = (AgentId::new("atlas"), AgentId::new("local"));
        let mut log = ChatLog::default();
        log.push(ChatTurn::new(a.clone(), "q2", "a2", 20));
        log.push(ChatTurn::new(b.clone(), "q", "a", 15));
        log.push(ChatTurn::new(a.clone(), "q1", "a1", 10));
        assert_eq!(log.for_companion(&a).map(|t| t.question.as_str()).collect::<Vec<_>>(), ["q1", "q2"]);
        let id = log.turns[0].id.clone();
        assert!(log.remove(&id));
        assert!(!log.remove(&id));
        log.clear_companion(&a);
        assert_eq!(log.turns.len(), 1);
    }

    #[test]
    fn long_answers_are_clipped_for_sync_on_a_char_boundary() {
        let t = ChatTurn::new(AgentId::new("a"), "q", "é".repeat(MAX_SYNCED_ANSWER), 1);
        let s = t.for_sync();
        assert!(s.answer.len() < t.answer.len());
        assert!(s.answer.ends_with("ran it)*"));
        let short = ChatTurn::new(AgentId::new("a"), "q", "short", 1);
        assert_eq!(short.for_sync(), short);
    }
}
