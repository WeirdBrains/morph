//! The living report of a research project (design: "Output", slice 4d): Markdown rebuilt from
//! the findings at each milestone (a step finishes, the project is done), kept as numbered
//! revisions so a change can be seen as a diff and an old revision exported.
//!
//! Saving to Notes or a journal is not wired in the desktop yet (the journal store has no
//! desktop user), and there is no desktop notification channel: a milestone is logged instead.

use serde::{Deserialize, Serialize};

use crate::research::{ProjectState, ResearchProject, StepStatus};
use crate::steer::{DiffKind, DiffLine, Verdict};

/// The most revisions kept; older ones are dropped.
pub const MAX_REVISIONS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRevision {
    pub n: u32,
    pub at_ms: u64,
    /// The milestone that made it.
    pub why: String,
    pub markdown: String,
}

impl ResearchProject {
    /// The report as Markdown now: the question, each finding that is not marked wrong with its
    /// sources (excluded sites left out), and the sources together.
    pub fn report_markdown(&self) -> String {
        let mut md = format!(
            "# {}\n\n*{} · plan v{} · {} of {} steps done*\n\n{}\n",
            self.title,
            self.companion,
            self.steering.plan_version,
            self.plan.iter().filter(|s| s.status == StepStatus::Done).count(),
            self.plan.iter().filter(|s| s.status != StepStatus::Skipped).count(),
            self.brief.trim()
        );
        let mut all: Vec<&str> = Vec::new();
        md.push_str("\n## Findings\n");
        let mut any = false;
        for (i, f) in self.findings.iter().enumerate() {
            if self.steering.verdict_of(i) == Some(Verdict::Wrong) {
                continue;
            }
            any = true;
            md.push_str(&format!("\n### {}\n\n{}\n", f.step, f.text.trim()));
            let srcs: Vec<&String> = f.sources.iter().filter(|u| !self.steering.excluded(u)).collect();
            if !srcs.is_empty() {
                md.push_str("\nSources: ");
                md.push_str(&srcs.iter().map(|u| format!("<{u}>")).collect::<Vec<_>>().join(" "));
                md.push('\n');
            }
            for u in srcs {
                if !all.contains(&u.as_str()) {
                    all.push(u);
                }
            }
        }
        if !any {
            md.push_str("\nNothing found yet.\n");
        }
        if !all.is_empty() {
            md.push_str("\n## All sources\n\n");
            for (n, u) in all.iter().enumerate() {
                md.push_str(&format!("{}. <{u}>\n", n + 1));
            }
        }
        md
    }

    /// A milestone: saves a new revision when the report changed since the last one, and logs
    /// it. Returns the revision number when one was made.
    pub fn milestone(&mut self, now: u64, why: &str) -> Option<u32> {
        let markdown = self.report_markdown();
        if self.reports.last().is_some_and(|r| r.markdown == markdown) {
            return None;
        }
        let n = self.reports.last().map_or(1, |r| r.n + 1);
        self.reports.push(ReportRevision { n, at_ms: now, why: why.into(), markdown });
        if self.reports.len() > MAX_REVISIONS {
            self.reports.remove(0);
        }
        let done = if self.state == ProjectState::Done { " (project done)" } else { "" };
        self.log(now, format!("milestone: report revision {n}: {why}{done}"));
        Some(n)
    }

    /// What changed between revision `a` and `b` (by number): lines removed, then added.
    pub fn report_diff(&self, a: u32, b: u32) -> Vec<DiffLine> {
        let find = |n: u32| self.reports.iter().find(|r| r.n == n).map(|r| r.markdown.as_str());
        let (Some(x), Some(y)) = (find(a), find(b)) else { return Vec::new() };
        let (xs, ys): (Vec<&str>, Vec<&str>) =
            (x.lines().filter(|l| !l.trim().is_empty()).collect(), y.lines().filter(|l| !l.trim().is_empty()).collect());
        let mut out: Vec<DiffLine> = xs.iter().filter(|l| !ys.contains(l)).map(|l| DiffLine { kind: DiffKind::Removed, text: (*l).into() }).collect();
        out.extend(ys.iter().filter(|l| !xs.contains(l)).map(|l| DiffLine { kind: DiffKind::Added, text: (*l).into() }));
        out
    }

