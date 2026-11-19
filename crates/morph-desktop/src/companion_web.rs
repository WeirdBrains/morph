//! Logged-out research over plain HTTP for the companion research host: a fresh connection
//! with no cookie store, so no site sees the user signed in. Search results come from
//! DuckDuckGo's HTML endpoint (no API key) or Google's results page, and pages are turned into
//! text here. Everything returned is untrusted; the tools wrap it before a model sees it.

use std::time::Duration;

use morph_ai::{FetchedPage, HostError, PageLink, PageSnapshot, SearchHit};

/// Bodies larger than this are cut (pages are trimmed further to the token budget).
const MAX_BODY: u64 = 4 * 1024 * 1024;
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Morph/0.3 Safari/605.1.15";

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).user_agent(USER_AGENT).http_status_as_error(false).max_redirects(8).build().into()
}

/// GET `url` with no cookies: (final address, status, body, content type).
pub fn get(url: &str, timeout: Duration) -> Result<(String, u16, String, String), HostError> {
    use ureq::ResponseExt;
    let mut resp = agent(timeout).get(url).header("Accept-Language", "en").call().map_err(|e| HostError::Fetch(e.to_string()))?;
    let status = resp.status().as_u16();
    let final_url = resp.get_uri().to_string();
    let kind = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if !(kind.is_empty() || kind.contains("html") || kind.starts_with("text/") || kind.contains("json") || kind.contains("xml")) {
        return Err(HostError::Fetch(format!("not a text page ({kind})")));
    }
    let body = resp.body_mut().with_config().limit(MAX_BODY).lossy_utf8(true).read_to_string().map_err(|e| HostError::Fetch(e.to_string()))?;
    Ok((final_url, status, body, kind))
}

/// `web.fetch` logged out: the body as text (HTML stripped to its text).
pub fn fetch(url: &str, timeout: Duration) -> Result<FetchedPage, HostError> {
    let (final_url, status, body, kind) = get(url, timeout)?;
    let bytes = body.len() as u64;
    let text = if kind.contains("html") || looks_like_html(&body) { html_to_text(&body) } else { body };
    Ok(FetchedPage { url: final_url, status: Some(status), text, bytes })
}

/// `web.read` logged out: title, main text and links.
pub fn read(url: &str, timeout: Duration) -> Result<(PageSnapshot, Option<u16>), HostError> {
    let (final_url, status, body, kind) = get(url, timeout)?;
    if !(kind.contains("html") || looks_like_html(&body)) {
        let snap = PageSnapshot { url: final_url, title: String::new(), text: body, isolated: true, ..Default::default() };
        return Ok((snap, Some(status)));
    }
    let snap = PageSnapshot {
        title: title_of(&body).unwrap_or_default(),
        text: readable_text(&body),
        links: links_of(&body, &final_url),
        url: final_url,
        // No page script ran at all.
        isolated: true,
        ..Default::default()
    };
    Ok((snap, Some(status)))
}

fn looks_like_html(body: &str) -> bool {
    let head = body.trim_start().get(..256.min(body.trim_start().len())).unwrap_or("").to_ascii_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html") || head.contains("<head")
}

