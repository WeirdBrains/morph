//! Steering a long research project (design: "Steering a long project", slices 4a to 4c).
//!
//! Every steer is a record. It waits in `pending` and is applied at a step boundary (never in
//! the middle of a step), then shows in the steering history. A change that touches the plan
//! makes a [`Replan`] with a diff and a reason that cites the steers; the user can undo it for
//! as long as the plan has not moved on. History is never rewritten: an undo is a new entry.
//! Notes, page text and sources stay untrusted data: only the user's own steers get here.
//!
//! This is Morph-local; like the rest of `research`, it may move to `weirdnet-agent` later.

use serde::{Deserialize, Serialize};

use crate::ids::new_id;
use crate::research::{PlanStep, ProjectState, ResearchProject, StepStatus, title_of};

const DAY_MS: u64 = 24 * 3_600_000;

/// How the user marks a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRule {
    /// Read it first.
    Pin,
    /// Prefer it over others.
    Prefer,
    /// Never use or cite it.
    Exclude,
}

impl SourceRule {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pin => "pinned",
            Self::Prefer => "preferred",
            Self::Exclude => "excluded",
        }
    }
}

/// A source rule for a site (`tokio.rs`) or one page (`https://tokio.rs/tokio/tutorial`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEntry {
    pub pattern: String,
    pub rule: SourceRule,
}

/// Feedback on a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Useful,
    /// Do not rely on it; the project checks the step again.
    Wrong,
    /// Go further into it; the project adds a step.
    DigDeeper,
}

/// What the user can change while a project runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Steer {
    /// Change the question or its scope. The steps still queued are replaced.
    Brief {
        text: String,
    },
    /// Add a sub-question as a step (before the write-up).
    AddStep {
        text: String,
    },
    /// Remove a queued step (by plan index); a step that ran or runs cannot be removed.
    RemoveStep {
        index: usize,
    },
    Note {
        text: String,
    },
    Source {
        pattern: String,
        rule: SourceRule,
    },
    ForgetSource {
        pattern: String,
    },
    Feedback {
        finding: usize,
        verdict: Verdict,
    },
    /// The answer to a direction check-in (one of its options, or the user's own words).
    CheckIn {
        id: String,
        choice: String,
    },
}

impl Steer {
    /// One line for the history.
    pub fn describe(&self) -> String {
        match self {
            Self::Brief { text } => format!("question changed to: {}", clip(text, 120)),
            Self::AddStep { text } => format!("sub-question added: {}", clip(text, 120)),
            Self::RemoveStep { index } => format!("sub-question {} removed", index + 1),
            Self::Note { text } => format!("note: {}", clip(text, 120)),
            Self::Source { pattern, rule } => format!("source {}: {pattern}", rule.label()),
            Self::ForgetSource { pattern } => format!("source rule dropped: {pattern}"),
            Self::Feedback { finding, verdict } => format!("finding {} marked {}", finding + 1, format!("{verdict:?}").to_lowercase()),
            Self::CheckIn { choice, .. } => format!("check-in answered: {}", clip(choice, 120)),
        }
    }
}

/// A steer with its author and time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteerRecord {
    pub id: String,
    pub at_ms: u64,
    /// Which device made it (a name; steering from other devices comes with sync).
    pub device: String,
    pub steer: Steer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

/// A visible change of the plan, with what it was before so it can be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replan {
    pub id: String,
    pub at_ms: u64,
    /// Plan version after this replan (the first plan is version 1).
    pub version: u32,
    pub reason: String,
    pub diff: Vec<DiffLine>,
    pub brief_before: String,
    pub before: Vec<PlanStep>,
    pub after: Vec<PlanStep>,
    #[serde(default)]
    pub undone: bool,
}

