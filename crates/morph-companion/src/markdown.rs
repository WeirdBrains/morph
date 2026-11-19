//! Markdown for Companion answers, as plain data the panel draws.
//!
//! Answers are untrusted model output: raw HTML is shown as text, images become their alt text,
//! and only `http(s)` links are links (each with the domain it really goes to). [`Markdown`] takes
//! streamed text and re-parses only the part after the last finished block, once per
//! [`Markdown::settle`], so a long answer does not re-layout while it grows. An unclosed code
//! fence is code until it closes.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A link the user can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    /// The host the link goes to, shown beside the label.
    pub domain: String,
}

/// A run of text with one style.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    /// Secondary text: the domain after a link.
    pub dim: bool,
    pub link: Option<Link>,
    /// A `[n]` marker that points at footnote `n`.
    pub footnote: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading(u8, Vec<Span>),
    Code { lang: Option<String>, text: String, closed: bool },
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Quote(Vec<Block>),
    Rule,
}

/// The host of an `http(s)` URL (no userinfo, port or path), or `None` for anything else.
pub fn domain_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.find(']').map_or(host, |i| &host[..i + 2]),
        None => host.split(':').next().unwrap_or(""),
    };
    (!host.is_empty()).then(|| host.to_lowercase())
}

enum Container {
    Root(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Item(Vec<Block>),
    Quote(Vec<Block>),
}

#[derive(Default)]
struct Builder {
    stack: Vec<Container>,
    bold: u32,
    italic: u32,
    link: Option<Link>,
    spans: Vec<Span>,
    code: Option<(Option<String>, String)>,
    heading: Option<u8>,
    /// A non-web link's target, shown as text after its label.
    pending_plain: Option<String>,
}

impl Builder {
    fn push_block(&mut self, b: Block) {
        match self.stack.last_mut() {
            Some(Container::Root(v) | Container::Item(v) | Container::Quote(v)) => v.push(b),
            _ => {}
        }
    }

    fn text(&mut self, t: &str, code: bool) {
        if t.is_empty() {
            return;
        }
        let (bold, italic, link) = (self.bold > 0, self.italic > 0, self.link.clone());
        if let Some(last) = self.spans.last_mut()
            && !code
            && !last.code
            && !last.dim
            && last.footnote.is_none()
            && last.bold == bold
            && last.italic == italic
            && last.link == link
        {
            last.text.push_str(t);
            return;
        }
        self.spans.push(Span { text: t.to_string(), bold, italic, code, link, ..Span::default() });
    }

    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let spans = split_footnotes(std::mem::take(&mut self.spans));
        match self.heading.take() {
            Some(level) => self.push_block(Block::Heading(level, spans)),
            None => self.push_block(Block::Paragraph(spans)),
        }
    }

    fn event(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => self.flush(),
                Tag::Heading { level, .. } => {
                    self.flush();
                    self.heading = Some(match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    });
                }
                Tag::BlockQuote(_) => {
                    self.flush();
                    self.stack.push(Container::Quote(Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    self.flush();
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => Some(l.split_whitespace().next().unwrap_or("").to_string()).filter(|l| !l.is_empty()),
                        CodeBlockKind::Indented => None,
                    };
                    self.code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    self.flush();
                    self.stack.push(Container::List { start, items: Vec::new() });
                }
                Tag::Item => self.stack.push(Container::Item(Vec::new())),
                Tag::Emphasis => self.italic += 1,
                Tag::Strong => self.bold += 1,
                Tag::Link { dest_url, .. } => {
                    self.link = domain_of(&dest_url).map(|domain| Link { url: dest_url.to_string(), domain });
                    if self.link.is_none() {
                        // Not a web link: show where it points, as plain text, after the label.
                        self.pending_plain = Some(dest_url.to_string());
                    }
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Paragraph | TagEnd::Heading(_) => self.flush(),
                TagEnd::BlockQuote(_) => {
                    self.flush();
                    if let Some(Container::Quote(v)) = self.stack.pop() {
                        self.push_block(Block::Quote(v));
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, text)) = self.code.take() {
                        self.push_block(Block::Code { lang, text: text.trim_end_matches('\n').to_string(), closed: true });
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Container::List { start, items }) = self.stack.pop() {
                        self.push_block(Block::List { start, items });
                    }
                }
                TagEnd::Item => {
                    self.flush();
                    if let Some(Container::Item(v)) = self.stack.pop()
                        && let Some(Container::List { items, .. }) = self.stack.last_mut()
                    {
                        items.push(v);
                    }
                }
                TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
                TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
                TagEnd::Link => {
                    if let Some(link) = self.link.take() {
                        self.spans.push(Span { text: format!(" ({})", link.domain), dim: true, ..Span::default() });
                    } else if let Some(url) = self.pending_plain.take() {
                        self.spans.push(Span { text: format!(" ({url})"), dim: true, ..Span::default() });
                    }
                }
                _ => {}
            },
            Event::Text(t) => match &mut self.code {
                Some((_, buf)) => buf.push_str(&t),
                None => self.text(&t, false),
            },
            Event::Code(t) => self.text(&t, true),
            // Never interpreted: shown as the text it is.
            Event::Html(t) | Event::InlineHtml(t) => match &mut self.code {
                Some((_, buf)) => buf.push_str(&t),
                None => self.text(&t, false),
            },
            Event::SoftBreak => self.text(" ", false),
            Event::HardBreak => self.text("\n", false),
            Event::Rule => {
                self.flush();
                self.push_block(Block::Rule);
            }
            _ => {}
        }
    }
}

