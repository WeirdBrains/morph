//! `weirdnet-morph://` links: Morph registers the scheme with every OS
//! (Linux desktop file, macOS Info.plist, Windows registry) so a link
//! clicked in another app reaches the running Morph through the
//! single-instance handoff (Apple Events on macOS).
//!
//! What such a link *means* is v0.3's (peer-to-peer content, PRIORITIES.md
//! roadmap). For now Morph only parses and routes it: [`parse`] checks the
//! form and [`Link::describe`] gives a log-safe form (no query, no
//! fragment, control characters dropped, length capped), so a link carrying
//! tokens never lands in a log whole.

/// The scheme, without `://`.
pub const SCHEME: &str = "weirdnet-morph";

/// The longest description [`Link::describe`] gives.
const DESCRIBE_MAX: usize = 120;

/// A parsed `weirdnet-morph://host/path?query#fragment` link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Lowercased host part (a peer or name, once v0.3 says which).
    pub host: String,
    /// Path segments, empty ones dropped.
    pub path: Vec<String>,
    /// Whether the link had a query or fragment (kept out of logs).
    pub has_query: bool,
}

/// What Morph does with a link today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// No handler knows this link yet; the shell logs [`Link::describe`]
    /// and tells the user.
    Unknown(Link),
}

/// Whether `text` is a `weirdnet-morph:` link (any case).
pub fn is_link(text: &str) -> bool {
    let text = text.trim();
    text.len() > SCHEME.len() && text[..SCHEME.len()].eq_ignore_ascii_case(SCHEME) && text[SCHEME.len()..].starts_with(':')
}

/// Parse a `weirdnet-morph://` link; `None` when it is not one or is
/// malformed (no `//`, control characters, an empty host, too long).
pub fn parse(text: &str) -> Option<Link> {
    let text = text.trim();
    if !is_link(text) || text.len() > 4096 || text.chars().any(char::is_control) {
        return None;
    }
    let rest = text[SCHEME.len() + 1..].strip_prefix("//")?;
    let cut = rest.find(['?', '#']).unwrap_or(rest.len());
    let (main, tail) = rest.split_at(cut);
    let (host, path) = main.split_once('/').unwrap_or((main, ""));
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':')) {
        return None;
    }
    Some(Link {
        host: host.to_ascii_lowercase(),
        path: path.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect(),
        has_query: !tail.is_empty(),
    })
}

/// Route a link. Nothing is handled yet (v0.3), so every link is unknown.
pub fn route(link: Link) -> Route {
    Route::Unknown(link)
}

impl Link {
    /// A log-safe form: scheme, host and path only, capped in length.
    pub fn describe(&self) -> String {
        let mut out = format!("{SCHEME}://{}", self.host);
        for segment in &self.path {
            out.push('/');
            out.push_str(segment);
        }
        if self.has_query {
            out.push_str("?…");
        }
        if out.chars().count() > DESCRIBE_MAX {
            out = out.chars().take(DESCRIBE_MAX).collect::<String>() + "…";
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_recognised_in_any_case() {
        assert!(is_link("weirdnet-morph://peer/a"));
        assert!(is_link(" WeirdNet-Morph://peer "));
        assert!(!is_link("weirdnet-morphx://peer"));
        assert!(!is_link("https://weirdnet-morph.example"));
        assert!(!is_link("weirdnet-morph"));
    }

    #[test]
    fn host_path_and_query_are_split() {
        let link = parse("weirdnet-morph://Peer.Example/site//page?token=secret#x").unwrap();
        assert_eq!(link.host, "peer.example");
        assert_eq!(link.path, ["site", "page"]);
        assert!(link.has_query);
        assert_eq!(parse("weirdnet-morph://peer").unwrap().path, Vec::<String>::new());
    }

    #[test]
    fn malformed_links_are_refused() {
        for bad in ["weirdnet-morph:peer", "weirdnet-morph:///path", "weirdnet-morph://pe er/x", "weirdnet-morph://peer/\u{7}", "https://peer"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(parse(&format!("weirdnet-morph://peer/{}", "a".repeat(5000))), None);
    }

    #[test]
    fn descriptions_leave_out_the_query_and_are_capped() {
        let link = parse("weirdnet-morph://peer/site?token=secret").unwrap();
        let text = link.describe();
        assert_eq!(text, "weirdnet-morph://peer/site?…");
        assert!(!text.contains("secret"));
        let long = parse(&format!("weirdnet-morph://peer/{}", "b".repeat(400))).unwrap().describe();
        assert!(long.chars().count() <= DESCRIBE_MAX + 1);
    }

    #[test]
    fn every_link_routes_to_unknown_for_now() {
        let link = parse("weirdnet-morph://peer/x").unwrap();
        assert_eq!(route(link.clone()), Route::Unknown(link));
    }
}