/// Replaces the common entities and numeric references.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.as_bytes().iter().take(12).position(|&b| b == b';');
        let decoded = end.and_then(|e| {
            let name = &tail[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some(' '),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "hellip" => Some('…'),
                "rsquo" => Some('’'),
                "lsquo" => Some('‘'),
                "rdquo" => Some('”'),
                "ldquo" => Some('“'),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Elements whose content is never text.
const SKIP: [&str; 8] = ["script", "style", "noscript", "template", "svg", "head", "iframe", "canvas"];
/// Elements that start a new line.
const BLOCK: [&str; 24] = [
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "tr",
    "table",
    "section",
    "article",
    "header",
    "footer",
    "pre",
    "blockquote",
    "dd",
    "dt",
    "hr",
    "main",
];

fn tag_name(tag: &str) -> (bool, String) {
    let t = tag.trim_start_matches('<');
    let closing = t.starts_with('/');
    let name: String = t.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (closing, name.to_ascii_lowercase())
}

/// The text of an HTML document (or part of one), scripts and styles left out, one line per block.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut skipping: Option<String> = None;
    let mut rest = html;
    while !rest.is_empty() {
        match rest.find('<') {
            Some(i) => {
                if skipping.is_none() {
                    out.push_str(&decode_entities(&rest[..i]));
                }
                let tail = &rest[i..];
                if tail.starts_with("<!--") {
                    rest = tail.find("-->").map(|e| &tail[e + 3..]).unwrap_or("");
                    continue;
                }
                let end = tail.find('>').map(|e| e + 1).unwrap_or(tail.len());
                let (closing, name) = tag_name(&tail[..end]);
                match &skipping {
                    Some(s) if closing && *s == name => skipping = None,
                    Some(_) => {}
                    None if !closing && SKIP.contains(&name.as_str()) && !tail[..end].ends_with("/>") => skipping = Some(name.clone()),
                    None if BLOCK.contains(&name.as_str()) => out.push('\n'),
                    None if name == "td" || name == "th" => out.push(' '),
                    None => {}
                }
                rest = &tail[end..];
            }
            None => {
                if skipping.is_none() {
                    out.push_str(&decode_entities(rest));
                }
                break;
            }
        }
    }
    tidy(&out)
}

/// Folds runs of spaces and drops empty lines.
fn tidy(s: &str) -> String {
    s.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

pub fn title_of(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = start + lower[start..].find('>')? + 1;
    let close = open_end + lower[open_end..].find("</title")?;
    Some(tidy(&decode_entities(&html[open_end..close])).replace('\n', " "))
}

/// The main content: `<main>` or `<article>` when the page has one, else the body.
pub fn readable_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    for tag in ["<main", "<article"] {
        if let Some(start) = lower.find(tag) {
            let close = format!("</{}", &tag[1..]);
            if let Some(end) = lower.rfind(&close)
                && end > start
            {
                let text = html_to_text(&html[start..end]);
                if text.len() > 200 {
                    return text;
                }
            }
        }
    }
    let body = lower.find("<body").map(|i| &html[i..]).unwrap_or(html);
    // Navigation and footers are rarely what research wants.
    strip_elements(body, &["nav", "footer", "aside", "form"])
}

fn strip_elements(html: &str, names: &[&str]) -> String {
    let mut s = html.to_string();
    for name in names {
        loop {
            let lower = s.to_ascii_lowercase();
            let Some(start) = lower.find(&format!("<{name}")) else { break };
            let next = lower.as_bytes().get(start + 1 + name.len()).copied();
            if next.is_some_and(|c| c.is_ascii_alphanumeric()) {
                // `<navbar>` is not `<nav>`; leave it.
                break;
            }
            let Some(rel) = lower[start..].find(&format!("</{name}>")) else { break };
            s.replace_range(start..start + rel + name.len() + 3, "\n");
        }
    }
    html_to_text(&s)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let after = lower[from..].trim_start();
        if !before_ok || !after.starts_with('=') {
            continue;
        }
        let value_start = tag.len() - after.len() + 1;
        let v = tag[value_start..].trim_start();
        let (quote, body) = match v.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), &v[1..]),
            _ => (None, v),
        };
        let end = match quote {
            Some(q) => body.find(q).unwrap_or(body.len()),
            None => body.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(body.len()),
        };
        return Some(decode_entities(&body[..end]));
    }
    None
}

/// Resolves `href` against `base` (absolute, root-relative and protocol-relative addresses).
pub fn absolute(href: &str, base: &str) -> Option<String> {
    let href = href.trim();
    if href.starts_with("https://") || href.starts_with("http://") {
        return Some(href.to_string());
    }
    let (scheme, rest) = base.split_once("://")?;
    if let Some(h) = href.strip_prefix("//") {
        return Some(format!("{scheme}://{h}"));
    }
    let host = rest.split('/').next()?;
    if href.starts_with('/') {
        return Some(format!("{scheme}://{host}{href}"));
    }
    if href.is_empty() || href.starts_with('#') || href.contains(':') {
        return None;
    }
    let dir = match rest.rfind('/') {
        Some(i) if i > host.len() => &rest[..i],
        _ => host,
    };
    Some(format!("{scheme}://{dir}/{href}"))
}

