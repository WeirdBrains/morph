//! The page side of the macOS and Windows backend (`wry_native`): the
//! script every page gets, the messages it sends and the scripts the backend
//! runs in pages. Free of any engine, so it is tested on every platform.

/// A message from [`PAGE_SCRIPT`] or the page bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// The page bridge posted this (JSON).
    Bridge(String),
    /// The page took focus.
    Focused,
    /// A shell shortcut was pressed in the page (canonical notation).
    Shortcut(String),
    /// The page changed its own address (`pushState`, `replaceState`, `popstate`, a hash change).
    Url(String),
    /// Matches found by [`crate::WebView::find`].
    Found(u32),
}

impl Message {
    pub fn parse(body: &str) -> Option<Self> {
        let (kind, rest) = body.split_at_checked(2)?;
        match kind {
            "b:" => Some(Message::Bridge(rest.to_string())),
            "f:" => Some(Message::Focused),
            "k:" if !rest.is_empty() => Some(Message::Shortcut(rest.to_string())),
            "u:" if !rest.is_empty() => Some(Message::Url(rest.to_string())),
            "n:" => rest.parse().ok().map(Message::Found),
            _ => None,
        }
    }
}

/// A keystroke in GPUI notation (`ctrl-shift-tab`, `cmd-,`, `ctrl--`) with
/// its modifiers in one order (ctrl, alt, shift, cmd), as [`PAGE_SCRIPT`]
/// writes them. `None` for an empty key.
pub fn canonical(keystroke: &str) -> Option<String> {
    let lower = keystroke.trim().to_ascii_lowercase();
    // The key is what follows the last separator; a key of "-" leaves "--".
    let (mods, key) = match lower.strip_suffix("--") {
        Some(mods) => (mods, "-"),
        None => match lower.rsplit_once('-') {
            Some((mods, key)) => (mods, key),
            None => ("", lower.as_str()),
        },
    };
    if key.is_empty() {
        return None;
    }
    let has = |m: &[&str]| mods.split('-').any(|p| m.contains(&p));
    let mut out = String::new();
    for (names, word) in
        [(&["ctrl", "control"][..], "ctrl-"), (&["alt", "option"], "alt-"), (&["shift"], "shift-"), (&["cmd", "super", "win", "platform"], "cmd-")]
    {
        if has(names) {
            out.push_str(word);
        }
    }
    out.push_str(key);
    Some(out)
}

/// The script every page gets (main frame only). `window.__morphKeys` is
/// set from Rust after each load ([`keys_script`]).
pub const PAGE_SCRIPT: &str = r#"(() => {
  if (window.__morphPage || !window.ipc) return;
  window.__morphPage = true;
  const post = (m) => { try { window.ipc.postMessage(m); } catch (_) {} };
  window.morphRust = Object.freeze({ postMessage: (payload) => post("b:" + String(payload)) });
  window.__morphKeys = window.__morphKeys || [];
  const names = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down", PageUp: "pageup",
    PageDown: "pagedown", " ": "space", Escape: "escape", Enter: "enter", Tab: "tab", Backspace: "backspace",
    Delete: "delete", Home: "home", End: "end", Insert: "insert" };
  addEventListener("keydown", (e) => {
    if (!e.key || ["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
    let key = names[e.key] || (e.code && /^Digit\d$/.test(e.code) ? e.code.slice(5) : e.key.toLowerCase());
    const stroke = (e.ctrlKey ? "ctrl-" : "") + (e.altKey ? "alt-" : "") + (e.shiftKey ? "shift-" : "") + (e.metaKey ? "cmd-" : "") + key;
    if (window.__morphKeys.includes(stroke)) {
      e.preventDefault();
      e.stopImmediatePropagation();
      post("k:" + stroke);
    }
  }, true);
  addEventListener("focus", () => post("f:"), true);
  addEventListener("mousedown", () => post("f:"), true);
  const url = () => post("u:" + location.href);
  for (const name of ["pushState", "replaceState"]) {
    const original = history[name];
    history[name] = function (...args) { const r = original.apply(this, args); url(); return r; };
  }
  addEventListener("popstate", url);
  addEventListener("hashchange", url);
})();"#;

