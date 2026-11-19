//! Footnotes in a companion's answer, turned into [`Citation`]s that the task's trace must back
//! (`weirdnet_agent::retain_valid`). The model is asked to mark claims `[n]` and to end with a
//! sources list (`[n]: <url> (step 4)`); a citation whose URL no step fetched, or whose tool no
//! step called, is dropped along with its markers, so the panel never shows an unbacked source.

use weirdnet_agent::{Citation, CitationTarget, StepId, StepKind, Trace, retain_valid};

/// An answer ready to show: the text without its sources list, and the citations the trace backs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footnoted {
    pub text: String,
    pub citations: Vec<Citation>,
    /// Footnotes the model wrote that the trace does not back (dropped from `text`).
    pub dropped: Vec<u32>,
}

/// `[n]`, `[^n]`, `[n]:` or `[^n]:` at the start of `s`: the number and the rest.
fn definition(line: &str) -> Option<(u32, &str)> {
    let s = line.trim_start().trim_start_matches(['-', '*']).trim_start();
    let s = s.strip_prefix('[')?;
    let s = s.strip_prefix('^').unwrap_or(s);
    let end = s.find(']')?;
    let n: u32 = s[..end].trim().parse().ok()?;
    let rest = s[end + 1..].trim_start();
    let rest = rest.strip_prefix(':').unwrap_or(rest).trim();
    (!rest.is_empty()).then_some((n, rest))
}

fn is_sources_heading(line: &str) -> bool {
    let t = line.trim().trim_start_matches('#').trim().trim_matches('*').trim().trim_end_matches(':').trim().to_ascii_lowercase();
    matches!(t.as_str(), "sources" | "references" | "citations" | "footnotes")
}

fn first_url(s: &str) -> Option<String> {
    let start = s.find("https://").or_else(|| s.find("http://"))?;
    let tail = &s[start..];
    let end = tail.find(|c: char| c.is_whitespace() || matches!(c, '>' | ')' | ']' | '"' | '\'' | ',')).unwrap_or(tail.len());
    Some(tail[..end].trim_end_matches(['.', ';']).to_string())
}

/// Step numbers written after "step" or "steps" in `s` ("step 4", "steps 2, 3 and 5").
fn step_numbers(s: &str) -> Vec<StepId> {
    let lower = s.to_ascii_lowercase();
    let mut out = Vec::new();
    for (i, _) in lower.match_indices("step") {
        let tail = lower[i + 4..].strip_prefix('s').unwrap_or(&lower[i + 4..]);
        for word in tail.split(|c: char| c.is_whitespace() || c == ',' || c == '&' || c == '#' || c == ':') {
            if word.is_empty() || word == "and" {
                continue;
            }
            let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
            match digits.parse::<u32>() {
                Ok(n) => out.push(StepId(n)),
                Err(_) => break,
            }
            if digits.len() != word.len() {
                break;
            }
        }
    }
    out.dedup();
    out
}

fn tool_target(trace: &Trace, steps: &[StepId], text: &str) -> Option<(CitationTarget, Vec<StepId>)> {
    let as_tool = |id: &StepId| match trace.step(*id).map(|s| &s.kind) {
        Some(StepKind::Tool { source, tool, is_error: false, .. }) => Some((source.clone(), tool.clone())),
        _ => None,
    };
    if let Some((source, tool)) = steps.iter().find_map(as_tool) {
        return Some((CitationTarget::ToolResult { source, tool }, steps.to_vec()));
    }
    // No step given: a tool named in the text ("page.text", "mouser-tools.search_issues").
    for s in &trace.steps {
        if let StepKind::Tool { source, tool, is_error: false, .. } = &s.kind
            && (text.contains(&format!("{source}.{tool}")) || text.split(|c: char| c.is_whitespace() || c == '`').any(|w| w == tool))
        {
            let ids: Vec<StepId> = trace
                .steps
                .iter()
                .filter(|x| matches!(&x.kind, StepKind::Tool { source: s2, tool: t2, is_error: false, .. } if s2 == source && t2 == tool))
                .map(|x| x.id)
                .collect();
            return Some((CitationTarget::ToolResult { source: source.clone(), tool: tool.clone() }, ids));
        }
    }
    None
}

fn url_key(url: &str) -> String {
    let no_frag = url.split('#').next().unwrap_or_default();
    no_frag.strip_suffix('/').unwrap_or(no_frag).to_ascii_lowercase()
}