/// Every `<a href>` with its text, absolute, in page order, without repeats.
pub fn links_of(html: &str, base: &str) -> Vec<PageLink> {
    let lower = html.to_ascii_lowercase();
    let mut out: Vec<PageLink> = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<a") {
        let start = from + i;
        from = start + 2;
        if !lower.as_bytes().get(start + 2).is_some_and(|c| c.is_ascii_whitespace()) {
            continue;
        }
        let Some(open_end) = lower[start..].find('>').map(|e| start + e + 1) else { break };
        let Some(href) = attr(&html[start..open_end], "href").and_then(|h| absolute(&h, base)) else { continue };
        let close = lower[open_end..].find("</a").map(|e| open_end + e).unwrap_or(open_end);
        let text = html_to_text(&html[open_end..close]).replace('\n', " ");
        if !out.iter().any(|l| l.href == href) {
            out.push(PageLink { href, text: text.chars().take(200).collect() });
        }
        from = close.max(from);
    }
    out
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(b) => {
                    out.push(b);
                    i += 3;
                }
                None => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// DuckDuckGo's HTML results (`html.duckduckgo.com/html/`), which need no API key.
pub fn parse_duckduckgo(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for block in html.split("class=\"result__a\"").skip(1) {
        let Some(href_at) = block.find("href=\"") else { continue };
        let href_tail = &block[href_at + 6..];
        let raw = decode_entities(&href_tail[..href_tail.find('"').unwrap_or(0)]);
        // Results link through duckduckgo.com/l/?uddg=<address>; ads go through y.js.
        let url = match query_param(&raw, "uddg") {
            Some(u) => u,
            None if raw.starts_with("http") && !raw.contains("duckduckgo.com/y.js") => raw.clone(),
            None => continue,
        };
        if raw.contains("duckduckgo.com/y.js") || url.contains("duckduckgo.com/y.js") {
            continue;
        }
        let title_start = href_tail.find('>').map(|i| i + 1).unwrap_or(0);
        let title_end = href_tail.find("</a>").unwrap_or(title_start);
        let title = html_to_text(&href_tail[title_start..title_end.max(title_start)]).replace('\n', " ");
        let snippet = block
            .find("result__snippet")
            .and_then(|i| {
                let s = &block[i..];
                let open = s.find('>')? + 1;
                let close = s[open..].find("</a>").or_else(|| s[open..].find("</td>")).or_else(|| s[open..].find("</div>"))? + open;
                Some(html_to_text(&s[open..close]).replace('\n', " "))
            })
            .unwrap_or_default();
        if !hits.iter().any(|h: &SearchHit| h.url == url) {
            hits.push(SearchHit { title, url, snippet });
        }
    }
    hits
}

/// Google's plain results page: links of the form `/url?q=<address>` or result anchors with an `<h3>`.
pub fn parse_google(html: &str) -> Vec<SearchHit> {
    let mut hits: Vec<SearchHit> = Vec::new();
    for link in links_of(html, "https://www.google.com/search") {
        let url = if link.href.starts_with("https://www.google.com/url?") {
            match query_param(&link.href, "q").or_else(|| query_param(&link.href, "url")) {
                Some(u) => u,
                None => continue,
            }
        } else {
            link.href.clone()
        };
        let host = url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or("");
        if !url.starts_with("http")
            || host.ends_with("google.com")
            || host.contains(".google.")
            || host.ends_with("gstatic.com")
            || link.text.trim().is_empty()
        {
            continue;
        }
        if !hits.iter().any(|h| h.url == url) {
            hits.push(SearchHit { title: link.text.clone(), url, snippet: String::new() });
        }
    }
    hits
}

