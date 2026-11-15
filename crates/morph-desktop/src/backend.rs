//! Which display backend the shell runs on (`MORPH_BACKEND=x11|wayland`).
//!
//! On a Wayland session the shell runs natively and pages are rendered off
//! screen and drawn by GPUI (`morph_webview::offscreen`). On X11, and under
//! XWayland when asked for or when GTK cannot use Wayland, web views are
//! X11 child windows of the shell's window. GPUI picks Wayland whenever
//! `WAYLAND_DISPLAY` is set, and GTK follows `GDK_BACKEND`, so the choice
//! is made in the environment before either starts.

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

/// Pick the backend and start GTK on it. Call first thing in `main`,
/// before any other thread exists (it changes the environment).
pub fn start() -> Result<Backend, String> {
    let (backend, fallback) =
        choose(std::env::var_os("MORPH_BACKEND").as_deref(), std::env::var_os("WAYLAND_DISPLAY").as_deref(), std::env::var_os("DISPLAY").as_deref())?;
    if backend == Backend::Wayland {
        // SAFETY: no other thread exists yet (see above).
        unsafe { std::env::set_var("GDK_BACKEND", "wayland") };
        match morph_webview::wry_linux::init() {
            Ok(()) => return Ok(Backend::Wayland),
            Err(e) if fallback => eprintln!("morph: {e}; using X11 (XWayland) instead"),
            Err(e) => return Err(e.to_string()),
        }
    }
    // SAFETY: as above.
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
}
