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

    /// The copy that syncs: the answer clipped to [`MAX_SYNCED_ANSWER`] bytes (see [`clip_markdown`]).
    pub fn for_sync(&self) -> ChatTurn {
        let mut t = self.clone();
        if t.answer.len() > MAX_SYNCED_ANSWER {
            t.answer = clip_markdown(&t.answer, MAX_SYNCED_ANSWER);
        }
        t
    }
}

/// The note a clipped answer ends with.
pub const CLIP_NOTE: &str = "*(clipped on this device; the full answer is on the device that ran it)*";

/// `text` cut to at most `keep` bytes on a char boundary, then the clip note. A code fence the
/// cut leaves open is closed first, so the note is not shown as code. Deterministic: the same
/// text and `keep` always give the same result (synced copies are recognised by it).
pub fn clip_markdown(text: &str, keep: usize) -> String {
    let mut cut = keep.min(text.len());
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = text[..cut].to_string();
    if let Some((ch, len)) = open_fence(&out) {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.extend(std::iter::repeat_n(ch, len));
    }
    out.push_str("\n\n");
    out.push_str(CLIP_NOTE);
    out
}

/// The fence (its character and length) still open at the end of `md`, if any. CommonMark
/// fences: up to three spaces, then three or more backticks or tildes; a backtick fence's info
/// string has no backticks; the closing fence uses the same character, is at least as long and
/// has nothing after it but spaces. Fences inside block quotes or list items are not tracked.
fn open_fence(md: &str) -> Option<(char, usize)> {
    let mut open: Option<(char, usize)> = None;
    for line in md.split('\n') {
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent > 3 {
            continue;
        }
        let rest = &line[indent..];
        let Some(ch) = rest.chars().next().filter(|c| *c == '`' || *c == '~') else { continue };
        let run = rest.len() - rest.trim_start_matches(ch).len();
        if run < 3 {
            continue;
        }
        let after = &rest[run..];
        match open {
            Some((c, n)) if c == ch && run >= n && after.trim().is_empty() => open = None,
            Some(_) => {}
            None if ch == '`' && after.contains('`') => {}
            None => open = Some((ch, run)),
        }
    }
    open
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
        assert!(s.answer.ends_with(CLIP_NOTE));
        let short = ChatTurn::new(AgentId::new("a"), "q", "short", 1);
        assert_eq!(short.for_sync(), short);
    }

    #[test]
    fn a_clip_inside_a_code_fence_closes_it_before_the_note() {
        let text = format!("Intro\n\n```rust\n{}\n```\n\nAfter", "let x = 1;\n".repeat(50));
        let c = clip_markdown(&text, 40);
        assert!(c.starts_with("Intro\n\n```rust\nlet x"));
        assert!(c.ends_with(&format!("\n```\n\n{CLIP_NOTE}")), "{c}");
        assert_eq!(open_fence(&c), None, "the note is outside the code block");
        // A longer tilde fence is closed with the same fence.
        let c = clip_markdown("~~~~\ncode ```\nmore", 12);
        assert!(c.ends_with(&format!("\n~~~~\n\n{CLIP_NOTE}")), "{c}");
        // Cut through the opening fence line itself: still closed.
        let c = clip_markdown("text\n```python\nprint()\n```", 9);
        assert_eq!(&c[..9], "text\n```p");
        assert!(c.ends_with(&format!("```p\n```\n\n{CLIP_NOTE}")), "{c}");
        // Cut through the closing fence line: "``" does not close, so it is closed again.
        let c = clip_markdown("```\ncode\n```\nafter", 11);
        assert_eq!(open_fence(&c), None, "{c}");
        assert!(c.contains("code\n``\n```\n\n"), "{c}");
    }

    #[test]
    fn a_clip_after_a_closed_fence_adds_no_fence() {
        let text = "```\ncode\n```\nplain text that goes on and on";
        let c = clip_markdown(text, 20);
        assert_eq!(c, format!("{}\n\n{CLIP_NOTE}", &text[..20]));
        // Not fences: inline code, two backticks, a four-space indent, a backtick info string.
        for t in ["use `x` here and more", "``\nnot code here", "    ```\nindented", "```a`b\nnot a fence"] {
            assert_eq!(clip_markdown(t, 12), format!("{}\n\n{CLIP_NOTE}", &t[..12]), "{t}");
        }
    }

    #[test]
    fn clips_respect_utf8_and_are_deterministic() {
        let text = format!("```\n{}", "é🦀".repeat(100));
        for keep in 0..40 {
            let c = clip_markdown(&text, keep);
            assert_eq!(c, clip_markdown(&text, keep));
            let body = c.strip_suffix(&format!("\n\n{CLIP_NOTE}")).unwrap();
            let body = body.strip_suffix("\n```").unwrap_or(body);
            assert!(body.len() <= keep && text.starts_with(body), "keep {keep}: {body:?}");
        }
        let long = ChatTurn::new(AgentId::new("a"), "q", format!("```\n{}", "🦀".repeat(MAX_SYNCED_ANSWER)), 1);
        let s = long.for_sync();
        assert!(s.answer.ends_with(&format!("🦀\n```\n\n{CLIP_NOTE}")));
        assert_eq!(s, long.for_sync());
    }
}
