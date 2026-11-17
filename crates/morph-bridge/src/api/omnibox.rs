//! The address-bar rule for the Flutter shell: the same `morph-core` rule the
//! desktop shell uses, so both open the same thing for the same text, with
//! the same search engines and the same `morph://` and `weirdnet-<app>://`
//! handling.

use morph_core::address;
use morph_core::app_call::{self, AppCallError, AppCaller, NotYet};
use morph_core::search;

/// What typing `input` into the address bar opens: a web address when the
/// text looks like one, else a search. `None` for blank input.
#[flutter_rust_bridge::frb(sync)]
pub fn omnibox_target(input: String) -> Option<String> {
    morph_core::omnibox::target(&input)
}

/// [`omnibox_target`], with phrases sent to the search engine `engine` (an
/// id from [`search_engines`]; unknown ids use the default).
#[flutter_rust_bridge::frb(sync)]
pub fn omnibox_target_for(input: String, engine: String) -> Option<String> {
    morph_core::omnibox::target_for(&input, search::engine(&engine))
}

/// One search engine for the selector beside the address.
pub struct SearchEngineView {
    /// Stable id (`google`).
    pub id: String,
    /// Name shown in the menu.
    pub name: String,
    /// A short mark for the selector button (`G`).
    pub mark: String,
}

/// The search engines, in menu order; the first is the default.
#[flutter_rust_bridge::frb(sync)]
pub fn search_engines() -> Vec<SearchEngineView> {
    search::ENGINES.iter().map(|e| SearchEngineView { id: e.id.into(), name: e.name.into(), mark: e.mark.into() }).collect()
}

/// What Morph opens itself instead of loading an address.
pub struct OwnAddressView {
    /// One of Morph's pages (`settings`, `history`, … as
    /// `morph_core::address::PAGES` lists them), for the shell to open.
    pub page: Option<String>,
    /// Otherwise Morph's error page: its title …
    pub title: String,
    /// … and explanation.
    pub body: String,
}

/// Whether `url` (as [`omnibox_target_for`] gave it) is Morph's to open,
/// and how: a `morph://` page, the page listing them for an unknown one, or
/// for a `weirdnet-<app>://` link the result of calling the app over
/// Weirdnet (in this build always "App call not supported yet"). `None`
/// for anything the web view loads. Neither kind is ever a search or handed
/// to the OS.
#[flutter_rust_bridge::frb(sync)]
pub fn own_address(url: String) -> Option<OwnAddressView> {
    own_address_with(&url, &NotYet)
}

fn own_address_with(url: &str, caller: &dyn AppCaller) -> Option<OwnAddressView> {
    if let Some(page) = address::page(url) {
        return Some(match page {
            Ok(page) => OwnAddressView { page: Some(page.name.into()), title: String::new(), body: String::new() },
            Err(name) => {
                let (title, body) = address::unknown_page_text(&name);
                OwnAddressView { page: None, title, body }
            }
        });
    }
    if !address::is_app(url) {
        return None;
    }
    let (title, body) = match app_call::open(url, caller) {
        Err(e) => app_call::error_text(url, &e),
        // Showing an app's page comes with the app call itself.
        Ok(response) => app_call::error_text(
            url,
            &AppCallError::Refused(format!("it answered ({}), but this version cannot show app pages yet", response.status)),
        ),
    };
    Some(OwnAddressView { page: None, title, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_core_rule() {
        assert_eq!(omnibox_target("example.com".into()).as_deref(), Some("https://example.com"));
        assert_eq!(omnibox_target("how to fix example.com".into()), Some(morph_core::omnibox::search_url("how to fix example.com")));
        assert_eq!(omnibox_target("javascript:alert(1)".into()), Some(morph_core::omnibox::search_url("javascript:alert(1)")));
        assert_eq!(omnibox_target("  ".into()), None);
    }

    #[test]
    fn phrases_go_to_the_chosen_engine() {
        assert_eq!(omnibox_target_for("rust".into(), "google".into()).as_deref(), Some("https://www.google.com/search?q=rust"));
        assert_eq!(omnibox_target_for("rust".into(), "duckduckgo".into()).as_deref(), Some("https://duckduckgo.com/?q=rust"));
        assert_eq!(omnibox_target_for("example.com".into(), "google".into()).as_deref(), Some("https://example.com"));
        let ids: Vec<_> = search_engines().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["google", "duckduckgo"]);
    }

    #[test]
    fn morph_and_weirdnet_addresses_are_morphs_own() {
        for typed in ["morph://settings", "morph://nope", "weirdnet-muninn://alice/a", "weirdnet://x"] {
            assert_eq!(omnibox_target_for(typed.into(), "google".into()).as_deref(), Some(typed), "{typed}");
        }
        let page = own_address("MORPH://Relays/".into()).unwrap();
        assert_eq!(page.page.as_deref(), Some("relays"));
        let unknown = own_address("morph://nope".into()).unwrap();
        assert_eq!((unknown.page, unknown.title.as_str()), (None, "morph://nope is not a Morph page"));
        assert!(unknown.body.contains("morph://settings") && unknown.body.contains("morph://relays"));
        let app = own_address("weirdnet-muninn://alice/a?token=x".into()).unwrap();
        assert_eq!((app.page, app.title.as_str()), (None, "App call not supported yet"));
        assert!(!app.body.contains("token"));
        assert!(own_address("https://example.com".into()).is_none());
    }
}