/// Splits `answer` into what to show and the citations `trace` backs.
pub fn parse_citations(answer: &str, trace: &Trace) -> Footnoted {
    let mut body: Vec<&str> = Vec::new();
    let mut wanted: Vec<Citation> = Vec::new();
    let fetched = trace.fetched_urls();
    let lines: Vec<&str> = answer.lines().collect();
    // The sources list is the trailing run of definitions (and its heading, and blank lines).
    let mut tail_start = lines.len();
    for (i, line) in lines.iter().enumerate().rev() {
        if line.trim().is_empty() || definition(line).is_some() || (is_sources_heading(line) && tail_start <= i + 1) {
            tail_start = i;
        } else {
            break;
        }
    }
    // A heading-less list of definitions still counts; a lone "[1] text" line in prose does not.
    for line in &lines[..tail_start] {
        body.push(line);
    }
    for line in &lines[tail_start..] {
        let Some((n, rest)) = definition(line) else { continue };
        let mut steps = step_numbers(rest);
        let target = match first_url(rest) {
            Some(url) => {
                if steps.is_empty() {
                    // No step written: the steps that fetched this URL, if any.
                    steps = fetched.iter().filter(|(u, _)| url_key(u) == url_key(&url)).flat_map(|(_, ids)| ids.clone()).collect();
                }
                Some((CitationTarget::Url { url, quote: None }, steps))
            }
            None => tool_target(trace, &steps, rest),
        };
        if let Some((target, steps)) = target
            && !wanted.iter().any(|c| c.n == n)
        {
            wanted.push(Citation { n, target, steps });
        }
    }
    let valid = retain_valid(trace, wanted.clone());
    let dropped: Vec<u32> = wanted.iter().map(|c| c.n).filter(|n| !valid.iter().any(|v| v.n == *n)).collect();
    let mut text = body.join("\n").trim_end().to_string();
    // `[^n]` reads as `[n]` in the panel; markers of dropped or unknown footnotes go.
    for c in &wanted {
        text = text.replace(&format!("[^{}]", c.n), &format!("[{}]", c.n));
    }
    text = strip_markers(&text, |n| valid.iter().any(|c| c.n == n));
    Footnoted { text, citations: valid, dropped }
}

/// Removes `[n]` and `[^n]` markers whose footnote `keep` rejects. Links (`[1](url)`) stay.
fn strip_markers(text: &str, keep: impl Fn(u32) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let inner = after.strip_prefix('^').unwrap_or(after);
        let caret = inner.len() != after.len();
        let close = inner.find(']');
        let parsed = close.and_then(|e| inner[..e].parse::<u32>().ok().map(|n| (n, e)));
        match parsed {
            Some((n, e)) if !inner[e + 1..].starts_with('(') => {
                let consumed = i + 1 + usize::from(caret) + e + 1;
                if keep(n) {
                    out.push_str(&format!("[{n}]"));
                } else if out.ends_with(' ') && rest[consumed..].starts_with(|c: char| c.is_ascii_punctuation() || c.is_whitespace()) {
                    out.pop();
                }
                rest = &rest[consumed..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use weirdnet_agent::{AgentId, SessionMark};

    fn trace() -> Trace {
        let mut t = Trace::new("t1", AgentId::new("atlas"));
        let lo = SessionMark::LoggedOut;
        t.push(StepKind::Search { provider: "duckduckgo".into(), query: "tokio".into(), results: 5 }, lo.clone(), None, 1);
        t.push(StepKind::Fetch { url: "https://tokio.rs/tokio/topics/shutdown".into(), status: Some(200), bytes: 900 }, lo.clone(), None, 2);
        t.push(StepKind::Extract { url: "https://tokio.rs/tokio/topics/shutdown".into(), chars: 400 }, lo.clone(), None, 3);
        t.push(
            StepKind::Tool { source: "browser".into(), tool: "page.text".into(), args_summary: "{}".into(), result_bytes: 10, is_error: false },
            lo,
            None,
            4,
        );
        t
    }

    #[test]
    fn backed_footnotes_stay_and_unbacked_ones_go_with_their_markers() {
        let answer = "Use a CancellationToken [1]. Drop is not async [2]. The page agrees [3].\n\nSources:\n[1]: https://tokio.rs/tokio/topics/shutdown/ (step 2)\n[2]: https://made-up.example/drop (step 2)\n[3]: page.text (step 4)";
        let f = parse_citations(answer, &trace());
        assert_eq!(f.text, "Use a CancellationToken [1]. Drop is not async. The page agrees [3].");
        assert_eq!(f.citations.len(), 2);
        assert_eq!(f.dropped, vec![2]);
        assert!(matches!(&f.citations[1].target, CitationTarget::ToolResult { source, tool } if source == "browser" && tool == "page.text"));
    }

    #[test]
    fn a_url_without_steps_takes_the_steps_that_fetched_it() {
        let f = parse_citations("Shut down cleanly [^1].\n\n[^1]: <https://Tokio.rs/tokio/topics/shutdown>", &trace());
        assert_eq!(f.text, "Shut down cleanly [1].");
        assert_eq!(f.citations[0].steps, vec![StepId(2)]);
    }

    #[test]
    fn a_step_that_did_not_fetch_the_url_does_not_back_it() {
        let f = parse_citations("X [1].\n\n## Sources\n- [1] https://tokio.rs/tokio/topics/shutdown (step 1)", &trace());
        assert!(f.citations.is_empty());
        assert_eq!(f.text, "X.");
    }

    #[test]
    fn links_and_plain_brackets_are_left_alone() {
        let f = parse_citations("See [docs](https://a.b) and [x] and [2](https://c.d).", &trace());
        assert_eq!(f.text, "See [docs](https://a.b) and [x] and [2](https://c.d).");
        assert!(f.citations.is_empty() && f.dropped.is_empty());
    }

    #[test]
    fn step_lists_parse() {
        assert_eq!(step_numbers("(steps 2, 3)"), vec![StepId(2), StepId(3)]);
        assert_eq!(step_numbers("(steps 2, 3 and 5)"), vec![StepId(2), StepId(3), StepId(5)]);
        assert_eq!(step_numbers("step 4"), vec![StepId(4)]);
        assert_eq!(step_numbers("no numbers"), Vec::<StepId>::new());
    }
}