/// Split `[n]` markers (the model's citation form) out of plain spans.
fn split_footnotes(spans: Vec<Span>) -> Vec<Span> {
    let mut out = Vec::new();
    for s in spans {
        if s.code || s.link.is_some() || s.dim {
            out.push(s);
            continue;
        }
        let mut rest = s.text.as_str();
        while let Some(open) = rest.find('[') {
            let after = &rest[open + 1..];
            let digits = after.chars().take_while(char::is_ascii_digit).count();
            if (1..=3).contains(&digits) && after[digits..].starts_with(']') {
                if open > 0 {
                    out.push(Span { text: rest[..open].to_string(), ..s.clone() });
                }
                let n: u32 = after[..digits].parse().unwrap_or(0);
                out.push(Span { text: format!("[{n}]"), footnote: Some(n), ..s.clone() });
                rest = &after[digits + 1..];
            } else {
                out.push(Span { text: rest[..=open].to_string(), ..s.clone() });
                rest = after;
            }
        }
        if !rest.is_empty() {
            out.push(Span { text: rest.to_string(), ..s.clone() });
        }
    }
    out
}

/// Whether `text` ends inside a fenced code block.
fn fence_open(text: &str) -> bool {
    let mut open: Option<(char, usize)> = None;
    for line in text.lines() {
        let t = line.trim_start();
        let c = t.chars().next();
        if let Some(c @ ('`' | '~')) = c {
            let n = t.chars().take_while(|&x| x == c).count();
            if n >= 3 {
                match open {
                    None if c != '`' || !t[n..].contains('`') => open = Some((c, n)),
                    Some((oc, on)) if oc == c && n >= on && t[n..].trim().is_empty() => open = None,
                    _ => {}
                }
            }
        }
    }
    open.is_some()
}

/// Parse a whole answer.
pub fn parse(text: &str) -> Vec<Block> {
    let mut b = Builder { stack: vec![Container::Root(Vec::new())], ..Builder::default() };
    for ev in Parser::new_ext(text, Options::empty()) {
        b.event(ev);
    }
    b.flush();
    let mut blocks = match b.stack.pop() {
        Some(Container::Root(v)) => v,
        _ => Vec::new(),
    };
    if fence_open(text)
        && let Some(Block::Code { closed, .. }) = blocks.last_mut()
    {
        *closed = false;
    }
    blocks
}

/// A list item line or an indented line: it can continue the block before a blank line.
fn list_like(line: &str) -> bool {
    let t = line.trim_start();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    line.starts_with("    ")
        || line.starts_with('\t')
        || ["- ", "* ", "+ "].iter().any(|m| t.starts_with(m))
        || (digits > 0 && t[digits..].starts_with(['.', ')']))
}

/// An answer that grows: finished blocks are parsed once and kept; only the tail is parsed again.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Markdown {
    src: String,
    /// Bytes of `src` already parsed into `done`.
    committed: usize,
    done: Vec<Block>,
    tail: Vec<Block>,
    dirty: bool,
}

impl Markdown {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add streamed text. Cheap: nothing is parsed until [`Markdown::settle`].
    pub fn push_str(&mut self, delta: &str) {
        if !delta.is_empty() {
            self.src.push_str(delta);
            self.dirty = true;
        }
    }

    pub fn source(&self) -> &str {
        &self.src
    }