    /// A file name for exporting revision `n`.
    pub fn report_file(&self, n: u32) -> String {
        let slug: String = self.title.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let slug = slug.split('-').filter(|s| !s.is_empty()).take(6).collect::<Vec<_>>().join("-");
        format!("exports/research-{}-r{n}.md", if slug.is_empty() { "project" } else { &slug })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::{Finding, StepEnd, finish_step};
    use crate::steer::{SourceRule, Steer};

    fn project() -> ResearchProject {
        let mut p = ResearchProject::from_question("How does Tokio cancel a task?", "atlas", 1);
        p.state = ProjectState::Running;
        p
    }

    fn end(text: &str, url: &str) -> StepEnd {
        StepEnd::Completed { text: text.into(), sources: vec![url.into()], pages: 1, tool_ms: 1 }
    }

    #[test]
    fn each_finished_step_is_a_milestone_with_a_revision_and_a_diff() {
        let mut p = project();
        assert_eq!(p.milestone(2, "start"), Some(1));
        assert_eq!(p.milestone(3, "again"), None, "no change, no revision");
        finish_step(&mut p, 0, end("Tokio uses a cancel token.", "https://tokio.rs/a"), 10);
        finish_step(&mut p, 1, end("Drop cancels futures.", "https://docs.rs/b"), 20);
        assert_eq!(p.reports.iter().map(|r| r.n).collect::<Vec<_>>(), vec![1, 2, 3]);
        let md = &p.reports[2].markdown;
        assert!(md.contains("### Search the Library and the web for the question") && md.contains("Drop cancels futures."));
        assert!(md.contains("1. <https://tokio.rs/a>") && md.contains("2. <https://docs.rs/b>"));
        let d = p.report_diff(2, 3);
        assert!(d.iter().any(|l| l.kind == DiffKind::Added && l.text == "Drop cancels futures."));
        assert!(d.iter().all(|l| l.kind != DiffKind::Added || !l.text.contains("Tokio uses")), "unchanged lines are not in the diff");
        assert!(p.log.iter().any(|l| l.text.starts_with("milestone: report revision 3")));
    }

    #[test]
    fn the_last_step_makes_the_done_milestone() {
        let mut p = project();
        for i in 0..3 {
            finish_step(&mut p, i, end(&format!("found {i}"), "https://tokio.rs/"), 10 + i as u64);
        }
        assert_eq!(p.state, ProjectState::Done);
        assert!(p.log.iter().any(|l| l.text.contains("(project done)")));
    }

    #[test]
    fn a_wrong_finding_and_an_excluded_site_are_not_in_the_report() {
        let mut p = project();
        p.findings.push(Finding { step: "bad".into(), text: "false claim".into(), sources: vec![], at_ms: 1 });
        p.findings.push(Finding {
            step: "ok".into(),
            text: "true claim".into(),
            sources: vec!["https://spam.example/x".into(), "https://tokio.rs/".into()],
            at_ms: 2,
        });
        p.submit_steer(3, "me", Steer::Feedback { finding: 0, verdict: Verdict::Wrong });
        p.submit_steer(3, "me", Steer::Source { pattern: "spam.example".into(), rule: SourceRule::Exclude });
        p.apply_pending(4);
        let md = p.report_markdown();
        assert!(!md.contains("false claim") && md.contains("true claim"));
        assert!(!md.contains("spam.example") && md.contains("<https://tokio.rs/>"));
    }

    #[test]
    fn revisions_are_capped_and_exports_get_a_clear_file_name() {
        let mut p = project();
        for i in 0..25 {
            p.findings.push(Finding { step: format!("s{i}"), text: format!("t{i}"), sources: vec![], at_ms: i });
            p.milestone(i, "x");
        }
        assert_eq!((p.reports.len(), p.reports[0].n, p.reports.last().unwrap().n), (MAX_REVISIONS, 6, 25));
        assert_eq!(p.report_file(7), "exports/research-how-does-tokio-cancel-a-task-r7.md");
        assert!(p.report_diff(1, 25).is_empty(), "a dropped revision has no diff");
    }
}
