//! Morph's own address-bar schemes, which never become a web search:
//!
//! - `morph://<page>`: Morph's own pages and panels ([`PAGES`]). An unknown
//!   one opens an error page listing the real ones ([`unknown_page_text`]).
//! - `weirdnet-<app>://…`: a Weirdnet app link, called over Weirdnet (never
//!   handed to a local OS handler) through [`crate::app_call`].
//!
//! Every shell (GPUI desktop, Flutter mobile through morph-bridge) asks these
//! functions, so `morph://settings` means the same everywhere.

/// One `morph://` page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// The name after `morph://`, lowercase.
    pub name: &'static str,
    /// What it opens, for the list on the error page.
    pub about: &'static str,
}

/// Morph's own pages, in the order the error page lists them.
pub const PAGES: [Page; 9] = [
    Page { name: "about", about: "About Morph: version and licences" },
    Page { name: "bookmarks", about: "Your bookmarks" },
    Page { name: "downloads", about: "Downloads settings" },
    Page { name: "history", about: "Pages you visited" },
    Page { name: "identity", about: "Your Morph identity and linked devices" },
    Page { name: "profiles", about: "Your profiles" },
    Page { name: "relays", about: "Relay status and Check now" },
    Page { name: "search", about: "Search engine settings" },
    Page { name: "settings", about: "All settings" },
];

/// Whether `scheme` (without `:`) is one the address bar keeps for Morph:
/// `morph` or any `weirdnet-<app>`.
pub fn is_own_scheme(scheme: &str) -> bool {
    scheme.eq_ignore_ascii_case("morph") || is_app_scheme(scheme)
}

/// Whether `scheme` is a Weirdnet scheme: `weirdnet-` and an app name, or
/// bare `weirdnet` (internal to Weirdnet: it gets an error page, not a
/// search).
pub fn is_app_scheme(scheme: &str) -> bool {
    let lower = scheme.to_ascii_lowercase();
    lower == "weirdnet" || lower.strip_prefix("weirdnet-").is_some_and(|name| !name.is_empty())
}

/// The scheme of `text` (`morph` for `morph://x` and `morph:x`), if it has
/// one before a `:`.
pub fn scheme_of(text: &str) -> Option<&str> {
    let text = text.trim();
    let (scheme, _) = text.split_once(':')?;
    let mut chars = scheme.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    ok.then_some(scheme)
}

/// Whether `text` is a `morph:` address.
pub fn is_morph(text: &str) -> bool {
    scheme_of(text).is_some_and(|s| s.eq_ignore_ascii_case("morph"))
}

/// Whether `text` is a `weirdnet-<app>:` address.
pub fn is_app(text: &str) -> bool {
    scheme_of(text).is_some_and(is_app_scheme)
}

/// What a `morph:` address names: `Some(Ok(page))` for one of [`PAGES`]
/// (`morph://Settings/`, `morph:history`), `Some(Err(name))` for an unknown
/// one, `None` when `text` is not a `morph:` address.
pub fn page(text: &str) -> Option<Result<&'static Page, String>> {
    let text = text.trim();
    if !is_morph(text) {
        return None;
    }
    let rest = &text["morph:".len()..];
    let name = rest.trim_start_matches('/').split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    let lower = name.to_ascii_lowercase();
    Some(PAGES.iter().find(|p| p.name == lower).ok_or_else(|| name.chars().take(60).collect()))
}

/// The address of `page`.
pub fn page_url(page: &Page) -> String {
    format!("morph://{}", page.name)
}

/// Title and body of the error page for an unknown `morph:` address
/// (`name` as [`page`] gave it): it lists every real page.
pub fn unknown_page_text(name: &str) -> (String, String) {
    let shown = if name.is_empty() { "morph://".to_string() } else { format!("morph://{name}") };
    let list = PAGES.iter().map(|p| format!("{} — {}", page_url(p), p.about)).collect::<Vec<_>>().join("\n");
    (format!("{shown} is not a Morph page"), format!("Morph has these pages:\n{list}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morph_pages_parse_in_any_usual_form() {
        for text in ["morph://settings", "morph:settings", " MORPH://Settings/ ", "morph:///settings?x#y"] {
            assert_eq!(page(text).unwrap().unwrap().name, "settings", "{text}");
        }
        for p in PAGES {
            assert_eq!(page(&page_url(&p)), Some(Ok(&PAGES[PAGES.iter().position(|q| q == &p).unwrap()])));
        }
        assert_eq!(page("morph://nope"), Some(Err("nope".into())));
        assert_eq!(page("morph://"), Some(Err(String::new())));
        assert_eq!(page("https://morph.example"), None);
        assert_eq!(page("morphing"), None);
    }

    #[test]
    fn the_unknown_page_lists_every_real_one() {
        let (title, body) = unknown_page_text("nope");
        assert_eq!(title, "morph://nope is not a Morph page");
        for p in PAGES {
            assert!(body.contains(&page_url(&p)), "{}", p.name);
        }
    }

    #[test]
    fn own_schemes_are_morph_and_any_weirdnet_app() {
        for s in ["morph", "MORPH", "weirdnet", "weirdnet-morph", "weirdnet-muninn", "Weirdnet-Notes"] {
            assert!(is_own_scheme(s), "{s}");
        }
        for s in ["weirdnet-", "https", "morphx", "x-weirdnet-a"] {
            assert!(!is_own_scheme(s), "{s}");
        }
        assert!(is_app("weirdnet-muninn://home.example.com/a"));
        assert!(is_app("weirdnet://x") && !is_app("morph://settings"));
    }
}
