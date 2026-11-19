//! Address bar suggestions: pages from history and bookmarks that match what
//! is being typed, best first, and the inline completion of the best one.
//!
//! Visits are folded into one [`Page`] per address once ([`Index::build`]),
//! so a keystroke costs one pass over distinct pages (at most the history
//! cap of 5000) and no allocation per page.

use std::collections::HashMap;

use morph_store::HistoryEntry;

/// Rows a suggestion list shows.
pub const MAX_SUGGESTIONS: usize = 6;

/// Where a suggestion came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    History,
    Bookmark,
}

/// One suggested page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub source: Source,
    /// The address to open.
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone)]
struct Page {
    /// The address of the newest visit (or the bookmark).
    url: String,
    title: String,
    /// `url` as typed without its scheme and `www.`, lower case.
    key: String,
    title_lower: String,
    visits: u32,
    last_ms: i64,
    bookmarked: bool,
}

/// Every page worth suggesting, folded from history and bookmarks.
#[derive(Debug, Clone, Default)]
pub struct Index {
    pages: Vec<Page>,
}

/// `url` without `http://`, `https://` and a leading `www.`, lower case, and
/// without a lone trailing `/`: the form people type.
pub fn key(url: &str) -> String {
    let lower = url.trim().to_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).unwrap_or(&lower);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    rest.strip_suffix('/').filter(|r| !r.contains('/')).unwrap_or(rest).to_string()
}

impl Index {
    /// Fold `history` (any order) and `bookmarks` (url, title) into pages.
    pub fn build(history: &[HistoryEntry], bookmarks: &[(String, String)]) -> Self {
        let mut at: HashMap<String, usize> = HashMap::with_capacity(history.len().min(8192));
        let mut pages: Vec<Page> = Vec::new();
        let mut slot = |url: &str, title: &str, ms: i64, visit: bool, bookmark: bool| {
            let key = key(url);
            if key.is_empty() {
                return;
            }
            let i = *at.entry(key.clone()).or_insert_with(|| {
                pages.push(Page {
                    url: url.trim().to_string(),
                    title: String::new(),
                    key,
                    title_lower: String::new(),
                    visits: 0,
                    last_ms: i64::MIN,
                    bookmarked: false,
                });
                pages.len() - 1
            });
            let page = &mut pages[i];
            page.visits += u32::from(visit);
            page.bookmarked |= bookmark;
            if (visit && ms >= page.last_ms) || page.title.is_empty() {
                if !title.trim().is_empty() {
                    page.title = title.trim().to_string();
                    page.title_lower = page.title.to_lowercase();
                }
                if visit && ms >= page.last_ms {
                    page.url = url.trim().to_string();
                }
            }
            if visit {
                page.last_ms = page.last_ms.max(ms);
            }
        };
        for e in history {
            slot(&e.url, &e.title, e.visited_ms, true, false);
        }
        for (url, title) in bookmarks {
            slot(url, title, 0, false, true);
        }
        Index { pages }
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// The best `limit` pages for `query`, best first: how well the text
    /// matches (an address that starts with it beats a word of the title,
    /// which beats a match in the middle), then how often and how recently
    /// the page was visited, and a bookmark counts extra.
    pub fn suggest(&self, query: &str, now_ms: i64, limit: usize) -> Vec<Suggestion> {
        let terms = terms(query);
        if terms.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut scored: Vec<(f64, usize)> = self.pages.iter().enumerate().filter_map(|(i, p)| score(p, &terms, now_ms).map(|s| (s, i))).collect();
        let by_score = |a: &(f64, usize), b: &(f64, usize)| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1));
        if scored.len() > limit {
            scored.select_nth_unstable_by(limit - 1, by_score);
            scored.truncate(limit);
        }
        scored.sort_by(by_score);
        scored
            .into_iter()
            .map(|(_, i)| {
                let p = &self.pages[i];
                Suggestion { source: if p.visits == 0 { Source::Bookmark } else { Source::History }, url: p.url.clone(), title: p.title.clone() }
            })
            .collect()
    }

    /// What the address bar completes `typed` to inline: `typed` followed by
    /// the rest of the best page whose address starts with it, up to the end
    /// of the host while `typed` is still within the host (`git` →
    /// `github.com`, `github.com/Weird` → `github.com/WeirdBrains/morph`).
    /// `None` when nothing continues it, or `typed` is a phrase.
    pub fn complete(&self, typed: &str, now_ms: i64) -> Option<String> {
        let typed = typed.trim_start();
        if typed.is_empty() || typed.chars().any(char::is_whitespace) {
            return None;
        }
        let stripped = key_prefix(typed);
        if stripped.is_empty() {
            return None;
        }
        let terms = [stripped.clone()];
        let best = self
            .pages
            .iter()
            .filter(|p| p.key.len() > stripped.len() && p.key.starts_with(&stripped))
            .filter_map(|p| score(p, &terms, now_ms).map(|s| (s, p)))
            .max_by(|a, b| a.0.total_cmp(&b.0))?
            .1;
        // Complete the host while typing inside it, the whole address after.
        let host_end = best.key.find('/').unwrap_or(best.key.len());
        let end = if stripped.len() < host_end { host_end } else { best.key.len() };
        Some(format!("{typed}{}", best.rest(stripped.len(), end)))
    }
}