/// Searches the web provider `provider` (`duckduckgo` or `google`) logged out.
pub fn search(provider: &str, query: &str, timeout: Duration) -> Result<Vec<SearchHit>, HostError> {
    let q = percent_encode(query);
    let (url, parse): (String, fn(&str) -> Vec<SearchHit>) = match provider {
        "duckduckgo" => (format!("https://html.duckduckgo.com/html/?q={q}"), parse_duckduckgo),
        "google" => (format!("https://www.google.com/search?q={q}&hl=en&gbv=1"), parse_google),
        other => return Err(HostError::Search(format!("unknown provider {other}"))),
    };
    let (_, status, body, _) = get(&url, timeout).map_err(|e| HostError::Search(e.to_string()))?;
    if status >= 400 {
        return Err(HostError::Search(format!("{provider} answered {status}")));
    }
    Ok(parse(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_text_without_scripts_styles_or_comments() {
        let html = "<html><head><title>T &amp; U</title><style>p{}</style></head><body><nav>Menu</nav><script>alert(1)</script>\
                    <!-- hidden --><h1>Hello&nbsp;world</h1><p>One &lt;two&gt; &#x41;&#66;</p><ul><li>a</li><li>b</li></ul></body></html>";
        assert_eq!(html_to_text(html), "Menu\nHello world\nOne <two> AB\na\nb");
        assert_eq!(title_of(html).as_deref(), Some("T & U"));
        assert_eq!(readable_text(html), "Hello world\nOne <two> AB\na\nb");
    }

    #[test]
    fn main_or_article_is_preferred_when_it_has_content() {
        let long = "word ".repeat(60);
        let html = format!("<body><div>chrome</div><article><p>{long}</p></article><footer>f</footer></body>");
        assert!(readable_text(&html).starts_with("word word"));
        assert!(!readable_text(&html).contains("chrome"));
    }

    #[test]
    fn links_resolve_against_the_page() {
        let html = r##"<a href="/docs">Docs</a> <a class="x" href='https://b.example/p?a=1&amp;b=2'>B <b>bold</b></a> <a href="#top">top</a> <a href="rel.html">R</a> <a href="//c.example/">C</a> <abbr>x</abbr>"##;
        let links = links_of(html, "https://a.example/dir/page.html");
        let hrefs: Vec<&str> = links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, vec!["https://a.example/docs", "https://b.example/p?a=1&b=2", "https://a.example/dir/rel.html", "https://c.example/"]);
        assert_eq!(links[1].text, "B bold");
    }

    #[test]
    fn duckduckgo_results_unwrap_their_redirects_and_skip_ads() {
        let html = r#"
            <div class="result results_links"><a rel="nofollow" class="result__a" href="https://duckduckgo.com/y.js?ad_provider=x&amp;u3=https%3A%2F%2Fad.example">Ad</a></div>
            <div class="result"><h2><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Ftokio.rs%2Ftokio%2Ftopics%2Fshutdown&amp;rut=abc">Graceful <b>Shutdown</b> | Tokio</a></h2>
            <a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">Use a <b>CancellationToken</b> to shut down.</a></div>
            <div class="result"><a rel="nofollow" class="result__a" href="https://docs.rs/tokio-util">tokio-util</a></div>"#;
        let hits = parse_duckduckgo(html);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://tokio.rs/tokio/topics/shutdown");
        assert_eq!(hits[0].title, "Graceful Shutdown | Tokio");
        assert_eq!(hits[0].snippet, "Use a CancellationToken to shut down.");
        assert_eq!(hits[1].url, "https://docs.rs/tokio-util");
    }

    #[test]
    fn google_results_keep_outside_links_only() {
        let html = r#"<a href="/url?q=https://tokio.rs/&amp;sa=U">Tokio</a><a href="https://maps.google.com/">Maps</a><a href="/search?q=x">More</a><a href="https://docs.rs/">Docs</a>"#;
        let urls: Vec<String> = parse_google(html).into_iter().map(|h| h.url).collect();
        assert_eq!(urls, vec!["https://tokio.rs/", "https://docs.rs/"]);
    }

    #[test]
    fn queries_encode_and_decode() {
        assert_eq!(percent_encode("tokio cancel & drop"), "tokio+cancel+%26+drop");
        assert_eq!(percent_decode("https%3A%2F%2Fa.b%2Fc+d"), "https://a.b/c d");
        assert_eq!(decode_entities("a &amp b &#9731; &bogus;"), "a &amp b ☃ &bogus;");
    }
}
