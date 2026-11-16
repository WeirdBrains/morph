//! Favicons: a script every page gets finds the page's icon (`<link
//! rel=icon>`, else `/favicon.ico`), draws it at 32×32 and hands it over as a
//! PNG data URL, over the backend's own message channel (so a page can only
//! name its own icon). [`decode`] turns that into PNG bytes.

/// Largest icon accepted, in bytes of PNG.
pub const MAX_ICON_BYTES: usize = 64 * 1024;

/// The script (main frame only). It posts `i:<data url>` on wry's IPC
/// (macOS, Windows) or the data URL to the `morphIcon` message handler
/// (WebKitGTK).
pub const ICON_SCRIPT: &str = r#"(() => {
  if (window.top !== window || window.__morphIcon) return;
  window.__morphIcon = true;
  const h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.morphIcon;
  const post = h ? (m) => h.postMessage(m) : (window.ipc ? (m) => window.ipc.postMessage("i:" + m) : null);
  if (!post || !/^https?:$/.test(location.protocol)) return;
  let sent = "";
  const candidates = () => {
    const links = [...document.querySelectorAll("link[rel]")].filter((l) => /(^|\s)(icon|shortcut icon|apple-touch-icon)(\s|$)/i.test(l.rel) && l.href);
    const size = (l) => { const m = /(\d+)x\d+/.exec(l.getAttribute("sizes") || ""); return m ? +m[1] : (/svg/.test(l.type || l.href) ? 64 : 16); };
    links.sort((a, b) => Math.abs(size(a) - 32) - Math.abs(size(b) - 32));
    return [...links.map((l) => l.href), location.origin + "/favicon.ico"];
  };
  const load = (urls) => {
    const url = urls.shift();
    if (!url || url === sent) return;
    const img = new Image();
    if (new URL(url, location.href).origin !== location.origin) img.crossOrigin = "anonymous";
    img.onload = () => {
      try {
        const c = document.createElement("canvas");
        c.width = c.height = 32;
        c.getContext("2d").drawImage(img, 0, 0, 32, 32);
        sent = url;
        post(c.toDataURL("image/png"));
      } catch (_) { load(urls); }
    };
    img.onerror = () => load(urls);
    img.src = url;
  };
  const run = () => load(candidates());
  if (document.readyState === "complete") run(); else addEventListener("load", run, { once: true });
  addEventListener("DOMContentLoaded", () => {
    if (document.head) new MutationObserver(run).observe(document.head, { childList: true, subtree: true, attributes: true, attributeFilter: ["href", "rel"] });
  }, { once: true });
})();"#;

/// PNG bytes from the script's `data:image/png;base64,…`; `None` for
/// anything else, malformed or too large.
pub fn decode(data_url: &str) -> Option<Vec<u8>> {
    let b64 = data_url.strip_prefix("data:image/png;base64,")?;
    if b64.len() > MAX_ICON_BYTES * 4 / 3 + 4 {
        return None;
    }
    let bytes = base64(b64)?;
    (bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() <= MAX_ICON_BYTES).then_some(bytes)
}

/// Standard base64 (with or without padding).
fn base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let text = text.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for chunk in text.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        match chunk.len() {
            4 => out.extend_from_slice(&bytes),
            3 => out.extend_from_slice(&bytes[..2]),
            2 => out.push(bytes[0]),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_with_and_without_padding() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64("aGk=").unwrap(), b"hi");
        assert_eq!(base64("YQ==").unwrap(), b"a");
        assert!(base64("a!b=").is_none());
        assert!(base64("Y").is_none());
    }

    #[test]
    fn only_png_data_urls_become_icons() {
        // The 8-byte PNG signature plus a little.
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
        assert_eq!(&decode(png).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        assert!(decode("data:image/svg+xml;base64,PHN2Zz4=").is_none());
        assert!(decode("data:image/png;base64,aGVsbG8=").is_none(), "not a PNG");
        assert!(decode(&format!("data:image/png;base64,{}", "A".repeat(200_000))).is_none(), "too large");
    }

    #[test]
    fn the_script_stays_in_the_main_frame_and_web_pages() {
        assert!(ICON_SCRIPT.contains("window.top !== window"));
        assert!(ICON_SCRIPT.contains("https?:"));
        assert!(ICON_SCRIPT.contains("/favicon.ico"));
    }
}