/// A direction check-in: findings so far and a few ways to go on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckIn {
    pub id: String,
    pub at_ms: u64,
    pub question: String,
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// No answer came before the next check-in: the project went on with its plan.
    #[serde(default)]
    pub lapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Steering {
    #[serde(default)]
    pub pending: Vec<SteerRecord>,
    #[serde(default)]
    pub sources: Vec<SourceEntry>,
    /// `(finding index, verdict)`; the last verdict for a finding counts.
    #[serde(default)]
    pub verdicts: Vec<(usize, Verdict)>,
    #[serde(default)]
    pub replans: Vec<Replan>,
    #[serde(default)]
    pub checkins: Vec<CheckIn>,
    /// How often the project asks for direction (default once a day).
    #[serde(default = "day")]
    pub checkin_every_ms: u64,
    #[serde(default)]
    pub last_checkin_ms: u64,
    /// "Wait for me": a check-in holds the project (Needs you) until it is answered.
    #[serde(default)]
    pub wait_for_me: bool,
    /// Ids of steers that came from other devices and were already taken in (so a late copy is ignored).
    #[serde(default)]
    pub seen_remote: Vec<String>,
    /// Plan version (see [`Replan::version`]).
    #[serde(default = "one")]
    pub plan_version: u32,
}

fn day() -> u64 {
    DAY_MS
}

fn one() -> u32 {
    1
}

impl Default for Steering {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            sources: Vec::new(),
            verdicts: Vec::new(),
            replans: Vec::new(),
            checkins: Vec::new(),
            checkin_every_ms: DAY_MS,
            last_checkin_ms: 0,
            wait_for_me: false,
            seen_remote: Vec::new(),
            plan_version: 1,
        }
    }
}

fn clip(t: &str, n: usize) -> String {
    let mut c: String = t.chars().take(n).collect();
    if t.chars().count() > n {
        c.push('…');
    }
    c
}

