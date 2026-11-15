//! Component kinds: what the host can place in a slot.

use serde::{Deserialize, Serialize};

/// A stable component id such as `morph.browser`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KindId(pub String);

impl KindId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for KindId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// What a component needs from the host, so the host can tell the user why
/// one is unavailable and set up shared services only when used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Needs {
    /// A system web view (the browser).
    WebView,
    /// The profile's bookmarks and history.
    Library,
    /// Network access beyond web pages (peers, model downloads).
    Network,
}

/// Describes a component the host can create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentKind {
    pub id: KindId,
    pub title: String,
    /// A short icon name the host maps to its own artwork.
    pub icon: String,
    pub needs: Vec<Needs>,
}

impl ComponentKind {
    pub fn new(id: &str, title: &str, icon: &str, needs: &[Needs]) -> Self {
        Self { id: KindId::new(id), title: title.into(), icon: icon.into(), needs: needs.to_vec() }
    }
}

/// The browser: tabs, address bar and pages.
pub const BROWSER: &str = "morph.browser";

/// Bookmarks and history, with the library's password prompt.
pub const LIBRARY: &str = "morph.library";

/// Kinds that ship with Morph. Later phases add chat, notes and projects
/// here as they are built.
pub fn builtin() -> Vec<ComponentKind> {
    vec![
        ComponentKind::new(BROWSER, "Browser", "globe", &[Needs::WebView, Needs::Library]),
        ComponentKind::new(LIBRARY, "Library", "bookmark", &[Needs::Library]),
    ]
}

/// A set of kinds the host knows how to make, in registration order.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    kinds: Vec<ComponentKind>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `kind`. Registering an id again replaces the earlier entry.
    pub fn register(&mut self, kind: ComponentKind) {
        match self.kinds.iter_mut().find(|k| k.id == kind.id) {
            Some(existing) => *existing = kind,
            None => self.kinds.push(kind),
        }
    }

    pub fn get(&self, id: &KindId) -> Option<&ComponentKind> {
        self.kinds.iter().find(|k| &k.id == id)
    }

    pub fn kinds(&self) -> &[ComponentKind] {
        &self.kinds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_is_a_builtin_component_needing_a_web_view() {
        let kinds = builtin();
        let browser = kinds.iter().find(|k| k.id.as_str() == BROWSER).expect("browser kind");
        assert!(browser.needs.contains(&Needs::WebView));
        let library = kinds.iter().find(|k| k.id.as_str() == LIBRARY).expect("library kind");
        assert_eq!(library.needs, [Needs::Library]);
    }

    #[test]
    fn registering_an_id_again_replaces_it() {
        let mut catalog = Catalog::new();
        catalog.register(ComponentKind::new("x.notes", "Notes", "note", &[]));
        catalog.register(ComponentKind::new("x.chat", "Chat", "chat", &[Needs::Network]));
        catalog.register(ComponentKind::new("x.notes", "Journal", "note", &[]));
        assert_eq!(catalog.kinds().len(), 2);
        assert_eq!(catalog.get(&"x.notes".into()).unwrap().title, "Journal");
        assert!(catalog.get(&"x.missing".into()).is_none());
    }
}
