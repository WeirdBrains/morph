//! Which display backend the shell runs on (`MORPH_BACKEND=x11|wayland`),
//! and the environment GTK and WebKit start with.
//!
//! On a Wayland session the shell runs natively and pages are rendered off
//! screen and drawn by GPUI (`morph_webview::offscreen`). On X11, and under
//! XWayland when asked for or when GTK cannot use Wayland, web views are
//! X11 child windows of the shell's window. GPUI picks Wayland whenever
//! `WAYLAND_DISPLAY` is set, and GTK follows `GDK_BACKEND`, so the choice
//! is made in the environment before either starts.
//!
//! Nothing here is chosen by hardware vendor: the DMA-BUF fallback follows
//! a probe of the allocation WebKit makes (`crate::dmabuf`), VA-API decode
//! follows the GStreamer decoders that registered, and pages follow the
//! shell's own scale (GDK_SCALE and GDK_DPI_SCALE are the shell's business,
//! not WebKit's: with GDK_SCALE=2 under a scale-1 compositor pages came out
//! twice the shell's size).

use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    X11,
    Wayland,
}

/// The backend for `MORPH_BACKEND` and the session's displays, and whether
/// X11 is there to fall back on. `None` for `MORPH_BACKEND` is automatic.
pub fn choose(wanted: Option<&OsStr>, wayland: Option<&OsStr>, x11: Option<&OsStr>) -> Result<(Backend, bool), String> {
    let set = |v: Option<&OsStr>| v.is_some_and(|v| !v.is_empty());
    let (wayland, x11) = (set(wayland), set(x11));
    match wanted.map(|w| w.to_string_lossy().trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("auto") => match (wayland, x11) {
            (true, x11) => Ok((Backend::Wayland, x11)),
            (false, true) => Ok((Backend::X11, false)),
            (false, false) => Err("no display: neither WAYLAND_DISPLAY nor DISPLAY is set".into()),
        },
        Some("wayland") if wayland => Ok((Backend::Wayland, false)),
        Some("wayland") => Err("MORPH_BACKEND=wayland but WAYLAND_DISPLAY is not set".into()),
        Some("x11") if x11 => Ok((Backend::X11, false)),
        Some("x11") => Err("MORPH_BACKEND=x11 but DISPLAY is not set (no X server or XWayland)".into()),
        Some(other) => Err(format!("MORPH_BACKEND must be x11 or wayland, not {other:?}")),
    }
}

/// GStreamer's VA-API decoders (the `va` plugin registers only those the
/// VA driver can run), ranked first so WebKit's decodebin picks them.
pub const VA_DECODERS: &str = "vah264dec:MAX,vah265dec:MAX,vavp8dec:MAX,vavp9dec:MAX,vaav1dec:MAX,vampeg2dec:MAX";

/// The `GST_PLUGIN_FEATURE_RANK` to set, if any: the VA decoders first,
/// unless the user ranks plugins already or `MORPH_VAAPI=0`.
pub fn va_ranks(current: Option<&OsStr>, morph_vaapi: Option<&OsStr>) -> Option<&'static str> {
    (current.is_none() && morph_vaapi.is_none_or(|v| v != "0")).then_some(VA_DECODERS)
}

/// Prepare the environment, pick the backend and start GTK on it. Call
/// first thing in `main`, before any other thread exists (it changes the
/// environment).
pub fn start() -> Result<Backend, String> {
    // SAFETY (every `set_var`/`remove_var` below): no other thread exists
    // yet; the web processes WebKit starts inherit the result.
    unsafe {
        std::env::remove_var("GDK_SCALE");
        std::env::remove_var("GDK_DPI_SCALE");
    }
    if let Some(ranks) = va_ranks(std::env::var_os("GST_PLUGIN_FEATURE_RANK").as_deref(), std::env::var_os("MORPH_VAAPI").as_deref()) {
        unsafe { std::env::set_var("GST_PLUGIN_FEATURE_RANK", ranks) };
    }
    let chosen = crate::dmabuf::USER_CHOICES.iter().any(|v| std::env::var_os(v).is_some());
    if !chosen && std::env::var_os("MORPH_DMABUF_PROBE").is_none_or(|v| v != "0") {
        let probe = crate::dmabuf::probe_in_child();
        if let Some(var) = crate::dmabuf::fallback(probe) {
            eprintln!("morph: this GPU refused the DMA-BUF buffers WebKit renders into; setting {var}=1 (MORPH_DMABUF_PROBE=0 skips this check)");
            unsafe { std::env::set_var(var, "1") };
        }
    }
    let (backend, fallback) =
        choose(std::env::var_os("MORPH_BACKEND").as_deref(), std::env::var_os("WAYLAND_DISPLAY").as_deref(), std::env::var_os("DISPLAY").as_deref())?;
    if backend == Backend::Wayland {
        unsafe {
            std::env::set_var("GDK_BACKEND", "wayland");
            // Overlay scrollbars in an off-screen page keep animating,
            // redrawing the whole page about 12 times a second while idle.
            if std::env::var_os("GTK_OVERLAY_SCROLLING").is_none() {
                std::env::set_var("GTK_OVERLAY_SCROLLING", "0");
            }
        }
        match morph_webview::wry_linux::init() {
            Ok(()) => return Ok(Backend::Wayland),
            Err(e) if fallback => eprintln!("morph: {e}; using X11 (XWayland) instead"),
            Err(e) => return Err(e.to_string()),
        }
    }
    unsafe {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("GDK_BACKEND", "x11");
    }
    morph_webview::wry_linux::init().map_err(|e| e.to_string())?;
    Ok(Backend::X11)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(wanted: Option<&str>, wayland: Option<&str>, x11: Option<&str>) -> Result<(Backend, bool), String> {
        choose(wanted.map(OsStr::new), wayland.map(OsStr::new), x11.map(OsStr::new))
    }

    #[test]
    fn a_wayland_session_runs_natively_with_x11_to_fall_back_on() {
        assert_eq!(pick(None, Some("wayland-0"), Some(":0")), Ok((Backend::Wayland, true)));
        assert_eq!(pick(Some("auto"), Some("wayland-0"), None), Ok((Backend::Wayland, false)));
        assert_eq!(pick(None, Some(""), Some(":0")), Ok((Backend::X11, false)));
        assert!(pick(None, None, None).is_err());
    }

    #[test]
    fn morph_backend_overrides_and_never_falls_back() {
        assert_eq!(pick(Some("x11"), Some("wayland-0"), Some(":0")), Ok((Backend::X11, false)));
        assert_eq!(pick(Some("Wayland"), Some("wayland-0"), Some(":0")), Ok((Backend::Wayland, false)));
        assert!(pick(Some("x11"), Some("wayland-0"), None).unwrap_err().contains("DISPLAY"));
        assert!(pick(Some("wayland"), None, Some(":0")).unwrap_err().contains("WAYLAND_DISPLAY"));
        assert!(pick(Some("gtk"), Some("wayland-0"), Some(":0")).unwrap_err().contains("x11 or wayland"));
    }

    #[test]
    fn va_decoders_go_first_unless_the_user_ranks_plugins() {
        assert_eq!(va_ranks(None, None), Some(VA_DECODERS));
        assert_eq!(va_ranks(Some(OsStr::new("nvh264dec:MAX")), None), None);
        assert_eq!(va_ranks(None, Some(OsStr::new("0"))), None);
        assert_eq!(va_ranks(None, Some(OsStr::new("1"))), Some(VA_DECODERS));
    }
}
