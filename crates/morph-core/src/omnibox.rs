//! What typing into the address bar opens: a web address when the text looks
//! like one, else a web search. Ported from the Flutter shell's
//! `omnibox.dart` and `MorphIncomingLinks.searchUrl` (ARCHITECTURE.md,
//! "Address bar: phrases are searches") so every shell shares one rule.

/// Schemes opened as typed, without a host check.
const DIRECT_SCHEMES: [&str; 4] = ["about", "file", "data", "view-source"];

/// Longest query sent to the search engine, in characters.
const MAX_QUERY_CHARS: usize = 2000;

/// The address `input` opens, or `None` for blank input.
///
/// Text opens as an address only when it looks like one: http(s) with a host
/// and no spaces, `about:`, `file:`, `data:`, `view-source:`,
/// `localhost[:port]`, an IPv4 address, or a bare `host.tld[:port][/path]`.
/// `morph:` and `weirdnet-<app>:` addresses are kept as typed for Morph to
/// open ([`crate::address`]). Everything else, including any phrase and
/// unknown schemes such as `javascript:`, becomes a search.
pub fn target(input: &str) -> Option<String> {
    target_with(input, &|q| search_url(q))
}

/// [`target`], with phrases sent to `engine` (see [`crate::search`]).
pub fn target_for(input: &str, engine: &crate::search::Engine) -> Option<String> {
    target_with(input, &|q| engine.url(q))
}

fn target_with(input: &str, search_url: &dyn Fn(&str) -> String) -> Option<String> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(scheme) = scheme(text) {
        // morph://settings and weirdnet-<app>://… are Morph's to open (an
        // error page if it can't), never a search.
        if crate::address::is_own_scheme(scheme) {
            return Some(text.to_string());
        }
        let scheme = scheme.to_ascii_lowercase();
        if scheme == "http" || scheme == "https" {
            if has_host(text) && !has_whitespace(text) {
                return Some(text.to_string());
            }
            return Some(search_url(text));
        }
        if DIRECT_SCHEMES.contains(&scheme.as_str()) {
            return Some(text.to_string());
        }
    }
    // A phrase is a search, even if one of its words looks like a host.
    if has_whitespace(text) {
        return Some(search_url(text));
    }
    if is_localhost(text) || is_ipv4(text) {
        return Some(format!("http://{text}"));
    }
    if is_bare_host(text) {
        return Some(format!("https://{text}"));
    }
    Some(search_url(text))
}

/// Morph's default web search (DuckDuckGo) until a search-engine setting
/// exists. Whitespace is collapsed and the query capped at 2000 characters.
pub fn search_url(query: &str) -> String {
    format!("https://duckduckgo.com/?q={}", encoded_query(query))
}

/// `query` with whitespace collapsed, capped at 2000 characters and
/// form-encoded, ready for a search engine's address.
pub fn encoded_query(query: &str) -> String {
    let collapsed = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let capped: String = collapsed.chars().take(MAX_QUERY_CHARS).collect();
    encode_query_component(&capped)
}

/// Form-style query encoding, as Dart's `Uri.encodeQueryComponent`: unreserved
/// characters stay, a space is `+`, everything else is `%XX` of its UTF-8.
fn encode_query_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `scheme:` at the start, followed by `//` or by anything but a digit, so a
/// bare `host:port` such as `example.com:8080` is not a scheme.
fn scheme(text: &str) -> Option<&str> {
    let colon = text.find(':')?;
    let name = &text[..colon];
    let mut chars = name.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
        return None;
    }
    let rest = &text[colon + 1..];
    if rest.starts_with("//") || !rest.starts_with(|c: char| c.is_ascii_digit()) { Some(name) } else { None }
}

/// An http(s) URL whose authority has a non-empty host.
fn has_host(text: &str) -> bool {
    let Some(start) = text.find("://") else {
        return false;
    };
    let rest = &text[start + 3..];
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let host = if host_port.starts_with('[') {
        host_port.split(']').next().map(|h| h.trim_start_matches('[')).unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    !host.is_empty()
}

fn has_whitespace(text: &str) -> bool {
    text.chars().any(char::is_whitespace)
}

/// Splits `text` into the part before the first `/`, `?` or `#` and the rest.
fn split_path(text: &str) -> (&str, &str) {
    match text.find(['/', '?', '#']) {
        Some(i) => (&text[..i], &text[i..]),
        None => (text, ""),
    }
}

/// `:digits` or nothing.
fn is_port_suffix(rest: &str) -> bool {
    match rest.strip_prefix(':') {
        None => rest.is_empty(),
        Some(digits) => !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()),
    }
}

/// `localhost[:port][/path]`, any case.
fn is_localhost(text: &str) -> bool {
    let (authority, path) = split_path(text);
    if !path.is_empty() && !path.starts_with('/') {
        return false;
    }
    let lower = authority.to_ascii_lowercase();
    lower.strip_prefix("localhost").is_some_and(is_port_suffix)
}

/// Four dot-separated groups of one to three digits, `[:port][/path]`.
fn is_ipv4(text: &str) -> bool {
    let (authority, path) = split_path(text);
    if !path.is_empty() && !path.starts_with('/') {
        return false;
    }
    let (address, port) = match authority.find(':') {
        Some(i) => (&authority[..i], &authority[i..]),
        None => (authority, ""),
    };
    let groups: Vec<&str> = address.split('.').collect();
    groups.len() == 4 && groups.iter().all(|g| (1..=3).contains(&g.len()) && g.chars().all(|c| c.is_ascii_digit())) && is_port_suffix(port)
}