/// The host of a pattern or URL, lower case, without `www.`.
pub fn host_of(s: &str) -> String {
    let s = s.trim().to_lowercase();
    let rest = s.split_once("://").map_or(s.as_str(), |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

impl SourceEntry {
    /// Whether this rule is about `url`: the same page, or any page of the site when the rule names a site.
    pub fn matches(&self, url: &str) -> bool {
        let pat = self.pattern.trim().to_lowercase();
        if pat.contains('/') {
            let u = url.trim().to_lowercase();
            u.trim_end_matches('/') == pat.trim_end_matches('/') || u.trim_end_matches('/').starts_with(&format!("{}/", pat.trim_end_matches('/')))
        } else {
            let h = host_of(url);
            let p = host_of(&pat);
            h == p || h.ends_with(&format!(".{p}"))
        }
    }
}

/// Whether `url` is covered by any of `patterns` (sites or pages, as a [`SourceEntry`] matches).
pub fn matches_any(patterns: &[String], url: &str) -> bool {
    patterns.iter().any(|p| SourceEntry { pattern: p.clone(), rule: SourceRule::Exclude }.matches(url))
}

impl Steering {
    /// Whether the user excluded this URL.
    pub fn excluded(&self, url: &str) -> bool {
        self.sources.iter().any(|s| s.rule == SourceRule::Exclude && s.matches(url))
    }

    pub fn verdict_of(&self, finding: usize) -> Option<Verdict> {
        self.verdicts.iter().rev().find(|(i, _)| *i == finding).map(|(_, v)| *v)
    }

    /// The source rules as lines of a step's prompt (empty without rules).
    pub fn source_prompt(&self) -> String {
        let list = |rule: SourceRule| -> Vec<&str> { self.sources.iter().filter(|s| s.rule == rule).map(|s| s.pattern.as_str()).collect() };
        let mut p = String::new();
        let (pin, prefer, exclude) = (list(SourceRule::Pin), list(SourceRule::Prefer), list(SourceRule::Exclude));
        if !pin.is_empty() {
            p.push_str(&format!("Read these first, with web.read: {}\n", pin.join(", ")));
        }
        if !prefer.is_empty() {
            p.push_str(&format!("Prefer sources from: {}\n", prefer.join(", ")));
        }
        if !exclude.is_empty() {
            p.push_str(&format!("Never read, use or cite: {}\n", exclude.join(", ")));
        }
        p
    }

    /// The patterns of the sources the user excluded, for the web tools to enforce.
    pub fn excluded_patterns(&self) -> Vec<String> {
        self.sources.iter().filter(|s| s.rule == SourceRule::Exclude).map(|s| s.pattern.clone()).collect()
    }

    /// The check-in that waits for an answer.
    pub fn open_checkin(&self) -> Option<&CheckIn> {
        self.checkins.last().filter(|c| c.answer.is_none() && !c.lapsed)
    }
}

const WRITE_UP: &str = "Write";

impl ResearchProject {
    /// Records a steer. It is applied at the next step boundary (see [`Self::apply_pending`]);
    /// a check-in answer is recorded at once and takes effect then.
    pub fn submit_steer(&mut self, now: u64, device: &str, steer: Steer) -> String {
        if let Steer::CheckIn { id, choice } = &steer
            && let Some(c) = self.steering.checkins.iter_mut().find(|c| c.id == *id && c.answer.is_none())
        {
            c.answer = Some(choice.clone());
        }
        let id = new_id("steer");
        self.log_steer(now, format!("steer ({device}): {}", steer.describe()));
        self.steering.pending.push(SteerRecord { id: id.clone(), at_ms: now, device: device.into(), steer, applied_ms: None });
        id
    }

    pub fn log_steer(&mut self, now: u64, text: impl Into<String>) {
        self.log.push(crate::research::LogEntry { at_ms: now, text: text.into(), steer: true });
    }

    /// Whether a step is running now (steers wait for it to end).
    pub fn step_running(&self) -> bool {
        self.plan.iter().any(|s| s.status == StepStatus::Running)
    }

    /// Applies the waiting steers, oldest first, when no step is running. A change of the plan or
    /// the question makes one [`Replan`] for the whole batch. Returns whether anything was applied.
    pub fn apply_pending(&mut self, now: u64) -> bool {
        if self.step_running() || self.steering.pending.is_empty() {
            return false;
        }
        let pending = std::mem::take(&mut self.steering.pending);
        let (plan_before, brief_before) = (self.plan.clone(), self.brief.clone());
        let mut reasons = Vec::new();
        for mut rec in pending {
            let said = rec.steer.describe();
            if self.apply_one(now, &rec.steer) {
                reasons.push(said.clone());
            }
            self.log_steer(now, format!("applied: {said}"));
            rec.applied_ms = Some(now);
        }
        if self.plan != plan_before || self.brief != brief_before {
            let diff = diff_of(&brief_before, &plan_before, &self.brief, &self.plan);
            self.steering.plan_version += 1;
            let reason = reasons.join("; ");
            self.log_steer(now, format!("replanned (plan v{}): {reason}", self.steering.plan_version));
            self.steering.replans.push(Replan {
                id: new_id("replan"),
                at_ms: now,
                version: self.steering.plan_version,
                reason,
                diff,
                brief_before,
                before: plan_before,
                after: self.plan.clone(),
                undone: false,
            });
        }
        // A check-in that was holding the project is answered.
        if self.state == ProjectState::NeedsYou && self.steering.open_checkin().is_none() {
            self.state = ProjectState::Running;
        }
        true
    }

    /// Applies one steer; whether it changed the plan or the question (for the reason).
    fn apply_one(&mut self, now: u64, steer: &Steer) -> bool {
        match steer {
            Steer::Brief { text } => {
                let text = text.trim();
                if text.is_empty() || text == self.brief {
                    return false;
                }
                self.brief = text.into();
                self.title = title_of(text);
                self.plan.retain(|s| s.status != StepStatus::Queued);
                let first = text.lines().next().unwrap_or(text);
                self.plan.push(PlanStep::new(&format!("Search for: {}", clip(first, 100))));
                self.plan.push(PlanStep::new("Read the best sources"));
                self.plan.push(PlanStep::new("Write the findings and a draft report with footnotes"));
                true
            }
            Steer::AddStep { text } => self.insert_step(text),
            Steer::RemoveStep { index } => match self.plan.get_mut(*index) {
                Some(s) if s.status == StepStatus::Queued => {
                    s.status = StepStatus::Skipped;
                    true
                }
                _ => {
                    self.log_steer(now, "a step that already ran cannot be removed");
                    false
                }
            },
            Steer::Note { text } => {
                if !text.trim().is_empty() {
                    self.notes.push(crate::research::LogEntry { at_ms: now, text: text.trim().into(), steer: true });
                }
                false
            }
            Steer::Source { pattern, rule } => {
                let pattern = pattern.trim().to_string();
                if pattern.is_empty() {
                    return false;
                }
                match self.steering.sources.iter_mut().find(|s| s.pattern.eq_ignore_ascii_case(&pattern)) {
                    Some(s) => s.rule = *rule,
                    None => self.steering.sources.push(SourceEntry { pattern, rule: *rule }),
                }
                false
            }
            Steer::ForgetSource { pattern } => {
                self.steering.sources.retain(|s| !s.pattern.eq_ignore_ascii_case(pattern));
                false
            }
            Steer::Feedback { finding, verdict } => {
                let Some(f) = self.findings.get(*finding) else { return false };
                let step = f.step.clone();
                self.steering.verdicts.push((*finding, *verdict));
                match verdict {
                    Verdict::Useful => false,
                    Verdict::DigDeeper => self.insert_step(&format!("Dig deeper into: {step}")),
                    Verdict::Wrong => self.insert_step(&format!("Check again, the user says it was wrong: {step}")),
                }
            }
            Steer::CheckIn { choice, .. } => {
                let note = match choice.as_str() {
                    "Continue as planned" => return false,
                    "Go broader" => "Go broader: include adjacent topics and related work.".to_string(),
                    "Go narrower" => "Stay narrow: only what the question asks directly.".to_string(),
                    other => other.to_string(),
                };
                self.apply_one(now, &Steer::Note { text: note })
            }
        }
    }

    fn insert_step(&mut self, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        let at = self.plan.iter().rposition(|s| s.status == StepStatus::Queued && s.title.starts_with(WRITE_UP)).unwrap_or(self.plan.len());
        self.plan.insert(at, PlanStep::new(text));
        true
    }

    /// Undoes a replan while the plan is still exactly what it made. Returns whether it did.
    pub fn undo_replan(&mut self, now: u64, replan_id: &str) -> bool {
        if self.step_running() {
            return false;
        }
        let Some(i) = self.steering.replans.iter().position(|r| r.id == replan_id && !r.undone) else { return false };
        if self.steering.replans[i].after != self.plan {
            return false;
        }
        let r = &mut self.steering.replans[i];
        r.undone = true;
        let (before, brief, reason) = (r.before.clone(), r.brief_before.clone(), r.reason.clone());
        self.plan = before;
        self.title = title_of(&brief);
        self.brief = brief;
        self.log_steer(now, format!("undid the replan: {reason}"));
        true
    }

    /// Whether `replan_id` can be undone now.
    pub fn can_undo(&self, replan_id: &str) -> bool {
        !self.step_running() && self.steering.replans.iter().any(|r| r.id == replan_id && !r.undone && r.after == self.plan)
    }

    /// Posts a direction check-in when one is due: the project runs, none waits for an answer,
    /// something was found, and the interval has passed. An unanswered earlier check-in lapses
    /// (the project went on with its plan) and the history says so. Returns whether one was posted.
    pub fn maybe_checkin(&mut self, now: u64) -> bool {
        if self.state != ProjectState::Running || self.findings.is_empty() || self.step_running() {
            return false;
        }
        let since = self.steering.last_checkin_ms.max(self.created_ms);
        if now.saturating_sub(since) < self.steering.checkin_every_ms {
            return false;
        }
        if let Some(c) = self.steering.checkins.last_mut()
            && c.answer.is_none()
            && !c.lapsed
        {
            c.lapsed = true;
            self.log_steer(now, "check-in unanswered: continued on the current plan");
        }
        let n = self.findings.len();
        let question = format!("{n} finding{} so far. Where should I go next?", if n == 1 { "" } else { "s" });
        self.steering.checkins.push(CheckIn {
            id: new_id("checkin"),
            at_ms: now,
            question: question.clone(),
            options: vec!["Continue as planned".into(), "Go broader".into(), "Go narrower".into()],
            answer: None,
            lapsed: false,
        });
        self.steering.last_checkin_ms = now;
        self.log_steer(now, format!("check-in: {question}"));
        if self.steering.wait_for_me {
            self.state = ProjectState::NeedsYou;
        }
        true
    }
}

/// The active steps (not skipped) as titles, for the diff.
fn active(plan: &[PlanStep]) -> Vec<&str> {
    plan.iter().filter(|s| s.status != StepStatus::Skipped).map(|s| s.title.as_str()).collect()
}

fn diff_of(brief_before: &str, before: &[PlanStep], brief_after: &str, after: &[PlanStep]) -> Vec<DiffLine> {
    let mut out = Vec::new();
    if brief_before != brief_after {
        out.push(DiffLine { kind: DiffKind::Changed, text: format!("question: {} → {}", clip(brief_before, 60), clip(brief_after, 60)) });
    }
    let (b, a) = (active(before), active(after));
    for t in &b {
        if !a.contains(t) {
            out.push(DiffLine { kind: DiffKind::Removed, text: (*t).into() });
        }
    }
    for t in &a {
        if !b.contains(t) {
            out.push(DiffLine { kind: DiffKind::Added, text: (*t).into() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::{Finding, starter_plan};

    fn project() -> ResearchProject {
        let mut p = ResearchProject::new("t", "How does Tokio cancel a task?", "atlas", starter_plan(), 1_000);
        p.state = ProjectState::Running;
        p
    }

    fn finding(p: &mut ResearchProject, step: &str) {
        p.findings.push(Finding { step: step.into(), text: "found".into(), sources: vec![], at_ms: 5 });
    }

    #[test]
    fn a_steer_waits_while_a_step_runs_and_lands_at_the_boundary() {
        let mut p = project();
        p.plan[0].status = StepStatus::Running;
        p.submit_steer(10, "my-desktop", Steer::Brief { text: "Async drop proposals only".into() });
        assert!(!p.apply_pending(11), "the running step is not cut");
        assert_eq!(p.brief, "How does Tokio cancel a task?");
        assert_eq!(p.steering.pending.len(), 1);
        p.plan[0].status = StepStatus::Done;
        assert!(p.apply_pending(12));
        assert_eq!(p.brief, "Async drop proposals only");
        assert!(p.steering.pending.is_empty());
        // The finished step stays; the queued ones are replaced by a plan for the new scope.
        assert_eq!(p.plan[0].status, StepStatus::Done);
        assert_eq!(p.plan[1].title, "Search for: Async drop proposals only");
        assert_eq!(p.plan.len(), 4);
        assert_eq!(p.steering.plan_version, 2);
    }

    #[test]
    fn a_scope_change_makes_a_replan_with_a_diff_a_reason_and_an_undo() {
        let mut p = project();
        p.submit_steer(10, "me", Steer::Brief { text: "Async drop proposals".into() });
        p.apply_pending(11);
        let r = p.steering.replans[0].clone();
        assert!(r.reason.contains("question changed to: Async drop proposals"));
        assert!(r.diff.iter().any(|d| d.kind == DiffKind::Changed));
        assert!(r.diff.iter().any(|d| d.kind == DiffKind::Added && d.text.starts_with("Search for: Async drop")));
        assert!(r.diff.iter().any(|d| d.kind == DiffKind::Removed && d.text == "Search the Library and the web for the question"));
        assert!(p.can_undo(&r.id));
        assert!(p.undo_replan(20, &r.id));
        assert_eq!((p.brief.as_str(), p.plan.clone()), ("How does Tokio cancel a task?", starter_plan()));
        assert!(p.log.iter().any(|l| l.steer && l.text.starts_with("undid the replan")), "an undo is a new history entry");
        assert!(p.log.iter().any(|l| l.text.starts_with("replanned")), "nothing was rewritten");
        assert!(!p.undo_replan(21, &r.id), "once");
    }

    #[test]
    fn a_replan_cannot_be_undone_after_the_plan_moved_on() {
        let mut p = project();
        p.submit_steer(10, "me", Steer::AddStep { text: "Compare the proposals".into() });
        p.apply_pending(11);
        let id = p.steering.replans[0].id.clone();
        p.plan[0].status = StepStatus::Done;
        assert!(!p.can_undo(&id) && !p.undo_replan(12, &id));
    }

    #[test]
    fn sub_questions_are_added_before_the_write_up_and_only_queued_ones_removed() {
        let mut p = project();
        p.plan[0].status = StepStatus::Done;
        p.submit_steer(1, "me", Steer::AddStep { text: "What do the RFCs say?".into() });
        p.submit_steer(2, "me", Steer::RemoveStep { index: 1 });
        p.submit_steer(3, "me", Steer::RemoveStep { index: 0 });
        p.apply_pending(4);
        let t: Vec<_> = p.plan.iter().map(|s| (s.title.as_str(), s.status)).collect();
        assert_eq!(t[3].0, "Write the findings and a draft report with footnotes");
        assert_eq!(t[2], ("What do the RFCs say?", StepStatus::Queued));
        assert_eq!(t[1].1, StepStatus::Skipped);
        assert_eq!(t[0].1, StepStatus::Done, "a step that ran stays");
        assert!(p.log.iter().any(|l| l.text.contains("already ran")));
        assert_eq!(p.next_step(), Some(2));
    }

    #[test]
    fn source_rules_reach_the_prompt_and_exclusions_match_a_site_or_a_page() {
        let mut p = project();
        for (pat, rule) in [
            ("tokio.rs", SourceRule::Pin),
            ("docs.rs", SourceRule::Prefer),
            ("https://spam.example/page", SourceRule::Exclude),
            ("Medium.com", SourceRule::Exclude),
        ] {
            p.submit_steer(1, "me", Steer::Source { pattern: pat.into(), rule });
        }
        p.apply_pending(2);
        let prompt = p.step_prompt(0);
        assert!(prompt.contains("Read these first, with web.read: tokio.rs"));
        assert!(prompt.contains("Prefer sources from: docs.rs"));
        assert!(prompt.contains("Never read, use or cite: https://spam.example/page, Medium.com"));
        assert!(p.steering.excluded("https://www.medium.com/@a/b") && p.steering.excluded("https://blog.medium.com/x"));
        assert!(p.steering.excluded("https://spam.example/page/") && p.steering.excluded("https://spam.example/page/2"));
        assert!(!p.steering.excluded("https://spam.example/other") && !p.steering.excluded("https://notmedium.com/"));
        p.submit_steer(3, "me", Steer::Source { pattern: "medium.com".into(), rule: SourceRule::Pin });
        p.submit_steer(3, "me", Steer::ForgetSource { pattern: "docs.rs".into() });
        p.apply_pending(4);
        assert!(!p.steering.excluded("https://medium.com/a"), "a rule for a site is replaced, not added twice");
        assert_eq!(p.steering.sources.len(), 3);
    }

    #[test]
    fn matches_any_covers_sites_and_pages() {
        let pats = vec!["bad.example".to_string(), "https://x.example/private".to_string()];
        assert!(matches_any(&pats, "https://www.bad.example/a") && matches_any(&pats, "https://x.example/private/2"));
        assert!(!matches_any(&pats, "https://x.example/public") && !matches_any(&[], "https://bad.example/"));
        let mut p = project();
        p.submit_steer(1, "me", Steer::Source { pattern: "bad.example".into(), rule: SourceRule::Exclude });
        p.submit_steer(1, "me", Steer::Source { pattern: "good.example".into(), rule: SourceRule::Pin });
        p.apply_pending(2);
        assert_eq!(p.steering.excluded_patterns(), vec!["bad.example"], "pins stay prompt hints");
    }

    #[test]
    fn excluded_sources_are_left_out_of_a_finding() {
        use crate::research::{StepEnd, finish_step};
        let mut p = project();
        p.submit_steer(1, "me", Steer::Source { pattern: "bad.example".into(), rule: SourceRule::Exclude });
        p.apply_pending(2);
        let end =
            StepEnd::Completed { text: "x".into(), sources: vec!["https://bad.example/a".into(), "https://tokio.rs/".into()], pages: 2, tool_ms: 1 };
        finish_step(&mut p, 0, end, 3);
        assert_eq!(p.findings[0].sources, vec!["https://tokio.rs/"]);
    }

    #[test]
    fn feedback_shapes_the_next_steps_and_the_prompt() {
        let mut p = project();
        finding(&mut p, "step one");
        finding(&mut p, "step two");
        p.submit_steer(1, "me", Steer::Feedback { finding: 0, verdict: Verdict::DigDeeper });
        p.submit_steer(1, "me", Steer::Feedback { finding: 1, verdict: Verdict::Wrong });
        p.submit_steer(1, "me", Steer::Feedback { finding: 7, verdict: Verdict::Useful });
        p.apply_pending(2);
        assert!(p.plan.iter().any(|s| s.title == "Dig deeper into: step one"));
        assert!(p.plan.iter().any(|s| s.title.starts_with("Check again") && s.title.ends_with("step two")));
        let text = p.step_prompt(0);
        assert!(text.contains("step two: the user says this was WRONG"));
        assert!(!text.contains("step two: found"), "a wrong finding is not handed on as fact");
        assert_eq!(p.steering.verdict_of(0), Some(Verdict::DigDeeper));
        assert_eq!(p.steering.verdicts.len(), 2, "feedback on a finding that does not exist is dropped");
    }

    #[test]
    fn a_note_applies_at_the_boundary_and_reaches_the_prompt() {
        let mut p = project();
        p.submit_steer(1, "me", Steer::Note { text: "only 2022 and later".into() });
        assert!(!p.step_prompt(0).contains("2022"));
        p.apply_pending(2);
        assert!(p.step_prompt(0).contains("only 2022 and later"));
        assert!(p.steering.replans.is_empty(), "a note is not a replan");
    }

    #[test]
    fn a_check_in_is_due_once_per_interval_and_an_unanswered_one_lapses_and_the_project_goes_on() {
        let mut p = project();
        p.steering.checkin_every_ms = 1_000;
        assert!(!p.maybe_checkin(5_000), "nothing found yet");
        finding(&mut p, "s");
        assert!(!p.maybe_checkin(1_500), "interval not reached");
        assert!(p.maybe_checkin(2_500));
        assert!(!p.maybe_checkin(2_600) && p.state == ProjectState::Running, "it asks, it does not stop");
        assert!(p.steering.open_checkin().is_some());
        assert!(p.maybe_checkin(4_000));
        assert!(p.steering.checkins[0].lapsed && p.steering.checkins.len() == 2);
        assert!(p.log.iter().any(|l| l.steer && l.text.contains("continued on the current plan")));
    }

    #[test]
    fn answering_a_check_in_steers_and_wait_for_me_holds_until_then() {
        let mut p = project();
        p.steering.checkin_every_ms = 1;
        p.steering.wait_for_me = true;
        finding(&mut p, "s");
        assert!(p.maybe_checkin(10_000));
        assert_eq!(p.state, ProjectState::NeedsYou);
        let id = p.steering.open_checkin().unwrap().id.clone();
        p.submit_steer(10_001, "me", Steer::CheckIn { id: id.clone(), choice: "Go broader".into() });
        assert!(p.steering.open_checkin().is_none());
        p.apply_pending(10_002);
        assert_eq!(p.state, ProjectState::Running);
        assert!(p.step_prompt(0).contains("Go broader: include adjacent topics"));
        assert_eq!(p.steering.checkins[0].answer.as_deref(), Some("Go broader"));
    }

    #[test]
    fn steering_survives_a_save_and_old_files_without_it_still_load() {
        let mut p = project();
        p.submit_steer(1, "me", Steer::Brief { text: "x".into() });
        p.apply_pending(2);
        let json = serde_json::to_string(&p).unwrap();
        let back: ResearchProject = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v.as_object_mut().unwrap().remove("steering");
        let old: ResearchProject = serde_json::from_value(v).unwrap();
        assert_eq!(old.steering, Steering::default());
        assert_eq!(old.steering.checkin_every_ms, DAY_MS);
    }
}
