//! Markdown export of one task: the question, the answer with footnotes, and the trace.

use weirdnet_agent::{Citation, Outcome, Trace};

use crate::view::{footnotes, trace_rows};

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// "9 steps, 1 search, 1 tool call, 4 pages".
fn summary_line(trace: &Trace) -> String {
    let s = trace.summary();
    format!(
        "{}, {}, {}, {}",
        plural(s.steps, "step", "steps"),
        plural(s.searches, "search", "searches"),
        plural(s.tool_calls, "tool call", "tool calls"),
        plural(s.pages, "page", "pages")
    )
}

/// Renders a task as Markdown. Only citations the trace backs are included.
pub fn export_markdown(companion: &str, question: &str, outcome: &Outcome, citations: Vec<Citation>) -> String {
    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", question.trim().lines().next().unwrap_or("Task")));
    md.push_str(&format!("*{companion}, {}*\n\n", summary_line(&outcome.trace)));
    md.push_str(outcome.text.trim());
    md.push_str("\n\n");
    let notes = footnotes(&outcome.trace, citations);
    if !notes.is_empty() {
        md.push_str("## Sources\n\n");
        for n in &notes {
            let steps = n.steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ");
            match &n.url {
                Some(u) => md.push_str(&format!("[{}] <{u}> (steps {steps})\n", n.n)),
                None => md.push_str(&format!("[{}] {} (steps {steps})\n", n.n, n.label)),
            }
        }
        md.push('\n');
    }
    md.push_str("## Trace\n\n");
    for r in trace_rows(&outcome.trace) {
        md.push_str(&format!("{}. {} ({})\n", r.step, r.text, r.session));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use weirdnet_agent::{AgentId, BudgetUsage, CitationTarget, SessionMark, StepId, StepKind, StopReason};

    fn outcome() -> Outcome {
        let mut t = Trace::new("t1", AgentId::new("local"));
        t.push(StepKind::Search { provider: "DuckDuckGo".into(), query: "tokio".into(), results: 5 }, SessionMark::LoggedOut, None, 1);
        t.push(StepKind::Fetch { url: "https://tokio.rs/".into(), status: Some(200), bytes: 10 }, SessionMark::LoggedOut, None, 2);
        Outcome {
            task_id: "t1".into(),
            text: "Tokio cancels tasks cooperatively [1].".into(),
            stop: StopReason::Completed,
            trace: t,
            usage: BudgetUsage::default(),
        }
    }

    #[test]
    fn export_has_answer_sources_and_trace_and_drops_unbacked_citations() {
        let ok = Citation { n: 1, target: CitationTarget::Url { url: "https://tokio.rs/".into(), quote: None }, steps: vec![StepId(2)] };
        let bad = Citation { n: 2, target: CitationTarget::Url { url: "https://nope.example/".into(), quote: None }, steps: vec![StepId(2)] };
        let md = export_markdown("Atlas", "How does tokio cancel?\nmore", &outcome(), vec![ok, bad]);
        assert!(md.starts_with("# How does tokio cancel?\n\n*Atlas, 2 steps, 1 search, 0 tool calls, 1 page*"));
        assert!(md.contains("Tokio cancels tasks cooperatively [1]."));
        assert!(md.contains("[1] <https://tokio.rs/> (steps 2)"));
        assert!(!md.contains("nope.example"));
        assert!(md.contains("1. Searched DuckDuckGo for “tokio” (5 results) (logged out)"));
    }

    #[test]
    fn export_without_citations_has_no_sources_heading() {
        let md = export_markdown("Local", "q", &outcome(), vec![]);
        assert!(!md.contains("## Sources"));
        assert!(md.contains("## Trace"));
    }
}
