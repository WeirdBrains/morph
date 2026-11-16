//! Search engines the address bar can send phrases to. The list is data, so
//! adding one is a new entry here; settings store an engine's `id`.

/// One search engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Engine {
    /// Stable id kept in settings (`google`).
    pub id: &'static str,
    /// Name shown in menus.
    pub name: &'static str,
    /// The results page; `{q}` is replaced by the encoded query.
    pub template: &'static str,
    /// A short mark for the selector button when no icon is drawn.
    pub mark: &'static str,
}

/// Engines in menu order; the first is the default.
pub const ENGINES: [Engine; 2] = [
    Engine { id: "google", name: "Google", template: "https://www.google.com/search?q={q}", mark: "G" },
    Engine { id: "duckduckgo", name: "DuckDuckGo", template: "https://duckduckgo.com/?q={q}", mark: "D" },
];

/// The default engine's id.
pub const DEFAULT_ENGINE: &str = ENGINES[0].id;

/// The engine with `id`, else the default.
pub fn engine(id: &str) -> &'static Engine {
    ENGINES.iter().find(|e| e.id == id).unwrap_or(&ENGINES[0])
}

/// Whether `id` names a known engine.
pub fn is_known(id: &str) -> bool {
    ENGINES.iter().any(|e| e.id == id)
}

impl Engine {
    /// The results page for `query`: whitespace collapsed, capped at
    /// 2000 characters, form-encoded.
    pub fn url(&self, query: &str) -> String {
        self.template.replace("{q}", &crate::omnibox::encoded_query(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_is_the_default_and_unknown_ids_fall_back_to_it() {
        assert_eq!(DEFAULT_ENGINE, "google");
        assert_eq!(engine("duckduckgo").name, "DuckDuckGo");
        assert_eq!(engine("nope").id, "google");
        assert!(is_known("google") && is_known("duckduckgo") && !is_known("bing"));
    }

    #[test]
    fn engines_build_their_results_pages() {
        assert_eq!(engine("google").url("c++  rust"), "https://www.google.com/search?q=c%2B%2B+rust");
        assert_eq!(engine("duckduckgo").url("café"), "https://duckduckgo.com/?q=caf%C3%A9");
        let ids: Vec<_> = ENGINES.iter().map(|e| e.id).collect();
        let mut unique = ids.clone();
        unique.dedup();
        assert_eq!(ids, unique);
    }
}