    /// Whether text arrived since the last [`Markdown::settle`].
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Bring the blocks up to date. Call once per frame; it parses only what follows the last
    /// finished block.
    pub fn settle(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let mut open = false;
        let mut pos = self.committed;
        let mut first_line: Option<&str> = None;
        let mut blank_end: Option<usize> = None;
        let mut commit_to = None;
        for line in self.src[self.committed..].split_inclusive('\n') {
            let end = pos + line.len();
            let t = line.trim();
            let fence = t.starts_with("```") || t.starts_with("~~~");
            if open {
                open = !fence;
            } else if t.is_empty() {
                if first_line.is_some() && blank_end.is_none() && line.ends_with('\n') {
                    blank_end = Some(end);
                }
            } else {
                if let (Some(b), Some(first)) = (blank_end.take(), first_line)
                    && !(list_like(first) && list_like(line))
                {
                    commit_to = Some(b);
                    first_line = None;
                }
                if first_line.is_none() {
                    first_line = Some(line.trim_end_matches('\n'));
                }
                open = fence;
            }
            pos = end;
        }
        if let Some(to) = commit_to {
            let mut blocks = parse(&self.src[self.committed..to]);
            self.done.append(&mut blocks);
            self.committed = to;
        }
        self.tail = parse(&self.src[self.committed..]);
    }