/// `label(.label)*.tld[.][:port][/?#rest]`: labels of letters, digits and
/// hyphens, a final label of 2 to 63 letters.
fn is_bare_host(text: &str) -> bool {
    let (authority, _rest) = split_path(text);
    let (host, port) = match authority.find(':') {
        Some(i) => (&authority[..i], &authority[i..]),
        None => (authority, ""),
    };
    if !is_port_suffix(port) {
        return false;
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let (tld, rest) = labels.split_last().expect("at least two labels");
    let label_ok = |l: &&str| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    rest.iter().all(label_ok) && (2..=63).contains(&tld.len()) && tld.chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::{search_url, target};

    // The same cases as the Flutter shell's `omnibox_test.dart`.
    fn ddg(q: &str) -> Option<String> {
        Some(search_url(q))
    }

    #[test]
    fn a_phrase_is_a_search_not_a_percent_encoded_url() {
        let phrase = "what do i need to install to get youtube working";
        assert_eq!(target(phrase), ddg(phrase));
        assert_eq!(target("  how to fix example.com  "), ddg("how to fix example.com"));
        assert_eq!(target("flutter"), ddg("flutter"));
        assert_eq!(target("c++"), ddg("c++"));
        assert_eq!(target("what is 2.5"), ddg("what is 2.5"));
    }

    #[test]
    fn addresses_open_directly() {
        assert_eq!(target("instagram.com").as_deref(), Some("https://instagram.com"));
        assert_eq!(target("www.youtube.com/watch?v=x").as_deref(), Some("https://www.youtube.com/watch?v=x"));
        assert_eq!(target("example.com:8080/a").as_deref(), Some("https://example.com:8080/a"));
        assert_eq!(target("https://example.com/a b"), ddg("https://example.com/a b"));
        assert_eq!(target("http://example.com").as_deref(), Some("http://example.com"));
        assert_eq!(target("HTTPS://Example.com/x").as_deref(), Some("HTTPS://Example.com/x"));
        assert_eq!(target("localhost:3000").as_deref(), Some("http://localhost:3000"));
        assert_eq!(target("192.168.1.1/admin").as_deref(), Some("http://192.168.1.1/admin"));
        assert_eq!(target("about:blank").as_deref(), Some("about:blank"));
        assert_eq!(target("file:///tmp/a.html").as_deref(), Some("file:///tmp/a.html"));
    }

    #[test]
    fn unsafe_or_unknown_schemes_search_instead_of_running() {
        assert_eq!(target("javascript:alert(1)"), ddg("javascript:alert(1)"));
        assert_eq!(target("https://"), ddg("https://"));
    }

    #[test]
    fn morph_and_weirdnet_addresses_are_never_searches() {
        for text in [
            "morph://settings",
            "morph:history",
            "morph://nope",
            "MORPH://Relays/",
            "weirdnet-morph://link/ABCD",
            "weirdnet-muninn://home.example.com/notes/a.md?x=1",
            "Weirdnet-Notes://alice",
            "weirdnet://open/x",
            "weirdnet-notes://a b",
        ] {
            assert_eq!(target(text).as_deref(), Some(text), "{text}");
            let google = crate::search::engine("google");
            assert_eq!(super::target_for(&format!("  {text} "), google).as_deref(), Some(text), "{text}");
        }
        // Not Morph's: a phrase mentioning one, and look-alike schemes.
        assert_eq!(target("open morph://settings"), ddg("open morph://settings"));
        assert_eq!(target("morphx://a"), ddg("morphx://a"));
        assert_eq!(target("weirdnet-://a"), ddg("weirdnet-://a"));
    }

    #[test]
    fn blank_input_does_nothing() {
        assert_eq!(target(""), None);
        assert_eq!(target("   "), None);
    }

    #[test]
    fn search_urls_encode_like_dart_uri_https() {
        // Uri.https('duckduckgo.com', '/', {'q': ...}).toString() on main.
        assert_eq!(search_url("c++"), "https://duckduckgo.com/?q=c%2B%2B");
        assert_eq!(search_url("  what   is\t2.5 "), "https://duckduckgo.com/?q=what+is+2.5");
        assert_eq!(search_url("javascript:alert(1)"), "https://duckduckgo.com/?q=javascript%3Aalert%281%29");
        assert_eq!(search_url("café"), "https://duckduckgo.com/?q=caf%C3%A9");
        assert_eq!(search_url(&"a".repeat(2500)).len(), "https://duckduckgo.com/?q=".len() + 2000);
    }

    #[test]
    fn phrases_go_to_the_chosen_engine_and_addresses_stay_addresses() {
        let google = crate::search::engine("google");
        assert_eq!(super::target_for("rust gpui", google).as_deref(), Some("https://www.google.com/search?q=rust+gpui"));
        assert_eq!(super::target_for("example.com", google).as_deref(), Some("https://example.com"));
        let ddg = crate::search::engine("duckduckgo");
        assert_eq!(super::target_for("rust", ddg).as_deref(), Some("https://duckduckgo.com/?q=rust"));
        assert_eq!(super::target_for("  ", ddg), None);
    }
}
