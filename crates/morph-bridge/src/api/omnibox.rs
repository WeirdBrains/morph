//! The address-bar rule for the Flutter shell: the same `morph-core` rule the
//! desktop shell uses, so both open the same thing for the same text, with
//! the same search engines and the same `morph://` and `weirdnet-<app>://`
//! handling.

use std::sync::atomic::AtomicBool;

use morph_core::address;
use morph_core::app_call::{self, AppCallError, AppCaller};
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
    /// A `weirdnet-<app>://` address to call over Weirdnet ([`open_app`]).
    pub app: bool,
    /// Otherwise Morph's error page: its title …
    pub title: String,
    /// … and explanation.
    pub body: String,
}

/// Whether `url` (as [`omnibox_target_for`] gave it) is Morph's to open,
/// and how: a `morph://` page, the page listing them for an unknown one, a
/// `weirdnet-<app>://` link to call over Weirdnet (`app`; the shell calls
/// [`open_app`]), or the error page for an app link that can't be called
/// (`weirdnet://`, an invalid app name). `None` for anything the web view
/// loads. None of them is ever a search or handed to the OS.
#[flutter_rust_bridge::frb(sync)]
pub fn own_address(url: String) -> Option<OwnAddressView> {
    if let Some(page) = address::page(&url) {
        return Some(match page {
            Ok(page) => OwnAddressView { page: Some(page.name.into()), app: false, title: String::new(), body: String::new() },
            Err(name) => {
                let (title, body) = address::unknown_page_text(&name);
                OwnAddressView { page: None, app: false, title, body }
            }
        });
    }
    if !address::is_app(&url) {
        return None;
    }
    Some(match app_call::request(&url) {
        Ok(_) => OwnAddressView { page: None, app: true, title: String::new(), body: String::new() },
        Err(e) => {
            let (title, body) = app_call::error_text(&url, &e);
            OwnAddressView { page: None, app: false, title, body }
        }
    })
}

/// An app's answer for the web view, or why there is none.
pub struct AppPageView {
    /// The `data:` address to load (marked with a token: the shell shows
    /// the app address for it), or `None` when the call failed.
    pub data_url: Option<String>,
    /// The page's title, or the error page's.
    pub title: String,
    /// The error page's explanation (empty for a page).
    pub body: String,
    /// The app's status (0 when it did not answer).
    pub status: u16,
}

/// The most of a `data:` address Android's WebView loads (Chromium's 2 MB
/// URL limit); WKWebView takes more.
const DATA_URL_MAX: Option<usize> = if cfg!(target_os = "android") { Some(2_000_000) } else { None };

/// Calls the app a `weirdnet-<app>://` address names over Weirdnet and
/// renders its answer (HTML, images, JSON, text; at most
/// `morph_core::app_call::BODY_MAX`). Blocks while the peer is found and
/// answers, on the bridge's worker thread, never the UI's.
pub fn open_app(url: String) -> AppPageView {
    open_app_with(&url, morph_services::app_calls::shared().as_ref())
}

fn open_app_with(url: &str, caller: &dyn AppCaller) -> AppPageView {
    let stop = AtomicBool::new(false);
    let page = app_call::fetch(url, caller, &stop, &mut |_| {})
        .and_then(|p| if DATA_URL_MAX.is_some_and(|max| p.data_url.len() > max) { Err(AppCallError::TooLarge) } else { Ok(p) });
    match page {
        Ok(p) => AppPageView { data_url: Some(p.data_url), title: p.title, body: String::new(), status: p.status },
        Err(e) => {
            let (title, body) = app_call::error_text(url, &e);
            AppPageView { data_url: None, title, body, status: 0 }
        }
    }
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
        assert!(app.app && app.page.is_none() && app.title.is_empty());
        let bad = own_address("weirdnet-No.Good://alice/a".into()).unwrap();
        assert!(!bad.app);
        assert_eq!(bad.title, "Not a valid Weirdnet app name");
        let internal = own_address("weirdnet://x/y".into()).unwrap();
        assert_eq!((internal.app, internal.title.as_str()), (false, "weirdnet:// can't be opened"));
        assert!(own_address("https://example.com".into()).is_none());
    }

    #[test]
    fn app_calls_give_a_page_or_the_reason() {
        let none = open_app_with("weirdnet-muninn://alice/a?token=x", &app_call::NotYet);
        assert_eq!((none.data_url, none.title.as_str()), (None, "App calls aren't available"));
        assert!(!none.body.contains("token") && none.body.contains("not sent to a search engine"));
        struct Text;
        impl AppCaller for Text {
            fn call(&self, _r: app_call::AppRequest) -> Result<app_call::AppResponse, AppCallError> {
                Ok(app_call::AppResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "text/plain".into())],
                    body: Box::new(std::io::Cursor::new(b"hello".to_vec())),
                })
            }
        }
        let page = open_app_with("weirdnet-notes://alice/a", &Text);
        let data = page.data_url.unwrap();
        assert!(app_call::page_token(&data).is_some());
        assert_eq!((page.title.as_str(), page.status), ("weirdnet-notes://alice/a", 200));
    }
}
