//! What Morph's About page (morph://about) says about this build and this
//! machine, and the diagnostics text its Copy button puts on the clipboard.
//! Nothing here reads identity, library or profile data, so diagnostics
//! never carry recovery words, shards or history.

use std::sync::OnceLock;

pub const WEBSITE: &str = "https://github.com/WeirdBrains/morph";
pub const REPOSITORY: &str = "https://github.com/WeirdBrains/morph";

/// One line of the About page: a label and its value.
pub type Line = (&'static str, String);

/// Build facts, from `build.rs`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
pub fn commit() -> &'static str {
    env!("MORPH_GIT_COMMIT")
}
pub fn build_date() -> &'static str {
    env!("MORPH_BUILD_DATE")
}

/// The platform and how pages are shown there.
pub fn platform(os: &str, wayland: Option<bool>) -> String {
    match (os, wayland) {
        ("linux", Some(true)) => "Linux, native Wayland (pages drawn by the shell)".into(),
        ("linux", _) => "Linux, X11 (pages in child windows)".into(),
        ("macos", _) => "macOS (WKWebView child view)".into(),
        ("windows", _) => "Windows (WebView2 child window)".into(),
        (other, _) => other.to_string(),
    }
}

/// The web engine and its version.
pub fn engine() -> String {
    morph_webview::engine_version()
}

/// `GStreamer 1.24.12` from `gst-inspect-1.0 --version`.
pub fn gst_version(output: &str) -> Option<String> {
    output.lines().find_map(|l| l.trim().strip_prefix("GStreamer ").map(|v| format!("GStreamer {}", v.trim())))
}

/// VA-API decode in a line: the decoders GStreamer registered (its `va`
/// plugin registers only those the driver runs), the legacy `vaapi`
/// plugin, a render node, and whether Morph ranks them first.
pub fn va_status(decoders: &[&str], legacy: bool, render_node: bool, ranked: bool) -> String {
    if !render_node {
        return "off (no GPU render node)".into();
    }
    if decoders.is_empty() {
        return if legacy { "gstreamer-vaapi only (Morph ranks the newer va decoders)".into() } else { "off (no GStreamer VA decoders; install gst-plugins-bad)".into() };
    }
    let names: Vec<&str> = decoders.iter().map(|d| d.trim_start_matches("va").trim_end_matches("dec")).collect();
    format!("on: {}{}", names.join(", "), if ranked { " (ranked first)" } else { " (not ranked: GST_PLUGIN_FEATURE_RANK or MORPH_VAAPI=0)" })
}

/// GStreamer and VA-API, found once on a background thread (they run
/// `gst-inspect-1.0`).
static MEDIA: OnceLock<Vec<Line>> = OnceLock::new();

/// Start looking for GStreamer and VA-API (Linux), off the UI thread.
pub fn probe_media(va_ranked: bool) {
    #[cfg(target_os = "linux")]
    std::thread::Builder::new()
        .name("morph-about-media".into())
        .spawn(move || {
            let _ = MEDIA.set(linux_media(va_ranked));
        })
        .ok();
    #[cfg(not(target_os = "linux"))]
    let _ = va_ranked;
}

#[cfg(target_os = "linux")]
fn linux_media(va_ranked: bool) -> Vec<Line> {
    use std::process::{Command, Stdio};
    let inspect = |args: &[&str]| Command::new("gst-inspect-1.0").args(args).stderr(Stdio::null()).output().ok();
    let version = inspect(&["--version"]).and_then(|o| gst_version(&String::from_utf8_lossy(&o.stdout)));
    let Some(version) = version else {
        return vec![("GStreamer", "not found (gst-inspect-1.0 is missing; media may not play)".into()), ("VA-API", "unknown".into())];
    };
    let exists = |name: &str| inspect(&["--exists", name]).is_some_and(|o| o.status.success());
    let decoders: Vec<&str> =
        ["vah264dec", "vah265dec", "vavp8dec", "vavp9dec", "vaav1dec", "vampeg2dec"].into_iter().filter(|d| exists(d)).collect();
    let legacy = exists("vaapidecodebin");
    let render_node = std::fs::read_dir("/dev/dri").is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("renderD")));
    vec![("GStreamer", version), ("VA-API", va_status(&decoders, legacy, render_node, va_ranked))]
}

/// Everything the About page lists, in order.
pub fn lines() -> Vec<Line> {
    #[cfg(target_os = "linux")]
    let startup = crate::backend::startup();
    #[cfg(target_os = "linux")]
    let wayland = startup.map(|s| s.backend == crate::backend::Backend::Wayland);
    #[cfg(not(target_os = "linux"))]
    let wayland = None;
    let mut out: Vec<Line> = vec![
        ("Version", version().to_string()),
        ("Commit", commit().to_string()),
        ("Built", build_date().to_string()),
        ("Platform", platform(std::env::consts::OS, wayland)),
        ("Architecture", std::env::consts::ARCH.to_string()),
        ("Web engine", engine()),
    ];
    #[cfg(target_os = "linux")]
    {
        if let Some(s) = startup {
            out.push(("Render path", s.render_path.clone()));
            if let Some(note) = &s.note {
                out.push(("Render check", note.clone()));
            }
        }
        match MEDIA.get() {
            Some(media) => out.extend(media.iter().cloned()),
            None => out.push(("GStreamer", "checking…".into())),
        }
    }
    out
}

/// The diagnostics text for the clipboard.
pub fn diagnostics(lines: &[Line]) -> String {
    let mut text = String::from("Morph diagnostics\n");
    for (label, value) in lines {
        text.push_str(&format!("{label}: {value}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_facts_are_filled_in() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
        assert!(!commit().is_empty());
        let date = build_date();
        assert_eq!(date.len(), 10, "{date}");
        assert_eq!(&date[4..5], "-");
    }

    #[test]
    fn platforms_name_how_pages_show() {
        assert!(platform("linux", Some(true)).contains("native Wayland"));
        assert!(platform("linux", Some(false)).contains("X11"));
        assert!(platform("linux", None).contains("X11"));
        assert!(platform("macos", None).contains("macOS"));
        assert!(platform("windows", None).contains("WebView2"));
    }

    #[test]
    fn gstreamer_version_comes_from_gst_inspect() {
        let out = "gst-inspect-1.0 version 1.24.12\nGStreamer 1.24.12\nhttps://archlinux.org/\n";
        assert_eq!(gst_version(out), Some("GStreamer 1.24.12".into()));
        assert_eq!(gst_version("nothing"), None);
    }

    #[test]
    fn va_status_says_what_decodes() {
        assert_eq!(va_status(&["vah264dec", "vavp9dec"], false, true, true), "on: h264, vp9 (ranked first)");
        assert!(va_status(&["vah264dec"], false, true, false).contains("not ranked"));
        assert!(va_status(&[], false, true, true).starts_with("off (no GStreamer VA decoders"));
        assert!(va_status(&[], true, true, true).starts_with("gstreamer-vaapi only"));
        assert_eq!(va_status(&["vah264dec"], false, false, true), "off (no GPU render node)");
    }

    #[test]
    fn diagnostics_list_every_line_and_nothing_private() {
        let lines = lines();
        let text = diagnostics(&lines);
        assert!(text.starts_with("Morph diagnostics\n"));
        assert!(text.contains(&format!("Version: {}", version())));
        assert!(text.contains("Web engine: "));
        for word in ["recovery", "shard", "password", "bookmark", "history"] {
            assert!(!text.to_lowercase().contains(word), "{word} in {text}");
        }
    }
}
