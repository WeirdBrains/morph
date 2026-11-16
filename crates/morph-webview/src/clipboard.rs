//! Copy, cut and paste for pages the shell draws ([`crate::Backend::draws_pages`]).
//!
//! WebKitGTK's own clipboard code crashed (SIGSEGV inside a GTK selection
//! signal) when a page in an off-screen window on native Wayland read the
//! clipboard, as github.com's paste handler does; and an off-screen window
//! never has the keyboard focus Wayland wants from a clipboard owner, so
//! its copies went nowhere. So WebKit never sees the clipboard keys: the
//! shell reads its own (GPUI's) clipboard and hands the page a paste
//! ([`crate::Input::Paste`]), and a copy runs in the page and comes back
//! for the shell to put on the clipboard ([`crate::WebView::take_copied`]).

use crate::Key;
use crate::keys::{CONTROL, MOD1, MOD4, SHIFT};
use crate::native_page::js_string;

/// A clipboard key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardKey {
    Copy,
    Cut,
    Paste,
}

/// What a paste hands the page: text, an image, or both.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Clipboard {
    pub text: Option<String>,
    /// An image's MIME type and bytes.
    pub image: Option<(String, Vec<u8>)>,
}

impl std::fmt::Debug for Clipboard {
    // The clipboard can hold anything, passwords included: never print it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Clipboard(text: {}, image: {})", self.text.as_ref().map_or(0, |t| t.len()), self.image.as_ref().map_or(0, |i| i.1.len()))
    }
}

const KEY_C: u32 = 0x63;
const KEY_V: u32 = 0x76;
const KEY_X: u32 = 0x78;
const INSERT: [u32; 2] = [0xff63, 0xff9e];
const DELETE: [u32; 2] = [0xffff, 0xff9f];
const XF86_COPY: u32 = 0x1008_ff57;
const XF86_CUT: u32 = 0x1008_ff58;
const XF86_PASTE: u32 = 0x1008_ff6d;
/// The XKB keycodes of C, V and X, for layouts whose letters are not Latin.
const CODE_C: u32 = 54;
const CODE_V: u32 = 55;
const CODE_X: u32 = 53;

/// The clipboard key `key` is, if any: Ctrl+C/X/V (Shift too for V),
/// Ctrl+Insert, Shift+Insert, Shift+Delete and the media keys.
pub fn clipboard_key(key: &Key) -> Option<ClipboardKey> {
    let mods = key.state & (SHIFT | CONTROL | MOD1 | MOD4);
    match key.keyval {
        XF86_COPY => return Some(ClipboardKey::Copy),
        XF86_CUT => return Some(ClipboardKey::Cut),
        XF86_PASTE => return Some(ClipboardKey::Paste),
        k if INSERT.contains(&k) && mods == CONTROL => return Some(ClipboardKey::Copy),
        k if INSERT.contains(&k) && mods == SHIFT => return Some(ClipboardKey::Paste),
        k if DELETE.contains(&k) && mods == SHIFT => return Some(ClipboardKey::Cut),
        _ => {}
    }
    if mods != CONTROL && mods != CONTROL | SHIFT {
        return None;
    }
    let lower = if (0x41..=0x5a).contains(&key.keyval) { key.keyval + 0x20 } else { key.keyval };
    let latin = lower < 0x100;
    let letter = match (latin, lower, key.keycode) {
        (true, KEY_C, _) | (false, _, CODE_C) => ClipboardKey::Copy,
        (true, KEY_X, _) | (false, _, CODE_X) => ClipboardKey::Cut,
        (true, KEY_V, _) | (false, _, CODE_V) => ClipboardKey::Paste,
        _ => return None,
    };
    // Ctrl+Shift+V pastes (as plain text, elsewhere); Ctrl+Shift+C and X are
    // not clipboard keys.
    (mods == CONTROL || letter == ClipboardKey::Paste).then_some(letter)
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The element with the keyboard, through same-origin frames, and its
/// document.
const FOCUSED: &str = r#"let doc = document, el = doc.activeElement;
  while (el && (el.tagName === "IFRAME" || el.tagName === "FRAME")) {
    let inner = null;
    try { inner = el.contentDocument; } catch (e) {}
    if (!inner) break;
    doc = inner; el = doc.activeElement;
  }
  const target = el || doc.body || doc.documentElement;"#;

/// A paste of `clip` where the page has the keyboard: a `paste` event
/// carrying it, then (unless the page handled it) the text typed in.
pub fn paste_script(clip: &Clipboard) -> String {
    let text = js_string(clip.text.as_deref().unwrap_or(""));
    let image = match &clip.image {
        Some((mime, bytes)) => format!("[{}, {}]", js_string(mime), js_string(&base64(bytes))),
        None => "null".into(),
    };
    format!(
        r#"(() => {{
  const text = {text}, image = {image};
  {FOCUSED}
  const data = new DataTransfer();
  if (text) data.setData("text/plain", text);
  if (image) try {{
    const raw = atob(image[1]), bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
    const ext = (image[0].split("/")[1] || "png").replace(/[^a-z0-9]/gi, "");
    data.items.add(new File([bytes], "image." + ext, {{ type: image[0] }}));
  }} catch (e) {{}}
  let event;
  try {{ event = new ClipboardEvent("paste", {{ clipboardData: data, bubbles: true, cancelable: true, composed: true }}); }}
  catch (e) {{ event = new Event("paste", {{ bubbles: true, cancelable: true, composed: true }}); }}
  if (target.dispatchEvent(event) && text) doc.execCommand("insertText", false, text);
}})()"#
    )
}

