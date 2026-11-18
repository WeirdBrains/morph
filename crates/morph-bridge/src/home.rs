//! The Morph home as Dart passes it: `None` or an empty string means the
//! default (`MORPH_HOME`, else `~/.morph`).

use std::path::PathBuf;

pub(crate) fn resolve(home: Option<String>) -> Result<PathBuf, String> {
    // Every call from Dart passes through here: the app syncs, this crate's
    // tests do not.
    if !cfg!(test) {
        morph_services::sync::enable_autostart();
    }
    let explicit = home.filter(|h| !h.is_empty()).map(PathBuf::from);
    morph_services::home::resolve(explicit.as_deref()).map_err(|e| e.to_string())
}

pub(crate) fn now_ms() -> i64 {
    morph_services::home::now_ms()
}