impl Page {
    /// `key[from..to]`, with the capitals the address has (paths are case
    /// sensitive), when the original cuts at the same places.
    fn rest(&self, from: usize, to: usize) -> String {
        let url = self.url.trim();
        let lower = url.to_ascii_lowercase();
        let after_scheme = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).unwrap_or(&lower);
        let mut skip = lower.len() - after_scheme.len();
        if after_scheme.starts_with("www.") {
            skip += 4;
        }
        match url.get(skip + from..skip + to) {
            Some(original) if original.to_lowercase() == self.key[from..to] => original.to_string(),
            _ => self.key[from..to].to_string(),
        }
    }
}

/// The text a typed address is matched as: lower case, no scheme, no `www.`.
fn key_prefix(typed: &str) -> String {
    let lower = typed.trim().to_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).unwrap_or(&lower);
    rest.strip_prefix("www.").unwrap_or(rest).to_string()
}

fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(key_prefix).filter(|t| !t.is_empty()).collect()
}

const DAY_MS: f64 = 86_400_000.0;

/// `None` when `page` doesn't match every term.
fn score(page: &Page, terms: &[String], now_ms: i64) -> Option<f64> {
    let mut quality = 0.0;
    for (n, term) in terms.iter().enumerate() {
        quality += if n == 0 && page.key.starts_with(term.as_str()) {
            100.0
        } else if page.key.starts_with(term.as_str()) {
            80.0
        } else if title_word_starts(&page.title_lower, term) {
            60.0
        } else if page.key.contains(term.as_str()) {
            40.0
        } else if page.title_lower.contains(term.as_str()) {
            30.0
        } else {
            return None;
        };
    }
    quality /= terms.len() as f64;
    let frequency = 8.0 * f64::from(page.visits + u32::from(page.bookmarked)).ln_1p();
    let recency = if page.last_ms == i64::MIN {
        0.0
    } else {
        let days = ((now_ms - page.last_ms) as f64 / DAY_MS).max(0.0);
        20.0 * 0.5f64.powf(days / 14.0)
    };
    Some(quality + frequency + recency + if page.bookmarked { 15.0 } else { 0.0 })
}

fn title_word_starts(title_lower: &str, term: &str) -> bool {
    title_lower.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(term))
}

/// What the address bar's text means: an address or search as always, or a question for a
/// Companion (`> question` for the default one, `>name question` for a named one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Everything that does not start with `>`: URLs, searches, history and bookmark matching.
    Normal,
    Companion(Ask),
}

/// A `>` input, taken apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// The companion's name as typed, `None` for the default companion.
    pub name: Option<String>,
    /// The question, trimmed (empty while it is still being typed).
    pub question: String,
    /// The name is still being typed: no space has followed it yet.
    pub typing_name: bool,
}

/// Decide the mode for the bar's text. Only a leading `>` (after spaces) starts Companion mode;
/// nothing else changes how input is handled.
pub fn dispatch(text: &str) -> Mode {
    let Some(rest) = text.trim_start().strip_prefix('>') else {
        return Mode::Normal;
    };
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        return Mode::Companion(Ask { name: None, question: rest.trim().to_string(), typing_name: rest.is_empty() });
    }
    match rest.split_once(char::is_whitespace) {
        Some((name, question)) => Mode::Companion(Ask { name: Some(name.to_string()), question: question.trim().to_string(), typing_name: false }),
        None => Mode::Companion(Ask { name: Some(rest.to_string()), question: String::new(), typing_name: true }),
    }
}

/// Companion names that continue what was typed after `>`, in roster order (case-insensitive prefix).
pub fn complete_names<'a>(typed: &str, names: &'a [String]) -> Vec<&'a str> {
    let typed = typed.to_lowercase();
    names.iter().map(String::as_str).filter(|n| n.to_lowercase().starts_with(&typed)).collect()
}