    /// Every block, finished ones first.
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.done.iter().chain(self.tail.iter())
    }

    /// How many blocks are finished and will not be parsed again.
    pub fn finished(&self) -> usize {
        self.done.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Span {
        Span { text: text.into(), ..Span::default() }
    }

    fn para(b: &Block) -> &[Span] {
        match b {
            Block::Paragraph(s) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn paragraphs_bold_italic_and_inline_code() {
        let b = parse("Two plus two is **four**, *really*, `2+2`.\n\nNext.");
        assert_eq!(b.len(), 2);
        let s = para(&b[0]);
        assert_eq!(s[0], plain("Two plus two is "));
        assert!(s[1].bold && s[1].text == "four");
        assert!(s[3].italic && s[3].text == "really");
        assert!(s[5].code && s[5].text == "2+2");
        assert_eq!(para(&b[1]), [plain("Next.")]);
    }

    #[test]
    fn headings_lists_and_numbering() {
        let b = parse("# Title\n\n- one\n- two **b**\n\n3. x\n4. y");
        assert!(matches!(&b[0], Block::Heading(1, s) if s[0].text == "Title"));
        match &b[1] {
            Block::List { start: None, items } => {
                assert_eq!(items.len(), 2);
                assert!(para(&items[1][0])[1].bold);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&b[2], Block::List { start: Some(3), items } if items.len() == 2));
    }

    #[test]
    fn nested_lists_keep_their_structure() {
        let b = parse("- a\n  - b\n- c");
        let Block::List { items, .. } = &b[0] else { panic!() };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0][1], Block::List { .. }));
    }

    #[test]
    fn fenced_code_keeps_text_and_language() {
        let b = parse("Here:\n\n```rust\nfn main() {}\n\nlet x = 1;\n```\n\nDone");
        assert_eq!(b[1], Block::Code { lang: Some("rust".into()), text: "fn main() {}\n\nlet x = 1;".into(), closed: true });
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn an_unclosed_fence_is_code_until_it_closes() {
        let b = parse("Run:\n\n```sh\nls -la\n");
        assert_eq!(b[1], Block::Code { lang: Some("sh".into()), text: "ls -la".into(), closed: false });
        assert!(matches!(&parse("```\nx\n```")[0], Block::Code { closed: true, .. }));
        assert!(matches!(&parse("```\nx\n``` y\n")[0], Block::Code { closed: false, .. }), "a closing fence has nothing after it");
    }

    #[test]
    fn web_links_show_their_domain_and_other_schemes_are_not_links() {
        let b = parse("See [the docs](https://user:pw@Docs.Rust-Lang.org:443/std?x=1) and [click](javascript:alert(1)).");
        let s = para(&b[0]);
        let link = s.iter().find(|s| s.link.is_some()).unwrap();
        assert_eq!(link.text, "the docs");
        assert_eq!(link.link.as_ref().unwrap().domain, "docs.rust-lang.org");
        assert!(s.iter().any(|s| s.dim && s.text == " (docs.rust-lang.org)"));
        assert!(s.iter().filter(|s| s.link.is_some()).count() == 1, "javascript: is not a link: {s:?}");
        assert!(s.iter().any(|s| s.dim && s.text.contains("javascript:alert(1)")));
        assert_eq!(domain_of("ftp://x.com"), None);
        assert_eq!(domain_of("https://"), None);
        assert_eq!(domain_of("http://[::1]:80/a").as_deref(), Some("[::1]"));
    }

    #[test]
    fn html_is_text_and_images_are_their_alt_text() {
        let b = parse("<script>alert(1)</script>\n\nhi <b>there</b> ![alt text](https://evil.com/p.png)");
        let all: String = b.iter().flat_map(|b| para(b).iter().map(|s| s.text.clone())).collect();
        assert!(all.contains("<script>alert(1)</script>"), "{all}");
        assert!(all.contains("<b>there</b>") && all.contains("alt text"));
        assert!(!all.contains("evil.com"));
    }

    #[test]
    fn footnote_markers_are_split_out_and_plain_brackets_are_not() {
        let b = parse("Rust is fast [1][12] and [x] is not, a[0] too, [1234] neither.");
        let s = para(&b[0]);
        let marks: Vec<u32> = s.iter().filter_map(|s| s.footnote).collect();
        assert_eq!(marks, [1, 12, 0]);
        let joined: String = s.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "Rust is fast [1][12] and [x] is not, a[0] too, [1234] neither.");
    }

    #[test]
    fn quotes_and_rules() {
        let b = parse("> quoted\n\n---\n\nafter");
        assert!(matches!(&b[0], Block::Quote(v) if para(&v[0])[0].text == "quoted"));
        assert_eq!(b[1], Block::Rule);
    }

    const SAMPLE: &str = "Intro with **bold**.\n\n1. first\n2. second\n\n```python\nprint(1)\n\nprint(2)\n```\n\nAfter [1].\n\n## End\n\nbye";

    fn stream(chunk: usize) -> Markdown {
        let mut md = Markdown::new();
        let mut i = 0;
        while i < SAMPLE.len() {
            let mut j = (i + chunk).min(SAMPLE.len());
            while !SAMPLE.is_char_boundary(j) {
                j += 1;
            }
            md.push_str(&SAMPLE[i..j]);
            i = j;
            md.settle();
        }
        md
    }

    #[test]
    fn streaming_in_any_chunking_ends_at_the_full_parse() {
        let want = parse(SAMPLE);
        for chunk in [1, 2, 3, 7, 16, 1000] {
            let md = stream(chunk);
            assert_eq!(md.blocks().cloned().collect::<Vec<_>>(), want, "chunk {chunk}");
        }
    }

    #[test]
    fn finished_blocks_are_kept_and_only_the_tail_is_reparsed() {
        let md = stream(1);
        assert!(md.finished() >= 4, "{}", md.finished());
        let mut md = Markdown::new();
        md.push_str("One.\n\nTwo is being wri");
        md.settle();
        assert_eq!(md.finished(), 1);
        let before = md.done.clone();
        md.push_str("tten");
        assert!(md.is_dirty());
        md.settle();
        assert!(!md.is_dirty());
        assert_eq!(md.done, before);
        assert_eq!(para(md.blocks().last().unwrap())[0].text, "Two is being written");
    }

    #[test]
    fn a_partial_fence_stays_code_while_streaming_and_blank_lines_inside_do_not_commit() {
        let mut md = Markdown::new();
        md.push_str("Look:\n\n```\nline one\n\nline");
        md.settle();
        assert_eq!(md.finished(), 1, "only the paragraph before the fence is final");
        let last = md.blocks().last().unwrap();
        assert!(matches!(last, Block::Code { closed: false, text, .. } if text == "line one\n\nline"), "{last:?}");
        md.push_str(" two\n```\n");
        md.settle();
        assert!(matches!(md.blocks().last().unwrap(), Block::Code { closed: true, .. }));
    }

    #[test]
    fn a_list_is_not_split_by_a_blank_line_between_items() {
        let mut md = Markdown::new();
        md.push_str("1. a\n\n2. b\n\n3. c");
        md.settle();
        assert_eq!(md.blocks().count(), 1);
        assert!(matches!(md.blocks().next().unwrap(), Block::List { start: Some(1), items } if items.len() == 3));
    }

    #[test]
    fn unsettled_text_is_not_parsed_and_empty_is_empty() {
        let mut md = Markdown::new();
        md.push_str("**x**");
        assert_eq!(md.blocks().count(), 0);
        md.settle();
        assert_eq!(md.blocks().count(), 1);
        assert_eq!(Markdown::new().blocks().count(), 0);
    }
}