/// Hands the shortcut list to the page's script.
pub fn keys_script(keys: &[String]) -> String {
    let list: Vec<String> = keys.iter().map(|k| format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""))).collect();
    format!("window.__morphKeys = [{}];", list.join(","))
}

/// A JavaScript string literal for `text`.
pub fn js_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Counts (up to the cap) and selects the first match of `query`, then
/// reports the count.
pub fn find_script(query: &str) -> String {
    format!(
        r#"(() => {{
  const q = {q}; window.__morphFind = q;
  getSelection().removeAllRanges();
  if (!q) {{ window.ipc.postMessage("n:0"); return; }}
  const text = (document.body ? document.body.innerText : "").toLowerCase(), needle = q.toLowerCase();
  let n = 0;
  for (let i = text.indexOf(needle); i >= 0 && n < {cap}; i = text.indexOf(needle, i + needle.length)) n++;
  if (n) window.find(q, false, false, true);
  window.ipc.postMessage("n:" + n);
}})();"#,
        q = js_string(query),
        cap = crate::FIND_MAX_MATCHES
    )
}

/// 16 bytes that name a profile's data store on macOS, the same for the
/// same directory across runs (FNV-1a, twice).
pub fn store_id(dir: &std::path::Path) -> [u8; 16] {
    let bytes = dir.as_os_str().as_encoded_bytes();
    let fnv = |seed: u64| bytes.iter().fold(seed, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
    let (a, b) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x6c62_272e_07bb_0142));
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&a.to_le_bytes());
    out[8..].copy_from_slice(&b.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_messages_parse() {
        assert_eq!(Message::parse(r#"b:{"id":"1"}"#), Some(Message::Bridge(r#"{"id":"1"}"#.into())));
        assert_eq!(Message::parse("f:"), Some(Message::Focused));
        assert_eq!(Message::parse("k:ctrl-t"), Some(Message::Shortcut("ctrl-t".into())));
        assert_eq!(Message::parse("u:https://example.com/#a"), Some(Message::Url("https://example.com/#a".into())));
        assert_eq!(Message::parse("n:12"), Some(Message::Found(12)));
        for bad in ["", "x", "k:", "u:", "n:x", "z:1"] {
            assert_eq!(Message::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn keystrokes_get_one_modifier_order() {
        assert_eq!(canonical("ctrl-shift-tab").as_deref(), Some("ctrl-shift-tab"));
        assert_eq!(canonical("shift-ctrl-Tab").as_deref(), Some("ctrl-shift-tab"));
        assert_eq!(canonical("cmd-shift-g").as_deref(), Some("shift-cmd-g"));
        assert_eq!(canonical("ctrl-,").as_deref(), Some("ctrl-,"));
        assert_eq!(canonical("ctrl--").as_deref(), Some("ctrl--"));
        assert_eq!(canonical("f5").as_deref(), Some("f5"));
        assert_eq!(canonical("alt-left").as_deref(), Some("alt-left"));
        assert_eq!(canonical(""), None);
    }

    #[test]
    fn scripts_quote_what_they_carry() {
        assert_eq!(keys_script(&["ctrl-t".into(), "ctrl-\"".into()]), r#"window.__morphKeys = ["ctrl-t","ctrl-\""];"#);
        assert_eq!(js_string("a\"b\\c\nd\u{2028}"), r#""a\"b\\c\nd\u2028""#);
        assert!(find_script("x\"y").contains(r#"const q = "x\"y";"#));
    }

    #[test]
    fn profiles_get_stable_distinct_stores() {
        let a = store_id(std::path::Path::new("/home/a/.morph/profiles/default/webkit"));
        assert_eq!(a, store_id(std::path::Path::new("/home/a/.morph/profiles/default/webkit")));
        assert_ne!(a, store_id(std::path::Path::new("/home/a/.morph/profiles/work/webkit")));
    }
}