/// The roster name `typed` means: an exact match ignoring case.
pub fn resolve_name<'a>(typed: &str, names: &'a [String]) -> Option<&'a str> {
    names.iter().map(String::as_str).find(|n| n.eq_ignore_ascii_case(typed) || n.to_lowercase() == typed.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const DAY: i64 = 86_400_000;

    fn visit(url: &str, title: &str, days_ago: i64) -> HistoryEntry {
        HistoryEntry { url: url.into(), title: title.into(), visited_ms: NOW - days_ago * DAY, ..Default::default() }
    }

    fn urls(list: &[Suggestion]) -> Vec<&str> {
        list.iter().map(|s| s.url.as_str()).collect()
    }

    #[test]
    fn keys_drop_scheme_www_and_a_lone_slash() {
        assert_eq!(key("https://www.Example.com/"), "example.com");
        assert_eq!(key("http://example.com/a/"), "example.com/a/");
        assert_eq!(key("https://example.com/a?b=1"), "example.com/a?b=1");
    }

    #[test]
    fn an_address_that_starts_with_the_text_beats_a_title_word_and_a_middle_match() {
        let index = Index::build(
            &[
                visit("https://example.com/notes-on-rust", "Notes", 0),
                visit("https://blog.example.com/", "All about Rust", 0),
                visit("https://rust-lang.org/", "Rust", 0),
            ],
            &[],
        );
        let found = index.suggest("rust", NOW, 6);
        assert_eq!(urls(&found), ["https://rust-lang.org/", "https://blog.example.com/", "https://example.com/notes-on-rust"]);
    }

    #[test]
    fn frequency_and_recency_break_ties() {
        let index = Index::build(
            &[
                visit("https://news.example.org/", "News", 40),
                visit("https://newsletter.example.org/", "Newsletter", 6),
                visit("https://newsletter.example.org/", "Newsletter", 5),
                visit("https://newsletter.example.org/", "Newsletter", 4),
                visit("https://newbie.example.org/", "Newbie", 5),
                visit("https://newest.example.org/", "Newest", 40),
                visit("https://newest.example.org/", "Newest", 41),
                visit("https://newest.example.org/", "Newest", 42),
            ],
            &[],
        );
        let found = index.suggest("new", NOW, 6);
        // Three visits this week beat one; many old visits beat one old visit;
        // a recent visit beats an equally frequent old one.
        assert_eq!(
            urls(&found),
            ["https://newsletter.example.org/", "https://newbie.example.org/", "https://newest.example.org/", "https://news.example.org/"]
        );
    }

    #[test]
    fn repeat_visits_fold_into_one_page_with_the_newest_title() {
        let index =
            Index::build(&[visit("https://a.test/", "Old", 5), visit("https://www.a.test", "New", 1), visit("http://a.test/", "Older", 9)], &[]);
        assert_eq!(index.len(), 1);
        let found = index.suggest("a.t", NOW, 6);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "New");
    }

    #[test]
    fn every_word_must_match_and_the_scheme_is_ignored() {
        let index = Index::build(
            &[visit("https://github.com/WeirdBrains/morph", "WeirdBrains/morph", 1), visit("https://github.com/other", "other", 1)],
            &[],
        );
        assert_eq!(urls(&index.suggest("weird morph", NOW, 6)), ["https://github.com/WeirdBrains/morph"]);
        assert_eq!(urls(&index.suggest("https://github.com/o", NOW, 6)), ["https://github.com/other"]);
        assert!(index.suggest("weird nothing", NOW, 6).is_empty());
        assert!(index.suggest("   ", NOW, 6).is_empty());
    }

    #[test]
    fn bookmarks_are_suggested_and_rank_above_equal_history() {
        let index = Index::build(
            &[visit("https://one.test/", "One", 3), visit("https://onyx.test/", "Onyx", 3)],
            &[("https://onyx.test/".into(), "Onyx".into()), ("https://only.test/".into(), "Only".into())],
        );
        let found = index.suggest("on", NOW, 6);
        assert_eq!(found[0].url, "https://onyx.test/");
        assert_eq!(found[0].source, Source::History);
        let only = found.iter().find(|s| s.url.contains("only")).unwrap();
        assert_eq!(only.source, Source::Bookmark);
    }

    #[test]
    fn the_list_is_capped_and_ordered() {
        let history: Vec<_> = (0..50).map(|i| visit(&format!("https://site{i}.test/"), "S", i)).collect();
        let found = Index::build(&history, &[]).suggest("site", NOW, 6);
        assert_eq!(found.len(), 6);
        assert_eq!(found[0].url, "https://site0.test/", "the newest of equals first");
    }

    #[test]
    fn completion_finishes_the_host_then_the_address() {
        let index = Index::build(
            &[visit("https://github.com/WeirdBrains/morph", "morph", 0), visit("https://github.com/WeirdBrains/morph", "morph", 1)],
            &[],
        );
        assert_eq!(index.complete("git", NOW).as_deref(), Some("github.com"));
        assert_eq!(index.complete("GIT", NOW).as_deref(), Some("GIThub.com"), "what was typed stays as typed");
        assert_eq!(index.complete("github.com", NOW).as_deref(), Some("github.com/WeirdBrains/morph"));
        assert_eq!(index.complete("github.com/Weird", NOW).as_deref(), Some("github.com/WeirdBrains/morph"));
        assert_eq!(index.complete("https://www.gi", NOW).as_deref(), Some("https://www.github.com"));
        assert_eq!(index.complete("github.com/WeirdBrains/morph", NOW), None, "nothing left to add");
        assert_eq!(index.complete("zzz", NOW), None);
        assert_eq!(index.complete("weird brains", NOW), None, "a phrase has no completion");
        assert_eq!(index.complete("", NOW), None);
    }

    #[test]
    fn completion_prefers_the_better_page() {
        let index = Index::build(
            &[
                visit("https://docs.example.com/", "Docs", 30),
                visit("https://dev.example.com/", "Dev", 0),
                visit("https://dev.example.com/", "Dev", 1),
            ],
            &[],
        );
        assert_eq!(index.complete("d", NOW).as_deref(), Some("dev.example.com"));
    }

    #[test]
    fn large_histories_stay_fast() {
        let history: Vec<_> = (0..5000)
            .map(|i| visit(&format!("https://host{}.example.com/path/{i}", i % 700), &format!("Page number {i} about things"), i64::from(i % 90)))
            .collect();
        let started = std::time::Instant::now();
        let index = Index::build(&history, &[]);
        let built = started.elapsed();
        let started = std::time::Instant::now();
        for q in ["h", "ho", "host1", "number 4", "zzz", "things"] {
            let _ = index.suggest(q, NOW, 6);
            let _ = index.complete(q, NOW);
        }
        let per_key = started.elapsed() / 12;
        // Generous for debug builds on a busy machine; release is ~100x faster.
        assert!(built.as_millis() < 500, "build {built:?}");
        assert!(per_key.as_millis() < 50, "per keystroke {per_key:?}");
    }
    fn ask(name: Option<&str>, question: &str, typing_name: bool) -> Mode {
        Mode::Companion(Ask { name: name.map(String::from), question: question.into(), typing_name })
    }

    #[test]
    fn a_leading_chevron_is_companion_mode_and_nothing_else_is() {
        assert_eq!(dispatch("> what is rust"), ask(None, "what is rust", false));
        assert_eq!(dispatch("  >   spaced  "), ask(None, "spaced", false));
        assert_eq!(dispatch(">"), ask(None, "", true));
        assert_eq!(dispatch(">atlas what is rust"), ask(Some("atlas"), "what is rust", false));
        assert_eq!(dispatch(">atlas"), ask(Some("atlas"), "", true));
        assert_eq!(dispatch(">atlas "), ask(Some("atlas"), "", false));
        for normal in
            ["", "github.com", "https://example.com/a?b=>1", "rust > go", "a>b", "what is >", "morph://bookmarks", "localhost:8080", "<script>"]
        {
            assert_eq!(dispatch(normal), Mode::Normal, "{normal:?}");
        }
    }

    #[test]
    fn names_complete_by_case_insensitive_prefix_and_resolve_exactly() {
        let names = vec!["Atlas".to_string(), "Archie".to_string(), "Bo".to_string()];
        assert_eq!(complete_names("a", &names), ["Atlas", "Archie"]);
        assert_eq!(complete_names("AT", &names), ["Atlas"]);
        assert_eq!(complete_names("", &names), ["Atlas", "Archie", "Bo"]);
        assert!(complete_names("z", &names).is_empty());
        assert_eq!(resolve_name("atlas", &names), Some("Atlas"));
        assert_eq!(resolve_name("at", &names), None);
    }

    #[test]
    fn normal_input_still_gets_history_and_bookmark_suggestions_and_completion() {
        let index = Index::build(&[visit("https://github.com/WeirdBrains/morph", "Morph", 1)], &[("https://example.com/".into(), "Example".into())]);
        for typed in ["git", "morph", "example"] {
            assert_eq!(dispatch(typed), Mode::Normal);
            assert!(!index.suggest(typed, NOW, MAX_SUGGESTIONS).is_empty(), "{typed}");
        }
        assert_eq!(index.complete("git", NOW).as_deref(), Some("github.com"));
        assert_eq!(index.suggest("morph", NOW, 6)[0].source, Source::History);
        assert_eq!(index.suggest("example", NOW, 6)[0].source, Source::Bookmark);
        // The index itself never treats the chevron as special; the bar decides by `dispatch`.
        assert!(index.suggest(">", NOW, 6).is_empty());
    }
}