/// A copy (or cut) where the page has the keyboard: a `copy` event the
/// page may fill, else the selection (never a password field's). The
/// script's value is the text to put on the clipboard.
pub fn copy_script(cut: bool) -> String {
    let kind = if cut { "cut" } else { "copy" };
    format!(
        r#"(() => {{
  {FOCUSED}
  const data = new DataTransfer();
  let event = null;
  try {{ event = new ClipboardEvent("{kind}", {{ clipboardData: data, bubbles: true, cancelable: true, composed: true }}); }} catch (e) {{}}
  if (event && !target.dispatchEvent(event)) return data.getData("text/plain");
  let text = "";
  const field = el && (el.tagName === "TEXTAREA" || (el.tagName === "INPUT" && /^(text|search|url|tel|email|number)?$/i.test(el.getAttribute("type") || "")));
  if (el && el.tagName === "INPUT" && /^password$/i.test(el.type)) return "";
  if (field && el.selectionStart != null) text = el.value.slice(el.selectionStart, el.selectionEnd);
  else text = String(doc.getSelection ? doc.getSelection() : "");
  if ({cut} && text) doc.execCommand("delete");
  return text;
}})()"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(keyval: u32, state: u32) -> Key {
        Key { pressed: true, keycode: 0, keyval, state, group: 0, is_modifier: false }
    }

    #[test]
    fn the_clipboard_keys_are_known() {
        assert_eq!(clipboard_key(&key(KEY_V, CONTROL)), Some(ClipboardKey::Paste));
        assert_eq!(clipboard_key(&key(0x56, CONTROL | SHIFT)), Some(ClipboardKey::Paste));
        assert_eq!(clipboard_key(&key(KEY_V, CONTROL | crate::keys::MOD2)), Some(ClipboardKey::Paste), "Num Lock does not matter");
        assert_eq!(clipboard_key(&key(0xff63, SHIFT)), Some(ClipboardKey::Paste));
        assert_eq!(clipboard_key(&key(KEY_C, CONTROL)), Some(ClipboardKey::Copy));
        assert_eq!(clipboard_key(&key(0xff63, CONTROL)), Some(ClipboardKey::Copy));
        assert_eq!(clipboard_key(&key(KEY_X, CONTROL)), Some(ClipboardKey::Cut));
        assert_eq!(clipboard_key(&key(0xffff, SHIFT)), Some(ClipboardKey::Cut));
        assert_eq!(clipboard_key(&key(XF86_PASTE, 0)), Some(ClipboardKey::Paste));
        // A Cyrillic layout's Ctrl+М is the V key.
        assert_eq!(clipboard_key(&Key { keycode: CODE_V, ..key(0x6cd, CONTROL) }), Some(ClipboardKey::Paste));
        for (keyval, state) in [(KEY_V, 0), (KEY_C, CONTROL | SHIFT), (KEY_V, CONTROL | MOD1), (KEY_C, MOD4), (0x61, CONTROL), (0xff63, 0)] {
            assert_eq!(clipboard_key(&key(keyval, state)), None, "{keyval:#x} {state}");
        }
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe]), "//4=");
    }

    #[test]
    fn scripts_carry_the_clipboard_quoted() {
        let s = paste_script(&Clipboard { text: Some("a\"b\n</script>".into()), image: Some(("image/png".into(), vec![1, 2, 3])) });
        assert!(s.contains(r#"const text = "a\"b\n</script>""#), "{s}");
        assert!(s.contains(r#"["image/png", "AQID"]"#), "{s}");
        assert!(paste_script(&Clipboard::default()).contains("image = null"));
        assert!(copy_script(true).contains(r#"ClipboardEvent("cut""#));
        assert!(copy_script(false).contains("if (false && text)"));
        assert_eq!(format!("{:?}", Clipboard { text: Some("hunter2".into()), image: None }), "Clipboard(text: 7, image: 0)");
    }
}
